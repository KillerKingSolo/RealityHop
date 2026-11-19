//! What starts at boot and at logon: the boot task that runs the PC tunnel (a login item on macOS), the logon task
//! that runs the watcher, and Stop / Start for when the person is away.

use crate::activity_log::say;
use crate::macos;
use crate::macos_system::{agent_enable, agent_enabled, agent_exists, agent_remove, agent_write};
use crate::paths::Paths;
use crate::pc_tunnel::{
    BOOT_TASK, boot_task_exists, boot_task_here, core_restart, core_running, core_stop, no_boot_task, note_boot_task,
};
use crate::process::{run, system_tool};
use crate::settings::{self, router_only};
use crate::windows_system::{elevated, user};
use crate::windows_tasks::{self, LOGON_TASK};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

const MAC: bool = !cfg!(windows);

// A path as text for a script
fn text(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

// Merge one field into this-pc.json
fn mark(paths: &Paths, k: &str, v: serde_json::Value) {
    let mut m = serde_json::Map::new();
    m.insert(k.into(), v);
    let _ = settings::set_this_pc(paths, m);
}

// schtasks /query /xml of the boot task: (exit code, XML)
fn boot_task_xml() -> (Option<i32>, String) {
    let tool = system_tool("schtasks");
    let args = [tool.as_os_str(), "/query".as_ref(), "/tn".as_ref(), BOOT_TASK.as_ref(), "/xml".as_ref()];
    match run(&args, None, Duration::from_secs(15), None) {
        Ok(o) => (o.code, o.stdout),
        Err(_) => (None, String::new()),
    }
}

// Start rh-core at boot, before logon and before the VPN app, as this user ("do not store password" mode), so the
// watcher can still restart it; one UAC prompt to add or remove the task. macOS: a login item starts it at login, no
// password needed. True when it took (engine.boot_task)
pub fn boot_task(paths: &Paths, on: bool) -> bool {
    if MAC {
        if on {
            let args = [text(&paths.core), "run".into(), "-c".into(), text(&paths.pc_cfg)];
            let _ = agent_write(macos::TUNNEL, &args, paths.core.parent().unwrap_or(&paths.data));
        } else {
            agent_remove(macos::TUNNEL);
        }
        let ok = boot_task_exists() == on;
        say(match (ok, on) {
            (true, true) => "rh-core now starts at login, before the VPN app",
            (true, false) => "login start of rh-core removed",
            _ => "not changed",
        });
        return ok;
    }
    let ps = if on {
        let (domain, name) = user();
        windows_tasks::boot_task_ps(&text(&paths.core), &text(&paths.pc_cfg), &domain, &name)
    } else {
        format!("Unregister-ScheduledTask -TaskName '{BOOT_TASK}' -Confirm:$false")
    };
    say(&format!(
        "{} the boot-time start of rh-core; answer the UAC prompt...",
        if on { "adding" } else { "removing" }
    ));
    elevated(&ps);
    let now = boot_task_exists();
    note_boot_task(paths, now);
    let ok = now == on;
    say(match (ok, on) {
        (true, true) => "rh-core now starts at boot, before the VPN app",
        (true, false) => "boot-time start removed",
        _ => "not changed (UAC declined or failed)",
    });
    ok
}

// "Ready", "Disabled" (switched off by Stop), or None when there is no boot task (engine.task_state)
pub fn task_state() -> Option<&'static str> {
    if MAC {
        return boot_task_exists().then(|| if agent_enabled(macos::TUNNEL) { "Ready" } else { "Disabled" });
    }
    let (code, xml) = boot_task_xml();
    windows_tasks::task_state(code, &xml)
}

// task_state, without schtasks when this PC has recorded that it has no boot task; records what it finds
// (engine.task_state_here)
pub fn task_state_here(paths: &Paths) -> Option<&'static str> {
    if no_boot_task(paths) {
        return None;
    }
    let s = task_state();
    if !MAC {
        note_boot_task(paths, s.is_some());
    }
    s
}

