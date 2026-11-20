//! The background watcher: the node check that fixes what is broken (watch_cycle), the slow-node fix, the tray state,
//! PIA's state feed, and the loop that runs them on schedule (engine.watch, watch_cycle, slow_fix, tray_mark).
//! The tray, notifications and the window are the app's: it passes them in as WatchHooks.

use crate::activity_log::{FileLog, dbg, say};
use crate::clash_api::{check_pc, wait_api};
use crate::history;
use crate::node_manager::{apply, inuse, replace, rescue, router_sync, router_up, scan};
use crate::node_tests::{node_test, router_test};
use crate::paths::Paths;
use crate::pc_tunnel::{core_ensure, core_log_trim, core_mode, core_restart, regroup};
use crate::router::{self, Ssh};
use crate::settings::{self, Settings, router_only, stopped};
use crate::translations::tr;
use serde_json::{Value, json};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The loopback port the running watcher holds, so a second one does not start.
pub const WATCH_PORT: u16 = 47291;
/// How often the watcher wakes at most; between wake-ups it does nothing at all.
pub const TICK: f64 = 30.0 * 60.0;
/// The app update check: one GitHub API call (two with a new version); GitHub allows 60 an hour per address.
pub const APP_EVERY: f64 = 6.0 * 3600.0;
/// After a logon start the first node check waits this long, so no node is judged before the network is up.
pub const LOGON_HOLD: f64 = 90.0;
/// The slow-node fix and the vault refresh run at most this often.
const SIX_HOURS: f64 = 6.0 * 3600.0;

// time.time()
fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// The local time as HH:MM, for the tray
fn clock() -> String {
    crate::local_time::now().format("%H:%M")
}

// A node's field as text
fn field(n: &Value, k: &str) -> String {
    n.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// What the watcher keeps between checks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WatchState {
    /// The tray: (state, text, HH:MM), state "ok", "warn", "bad" or "unk".
    pub last: Option<(String, String, String)>,
    /// One ring segment per node: "ok", "warn", "bad".
    pub segs: Vec<String>,
    /// When the slow-node fix last ran.
    pub speed_t: f64,
    /// When the vault was last tested again.
    pub vault_t: f64,
}

/// Test results in slot order: (tag, ms or None when dead).
type Results = Vec<(String, Option<f64>)>;

// The tray state from test results; dead defaults to the nodes without a time (engine.tray_mark)
pub fn tray_mark(paths: &Paths, st: &mut WatchState, res: &Results, dead: Option<&[String]>) {
    let slow = Settings::read(paths).map_or(200, |s| s.slow_ms) as f64;
    let m = crate::tray_state::tray_mark(res, dead, slow);
    let text = tr("{ok} of {n} nodes working", &[("ok", m.working.to_string()), ("n", m.total.to_string())]);
    st.last = Some((m.state.into(), text, clock()));
    st.segs = m.segs.into_iter().map(String::from).collect();
}

// The results of one live check: field k (pc_ms or phone_ms) of each node
fn results(cur: &[Value], k: &str) -> Results {
    cur.iter().map(|n| (field(n, "tag"), n.get(k).and_then(Value::as_f64))).collect()
}

// The tags without a time
fn dead_of(res: &Results) -> Vec<String> {
    res.iter().filter(|(_, ms)| ms.is_none()).map(|(t, _)| t.clone()).collect()
}

// "Replaced vless-2 (a -> b), vless-5 (c -> d)"
fn replaced(swaps: &[crate::node_manager::Swap]) -> String {
    let each: Vec<String> = swaps.iter().map(|(t, o, n)| format!("{t} ({o} -> {})", field(n, "server"))).collect();
    format!("Replaced {}", each.join(", "))
}

// Add an event to history.json
fn event(paths: &Paths, kind: &str, text: &str) {
    history::record(&paths.history, &[], Some((kind, text)), now());
}

// Which tunnel judges the nodes
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Pc,
    Router,
}

// One live test of the nodes in use through the judging tunnel
fn live_test(paths: &Paths, ssh: &mut impl Ssh, cur: &mut [Value], side: Side) -> Result<Results, String> {
    if side == Side::Router {
        let secret = router::secret(&crate::data_files::read_json(&paths.router_cfg).map_err(|e| e.to_string())?);
        router::check_phone(ssh, cur, &secret).map_err(|e| e.to_string())?;
        return Ok(results(cur, "phone_ms"));
    }
    check_pc(paths, cur, false, now()).map_err(|_| "not a JSON object".to_string())?;
    Ok(results(cur, "pc_ms"))
}

