//! Writing an anonymous problem report: the logs and a setup summary, run through the Scrubber, into the
//! anonymous-logs folder. Nothing is sent anywhere (report.make, report.summary).

use crate::paths::Paths;
use crate::problem_report::{Identity, Scrubber, core_lines};
use crate::router::Ssh;
use crate::settings::{self, Settings};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// Lines kept from the end of each log.
pub const LINES: usize = 400;
/// Reports kept in the folder.
const KEEP: usize = 5;

static PROBLEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(dead|fail|failed|error|did not|could not|cannot|not reachable|timed? ?out|blocked|missing|refused)\b",
    )
    .unwrap()
});

// The last n lines of a file, without their line ends; none when it cannot be read (report.tail)
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let text = crate::data_files::read_text(path).unwrap_or_default();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.trim_end_matches('\n').to_string()).collect()
}

// A titled part of the report (report.section)
fn section(title: &str, lines: Vec<String>) -> Vec<String> {
    let body = if lines.is_empty() { vec!["(nothing)".to_string()] } else { lines };
    [String::new(), title.to_string(), "-".repeat(title.chars().count())].into_iter().chain(body).collect()
}

// Who and what to take out: the configs' secrets, every node address known, and this user's names
fn scrubber(paths: &Paths) -> Scrubber {
    let mut secrets = Vec::new();
    let mut nodes: HashSet<String> = HashSet::new();
    for cfg in [&paths.pc_cfg, &paths.router_cfg] {
        if let Ok(c) = crate::data_files::read_json(cfg) {
            secrets.push(crate::router::secret(&c));
        }
        if let Ok(list) = crate::singbox_config::nodes_of(cfg) {
            nodes.extend(list.into_iter().map(|(_, n)| n.server));
        }
    }
    nodes.extend(crate::vault::load(paths).keys().cloned());
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    let user = env("USERNAME").or_else(|| env("USER"));
    let computer = env("COMPUTERNAME").or_else(|| {
        crate::process::run(&["hostname"], None, Duration::from_secs(10), None)
            .ok()
            .map(|o| crate::python_compat::strip(&o.stdout).to_string())
            .filter(|h| !h.is_empty())
    });
    let domain = env("USERDOMAIN");
    let id = Identity {
        secrets,
        nodes,
        home: &home,
        user: user.as_deref(),
        computer: computer.as_deref(),
        domain: domain.as_deref(),
        sep: std::path::MAIN_SEPARATOR,
    };
    Scrubber::new(&id)
}

// The system's name and version (platform.platform)
fn platform() -> String {
    let out = |cmd: &[&str]| {
        crate::process::run(cmd, None, Duration::from_secs(10), None)
            .map(|o| crate::python_compat::strip(&o.stdout).to_string())
            .unwrap_or_default()
    };
    if cfg!(windows) {
        let cmd = crate::process::system_tool("cmd").to_string_lossy().into_owned();
        out(&[&cmd, "/c", "ver"])
    } else {
        format!("macOS {} ({})", out(&["/usr/bin/sw_vers", "-productVersion"]), std::env::consts::ARCH)
    }
}

// The PC tunnel now: running, its control API, the node it uses, a node set waiting for a restart, PIA's state. Asks
// the local tunnel and the PIA app only, and only when this data folder has its own rh-core
fn pc_state(paths: &Paths) -> String {
    use crate::pc_tunnel::{core_mode, core_running};
    if settings::router_only(paths) {
        return "none (router only)".into();
    }
    if !core_mode(paths) {
        return "rh-core missing".into();
    }
    if !core_running(paths) {
        return "not running".into();
    }
    let api = crate::clash_api::ControlApi::pc(paths).ok();
    let px: Option<Value> =
        api.and_then(|a| a.get("/proxies", Duration::from_secs(3)).ok()).and_then(|t| serde_json::from_str(&t).ok());
    let mut out = match px.as_ref().and_then(|v| v["proxies"]["auto"]["now"].as_str()) {
        Some(now) => format!("running, using {now}"),
        None => "running, control API not answering".into(),
    };
    if crate::node_manager::pending_deploy(paths) == Some(true) {
        out += ", a new node set waits for a restart";
    }
    if std::path::Path::new(crate::pia_split_tunnel::PIACTL).exists() {
        let st = crate::watcher::pia_state();
        out += &format!("; PIA {}", if st.is_empty() { "not answering".into() } else { st.to_lowercase() });
    }
    out
}

