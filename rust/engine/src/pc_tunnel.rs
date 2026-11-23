//! The PC tunnel: the official sing-box, saved as rh-core, running pc-config.json. Stop, start, restart, the daily
//! update with its checksum check, and the config check (engine.core_*).

use crate::activity_log::{dbg, say};
use crate::clash_api::ControlApi;
use crate::paths::Paths;
use crate::processes_and_ports::{kill_name, pids, port_open, port_owner, port_taken, procs};
use crate::settings::{self, Settings};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The scheduled task that starts rh-core at boot (Windows).
pub const BOOT_TASK: &str = "RealityHop tunnel";
/// The PC tunnel's name in the test builds before 1.0; their boot task may still start it.
pub const LEGACY_CORE: &str = "sb-core.exe";
/// The official sing-box releases.
const RELEASES: &str = "https://api.github.com/repos/SagerNet/sing-box/releases?per_page=40";

// Seconds since 1970, as Python's time.time()
fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// rh-core's program file name
fn core_name(paths: &Paths) -> String {
    paths.core.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

// PowerShell that closes this install's rh-core by its full path. Run as admin: a copy started by the boot task or by
// an elevated install cannot be closed without admin rights
pub fn kill_ps(core: &Path) -> String {
    let name = core.file_stem().unwrap_or_default().to_string_lossy();
    format!(
        "Get-Process -Name {} -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq {} }} | Stop-Process -Force",
        crate::powershell::psq(&name),
        crate::powershell::psq(&core.to_string_lossy())
    )
}

// The folder rh-core runs in
fn core_dir(paths: &Paths) -> &Path {
    paths.core.parent().unwrap_or(&paths.data)
}

// A run of a program by its path, as text, with the time limit in seconds
fn run(cmd: &[&std::ffi::OsStr], secs: u64, cwd: Option<&Path>) -> Result<crate::process::Output, String> {
    crate::process::run(cmd, None, Duration::from_secs(secs), cwd).map_err(|e| e.to_string())
}

// For debug.log: the Reality Hop, sing-box and piactl processes (engine.rh_procs)
fn rh_procs() -> String {
    let mine = |n: &str| ["rh-", "realityhop", "piactl", "sb-core"].iter().any(|k| n.to_lowercase().starts_with(k));
    procs().into_iter().filter(|(_, n)| mine(n)).map(|(p, n)| format!("{p} {n}")).collect::<Vec<_>>().join(", ")
}

// An event in history.json for the window's graph
fn event(paths: &Paths, kind: &str, text: &str) {
    crate::history::record(&paths.history, &[], Some((kind, text)), now());
}

// The sing-box release file for this PC (macOS: the chip this build runs as)
fn asset(tag: &str) -> String {
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    crate::version::core_asset(tag, (!cfg!(windows)).then_some(arch))
}

// True when rh-core exists: the setup downloads it (engine.core_mode)
pub fn core_mode(paths: &Paths) -> bool {
    paths.core.exists()
}

// Is rh-core alive? Asks the process list, not the port, so a busy PC right after boot is not mistaken for a dead
// tunnel (engine.core_running)
pub fn core_running(paths: &Paths) -> bool {
    !pids(&core_name(paths)).is_empty()
}

// Whether the boot task (macOS: the login item) that starts rh-core exists (engine.boot_task_exists)
pub fn boot_task_exists() -> bool {
    if !cfg!(windows) {
        return crate::macos_system::agent_exists(crate::macos::TUNNEL);
    }
    let schtasks = crate::process::system_tool("schtasks");
    let cmd = [schtasks.as_os_str(), "/query".as_ref(), "/tn".as_ref(), BOOT_TASK.as_ref()];
    run(&cmd, 15, None).is_ok_and(|r| r.code == Some(0))
}

// True once this PC has recorded that it has no boot task: a start then does not run schtasks at all (Windows)
pub fn no_boot_task(paths: &Paths) -> bool {
    cfg!(windows) && settings::this_pc(paths).get("boot_task") == Some(&Value::Bool(false))
}

// Record whether the boot task exists, in this-pc.json (Windows)
pub fn note_boot_task(paths: &Paths, on: bool) {
    if cfg!(windows) && settings::this_pc(paths).get("boot_task") != Some(&Value::Bool(on)) {
        let mut m = Map::new();
        m.insert("boot_task".into(), Value::Bool(on));
        let _ = settings::set_this_pc(paths, m);
    }
}

// boot_task_exists, without schtasks when this PC has recorded that it has none; records what it finds
// (engine.boot_task_here)
pub fn boot_task_here(paths: &Paths) -> bool {
    if no_boot_task(paths) {
        return false;
    }
    let on = boot_task_exists();
    note_boot_task(paths, on);
    on
}

// Stop every rh-core: the boot task's copy (only the task can stop it) and our own; wait until 2080 is free, 10 s at
// most (engine.core_stop)
pub fn core_stop(paths: &Paths) -> bool {
    RESTARTED.store(now() as u64, std::sync::atomic::Ordering::SeqCst);
    let t = Instant::now();
    dbg(|| format!("stop: before {}", rh_procs()));
    if cfg!(windows) && boot_task_here(paths) {
        let schtasks = crate::process::system_tool("schtasks");
        let _ = run(&[schtasks.as_os_str(), "/end".as_ref(), "/tn".as_ref(), BOOT_TASK.as_ref()], 20, None);
    }
    kill_name(&core_name(paths));
    for _ in 0..100 {
        if !core_running(paths) && !port_taken(2080) {
            dbg(|| format!("stop: done in {:.1} s", t.elapsed().as_secs_f64()));
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    dbg(|| {
        format!(
            "stop: GAVE UP after {:.1} s: port 2080 by {:?} {}",
            t.elapsed().as_secs_f64(),
            port_owner(2080),
            rh_procs()
        )
    });
    false
}

// When rh-core cannot be closed without admin rights: close it as admin (one UAC prompt). True when it is gone
fn core_stop_admin(paths: &Paths) -> bool {
    if !cfg!(windows) || !core_running(paths) {
        return false;
    }
    say("rh-core runs with admin rights and will not close; answer the UAC prompt to close it...");
    if crate::windows_system::elevated(&kill_ps(&paths.core)) == crate::windows_system::DECLINED {
        say("UAC declined: rh-core keeps running on the old nodes");
        return false;
    }
    for _ in 0..50 {
        if !core_running(paths) && !port_taken(2080) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

// Start rh-core directly, hidden and on its own: it keeps running after Reality Hop exits (engine.core_direct)
pub fn core_direct(paths: &Paths) -> std::io::Result<()> {
    std::fs::create_dir_all(&paths.logs)?;
    let mut c = std::process::Command::new(&paths.core);
    c.args(["run".as_ref(), "-c".as_ref(), paths.pc_cfg.as_os_str()]).current_dir(core_dir(paths));
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000 | 0x0000_0200); // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0); // its own group: a signal to Reality Hop does not reach it
    }
    let child = c.spawn()?;
    dbg(|| format!("start: rh-core pid {}", child.id()));
    Ok(())
}

// Logon fast path: when rh-core exists and nothing is on 2080, start it now; no checks, no waiting. True when it was
// started (engine.core_fast_start)
pub fn core_fast_start(paths: &Paths) -> bool {
    if !paths.core.exists()
        || settings::stopped(paths)
        || settings::router_only(paths)
        || core_running(paths)
        || port_open(2080)
    {
        return false;
    }
    if cfg!(windows) && boot_task_here(paths) {
        let tool = crate::process::system_tool("schtasks");
        let args = [tool.as_os_str(), "/run".as_ref(), "/tn".as_ref(), BOOT_TASK.as_ref()];
        if run(&args, 20, None).is_ok_and(|o| o.code == Some(0)) {
            return true;
        }
    }
    core_direct(paths).is_ok()
}

// Start rh-core hidden: through the boot task when it exists (so it lives in the same place it does after a reboot),
// otherwise directly. macOS: always directly; the login item only starts it at login. "task" or "direct"
// (engine.core_start)
pub fn core_start(paths: &Paths) -> &'static str {
    if cfg!(windows) && boot_task_here(paths) {
        let schtasks = crate::process::system_tool("schtasks");
        let r = run(&[schtasks.as_os_str(), "/run".as_ref(), "/tn".as_ref(), BOOT_TASK.as_ref()], 20, None);
        if r.is_ok_and(|r| r.code == Some(0)) {
            return "task";
        }
    }
    if let Err(e) = core_direct(paths) {
        dbg(|| format!("start: direct start failed: {e}"));
    }
    "direct"
}

// True as soon as the PC tunnel answers with the node set of pc-config.json; asked 4 times a second (engine.core_up)
pub fn core_up(paths: &Paths, wait: Duration) -> bool {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(250));
        let Ok(nodes) = crate::singbox_config::nodes_of(&paths.pc_cfg) else { continue };
        let mark = crate::singbox_config::set_mark(nodes.iter().map(|(_, n)| n.server.as_str()));
        let up = ControlApi::pc(paths)
            .ok()
            .and_then(|api| api.get("/proxies", Duration::from_secs(3)).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .is_some_and(|v| v["proxies"].get(&mark).is_some());
        if up {
            return true;
        }
    }
    false
}

// The other program on port 2080 that keeps rh-core from starting, or None. The old sb-core is stopped if it can be
// (engine.port_blocker)
pub fn port_blocker(paths: &Paths) -> Option<String> {
    let owner = || if port_taken(2080) { port_owner(2080) } else { None };
    let mut o = owner();
    if o.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(LEGACY_CORE)) {
        kill_name(LEGACY_CORE);
        std::thread::sleep(Duration::from_secs(1));
        o = owner();
    }
    o.filter(|n| !n.eq_ignore_ascii_case(&core_name(paths)))
}

