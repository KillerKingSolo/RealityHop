//! What the window does once it opens: show the sing-box versions, look for a new Reality Hop release (UpdateDialog),
//! keep sing-box current on its channel, and offer the router build of sing-box (RouterCoreDialog). Nothing installs
//! unasked.

use crate::jobs::{self, Ctx};
use crate::window_state::{Job, Mode, Worker};
use engine::activity_log::say;
use engine::router_actions as ra;
use engine::router_ssh::RouterSsh;
use engine::translations::{ltr, tr};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

static ROUTER_LOOK: AtomicBool = AtomicBool::new(false);

/// The changelog this build ships with: what the window shows after an update, with no download.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

// The router's sing-box under the PC's; GL.iNet's package says so (show_router_core)
fn rcore_text(st: &Value) -> Option<String> {
    let ver = st.get("ver").and_then(Value::as_str).filter(|v| !v.is_empty())?;
    let pkg = if st.get("mark").is_some_and(|m| !m.is_null()) {
        String::new()
    } else {
        format!(" · {}", tr("GL.iNet package", &[]))
    };
    Some(format!("{}: {}{pkg}", tr("Router", &[]), ltr(ver)))
}

fn set_rcore(ctx: &Ctx) {
    let st = engine::settings::this_pc(&ctx.paths).get("router_core").cloned().unwrap_or_default();
    if let Some(t) = rcore_text(&st) {
        ctx.win.lock().unwrap_or_else(|e| e.into_inner()).rcore_text = t;
    }
}

// The window opened: versions now, the update check, sing-box after 30 s, the router build after 45 s and every 6 hours
#[tauri::command]
pub fn window_opened(app: AppHandle) {
    let h = app.clone();
    std::thread::spawn(move || {
        let ctx = h.state::<Ctx>();
        let mode = Mode::of(&ctx.paths);
        if !mode.ronly {
            let ver = engine::pc_tunnel::core_version(&ctx.paths);
            let text = match ver {
                Some(v) if mode.router => format!("sing-box\n{}: {}", tr("PC", &[]), ltr(&v)),
                Some(v) => format!("sing-box {}", ltr(&v)),
                None => tr("sing-box: not installed", &[]),
            };
            ctx.win.lock().unwrap_or_else(|e| e.into_inner()).core_text = text;
        }
        if mode.router {
            set_rcore(&ctx);
            jobs::changed(&h);
            if ra::router_core_state(&ctx.paths, &mut RouterSsh { paths: &ctx.paths }).is_ok() {
                set_rcore(&ctx); // out of reach: the last known version stays
            }
        }
        jobs::changed(&h);
        find_update(&h);
    });
    let h = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        maybe_core_update(&h);
    });
    if !ROUTER_LOOK.swap(true, Ordering::SeqCst) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(45));
            loop {
                if app.get_webview_window("main").is_some() {
                    maybe_router_core(&app, false);
                }
                std::thread::sleep(Duration::from_secs(6 * 3600));
            }
        });
    }
}

// A newer Reality Hop: the Update button, and the dialog at once (find_update, do_update)
fn find_update(app: &AppHandle) {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(debug_assertions) || !engine::app_install::installed_here(&exe) {
        return; // a development build or a copy outside the install folder never updates itself
    }
    let ctx = app.state::<Ctx>();
    if let Ok(Some(up)) = engine::app_install::check_update(&ctx.paths) {
        ctx.win.lock().unwrap_or_else(|e| e.into_inner()).update = Some(up);
        jobs::changed(app);
        std::thread::sleep(Duration::from_millis(1500));
        let _ = app.emit("offer", "update");
    } // no internet or GitHub down: the watcher tries again tomorrow
}

// sing-box keeps itself current on its own channel: daily (the watcher does it when it runs) and at once after a
// channel change (maybe_core_update)
fn maybe_core_update(app: &AppHandle) {
    let ctx = app.state::<Ctx>();
    if engine::settings::stopped(&ctx.paths) {
        return; // stopped, or being uninstalled: no download into a folder about to go
    }
    let channel = engine::settings::Settings::read(&ctx.paths).map(|s| s.singbox).unwrap_or_default();
    let changed = engine::settings::this_pc(&ctx.paths).get("core_channel").and_then(Value::as_str) != Some(&channel);
    if engine::pc_tunnel::core_update_due(&ctx.paths) && (changed || !crate::runs_watcher()) {
        jobs::work(app, Job { name: "core_update".into(), args: Value::Null });
    }
}

// The router build of sing-box: once a day, and at once after the router's channel changes; asks, never installs
// unasked (maybe_router_core)
pub fn maybe_router_core(app: &AppHandle, force: bool) {
    let ctx = app.state::<Ctx>();
    if !Mode::of(&ctx.paths).router || !(force || ra::router_core_due(&ctx.paths)) {
        return;
    }
    if ctx.win.lock().unwrap_or_else(|e| e.into_inner()).rc_offer.is_some() {
        return;
    }
    let offer = ra::router_core_offer(&ctx.paths, &mut RouterSsh { paths: &ctx.paths });
    set_rcore(&ctx);
    jobs::changed(app);
    match offer {
        Ok(Some(o)) => {
            ctx.win.lock().unwrap_or_else(|e| e.into_inner()).rc_offer = Some(o);
            let _ = app.emit("offer", "rcore");
        }
        Ok(None) => {}
        Err(e) => say(&format!("router sing-box check failed: {e}")), // router or GitHub out of reach: tomorrow again
    }
}