// The last 48 hours from history.json: how often each thing happened, then the latest events, oldest first
fn activity(paths: &Paths) -> Vec<String> {
    let h = crate::history::load(&paths.history);
    let events: Vec<&Value> = h["events"].as_array().map(|e| e.iter().collect()).unwrap_or_default();
    if events.is_empty() {
        return Vec::new();
    }
    let names = [
        ("drop", "PIA dropped"),
        ("back", "PIA back"),
        ("restart", "tunnel restarts"),
        ("replace", "nodes replaced"),
        ("heal", "followed the router"),
        ("rescue", "rescues"),
        ("error", "errors"),
    ];
    let count = |k: &str| events.iter().filter(|e| e[1] == k).count();
    let counts: Vec<String> =
        names.iter().map(|(k, n)| (count(k), n)).filter(|(c, _)| *c > 0).map(|(c, n)| format!("{n} {c}")).collect();
    let mut out = vec![format!("{} events: {}", events.len(), counts.join(", ")), String::new()];
    for e in &events[events.len().saturating_sub(60)..] {
        let at = crate::local_time::at(e[0].as_f64().unwrap_or(0.0) as i64).format("%m-%d %H:%M:%S");
        out.push(format!("{at}  {:<8} {}", e[1].as_str().unwrap_or_default(), e[2].as_str().unwrap_or_default()));
    }
    out
}

// Each node in use: where it is and how its tunnel pings went over 48 hours (median, worst, lost), from history.json
fn node_health(paths: &Paths) -> Vec<String> {
    let Ok(nodes) = crate::singbox_config::nodes_of(&paths.pc_cfg) else { return Vec::new() };
    let h = crate::history::load(&paths.history);
    let vault = crate::vault::load(paths);
    nodes
        .iter()
        .map(|(tag, n)| {
            let pings: Vec<Option<f64>> =
                h["pings"][&n.server].as_array().map(|l| l.iter().map(|p| p[1].as_f64()).collect()).unwrap_or_default();
            let mut ok: Vec<f64> = pings.iter().flatten().copied().collect();
            ok.sort_by(f64::total_cmp);
            let lost = pings.len() - ok.len();
            let stats = if pings.is_empty() {
                "no pings recorded".to_string()
            } else if ok.is_empty() {
                format!("{} checks, all lost", pings.len())
            } else {
                format!(
                    "{} checks, median {:.0} ms, worst {:.0} ms, lost {lost}",
                    pings.len(),
                    ok[ok.len() / 2],
                    ok[ok.len() - 1]
                )
            };
            let place = vault.get(&n.server).and_then(|v| v["where"].as_str()).unwrap_or("?");
            format!("{tag:<7} {}  {place}  {stats}", n.server)
        })
        .collect()
}

// Plain facts about the setup as (label, value); nothing personal, and the scrubber runs over it too (report.summary)
pub fn summary(paths: &Paths, ssh: &mut impl Ssh) -> Vec<(String, String)> {
    let s = Settings::read(paths).unwrap_or_default();
    let t = settings::this_pc(paths);
    let tray = crate::data_files::jload(&paths.tray_last);
    let tray: Vec<String> =
        tray.get("last").and_then(Value::as_array).into_iter().flatten().map(crate::python_json::py_str).collect();
    let nodes = crate::singbox_config::nodes_of(&paths.pc_cfg).map_or(0, |n| n.len());
    let check =
        if tray.len() == 3 { format!("; last check {} at {} ({})", tray[1], tray[2], tray[0]) } else { String::new() };
    let vpn = crate::setup_steps::vpn_check(paths);
    let on = |b: bool| if b { "on" } else { "off" };
    let yes = |k: &str| t.get(k).is_some_and(settings::py_truthy);
    let mut router = on(s.router != 0).to_string();
    if settings::router_only(paths) {
        router += ", router only (no PC tunnel)";
    }
    if s.router != 0 {
        router += &if crate::router::all_devices(&s.devices) {
            ", every device on the main network".to_string()
        } else {
            format!(", {} device(s) chosen", crate::router::devices(&s.devices).len())
        };
    }
    let mut rows = vec![
        ("Reality Hop".to_string(), format!("{} (installed)", crate::VERSION)),
        ("Windows".to_string(), platform()),
        (
            "sing-box".to_string(),
            format!(
                "{} (channel: {})",
                crate::pc_tunnel::core_version(paths).unwrap_or_else(|| "missing".into()),
                s.singbox
            ),
        ),
        ("VPN app".to_string(), vpn.split_once(": ").map_or(vpn.clone(), |(_, r)| r.to_string())),
        ("Nodes".to_string(), format!("{nodes} in use{check}")),
        (
            "Settings".to_string(),
            format!(
                "check every {} min, slow above {} ms, switch margin {} ms, speed floor {} Mbps, {} candidates per scan, auto-update {}",
                s.check_min,
                s.slow_ms,
                s.tolerance_ms,
                s.min_mbps,
                s.top,
                on(s.auto_update != 0)
            ),
        ),
        (
            "State".to_string(),
            format!(
                "{}, setup {}, boot start {}, background at logon {}",
                if yes("stopped") { "stopped" } else { "running" },
                if yes("setup_done") { "done" } else { "not finished" },
                crate::startup_tasks::task_state().unwrap_or("off"),
                on(crate::startup_tasks::startup_on(paths))
            ),
        ),
        ("Router features".to_string(), router),
    ];
    rows.push(("PC tunnel".to_string(), pc_state(paths)));
    let tmp = std::fs::read_dir(&paths.data)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    if tmp > 0 {
        rows.push(("Leftover files".to_string(), format!("{tmp} unfinished write(s) (.tmp) in the data folder")));
    }
    if s.router != 0 {
        rows.push((
            "Router".to_string(),
            match crate::router::router_info(ssh) {
                Ok(i) => format!(
                    "{}, firmware {}, sing-box {}",
                    i.model,
                    i.firmware,
                    i.singbox.unwrap_or_else(|| "not installed".into())
                ),
                Err(e) => format!("not reachable ({e})"),
            },
        ));
    }
    rows
}

