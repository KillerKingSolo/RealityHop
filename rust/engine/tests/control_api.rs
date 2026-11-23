//! The control API client and check_pc against a fake sing-box API on a free loopback port. Nothing reaches the
//! running tunnel's real API on 9090.

use engine::clash_api::{self, ApiError, ControlApi, PcCheck};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const SERVERS: [&str; 3] = ["203.0.113.1", "203.0.113.2", "203.0.113.3"];
const NOW: f64 = 1_800_000_000.0;
static RETRIED: AtomicBool = AtomicBool::new(false);

// A fresh data folder in the temp directory
fn temp_data(name: &str) -> engine::paths::Paths {
    let d = std::env::temp_dir().join(format!("rh-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    engine::paths::Paths::new(&d)
}

// What the fake API answers: 401 without the secret; vless-0 works, vless-1 times out, vless-2 works on the retry
fn answer(path: &str, auth: &str) -> (u16, String) {
    let delay = |tag: &str| format!("/proxies/{tag}/delay?url=https://www.gstatic.com/generate_204&timeout=8000");
    if auth != "Bearer s3cret" {
        return (401, r#"{"message":"Unauthorized"}"#.into());
    }
    if path == "/proxies" {
        let mark = engine::singbox_config::set_mark(SERVERS);
        return (200, json!({"proxies": {"auto": {"now": "vless-0"}, mark: {"type": "Direct"}}}).to_string());
    }
    if path == delay("vless-0") {
        return (200, r#"{"delay":42}"#.into());
    }
    if path == delay("vless-2") && RETRIED.swap(true, Ordering::SeqCst) {
        return (200, r#"{"delay":77}"#.into());
    }
    (504, r#"{"message":"Timeout"}"#.into())
}

// A fake control API that answers each request with answer() until the test ends
fn fake_api(secret: &str) -> ControlApi {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let (mut b, mut buf) = (Vec::new(), [0u8; 1024]);
                while !b.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => b.extend_from_slice(&buf[..n]),
                    }
                }
                let req = String::from_utf8_lossy(&b).into_owned();
                let path = req.split(' ').nth(1).unwrap_or_default();
                let auth = req.lines().find_map(|l| l.strip_prefix("Authorization: ")).unwrap_or_default();
                let (code, body) = answer(path, auth);
                let _ = write!(s, "HTTP/1.0 {code} X\r\nContent-Type: application/json\r\n\r\n{body}");
            });
        }
    });
    ControlApi { addr, secret: secret.into() }
}

// Nodes as the engine keeps them
fn nodes(servers: &[&str]) -> Vec<Value> {
    servers.iter().enumerate().map(|(i, s)| json!({"server": s, "tag": format!("vless-{i}")})).collect()
}

#[test]
fn wrong_secret_is_an_http_error_with_its_text() {
    let e = fake_api("wrong").get("/proxies", Duration::from_secs(5)).unwrap_err();
    assert!(matches!(e, ApiError::Http(401, ref t) if t.contains("Unauthorized")), "{e:?}");
}

#[test]
fn check_pc_tests_each_node_with_one_retry_and_notes_tunnel_fails() {
    let p = temp_data("check");
    let old = json!({SERVERS[0]: NOW - 60.0, "198.51.100.9": NOW - 13.0 * 3600.0, "198.51.100.8": NOW - 3600.0});
    engine::data_files::jsave(&p.tunnel_fails, &old, None).unwrap();
    let mut ns = nodes(&SERVERS);
    let r = clash_api::check_pc_with(&fake_api("s3cret"), &p, &mut ns, false, NOW).unwrap();
    assert_eq!(r, Some(PcCheck { now: Some(json!("vless-0")), exit: None, stale: false }));
    let got: Vec<_> = ns.iter().map(|n| (n["pc_ms"].clone(), n["pc_why"].clone())).collect();
    assert_eq!(got, [(json!(42), json!("")), (Value::Null, json!("Timeout")), (json!(77), json!(""))]);
    // the working node leaves the list, the failed one joins it, the one older than 12 hours drops out
    let f = engine::data_files::jload(&p.tunnel_fails);
    assert_eq!(Value::Object(f), json!({"198.51.100.8": NOW - 3600.0, SERVERS[1]: NOW}));
}

#[test]
fn an_older_node_set_is_not_tested() {
    let p = temp_data("stale");
    let mut ns = nodes(&["203.0.113.9"]);
    let r = clash_api::check_pc_with(&fake_api("s3cret"), &p, &mut ns, false, NOW).unwrap().unwrap();
    assert!(r.stale && r.now == Some(json!("vless-0")));
    assert_eq!((&ns[0]["pc_ms"], &ns[0]["pc_why"]), (&Value::Null, &Value::Null));
    assert!(!p.tunnel_fails.exists());
}

#[test]
fn api_down_marks_every_node() {
    let p = temp_data("down");
    let mut ns = nodes(&SERVERS);
    assert_eq!(clash_api::check_pc(&p, &mut ns, false, NOW), Ok(None)); // no pc-config.json: no secret
    let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap(); // closed once the listener drops
    let api = ControlApi { addr: free, secret: "s3cret".into() };
    assert_eq!(clash_api::check_pc_with(&api, &p, &mut ns, false, NOW), Ok(None));
    assert!(ns.iter().all(|n| n["pc_ms"].is_null() && n["pc_why"] == "api down"));
}

#[test]
fn all_dead_writes_no_tunnel_fails() {
    let p = temp_data("dead");
    let ns: Vec<Value> = SERVERS.iter().map(|s| json!({"server": s, "pc_ms": null})).collect();
    clash_api::tunnel_fails_note(&p, &ns, NOW);
    assert!(!p.tunnel_fails.exists());
}

#[test]
fn a_node_that_breaks_pia_stays_out_even_when_it_passes() {
    let p = temp_data("breaks");
    clash_api::pia_break_note(&p, "198.51.100.7", NOW - 60.0);
    // it works in the tunnel again: that clears tunnel-fails.json, not the break
    let mut ns = nodes(&["198.51.100.7"]);
    ns[0]["pc_ms"] = json!(50);
    clash_api::tunnel_fails_note(&p, &ns, NOW);
    assert!(clash_api::tunnel_fails(&p, NOW).contains_key("198.51.100.7"));
    assert!(!clash_api::tunnel_fails(&p, NOW + 12.0 * 3600.0).contains_key("198.51.100.7"), "kept past 12 hours");
}
