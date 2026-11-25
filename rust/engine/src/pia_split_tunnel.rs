//! PIA's Split Tunnel list: Reality Hop keeps itself on it, so nobody has to set it by hand (engine.pia_bypass).
//! macOS: the app as an app rule (Bypass VPN), and the tunnel's node addresses as IP rules. PIA's Mac kill switch
//! blocks every program whatever its app rules say; IP rules are its one exception, so without them a kill switch
//! cuts the connection the VPN itself runs through. Split Tunnel is switched on when it is off.
//! Windows: rh-core.exe and rh-test.exe as app rules (Bypass VPN), no IP rules: PIA's Windows kill switch lets a Bypass
//! VPN app through. Split Tunnel is left as the person set it.
//! Only what Reality Hop added is ever changed; the person's own rules stay.

use crate::activity_log::{dbg, say};
use crate::paths::Paths;
use crate::settings::py_truthy;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// PIA's command line tool.
pub const PIACTL: &str = if cfg!(windows) {
    r"C:\Program Files\Private Internet Access\piactl.exe"
} else {
    "/Applications/Private Internet Access.app/Contents/MacOS/piactl"
};
/// PIA's settings, readable by every user.
pub const PIA_SETTINGS: &str = if cfg!(windows) {
    r"C:\Program Files\Private Internet Access\data\settings.json"
} else {
    "/Library/Preferences/com.privateinternetaccess.vpn/settings.json"
};

/// What pia_bypass changes in PIA (empty: nothing), and what Reality Hop then has on the list itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Bypass {
    pub change: Map<String, Value>,
    /// The node addresses Reality Hop added as IP rules (this-pc.json pia_ips).
    pub ips: Vec<String>,
    /// The app rules Reality Hop added (this-pc.json pia_apps).
    pub apps: Vec<String>,
}

// Python's os.path.normpath of a path, with \ as well as / taken as separators on Windows
fn normpath(p: &str, windows: bool) -> String {
    let sep = if windows { '\\' } else { '/' };
    let p = if windows { p.replace('/', "\\") } else { p.to_string() };
    let (drive, rest) = match p.as_bytes() {
        [d, b':', ..] if windows && d.is_ascii_alphabetic() => p.split_at(2),
        _ => ("", p.as_str()),
    };
    let lead = rest.chars().take_while(|&c| c == sep).count();
    let root = match lead {
        0 => String::new(),
        2 if !windows => "//".into(),
        _ => sep.to_string(),
    };
    let mut parts: Vec<&str> = Vec::new();
    for c in rest.split(sep).filter(|c| !c.is_empty() && *c != ".") {
        if c == ".." && parts.last().is_some_and(|l| *l != "..") {
            parts.pop();
        } else if !(c == ".." && !root.is_empty()) {
            parts.push(c);
        }
    }
    let out = format!("{drive}{root}{}", parts.join(&sep.to_string()));
    if out.is_empty() { ".".into() } else { out }
}

// A path as PIA's list is compared: trailing separators off, normalized, and on Windows any letter case
fn key(p: Option<&Value>, windows: bool) -> String {
    let Some(p) = p.filter(|p| py_truthy(p)) else { return String::new() };
    let s = crate::python_json::py_str(p);
    let n = normpath(s.trim_end_matches(['/', '\\']), windows);
    if windows { n.to_lowercase() } else { n }
}

// IPv4 text as numbers, for sorting addresses in order
fn ip_order(ip: &str) -> Vec<i64> {
    ip.split('.').map(|k| k.parse().unwrap_or(i64::MAX)).collect()
}

// The strings in a this-pc.json list
fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

