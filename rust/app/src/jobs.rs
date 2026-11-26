//! The window's actions: one at a time on a worker thread, the rest queued in order, each once; Cancel stops what
//! runs; an action over 6 minutes is cancelled (App.work, done, cancel, watchdog). The activity log goes to the
//! window and to logs/window.log (actions) or logs/watch.log (the watcher).

use crate::window_state::{Job, Shared, Worker, save_last};
use engine::activity_log::{FileLog, say};
use engine::paths::Paths;
use engine::translations::tr;
use serde_json::{Value, json};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

thread_local! {
    // true on the window's worker thread: its lines go to window.log
    static WINDOW_JOB: Cell<bool> = const { Cell::new(false) };
}

/// What the jobs need from the app.
pub struct Ctx {
    pub paths: Paths,
    pub win: Arc<Shared>,
    pub watch: Arc<engine::watcher::Watch>,
    pub cmd: Vec<String>,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
// Cancel pressed while the action still waits for the watcher's check: it is dropped when its turn comes
static DROP_WAITING: AtomicBool = AtomicBool::new(false);
// Where the data went when the setup installed this copy: the logs and last.json follow it
static MOVED: Mutex<Option<Paths>> = Mutex::new(None);

// Send say() to the window and to the right log file
pub fn log_sink(app: &AppHandle, paths: &Paths) {
    let _ = APP.set(app.clone());
    let (window, watch) = (FileLog::new(&paths.logs, "window"), FileLog::new(&paths.logs, "watch"));
    let h = app.clone();
    engine::activity_log::set_say(move |line| {
        if WINDOW_JOB.with(Cell::get) {
            window.write(line)
        } else {
            watch.write(line)
        }
        let at = engine::local_time::now().format("%H:%M:%S");
        let _ = h.emit("log", json!({"at": at, "line": engine::translations::cap_first(line)}));
    });
}

// The setup moved the data folder (install): log and keep results there from now on. Lines logged between the move
// and now went to the old folder: they are carried over, and the old folder goes once it holds no settings
pub fn data_moved(from: &Paths, to: &Paths) {
    if let Some(app) = APP.get() {
        log_sink(app, to);
    }
    *MOVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(to.clone());
    if from.data == to.data || from.data.join("settings.json").exists() {
        return;
    }
    for name in ["window.log", "watch.log"] {
        if let Ok(t) = std::fs::read_to_string(from.logs.join(name))
            && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(to.logs.join(name))
        {
            let _ = std::io::Write::write_all(&mut f, t.as_bytes());
        }
    }
    let _ = std::fs::remove_dir_all(&from.data);
}

// Change the window's state from inside an action (a test's progress) and draw it again
pub fn update_win(f: impl FnOnce(&mut crate::window_state::Win)) {
    if let Some(app) = APP.get() {
        let ctx = app.state::<Ctx>();
        f(&mut ctx.win.lock().unwrap_or_else(|e| e.into_inner()));
        changed(app);
    }
}

// Tell the window to draw again
pub fn changed(app: &AppHandle) {
    let _ = app.emit("changed", ());
}

// Run an action now, or queue it behind the one that runs (work)
pub fn work(app: &AppHandle, job: Job) {
    let ctx = app.state::<Ctx>();
    {
        let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
        if w.busy {
            if w.current.as_ref() != Some(&job) && !w.jobs.contains(&job) {
                w.jobs.push(job);
                drop(w);
                say("queued; it runs when the current action finishes");
            }
            return;
        }
        w.busy = true;
        w.started = None; // set once it has the work turn: waiting for the watcher does not count
        w.current = Some(job.clone());
        DROP_WAITING.store(false, Ordering::SeqCst);
    }
    changed(app);
    let h = app.clone();
    std::thread::spawn(move || {
        WINDOW_JOB.with(|c| c.set(true));
        let ctx = h.state::<Ctx>();
        let turn = engine::process::try_work().unwrap_or_else(|| {
            say("waiting for the background check to finish...");
            engine::process::work()
        });
        let dropped = {
            let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
            let d = DROP_WAITING.swap(false, Ordering::SeqCst);
            if !d {
                w.started = Some(Instant::now());
            }
            d
        };
        changed(&h); // the busy bar starts filling
        if dropped {
            say("cancelled");
            drop(turn);
            return done(&h);
        }
        let t = Instant::now();
        match run(&h, &ctx, &job) {
            Ok(()) if engine::process::cancelled() => say("cancelled"),
            Ok(()) => say(&format!("done ({:.0} s)", t.elapsed().as_secs_f64())),
            Err(e) => {
                say(&format!("error: {e}"));
                let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
                w.status_text = tr("Something failed: {e}", &[("e", e)]);
                w.status_state = "bad".into();
            }
        }
        drop(turn);
        done(&h);
    });
}

// The action finished: free the window, keep the results, start the next queued one (done)
fn done(app: &AppHandle) {
    let ctx = app.state::<Ctx>();
    let next = {
        let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
        w.busy = false;
        // a cancelled action says nothing about how long the next one takes
        if let (Some(j), Some(s)) = (w.current.take(), w.started)
            && !engine::process::cancelled()
        {
            w.took.insert(j.name, s.elapsed().as_secs_f64());
        }
        let moved = MOVED.lock().unwrap_or_else(|e| e.into_inner()).clone();
        save_last(moved.as_ref().unwrap_or(&ctx.paths), &w);
        tray_follows(app, &ctx.paths, &w);
        (!w.jobs.is_empty()).then(|| w.jobs.remove(0))
    };
    changed(app);
    if let Some(j) = next {
        work(app, j);
    }
}

// The tray shows what the window found: the same ring, one segment per node (set_icon)
fn tray_follows(app: &AppHandle, paths: &Paths, w: &crate::window_state::Win) {
    let mode = crate::window_state::Mode::of(paths);
    let slow = engine::settings::Settings::read(paths).map_or(200, |s| s.slow_ms) as f64;
    let mut segs = w.segs(&mode, slow);
    if w.status_state == "unk" || segs.iter().all(|s| s == "unk") {
        return;
    }
    let ok = segs.iter().filter(|s| *s != "bad" && *s != "unk").count();
    // out of all 6 slots: an empty slot shows as a dead segment
    segs.resize(segs.len().max(engine::node_manager::SLOTS), "bad".into());
    let text = tr("{ok} of {n} nodes working", &[("ok", ok.to_string()), ("n", segs.len().to_string())]);
    let at = engine::local_time::now().format("%H:%M");
    let st = engine::watcher::WatchState { last: Some((w.status_state.clone(), text, at)), segs, ..Default::default() };
    let (state, tip) = engine::watcher::tray_tip(&st, 0, engine::pc_tunnel::core_mode(paths));
    let _ = engine::data_files::jsave(
        &paths.tray_last,
        &json!({"last": st.last.as_ref().map(|l| [&l.0, &l.1, &l.2]), "segs": st.segs}),
        None,
    );
    crate::show_tray(app, &state, &tip, &st.segs);
}

// Cancel what runs and drop the queue (cancel)
pub fn cancel(app: &AppHandle) {
    let ctx = app.state::<Ctx>();
    let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    if w.busy {
        let n = std::mem::take(&mut w.jobs).len();
        // still waiting for the watcher's check: it never starts, and the check runs on
        let waiting = w.started.is_none();
        if waiting {
            DROP_WAITING.store(true, Ordering::SeqCst);
        }
        drop(w);
        say("cancelling the current action...");
        if n > 0 {
            say(&format!("dropped {n} queued action(s)"));
        }
        if !waiting {
            engine::process::cancel();
        }
    }
}

// Nothing may keep the window busy: an action over 6 minutes is cancelled (watchdog). And the automatic check every
// check_min minutes while the window is open (auto_tick)
pub fn ticker(app: AppHandle) {
    std::thread::spawn(move || {
        let mut next_auto: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let ctx = app.state::<Ctx>();
            let (busy_long, auto_on) = {
                let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
                // testing every candidate takes longer: 30 minutes
                let limit = if w.current.as_ref().is_some_and(|j| j.name == "cand_all") { 1800 } else { 360 };
                (w.busy && w.started.is_some_and(|s| s.elapsed() > Duration::from_secs(limit)), w.auto_on)
            };
            if busy_long {
                say("the action ran too long; cancelled");
                engine::process::cancel();
                let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
                w.status_text = tr("The last action took too long and was cancelled.", &[]);
                w.status_state = "bad".into();
                w.jobs.clear();
            }
            if app.get_webview_window("main").is_none() {
                next_auto = None; // the first check runs when the window opens; the next one check_min later
                continue;
            }
            let mins = engine::settings::Settings::read(&ctx.paths).map_or(60, |s| s.check_min).max(15) as u64;
            let due = next_auto.get_or_insert_with(|| Instant::now() + Duration::from_secs(mins * 60));
            if Instant::now() >= *due {
                *due = Instant::now() + Duration::from_secs(mins * 60);
                if auto_on && !engine::settings::stopped(&ctx.paths) {
                    work(&app, Job { name: "auto_run".into(), args: Value::Null });
                }
            }
            let at =
                engine::local_time::at(now() as i64 + due.saturating_duration_since(Instant::now()).as_secs() as i64);
            let text = if engine::settings::stopped(&ctx.paths) {
                tr("Stopped", &[])
            } else if auto_on {
                tr("Next auto-check {at}", &[("at", at.format("%H:%M"))])
            } else {
                String::new()
            };
            ctx.win.lock().unwrap_or_else(|e| e.into_inner()).next_auto = text;
        }
    });
}

