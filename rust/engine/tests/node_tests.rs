//! The node tests on this PC: connect times, free test ports, the rh-test copy, and local_test's refusals. A live run
//! of local_test needs a real sing-box: set RH_TEST_CORE to one and run with --ignored.

use engine::node_tests::{connect_ms, free_base, local_test, test_exe};
use engine::paths::{EXE, Paths};
use serde_json::{Value, json};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

// An empty data folder of this test's own
fn data(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-node-tests-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn connect_time_is_measured_and_a_closed_port_gives_none() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(connect_ms("127.0.0.1", port, Duration::from_secs(2)).is_some_and(|ms| (0..2000).contains(&ms)));
    drop(l);
    assert_eq!(connect_ms("127.0.0.1", port, Duration::from_secs(2)), None);
    assert_eq!(connect_ms("no such host.invalid", 443, Duration::from_secs(2)), None);
}

#[test]
fn test_ports_skip_a_range_with_a_port_in_use() {
    let base = free_base(3).unwrap();
    assert!((23000..33000).contains(&base) && (base - 23000).is_multiple_of(500));
    let held = TcpListener::bind(("127.0.0.1", base + 1)).unwrap();
    let next = free_base(3).unwrap();
    assert_ne!(next, base, "a range with a port in use was handed out");
    drop(held);
}

#[test]
fn the_test_copy_follows_rh_core() {
    let d = data("copy");
    let paths = Paths::new(&d);
    assert_eq!(test_exe(&paths), paths.core, "no rh-core: rh-core itself (which local_test refuses)");
    std::fs::create_dir_all(&paths.pc_dir).unwrap();
    std::fs::write(&paths.core, b"first").unwrap();
    let t = test_exe(&paths);
    assert_eq!(t, d.join(format!("rh-test{EXE}")));
    assert_eq!(std::fs::read(&t).unwrap(), b"first");
    std::fs::write(&paths.core, b"second build").unwrap();
    assert_eq!(std::fs::read(test_exe(&paths)).unwrap(), b"second build", "a changed rh-core is copied again");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_test_copy_is_a_link_not_a_second_file() {
    use std::io::Write;
    let d = data("link");
    let paths = Paths::new(&d);
    std::fs::create_dir_all(&paths.pc_dir).unwrap();
    std::fs::write(&paths.core, b"build").unwrap();
    // a copy from before: current, but a file of its own
    let t = d.join(format!("rh-test{EXE}"));
    std::fs::write(&t, b"build").unwrap();
    assert_eq!(test_exe(&paths), t);
    // one file on disk: a change to rh-core shows in rh-test at once
    std::fs::OpenOptions::new().append(true).open(&paths.core).unwrap().write_all(b" 2").unwrap();
    assert_eq!(std::fs::read(&t).unwrap(), b"build 2", "rh-test is not a link of rh-core");
    assert_eq!(test_exe(&paths), t, "the link stays");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn local_test_refuses_without_rh_core_and_passes_no_nodes() {
    let d = data("refuse");
    let paths = Paths::new(&d);
    let mut nodes = vec![json!({"server": "192.0.2.1"})];
    let e = local_test(&paths, &mut nodes, false).unwrap_err();
    assert_eq!(e, format!("rh-core{EXE} is missing; run the setup again (Settings > Run setup again)"));
    std::fs::create_dir_all(&paths.pc_dir).unwrap();
    std::fs::write(&paths.core, b"x").unwrap();
    local_test(&paths, &mut [], false).unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_empty_vault_stays_unwritten() {
    let d = data("vault");
    let paths = Paths::new(&d);
    assert!(engine::vault::load(&paths).is_empty());
    assert!(!paths.vault.exists());
    let _ = std::fs::remove_dir_all(&d);
}

// Two made-up nodes in TEST-NET-1 (never routed): the instance starts, both fail, and the test cleans up
#[test]
#[ignore = "needs a real sing-box in RH_TEST_CORE"]
fn live_local_test_with_unreachable_nodes() {
    let core = PathBuf::from(std::env::var_os("RH_TEST_CORE").expect("set RH_TEST_CORE"));
    let d = data("live");
    let paths = Paths::new(&d);
    std::fs::create_dir_all(&paths.pc_dir).unwrap();
    std::fs::copy(&core, &paths.core).unwrap();
    let node = |ip: &str| {
        json!({"server": ip, "uuid": "00000000-0000-4000-8000-000000000000",
            "pbk": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "sid": "", "sni": "www.apple.com", "fp": "chrome"})
    };
    let mut nodes = vec![node("192.0.2.1"), node("192.0.2.2")];
    let t = std::time::Instant::now();
    local_test(&paths, &mut nodes, false).unwrap();
    println!("{:.1} s: {}", t.elapsed().as_secs_f64(), Value::Array(nodes.clone()));
    for n in &nodes {
        assert_eq!(
            (&n["ok"], &n["exit"], &n["ms"], &n["where"]),
            (&json!(false), &json!("-"), &Value::Null, &json!("?"))
        );
        assert!(n["why"].is_string());
    }
    let left: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(&format!("rh-test-{}-", std::process::id())))
        .collect();
    assert!(left.is_empty(), "the test folder was left behind");
    let _ = std::fs::remove_dir_all(&d);
}