// What to change in PIA's settings st, given this-pc.json me, the nodes in use and extra addresses (macOS), and
// Reality Hop's own programs. in_use false (uninstall) takes Reality Hop's rules off again (engine.pia_bypass)
pub fn bypass_plan(
    st: &Map<String, Value>,
    me: &Map<String, Value>,
    mac: bool,
    nodes: &[String],
    extra: &[String],
    programs: &[String],
    in_use: bool,
) -> Bypass {
    let list = |k: &str| st.get(k).filter(|v| py_truthy(v)).and_then(Value::as_array).cloned().unwrap_or_default();
    let (cur, apps) = (list("bypassSubnets"), list("splitTunnelRules"));
    let mut change = Map::new();
    let mut add = Vec::new();
    let mut rules = Vec::new();
    let mut added: Vec<String> = strings(me.get("pia_apps"));
    if mac {
        let mine: Vec<String> = strings(me.get("pia_ips")).iter().map(|ip| format!("{ip}/32")).collect();
        let theirs: Vec<Value> = cur
            .iter()
            .filter(|r| {
                r.is_object() && !r.get("subnet").and_then(Value::as_str).is_some_and(|s| mine.iter().any(|m| m == s))
            })
            .cloned()
            .collect();
        let keep: Vec<&Value> = theirs.iter().map(|r| r.get("subnet").unwrap_or(&Value::Null)).collect();
        let mut want: Vec<String> = if in_use { nodes.to_vec() } else { Vec::new() };
        want.extend(extra.iter().cloned());
        want.sort_by_key(|ip| ip_order(ip));
        want.dedup();
        add = want
            .into_iter()
            .filter(|ip| crate::validate::public_ip(ip) && !keep.contains(&&Value::from(format!("{ip}/32"))))
            .collect();
        rules = theirs;
        rules.extend(add.iter().map(|ip| json!({"subnet": format!("{ip}/32"), "mode": "exclude"})));
        let subnets = |rs: &[Value]| -> Vec<Value> {
            rs.iter().filter(|r| r.is_object()).map(|r| r.get("subnet").cloned().unwrap_or(Value::Null)).collect()
        };
        if subnets(&rules) != subnets(&cur) {
            change.insert("bypassSubnets".into(), Value::Array(rules.clone()));
        }
        if me.get("pia_app").is_some_and(py_truthy) {
            added.extend(programs.iter().cloned()); // pia_app: up to 1.7.1
        }
    }
    let same = |r: &Value, p: &str| r.is_object() && key(r.get("path"), !mac) == key(Some(&Value::from(p)), !mac);
    let mut new = apps.clone();
    for p in programs {
        if in_use {
            if !new.iter().any(|r| same(r, p) && r.get("mode") == Some(&json!("exclude"))) {
                // missing, or set to Only VPN: Bypass VPN
                if !new.iter().any(|r| same(r, p)) && !added.contains(p) {
                    added.push(p.clone());
                }
                new.retain(|r| !same(r, p));
                new.push(json!({"path": p, "linkTarget": "", "mode": "exclude"}));
            }
        } else if added.contains(p) {
            // uninstall: only a rule Reality Hop added itself
            new.retain(|r| !same(r, p));
            added.retain(|a| a != p);
        }
    }
    if new != apps {
        change.insert("splitTunnelRules".into(), Value::Array(new));
    }
    let enabled = st.get("splitTunnelEnabled").is_some_and(py_truthy);
    if mac && in_use && (!rules.is_empty() || change.contains_key("splitTunnelRules")) && !enabled {
        change.insert("splitTunnelEnabled".into(), Value::Bool(true));
    }
    let mut apps_kept: Vec<String> = added.into_iter().filter(|p| programs.contains(p)).collect();
    apps_kept.sort();
    apps_kept.dedup();
    Bypass { change, ips: add, apps: apps_kept }
}

// The .app a program file runs from (<app>/Contents/MacOS/<program>), or None outside an app (macos.bundle)
pub fn bundle_of(exe: &std::path::Path) -> Option<PathBuf> {
    let b = exe.parent()?.parent()?.parent()?;
    b.to_string_lossy().ends_with(".app").then(|| b.to_path_buf())
}

// The RealityHop.app this program runs from
fn bundle() -> Option<String> {
    bundle_of(&std::env::current_exe().ok()?).map(|b| b.to_string_lossy().into_owned())
}

// Reality Hop's programs that belong on the list: the app on macOS; rh-core and rh-test on Windows
fn programs(paths: &Paths) -> Vec<String> {
    if cfg!(windows) {
        let test: PathBuf = paths.data.join(format!("rh-test{}", crate::paths::EXE));
        vec![paths.core.to_string_lossy().into_owned(), test.to_string_lossy().into_owned()]
    } else {
        bundle().into_iter().collect()
    }
}