// What to do about the program on port 2080 (engine.blocker_hint)
pub fn blocker_hint(owner: &str) -> String {
    let what = if owner.eq_ignore_ascii_case(LEGACY_CORE) {
        "It is the tunnel of an old Reality Hop test build, started at boot: press Run setup again, which removes it \
         (one UAC prompt)."
    } else {
        "Close that program, then press Restart PC tunnel."
    };
    format!("{owner} uses port 2080, so the PC tunnel cannot start. {what}")
}

// The macOS PIA list: the nodes in use plus extra
fn pia(paths: &Paths, extra: &[String]) {
    if !cfg!(windows) {
        crate::pia_split_tunnel::pia_bypass(paths, extra, true);
    }
}

// When this process last restarted or stopped the PC tunnel (seconds since 1970)
static RESTARTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// Whether this process restarted or stopped the PC tunnel within the last secs: a PIA drop then is its own doing
pub fn restarted_within(secs: f64) -> bool {
    now() - (RESTARTED.load(std::sync::atomic::Ordering::SeqCst) as f64) < secs
}

// Restart the PC tunnel on pc-config.json, no UAC. If the task-started copy does not come up, a direct start is the
// fallback, so the tunnel is never left down (engine.core_restart)
pub fn core_restart(paths: &Paths) -> bool {
    if settings::router_only(paths) {
        say("router-only mode: no tunnel on this PC");
        return true;
    }
    RESTARTED.store(now() as u64, std::sync::atomic::Ordering::SeqCst);
    let t0 = Instant::now();
    dbg(|| format!("restart requested; stopped: {}, running: {}", settings::stopped(paths), core_running(paths)));
    // make before break: the new nodes join PIA's list next to the old ones, which carry the VPN until the restart
    let old: Vec<String> = settings::this_pc(paths)
        .get("pia_ips")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    pia(paths, &old);
    if let Some(owner) = port_blocker(paths) {
        say(&format!("PC tunnel: {}", blocker_hint(&owner)));
        event(paths, "error", &format!("{owner} holds port 2080"));
        return false;
    }
    if !core_stop(paths) && !core_stop_admin(paths) {
        say("an old rh-core would not stop; trying anyway");
    }
    let how = core_start(paths);
    if core_up(paths, Duration::from_secs(15)) {
        dbg(|| format!("restart: up in {:.1} s ({how}) {}", t0.elapsed().as_secs_f64(), rh_procs()));
        pia(paths, &[]); // the new nodes carry the VPN now: the old addresses come off PIA's list
        say(&format!("PC tunnel restarted ({how})"));
        return true;
    }
    dbg(|| format!("restart: NOT up after {:.1} s ({how}) {}", t0.elapsed().as_secs_f64(), rh_procs()));
    if how == "task" {
        say("the boot task did not bring rh-core up; starting it directly instead");
        core_stop(paths);
        let _ = core_direct(paths);
        if core_up(paths, Duration::from_secs(15)) {
            pia(paths, &[]);
            say("PC tunnel restarted (direct)");
            return true;
        }
    }
    say(&format!("PC tunnel did not come back after the restart{}", config_problem(paths, &paths.core)));
    event(paths, "error", "PC tunnel did not come back after a restart");
    false
}

