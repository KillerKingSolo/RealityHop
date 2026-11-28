//! The background watcher: the node check that fixes what is broken (watch_cycle), the slow-node fix, the tray state,
//! PIA's state feed, and the loop that runs them on schedule (engine.watch, watch_cycle, slow_fix, tray_mark).
//! The tray, notifications and the window are the app's: it passes them in as WatchHooks.

use crate::activity_log::{FileLog, dbg, say};
use crate::clash_api::{check_pc, wait_api};
use crate::history;
use crate::node_manager::{SLOTS, apply, inuse, replace, rescue, router_sync, router_up, scan};
use crate::node_tests::{node_test, router_test};
use crate::paths::Paths;
use crate::pc_tunnel::{core_ensure, core_log_trim, core_mode, core_restart, regroup};
use crate::router::{self, Ssh};
use crate::settings::{self, Settings, router_only, stopped};
use crate::translations::tr;
use serde_json::{Value, json};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
const DAY: f64 = 24.0 * 3600.0;
/// Traffic through the PC tunnel under this (KB/s) counts as a quiet line: the speed check may run.
pub const IDLE_KBPS: f64 = 10.0;
/// PIA drops within an hour, each while the same node carried the tunnel, that get that node replaced.
pub const BREAKS: usize = 3;
/// PIA drops closer together than this are one burst: one blame and one node test, not one per blip.
pub const BURST: f64 = 30.0;
/// Speed checks, node swaps for PIA drops and filling empty slots wait until PIA has not dropped for this long, so a
/// drop storm (a network change, Wi-Fi waking) does not judge nodes on a line that is down every few seconds.
pub const CALM: f64 = 300.0;

