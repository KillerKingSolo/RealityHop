//! Where each node is (city, region) and its ISP: from the vault, else from ip-api.com.

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

/// The batch geolocation service, asked through the PC tunnel.
pub const IP_API: &str = "http://ip-api.com/batch";

static JUNK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s\x00-\x1f\x7f]+").unwrap());

// A place or ISP name that crossed a public node: whitespace and control characters folded, at most k characters
fn clean(v: Option<&Value>, k: usize) -> String {
    let s = match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(false)) => String::new(),
        Some(x) if x.as_f64() == Some(0.0) => String::new(),
        Some(x) => crate::python_json::py_str(x),
    };
    let s: String = crate::python_compat::strip(&JUNK.replace_all(&s, " ")).chars().take(k).collect();
    if s.is_empty() { "?".into() } else { s }
}

/// The longest ISP name kept. 1.x kept 22 characters; a name cut there is looked up again.
const ISP_MAX: usize = 60;
const ISP_OLD_MAX: usize = 22;

/// Nodes whose 22-character ISP name was looked up again in this run: a name that is that long is not asked twice.
static RECHECKED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

// A node's where, if it is known (not missing, empty or "?"), with an ISP name that 1.x did not cut short
fn known(n: &Value) -> bool {
    let cut = n.get("isp").and_then(Value::as_str).is_some_and(|i| i.chars().count() == ISP_OLD_MAX)
        && !RECHECKED.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|s| n["server"] == s.as_str());
    n.get("where").and_then(Value::as_str).is_some_and(|w| !w.is_empty() && w != "?") && !cut
}

// Fill in where and isp on every node (engine.geolocate): from the vault first, then one ip-api batch for the rest.
// curl gets the arguments after the curl path and the request body; via_tunnel is false in router-only mode.
pub fn geolocate(
    nodes: &mut [Value],
    vault: &Map<String, Value>,
    via_tunnel: bool,
    curl: impl FnOnce(&[String], &str) -> Option<String>,
) {
    for n in nodes.iter_mut() {
        let Some(e) = vault.get(n["server"].as_str().unwrap_or_default()) else { continue };
        let has_where = n.get("where").is_some_and(|w| !w.is_null() && w != "");
        if !has_where && known(e) {
            let isp = e.get("isp").cloned().unwrap_or_else(|| "?".into());
            let m = n.as_object_mut().unwrap();
            m.insert("where".into(), e["where"].clone());
            m.insert("isp".into(), isp);
        }
    }
    let todo: Vec<usize> = (0..nodes.len()).filter(|&i| !known(&nodes[i])).collect();
    if todo.is_empty() {
        return;
    }
    RECHECKED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(todo.iter().map(|&i| nodes[i]["server"].as_str().unwrap_or_default().to_string()));
    let body = format!(
        "[{}]",
        todo.iter()
            .take(100)
            .map(|&i| format!(
                "{{\"query\":\"{}\",\"fields\":\"query,city,regionName,countryCode,isp\"}}",
                nodes[i]["server"].as_str().unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut args: Vec<String> = vec!["-s".into()];
    if via_tunnel {
        args.extend(["--socks5-hostname".into(), "127.0.0.1:2080".into()]);
    }
    args.extend(["-m", "20", "-H", "Content-Type: application/json", "--data-binary", "@-", IP_API].map(String::from));
    let geo: Map<String, Value> = curl(&args, &body)
        .and_then(|out| serde_json::from_str::<Value>(&out).ok())
        .and_then(|v| {
            let list = v.as_array()?.clone();
            list.into_iter().map(|g| Some((g.get("query")?.as_str()?.to_string(), g))).collect::<Option<Map<_, _>>>()
        })
        .unwrap_or_default();
    for i in todo {
        let g = geo.get(nodes[i]["server"].as_str().unwrap_or_default()).cloned().unwrap_or(Value::Null);
        let has_city = g.get("city").is_some_and(|c| !c.is_null() && c != "" && c != false && c != 0);
        let place = if has_city {
            format!("{}, {}", clean(g.get("city"), 30), clean(g.get("regionName"), 30))
        } else {
            "?".into()
        };
        let m = nodes[i].as_object_mut().unwrap();
        m.insert("where".into(), place.into());
        m.insert("isp".into(), clean(g.get("isp"), ISP_MAX).into());
    }
}