// Check the nodes in use and fix what is broken right away: a dead node is checked again after 30 s and replaced,
// every node dead: rescue. The router's live tunnel judges when it answers, else the PC tunnel. A sentence when
// something changed, for a notification (engine.watch_cycle)
pub fn watch_cycle(paths: &Paths, ssh: &mut impl Ssh, st: &mut WatchState) -> Result<Option<String>, String> {
    if stopped(paths) {
        say("Reality Hop is stopped; no check");
        return Ok(None);
    }
    if core_mode(paths) {
        core_ensure(paths);
    }
    let mut cur = inuse(paths)?;
    if cur.is_empty() {
        // nothing in use yet (a setup that could not finish): find the first set
        let nodes = scan(paths, ssh, None)?;
        apply(paths, ssh, &nodes, 6)?;
        let out = "No nodes were in use; the 6 best from a fresh scan are in now.".to_string();
        say(&out);
        return Ok(Some(out));
    }
    if router_up(paths, ssh) {
        if let Some(healed) = router_sync(paths, ssh)? {
            say(&healed);
            event(paths, "heal", &healed);
            return Ok(Some(healed));
        }
        // the router's self-repair stands down while this PC looks after the nodes
        let _ = ssh.run("date +%s > /tmp/pc-alive", None, router::SSH_TIMEOUT);
        let busy = "[ -d /tmp/heal.lock ] && [ $(( $(date +%s) - $(cat /tmp/heal.started 2>/dev/null || echo 0) )) -lt 3600 ] && echo busy";
        if crate::python_compat::strip(&ssh.run(busy, None, router::SSH_TIMEOUT).map_err(|e| e.to_string())?) == "busy"
        {
            say("router self-repair is running; leaving it to finish");
            return Ok(None);
        }
    }
    let r = check_pc(paths, &mut cur, false, now()).map_err(|_| "not a JSON object".to_string())?;
    let side = if r.is_some_and(|r| !r.stale) {
        Side::Pc // this PC's own tunnel is the judge
    } else if router_up(paths, ssh) {
        Side::Router
    } else {
        say("the PC tunnel cannot test the current set and the router is unreachable; nothing to judge");
        return Ok(None);
    };
    let name = if side == Side::Pc { "PC" } else { "router" };
    // the PC tunnel tested them just above: not twice
    let mut res = if side == Side::Pc { results(&cur, "pc_ms") } else { live_test(paths, ssh, &mut cur, side)? };
    let mut dead = dead_of(&res);
    let tally = |res: &Results, dead: &[String]| {
        let list = if dead.is_empty() { String::new() } else { format!("; dead: {}", dead.join(", ")) };
        format!("{} of {} nodes working{list}", res.len() - dead.len(), res.len())
    };
    say(&format!("{name} check: {}", tally(&res, &dead)));
    tray_mark(paths, st, &res, None);
    if !dead.is_empty() {
        say("re-checking in 30 s so one dropped probe does not cost a node...");
        std::thread::sleep(Duration::from_secs(30));
        res = live_test(paths, ssh, &mut cur, side)?;
        dead = dead_of(&res);
        say(&format!("re-check: {}", tally(&res, &dead)));
    }
    if !dead.is_empty() && (paths.core.exists() || router_only(paths)) {
        // second opinion: a fresh test instance, not the running tunnel
        let mut again: Vec<Value> = cur.iter().filter(|n| dead.contains(&field(n, "tag"))).cloned().collect();
        node_test(paths, ssh, &mut again, false)?;
        let fine: Vec<String> = again.iter().filter(|n| n["ok"] == true).map(|n| field(n, "tag")).collect();
        if !fine.is_empty() {
            say(&format!("independent test: {} work after all; not replacing them", fine.join(", ")));
            dead.retain(|t| !fine.contains(t));
            for n in again.iter().filter(|n| n["ok"] == true) {
                if let Some(slot) = res.iter_mut().find(|(t, _)| *t == field(n, "tag")) {
                    slot.1 = n.get("ms").and_then(Value::as_f64);
                }
            }
        }
    }
    tray_mark(paths, st, &res, Some(&dead));
    let pings: Vec<(String, Option<i64>)> = cur
        .iter()
        .map(|n| {
            let tag = field(n, "tag");
            let ms = if dead.contains(&tag) { None } else { res.iter().find(|(t, _)| *t == tag).and_then(|(_, m)| *m) };
            (field(n, "server"), ms.map(|m| m as i64))
        })
        .collect();
    history::record(&paths.history, &pings, None, now());
    let mut out = None;
    if !dead.is_empty() && dead.len() == res.len() {
        out = Some(rescue(paths, ssh, 6)?);
    } else if !dead.is_empty() {
        let slots: Vec<usize> = (0..cur.len()).filter(|&i| dead.contains(&field(&cur[i], "tag"))).collect();
        out = Some(format!("{}.", replaced(&replace(paths, ssh, &slots, None, false, false)?)));
    }
    let s = Settings::read(paths).unwrap_or_default();
    if out.is_none() && s.min_mbps != 0 && paths.core.exists() && !router_only(paths) && now() - st.speed_t > SIX_HOURS
    {
        st.speed_t = now();
        out = slow_fix(paths, ssh, &cur)?;
    }
    match &out {
        Some(o) => {
            say(o);
            event(paths, if o.starts_with("Replaced") { "replace" } else { "rescue" }, o);
        }
        None => regroup(paths),
    }
    if paths.core.exists() && now() - st.vault_t > SIX_HOURS {
        // keep the vault fresh
        st.vault_t = now();
        let used: Vec<String> = cur.iter().map(|n| field(n, "server")).collect();
        let used: Vec<&str> = used.iter().map(String::as_str).collect();
        let mut v: Vec<Value> =
            crate::vault::vault_nodes(paths, &used).into_iter().take(s.top.max(0) as usize).collect();
        if !v.is_empty() {
            router_test(paths, ssh, &mut v, false)?;
            let ok = v.iter().filter(|n| n["ok"] == true).count();
            say(&format!("vault refresh: {ok} of {} still work", v.len()));
        }
    }
    Ok(out)
}

