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
    /// The download speed check runs at most this often (hours); 2.1.1 on, not in 1.x
    pub speed_hours: i64,
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
            speed_hours: 1,
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
        // a null text setting keeps its default (1.7.x read it as the text "None")
        let text = |k: &str, def: &str| raw.get(k).filter(|v| !v.is_null()).map_or(def.to_string(), py_str);
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
            speed_hours: int("speed_hours", d.speed_hours, 1, 24),
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

// this-pc.json: what only this PC knows (engine.this_pc)
pub fn this_pc(paths: &crate::paths::Paths) -> Map<String, Value> {
    crate::data_files::jload(&paths.this_pc)
}

// Merge fields into this-pc.json (engine.set_this_pc)
pub fn set_this_pc(paths: &crate::paths::Paths, kw: Map<String, Value>) -> std::io::Result<()> {
    let mut d = this_pc(paths);
    d.extend(kw);
    crate::data_files::jsave(&paths.this_pc, &Value::Object(d), None)
}

impl Settings {
    // settings.json from the data folder, with the one-time move of top from 40 to 50 saved (engine.settings_load)
    pub fn read(paths: &crate::paths::Paths) -> std::io::Result<Settings> {
        let raw = crate::data_files::jload(&paths.settings);
        let top50 = this_pc(paths).get("top50").is_some_and(|v| v.as_bool().unwrap_or(!v.is_null()));
        let (s, migrated) = Settings::load(&raw, top50);
        if let Some(m) = migrated {
            crate::data_files::jsave(&paths.settings, &Value::Object(m), Some(1))?;
            set_this_pc(paths, Map::from_iter([("top50".to_string(), Value::Bool(true))]))?;
        }
        Ok(s)
    }

    // Save some or all settings (unknown names ignored); the switching margin also goes into both configs. Returns the
    // settings read back, numbers out of range clamped (engine.settings_save)
    pub fn save(paths: &crate::paths::Paths, changes: &Map<String, Value>) -> std::io::Result<Settings> {
        let cur = Settings::read(paths)?;
        let Value::Object(mut file) = serde_json::to_value(&cur).map_err(std::io::Error::other)? else {
            unreachable!()
        };
        for (k, v) in changes {
            let typed = match file.get(k) {
                Some(Value::Number(_)) => py_int(v).map(Value::from),
                Some(Value::String(_)) => Some(Value::from(py_str(v))),
                _ => None,
            };
            if let Some(t) = typed {
                file.insert(k.clone(), t);
            }
        }
        crate::data_files::jsave(&paths.settings, &Value::Object(file), Some(1))?;
        let d = Settings::read(paths)?;
        for f in [&paths.pc_cfg, &paths.router_cfg] {
            if !f.exists() {
                continue;
            }
            let mut c = crate::data_files::read_json(f)?;
            let mut changed = false;
            for o in c["outbounds"].as_array_mut().into_iter().flatten() {
                if o["type"] == "urltest" && o.get("tolerance") != Some(&Value::from(d.tolerance_ms)) {
                    o["tolerance"] = Value::from(d.tolerance_ms);
                    changed = true;
                }
            }
            if changed {
                crate::data_files::jsave(f, &c, Some(2))?;
            }
        }
        Ok(d)
    }
}

// Python's bool() of a JSON value
pub fn py_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

// The settings, or the defaults when settings.json cannot be saved back (the read itself never fails)
fn current(paths: &crate::paths::Paths) -> Settings {
    Settings::read(paths).unwrap_or_else(|_| Settings::load(&crate::data_files::jload(&paths.settings), true).0)
}

// Only the router tunnels: no tunnel on this PC. Node tests then run on the router, since this PC's own traffic may
// go through the router tunnel and would skew them (engine.router_only)
pub fn router_only(paths: &crate::paths::Paths) -> bool {
    let s = current(paths);
    s.router != 0 && s.pc_tunnel == 0
}

// Router features off: only the PC tunnel, no SSH, no router section (engine.pc_only)
pub fn pc_only(paths: &crate::paths::Paths) -> bool {
    current(paths).router == 0
}

// After Stop: nothing starts the PC tunnel or the watcher until Start (engine.stopped)
pub fn stopped(paths: &crate::paths::Paths) -> bool {
    this_pc(paths).get("stopped").is_some_and(py_truthy)
}

// Stop or Start (engine.set_stopped)
pub fn set_stopped(paths: &crate::paths::Paths, on: bool) -> std::io::Result<()> {
    set_this_pc(paths, Map::from_iter([("stopped".to_string(), Value::Bool(on))]))
}
