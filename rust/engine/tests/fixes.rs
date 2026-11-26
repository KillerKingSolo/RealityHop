//! The 1.7.x bugs the Rust version fixes, one test each. tests/parity.rs also checks that these cases now differ
//! from Python's output.

use engine::problem_report::{Identity, Scrubber};
use engine::router::{self, Error, Ssh};
use engine::settings::Settings;
use engine::{node_feed, validate};
use serde_json::{Value, json};

// A scrubber for a made-up person on Windows
fn scrubber() -> Scrubber {
    Scrubber::new(&Identity {
        secrets: vec![],
        nodes: Default::default(),
        home: r"C:\Users\patricia",
        user: Some("patricia"),
        computer: Some("DESKTOP-7Q2"),
        domain: None,
        sep: '\\',
    })
}

#[test]
fn report_hides_keys_that_end_in_dash_or_underscore() {
    let mut sc = scrubber();
    let dash = format!("{}c_-", "Ab".repeat(20));
    let under = format!("-{}x_", "Ab".repeat(20));
    assert_eq!(sc.scrub(&format!("key {dash} end")), "key <key> end");
    assert_eq!(sc.scrub(&format!("pbk={under}")), "pbk=<key>");
    let longer = "Z".repeat(44); // not hex, so the 32-64 hex secret rule does not apply either
    assert_eq!(sc.scrub(&longer), longer, "a 44-character word is no key");
}

#[test]
fn report_hides_the_whole_profile_folder_name() {
    let mut sc = scrubber();
    assert_eq!(sc.scrub(r"D:\Users\Jo Smith\AppData\x.txt"), r"D:\Users\<user>\AppData\x.txt");
    assert_eq!(sc.scrub("/Users/Jo Smith/Library/x"), "/Users/<user>/Library/x");
    assert_eq!(sc.scrub(r"C:\Users\sam then C:\Windows\x"), r"C:\Users\<user> then C:\Windows\x");
    assert_eq!(sc.scrub("/Users/Shared/x"), "/Users/Shared/x");
}

