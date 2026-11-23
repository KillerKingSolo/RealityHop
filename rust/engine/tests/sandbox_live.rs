//! Live check of following the router's nodes (follow_router) with a real rh-core, real nodes and a stand-in router.
//! For Windows Sandbox only: it starts rh-core on ports 2080 and 9090. Needs RH_SANDBOX_IN, a folder with
//! rh-core.exe, pc-config.json, router-config.json and vault.json. Run with --ignored.

use engine::paths::Paths;
use engine::router::{Error, Ssh};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// A router that answers only the sync read (its config, with nodes another PC pushed); anything else fails
struct Router {
    reply: String,
}

impl Ssh for Router {
    fn run(&mut self, cmd: &str, _: Option<&str>, _: u64) -> Result<String, Error> {
        if cmd.starts_with("md5sum /etc/sing-box/config.json") {
            return Ok(self.reply.clone());
        }
        if cmd.starts_with("rm -f /etc/sing-box/heal.state") {
            return Ok(String::new());
        }
        Err(Error::Failed("router unreachable over ssh".into()))
    }
}

// Stops the test's rh-core when a step ends, also after a failed assert
struct Core(Paths);

impl Drop for Core {
    fn drop(&mut self) {
        engine::pc_tunnel::core_stop(&self.0);
    }
}

fn input() -> PathBuf {
    PathBuf::from(std::env::var_os("RH_SANDBOX_IN").expect("RH_SANDBOX_IN is not set"))
}

fn read(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(input().join(name)).unwrap()).unwrap()
}