// ": config check says ..." when this sing-box rejects pc-config.json, else "; see logs/core.log"
fn config_problem(paths: &Paths, core: &Path) -> String {
    let cmd = [core.as_os_str(), "check".as_ref(), "-c".as_ref(), paths.pc_cfg.as_os_str()];
    match run(&cmd, 30, Some(core_dir(paths))) {
        Ok(r) if r.code == Some(0) => "; see logs/core.log".into(),
        Ok(r) => {
            let why = crate::python_compat::strip(if r.stderr.is_empty() { &r.stdout } else { &r.stderr });
            format!(": config check says {}", why.chars().take(200).collect::<String>())
        }
        Err(e) => format!(": config check says {}", e.chars().take(200).collect::<String>()),
    }
}

// Keep logs/core.log to its last 200 lines once it passes 256 KB. sing-box appends, so it is trimmed in place
// (engine.core_log_trim)
pub fn core_log_trim(paths: &Paths) {
    use std::io::Write;
    if std::fs::metadata(&paths.core_log).map_or(true, |m| m.len() <= 256_000) {
        return;
    }
    let Ok(text) = crate::data_files::read_text(&paths.core_log) else { return };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let keep = lines[lines.len().saturating_sub(200)..].concat();
    let keep = if cfg!(windows) { keep.replace('\n', "\r\n") } else { keep };
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&paths.core_log) {
        let _ = f.write_all(keep.as_bytes()).and_then(|_| f.set_len(keep.len() as u64));
    }
}

