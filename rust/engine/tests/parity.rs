//! Parity with the Python code. tools/parity-dump.py runs the Python functions and writes rust/parity/<fn>.json:
//! {"fn": name, "source": file, "cases": [{"args": [...], "out": ...}]}. One test per function.

use engine::node::Node;
use engine::settings::Settings;
use engine::speed_test::{self, Speed};
use engine::{clash_api, node_feed, node_tables, powershell, python_json, singbox_config, validate, vault, version};
use serde_json::{Value, json};
use std::path::PathBuf;

// The fixtures folder, rust/parity
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("parity")
}

// Every fixture as (file stem, parsed JSON)
fn fixtures() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir()).expect("rust/parity is missing") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            let text = std::fs::read_to_string(&p).unwrap();
            let v = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            out.push((p.file_stem().unwrap().to_string_lossy().into_owned(), v));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// Run f on every recorded case of a Python function and compare the outputs
fn check(name: &str, f: impl Fn(&[Value]) -> Value) {
    let (_, v) = fixtures().into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no fixture for {name}"));
    let mut bad = Vec::new();
    for c in v["cases"].as_array().unwrap() {
        let got = f(c["args"].as_array().unwrap());
        if got != c["out"] {
            bad.push(format!("  args {}\n    python {}\n    rust   {}", c["args"], c["out"], got));
        }
    }
    assert!(bad.is_empty(), "{name}: {} case(s) differ\n{}", bad.len(), bad.join("\n"));
}

// An argument as text; None (null) as ""
fn s(a: &[Value], i: usize) -> &str {
    a[i].as_str().unwrap_or("")
}

// Python's {"error": type} for a raised exception
fn err(kind: &str) -> Value {
    json!({ "error": kind })
}

#[test]
fn fixtures_are_well_formed() {
    let all = fixtures();
    assert!(!all.is_empty(), "no fixtures: run python tools/parity-dump.py");
    for (stem, v) in all {
        assert_eq!(v["fn"].as_str(), Some(stem.as_str()), "{stem}: fn does not match the file name");
        assert!(v["source"].as_str().is_some_and(|s| s.starts_with("src/")), "{stem}: no source file");
        let cases = v["cases"].as_array().unwrap_or_else(|| panic!("{stem}: no cases"));
        assert!(!cases.is_empty(), "{stem}: no cases");
        for c in cases {
            assert!(c["args"].is_array() && c.get("out").is_some(), "{stem}: a case needs args and out");
        }
    }
}

#[test]
fn host_ok() {
    check("host_ok", |a| json!(validate::host_ok(s(a, 0))));
}

#[test]
fn public_ip() {
    check("public_ip", |a| json!(validate::public_ip(s(a, 0))));
}

#[test]
fn lan_ip() {
    check("lan_ip", |a| json!(validate::lan_ip(s(a, 0))));
}

#[test]
fn mirrors() {
    check("mirrors", |a| json!(node_feed::mirrors(s(a, 0))));
}

#[test]
fn feed_parse() {
    check("feed_parse", |a| json!(node_feed::parse(s(a, 0))));
}

#[test]
fn psq() {
    check("psq", |a| json!(powershell::psq(s(a, 0))));
}

#[test]
fn set_mark() {
    check("set_mark", |a| {
        let nodes = a[0].as_array().unwrap();
        json!(singbox_config::set_mark(nodes.iter().map(|n| n["server"].as_str().unwrap())))
    });
}

#[test]
fn parse_delay() {
    check("parse_delay", |a| match clash_api::parse_delay(s(a, 0)) {
        Ok((ms, why)) => json!([ms, why]),
        Err(_) => err("AttributeError"),
    });
}

#[test]
fn vkey() {
    check("vkey", |a| json!(version::vkey(s(a, 0))));
}

// A node from a fixture's JSON object
fn node(v: &Value) -> Node {
    serde_json::from_value(v.clone()).unwrap()
}

// A list of nodes from a fixture
fn nodes(v: &Value) -> Vec<Node> {
    v.as_array().unwrap().iter().map(node).collect()
}

#[test]
fn node_ok() {
    check("node_ok", |a| json!(validate::node_ok(&node(&a[0]))));
}

#[test]
fn outbound19() {
    check("outbound19", |a| singbox_config::outbound19(&node(&a[0]), s(a, 1)));
}

#[test]
fn from_outbound() {
    check("from_outbound", |a| {
        let (tag, n) = singbox_config::from_outbound(&a[0]).unwrap();
        json!({"tag": tag, "server": n.server, "uuid": n.uuid, "sni": n.sni, "fp": n.fp, "pbk": n.pbk, "sid": n.sid,
            "tcp_ms": null})
    });
}

#[test]
fn router_cfg_new() {
    check("router_cfg_new", |a| singbox_config::router_cfg_new(&a[0]));
}

#[test]
fn json_dumps() {
    check("json_dumps", |a| json!(python_json::dumps(&a[0], a[1].as_u64().map(|n| n as usize))));
}

