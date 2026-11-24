//! Parity with the Python code. tools/parity-dump.py runs the Python functions and writes rust/parity/<fn>.json:
//! {"fn": name, "source": file, "cases": [{"args": [...], "out": ...}]}. One test per function.

use engine::node::Node;
use engine::router::{self, Ssh};
use engine::settings::Settings;
use engine::speed_test::{self, Speed};
use engine::{
    clash_api, installer, node_feed, node_location, node_tables, powershell, problem_report, python_json,
    singbox_config, translations, validate, vault, version, windows_tasks,
};
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
    run_cases(name, f, |_, _, _| None);
}

/// A 1.7.x bug the Rust version fixes: given (args, Python's output, Rust's output), the output now expected for a
/// case that is meant to differ, or None for a case that must still match Python.
type Fix = fn(&[Value], &Value, &Value) -> Option<Value>;

// As check, except the cases a fix names: those must differ from Python and give the fixed output
fn check_fixed(name: &str, f: impl Fn(&[Value]) -> Value, fix: Fix) {
    assert!(run_cases(name, f, fix) > 0, "{name}: no case shows the fix any more; update the fixture or the fix");
}

// Compare every case; returns how many cases a fix named
fn run_cases(name: &str, f: impl Fn(&[Value]) -> Value, fix: Fix) -> usize {
    let (_, v) = fixtures().into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no fixture for {name}"));
    let (mut bad, mut fixed) = (Vec::new(), 0);
    for c in v["cases"].as_array().unwrap() {
        let args = c["args"].as_array().unwrap();
        let got = f(args);
        let want = match fix(args, &c["out"], &got) {
            Some(w) => {
                fixed += 1;
                assert_ne!(w, c["out"], "{name}: a fix that gives Python's own output: {}", c["args"]);
                w
            }
            None => c["out"].clone(),
        };
        if got != want {
            bad.push(format!("  args {}\n    expected {}\n    rust     {}", c["args"], want, got));
        }
    }
    assert!(bad.is_empty(), "{name}: {} case(s) differ\n{}", bad.len(), bad.join("\n"));
    fixed
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
    assert!(!all.is_empty(), "no fixtures in rust/parity");
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

// A dotted address in 224.0.0.0/4 (multicast)
fn multicast(ip: &str) -> bool {
    ip.parse::<std::net::Ipv4Addr>().is_ok_and(|a| a.is_multicast())
}

#[test]
fn public_ip() {
    // fix: multicast is no node address
    check_fixed(
        "public_ip",
        |a| json!(validate::public_ip(s(a, 0))),
        |a, _, _| multicast(s(a, 0)).then(|| json!(false)),
    );
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
    // fix: a node list line that points at a multicast address is dropped
    check_fixed(
        "feed_parse",
        |a| json!(node_feed::parse(s(a, 0))),
        |_, py, _| {
            let list = py.as_array()?;
            let kept: Vec<Value> =
                list.iter().filter(|n| !multicast(n["server"].as_str().unwrap_or_default())).cloned().collect();
            (kept.len() < list.len()).then(|| json!(kept))
        },
    );
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
    let load = |a: &[Value]| {
        let raw = a[0].as_object().unwrap();
        let (st, migrated) = Settings::load(raw, a[1] == true);
        let file = migrated.map_or_else(|| a[0].clone(), Value::Object);
        json!([st, python_json::dumps(&file, Some(1))])
    };
    // fix: a null text setting keeps its default instead of becoming the text "None"
    check_fixed("settings_load", load, |a, py, _| {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        let mut want = py.clone();
        want[0]["speed_hours"] = defaults["speed_hours"].clone(); // a setting 1.x does not have
        for (k, v) in a[0].as_object()? {
            if v.is_null() && defaults[k].is_string() {
                want[0][k] = defaults[k].clone();
            }
        }
        (want != *py).then_some(want)
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

/// Replays recorded router answers and records every command, as the dumper's over_ssh does.
struct Replay {
    replies: Vec<Value>,
    calls: Vec<Value>,
}

impl Replay {
    // From a fixture's list of replies (text, or {"error": message})
    fn new(v: &Value) -> Self {
        Replay { replies: objs(v), calls: Vec::new() }
    }
}

impl Ssh for Replay {
    // Record the call, then answer with the next reply
    fn run(&mut self, cmd: &str, stdin: Option<&str>, timeout: u64) -> Result<String, router::Error> {
        self.calls.push(json!({"cmd": cmd, "stdin": stdin, "timeout": timeout}));
        if self.replies.is_empty() {
            return Err(router::Error::Failed("router unreachable over ssh".into()));
        }
        match self.replies.remove(0) {
            Value::String(s) => Ok(s),
            e => Err(router::Error::Failed(e["error"].as_str().unwrap_or_default().into())),
        }
    }
}

// A router error as the dumper records it
fn router_err(e: router::Error) -> Value {
    match e {
        router::Error::Failed(m) => json!({"error": "RuntimeError", "message": m}),
        router::Error::BadAnswer(_) => err("ValueError"),
    }
}

// A file under rust/resources
fn resource(path: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources").join(path)).unwrap()
}

#[test]
fn handshake() {
    let run = |a: &[Value]| {
        let (mut nodes, mut r) = (objs(&a[0]), Replay::new(&a[1]));
        match router::handshake(&mut r, &mut nodes) {
            Ok(()) => json!([nodes, r.calls]),
            Err(e @ router::Error::BadAnswer(_)) => router_err(e),
            Err(e) => json!([router_err(e), r.calls]),
        }
    };
    // fix: a decimal comma in a time (1,5) is read as 1.5 s instead of stopping the whole test; a node with a pass
    // but no time keeps exit "-" as in 1.7.x
    check_fixed("handshake", run, |a, py, rust| {
        if *py != err("ValueError") {
            return None;
        }
        let results = [(true, "-", Value::Null), (false, "-", Value::Null), (true, "9.9.9.9", json!(1500))];
        let nodes: Vec<Value> = objs(&a[0])
            .into_iter()
            .zip(results)
            .map(|(mut n, (ok, exit, ms))| {
                let m = n.as_object_mut().unwrap();
                m.insert("ok".into(), json!(ok));
                m.insert("exit".into(), json!(exit));
                m.insert("ms".into(), ms);
                m.insert("why".into(), json!("no result"));
                n
            })
            .collect();
        Some(json!([nodes, rust[1]]))
    });
}

#[test]
fn router_ping() {
    check("router_ping", |a| {
        let (mut nodes, mut r) = (objs(&a[0]), Replay::new(&a[1]));
        let live = router::router_ping(&mut r, &mut nodes)
            .map_or_else(router_err, |l| json!(l.iter().map(|&k| nodes[k]["server"].clone()).collect::<Vec<_>>()));
        json!([live, nodes.iter().map(|n| n.get("tcp_ms").cloned()).collect::<Vec<_>>(), r.calls])
    });
}

#[test]
fn check_phone() {
    check("check_phone", |a| {
        let (mut nodes, mut r) = (objs(&a[0]), Replay::new(&a[1]));
        let res = router::check_phone(&mut r, &mut nodes, "0123456789abcdef0123456789abcdef")
            .map_or_else(router_err, |c| json!({"service": c.service, "now": c.now, "cfg": c.cfg}));
        let fields: Vec<Value> = nodes.iter().map(|n| json!([n.get("phone_ms"), n.get("phone_why")])).collect();
        json!([res, fields, r.calls])
    });
}

#[test]
fn router_now() {
    check("router_now", |a| {
        let mut r = Replay::new(&a[1]);
        let now = router::router_now(&mut r, "0123456789abcdef0123456789abcdef");
        json!([now, r.calls])
    });
}

#[test]
fn router_feed() {
    check("router_feed", |a| {
        let mut r = Replay::new(&a[1]);
        json!([router::router_feed(&mut r).map_or_else(router_err, Value::from), r.calls])
    });
}

#[test]
fn router_info() {
    check("router_info", |a| {
        let mut r = Replay::new(&a[1]);
        let out = router::router_info(&mut r).map_or_else(router_err, |i| {
            json!({"model": i.model, "firmware": i.firmware, "singbox": i.singbox, "too_new": i.too_new, "tools": i.tools})
        });
        json!([out, r.calls])
    });
}

#[test]
fn router_core_state() {
    check("router_core_state", |a| {
        let mut r = Replay::new(&a[1]);
        let out = router::router_core_state(&mut r)
            .map_or_else(router_err, |s| json!({"ver": s.ver, "mark": s.mark, "arch": s.arch}));
        json!([out, r.calls])
    });
}

#[test]
fn router_clients() {
    check("router_clients", |a| {
        let mut r = Replay::new(&a[1]);
        json!([router::router_clients(&mut r).map_or_else(router_err, |c| json!(c)), r.calls])
    });
}

#[test]
fn router_off() {
    check("router_off", |a| {
        let mut r = Replay::new(&a[1]);
        json!([router::router_off(&mut r).map_or_else(router_err, Value::from), r.calls])
    });
}

#[test]
fn router_install() {
    check("router_install", |a| {
        let mut r = Replay::new(&a[1]);
        json!([router::router_install(&mut r).map_or_else(router_err, Value::from), r.calls])
    });
}

#[test]
fn push() {
    let (nft, heal) = (resource("router/nft-devices.sh"), resource("router/heal.sh"));
    check("push", |a| {
        let mut r = Replay::new(&a[1]);
        let used: Vec<&str> = a[2]["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["type"] == "vless")
            .map(|o| o["server"].as_str().unwrap())
            .collect();
        let vault = vault::nodes(a[3].as_object().unwrap(), &used);
        let p = router::Push { router_cfg: &a[2], nft_template: &nft, devices: s(a, 0), heal_sh: &heal, vault: &vault };
        match router::push(&mut r, &p) {
            Ok(done) => json!([done.answer, r.calls, done.md5]),
            Err(e) => json!([router_err(e), r.calls, null]),
        }
    });
}

#[test]
fn nft_script() {
    let nft = resource("router/nft-devices.sh");
    check("nft_script", |a| json!(router::nft_script(&nft, s(a, 0))));
}

#[test]
fn rc_script() {
    check("rc_script", |a| {
        let m = router::Manifest {
            channel: a[0]["channel"].as_str().unwrap(),
            singbox: a[0]["singbox"].as_str().unwrap(),
            sha256: a[0]["sha256"].as_str().unwrap(),
        };
        json!(router::rc_script(&m, &a[1], &router::secret(&a[1])))
    });
}

#[test]
fn boot_task_ps() {
    check("boot_task_ps", |a| json!(windows_tasks::boot_task_ps(s(a, 0), s(a, 1), s(a, 2), s(a, 3))));
}

#[test]
fn logon_task() {
    check("logon_task", |a| {
        let cmd: Vec<&str> = a[0].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        let ps = windows_tasks::logon_task_ps(&cmd, r"C:\Program Files\RealityHop", s(a, 1), s(a, 2));
        json!([true, [{"cmd": ["powershell", "-NoProfile", "-Command", ps], "stdin": null, "timeout": 60}]])
    });
}

#[test]
fn lock_ps() {
    check("lock_ps", |a| json!(windows_tasks::lock_ps(s(a, 0))));
}

#[test]
fn legacy_ps() {
    check("legacy_ps", |_| json!(windows_tasks::legacy_ps()));
}

#[test]
fn elevated_params() {
    check("elevated_params", |a| json!(windows_tasks::elevated_params(s(a, 0))));
}

#[test]
fn shortcuts() {
    let menu = r"C:\Users\Pat\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Reality Hop.lnk";
    let desktop = "C:\\Users\\Pat O\u{2019}Neil\\Desktop\\Reality Hop.lnk";
    check("shortcuts", |a| {
        let calls: Vec<Value> = [(menu, &a[1]), (desktop, &a[2])]
            .into_iter()
            .filter(|(_, on)| **on == 1)
            .map(|(lnk, _)| {
                let ps = windows_tasks::shortcut_ps(lnk, s(a, 0));
                json!({"cmd": ["powershell", "-NoProfile", "-Command", ps], "stdin": null, "timeout": 30})
            })
            .collect();
        json!(calls)
    });
}

#[test]
fn add_key_console() {
    check("add_key_console", |a| {
        json!([windows_tasks::add_key_cmd("<ssh>", s(a, 0), s(a, 1)), [["cmd.exe", "/c", r".\add-router-key.cmd"]]])
    });
}

#[test]
fn router_key_remove() {
    check("router_key_remove", |a| {
        if !s(a, 1).is_empty() {
            return json!(["router: your own key file was used, so it stays on the router", []]);
        }
        let mut r = Replay::new(&json!(["router: the key of this PC is removed\n"]));
        let out = r.run(&windows_tasks::remove_key_cmd(s(a, 0)), None, 30).unwrap();
        json!([out.trim(), r.calls])
    });
}

#[test]
fn user_sid() {
    check("user_sid", |a| json!(installer::user_sid(s(a, 0))));
}

#[test]
fn entry() {
    check("entry", |a| {
        let ver = a[2].as_str().unwrap_or("1.7.2");
        Value::Object(
            installer::entry(s(a, 0), s(a, 1), ver).into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect(),
        )
    });
}

#[test]
fn latest() {
    check("latest", |a| match installer::latest(&a[0], installer::ASSET) {
        Some(l) => json!({"ver": l.ver, "asset": l.asset, "sha": l.sha, "notes": l.notes}),
        None => Value::Null,
    });
}

#[test]
fn changes() {
    check("changes", |a| json!(installer::changes(&a[0], s(a, 1))));
}

#[test]
fn geolocate() {
    // fix: an ISP name is kept up to 60 characters; 1.x cut it at 22
    let fix: Fix = |_, py, got| {
        let mut want = py.clone();
        let mut changed = false;
        for (w, g) in want[0].as_array_mut()?.iter_mut().zip(got[0].as_array()?) {
            let (old, new) = (w["isp"].as_str().unwrap_or_default(), g["isp"].as_str().unwrap_or_default());
            if old.chars().count() == 22 && new.len() > old.len() && new.starts_with(old) && new.chars().count() <= 60 {
                w["isp"] = json!(new);
                changed = true;
            }
        }
        changed.then_some(want)
    };
    check_fixed(
        "geolocate",
        |a| {
            let mut nodes = objs(&a[0]);
            let answer = s(a, 2).to_string();
            let mut calls = Vec::new();
            node_location::geolocate(&mut nodes, a[1].as_object().unwrap(), a[3] != true, |args, body| {
                let mut cmd = vec![json!("curl.exe")];
                cmd.extend(args.iter().map(|x| json!(x)));
                calls.push(json!({"cmd": cmd, "stdin": body, "timeout": 30}));
                Some(answer)
            });
            json!([nodes, calls])
        },
        fix,
    );
}

#[test]
fn scrub() {
    // fix: the whole profile folder name is hidden, spaces included, and a key that ends in - or _ is hidden too
    let fix: Fix = |_, py, _| {
        let key = format!("{}c_-", "Ab".repeat(20));
        let want: Vec<Value> = py
            .as_array()?
            .iter()
            .map(|t| json!(t.as_str().unwrap().replace("<user> Else", "<user>").replace(&key, "<key>")))
            .collect();
        (json!(want) != *py).then(|| json!(want))
    };
    check_fixed("scrub", scrub_with, fix);
}

// The scrubber over a fixture's texts, for the fixture's made-up person
fn scrub_with(a: &[Value]) -> Value {
    let me = &a[1];
    let id = problem_report::Identity {
        secrets: vec!["0123456789abcdef0123456789abcdef".into(); 2],
        nodes: ["8.8.8.8", "1.1.1.1", "9.9.9.9"].map(String::from).into(),
        home: me["USERPROFILE"].as_str().unwrap(),
        user: me["USERNAME"].as_str(),
        computer: me["COMPUTERNAME"].as_str(),
        domain: me["USERDOMAIN"].as_str(),
        sep: '\\',
    };
    let mut sc = problem_report::Scrubber::new(&id);
    json!(a[0].as_array().unwrap().iter().map(|t| sc.scrub(t.as_str().unwrap())).collect::<Vec<_>>())
}

#[test]
fn core_lines() {
    check("core_lines", |a| {
        let lines: Vec<String> = a[0].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
        json!(problem_report::core_lines(&lines))
    });
}

#[test]
fn issue_url() {
    check("issue_url", |a| json!(problem_report::issue_url(s(a, 0), s(a, 1))));
}

// A language's catalog from rust/resources/lang
fn catalog(lang: &str) -> Option<String> {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../resources/lang/{lang}.json")))
        .ok()
}

#[test]
fn t() {
    check("t", |a| {
        let tr = translations::Translations::new(s(a, 0), catalog(s(a, 0)).as_deref());
        let kw: Vec<(&str, String)> =
            a[3].as_object().unwrap().iter().map(|(k, v)| (k.as_str(), python_json::py_str(v))).collect();
        json!(tr.t(s(a, 2), &kw, a[1] == true))
    });
}

#[test]
fn mac_words() {
    check("mac_words", |a| json!(translations::mac_words(s(a, 0), s(a, 1))));
}

#[test]
fn tray_mark() {
    check("tray_mark", |a| {
        let res: Vec<(String, Option<f64>)> =
            a[0].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_f64())).collect();
        let dead: Option<Vec<String>> =
            a[1].as_array().map(|d| d.iter().map(|x| x.as_str().unwrap().to_string()).collect());
        let m = engine::tray_state::tray_mark(&res, dead.as_deref(), 200.0);
        json!({"last": [m.state, format!("{} of {} nodes working", m.working, m.total), "12:34"], "segs": m.segs})
    });
}

#[test]
fn vpn_state() {
    check("vpn_state", |a| {
        let english = |s: &str, kw: &[(&str, String)]| translations::format(s, kw).unwrap();
        let (found, wrong) =
            if a[0].is_null() { (false, vec![]) } else { (true, engine::pia::wrong_settings(&a[0], None)) };
        json!([[found, wrong], engine::pia::vpn_check(found, &wrong, english)])
    });
}

#[test]
fn arp_windows() {
    check("arp_windows", |a| json!(router::arp_mac_windows(s(a, 0))));
}

#[test]
fn mac_from_arp() {
    check("mac_from_arp", |a| json!(engine::macos::mac_from_arp(s(a, 0))));
}

#[test]
fn dns_proxies() {
    check("dns_proxies", |a| json!(engine::macos::dns_proxies(s(a, 0))));
}

#[test]
fn agent_plist() {
    check("agent_plist", |a| {
        let args: Vec<&str> = a[1].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        json!(engine::macos::agent_plist(s(a, 0), &args, s(a, 2)))
    });
}

// An empty folder for one test under the system temp folder, removed first if a run left it
fn temp_data(name: &str) -> engine::paths::Paths {
    let d = std::env::temp_dir().join(format!("rh-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    engine::paths::Paths::new(&d)
}

// A data file as Python's text mode reads it, after checking the line endings on disk are Python's
fn disk_text(p: &std::path::Path) -> String {
    let raw = String::from_utf8(std::fs::read(p).unwrap()).unwrap();
    if cfg!(windows) && raw.contains('\n') {
        assert_eq!(raw.matches('\n').count(), raw.matches("\r\n").count(), "{}: every newline is CRLF", p.display());
    }
    engine::data_files::read_text(p).unwrap()
}

#[test]
fn settings_save() {
    // the new speed_hours setting is added at the end of settings.json; everything else as 1.x
    check_fixed("settings_save", settings_save_one, |_, py, _| {
        let mut want = py.clone();
        want[0]["speed_hours"] = json!(1);
        let text = py[1].as_str()?;
        want[1] = json!(format!(
            "{},
 \"speed_hours\": 1
}}",
            text.trim_end().strip_suffix(
                "
}"
            )?
        ));
        Some(want)
    });
}

fn settings_save_one(a: &[Value]) -> Value {
    let p = temp_data("settings-save");
    engine::data_files::jsave(&p.settings, &a[0], Some(1)).unwrap();
    engine::data_files::jsave(&p.pc_cfg, &a[2], Some(2)).unwrap();
    engine::data_files::jsave(&p.router_cfg, &a[3], Some(2)).unwrap();
    engine::data_files::jsave(&p.this_pc, &json!({}), None).unwrap();
    let d = Settings::save(&p, a[1].as_object().unwrap()).unwrap();
    let out = json!([d, disk_text(&p.settings), disk_text(&p.pc_cfg), disk_text(&p.router_cfg)]);
    let _ = std::fs::remove_dir_all(&p.data);
    out
}

#[test]
fn hist_add() {
    check("hist_add", |a| {
        let p = temp_data("hist-add");
        if !a[0].is_null() {
            engine::data_files::jsave(&p.history, &a[0], None).unwrap();
        }
        let pings: Vec<(String, Option<i64>)> =
            a[1].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.as_i64())).collect();
        let event = a[2].as_array().map(|e| (e[0].as_str().unwrap(), e[1].as_str().unwrap()));
        engine::history::record(&p.history, &pings, event, NOW);
        let out = json!(disk_text(&p.history));
        let _ = std::fs::remove_dir_all(&p.data);
        out
    });
}

