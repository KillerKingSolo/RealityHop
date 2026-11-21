//! The watcher's own state: PIA drops and wake-ups, the tray tooltip and tray.json, and a stopped watcher. Nothing
//! here talks to a running tunnel.

use engine::paths::Paths;
use engine::router::{Error, Ssh};
use engine::watcher::{Watch, WatchState, tray_adopt, tray_load, tray_tip, watch_cycle};
use std::path::PathBuf;
use std::time::{Duration, Instant};

// An empty data folder of this test's own
fn data(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-watcher-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_pia_drop_counts_once_and_wakes_the_loop() {
    let d = data("pia");
    let paths = Paths::new(&d);
    let w = Watch::new();
    w.on_pia(&paths, "Connected");
    assert_eq!(w.pia_drops(), 0);
    w.on_pia(&paths, "Reconnecting");
    w.on_pia(&paths, "Connected");
    w.on_pia(&paths, "Connecting");
    assert_eq!(w.pia_drops(), 1, "a second blip within 30 s is the same drop");
    let h = engine::history::load(&paths.history);
    let kinds: Vec<&str> = h["events"].as_array().unwrap().iter().map(|e| e[1].as_str().unwrap()).collect();
    assert_eq!(kinds, ["drop", "back"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_tray_request_wakes_the_loop_and_quit_ends_it() {
    let w = Watch::new();
    let w2 = std::sync::Arc::clone(&w);
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        w2.ask("check");
        std::thread::sleep(Duration::from_millis(200));
        w2.stop();
    });
    let t = Instant::now();
    assert!(w.sleep(30.0), "woken by the request, not stopped");
    assert!(t.elapsed() < Duration::from_secs(5));
    assert!(!w.sleep(30.0), "Quit ends the loop");
    assert!(t.elapsed() < Duration::from_secs(5));
    h.join().unwrap();
    let t = Instant::now();
    assert!(!w.sleep(30.0), "a stopped watch stays stopped");
    assert!(t.elapsed() < Duration::from_secs(1));
}

#[test]
fn the_tooltip_says_what_the_tray_shows() {
    let st =
        WatchState { last: Some(("warn".into(), "5 of 6 nodes working".into(), "12:34".into())), ..Default::default() };
    let (state, tip) = tray_tip(&st, 2, true);
    assert_eq!(state, "warn");
    assert_eq!(tip, "Reality Hop  ·  5 of 6 nodes working  ·  12:34  ·  PIA dropped 2x in the last hour");
    let (state, tip) = tray_tip(&WatchState::default(), 0, false);
    assert_eq!((state.as_str(), tip.as_str()), ("unk", "Reality Hop  ·  Not checked yet  ·  rh-core missing"));
}

#[test]
fn the_last_tray_state_comes_back_from_tray_json() {
    let d = data("tray");
    let paths = Paths::new(&d);
    assert_eq!(tray_load(&paths), WatchState::default());
    let t = serde_json::json!({"last": ["ok", "6 of 6 nodes working", "09:15"], "segs": ["ok", "warn"]});
    engine::data_files::jsave(&paths.tray_last, &t, None).unwrap();
    let st = tray_load(&paths);
    assert_eq!(st.last, Some(("ok".into(), "6 of 6 nodes working".into(), "09:15".into())));
    assert_eq!(st.segs, ["ok", "warn"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_tray_follows_the_window_unless_the_watcher_found_something_new() {
    let d = data("adopt");
    let paths = Paths::new(&d);
    let old =
        WatchState { last: Some(("bad".into(), "2 of 6 nodes working".into(), "08:00".into())), ..Default::default() };
    let t = serde_json::json!({"last": ["ok", "6 of 6 nodes working", "09:15"], "segs": ["ok", "ok"]});
    engine::data_files::jsave(&paths.tray_last, &t, None).unwrap();
    // the watcher's pass changed nothing: the window's newer result shows
    let mut st = old.clone();
    tray_adopt(&paths, &mut st, &old);
    assert_eq!(st.last.as_ref().map(|l| l.0.as_str()), Some("ok"));
    assert_eq!(st.segs, ["ok", "ok"]);
    // the watcher found something itself: its own result stays
    let mut st =
        WatchState { last: Some(("warn".into(), "5 of 6 nodes working".into(), "09:20".into())), ..old.clone() };
    tray_adopt(&paths, &mut st, &old);
    assert_eq!(st.last.as_ref().map(|l| l.0.as_str()), Some("warn"));
    let _ = std::fs::remove_dir_all(&d);
}

/// Fails every command: a stopped watcher must not reach the router.
struct NoRouter;

impl Ssh for NoRouter {
    fn run(&mut self, _: &str, _: Option<&str>, _: u64) -> Result<String, Error> {
        panic!("a stopped watcher contacted the router");
    }
}

#[test]
fn a_stopped_watcher_checks_nothing() {
    let d = data("stopped");
    let paths = Paths::new(&d);
    engine::settings::set_stopped(&paths, true).unwrap();
    let mut st = WatchState::default();
    assert_eq!(watch_cycle(&paths, &mut NoRouter, &mut st).unwrap(), None);
    assert_eq!(st, WatchState::default());
    let _ = std::fs::remove_dir_all(&d);
}
