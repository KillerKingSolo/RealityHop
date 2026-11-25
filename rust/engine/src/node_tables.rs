//! The text the engine prints and keeps: the ranked table, the status table and nodes.txt (and reading it back).
//! Nodes here are JSON objects with the test results (ok, ms, exit, why, mbps, where, isp, tcp_ms, inuse), as in
//! engine.py.

use crate::python_compat;
use crate::python_json::py_str;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

// A field as text: n.get(k, default) through str()
fn get(n: &Value, k: &str, default: &str) -> String {
    n.get(k).map_or(default.to_string(), py_str)
}

// n.get(k) or 9e9, the sort key for "fastest first, untested last"
fn ms_or_last(n: &Value) -> f64 {
    n.get("ms").and_then(Value::as_f64).filter(|m| *m != 0.0).unwrap_or(9e9)
}

// Python truthiness of n.get(k)
fn yes(n: &Value, k: &str) -> bool {
    match n.get(k) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(x)) => x.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(m)) => !m.is_empty(),
    }
}

// The ranked table of one scan (engine.table), one line per item
pub fn table(nodes: &[Value]) -> Vec<String> {
    let mut out = vec![format!(
        "\n{:>2} {:16} {:20} {:24} {:>5} {:>7}  result",
        "#", "server", "front", "where", "tcp", "tunnel"
    )];
    let mut sorted: Vec<&Value> = nodes.iter().collect();
    sorted.sort_by(|a, b| (!yes(a, "ok"), ms_or_last(a)).partial_cmp(&(!yes(b, "ok"), ms_or_last(b))).unwrap());
    for (i, n) in sorted.into_iter().enumerate() {
        let mut res = if yes(n, "ok") {
            let speed = match n.get("mbps") {
                Some(m) if !m.is_null() => format!("  {} Mbps", py_str(m)),
                _ => String::new(),
            };
            format!("PASS exit={}{speed}", get(n, "exit", ""))
        } else {
            format!("FAIL {}", get(n, "why", ""))
        };
        if yes(n, "inuse") {
            res.push_str("  (in use)");
        }
        let tun = if yes(n, "ok") { format!("{}ms", get(n, "ms", "")) } else { "-".into() };
        let tcp = if yes(n, "tcp_ms") { get(n, "tcp_ms", "") } else { "?".into() };
        out.push(format!(
            "{:>2} {:16} {:20} {:24} {:>5} {:>7}  {res}",
            i + 1,
            get(n, "server", ""),
            get(n, "sni", ""),
            get(n, "where", "?"),
            tcp,
            tun
        ));
    }
    out
}

// The live check table (engine.status_table): the router and PC result of each node in use
pub fn status_table(nodes: &[Value], ph: &Value, pc: &Value) -> Vec<String> {
    let fmt = |ms: &Value, why: &Value| {
        if ms.is_null() { format!("FAIL {}", py_str(why)) } else { format!("{}ms", py_str(ms)) }
    };
    let or_q = |v: &Value| {
        if yes(&Value::Object(Map::from_iter([("v".into(), v.clone())])), "v") { py_str(v) } else { "?".into() }
    };
    let mut out =
        vec![format!("\n{:8} {:16} {:20} {:24} {:>10}  {:>10}", "slot", "server", "front", "where", "phone", "PC")];
    for n in nodes {
        out.push(format!(
            "{:8} {:16} {:20} {:24} {:>10}  {:>10}",
            get(n, "tag", ""),
            get(n, "server", ""),
            get(n, "sni", ""),
            get(n, "where", "?"),
            fmt(&n["phone_ms"], &n["phone_why"]),
            fmt(&n["pc_ms"], &n["pc_why"])
        ));
    }
    let service = if yes(ph, "service") { "service running" } else { "SERVICE NOT RUNNING" };
    out.push(format!("\nrouter: {service}, active {}", or_q(&ph["now"])));
    out.push(if pc.is_null() {
        "PC: control API not reachable; rh-core is not running the current config yet (restart the PC tunnel)".into()
    } else {
        format!("PC: active {}, exit {}", or_q(&pc["now"]), or_q(&pc["exit"]))
    });
    out
}

