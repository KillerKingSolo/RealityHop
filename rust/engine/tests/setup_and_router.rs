//! The setup's file steps in a sandbox data folder (no download: a stand-in rh-core is put there first), the random
//! API secret, the router key made with the system's ssh-keygen, and its removal from a fake router.

use engine::paths::Paths;
use engine::router::{Error, Ssh};
use serde_json::Value;
use std::path::PathBuf;

// An empty data folder of this test's own
fn data(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-setup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn secrets_are_random_hex() {
    let (a, b) = (engine::setup_steps::token_hex(16).unwrap(), engine::setup_steps::token_hex(16).unwrap());
    assert_eq!(a.len(), 32);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(a, b);
}

#[test]
fn prepare_makes_the_configs_and_keeps_what_exists() {
    let d = data("prepare");
    let paths = Paths::new(&d);
    std::fs::create_dir_all(&paths.pc_dir).unwrap();
    std::fs::write(&paths.core, b"not a program").unwrap(); // no download; its config check fails, so nothing changes
    engine::setup_steps::prepare(&paths).unwrap();
    let pc = engine::data_files::read_json(&paths.pc_cfg).unwrap();
    let r = engine::data_files::read_json(&paths.router_cfg).unwrap();
    let secret = |c: &Value| c["experimental"]["clash_api"]["secret"].as_str().unwrap().to_string();
    assert_eq!(secret(&pc).len(), 32);
    assert_ne!(secret(&pc), secret(&r), "each config has its own secret");
    assert_eq!(pc["log"]["output"], paths.core_log.to_string_lossy().as_ref());
    assert!(paths.logs.is_dir() && paths.router_dir.is_dir());
    let readme = std::fs::read(paths.pc_dir.join("README.txt")).unwrap();
    assert_eq!(readme.windows(2).any(|w| w == b"\r\n"), cfg!(windows), "README in the platform's line endings");
    assert!(String::from_utf8_lossy(&readme).contains(r"The logs are in ..\logs."));
    let before = std::fs::read(&paths.pc_cfg).unwrap();
    engine::setup_steps::prepare(&paths).unwrap();
    assert_eq!(std::fs::read(&paths.pc_cfg).unwrap(), before, "an existing config stays as it is");
    let leftover = std::fs::read_dir(&paths.pc_dir)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".check.json"));
    assert_eq!(leftover.count(), 0, "the config check's copy is removed");
    let _ = std::fs::remove_dir_all(&d);
}

/// Records each command; answers with fixed text.
struct Fake(Vec<String>);

impl Ssh for Fake {
    fn run(&mut self, cmd: &str, _: Option<&str>, _: u64) -> Result<String, Error> {
        self.0.push(cmd.to_string());
        Ok("router: the key of this PC is removed\n".into())
    }
}

// Whether the system's ssh-keygen can make a key here (Windows Sandbox has none that runs)
fn keygen_works(d: &std::path::Path) -> bool {
    std::fs::create_dir_all(d).unwrap();
    let probe = d.join("probe");
    let ok = std::process::Command::new(engine::process::system_tool("ssh-keygen"))
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&probe)
        .output()
        .is_ok_and(|o| o.status.success());
    let _ = std::fs::remove_dir_all(d);
    ok
}