// In-use nodes under the floor on two download tests in a row are swapped for spares that reach it; every 6 hours from
// the check (up to 50 MB per node). A sentence when something changed (engine.slow_fix)
pub fn slow_fix(paths: &Paths, ssh: &mut impl Ssh, cur: &[Value]) -> Result<Option<String>, String> {
    let min = Settings::read(paths).unwrap_or_default().min_mbps;
    let floor = || crate::node_tests::floor_now(paths, min, now());
    let under =
        |n: &Value| n["ok"] == true && n.get("mbps").and_then(Value::as_f64).is_some_and(|m| m < floor() as f64);
    let mut t = cur.to_vec();
    node_test(paths, ssh, &mut t, true)?;
    let mut slow: Vec<usize> = (0..t.len()).filter(|&i| under(&t[i])).collect();
    if !slow.is_empty() {
        let tags: Vec<String> = slow.iter().map(|&i| field(&cur[i], "tag")).collect();
        say(&format!("under {} Mbps: {}; testing them again...", floor(), tags.join(", ")));
        let mut again: Vec<Value> = slow.iter().map(|&i| cur[i].clone()).collect();
        node_test(paths, ssh, &mut again, true)?;
        slow = slow.into_iter().zip(&again).filter(|(_, n)| under(n)).map(|(i, _)| i).collect();
    }
    if slow.is_empty() {
        say(&format!("speed check: no working node is under {} Mbps", floor()));
        return Ok(None);
    }
    let swaps = replace(paths, ssh, &slow, None, false, true)?;
    Ok((!swaps.is_empty()).then(|| format!("{} (under {} Mbps).", replaced(&swaps), floor())))
}