// What changed in every version since this one (UpdateDialog)
#[tauri::command]
pub fn update_info(ctx: State<Ctx>) -> Option<Value> {
    let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    let u = w.update.as_ref()?;
    let changes = if u.changes.is_empty() {
        vec![(u.latest.ver.clone(), String::new(), u.latest.notes.clone())]
    } else {
        u.changes.clone()
    };
    Some(json!({"ver": u.latest.ver, "have": engine::VERSION, "changes": changes, "repo": engine::REPO}))
}

// What changed since the version last opened here, once after each update (WhatsNew). None in a development build
// and in a copy outside the install folder
#[tauri::command]
pub fn whats_new(ctx: State<Ctx>) -> Option<Value> {
    let exe = std::env::current_exe().unwrap_or_default();
    if !cfg!(debug_assertions) && !engine::app_install::installed_here(&exe) {
        return None;
    }
    let changes = engine::app_install::whats_new(&ctx.paths, CHANGELOG);
    (!changes.is_empty()).then(|| json!({"ver": engine::VERSION, "changes": changes, "repo": engine::REPO}))
}

// The router build on offer: what the router runs, what it would get, the download size (RouterCoreDialog)
#[tauri::command]
pub fn rcore_info(ctx: State<Ctx>) -> Option<Value> {
    let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    let (st, m, a) = w.rc_offer.as_ref()?;
    let ch = |c: &str| tr(if c == "stable" { "Stable" } else { "Test builds (beta, alpha)" }, &[]);
    let build = |c: &str| tr("Reality Hop build, {channel}", &[("channel", ch(c))]);
    let now = match st.mark.as_ref().and_then(|k| k.first()) {
        Some(c) => build(c),
        None => tr("GL.iNet package", &[]),
    };
    let text = |k: &str| m[k].as_str().unwrap_or_default().to_string();
    let rows = [
        (tr("On the router now", &[]), format!("sing-box {}  ·  {now}", ltr(st.ver.as_deref().unwrap_or_default()))),
        (tr("New", &[]), format!("sing-box {}  ·  {}", ltr(&text("singbox")), build(&text("channel")))),
        (tr("Download", &[]), format!("{:.1} MB", a["size"].as_f64().unwrap_or(0.0) / 1e6)),
    ];
    Some(json!({"rows": rows, "repo": engine::REPO}))
}

// Later: not again within 3 days for this build (RouterCoreDialog.later)
#[tauri::command]
pub fn rcore_later(ctx: State<Ctx>) {
    let offer = ctx.win.lock().unwrap_or_else(|e| e.into_inner()).rc_offer.take();
    if let Some((_, m, _)) = offer {
        ra::router_core_later(&ctx.paths, &m);
    }
}

impl Worker<'_> {
    // Install the router build on offer (router_core_install)
    pub fn router_core_install(&self) -> Result<(), String> {
        let (_, m, a) = self.w().rc_offer.take().ok_or("no router build on offer")?;
        let (ok, out) = ra::router_core_install(self.paths, &mut RouterSsh { paths: self.paths }, &m, &a)?;
        for line in out.lines() {
            say(&format!("router: {line}"));
        }
        let ver = ltr(m["singbox"].as_str().unwrap_or_default());
        let (text, state) = if ok {
            (tr("The router runs sing-box {ver} now.", &[("ver", ver)]), "ok")
        } else {
            (tr("The router's sing-box was not updated; it runs as before. See the activity log.", &[]), "warn")
        };
        {
            let mut w = self.w();
            w.status_text = text;
            w.status_state = state.into();
            let st = engine::settings::this_pc(self.paths).get("router_core").cloned().unwrap_or_default();
            if let Some(t) = rcore_text(&st) {
                w.rcore_text = t;
            }
        }
        if ok {
            self.check_local("phone");
        }
        Ok(())
    }
}

// Install the new Reality Hop, then start again on it; the tunnel keeps running (apply_update)
pub fn app_update(app: &AppHandle, ctx: &Ctx) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(debug_assertions) || !engine::app_install::installed_here(&exe) {
        return Err("a development build never updates itself".into());
    }
    let rel = ctx.win.lock().unwrap_or_else(|e| e.into_inner()).update.as_ref().map(|u| u.latest.clone());
    let rel = rel.ok_or("no update found")?;
    say(&format!("installing Reality Hop {}", rel.ver));
    engine::app_install::apply_update(&ctx.paths, &rel, &exe)?;
    engine::app_install::mark_seen(&ctx.paths, &rel.ver); // its notes were in the Update dialog
    std::process::Command::new(&exe).arg("--after-update").spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}