// Write a new anonymous report; its path and text. The last 5 reports are kept (report.make)
pub fn make(paths: &Paths, ssh: &mut impl Ssh) -> Result<(PathBuf, String), String> {
    let mut sc = scrubber(paths);
    let mut logs = std::collections::HashMap::new();
    for n in ["window", "watch", "core", "debug"] {
        let lines: Vec<String> =
            tail(&paths.logs.join(format!("{n}.log")), LINES).iter().map(|l| sc.scrub(l)).collect();
        logs.insert(n, lines);
    }
    let rows: Vec<(String, String)> = summary(paths, ssh).into_iter().map(|(k, v)| (k, sc.scrub(&v))).collect();
    let w = rows.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let mut problems: Vec<String> = ["window", "watch"]
        .iter()
        .flat_map(|n| logs[n].iter().filter(|l| PROBLEM.is_match(l)).map(move |l| format!("{n:<6}  {l}")))
        .collect();
    let when = |l: &String| l.chars().skip(8).take(19).collect::<String>();
    problems.sort_by_key(when);
    let mut out = vec![
        "Reality Hop problem report".to_string(),
        format!(
            "Made {}. Personal data is redacted (see Redaction key at the end).",
            crate::local_time::now().format("%Y-%m-%d %H:%M")
        ),
    ];
    out.extend(section("Setup", rows.iter().map(|(k, v)| format!("{k:<w$}  {v}")).collect()));
    let nodes: Vec<String> = node_health(paths).iter().map(|l| sc.scrub(l)).collect();
    out.extend(section("Nodes in use (tunnel pings over 48 hours)", nodes));
    let acts: Vec<String> = activity(paths).iter().map(|l| sc.scrub(l)).collect();
    out.extend(section("Activity (last 48 hours, newest last)", acts));
    let last40 = problems[problems.len().saturating_sub(40)..].to_vec();
    out.extend(section("Problems found in the window and watcher logs (newest last)", last40));
    out.extend(section(&format!("Watcher log (watch.log, last {LINES} lines)"), logs["watch"].clone()));
    out.extend(section(&format!("Window log (window.log, last {LINES} lines)"), logs["window"].clone()));
    out.extend(section(
        &format!("Tunnel engine log (core.log, last {LINES} lines, repeats grouped)"),
        core_lines(&logs["core"]),
    ));
    if !logs["debug"].is_empty() {
        out.extend(section(&format!("Debug log (debug.log, last {LINES} lines)"), logs["debug"].clone()));
    }
    let key = [
        "node-N    a public node from the node list",
        "lan-N     an address on your own network",
        "ip-N      another internet address",
        "host-N    a website the tunnel connected to",
        "ipv6-N    an IPv6 address",
        "mac-N     a MAC address",
        "<user>    your Windows user name",
        "<pc>      your computer name",
        "<key>, <secret>, <uuid>, <email>    keys, IDs and e-mail addresses",
        "The same token always means the same thing within this report. Nothing was sent anywhere.",
    ];
    out.extend(section("Redaction key", key.iter().map(|s| s.to_string()).collect()));
    let text = out.join("\n") + "\n";
    std::fs::create_dir_all(&paths.reports).map_err(|e| e.to_string())?;
    let path = paths.reports.join(format!("report-{}.txt", crate::local_time::now().format("%Y%m%d-%H%M%S")));
    crate::data_files::write_text(&path, &text, crate::data_files::Newlines::Platform).map_err(|e| e.to_string())?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(&paths.reports)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("report-") && n.to_string_lossy().ends_with(".txt"))
        })
        .collect();
    old.sort();
    for p in &old[..old.len().saturating_sub(KEEP)] {
        let _ = std::fs::remove_file(p);
    }
    Ok((path, text))
}