// time.time()
fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// One action
fn run(app: &AppHandle, ctx: &Ctx, job: &Job) -> Result<(), String> {
    let wk = Worker { paths: &ctx.paths, win: &ctx.win };
    let arg = |k: &str| job.args.get(k).cloned().unwrap_or(Value::Null);
    let (watch, h) = (Arc::clone(&ctx.watch), app.clone());
    // Start and Settings switch the background watcher in this process too, not only at logon
    let watcher = move |on: bool| {
        if on {
            crate::start_watcher(&h, false);
        } else {
            watch.stop();
        }
    };
    match job.name.as_str() {
        "check" => wk.first_check(),
        "check_local" => {
            wk.check_local("both");
            Ok(())
        }
        "test_router" => wk.check_router(),
        "restart_pc" => {
            wk.restart_pc();
            Ok(())
        }
        "push_router" => wk.push_router(),
        "pin" => {
            let who = arg("who").as_str().unwrap_or("").to_string();
            wk.pin(&who, arg("server").as_str())
        }
        "fix" => {
            let slot = arg("slot").as_u64().ok_or("no slot")? as usize;
            wk.fix(slot, arg("how").as_str().unwrap_or("online"))
        }
        "replace_all" => wk.replace_all(),
        "rescue" => wk.rescue(),
        "auto_run" => wk.auto_run(),
        "power" => {
            wk.power(arg("stop") == true, &ctx.cmd, watcher);
            Ok(())
        }
        "set_boot" => {
            let on = arg("on") == true;
            let ok = engine::startup_tasks::boot_task(&ctx.paths, on);
            ctx.win.lock().unwrap_or_else(|e| e.into_inner()).boot_on = if ok { on } else { !on };
            Ok(())
        }
        "set_startup" => {
            engine::startup_tasks::startup(&ctx.paths, arg("on") == true, &ctx.cmd, watcher);
            Ok(())
        }
        "core_update" => {
            engine::settings::set_this_pc(
                &ctx.paths,
                json!({"core_checked": now()}).as_object().cloned().unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
            match engine::pc_tunnel::core_update(&ctx.paths, false) {
                Ok(Some(ver)) => {
                    wk.check_local("pc");
                    let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
                    w.status_text = tr("sing-box {ver} installed; the tunnel is running on it.", &[("ver", ver)]);
                    w.status_state = "ok".into();
                }
                Ok(None) => {}
                Err(e) => say(&format!("sing-box update check failed: {e}")),
            }
            Ok(())
        }
        "cand_scan" => {
            wk.cand_ping()?;
            changed(app);
            wk.cand_test()
        }
        "cand_test" => wk.cand_test(),
        "cand_more" => wk.cand_more(),
        "cand_all" => wk.cand_all(),
        "cand_test_tunnel" => {
            wk.cand_test_tunnel();
            Ok(())
        }
        "cand_put" => {
            let picks: Vec<(usize, String)> = arg("picks")
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some((p.get(0)?.as_u64()? as usize, p.get(1)?.as_str()?.to_string())))
                .collect();
            wk.cand_put(&picks)
        }
        "cand_best" => wk.cand_best(),
        "devices_push" => wk.devices_push(),
        "router_core_install" => wk.router_core_install(),
        "app_update" => crate::updates::app_update(app, ctx),
        n if n.starts_with("wz_") => wk.wizard(n, &job.args, &ctx.cmd),
        other => Err(format!("unknown action: {other}")),
    }
}