// pia_bypass: the plan, played out as the fixture records it (result, piactl arguments, messages, this-pc.json)
#[test]
fn pia_bypass() {
    use engine::pia_split_tunnel::bypass_plan;
    let strs =
        |v: &Value| -> Vec<String> { v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect() };
    check("pia_bypass", |a| {
        let me = a[1].as_object().unwrap().clone();
        let Some(st) = a[0].as_object() else { return json!([false, [], [], me]) };
        let (mac, app, in_use, fail) = (a[2] == true, a[5] == true, a[6] == true, a.get(7) == Some(&json!(true)));
        let programs: Vec<String> = match (mac, app) {
            (false, _) => vec![
                r"C:\Program Files\RealityHop\PC\rh-core.exe".into(),
                r"C:\Program Files\RealityHop\rh-test.exe".into(),
            ],
            (true, true) => vec!["/Applications/RealityHop.app".into()],
            (true, false) => vec![],
        };
        let plan = bypass_plan(st, &me, mac, &strs(&a[3]), &strs(&a[4]), &programs, in_use);
        let mut after = me.clone();
        after
            .extend(json!({"pia_ips": plan.ips, "pia_apps": plan.apps, "pia_app": false}).as_object().unwrap().clone());
        if plan.change.is_empty() {
            return json!([true, [], [], after]);
        }
        let calls =
            json!([["-u", "applysettings", engine::python_json::dumps(&Value::Object(plan.change.clone()), None)]]);
        if fail {
            let why = "PIA: could not update its split tunnel list (bad settings); with a kill switch on, PIA cannot connect through Reality Hop";
            return json!([false, calls, [why], me]);
        }
        let mut said = vec![];
        if plan.change.contains_key("splitTunnelRules") && in_use {
            let on = if plan.change.contains_key("splitTunnelEnabled") { "; split tunnel switched on" } else { "" };
            said.push(format!("PIA: Reality Hop is in its split tunnel list (Bypass VPN){on}"));
        }
        json!([true, calls, said, after])
    });
}