// The boot task's priority (7, below normal, when the task sets none); None when there is no task or on macOS
// (engine.boot_task_priority)
pub fn boot_task_priority() -> Option<i64> {
    if MAC {
        return None;
    }
    let (code, xml) = boot_task_xml();
    windows_tasks::task_priority(code, &xml)
}

// Boot tasks made up to 1.4.0 run rh-core below normal priority. One UAC prompt sets it to normal, from the next
// tunnel restart. True when it is set (engine.boot_task_priority_fix)
pub fn boot_task_priority_fix(paths: &Paths) -> bool {
    say("giving the PC tunnel normal priority at boot; answer the UAC prompt...");
    let (domain, name) = user();
    elevated(&windows_tasks::boot_task_ps(&text(&paths.core), &text(&paths.pc_cfg), &domain, &name));
    let ok = boot_task_priority() == Some(5);
    say(if ok {
        "PC tunnel: normal priority from the next restart"
    } else {
        "PC tunnel priority not changed (UAC declined)"
    });
    ok
}

// The Startup folder shortcut: the fallback start at logon when Task Scheduler refuses the logon task
pub fn startup_lnk() -> PathBuf {
    let appdata = std::env::var_os("APPDATA").unwrap_or_default();
    PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup").join(format!("{}.lnk", crate::NAME))
}

// Up to 1.0.4 the shortcuts were called RealityHop.lnk; now they carry the name people see (engine.lnk_migrate)
pub fn lnk_migrate(path: &std::path::Path) {
    let old = path.with_file_name(format!("{}.lnk", crate::APP));
    if old.exists() {
        let _ = if path.exists() { std::fs::remove_file(&old) } else { std::fs::rename(&old, path) };
    }
}

// Register the logon task for cmd as this user, not elevated (no UAC prompt), normal priority: the Task Scheduler
// default is below normal, and what the watcher starts would inherit it. True when it is in place (engine.logon_task)
pub fn logon_task(paths: &Paths, cmd: &[String]) -> bool {
    let cmd: Vec<&str> = cmd.iter().map(String::as_str).collect();
    let (domain, name) = user();
    let ps = windows_tasks::logon_task_ps(&cmd, &text(&paths.data), &domain, &name);
    let tool = system_tool("powershell");
    let args = [tool.as_os_str(), "-NoProfile".as_ref(), "-Command".as_ref(), ps.as_ref()];
    run(&args, None, Duration::from_secs(60), None).is_ok_and(|o| o.code == Some(0))
}

// Start the watcher at logon, or not; cmd runs it (its --logon flag is added here). watcher(on) starts or stops the
// running watcher now. Task Scheduler starts it a few seconds after logon; a Startup folder shortcut waits for
// Explorer (about 90 s on a busy PC), so it is only the fallback. macOS: a login item (engine.startup)
pub fn startup(paths: &Paths, on: bool, cmd: &[String], watcher: impl FnOnce(bool)) {
    let mut logon = cmd.to_vec();
    logon.push("--logon".into());
    if MAC {
        if on {
            crate::macos_system::lockdown(&paths.data);
            let _ = agent_write(macos::WATCHER, &logon, &paths.data);
            mark(paths, "logon_task", json!(true));
            watcher(true);
            say("background watcher: on, starts at every login");
        } else {
            agent_remove(macos::WATCHER);
            mark(paths, "logon_task", json!(false));
            watcher(false);
            say("background watcher: off");
        }
        return;
    }
    let lnk = startup_lnk();
    lnk_migrate(&lnk);
    if on {
        let _ = std::fs::create_dir_all(&paths.data);
        let task = logon_task(paths, &logon);
        mark(paths, "logon_task", json!(task));
        if task {
            let _ = std::fs::remove_file(&lnk); // one start at logon, not two
        } else {
            let cmd: Vec<&str> = cmd.iter().map(String::as_str).collect();
            let ps = windows_tasks::startup_lnk_ps(&text(&lnk), &cmd, &text(&paths.data));
            let tool = system_tool("powershell");
            let _ = run(
                &[tool.as_os_str(), "-NoProfile".as_ref(), "-Command".as_ref(), ps.as_ref()],
                None,
                Duration::from_secs(30),
                None,
            );
        }
        watcher(true);
        let why = if task { "" } else { " (Startup folder: Task Scheduler refused the logon task)" };
        say(&format!("background watcher: on, starts at every logon{why}"));
    } else {
        let _ = std::fs::remove_file(&lnk);
        if settings::this_pc(paths).get("logon_task").is_some_and(settings::py_truthy) {
            let tool = system_tool("schtasks");
            let args = [tool.as_os_str(), "/delete".as_ref(), "/tn".as_ref(), LOGON_TASK.as_ref(), "/f".as_ref()];
            let _ = run(&args, None, Duration::from_secs(20), None);
        }
        mark(paths, "logon_task", json!(false));
        watcher(false);
        say("background watcher: off");
    }
}

