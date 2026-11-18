//! The SSH transport's arguments and its network gate. The router address is 192.0.2.1 (a documentation address
//! that is never routed), so no test reaches a real router and ssh never runs.

use engine::router::{Error, Ssh};
use engine::router_ssh::{self, RouterSsh};
use engine::settings::Settings;
use std::time::{Duration, Instant};

// A fresh data folder in the temp directory
fn temp_data(name: &str) -> engine::paths::Paths {
    let d = std::env::temp_dir().join(format!("rh-ssh-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    engine::paths::Paths::new(&d)
}

#[test]
fn ssh_arguments() {
    let p = temp_data("args");
    let a: Vec<String> =
        router_ssh::ssh_base(&p, &Settings::default()).iter().map(|s| s.to_string_lossy().into_owned()).collect();
    let known = format!("UserKnownHostsFile=\"{}\"", p.known_hosts.display());
    let rest = [
        "-i",
        &p.router_key.display().to_string(),
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        &known,
        "root@192.168.8.1",
    ];
    assert_eq!(a[1..], rest);
    let own_key = Settings { router_key: "k".into(), ..Settings::default() };
    assert_eq!(router_ssh::ssh_base(&p, &own_key)[2], "k");
}

#[test]
fn away_from_the_router_nothing_is_sent() {
    let p = temp_data("away");
    std::fs::write(&p.settings, r#"{"router_host": "192.0.2.1"}"#).unwrap();
    let why = "not on your router's network: 192.0.2.1 does not answer; router steps skipped";
    assert_eq!(router_ssh::router_here(&p), Err(why.to_string()));
    let start = Instant::now(); // kept 30 seconds: no second wait for the port
    assert_eq!(RouterSsh { paths: &p }.run("true", None, 20), Err(Error::Failed(why.into())));
    assert!(start.elapsed() < Duration::from_millis(500));
}