// Ask the PC tunnel to re-test its group now, so the lowest-ping node takes over without a restart (engine.regroup)
pub fn regroup(paths: &Paths) {
    if let Ok(api) = ControlApi::pc(paths) {
        let _ = api
            .get(&format!("/group/auto/delay?url={}&timeout=5000", crate::router::TEST_URL), Duration::from_secs(20));
    }
}

// The sing-box version rh-core is, from `rh-core version` (engine.core_version)
pub fn core_version(paths: &Paths) -> Option<String> {
    let out = run(&[paths.core.as_os_str(), "version".as_ref()], 20, None).ok()?.stdout;
    out.split(crate::python_compat::is_space).filter(|w| !w.is_empty()).nth(2).map(str::to_string)
}

/// The certificate store setting: sing-box's own copy of Mozilla's root list, so the Windows store is not touched.
pub const CERT_STORE: &str = "mozilla";

static STORE: std::sync::Mutex<Option<((SystemTime, u64), bool)>> = std::sync::Mutex::new(None);

// True when rh-core is sing-box 1.12.0 or newer, which knows the certificate store setting. Kept per rh-core file
// (engine.store_ok)
pub fn store_ok(paths: &Paths) -> bool {
    let Ok(m) = std::fs::metadata(&paths.core) else { return false };
    let k = (m.modified().unwrap_or(UNIX_EPOCH), m.len());
    let mut cache = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((key, ok)) = *cache
        && key == k
    {
        return ok;
    }
    let ok = core_version(paths).is_some_and(|v| crate::version::vkey(&v) >= vec![1, 12, 0]);
    *cache = Some((k, ok));
    ok
}

