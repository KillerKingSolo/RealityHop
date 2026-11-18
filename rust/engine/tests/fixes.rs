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