// nodes.txt (engine.write_txt): the set in use, the spares and the failures of one scan, dated today
pub fn write_txt(nodes: &[Value], best: &[Value], today: &str) -> String {
    let row = |n: &Value, tag: &str| {
        let ms = if yes(n, "ms") { format!("{:>5}ms", get(n, "ms", "")) } else { "      ?".into() };
        let sid = if yes(n, "sid") { get(n, "sid", "") } else { "(empty)".into() };
        [
            format!(
                "  {tag:8} {:16} {:20} {ms}  {:26}  {}",
                get(n, "server", ""),
                get(n, "sni", ""),
                get(n, "where", "?"),
                get(n, "isp", "?")
            ),
            format!("           uuid={}  pbk={}  sid={sid}", get(n, "uuid", ""), get(n, "pbk", "")),
        ]
    };
    let mut l = vec![
        format!("VERIFIED REALITY NODES  -  tested {today} by Reality Hop (REALITY handshake + HTTPS fetch)"),
        String::new(),
        "tunnel ms = full HTTPS fetch through the node. TCP-open alone proves nothing:".into(),
        "a rejected key is silently forwarded to the front site and looks like a healthy host.".into(),
        String::new(),
        "IN USE  (both configs, urltest group \"auto\")".into(),
    ];
    for (i, n) in best.iter().enumerate() {
        l.extend(row(n, &format!("vless-{}", i + 1)));
    }
    let mut spares: Vec<&Value> = nodes.iter().collect();
    spares.sort_by(|a, b| ms_or_last(a).partial_cmp(&ms_or_last(b)).unwrap());
    l.push(String::new());
    l.push("SPARES  (passed, not in use; the window or  python engine.py --apply N  rotates them in)".into());
    for n in spares.into_iter().filter(|n| yes(n, "ok") && !best.contains(n)) {
        l.extend(row(n, ""));
    }
    l.push(String::new());
    l.push("FAILED  (do not retest until the feed changes their key)".into());
    for n in nodes.iter().filter(|n| !yes(n, "ok")) {
        l.push(format!("  {:16} {:20} {}", get(n, "server", ""), get(n, "sni", ""), get(n, "why", "")));
    }
    l.push(String::new());
    l.push(
        "WORKFLOW: Reality Hop window     python engine.py --check (health)     python engine.py (find + rank + apply)"
            .into(),
    );
    l.join("\n") + "\n"
}

// The lines of a text file as Python's text mode reads them: \r\n and \r become \n, each line keeps its \n
pub fn text_lines(text: &str) -> Vec<String> {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    t.split_inclusive('\n').map(str::to_string).collect()
}

static ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s+(?:vless-\d+\s+)?(\d+\.\d+\.\d+\.\d+)\s+\S+\s+(?:\d+ms|\?)\s+(.*)").unwrap());
static SPLIT2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());
static FAILED_ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s+(\d+\.\d+\.\d+\.\d+)\s").unwrap());

/// Where a node is, from nodes.txt.
#[derive(Debug, Clone, PartialEq)]
pub struct Where {
    pub place: String,
    pub isp: String,
}

// server -> location from the last nodes.txt (engine.known_where); no network needed; first row of a server wins
pub fn known_where(text: &str) -> Vec<(String, Where)> {
    let mut out: Vec<(String, Where)> = Vec::new();
    for l in text_lines(text) {
        let Some(m) = ROW.captures(&l) else { continue };
        let rest = python_compat::strip(m.get(2).map_or("", |x| x.as_str()));
        let mut parts: Vec<&str> = SPLIT2.split(rest).collect();
        parts.push("?");
        if !parts[0].is_empty() && parts[0] != "?" && !out.iter().any(|(s, _)| s == &m[1]) {
            out.push((m[1].to_string(), Where { place: parts[0].to_string(), isp: parts[1].to_string() }));
        }
    }
    out
}

// Servers in the FAILED section of the last nodes.txt (engine.known_failed), sorted
pub fn known_failed(text: &str) -> Vec<String> {
    let (mut out, mut failed) = (Vec::new(), false);
    for l in text_lines(text) {
        failed = failed || l.starts_with("FAILED");
        if failed && let Some(m) = FAILED_ROW.captures(&l) {
            out.push(m[1].to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// How long a failure that can pass later (no answer, too slow, cut) keeps a node out of the tests.
pub const FAIL_HOLD_SECS: u64 = 3600;

// known_failed, but once the scan is older than FAIL_HOLD_SECS only the nodes that serve another site's certificate
// stay out: a scan during a network fault fails good nodes for reasons that pass later (stale)
pub fn known_failed_fresh(text: &str, age_secs: u64) -> Vec<String> {
    if age_secs < FAIL_HOLD_SECS {
        return known_failed(text);
    }
    let (mut out, mut failed) = (Vec::new(), false);
    for l in text_lines(text) {
        failed = failed || l.starts_with("FAILED");
        if failed
            && l.contains("certificate is valid for")
            && let Some(m) = FAILED_ROW.captures(&l)
        {
            out.push(m[1].to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

// Seconds since nodes.txt was last written; u64::MAX when unknown
pub fn age_secs(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map_or(u64::MAX, |d| d.as_secs())
}
