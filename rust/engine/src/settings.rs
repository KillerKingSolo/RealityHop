//! settings.json: the person's settings, read the way engine.settings_load reads them.

use crate::python_json::py_str;
use crate::validate::host_ok;
use serde::Serialize;
use serde_json::{Map, Value};

/// The language codes Reality Hop speaks (i18n.CODES).
pub const LANG_CODES: [&str; 9] = ["en", "es", "zh", "ru", "uk", "fa", "ar", "tr", "vi"];
/// The name servers the configs can use (Settings 'dns').
pub const DNS_IPS: [(&str, &str); 2] = [("quad9", "9.9.9.9"), ("cloudflare", "1.1.1.1")];

/// Every setting, in engine.DEFAULTS order (settings.json is written in this order).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub check_min: i64,
    pub slow_ms: i64,
    pub tolerance_ms: i64,
    pub top: i64,
    pub router: i64,
    pub router_host: String,
    pub router_key: String,
    pub devices: String,
    pub auto_update: i64,
    pub singbox: String,
    pub router_singbox: String,
    pub backend_files: i64,
    pub start_menu: i64,
    pub desktop_icon: i64,
    pub pc_tunnel: i64,
    pub min_mbps: i64,
    pub lang: String,
    pub dns: String,
}

impl Default for Settings {
    // engine.DEFAULTS
    fn default() -> Self {
        Settings {
            check_min: 60,
            slow_ms: 200,
            tolerance_ms: 25,
            top: 50,
            router: 0,
            router_host: "192.168.8.1".into(),
            router_key: String::new(),
            devices: String::new(),
            auto_update: 1,
            singbox: "stable".into(),
            router_singbox: String::new(),
            backend_files: 0,
            start_menu: 1,
            desktop_icon: 0,
            pc_tunnel: 1,
            min_mbps: 100,
            lang: String::new(),
            dns: String::new(),
        }
    }
}

// Python's int(x) for a value from json.load; None where Python raises TypeError or ValueError
pub fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(*b as i64),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.is_finite()).map(|f| f.trunc() as i64)),
        Value::String(s) => int_literal(crate::python_compat::strip(s)),
        _ => None,
    }
}

// int("..."): optional sign, ASCII digits with single underscores between them
fn int_literal(s: &str) -> Option<i64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let b = body.as_bytes();
    let ok = !b.is_empty()
        && b[0].is_ascii_digit()
        && b[b.len() - 1].is_ascii_digit()
        && b.iter().all(|c| c.is_ascii_digit() || *c == b'_')
        && !body.contains("__");
    if !ok {
        return None;
    }
    let n: i64 = body.replace('_', "").parse().ok()?;
    Some(if neg { -n } else { n })
}

impl Settings {
    /// settings.json as engine.settings_load reads it. top50 is this-pc.json's flag. The second value is the raw
    /// file to write back when the one-time move of top from 40 (the default up to 1.5.2) to 50 applies.
    pub fn load(raw: &Map<String, Value>, top50: bool) -> (Settings, Option<Map<String, Value>>) {
        let mut raw = raw.clone();
        let mut migrated = None;
        if raw.get("top").and_then(Value::as_f64) == Some(40.0) && !raw["top"].is_boolean() && !top50 {
            raw.insert("top".into(), Value::from(50));
            migrated = Some(raw.clone());
        }
        let d = Settings::default();
        let int =
            |k: &str, def: i64, lo: i64, hi: i64| raw.get(k).map_or(Some(def), py_int).unwrap_or(def).clamp(lo, hi);
        let flag = |k: &str, def: i64| raw.get(k).map_or(Some(def), py_int).unwrap_or(def);
        let text = |k: &str, def: &str| raw.get(k).map_or(def.to_string(), py_str);
        let mut s = Settings {
            check_min: int("check_min", d.check_min, 15, 1440),
            slow_ms: int("slow_ms", d.slow_ms, 50, 5000),
            tolerance_ms: int("tolerance_ms", d.tolerance_ms, 0, 1000),
            top: int("top", d.top, 10, 120),
            router: flag("router", d.router),
            router_host: text("router_host", &d.router_host),
            router_key: text("router_key", &d.router_key),
            devices: text("devices", &d.devices),
            auto_update: flag("auto_update", d.auto_update),
            singbox: text("singbox", &d.singbox),
            router_singbox: text("router_singbox", &d.router_singbox),
            backend_files: flag("backend_files", d.backend_files),
            start_menu: flag("start_menu", d.start_menu),
            desktop_icon: flag("desktop_icon", d.desktop_icon),
            pc_tunnel: flag("pc_tunnel", d.pc_tunnel),
            min_mbps: int("min_mbps", d.min_mbps, 0, 1000),
            lang: text("lang", &d.lang),
            dns: text("dns", &d.dns),
        };
        if !host_ok(&s.router_host) {
            s.router_host = d.router_host;
        }
        if !["", "stable", "testing"].contains(&s.router_singbox.as_str()) {
            s.router_singbox = String::new();
        }
        if !s.dns.is_empty() && !DNS_IPS.iter().any(|(k, _)| *k == s.dns) {
            s.dns = "quad9".into();
        }
        if !LANG_CODES.contains(&s.lang.as_str()) {
            s.lang = String::new();
        }
        (s, migrated)
    }
}