// After the tunnel restarts or the nodes change: test what runs now, so the tray does not keep showing the old set
// (engine.tray_recheck)
pub fn tray_recheck(paths: &Paths, ssh: &mut impl Ssh, st: &mut WatchState) {
    let r = (|| -> Result<(), String> {
        let mut cur = inuse(paths)?;
        if cur.is_empty() || stopped(paths) {
            return Ok(());
        }
        let after =
            |st: &WatchState| say(&format!("after the change: {}", st.last.as_ref().map_or("", |l| l.1.as_str())));
        if !router_only(paths) && wait_api(paths, Duration::from_secs(30)) {
            let r = check_pc(paths, &mut cur, false, now()).map_err(|_| "not a JSON object".to_string())?;
            if r.is_some_and(|r| !r.stale) {
                tray_mark(paths, st, &results(&cur, "pc_ms"), None);
                after(st);
                return Ok(());
            }
        }
        if router_up(paths, ssh) {
            let res = live_test(paths, ssh, &mut cur, Side::Router)?;
            tray_mark(paths, st, &res, None);
            after(st);
        }
        Ok(())
    })();
    if let Err(e) = r {
        say(&format!("tray re-check failed: {e}"));
    }
}

// PIA's own connection state (Connected, Reconnecting, ...). Asks the PIA app only; sends nothing through the tunnel
// (engine.pia_state)
pub fn pia_state() -> String {
    let r = crate::process::run(
        &[crate::pia_split_tunnel::PIACTL, "get", "connectionstate"],
        None,
        Duration::from_secs(15),
        None,
    );
    r.map(|o| crate::python_compat::strip(&o.stdout).to_string()).unwrap_or_default()
}

static FEED: Mutex<Option<std::process::Child>> = Mutex::new(None);

// PIA's own state feed (piactl monitor): one line per change, no polling and no network traffic. Starts again 20 s
// after it ends (PIA not running yet). Nothing without PIA: the hourly check covers other VPN apps (engine.pia_feed)
pub fn pia_feed(on_change: impl Fn(&str) + Send + 'static) {
    if !std::path::Path::new(crate::pia_split_tunnel::PIACTL).exists() {
        return;
    }
    std::thread::spawn(move || {
        loop {
            let mut c = std::process::Command::new(crate::pia_split_tunnel::PIACTL);
            c.args(["monitor", "connectionstate"]);
            c.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
            }
            if let Ok(mut child) = c.spawn() {
                let out = child.stdout.take();
                *FEED.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
                if let Some(out) = out {
                    use std::io::BufRead;
                    for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                        let line = crate::python_compat::strip(&line);
                        if !line.is_empty() {
                            on_change(line);
                        }
                    }
                }
                if let Some(mut c) = FEED.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = c.wait();
                }
            }
            std::thread::sleep(Duration::from_secs(20));
        }
    });
}