// The last PIA drop, as f64 bits (0: none since start); the watcher loop and its checks read it
static LAST_BLIP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// Seconds since PIA last dropped, at time now; infinite when it has not dropped since the watcher started
pub fn pia_calm_for(now: f64) -> f64 {
    let t = f64::from_bits(LAST_BLIP.load(std::sync::atomic::Ordering::SeqCst));
    if t == 0.0 { f64::INFINITY } else { now - t }
}

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
    /// Since when the due speed check waits for a quiet line; 0 when it does not wait.
    pub speed_wait: f64,
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
        apply(paths, ssh, &nodes, SLOTS)?;
        let out = "No nodes were in use; the 6 best from a fresh scan are in now.".to_string();
        say(&out);
        return Ok(Some(out));
    }
    if router_up(paths, ssh) {
        if let Some(healed) = follow_router(paths, ssh)? {
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
    let mut r = check_pc(paths, &mut cur, false, now()).map_err(|_| "not a JSON object".to_string())?;
    if r.as_ref().is_some_and(|r| r.stale) && core_mode(paths) && running_set_failing(paths) {
        // the PC tunnel still runs an older set (it waits while PIA is connected) and most of its nodes fail: the
        // waiting set is better than a dying one
        say("most nodes the PC tunnel still runs fail; it takes the waiting node set now");
        core_restart(paths);
        event(paths, "restart", "PC tunnel took the waiting node set: most of its running nodes failed");
        r = check_pc(paths, &mut cur, false, now()).map_err(|_| "not a JSON object".to_string())?;
    }
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
        format!("{} of {} nodes working{list}", res.len() - dead.len(), res.len().max(SLOTS))
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
    // second opinion: a fresh test instance on this PC, not the running tunnel. It speaks for the router only when every
    // node failed there (the router's tunnel may be the problem): a node can fail from the router and pass from the PC
    let second = side == Side::Pc || dead.len() == res.len();
    if !dead.is_empty() && second && (paths.core.exists() || router_only(paths)) {
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
    if side == Side::Pc && router_up(paths, ssh) {
        dead.extend(router_dead(paths, ssh, &cur, &dead, &tally)?);
    }
    tray_mark(paths, st, &res, Some(&dead));
    if side == Side::Pc {
        settle(paths, &res, &dead);
    }
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
        out = Some(rescue(paths, ssh, SLOTS)?);
    } else if !dead.is_empty() {
        let slots: Vec<usize> = (0..cur.len()).filter(|&i| dead.contains(&field(&cur[i], "tag"))).collect();
        out = Some(format!("{}.", replaced(&replace(paths, ssh, &slots, None, false, false, false)?)));
    }
    let calm = pia_calm_for(now());
    if out.is_none() && cur.len() < SLOTS && calm < CALM {
        say(&format!("only {} of {SLOTS} nodes in use; filling them waits until PIA is steady", cur.len()));
    } else if out.is_none() && cur.len() < SLOTS {
        // a scan that found fewer working nodes left slots empty: fill them when spares work again
        say(&format!("only {} of {SLOTS} nodes in use; looking for working spares...", cur.len()));
        match replace(paths, ssh, &[], None, false, false, false) {
            Ok(sw) if !sw.is_empty() => out = Some(format!("{}.", replaced(&sw))),
            Ok(_) => {}
            Err(e) => say(&format!("empty slots stay empty for now: {e}")),
        }
    }
    let s = Settings::read(paths).unwrap_or_default();
    if out.is_none() && s.min_mbps != 0 && paths.core.exists() && !router_only(paths)
        // once per periodic check, or less often when Settings say so: a check asked for or a PIA problem soon
        // after does not test again
        && now() - st.speed_t > ((s.check_min * 60).max(s.speed_hours * 3600)) as f64 * 0.9
    {
        let rate = crate::clash_api::tunnel_rate(paths);
        if st.speed_wait == 0.0 {
            st.speed_wait = now();
        }
        if calm < CALM {
            say(&format!("speed check postponed: PIA dropped {calm:.0} s ago; speeds now would not be real"));
        } else if speed_check_go(rate, now() - st.speed_wait) {
            (st.speed_t, st.speed_wait) = (now(), 0.0);
            out = slow_fix(paths, ssh, &cur)?;
        } else {
            say(&format!(
                "speed check postponed: {:.0} KB/s through the PC tunnel now (a game, a call or a download)",
                rate.unwrap_or(0.0)
            ));
        }
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

// The PC tunnel runs an older node set and fewer than half of its nodes pass a quick test
fn running_set_failing(paths: &Paths) -> bool {
    crate::pc_tunnel::regroup_within(paths, 5000).len() < SLOTS / 2
}

// The PC judged the nodes; the router devices use the same set through the router's own tunnel, where a node can fail
// while it passes from the PC. The nodes that fail there twice, 30 s apart, while others pass. Empty when the router
// runs another set or its tunnel is down (nothing to say about the nodes)
fn router_dead(
    paths: &Paths,
    ssh: &mut impl Ssh,
    cur: &[Value],
    dead: &[String],
    tally: &dyn Fn(&Results, &[String]) -> String,
) -> Result<Vec<String>, String> {
    let secret = router::secret(&crate::data_files::read_json(&paths.router_cfg).map_err(|e| e.to_string())?);
    let mut test = |rc: &mut Vec<Value>| -> Option<Vec<String>> {
        let ck = router::check_phone(ssh, rc, &secret).ok()?;
        if !ck.service || crate::node_manager::router_stale(paths, &ck.cfg) {
            return None;
        }
        let res = results(rc, "phone_ms");
        let rd = dead_of(&res);
        if rd.len() == res.len() {
            return None; // every node fails there: the router's tunnel or line, not the nodes
        }
        say(&format!("router check: {}", tally(&res, &rd)));
        Some(rd.into_iter().filter(|t| !dead.contains(t)).collect())
    };
    let mut rc = cur.to_vec();
    let Some(first) = test(&mut rc).filter(|d| !d.is_empty()) else { return Ok(vec![]) };
    say("re-checking the router in 30 s so one dropped probe does not cost a node...");
    std::thread::sleep(Duration::from_secs(30));
    let mut rc = cur.to_vec();
    let again = test(&mut rc).unwrap_or_default();
    let both: Vec<String> = first.into_iter().filter(|t| again.contains(t)).collect();
    if !both.is_empty() {
        say(&format!("{} fail on the router while they pass on this PC; replacing them", both.join(", ")));
    }
    Ok(both)
}

// Follow the router's nodes when another PC or the router's self-repair changed them (router_sync), logged and kept in
// the history. No download test here: while PIA is connected the PC tunnel keeps its own working nodes, and the
// speed check of the next periodic check judges the new ones. A sentence when the nodes changed
pub fn follow_router(paths: &Paths, ssh: &mut impl Ssh) -> Result<Option<String>, String> {
    let Some(healed) = router_sync(paths, ssh)? else { return Ok(None) };
    say(&healed);
    event(paths, "heal", &healed);
    Ok(Some(healed))
}

// The speed check runs with each periodic check, so a node that throttles later is caught. It fills the line for a
// few seconds, so it waits while something uses the PC tunnel (traffic over IDLE_KBPS: a game, a call, a download),
// but not longer than a day. rate: the traffic now, None when unknown
pub fn speed_check_go(rate_kbps: Option<f64>, waited: f64) -> bool {
    rate_kbps.is_none_or(|r| r < IDLE_KBPS) || waited > DAY
}

// A node that carried the tunnel at BREAKS PIA drops within an hour: it passes the short tests but breaks long
// connections. It stays out of the spares for 12 hours and is replaced at once (a real failure, not a speed swap).
// A sentence when something changed
pub fn break_fix(paths: &Paths, ssh: &mut impl Ssh, server: &str) -> Result<Option<String>, String> {
    crate::clash_api::pia_break_note(paths, server, now());
    let cur = inuse(paths)?;
    let Some(slot) = cur.iter().position(|n| field(n, "server") == server) else {
        say("the node that broke PIA's connection is no longer in use");
        return Ok(None);
    };
    say(&format!("{} broke PIA's connection {BREAKS} times in an hour; replacing it", field(&cur[slot], "tag")));
    let swaps = replace(paths, ssh, &[slot], None, false, false, false)?;
    Ok((!swaps.is_empty())
        .then(|| format!("{} (it broke PIA's connection {BREAKS} times in an hour).", replaced(&swaps))))
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
    let swaps = replace(paths, ssh, &slow, None, false, true, false)?;
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
                let res = results(&cur, "pc_ms");
                settle(paths, &res, &dead_of(&res));
                tray_mark(paths, st, &res, None);
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

// Wait up to max_s for PIA to be steady (Connected, or off), then settle_s more when it had to wait. False when it is
// still reconnecting. Node tests through the PC tunnel fail while PIA reconnects (engine.pia_wait_steady)
pub fn pia_wait_steady(max_s: u64, settle_s: u64) -> bool {
    if !std::path::Path::new(crate::pia_split_tunnel::PIACTL).exists() {
        return true;
    }
    let t0 = Instant::now();
    let mut waited = false;
    loop {
        let st = pia_state();
        if matches!(st.as_str(), "Connected" | "Disconnected" | "") {
            if waited {
                std::thread::sleep(Duration::from_secs(settle_s));
            }
            return true;
        }
        if t0.elapsed() >= Duration::from_secs(max_s) {
            return false;
        }
        if !waited {
            say(&format!("PIA is {}; waiting for it before testing the nodes...", st.to_lowercase()));
        }
        waited = true;
        std::thread::sleep(Duration::from_secs(2));
    }
}

// Whether PIA is installed and connected now
pub fn pia_connected() -> bool {
    std::path::Path::new(crate::pia_split_tunnel::PIACTL).exists() && pia_state() == "Connected"
}

static FEED: Mutex<Option<std::process::Child>> = Mutex::new(None);
// Which feed may run: pia_feed_stop moves it on, so an old feed thread ends instead of starting piactl again
static FEED_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// PIA's own state feed (piactl monitor): one line per change, no polling and no network traffic. Starts again 20 s
// after it ends (PIA not running yet), until pia_feed_stop. One feed at a time: a new one ends the old. Nothing
// without PIA: the hourly check covers other VPN apps (engine.pia_feed)
pub fn pia_feed(on_change: impl Fn(&str) + Send + 'static) {
    if !std::path::Path::new(crate::pia_split_tunnel::PIACTL).exists() {
        return;
    }
    pia_feed_stop();
    let mine = FEED_GEN.load(std::sync::atomic::Ordering::SeqCst);
    let live = move || FEED_GEN.load(std::sync::atomic::Ordering::SeqCst) == mine;
    std::thread::spawn(move || {
        while live() {
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
                {
                    let mut f = FEED.lock().unwrap_or_else(|e| e.into_inner());
                    if !live() {
                        // stopped while piactl started
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                    *f = Some(child);
                }
                if let Some(out) = out {
                    use std::io::BufRead;
                    for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                        let line = crate::python_compat::strip(&line);
                        if !line.is_empty() && live() {
                            on_change(line);
                        }
                    }
                }
                let mut f = FEED.lock().unwrap_or_else(|e| e.into_inner());
                if live()
                    && let Some(mut c) = f.take()
                {
                    let _ = c.wait();
                }
            }
            for _ in 0..20 {
                if !live() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

// Stop the PIA state feed for good (Quit, Stop, the watcher ending)
pub fn pia_feed_stop() {
    let mut f = FEED.lock().unwrap_or_else(|e| e.into_inner());
    FEED_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(mut c) = f.take() {
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
    // (when, node in use) for each drop
    blame: Vec<(f64, String)>,
}

/// The node tag that carries PIA's connection since it last connected, and a count of PIA's state changes, so a
/// late lookup does not outlive the connection it was for.
static CARRIER: Mutex<Option<String>> = Mutex::new(None);
static PIA_CHANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

    // End the loop (Quit, Stop)
    pub fn stop(&self) {
        self.lock().stop = true;
        self.wake();
    }

    // Let the loop run again (Start after Stop): a loop that has not ended yet carries on; tray requests made while
    // stopped are dropped
    pub fn resume(&self) {
        let mut g = self.lock();
        g.stop = false;
        g.want.clear();
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
        let (prev, wake, first, carrier) = {
            let mut g = self.lock();
            let prev = std::mem::replace(&mut g.pia.state, state.to_string());
            if state == "Connected" || state == "Disconnected" {
                g.pia.bad_since = None;
            } else if g.pia.bad_since.is_none() {
                g.pia.bad_since = Some(t); // PIA passes through several "connecting" states; count from the first
            }
            let dropped = prev == "Connected" && state != "Connected";
            if prev != state {
                PIA_CHANGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            let carrier = if dropped { CARRIER.lock().unwrap_or_else(|e| e.into_inner()).take() } else { None };
            // PIA often blips twice in a row, or every few seconds for minutes after a network change: one burst
            let first = dropped && pia_calm_for(t) > BURST;
            if dropped {
                LAST_BLIP.store(t.to_bits(), std::sync::atomic::Ordering::SeqCst);
            }
            if dropped && g.pia.drops.last().is_none_or(|d| t - d > BURST) {
                g.pia.drops.push(t);
                event(paths, "drop", &format!("PIA {}", state.to_lowercase()));
            }
            (prev, dropped, first, carrier)
        };
        // a drop of PIA's own, not a disconnect by hand or one a tunnel restart caused
        let own = wake && state == "Reconnecting" && !crate::pc_tunnel::restarted_within(60.0);
        // only the first drop of a burst: in a storm every node in turn carries a blip, which says nothing about it.
        // The node is the one PIA's connection went through, else the one the tunnel uses now
        let node =
            carrier.as_deref().and_then(|tag| server_of(paths, tag)).or_else(|| crate::clash_api::tunnel_node(paths));
        if own
            && first
            && let Some(node) = node
        {
            self.blame(&node, t);
        }
        dbg(|| format!("PIA {} -> {state}", if prev.is_empty() { "?" } else { &prev }));
        // the tunnel's group tests its nodes only every minute: test them all now, so PIA's next try goes through a
        // working one. Once per burst: switching nodes at every blip of a storm changes the address PIA sees
        if first && core_mode(paths) && !stopped(paths) {
            let p = paths.clone();
            std::thread::spawn(move || steer(&p, own, carrier));
        }
        // note which node carries PIA's new connection, once it is up
        if state == "Connected" && prev != "Connected" && core_mode(paths) {
            let p = paths.clone();
            let mine = PIA_CHANGES.load(std::sync::atomic::Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                let tag = crate::clash_api::pia_carrier(&p);
                dbg(|| format!("PIA's connection goes through {}", tag.as_deref().unwrap_or("?")));
                let mut c = CARRIER.lock().unwrap_or_else(|e| e.into_inner());
                if PIA_CHANGES.load(std::sync::atomic::Ordering::SeqCst) == mine {
                    *c = tag;
                }
            });
        }
        if wake {
            say(&format!("PIA dropped ({state})"));
            self.wake();
        } else if state == "Connected" && !prev.is_empty() && prev != "Connected" {
            event(paths, "back", "PIA connected");
            say("PIA connected again");
        } else if state == "Disconnected" && prev != state {
            self.wake(); // a node set waiting for the PC tunnel can go in now
        }
    }

    // A PIA drop while node carried the tunnel
    pub fn blame(&self, node: &str, t: f64) {
        self.lock().pia.blame.push((t, node.to_string()));
    }

    // A node that carried the tunnel at BREAKS PIA drops within the last hour, and at more than half of them, at time
    // now. Drops spread over several nodes point at PIA or the network, not at one node
    pub fn breaker(&self, now: f64) -> Option<String> {
        let mut g = self.lock();
        g.pia.blame.retain(|(t, _)| now - t < 3600.0);
        let b = &g.pia.blame;
        let count = |n: &String| b.iter().filter(|(_, m)| m == n).count();
        b.iter().map(|(_, n)| n).find(|n| count(n) >= BREAKS && count(n) * 2 > b.len()).cloned()
    }

    // Forget the drops blamed on node (it was replaced)
    pub fn forgive(&self, node: &str) {
        self.lock().pia.blame.retain(|(_, n)| n != node);
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
        st.last.clone().unwrap_or_else(|| ("unk".into(), tr("Not checked yet", &[]), String::new()));
    let mut tip = format!("Reality Hop {}  ·  {text}", crate::VERSION);
    if !at.is_empty() {
        tip += &format!("  ·  {at}");
    }
    if drops > 0 {
        tip += &format!("  ·  {}", tr("PIA dropped {n}x in the last hour", &[("n", drops.to_string())]));
    }
    if !core {
        tip += &format!("  ·  {}", tr("rh-core missing", &[]));
    }
    // Windows shows at most 127 characters in a tray tooltip and drops the rest silently
    if tip.chars().count() > TIP_MAX {
        tip = tip.chars().take(TIP_MAX - 1).collect::<String>().trim_end().to_string() + "…";
    }
    (state, tip)
}

/// The longest tray tooltip Windows shows.
pub const TIP_MAX: usize = 127;

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
    let turn = crate::process::work();
    crate::setup_steps::api_lockdown(paths);
    crate::pia_split_tunnel::pia_bypass(paths, &[], true); // at login, before the tunnel starts
    let started = crate::pc_tunnel::core_fast_start(paths); // first, so PIA finds the proxy when it connects
    drop(turn);
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
        // never together with a window action: it waits for this check, and this check for it
        let turn = crate::process::work();
        if watch.lock().stop {
            pia_feed_stop(); // stopped while it waited for the window's action
            return;
        }
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
                let out = rescue(paths, &mut ssh, SLOTS)?;
                say(&out);
                hooks.notify(&out);
                event(paths, "rescue", &out);
                last_full = t;
                tray_recheck(paths, &mut ssh, &mut st);
            } else if watch.take("check") {
                last_full = t;
                let out = watch_cycle(paths, &mut ssh, &mut st)?;
                fixed(out, &mut st, &mut ssh);
            } else if let Some(bad) = watch.breaker(t).filter(|_| pia_calm_for(t) >= CALM) {
                watch.forgive(&bad);
                let out = break_fix(paths, &mut ssh, &bad)?;
                if let Some(o) = &out {
                    say(o);
                    event(paths, "replace", o);
                }
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
            } else if core_mode(paths)
                && !stopped(paths)
                && watch.lock().pia.state == "Disconnected"
                && crate::node_manager::pending_deploy(paths) == Some(true)
            {
                // a node set waits for the PC tunnel (another PC's change, a speed swap) and PIA is off: the restart
                // costs nothing now
                say("PIA is not connected: the PC tunnel takes the waiting node set now");
                core_restart(paths);
                event(paths, "restart", "PC tunnel took the waiting node set while PIA was not connected");
                tray_recheck(paths, &mut ssh, &mut st);
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
        drop(turn);
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

// The PC tunnel's selector after a check of every slot (res, dead: from that check). A pin holds while its node
// works and ends when it fails. Otherwise the group "auto" (the fast nodes) while one of them works, and the best
// working slow node, as a backup, while none does (settle)
pub fn settle(paths: &Paths, res: &Results, dead: &[String]) {
    use crate::clash_api::{pick, pin_of, proxies, set_pin, tag_of};
    let Some(px) = proxies(paths) else { return };
    let Some(held) = px.get(crate::singbox_config::PICK).map(|p| p["now"].as_str().unwrap_or("auto").to_string())
    else {
        return; // a config from before 2.1.1: no selector
    };
    let works = |t: &str| !dead.iter().any(|d| d == t) && res.iter().any(|(x, m)| x == t && m.is_some());
    if let Some(server) = pin_of(paths, "pc") {
        match tag_of(&paths.pc_cfg, &server) {
            Some(t) if works(&t) => {
                if held != t {
                    pick(paths, &t);
                }
                return;
            }
            // no node works at all: the network is down, not the pinned node
            Some(_) if !res.iter().any(|(x, _)| works(x)) => return,
            t => {
                set_pin(paths, "pc", None);
                say(&format!(
                    "the pinned node {server} {}; the PC tunnel picks its node automatically again",
                    if t.is_some() { "failed" } else { "is no longer in a slot" }
                ));
                pick(paths, "auto");
            }
        }
    }
    let group: Vec<String> = px
        .get("auto")
        .and_then(|g| g["all"].as_array())
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    if group.iter().any(|t| works(t)) {
        if held != "auto" && !group.contains(&held) {
            pick(paths, "auto");
            say("a fast node works again; the PC tunnel is back on the fast nodes");
        }
        return;
    }
    let backup = res
        .iter()
        .filter(|(t, m)| m.is_some() && works(t) && !group.contains(t))
        .min_by(|a, b| a.1.unwrap_or(f64::MAX).total_cmp(&b.1.unwrap_or(f64::MAX)));
    if let Some((t, _)) = backup
        && held != *t
        && pick(paths, t)
    {
        say(&format!("no fast node works; the PC tunnel uses {t}, a slower node, until one does"));
    }
}

/// How long the tunnel holds another node after the one in use broke PIA's connection but still passes the quick test.
pub const HOLD: Duration = Duration::from_secs(600);

/// Which hold is the latest: an older one that ends after a newer one started must not end the newer one.
static HOLDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// The address of a node tag in pc-config.json
fn server_of(paths: &Paths, tag: &str) -> Option<String> {
    let list = crate::singbox_config::nodes_of(&paths.pc_cfg).ok()?;
    list.into_iter().find(|(t, _)| t == tag).map(|(_, n)| n.server)
}

/// How many times, and how far apart, a PIA drop tests a pinned node while no node answers at all.
pub const PIN_TRIES: u32 = 6;
pub const PIN_GAP: Duration = Duration::from_secs(10);

// The pinned node after a PIA drop: Some(true) it passes, Some(false) it fails while another node passes (or it is
// no longer in a slot), None no node answered in any try
fn pin_verdict(p: &Paths, server: &str, tries: u32, gap: Duration) -> Option<bool> {
    let Some(tag) = crate::clash_api::tag_of(&p.pc_cfg, server) else { return Some(false) };
    let api = crate::clash_api::ControlApi::pc(p).ok()?;
    pin_judge(
        tries,
        gap,
        || api.delay(&tag).is_ok_and(|(ms, _)| ms.is_some()),
        || crate::clash_api::best_pass(&crate::pc_tunnel::regroup_within(p, 3000), &tag).is_some(),
    )
}

/// The pin rule on its own (pin_verdict): each try, the pinned node passing keeps the pin, and another node passing
/// while it fails ends it; when nothing passes, the next try waits `gap`. None after `tries` tries with no answer.
pub fn pin_judge(
    tries: u32,
    gap: Duration,
    mut pin_ok: impl FnMut() -> bool,
    mut other_ok: impl FnMut() -> bool,
) -> Option<bool> {
    for i in 0..tries {
        if i > 0 {
            std::thread::sleep(gap);
        }
        if pin_ok() {
            return Some(true);
        }
        if other_ok() {
            return Some(false);
        }
    }
    None
}

// After a PIA drop: test every node at once, so PIA's next try goes through a working one instead of waiting for the
// group's next test. own: PIA lost the connection itself; carrier: the node its connection went through. When the
// group keeps that node (it answers the quick test but broke PIA), the tunnel holds the best node that passed this
// test for HOLD, then goes back to automatic
fn steer(p: &Paths, own: bool, carrier: Option<String>) {
    use std::sync::atomic::Ordering;
    let mine = HOLDS.fetch_add(1, Ordering::SeqCst) + 1;
    // a node set waits for the PC tunnel while PIA is connected (a restart would drop it). PIA is down now: the restart
    // costs nothing, and the running set may be the reason it dropped
    if crate::node_manager::pending_deploy(p) == Some(true) && core_restart(p) {
        say("PIA drop: the PC tunnel takes the waiting node set now");
        event(p, "restart", "PC tunnel took the waiting node set during a PIA drop");
        return;
    }
    // a pinned node stays while it passes a test of its own. It fails only when another node passes the same test:
    // while PIA reconnects there is no network and every test fails, which says nothing about the pinned node
    if let Some(server) = crate::clash_api::pin_of(p, "pc") {
        match pin_verdict(p, &server, PIN_TRIES, PIN_GAP) {
            Some(true) => {
                say(&format!("PIA drop: the pinned node {server} still passes; the PC tunnel stays on it"));
                return;
            }
            None => {
                say(&format!("PIA drop: no node answers yet; the PC tunnel stays on the pinned node {server}"));
                return;
            }
            Some(false) => {
                crate::clash_api::set_pin(p, "pc", None);
                say(&format!(
                    "PIA drop: the pinned node {server} failed while other nodes pass; the PC tunnel picks its node \
                     automatically again"
                ));
            }
        }
    }
    crate::clash_api::pick(p, "auto"); // a new drop ends any hold
    let tag = |p: &Paths| crate::clash_api::proxies(p).as_ref().and_then(crate::clash_api::now_tag);
    let before = carrier.clone().or_else(|| tag(p));
    let passes = crate::pc_tunnel::regroup_within(p, 3000);
    let after = tag(p);
    if after.is_some() && after != before {
        say("PIA drop: the nodes were tested at once and the PC tunnel moved to a working one");
        return;
    }
    let Some(bad) = after.filter(|_| own) else { return };
    let Some(alt) = crate::clash_api::best_pass(&passes, &bad) else { return };
    if !crate::clash_api::pick(p, &alt) {
        return; // the tunnel runs a config from before 2.1.1: no selector until its next start
    }
    say(&format!("PIA drop: {bad} still passes the quick test; the PC tunnel uses {alt} for 10 minutes"));
    std::thread::sleep(HOLD);
    if HOLDS.load(Ordering::SeqCst) == mine && tag(p).as_deref() == Some(alt.as_str()) {
        crate::clash_api::pick(p, "auto");
        say(&format!("the 10-minute hold on {alt} ended; the PC tunnel picks its node automatically again"));
    }
}