#[test]
fn the_router_key_is_made_once_and_removed_by_its_own_text() {
    let d = data("key");
    if !keygen_works(&d.with_extension("probe")) {
        eprintln!("skipped: no working ssh-keygen on this machine");
        return;
    }
    let paths = Paths::new(&d);
    let public = engine::router_actions::make_key(&paths).unwrap();
    assert!(public.starts_with("ssh-ed25519 ") && public.ends_with(" realityhop"), "{public}");
    assert_eq!(engine::router_actions::make_key(&paths).unwrap(), public, "an existing key is kept");
    engine::data_files::jsave(&paths.settings, &serde_json::json!({"router": 1}), Some(1)).unwrap();
    let mut f = Fake(vec![]);
    let out = engine::router_actions::router_key_remove(&paths, &mut f).unwrap();
    assert_eq!(out, "router: the key of this PC is removed");
    let k = public.split_whitespace().nth(1).unwrap();
    assert_eq!(
        f.0,
        [format!(r"sed -i '\|{k}|d' /etc/dropbear/authorized_keys; echo 'router: the key of this PC is removed'")]
    );
    engine::data_files::jsave(&paths.settings, &serde_json::json!({"router": 1, "router_key": "C:/my.key"}), Some(1))
        .unwrap();
    let out = engine::router_actions::router_key_remove(&paths, &mut f).unwrap();
    assert_eq!(out, "router: your own key file was used, so it stays on the router");
    assert_eq!(f.0.len(), 1, "a key file of the person's own is never touched");
    let _ = std::fs::remove_dir_all(&d);
}

/// Router features are off in the report test: any router call is a bug.
struct Unused;

impl Ssh for Unused {
    fn run(&mut self, _: &str, _: Option<&str>, _: u64) -> Result<String, Error> {
        panic!("the report asked the router with router features off");
    }
}

#[test]
fn a_problem_report_takes_out_personal_data_and_keeps_five() {
    let d = data("report");
    let paths = Paths::new(&d);
    std::fs::create_dir_all(&paths.logs).unwrap();
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let log = format!(
        "2026-11-18 10:00:00  PC check: 5 of 6 nodes working; dead: vless-3\n2026-11-18 10:00:01  rh-core: could not start from C:{0}Users{0}{user}{0}x\n",
        std::path::MAIN_SEPARATOR
    );
    std::fs::write(paths.logs.join("watch.log"), log).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let h = serde_json::json!({"pings": {}, "events": [
        [now - 60, "drop", "PIA reconnecting"],
        [now - 50, "replace", "Replaced vless-2 (45.144.112.55 -> 169.40.42.89) (under 100 Mbps)."],
    ]});
    engine::data_files::jsave(&paths.history, &h, None).unwrap();
    for _ in 0..6 {
        engine::problem_report_file::make(&paths, &mut Unused).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100)); // one report per second: the name has the time
    }
    let reports = std::fs::read_dir(&paths.reports).unwrap().count();
    assert_eq!(reports, 5, "the oldest report is removed");
    let (_, text) = engine::problem_report_file::make(&paths, &mut Unused).unwrap();
    assert!(text.starts_with("Reality Hop problem report\n"));
    assert!(text.contains("\nSetup\n-----\n") && text.contains("Router features  off"));
    assert!(text.contains("watch   2026-11-18 10:00:01  rh-core: could not start"), "{text}");
    assert!(text.contains("PC tunnel        rh-core missing"), "{text}");
    assert!(text.contains("2 events: PIA dropped 1, nodes replaced 1"), "{text}");
    assert!(text.contains("replace  Replaced vless-2 (ip-1 -> ip-2)"), "{text}");
    assert!(!text.contains("45.144.112.55") && !text.contains("169.40.42.89"), "an address is left in");
    if user.chars().count() > 2 {
        assert!(!text.contains(&user), "the user name is taken out");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn leftover_files_go_at_the_start_once_ten_minutes_old() {
    let d = data("leftovers");
    std::fs::create_dir_all(&d).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(700);
    for name in ["rh-test.exe", "history.json.1-ThreadId2.tmp", "tray-window.json", "add-router-key.cmd", "nodes.txt"] {
        std::fs::File::create(d.join(name)).unwrap().set_modified(old).unwrap();
    }
    std::fs::write(d.join("last.json.3-ThreadId4.tmp"), b"a write of a running copy").unwrap();
    engine::data_files::leftovers_remove(&d);
    let mut left: Vec<String> =
        std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["last.json.3-ThreadId4.tmp", "nodes.txt"]);
    let _ = std::fs::remove_dir_all(&d);
}