// True when the watcher starts at logon (engine.startup_on)
pub fn startup_on(paths: &Paths) -> bool {
    if MAC {
        return agent_exists(macos::WATCHER);
    }
    settings::this_pc(paths).get("logon_task").is_some_and(settings::py_truthy) || startup_lnk().exists()
}

// Stop everything on this PC for when the person is away: the PC tunnel, the watcher (and its tray icon), the start at
// logon and the start at boot. The router and the files are not touched. cmd and watcher as for startup. The lines
// for the window (engine.manager_stop)
pub fn manager_stop(paths: &Paths, cmd: &[String], watcher: impl FnOnce(bool)) -> Vec<String> {
    let _ = settings::set_stopped(paths, true);
    let mut out: Vec<String> = Vec::new();
    if boot_task_here(paths) {
        if MAC {
            agent_enable(macos::TUNNEL, false);
        } else {
            elevated(&format!("schtasks /end /tn '{BOOT_TASK}'; schtasks /change /tn '{BOOT_TASK}' /disable"));
        }
        out.push(if task_state() == Some("Disabled") {
            "boot start: switched off".into()
        } else {
            "boot start: NOT switched off (UAC declined); rh-core will still start at boot".into()
        });
    }
    startup(paths, false, cmd, watcher);
    out.push("background watcher: stopped, no longer starts at logon".into());
    core_stop(paths);
    out.push(if core_running(paths) { "PC tunnel: could not be stopped" } else { "PC tunnel: stopped" }.into());
    if !router_only(paths) {
        out.push("VPN app: turn its proxy off now (PIA: Settings > Multi-Hop, untick \"Multi-hop and Obfuscation\"), or it cannot connect".into());
    }
    for l in &out {
        say(l);
    }
    out
}

// Undo Stop: the boot start on again, the PC tunnel running, the watcher on at logon. The lines for the window
// (engine.manager_start)
pub fn manager_start(paths: &Paths, cmd: &[String], watcher: impl FnOnce(bool)) -> Vec<String> {
    let _ = settings::set_stopped(paths, false);
    let mut out: Vec<String> = Vec::new();
    if boot_task_here(paths) && task_state() == Some("Disabled") {
        if MAC {
            agent_enable(macos::TUNNEL, true);
        } else {
            elevated(&format!("schtasks /change /tn '{BOOT_TASK}' /enable"));
        }
        out.push(
            if task_state() != Some("Disabled") { "boot start: on" } else { "boot start: still OFF (UAC declined)" }
                .into(),
        );
    }
    if !router_only(paths) {
        out.push(
            if core_restart(paths) { "PC tunnel: running" } else { "PC tunnel: did not come up, see the log" }.into(),
        );
    }
    startup(paths, true, cmd, watcher);
    out.push("background watcher: running, starts at every logon".into());
    if !router_only(paths) {
        out.push("VPN app: turn its proxy back on (PIA: Settings > Multi-Hop, tick \"Multi-hop and Obfuscation\", SOCKS5 127.0.0.1:2080)".into());
    }
    for l in &out {
        say(l);
    }
    out
}