fn servers(cfg: &Value) -> Vec<String> {
    cfg["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["type"] == "vless")
        .map(|o| o["server"].as_str().unwrap().to_string())
        .collect()
}

// A data folder with this PC's nodes and rh-core; nothing synced from the router yet
fn data(name: &str, min_mbps: i64) -> Paths {
    let d = std::env::temp_dir().join(format!("rh-live-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    let paths = Paths::new(&d);
    std::fs::create_dir_all(paths.core.parent().unwrap()).unwrap();
    std::fs::create_dir_all(paths.router_cfg.parent().unwrap()).unwrap();
    std::fs::copy(input().join("rh-core.exe"), &paths.core).unwrap();
    std::fs::copy(input().join("router-config.json"), &paths.router_cfg).unwrap();
    // the tunnel logs into this folder, not the install's
    let mut pc = read("pc-config.json");
    pc["log"]["output"] = json!(paths.logs.join("core.log").to_string_lossy());
    std::fs::create_dir_all(&paths.logs).unwrap();
    engine::data_files::jsave(&paths.pc_cfg, &pc, Some(2)).unwrap();
    std::fs::copy(input().join("vault.json"), &paths.vault).unwrap();
    engine::data_files::jsave(&paths.settings, &json!({"router": 1, "min_mbps": min_mbps}), Some(1)).unwrap();
    paths
}

// The router's config as another PC left it: this PC's nodes, slot 1 swapped for new (None: the same nodes)
fn router_reply(new: Option<&Value>, md5: &str) -> String {
    let mut c = read("router-config.json");
    let pc = read("pc-config.json");
    let own: Vec<Value> =
        pc["outbounds"].as_array().unwrap().iter().filter(|o| o["type"] == "vless").cloned().collect();
    for (o, mine) in c["outbounds"].as_array_mut().unwrap().iter_mut().filter(|o| o["type"] == "vless").zip(&own) {
        *o = mine.clone();
    }
    if let Some(n) = new {
        let o = c["outbounds"].as_array_mut().unwrap().iter_mut().find(|o| o["type"] == "vless").unwrap();
        let tag = o["tag"].as_str().unwrap().to_string();
        *o = engine::singbox_config::outbound19(&engine::router::node_of(n), &tag);
    }
    format!("{md5}\n---\nall\n---\n\n---\n{}", serde_json::to_string(&c).unwrap())
}

// The vault node most likely to work that this PC does not use
fn spare() -> Value {
    let used = servers(&read("pc-config.json"));
    let v = read("vault.json");
    let mut c: Vec<Value> = v
        .as_object()
        .unwrap()
        .values()
        .filter(|n| !used.contains(&n["server"].as_str().unwrap_or_default().to_string()))
        .filter(|n| n["fail_streak"].as_i64().unwrap_or(0) == 0)
        .cloned()
        .collect();
    c.sort_by_key(|n| -n["passes"].as_i64().unwrap_or(0));
    c.into_iter().next().expect("no spare node in the vault")
}

#[test]
#[ignore]
fn follow_router_live() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let l = Arc::clone(&log);
    engine::activity_log::set_say(move |m| {
        println!("  | {m}");
        l.lock().unwrap().push(m.to_string());
    });
    let said = |s: &str| log.lock().unwrap().iter().any(|m| m.contains(s));
    let new = spare();
    let ns = new["server"].as_str().unwrap().to_string();
    let saved = |p: &Paths| servers(&engine::data_files::read_json(&p.pc_cfg).unwrap()).contains(&ns);
    println!("spare node from the vault: slot vless-1 gets a new node");

    println!("== 1: the router has the same nodes: followed quietly");
    let p = data("same", 100);
    let core = Core(p.clone());
    let out = engine::watcher::follow_router(&p, &mut Router { reply: router_reply(None, &"a".repeat(32)) }).unwrap();
    assert_eq!(out, None);
    drop(core);

    println!("== 2: a new node, the PC tunnel not running: it switches at once, no download test");
    log.lock().unwrap().clear();
    let p = data("down", 100);
    let core = Core(p.clone());
    let out = engine::watcher::follow_router(&p, &mut Router { reply: router_reply(Some(&new), &"b".repeat(32)) });
    let out = out.unwrap().expect("a heal sentence");
    assert!(out.starts_with("Another PC changed the router's nodes"), "{out}");
    assert!(said("PC tunnel restarted"), "the tunnel did not take the new set");
    assert!(!said("speed:"), "a download test ran");
    assert!(saved(&p));
    drop(core);

    let p = data("live", 100);
    let core = Core(p.clone());
    assert!(engine::pc_tunnel::core_restart(&p), "the PC tunnel did not start");
    // the Sandbox run puts a stand-in piactl in place that says PIA is connected
    if std::path::Path::new(engine::pia_split_tunnel::PIACTL).exists() {
        println!("== 3: PIA connected through the running PC tunnel: the new set waits, no restart, no download test");
        log.lock().unwrap().clear();
        let out = engine::watcher::follow_router(&p, &mut Router { reply: router_reply(Some(&new), &"c".repeat(32)) });
        assert!(out.unwrap().is_some_and(|o| o.starts_with("Another PC changed the router's nodes")));
        assert!(said("PIA is connected through the PC tunnel"));
        assert!(!said("PC tunnel restarted"), "the tunnel restarted while PIA was connected");
        assert!(!said("speed:"), "a download test ran");
        assert!(saved(&p), "the new set is not saved");
        assert_eq!(engine::node_manager::pending_deploy(&p), Some(true), "the new set is not waiting");
        assert!(engine::pc_tunnel::core_restart(&p));
        assert_eq!(engine::node_manager::pending_deploy(&p), Some(false), "the next restart did not take the set");
    } else {
        println!("== 3 skipped: no piactl");
    }

    println!("== 4: the speed check waits while traffic goes through the PC tunnel");
    use engine::watcher::{IDLE_KBPS, speed_check_go};
    let quiet = engine::clash_api::tunnel_rate(&p).expect("the control API gave no byte totals");
    println!("quiet tunnel: {quiet:.1} KB/s");
    assert!(quiet < IDLE_KBPS && speed_check_go(Some(quiet), 0.0), "a quiet tunnel counts as busy");
    let curl = engine::process::system_tool("curl");
    let mut dl = std::process::Command::new(curl)
        .args(["-s", "--ssl-no-revoke", "--socks5-hostname", "127.0.0.1:2080", "-o", "nul", "-m", "20"])
        .arg(engine::node_tests::SPEED_URL)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let busy = engine::clash_api::tunnel_rate(&p).expect("the control API gave no byte totals");
    let _ = dl.kill();
    let _ = dl.wait();
    println!("download through the tunnel: {busy:.1} KB/s");
    assert!(busy > IDLE_KBPS && !speed_check_go(Some(busy), 0.0), "a download counts as quiet");

    println!("== 5: the node in use at 3 PIA drops in an hour is replaced at once and kept out");
    log.lock().unwrap().clear();
    let bad = engine::clash_api::tunnel_node(&p).expect("the tunnel names no node in use");
    let w = engine::watcher::Watch::new();
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    for k in 0..3 {
        w.blame(&bad, t - 600.0 * f64::from(k));
    }
    assert_eq!(w.breaker(t).as_deref(), Some(bad.as_str()));
    let out = engine::watcher::break_fix(&p, &mut Router { reply: String::new() }, &bad).unwrap();
    println!("result: {out:?}");
    assert!(out.is_some_and(|o| o.contains("broke PIA's connection 3 times in an hour")));
    assert!(said("PC tunnel restarted"), "a real failure waits for nothing");
    let now_in = servers(&engine::data_files::read_json(&p.pc_cfg).unwrap());
    assert!(!now_in.contains(&bad), "the node is still in use");
    assert!(engine::clash_api::tunnel_fails(&p, t).contains_key(&bad), "the node can come back as a spare");
    drop(core);
}