// The node addresses on PIA's bypass list that are Reality Hop's: the ones it remembers adding, and any other single
// address that is a known node (node list, nodes.txt, vault). Two processes testing at once, or a lost this-pc.json,
// used to leave test addresses behind as if they were the user's; the list grew by up to 50 with each test
pub fn owned_ips(bypass: &[Value], remembered: &[String], known: &std::collections::HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = remembered.to_vec();
    for r in bypass {
        if let Some(ip) = r.get("subnet").and_then(Value::as_str).and_then(|s| s.strip_suffix("/32"))
            && known.contains(ip)
            && !out.iter().any(|o| o == ip)
        {
            out.push(ip.to_string());
        }
    }
    out
}

// Every node address Reality Hop knows of, offline: the saved node list, nodes.txt and the vault
fn known_nodes(paths: &Paths) -> std::collections::HashSet<String> {
    let mut k: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(raw) = crate::data_files::read_text(&paths.feed_cache) {
        k.extend(crate::node_feed::parse(&raw).into_iter().map(|n| n.server));
    }
    let txt = crate::data_files::read_text(&paths.nodes_txt).unwrap_or_default();
    k.extend(crate::node_tables::known_where(&txt).into_iter().map(|(s, _)| s));
    k.extend(crate::node_tables::known_failed(&txt));
    k.extend(crate::data_files::jload(&paths.vault).keys().cloned());
    k
}

// Keep Reality Hop on PIA's Split Tunnel list, through piactl's applysettings, which PIA marks unstable. False when
// PIA is not installed or the change failed (Check PIA then warns) (engine.pia_bypass)
pub fn pia_bypass(paths: &Paths, extra: &[String], in_use: bool) -> bool {
    if !std::path::Path::new(PIACTL).exists() {
        return false;
    }
    let Ok(Value::Object(st)) = crate::data_files::read_json(std::path::Path::new(PIA_SETTINGS)) else { return false };
    let mut me = crate::settings::this_pc(paths);
    let mac = !cfg!(windows);
    if mac {
        let cur = st.get("bypassSubnets").and_then(Value::as_array).cloned().unwrap_or_default();
        let owned = owned_ips(&cur, &strings(me.get("pia_ips")), &known_nodes(paths));
        me.insert("pia_ips".into(), json!(owned));
    }
    let nodes: Vec<String> = if mac && in_use {
        crate::singbox_config::nodes_of(&paths.pc_cfg).unwrap_or_default().into_iter().map(|(_, n)| n.server).collect()
    } else {
        Vec::new()
    };
    let plan = bypass_plan(&st, &me, mac, &nodes, extra, &programs(paths), in_use);
    let remember = |b: &Bypass| {
        let kw = json!({"pia_ips": b.ips, "pia_apps": b.apps, "pia_app": false});
        let _ = crate::settings::set_this_pc(paths, kw.as_object().cloned().unwrap_or_default());
    };
    if plan.change.is_empty() {
        remember(&plan);
        return true;
    }
    let arg = crate::python_json::dumps(&Value::Object(plan.change.clone()), None);
    let r = crate::process::run(&[PIACTL, "-u", "applysettings", &arg], None, Duration::from_secs(20), None);
    let (code, out, err) = match r {
        Ok(o) => (o.code.unwrap_or(1), o.stdout, o.stderr),
        Err(e) => (1, String::new(), e.to_string()),
    };
    dbg(|| {
        format!(
            "PIA split tunnel: {:?} {} addresses of Reality Hop; piactl exit {code} {}",
            plan.change.keys().collect::<Vec<_>>(),
            plan.ips.len(),
            crate::python_compat::strip(&err)
        )
    });
    if code != 0 {
        let why =
            crate::python_compat::strip(if err.is_empty() { &out } else { &err }).chars().take(200).collect::<String>();
        say(&format!(
            "PIA: could not update its split tunnel list ({why}); with a kill switch on, PIA cannot connect through Reality Hop"
        ));
        return false;
    }
    if plan.change.contains_key("splitTunnelRules") && in_use {
        let on = if plan.change.contains_key("splitTunnelEnabled") { "; split tunnel switched on" } else { "" };
        say(&format!("PIA: Reality Hop is in its split tunnel list (Bypass VPN){on}"));
    }
    remember(&plan);
    true
}