// The official sing-box releases
fn releases(paths: &Paths) -> Result<Vec<Value>, String> {
    let b = crate::downloads::gh_get(paths, RELEASES, 30, &[]).map_err(|e| e.to_string())?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}

// The releases with a build named by asset (tag -> file name), drafts left out
fn with_build(rels: &[Value], asset: &impl Fn(&str) -> String) -> Vec<Value> {
    let has = |r: &Value| {
        let name = asset(r["tag_name"].as_str().unwrap_or_default());
        r["assets"].as_array().into_iter().flatten().any(|a| a["name"] == name.as_str())
    };
    rels.iter().filter(|r| !r["draft"].as_bool().unwrap_or(false) && has(r)).cloned().collect()
}

// The release with the highest version; the first of equal ones
fn newest(rels: impl IntoIterator<Item = Value>) -> Option<Value> {
    rels.into_iter().fold(None, |best: Option<Value>, r| {
        let k = |r: &Value| crate::version::vkey(r["tag_name"].as_str().unwrap_or_default());
        match best {
            Some(b) if k(&b) >= k(&r) => Some(b),
            _ => Some(r),
        }
    })
}

// The newest release with a build: stable = the newest full release, testing = the newest of all
pub fn pick_release(rels: &[Value], testing: bool, asset: impl Fn(&str) -> String) -> Option<Value> {
    newest(with_build(rels, &asset).into_iter().filter(|r| testing || !r["prerelease"].as_bool().unwrap_or(false)))
}

// The newest version with a build on each channel: (stable, testing)
pub fn channel_versions(rels: &[Value], asset: impl Fn(&str) -> String) -> (Option<String>, Option<String>) {
    let ver = |r: Option<Value>| r.and_then(|r| r["tag_name"].as_str().map(|t| t.trim_start_matches('v').to_string()));
    (ver(pick_release(rels, false, &asset)), ver(pick_release(rels, true, &asset)))
}

// The newest sing-box release on the chosen channel: stable = the newest full release, testing = the newest of all,
// alpha and beta included (engine.core_release)
pub fn core_release(paths: &Paths) -> Result<Value, String> {
    let testing = Settings::read(paths).map_err(|e| e.to_string())?.singbox == "testing";
    pick_release(&releases(paths)?, testing, asset).ok_or_else(|| {
        format!("no sing-box release for {} found on GitHub", if cfg!(windows) { "Windows" } else { "macOS" })
    })
}

// The newest sing-box for this PC on each channel: (stable, testing), one GitHub call (engine.core_versions)
pub fn core_versions(paths: &Paths) -> Result<(Option<String>, Option<String>), String> {
    Ok(channel_versions(&releases(paths)?, asset))
}