#[test]
fn write_configs() {
    check("write_configs", |a| {
        let (pc, r) = singbox_config::write_configs(&a[0], &a[1], &nodes(&a[2]));
        json!([python_json::dumps(&pc, Some(2)), python_json::dumps(&r, Some(2))])
    });
}

#[test]
fn dns_apply() {
    check("dns_apply", |a| {
        let want = if s(a, 2).is_empty() { "quad9" } else { s(a, 2) };
        let (mut pc, mut r) = (a[0].clone(), a[1].clone());
        let changed = singbox_config::dns_apply(&mut pc, want) | singbox_config::dns_apply(&mut r, want);
        json!([changed, python_json::dumps(&pc, Some(2)), python_json::dumps(&r, Some(2))])
    });
}

#[test]
fn api_lockdown() {
    check("api_lockdown", |a| {
        let mut c = a[0].clone();
        let fixed = !singbox_config::api_lockdown(&mut c, a[1] == true).is_empty();
        let keep = if fixed && a[2] == true { &c } else { &a[0] };
        json!(python_json::dumps(keep, Some(2)))
    });
}

#[test]
fn settings_load() {
    check("settings_load", |a| {
        let raw = a[0].as_object().unwrap();
        let (st, migrated) = Settings::load(raw, a[1] == true);
        let file = migrated.map_or_else(|| a[0].clone(), Value::Object);
        json!([st, python_json::dumps(&file, Some(1))])
    });
}

#[test]
fn vtuple() {
    check("vtuple", |a| json!(version::vtuple(s(a, 0))));
}

// A JSON list of node objects
fn objs(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

// The fixed "now" of the fixtures: tools/parity-dump.py's NOW
const NOW: f64 = 1_800_000_000.0;

#[test]
fn table() {
    check("table", |a| json!(node_tables::table(&objs(&a[0]))));
}

#[test]
fn status_table() {
    check("status_table", |a| json!(node_tables::status_table(&objs(&a[0]), &a[1], &a[2])));
}

#[test]
fn write_txt() {
    check("write_txt", |a| {
        let all = objs(&a[0]);
        let best: Vec<Value> =
            objs(&a[1]).into_iter().map(|b| b.as_u64().map_or(b.clone(), |i| all[i as usize].clone())).collect();
        json!(node_tables::write_txt(&all, &best, "2026-11-18"))
    });
}

#[test]
fn known_where() {
    check("known_where", |a| {
        let full = a[1] == true;
        let m: serde_json::Map<String, Value> = node_tables::known_where(s(a, 0))
            .into_iter()
            .map(|(k, w)| (k, if full { json!({"where": w.place, "isp": w.isp}) } else { json!(w.place) }))
            .collect();
        Value::Object(m)
    });
}

#[test]
fn known_failed() {
    check("known_failed", |a| json!(node_tables::known_failed(s(a, 0))));
}

#[test]
fn vault_seed() {
    check("vault_seed", |a| {
        let inuse: Vec<Node> = a[1]["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["type"] == "vless")
            .map(|o| singbox_config::from_outbound(o).unwrap().1)
            .collect();
        let v = Value::Object(vault::seed(s(a, 0), &inuse, "2026-11-18"));
        let file = if v.as_object().unwrap().is_empty() { Value::Null } else { json!(python_json::dumps(&v, Some(1))) };
        json!([v, file])
    });
}

#[test]
fn vault_record() {
    check("vault_record", |a| {
        let mut v = a[0].as_object().cloned().unwrap();
        vault::record(&mut v, &objs(&a[1]), "2026-11-18 12:34:56");
        json!(python_json::dumps(&Value::Object(v), Some(1)))
    });
}

#[test]
fn vault_nodes() {
    check("vault_nodes", |a| {
        let ex: Vec<&str> = a[1].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        json!(vault::nodes(a[0].as_object().unwrap(), &ex))
    });
}

// this-pc.json's top_mbps from a fixture argument
fn top(v: &Value) -> speed_test::TopMbps {
    v.as_array().map(|t| (t[0].as_f64().unwrap(), t[1].as_f64().unwrap()))
}

#[test]
fn floor() {
    check("floor", |a| json!(speed_test::floor(a[0].as_i64().unwrap(), top(&a[1]), NOW)));
}

#[test]
fn speed_rank() {
    check("speed_rank", |a| {
        let min = a[0].as_i64().unwrap();
        let fl = speed_test::floor(min, top(&a[1]), NOW);
        let (g, v) = speed_test::speed_rank(min, fl, a[2]["mbps"].as_f64(), a[2]["ms"].as_f64());
        let py = |x: f64| if x == 9e9 { json!(x) } else { json!(x as i64) }; // ms and Mbps are ints, 9e9 a float
        json!([g, py(v)])
    });
}

#[test]
fn mbps() {
    check("mbps", |a| match speed_test::parse_mbps(s(a, 0)) {
        Speed::Mbps(m) => json!(m),
        Speed::Unknown => Value::Null,
    });
}