// Stop this watcher's PIA state feed (Quit)
pub fn pia_feed_stop() {
    if let Some(mut c) = FEED.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

// Stop the PIA monitors a stopped watcher left behind (engine.kill_feeds)
pub fn kill_feeds() {
    if cfg!(windows) {
        use crate::processes_and_ports::{cmdline, pids, stop_pid};
        for pid in pids("piactl.exe") {
            if cmdline(pid).is_some_and(|c| c.to_lowercase().contains("monitor connectionstate")) {
                stop_pid(pid);
            }
        }
        return;
    }
    let out = crate::process::run(&["/bin/ps", "-axww", "-o", "pid=,command="], None, Duration::from_secs(15), None);
    let own = std::process::id();
    let stray: Vec<String> = crate::macos::ps_rows(&out.map(|o| o.stdout).unwrap_or_default())
        .into_iter()
        .filter(|(p, c)| *p != own && c.contains("piactl") && c.contains("monitor connectionstate"))
        .map(|(p, _)| p.to_string())
        .collect();
    if !stray.is_empty() {
        let args: Vec<&str> = ["/bin/kill", "-KILL"].into_iter().chain(stray.iter().map(String::as_str)).collect();
        let _ = crate::process::run(&args, None, Duration::from_secs(15), None);
    }
}

// The watcher sleeps almost all the time: give its memory back until the next check needs it (engine.memory_trim)
pub fn memory_trim() {
    crate::downloads::feed_forget_stale();
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

/// What the app gives the watcher: the tray, notifications and its own update check.
pub trait WatchHooks {
    // Show a notification (Windows toast, macOS Notification Center)
    fn notify(&self, text: &str);
    // Show the tray state: "ok", "warn", "bad" or "unk", the tooltip, and one ring segment per node
    fn show(&self, state: &str, tip: &str, segs: &[String]);
    // True while the window is open: it does the checks then
    fn window_open(&self) -> bool;
    // The app update check, every 6 hours; true when an update was installed and this process must end
    fn app_update(&self) -> bool {
        false
    }
}

// What PIA's feed told the watcher
#[derive(Default)]
struct Pia {
    state: String,
    bad_since: Option<f64>,
    drops: Vec<f64>,
}

#[derive(Default)]
struct Inner {
    want: Vec<String>,
    woke: bool,
    stop: bool,
    pia: Pia,
}

/// The watcher's wake-up: tray requests, PIA changes and Quit reach the loop through it.
#[derive(Default)]
pub struct Watch {
    inner: Mutex<Inner>,
    cv: Condvar,
}

impl Watch {
    // A new one, to share between the loop, the tray and PIA's feed
    pub fn new() -> Arc<Watch> {
        Arc::new(Watch::default())
    }

    // The lock, poisoned or not
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Wake the loop now
    fn wake(&self) {
        self.lock().woke = true;
        self.cv.notify_all();
    }

    // A request from the tray: "check", "restart" or "rescue"
    pub fn ask(&self, what: &str) {
        self.lock().want.push(what.into());
        self.wake();
    }

    // End the loop (Quit)
    pub fn stop(&self) {
        self.lock().stop = true;
        self.wake();
    }

    // Take a request when it was made
    fn take(&self, what: &str) -> bool {
        let mut g = self.lock();
        let had = g.want.iter().any(|w| w == what);
        g.want.retain(|w| w != what);
        had
    }

    // PIA's connection state changed (one feed line)
    pub fn on_pia(&self, paths: &Paths, state: &str) {
        let t = now();
        let (prev, wake) = {
            let mut g = self.lock();
            let prev = std::mem::replace(&mut g.pia.state, state.to_string());
            if state == "Connected" || state == "Disconnected" {
                g.pia.bad_since = None;
            } else if g.pia.bad_since.is_none() {
                g.pia.bad_since = Some(t); // PIA passes through several "connecting" states; count from the first
            }
            let dropped = prev == "Connected" && state != "Connected";
            // PIA often blips twice in a row: one drop
            if dropped && g.pia.drops.last().is_none_or(|d| t - d > 30.0) {
                g.pia.drops.push(t);
                event(paths, "drop", &format!("PIA {}", state.to_lowercase()));
            }
            (prev, dropped)
        };
        dbg(|| format!("PIA {} -> {state}", if prev.is_empty() { "?" } else { &prev }));
        if wake {
            say(&format!("PIA dropped ({state})"));
            self.wake();
        } else if state == "Connected" && !prev.is_empty() && prev != "Connected" {
            event(paths, "back", "PIA connected");
            say("PIA connected again");
        }
    }

    // PIA drops in the last hour
    pub fn pia_drops(&self) -> usize {
        let mut g = self.lock();
        let cut = now() - 3600.0;
        g.pia.drops.retain(|t| *t > cut);
        g.pia.drops.len()
    }

    // Sleep up to secs, or until woken; false once stopped
    pub fn sleep(&self, secs: f64) -> bool {
        let g = self.lock();
        let (mut g, _) = self
            .cv
            .wait_timeout_while(g, Duration::from_secs_f64(secs.max(0.0)), |g| !g.woke && !g.stop)
            .unwrap_or_else(|e| e.into_inner());
        g.woke = false;
        !g.stop
    }
}

// The tray's tooltip for a state (engine.watch.show)
pub fn tray_tip(st: &WatchState, drops: usize, core: bool) -> (String, String) {
    let (state, text, at) =
        st.last.clone().unwrap_or_else(|| ("unk".into(), tr("not checked yet", &[]), String::new()));
    let mut tip = format!("Reality Hop  ·  {text}");
    if !at.is_empty() {
        tip += &format!("  ·  {at}");
    }
    if drops > 0 {
        tip += &format!("  ·  {}", tr("PIA dropped {n}x in the last hour", &[("n", drops.to_string())]));
    }
    if !core {
        tip += &format!("  ·  {}", tr("rh-core missing", &[]));
    }
    (state, tip)
}

// The last tray state, from tray.json, so the icon is not grey while the first check runs
pub fn tray_load(paths: &Paths) -> WatchState {
    let t = crate::data_files::jload(&paths.tray_last);
    let mut st = WatchState::default();
    let last: Vec<String> = t
        .get("last")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    if let [a, b, c] = &last[..] {
        st.last = Some((a.clone(), b.clone(), c.clone()));
        st.segs = t
            .get("segs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect();
    }
    st
}

// The window checks while it is open and writes what it found to tray.json. When this pass of the watcher found
// nothing new, the tray follows that file, not the watcher's older state
pub fn tray_adopt(paths: &Paths, st: &mut WatchState, before: &WatchState) {
    if (&st.last, &st.segs) != (&before.last, &before.segs) {
        return;
    }
    let disk = tray_load(paths);
    if disk.last.is_some() {
        (st.last, st.segs) = (disk.last, disk.segs);
    }
}

// Show the state in the tray and keep it in tray.json
fn show(paths: &Paths, watch: &Watch, hooks: &impl WatchHooks, st: &WatchState) {
    if let Some((a, b, c)) = &st.last {
        let _ = crate::data_files::jsave(&paths.tray_last, &json!({"last": [a, b, c], "segs": st.segs}), None);
    }
    let (state, tip) = tray_tip(st, watch.pia_drops(), core_mode(paths));
    hooks.show(&state, &tip, &st.segs);
}

// Hold the watcher's port, so a second watcher does not start; after an update the old one may still be exiting
pub fn watch_lock() -> Option<std::net::TcpListener> {
    for i in 0..10 {
        if let Ok(l) = std::net::TcpListener::bind(("127.0.0.1", WATCH_PORT)) {
            return Some(l);
        }
        if i < 9 {
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    None
}

// The watcher (--watch, started at logon). Pauses its checks while the window is open. A node check every check_min
// minutes (Settings); PIA's state feed wakes it the moment PIA drops, and when PIA is still not connected a minute
// later the nodes are checked and fixed at once. Daily sing-box update on its channel. Returns when stopped, or when
// an app update needs this process to end (engine.watch). cmd starts the watcher; logon: started at logon
pub fn watch_loop(paths: &Paths, watch: &Arc<Watch>, hooks: &impl WatchHooks, cmd: &[String], logon: bool) {
    if stopped(paths) {
        return; // stopped from the window: off until Start
    }
    if cfg!(windows) {
        crate::startup_tasks::lnk_migrate(&crate::startup_tasks::startup_lnk());
    }
    crate::setup_steps::api_lockdown(paths);
    crate::pia_split_tunnel::pia_bypass(paths, &[], true); // at login, before the tunnel starts
    let started = crate::pc_tunnel::core_fast_start(paths); // first, so PIA finds the proxy when it connects
    kill_feeds();
    let log = FileLog::new(&paths.logs, "watch");
    if !crate::activity_log::has_say() {
        crate::activity_log::set_say(move |m| log.write(m)); // the app sends it to its own log and window
    }
    let started_at = now();
    say(if router_only(paths) {
        "router-only mode: no tunnel on this PC"
    } else if started {
        "rh-core started at logon"
    } else {
        "rh-core already running"
    });
    let s = Settings::read(paths).unwrap_or_default();
    crate::translations::use_lang(&s.lang);
    let pia_on = std::path::Path::new(crate::pia_split_tunnel::PIACTL).exists();
    say(&format!(
        "watcher started: node check every {} min, PIA state feed {}, {} mode",
        s.check_min,
        if pia_on { "on" } else { "not available (PIA not installed)" },
        if core_mode(paths) { "core" } else { "NO rh-core" }
    ));
    if cfg!(windows)
        && crate::startup_tasks::startup_lnk().exists()
        && !settings::this_pc(paths).contains_key("logon_task")
    {
        crate::startup_tasks::startup(paths, true, cmd, |_| {}); // up to 1.4.0 the Startup folder started the watcher
    }
    let hold = started_at + if logon { LOGON_HOLD } else { 0.0 }; // no node is judged before the network is up
    let full_every = |p: &Paths| Settings::read(p).map_or(3600.0, |s| (s.check_min * 60) as f64);
    let (mut last_full, mut last_fix) = (hold - full_every(paths), 0.0);
    let mut last_app = now() - APP_EVERY + 120.0; // app update check 2 min after logon
    let mut st = tray_load(paths);
    {
        let (w, p) = (Arc::clone(watch), paths.clone());
        pia_feed(move |line| w.on_pia(&p, line));
    }
    show(paths, watch, hooks, &st);
    let mut ssh = crate::router_ssh::RouterSsh { paths };
    wait_api(paths, Duration::from_secs(90));
    loop {
        crate::process::reset_cancel();
        let t = now();
        let before = st.clone();
        let s = Settings::read(paths).unwrap_or_default();
        crate::translations::use_lang(&s.lang);
        let every = (s.check_min * 60) as f64;
        let bad_since = watch.lock().pia.bad_since;
        dbg(|| {
            format!(
                "watch wake: next full check in {} s; PIA bad for {} s",
                (last_full + every - t) as i64,
                bad_since.map_or(0, |b| (t - b) as i64)
            )
        });
        let r = (|| -> Result<bool, String> {
            if crate::pc_tunnel::core_update_due(paths) && t - started_at > 600.0 {
                // sing-box: daily, 10 min after logon at the earliest
                let _ =
                    settings::set_this_pc(paths, json!({"core_checked": t}).as_object().cloned().unwrap_or_default());
                if let Err(e) = crate::pc_tunnel::core_update(paths, false) {
                    say(&format!("rh-core update check failed: {e}"));
                }
            }
            if t - last_app >= APP_EVERY {
                last_app = t;
                if hooks.app_update() {
                    return Ok(true);
                }
            }
            let fixed = |out: Option<String>, st: &mut WatchState, ssh: &mut crate::router_ssh::RouterSsh| {
                if let Some(o) = &out {
                    hooks.notify(o);
                    tray_recheck(paths, ssh, st); // the old state still shows the nodes that were just replaced
                }
                out.is_some()
            };
            if watch.take("restart") {
                say("restart requested from the tray");
                let ok = core_restart(paths);
                event(paths, "restart", "PC tunnel restarted from the tray");
                hooks.notify(&tr(
                    if ok { "PC tunnel restarted." } else { "PC tunnel did not come back; open Reality Hop." },
                    &[],
                ));
                tray_recheck(paths, &mut ssh, &mut st);
            } else if watch.take("rescue") {
                let out = rescue(paths, &mut ssh, 6)?;
                say(&out);
                hooks.notify(&out);
                event(paths, "rescue", &out);
                last_full = t;
                tray_recheck(paths, &mut ssh, &mut st);
            } else if watch.take("check") {
                last_full = t;
                let out = watch_cycle(paths, &mut ssh, &mut st)?;
                fixed(out, &mut st, &mut ssh);
            } else if bad_since.is_some_and(|b| t - b >= 60.0) && t - last_fix > 300.0 && t >= hold {
                let state = watch.lock().pia.state.clone();
                say(&format!("PIA still \"{state}\" after a minute; checking the nodes now"));
                (last_fix, last_full) = (t, t);
                let out = watch_cycle(paths, &mut ssh, &mut st)?;
                if !fixed(out, &mut st, &mut ssh) && core_mode(paths) {
                    say("every node checks fine; restarting the PC tunnel");
                    core_restart(paths);
                    event(paths, "restart", "PC tunnel restarted because PIA could not reconnect");
                    tray_recheck(paths, &mut ssh, &mut st);
                }
            } else if hooks.window_open() {
                last_full = t; // the window does the checks while it is open
            } else if t - last_full > every {
                last_full = t;
                let out = watch_cycle(paths, &mut ssh, &mut st)?;
                fixed(out, &mut st, &mut ssh);
            } else if core_mode(paths) {
                core_ensure(paths);
            }
            core_log_trim(paths);
            Ok(false)
        })();
        match r {
            Ok(true) => {
                pia_feed_stop();
                return;
            }
            Ok(false) => {}
            Err(e) => {
                say(&format!("error: {e}"));
                st.last = Some(("bad".into(), format!("error: {e}").chars().take(80).collect(), clock()));
            }
        }
        tray_adopt(paths, &mut st, &before);
        show(paths, watch, hooks, &st);
        memory_trim();
        // sleep until the next check is due, a tray request, or a PIA change (checked again a minute after a drop)
        let due = if watch.lock().pia.bad_since.is_some() {
            60.0
        } else {
            let n = now();
            TICK.min(last_full + every - n).min(last_app + APP_EVERY - n).max(30.0)
        };
        if !watch.sleep(due) {
            pia_feed_stop();
            return;
        }
    }
}
