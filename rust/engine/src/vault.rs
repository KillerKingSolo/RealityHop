//! vault.json: every node that ever passed a test, the rescue pool. Entries are JSON objects whose key order
//! follows engine.py's dict operations, so the file comes out the same.

use crate::node::Node;
use crate::node_tables::{known_where, text_lines};
use crate::paths::Paths;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

/// The node fields a vault entry keeps.
pub const KEYS: [&str; 8] = ["server", "uuid", "pbk", "sid", "sni", "fp", "where", "isp"];

static ROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s+(?:vless-\d+\s+)?(\d+\.\d+\.\d+\.\d+)\s+(\S+)\s+(?:(\d+)ms|\?)").unwrap());
static KEY_ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s+uuid=(\S+)\s+pbk=(\S+)\s+sid=(\S+)").unwrap());

// The first vault, from the nodes listed in nodes.txt (in use + spares) and the nodes in use (engine.vault_seed)
pub fn seed(nodes_txt: &str, inuse: &[Node], today: &str) -> Map<String, Value> {
    let meta = known_where(nodes_txt);
    let (mut v, mut cur): (Map<String, Value>, Option<Map<String, Value>>) = (Map::new(), None);
    for l in text_lines(nodes_txt) {
        if l.starts_with("FAILED") {
            break;
        }
        if let Some(m) = ROW.captures(&l) {
            let ms = m.get(3).and_then(|x| x.as_str().parse::<i64>().ok()).filter(|n| *n != 0);
            let mut e = Map::new();
            e.insert("server".into(), m[1].into());
            e.insert("sni".into(), m[2].into());
            e.insert("fp".into(), "chrome".into());
            e.insert("ms".into(), ms.map_or(Value::Null, Value::from));
            if let Some((_, w)) = meta.iter().find(|(s, _)| s == &m[1]) {
                e.insert("where".into(), w.place.clone().into());
                e.insert("isp".into(), w.isp.clone().into());
            }
            cur = Some(e);
            continue;
        }
        if let Some(mut e) = cur.take() {
            if let Some(k) = KEY_ROW.captures(&l) {
                let sid = if &k[3] == "(empty)" { "" } else { &k[3] };
                for (key, val) in [
                    ("uuid", json!(&k[1])),
                    ("pbk", json!(&k[2])),
                    ("sid", json!(sid)),
                    ("last_ok", json!(today)),
                    ("passes", json!(1)),
                    ("fail_streak", json!(0)),
                ] {
                    e.insert(key.into(), val);
                }
                let server = e["server"].as_str().unwrap_or_default().to_string();
                v.insert(server, Value::Object(e));
            } else {
                cur = Some(e);
            }
        }
    }
    for n in inuse {
        if !v.contains_key(&n.server) {
            let node = serde_json::to_value(n).unwrap();
            let mut e: Map<String, Value> =
                KEYS.iter().map(|k| (k.to_string(), node.get(*k).cloned().unwrap_or(Value::Null))).collect();
            e.insert("last_ok".into(), today.into());
            e.insert("passes".into(), 1.into());
            e.insert("fail_streak".into(), 0.into());
            v.insert(n.server.clone(), Value::Object(e));
        }
    }
    v
}

// Test results into the vault (engine.vault_record): a pass refreshes the entry, three failures in a row remove it;
// the 200 most recently verified stay. now: "YYYY-MM-DD HH:MM:SS".
pub fn record(v: &mut Map<String, Value>, nodes: &[Value], now: &str) {
    for n in nodes {
        let Some(ok) = n.get("ok") else { continue };
        let server = n["server"].as_str().unwrap_or_default().to_string();
        if ok.as_bool().unwrap_or(false) {
            let e = v.entry(server).or_insert_with(|| json!({"passes": 0})).as_object_mut().unwrap();
            for k in KEYS {
                if let Some(x) = n.get(k).filter(|x| !x.is_null()) {
                    e.insert(k.into(), x.clone());
                }
            }
            let passes = e.get("passes").and_then(Value::as_i64).unwrap_or(0) + 1;
            e.insert("last_ok".into(), now.into());
            e.insert("ms".into(), n.get("ms").cloned().unwrap_or(Value::Null));
            e.insert("passes".into(), passes.into());
            e.insert("fail_streak".into(), 0.into());
        } else if let Some(e) = v.get_mut(&server).and_then(Value::as_object_mut) {
            let streak = e.get("fail_streak").and_then(Value::as_i64).unwrap_or(0) + 1;
            e.insert("fail_streak".into(), streak.into());
            if streak >= 3 {
                v.shift_remove(&server);
            }
        }
    }
    let mut items: Vec<(String, Value)> = std::mem::take(v).into_iter().collect();
    let last_ok = |e: &Value| e.get("last_ok").and_then(Value::as_str).unwrap_or_default().to_string();
    items.sort_by_key(|(_, e)| std::cmp::Reverse(last_ok(e)));
    v.extend(items.into_iter().take(200));
}

// Vault entries as nodes, most recently verified first, then fastest (engine.vault_nodes)
pub fn nodes(v: &Map<String, Value>, exclude: &[&str]) -> Vec<Value> {
    let mut out: Vec<Value> = v
        .iter()
        .filter(|(s, _)| !exclude.contains(&s.as_str()))
        .map(|(_, e)| {
            let mut e = e.as_object().cloned().unwrap_or_default();
            let fp =
                e.get("fp").filter(|f| f.as_str().is_some_and(|s| !s.is_empty())).cloned().unwrap_or("chrome".into());
            e.insert("fp".into(), fp);
            e.insert("tcp_ms".into(), Value::Null);
            Value::Object(e)
        })
        .collect();
    let key = |n: &Value| {
        let day: String = n.get("last_ok").and_then(Value::as_str).unwrap_or_default().chars().take(10).collect();
        let ms = n.get("ms").and_then(Value::as_f64).filter(|m| *m != 0.0).unwrap_or(9e9);
        (day, -ms)
    };
    out.sort_by(|a, b| key(b).partial_cmp(&key(a)).unwrap());
    out
}

// vault.json, or on the first run the vault built from nodes.txt and the nodes in use, saved (engine.vault_load)
pub fn load(paths: &Paths) -> Map<String, Value> {
    let v = crate::data_files::jload(&paths.vault);
    if !v.is_empty() {
        return v;
    }
    let txt = crate::data_files::read_text(&paths.nodes_txt).unwrap_or_default();
    let cfg = if crate::settings::pc_only(paths) { &paths.pc_cfg } else { &paths.router_cfg };
    let inuse: Vec<Node> =
        crate::singbox_config::nodes_of(cfg).unwrap_or_default().into_iter().map(|(_, n)| n).collect();
    let v = seed(&txt, &inuse, &crate::local_time::now().format("%Y-%m-%d"));
    if !v.is_empty() {
        let _ = crate::data_files::jsave(&paths.vault, &Value::Object(v.clone()), Some(1));
    }
    v
}

// Test results into vault.json (engine.vault_record)
pub fn record_file(paths: &Paths, nodes: &[Value]) -> std::io::Result<()> {
    let mut v = load(paths);
    record(&mut v, nodes, &crate::local_time::now().format("%Y-%m-%d %H:%M:%S"));
    crate::data_files::jsave(&paths.vault, &Value::Object(v), Some(1))
}

// vault.json's nodes, most recently verified first, then fastest (engine.vault_nodes)
pub fn vault_nodes(paths: &Paths, exclude: &[&str]) -> Vec<Value> {
    nodes(&load(paths), exclude)
}