// core_release and core_versions: the release picked for each channel (Windows builds)
#[test]
fn core_release() {
    use engine::pc_tunnel::{channel_versions, pick_release};
    let win = |tag: &str| engine::version::core_asset(tag, None);
    check("core_release", |a| {
        let rels = a[0].as_array().unwrap();
        let tag = match pick_release(rels, a[1] == "testing", win) {
            Some(r) => r["tag_name"].clone(),
            None => json!({"error": "no sing-box release for Windows found on GitHub"}),
        };
        let (stable, testing) = channel_versions(rels, win);
        json!([tag, {"stable": stable, "testing": testing}])
    });
}

// speed_test: the download tests run, the speeds set, the line said and this-pc.json after, with the download test
// faked from the fixture's table
#[test]
fn speed_test_plan() {
    use std::sync::Mutex;
    static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
    engine::activity_log::set_say(|m| SAID.lock().unwrap().push(m.to_string()));
    check("speed_test", |a| {
        let d = std::env::temp_dir().join(format!("rh-parity-speed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let paths = engine::paths::Paths::new(&d);
        let me = if a[3].is_null() { json!({}) } else { json!({"top_mbps": a[3]}) };
        engine::data_files::jsave(&paths.this_pc, &me, None).unwrap();
        let mut nodes = a[0].as_array().unwrap().clone();
        let (shared, alone) = (Mutex::new(vec![]), Mutex::new(vec![]));
        SAID.lock().unwrap().retain(|l| !l.starts_with("speed:"));
        engine::node_tests::speed_test_with(
            &paths,
            &mut nodes,
            23000,
            a[2].as_i64().unwrap(),
            NOW,
            |port, secs, give_up| {
                (if give_up { &alone } else { &shared }).lock().unwrap().push(json!([port, secs]));
                Ok(a[1][port.to_string()][usize::from(give_up)].as_i64())
            },
        )
        .unwrap();
        let mut shared = shared.into_inner().unwrap();
        shared.sort_by_key(|c| c[0].as_u64());
        let said: Vec<String> = SAID.lock().unwrap().iter().filter(|l| l.starts_with("speed:")).cloned().collect();
        let after = Value::Object(engine::data_files::jload(&paths.this_pc));
        let _ = std::fs::remove_dir_all(&d);
        json!([nodes, shared, alone.into_inner().unwrap(), said, after])
    });
}

// router_sync: following another PC's or the router's own node change, in a sandbox data folder; the PC tunnel
// restart is left out (no rh-core there). nodes.txt carries today's date, so its date is set to the fixture's.
#[test]
fn router_sync() {
    sync_cases("sync", engine::node_manager::router_sync);
}

// The watcher and the window follow the router through follow_router: with no rh-core it does no speed test, so it
// must give the same result as router_sync
#[test]
fn follow_router_without_core_is_router_sync() {
    sync_cases("follow", engine::watcher::follow_router);
}

type SyncFn = fn(&engine::paths::Paths, &mut Replay) -> Result<Option<String>, String>;

fn sync_cases(tag: &str, f: SyncFn) {
    use engine::data_files::{jsave, read_text};
    let date = regex::Regex::new(r"tested \d{4}-\d{2}-\d{2} ").unwrap();
    check("router_sync", |a| {
        let d = std::env::temp_dir().join(format!("rh-parity-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let paths = engine::paths::Paths::new(&d);
        jsave(&paths.settings, &json!({"router": 1, "devices": a[3]}), Some(1)).unwrap();
        jsave(&paths.router_cfg, &a[4], Some(2)).unwrap();
        jsave(&paths.pc_cfg, &a[5], Some(2)).unwrap();
        jsave(&paths.this_pc, &a[1], None).unwrap();
        jsave(&paths.vault, &a[2], Some(1)).unwrap();
        let mut r = Replay::new(&json!([a[0], ""]));
        let out = match f(&paths, &mut r) {
            Ok(s) => json!(s),
            Err(e) => json!({"error": "RuntimeError", "message": e}),
        };
        let text = |p: &std::path::Path| read_text(p).ok().map(|t| date.replace(&t, "tested 2026-11-18 ").into_owned());
        let mut me = engine::data_files::jload(&paths.this_pc);
        if let Some(l) = me.get("router_local").cloned() {
            me.insert("router_local".into(), json!(l == engine::node_manager::file_md5(&paths.router_cfg).as_str()));
        }
        let devices = Settings::read(&paths).unwrap().devices;
        let got =
            json!([out, r.calls, text(&paths.router_cfg), text(&paths.pc_cfg), text(&paths.nodes_txt), devices, me]);
        let _ = std::fs::remove_dir_all(&d);
        got
    });
}

// key_console: the macOS Terminal script that adds this PC's key to the router
#[test]
fn key_console() {
    check("key_console", |a| json!(engine::router_actions::key_command(s(a, 0), s(a, 1), s(a, 2))));
}