// Keep sing-box current on the chosen channel. SHA-256 checked against GitHub's digest, then the new build must
// accept the PC config before it replaces the old one. The new version, or None when it is current (engine.core_update)
pub fn core_update(paths: &Paths, force: bool) -> Result<Option<String>, String> {
    let rel = core_release(paths)?;
    let channel = Settings::read(paths).map_err(|e| e.to_string())?.singbox;
    let ver = rel["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').to_string();
    let mut mark = Map::new();
    mark.insert("core_checked".into(), Value::from(now()));
    mark.insert("core_channel".into(), Value::from(channel.clone()));
    settings::set_this_pc(paths, mark).map_err(|e| e.to_string())?;
    if Some(&ver) == core_version(paths).as_ref() && !force {
        say(&format!("sing-box {ver} is current ({channel})"));
        return Ok(None);
    }
    let name = asset(&ver);
    let a = rel["assets"].as_array().into_iter().flatten().find(|a| a["name"] == name.as_str()).ok_or("no asset")?;
    let d = crate::data_files::TempDir::new("rh-core-update").map_err(|e| e.to_string())?;
    let size = a["size"].as_u64().unwrap_or(0) / 1_000_000;
    say(&format!(
        "downloading sing-box {ver} ({size} MB) from the official releases (github.com/SagerNet/sing-box)..."
    ));
    let url = a["browser_download_url"].as_str().unwrap_or_default();
    let b = crate::downloads::gh_get(paths, url, 180, &[]).map_err(|e| e.to_string())?;
    let digest: String = Sha256::digest(&b).iter().map(|x| format!("{x:02x}")).collect();
    if a["digest"] != format!("sha256:{digest}").as_str() {
        return Err("download does not match the checksum GitHub publishes; not installed".into());
    }
    let new = unpack(d.path(), &name, &b, &ver)?;
    let has_nodes = crate::singbox_config::nodes_of(&paths.pc_cfg).is_ok_and(|n| !n.is_empty());
    if has_nodes {
        let cmd = [new.as_os_str(), "check".as_ref(), "-c".as_ref(), paths.pc_cfg.as_os_str()];
        let r = run(&cmd, 30, Some(core_dir(paths)))?;
        if r.code != Some(0) {
            let why: String = crate::python_compat::strip(&r.stderr).chars().take(200).collect();
            return Err(format!("sing-box {ver} rejects the PC config, keeping the current build: {why}"));
        }
    }
    let was = core_running(paths);
    core_stop(paths);
    let copied = std::fs::create_dir_all(&paths.pc_dir).and_then(|_| std::fs::copy(&new, &paths.core));
    if copied.is_ok() {
        say(&format!("sing-box {ver} ({channel}) installed as {}", core_name(paths)));
    }
    if was {
        core_restart(paths); // back up even if the copy failed
    }
    copied.map_err(|e| e.to_string())?;
    event(paths, "restart", &format!("rh-core updated to {ver}"));
    Ok(Some(ver))
}

// Save the download in d and take out the one sing-box program it holds, nothing else; its path. The system's tar
// reads both the Windows zip and the macOS tar.gz.
pub fn unpack(d: &Path, name: &str, b: &[u8], ver: &str) -> Result<PathBuf, String> {
    let archive = d.join(name);
    std::fs::write(&archive, b).map_err(|e| e.to_string())?;
    let member = if cfg!(windows) {
        format!("sing-box-{ver}-windows-amd64/sing-box.exe")
    } else {
        format!("{}/sing-box", asset(ver).trim_end_matches(".tar.gz")) // one named member: from the version, not the name
    };
    let tar = crate::process::system_tool("tar");
    let cmd = [tar.as_os_str(), "-xf".as_ref(), archive.as_os_str(), "-C".as_ref(), d.as_os_str(), member.as_ref()];
    let r = run(&cmd, 120, None)?;
    let got = d.join(&member);
    if r.code != Some(0) || !got.is_file() {
        return Err(format!("could not unpack {member}: {}", crate::python_compat::strip(&r.stderr)));
    }
    let new = d.join(format!("rh-core{}", crate::paths::EXE));
    std::fs::rename(&got, &new).map_err(|e| e.to_string())?;
    if !cfg!(windows) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
        }
        crate::macos_system::core_sign(&new, say); // so PIA's split tunnel rule for RealityHop.app covers it
    }
    Ok(new)
}

// True when the daily sing-box check is due, or the channel was changed since the last one (engine.core_update_due)
pub fn core_update_due(paths: &Paths) -> bool {
    let t = settings::this_pc(paths);
    let channel = Settings::read(paths).map(|s| s.singbox).unwrap_or_default();
    let checked = t.get("core_checked").and_then(Value::as_f64).unwrap_or(0.0);
    core_mode(paths)
        && (t.get("core_channel").and_then(Value::as_str) != Some(channel.as_str()) || now() - checked > 86400.0)
}

// Start the PC tunnel only if it is really not running: no rh-core process and port 2080 closed on three tries 5 s
// apart. Only local checks, no traffic. Never while Reality Hop is stopped (engine.core_ensure)
pub fn core_ensure(paths: &Paths) -> bool {
    if settings::stopped(paths) || settings::router_only(paths) || core_running(paths) {
        return true;
    }
    for _ in 0..3 {
        if port_open(2080) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    say("PC tunnel not running; starting it");
    core_restart(paths)
}