/// Answers every command with one fixed reply.
struct Reply(&'static str);

impl Ssh for Reply {
    // The fixed reply
    fn run(&mut self, _: &str, _: Option<&str>, _: u64) -> Result<String, Error> {
        Ok(self.0.to_string())
    }
}

#[test]
fn router_test_reads_a_decimal_comma_and_skips_garbled_lines() {
    let node =
        |s: &str| json!({"server": s, "uuid": "u", "pbk": "p", "sid": "", "sni": "www.apple.com", "fp": "chrome"});
    let mut nodes = vec![node("1.1.1.1"), node("2.2.2.2")];
    let mut r =
        Reply("nt-0 PASS 1.1.1.1 0,25\nnt-x PASS 9.9.9.9 1\nnt-7 PASS 9.9.9.9 1\nnt-1 PASS 2.2.2.2 abc\n---LOG---\n");
    router::handshake(&mut r, &mut nodes).unwrap();
    assert_eq!((&nodes[0]["ok"], &nodes[0]["ms"], &nodes[0]["exit"]), (&json!(true), &json!(250), &json!("1.1.1.1")));
    assert_eq!((&nodes[1]["ok"], &nodes[1]["ms"], &nodes[1]["exit"]), (&json!(true), &Value::Null, &json!("-")));
}

#[test]
fn multicast_is_no_node_address() {
    for ip in ["224.0.0.1", "239.255.255.250", "233.1.2.3"] {
        assert!(!validate::public_ip(ip), "{ip}");
    }
    assert!(validate::public_ip("8.8.8.8"));
    let line = "vless://a1b2c3d4-e5f6-7890-abcd-ef1234567890@224.0.0.1:443?security=reality&flow=xtls-rprx-vision&pbk=AbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAbAbc_-&sni=www.apple.com";
    assert!(node_feed::parse(line).is_empty());
}

#[test]
fn null_text_settings_keep_their_default() {
    let raw = json!({"router_host": null, "router_key": null, "singbox": null, "dns": null, "devices": null});
    let (s, _) = Settings::load(raw.as_object().unwrap(), false);
    let d = Settings::default();
    assert_eq!(
        (s.router_host, s.router_key, s.singbox, s.dns, s.devices),
        (d.router_host, d.router_key, d.singbox, d.dns, d.devices)
    );
}

#[test]
fn test_addresses_left_on_pias_bypass_list_are_taken_off_again() {
    use engine::pia_split_tunnel::{bypass_plan, owned_ips};
    let r = |ip: &str| json!({"subnet": format!("{ip}/32"), "mode": "exclude"});
    // the user's own DNS, a test address that was left behind, and the node in use
    let cur = vec![r("9.9.9.9"), r("1.2.3.4"), r("5.6.7.8")];
    let known: std::collections::HashSet<String> = ["1.2.3.4", "5.6.7.8"].iter().map(|s| s.to_string()).collect();
    let owned = owned_ips(&cur, &[], &known);
    assert_eq!(owned, ["1.2.3.4", "5.6.7.8"], "this-pc.json had forgotten them");
    let st = json!({"bypassSubnets": cur, "splitTunnelRules": []}).as_object().cloned().unwrap();
    let me = json!({"pia_ips": owned}).as_object().cloned().unwrap();
    let plan = bypass_plan(&st, &me, true, &["5.6.7.8".to_string()], &[], &[], true);
    let left: Vec<&str> =
        plan.change["bypassSubnets"].as_array().unwrap().iter().map(|r| r["subnet"].as_str().unwrap()).collect();
    assert_eq!(left, ["9.9.9.9/32", "5.6.7.8/32"], "the user's rule stays, the leftover goes");
    assert_eq!(plan.ips, ["5.6.7.8"]);
}

// A node that passes the quick test but cuts a lasting download fails the scan; an HTTP error says nothing about it
#[test]
fn a_node_that_cuts_a_lasting_connection_fails() {
    use engine::node_tests::{hold_out, hold_result};
    assert_eq!(hold_result(" 200 640000 10.002"), Some(None), "ran the full 10 s");
    assert_eq!(hold_result(" 200 98000 2.8"), Some(Some(2.8)), "cut after 2.8 s");
    assert_eq!(hold_result(" 000 0 1.0"), Some(Some(1.0)));
    assert_eq!(hold_result(" 503 0 0.4"), None);
    let mut nodes = vec![json!({"ok": true}), json!({"ok": true}), json!({"ok": false}), json!({"ok": true})];
    hold_out(&mut nodes, |i| Ok([Some(None), Some(Some(3.0)), Some(Some(1.0)), None][i])).unwrap();
    assert_eq!(nodes[0]["ok"], true);
    assert_eq!(nodes[1]["ok"], false);
    assert_eq!(nodes[1]["why"], "cut a lasting connection after 3 s");
    assert!(nodes[2].get("why").is_none(), "a node that already failed is not tested");
    assert_eq!(nodes[3]["ok"], true);
}

// A node under the hard minimum fails after a test of its own; 0 Mbps fails at once
#[test]
fn a_node_under_the_hard_minimum_fails_after_its_own_test() {
    use engine::node_tests::slow_out;
    let mut nodes = vec![
        json!({"ok": true, "mbps": 30}),
        json!({"ok": true, "mbps": 1}),
        json!({"ok": true, "mbps": 2}),
        json!({"ok": true, "mbps": 0}),
        json!({"ok": true}),
    ];
    let asked = std::sync::Mutex::new(vec![]);
    slow_out(&mut nodes, 5, |i| {
        asked.lock().unwrap().push(i);
        Ok(Some(if i == 2 { 9 } else { 1 }))
    })
    .unwrap();
    let mut asked = asked.into_inner().unwrap();
    asked.sort();
    assert_eq!(asked, vec![1, 2], "only the slow ones that moved some data are tested again");
    assert_eq!(nodes[0]["ok"], true);
    assert_eq!(nodes[1]["ok"], false);
    assert_eq!(nodes[1]["why"], "too slow: 1 Mbps (needs at least 5)");
    assert_eq!((nodes[2]["ok"].clone(), nodes[2]["mbps"].clone()), (json!(true), json!(9)));
    assert_eq!(nodes[3]["ok"], false);
    assert_eq!(nodes[4]["ok"], true, "untested speed keeps the node");
}

// Older PC configs get the 1-minute re-test and the info log; values changed by hand stay
#[test]
fn older_pc_configs_retest_every_minute_and_log_each_connection() {
    use engine::singbox_config::pc_tune;
    let old = json!({"log": {"level": "warn"}, "outbounds": [{"type": "urltest", "tag": "auto", "interval": "10m"}],
        "route": {"auto_detect_interface": true}});
    let mut c = old.clone();
    assert!(pc_tune(&mut c, false));
    assert_eq!((c["log"]["level"].clone(), c["outbounds"][0]["interval"].clone()), (json!("info"), json!("1m")));
    assert_eq!(c["route"]["auto_detect_interface"], true, "Windows keeps the interface watch");
    assert!(!pc_tune(&mut c, false), "already up to date");
    // a Mac keeps the interface watch, and one a test build turned off is turned back on
    let mut m = old.clone();
    assert!(pc_tune(&mut m, true));
    assert_eq!(m["route"]["auto_detect_interface"], true);
    let mut off = json!({"route": {"auto_detect_interface": false}});
    assert!(pc_tune(&mut off, true));
    assert_eq!(off["route"]["auto_detect_interface"], true);
    assert!(!pc_tune(&mut off, true));
    let mut own =
        json!({"log": {"level": "debug"}, "outbounds": [{"type": "urltest", "tag": "auto", "interval": "3m"}]});
    assert!(!pc_tune(&mut own, true));
    let t: serde_json::Value = serde_json::from_str(engine::setup_steps::PC_TEMPLATE).unwrap();
    assert_eq!((t["log"]["level"].clone(), t["outbounds"][0]["interval"].clone()), (json!("info"), json!("1m")));
}

// PIA's connection is the oldest one through a node; a hold goes to the best node that passed this test
#[test]
fn the_node_carrying_pia_is_the_oldest_connection_and_the_hold_takes_a_fresh_pass() {
    use engine::clash_api::{best_pass, carrier_of};
    let conns = json!({"connections": [
        {"start": "2026-11-25T11:09:22.5-08:00", "chains": ["vless-3", "auto", "pick"]},
        {"start": "2026-11-25T11:09:20.12-08:00", "chains": ["vless-2", "auto", "pick"]},
        {"start": "2026-11-25T11:09:19-08:00", "chains": ["direct"]},
    ]});
    assert_eq!(carrier_of(&conns).as_deref(), Some("vless-2"));
    assert_eq!(carrier_of(&json!({"connections": []})), None);
    let passes = json!({"vless-1": 120, "vless-2": 90, "vless-4": 300}).as_object().cloned().unwrap();
    assert_eq!(best_pass(&passes, "vless-2").as_deref(), Some("vless-1"));
    assert_eq!(best_pass(&Default::default(), "vless-2"), None);
}

// The 443 ping waits for the server's reply to a TLS hello: a proxy that only accepts the connection does not count
#[test]
fn the_443_ping_needs_a_reply_not_just_an_accepted_connection() {
    use engine::node_tests::{client_hello, hello_ms_on};
    use std::io::{Read, Write};
    use std::time::Duration;
    let h = client_hello("www.example.com", 7);
    assert_eq!(&h[..3], &[0x16, 3, 1]);
    assert_eq!(u16::from_be_bytes([h[3], h[4]]) as usize, h.len() - 5, "record length");
    assert_eq!(h[5], 1, "ClientHello");
    assert!(h.windows(15).any(|w| w == b"www.example.com"), "the SNI is in it");
    // accepts (the OS queues the connection) but never answers
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let sp = silent.local_addr().unwrap().port();
    assert_eq!(hello_ms_on("127.0.0.1", sp, "x", Duration::from_millis(400)), None);
    let answers = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ap = answers.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = answers.accept() {
            let mut b = [0u8; 5];
            let _ = s.read(&mut b);
            let _ = s.write_all(&[0x15]);
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    assert!(hello_ms_on("127.0.0.1", ap, "x", Duration::from_secs(2)).is_some_and(|ms| (0..2000).contains(&ms)));
    drop(silent);
}

// A failure that can pass later keeps a node out of the tests for an hour; a wrong certificate keeps it out for good
#[test]
fn old_scan_failures_expire_except_wrong_certificates() {
    use engine::node_tables::{FAIL_HOLD_SECS, known_failed_fresh};
    let txt = "IN USE\n\nFAILED  (do not retest until the feed changes their key)\n  \
        1.1.1.1          www.bing.com         certificate is valid for x.ru\n  \
        2.2.2.2          www.apple.com        no-response\n  \
        3.3.3.3          www.amd.com          cut a lasting connection after 10 s\n";
    assert_eq!(known_failed_fresh(txt, 60), vec!["1.1.1.1", "2.2.2.2", "3.3.3.3"]);
    assert_eq!(known_failed_fresh(txt, FAIL_HOLD_SECS), vec!["1.1.1.1"]);
}

// Working nodes are counted out of all 6 slots: 5 nodes that all work read "5 of 6", with a dead ring segment
#[test]
fn working_nodes_are_counted_out_of_six_slots() {
    use engine::tray_state::tray_mark;
    let five: Vec<(String, Option<f64>)> = (1..=5).map(|i| (format!("vless-{i}"), Some(100.0))).collect();
    let m = tray_mark(&five, None, 200.0);
    assert_eq!((m.working, m.total, m.state), (5, 6, "warn"));
    assert_eq!(m.segs, vec!["ok", "ok", "ok", "ok", "ok", "bad"]);
    let six: Vec<(String, Option<f64>)> = (1..=6).map(|i| (format!("vless-{i}"), Some(100.0))).collect();
    assert_eq!(tray_mark(&six, None, 200.0).state, "ok");
}

#[test]
fn the_tunnel_picks_among_fast_nodes_and_keeps_slow_ones_as_backup() {
    use engine::singbox_config::{PICK, pick_add, write_configs};
    let node = |s: &str| engine::node::Node {
        server: s.into(),
        uuid: "u".into(),
        pbk: "p".into(),
        sid: "".into(),
        sni: "www.apple.com".into(),
        fp: "chrome".into(),
    };
    let group = json!({"type": "urltest", "tag": "auto", "outbounds": []});
    let mut pc = json!({"outbounds": [group.clone()], "route": {"final": "auto"}});
    pick_add(&mut pc);
    let router = json!({"outbounds": [group]});
    let set = [node("1.1.1.1"), node("2.2.2.2"), node("3.3.3.3")];
    let of =
        |c: &Value, tag: &str| c["outbounds"].as_array().unwrap().iter().find(|o| o["tag"] == tag).unwrap().clone();
    // one fast node: the PC group holds it alone; the router keeps all three, since it has no backup
    let (p, r) = write_configs(&pc, &router, &set, &[false, true, false]);
    assert_eq!(of(&p, "auto")["outbounds"], json!(["vless-2"]));
    assert_eq!(of(&p, PICK)["outbounds"], json!(["auto", "vless-1", "vless-2", "vless-3"]));
    assert_eq!(of(&r, "auto")["outbounds"], json!(["vless-1", "vless-2", "vless-3"]));
    // two fast: the router group drops the slow one too
    let (_, r) = write_configs(&pc, &router, &set, &[true, false, true]);
    assert_eq!(of(&r, "auto")["outbounds"], json!(["vless-1", "vless-3"]));
    // none fast: every node stays in the group
    let (p, _) = write_configs(&pc, &router, &set, &[false, false, false]);
    assert_eq!(of(&p, "auto")["outbounds"], json!(["vless-1", "vless-2", "vless-3"]));
}

#[test]
fn a_router_pin_shows_as_the_node_in_use() {
    let pinned = r#"{"type":"Selector","now":"vless-4"}
{"type":"URLTest","now":"vless-2"}"#;
    assert_eq!(router::router_now(&mut Reply(pinned), "s"), Some(json!("vless-4")));
    let auto = r#"{"type":"Selector","now":"auto"}
{"type":"URLTest","now":"vless-2"}"#;
    assert_eq!(router::router_now(&mut Reply(auto), "s"), Some(json!("vless-2")));
    // a router config from before the switch: the selector is not found, the group answers
    let old = r#"{"message":"Resource not found"}
{"type":"URLTest","now":"vless-3"}"#;
    assert_eq!(router::router_now(&mut Reply(old), "s"), Some(json!("vless-3")));
}

#[test]
fn a_pin_survives_a_pia_drop_with_no_network() {
    use engine::watcher::pin_judge;
    use std::time::Duration;
    let z = Duration::ZERO;
    // no node answers in any try (PIA reconnecting): the pin stays
    assert_eq!(pin_judge(3, z, || false, || false), None);
    // the network comes back on the third try and the pinned node passes
    let mut n = 0;
    assert_eq!(
        pin_judge(
            6,
            z,
            || {
                n += 1;
                n >= 3
            },
            || false
        ),
        Some(true)
    );
    // the pinned node fails while another node passes: the pin ends
    assert_eq!(pin_judge(6, z, || false, || true), Some(false));
}

#[test]
fn the_node_list_comes_from_the_outputs_branch() {
    assert!(node_feed::FEEDS[0].contains("/FastNodes/outputs/"));
    let m = node_feed::mirrors(node_feed::FEEDS[0]);
    assert_eq!(m[0], "https://cdn.jsdelivr.net/gh/rtwo2/FastNodes@outputs/sub/verified_tls.txt");
    assert_eq!(m[1], "https://fastly.jsdelivr.net/gh/rtwo2/FastNodes@outputs/sub/verified_tls.txt");
}

#[test]
fn cloudflare_moves_to_its_malware_blocking_servers() {
    use engine::singbox_config::dns_apply;
    let mut c = json!({"dns": {"final": "cloudflare", "servers": [{"tag": "cloudflare", "type": "https", "server": "1.1.1.1"}]},
        "inbounds": [{"override_address": "1.1.1.1"}]});
    assert!(dns_apply(&mut c, "cloudflare"));
    assert_eq!(c["dns"]["servers"][0]["server"], "1.1.1.2");
    assert_eq!(c["inbounds"][0]["override_address"], "1.1.1.2");
    assert!(!dns_apply(&mut c, "cloudflare")); // done once
    let mut r = json!({"dns": {"final": "cloudflare", "servers": [{"tag": "cloudflare", "address": "https://1.1.1.1/dns-query"}]}});
    assert!(dns_apply(&mut r, "cloudflare"));
    assert_eq!(r["dns"]["servers"][0]["address"], "https://1.1.1.2/dns-query");
}

#[test]
fn failures_during_a_pia_drop_do_not_count_except_wrong_certificates_and_keys() {
    use engine::node_tests::{PIA_SKIP, forgive_pia_drop};
    use serde_json::json;
    let fresh = || {
        vec![
            json!({"server": "1.1.1.1", "ok": false, "why": "no-response"}),
            json!({"server": "2.2.2.2", "ok": false, "why": "certificate is valid for example.com"}),
            json!({"server": "3.3.3.3", "ok": false, "why": "reality verification failed"}),
            json!({"server": "4.4.4.4", "ok": true, "ms": 300}),
        ]
    };
    // PIA dropped 30 s into a 60 s test: the no-response is not counted
    let mut n = fresh();
    forgive_pia_drop(&mut n, 1000.0, 1060.0, 30.0);
    assert!(n[0].get("ok").is_none());
    assert_eq!(n[0]["why"], PIA_SKIP);
    assert_eq!((&n[1]["ok"], &n[2]["ok"], &n[3]["ok"]), (&json!(false), &json!(false), &json!(true)));
    let txt = engine::node_tables::write_txt(&n, &[], "today");
    assert!(!txt.contains("1.1.1.1") && txt.contains("2.2.2.2"));
    // PIA steady for the whole test: every failure counts
    let mut n = fresh();
    forgive_pia_drop(&mut n, 1000.0, 1060.0, f64::INFINITY);
    assert_eq!(n[0]["ok"], json!(false));
}
