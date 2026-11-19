//! RealityHop.exe: the tray icon and the background watcher, and the window when it is open, in one process. The
//! logon task starts it with --watch (tray and watcher, no window); a second start (Start menu, --setup, --uninstall)
//! reaches the running process, which opens the window. Closing the window frees its webview; Quit in the tray ends
//! the process. --unpack-to <folder> is 1.7.x's updater asking for the new version: answered without any window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod candidates;
mod dialogs;
mod jobs;
mod start;
mod tray_icon;
mod updates;
mod window_state;
mod wizard;

use engine::app_install;
use engine::paths::Paths;
use engine::translations::tr;
use engine::watcher::{Watch, WatchHooks};
use jobs::Ctx;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;
use window_state::{Job, Win};

/// The tray as last shown: state, tooltip, ring segments.
struct Tray(Mutex<(String, String, Vec<String>)>);

/// The watcher's tray, notifications and update check, through the app.
struct Hooks {
    app: AppHandle,
    exe: PathBuf,
}

impl WatchHooks for Hooks {
    // A notification from the watcher
    fn notify(&self, text: &str) {
        let _ = self.app.notification().builder().title(engine::NAME).body(text).show();
    }

    // The tray icon and tooltip, and the same state to the window
    fn show(&self, state: &str, tip: &str, segs: &[String]) {
        show_tray(&self.app, state, tip, segs);
    }

    // The window does the checks while it is open
    fn window_open(&self) -> bool {
        self.app.get_webview_window("main").is_some()
    }

    // Every 6 hours: a newer release is installed (auto-update on, window closed) or announced. True when this process
    // must end: the new version starts once it has
    fn app_update(&self) -> bool {
        let ctx = self.app.state::<Ctx>();
        if cfg!(debug_assertions) || !app_install::installed_here(&self.exe) {
            return false; // a development build or a copy outside the install folder never updates itself
        }
        let paths = &ctx.paths;
        let up = match app_install::check_update(paths) {
            Ok(Some(u)) => u,
            Ok(None) => return false,
            Err(e) => {
                engine::activity_log::say(&format!("app update check failed: {e}"));
                return false;
            }
        };
        let rel = up.latest.clone();
        ctx.win.lock().unwrap_or_else(|e| e.into_inner()).update = Some(up);
        let auto = engine::settings::Settings::read(paths).is_ok_and(|s| s.auto_update != 0);
        if !auto || self.window_open() {
            self.notify(&tr(
                "Reality Hop {ver} is available; open Reality Hop to install it.",
                &[("ver", rel.ver.clone())],
            ));
            return false;
        }
        engine::activity_log::say(&format!("installing Reality Hop {}", rel.ver));
        if let Err(e) = app_install::apply_update(paths, &rel, &self.exe) {
            engine::activity_log::say(&format!("app update failed: {e}"));
            return false;
        }
        let _ = std::process::Command::new(&self.exe).args(["--watch", "--after-update"]).spawn();
        self.app.exit(0);
        true
    }
}

// Show a state in the tray and tell an open window
pub(crate) fn show_tray(app: &AppHandle, state: &str, tip: &str, segs: &[String]) {
    *app.state::<Tray>().0.lock().unwrap_or_else(|e| e.into_inner()) = (state.into(), tip.into(), segs.to_vec());
    let _ = app.emit("tray", json!({"state": state, "tip": tip, "segs": segs}));
    let (h, state, tip, segs) = (app.clone(), state.to_string(), tip.to_string(), segs.to_vec());
    let _ = app.run_on_main_thread(move || {
        if let Some(t) = h.tray_by_id("main") {
            let s = tray_icon::SIZE;
            let _ = t.set_icon(Some(Image::new_owned(tray_icon::draw(&state, &segs), s, s)));
            let _ = t.set_tooltip(Some(&tip));
        }
    });
}

// Open the window, or bring it to the front; page: "" or the part to show first ("setup", "uninstall")
fn open_window(app: &AppHandle, page: &str) {
    if let Some(w) = app.get_webview_window("main") {
        if !page.is_empty()
            && let Ok(url) = w.url()
            && let Ok(to) = url.join(&page_url(page))
        {
            let _ = w.navigate(to); // a second start asked for setup or uninstall while the window is open
        }
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let ctx = app.state::<Ctx>();
    let exe = std::env::current_exe().unwrap_or_default();
    let is_setup = engine::setup_steps::is_setup(&ctx.paths);
    // set up already, with this copy from the Downloads folder: the window asks to replace the installed copy
    let replaces =
        app_install::installed_exe(&exe).exists() || app_install::legacy_dir().join(app_install::EXE_NAME).exists();
    let setup_due = !is_setup || (!cfg!(debug_assertions) && app_install::needs_install(&exe) && !replaces);
    let page = if page.is_empty() && setup_due { "setup" } else { page };
    let setup = page.starts_with("setup");
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(page_url(page).into()))
        .title(engine::NAME)
        .inner_size(if setup { 1120.0 } else { 1440.0 }, if setup { 800.0 } else { 1000.0 })
        .min_inner_size(if setup { 1000.0 } else { 1060.0 }, if setup { 700.0 } else { 560.0 })
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::window::Color(12, 12, 14, 255)) // app.css --bg: no white while WebView2 starts
        .build();
}

// The file and part for a page
fn page_url(page: &str) -> String {
    match page.strip_prefix("setup") {
        Some(step) => format!("setup.html#{}", step.trim_start_matches(':')),
        None if page.is_empty() => "index.html".to_string(),
        None => format!("index.html#{page}"),
    }
}

// The page a command line asks for
fn page_of(args: &[String]) -> &'static str {
    if args.iter().any(|a| a == "--uninstall") {
        "uninstall"
    } else if args.iter().any(|a| a == "--setup") {
        if args.iter().any(|a| a == "--router") {
            "setup:ronly"
        } else if args.windows(2).any(|w| w[0] == "--step" && w[1] == "router") {
            "setup:router"
        } else {
            "setup"
        }
    } else {
        ""
    }
}

// Everything the window draws
#[tauri::command]
fn snapshot(ctx: State<Ctx>, tray: State<Tray>) -> Value {
    let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = window_state::snapshot(&ctx.paths, &w);
    let t = tray.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    s["tray"] = json!({"state": t.0, "tip": t.1, "segs": t.2});
    s
}

// Start an action from the window
#[tauri::command]
fn act(app: AppHandle, name: String, args: Option<Value>) {
    jobs::work(&app, Job { name, args: args.unwrap_or(Value::Null) });
}

// Cancel the action that runs
#[tauri::command]
fn cancel(app: AppHandle) {
    jobs::cancel(&app);
}

// The automation switches: check every N minutes (kept in Settings), auto-repair
#[tauri::command]
fn set_auto(app: AppHandle, ctx: State<Ctx>, on: bool, fix: bool, minutes: Option<i64>) -> Result<(), String> {
    {
        let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
        w.auto_on = on;
        w.auto_fix = fix;
    }
    if let Some(m) = minutes {
        let mut c = Map::new();
        c.insert("check_min".into(), json!(m.clamp(15, 1440)));
        engine::settings::Settings::save(&ctx.paths, &c).map_err(|e| e.to_string())?;
    }
    jobs::changed(&app);
    Ok(())
}

// Open the data folder or the logs folder in Explorer or Finder
#[tauri::command]
fn open_folder(ctx: State<Ctx>, which: String) {
    let p = match which.as_str() {
        "logs" => ctx.paths.logs.clone(),
        "reports" => ctx.paths.reports.clone(),
        _ => ctx.paths.data.clone(),
    };
    let tool =
        if cfg!(windows) { engine::process::system_tool("explorer").into_os_string() } else { "/usr/bin/open".into() };
    let _ = std::process::Command::new(tool).arg(p).spawn();
}

// Open a web page in the default browser: only GitHub pages (Reality Hop's own and the projects it names)
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    // explorer.exe reads a comma as a switch separator (/root, /select)
    if !url.starts_with("https://github.com/") || url.contains(['"', ' ', '&', '^', '|', '%', ',']) {
        return Err("not a link Reality Hop opens".into());
    }
    let tool =
        if cfg!(windows) { engine::process::system_tool("explorer").into_os_string() } else { "/usr/bin/open".into() };
    std::process::Command::new(tool).arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// A text in the person's language, {name} fields filled from kw
#[tauri::command]
fn text(s: String, kw: Option<Map<String, Value>>) -> String {
    let kw: Vec<(String, String)> =
        kw.unwrap_or_default().into_iter().map(|(k, v)| (k, engine::python_json::py_str(&v))).collect();
    let kw: Vec<(&str, String)> = kw.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    tr(&s, &kw)
}

// Open the Candidates dialog: a fresh ping and test (Candidates.__init__)
#[tauri::command]
fn cand_open(app: AppHandle, ctx: State<Ctx>) {
    {
        let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
        w.cand = candidates::Cand {
            open: true,
            msg: tr("Downloading the node list and pinging from this PC...", &[]),
            ..Default::default()
        };
    }
    jobs::work(&app, window_state::Job { name: "cand_scan".into(), args: Value::Null });
}

// Close it; its queued scans and tests are dropped (Candidates.close)
#[tauri::command]
fn cand_close(app: AppHandle, ctx: State<Ctx>) {
    let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    w.cand.open = false;
    w.jobs.retain(|j| j.name != "cand_scan" && j.name != "cand_test_tunnel");
    drop(w);
    jobs::changed(&app);
}

// What the Candidates dialog shows
#[tauri::command]
fn cand_view(ctx: State<Ctx>) -> Value {
    let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    candidates::view(&ctx.paths, &w)
}

// Settings the window needs to lay itself out
#[tauri::command]
fn top(ctx: State<Ctx>) -> i64 {
    engine::settings::Settings::read(&ctx.paths).map_or(50, |s| s.top)
}

// The tray icon with its menu (the same ids as 1.7.x: 1 open, 4 restart, 2 check, 3 rescue, 9 quit)
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let h = app.handle();
    let item = |id: &str, text: &str| MenuItem::with_id(h, id, tr(text, &[]), true, None::<&str>);
    let menu = Menu::with_items(
        h,
        &[
            &item("1", "Open Reality Hop")?,
            &item("4", "Restart PC tunnel")?,
            &item("2", "Check now")?,
            &item("3", "Rescue from verified vault")?,
            &PredefinedMenuItem::separator(h)?,
            &item("9", "Quit background watcher")?,
        ],
    )?;
    let s = tray_icon::SIZE;
    TrayIconBuilder::with_id("main")
        .icon(Image::new_owned(tray_icon::draw("unk", &[]), s, s))
        .tooltip(engine::NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| {
            let watch = Arc::clone(&app.state::<Ctx>().watch);
            match ev.id().as_ref() {
                "1" => open_window(app, ""),
                "2" => watch.ask("check"),
                "3" => watch.ask("rescue"),
                "4" => watch.ask("restart"),
                "9" => {
                    engine::activity_log::say("watcher stopped from the tray menu");
                    watch.stop();
                    engine::watcher::pia_feed_stop();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = ev {
                open_window(tray.app_handle(), "");
            }
        })
        .build(h)?;
    Ok(())
}

// Whether this process runs the watcher. A development build never does on its own: the watcher starts rh-core, asks
// PIA to change its split tunnel and stops stray PIA monitors, which a build under test must not do to a real setup
pub(crate) fn runs_watcher() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("REALITYHOP_DEV_WATCH").is_some()
}

// Wait up to 30 s until the previous process has let go of the watcher's port (a restart after an update)
fn wait_for_previous() {
    for _ in 0..60 {
        if std::net::TcpListener::bind(("127.0.0.1", engine::watcher::WATCH_PORT)).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

// Answer --unpack-to without any window, or start the app
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().unwrap_or_default();
    if let Some(i) = args.iter().position(|a| a == "--unpack-to") {
        let Some(dir) = args.get(i + 1) else { return std::process::ExitCode::from(app_install::NOT_UNPACKED) };
        return std::process::ExitCode::from(app_install::unpack_to(&exe, std::path::Path::new(dir)));
    }
    if args.iter().any(|a| a == "--after-update") {
        wait_for_previous();
    }
    let paths = Paths::from_env().unwrap_or_else(|| Paths::new(&app_install::data_dir()));
    let lang = engine::settings::Settings::read(&paths).map(|s| s.lang).unwrap_or_default();
    engine::translations::use_lang(&lang);
    if !cfg!(windows) && !cfg!(debug_assertions) && engine::macos_system::misplaced(&exe) {
        // a login item must not point into the disk image or a quarantine copy; nothing is written
        let msg = tr("Drag Reality Hop into the Applications folder first, then open it from there.", &[]);
        engine::macos_system::alert(engine::NAME, &msg);
        return std::process::ExitCode::SUCCESS;
    }
    if app_install::installed_here(&exe) && !cfg!(debug_assertions) {
        app_install::cleanup(&exe); // old exes and 1.x program folders
        app_install::entry_fix(&exe);
        if !cfg!(windows) {
            engine::macos_system::start_fix(&paths, &exe);
        }
    }
    let watch = Watch::new();
    let mut win = Win::new();
    window_state::load_last(&paths, &mut win);
    let cmd = vec![exe.to_string_lossy().into_owned(), "--watch".into()];
    let ctx = Ctx { paths: paths.clone(), win: Arc::new(Mutex::new(win)), watch: Arc::clone(&watch), cmd: cmd.clone() };
    let background = args.iter().any(|a| a == "--watch");
    let logon = args.iter().any(|a| a == "--logon");
    let first_page = page_of(&args);
    // a copy from the Downloads folder runs on its own next to the installed one: it installs over it, or is the setup
    let installer = !cfg!(debug_assertions) && app_install::needs_install(&exe);
    let mut builder = tauri::Builder::default();
    if !installer {
        builder =
            builder.plugin(tauri_plugin_single_instance::init(|app, argv, _| open_window(app, page_of(&argv[1..]))));
    }
    let app = builder
        .plugin(tauri_plugin_notification::init())
        .manage(ctx)
        .manage(Tray(Mutex::new(("unk".into(), engine::NAME.into(), vec![]))))
        .invoke_handler(tauri::generate_handler![
            snapshot,
            act,
            cancel,
            set_auto,
            open_folder,
            open_url,
            text,
            top,
            cand_open,
            cand_close,
            cand_view,
            dialogs::node_details,
            dialogs::settings_get,
            dialogs::sb_versions,
            dialogs::settings_save,
            dialogs::check_updates,
            dialogs::uninstall,
            dialogs::quit,
            dialogs::report_make,
            dialogs::router_forget,
            dialogs::report_issue,
            dialogs::devices_get,
            dialogs::lan_ip,
            dialogs::devices_save,
            dialogs::open_router_panel,
            updates::window_opened,
            updates::update_info,
            updates::rcore_info,
            updates::rcore_later,
            wizard::wz_info,
            wizard::wz_view,
            wizard::wz_lang,
            wizard::wz_france,
            wizard::wz_mode,
            wizard::wz_add_key,
            wizard::wz_own_key,
            wizard::wz_check_pia,
            wizard::wz_relaunch,
            wizard::wz_act,
            wizard::wz_note_clear,
            start::start_asks,
            start::start_answer
        ])
        .setup(move |app| {
            jobs::log_sink(app.handle(), &paths);
            build_tray(app)?;
            jobs::ticker(app.handle().clone());
            let h = app.handle().clone();
            std::thread::spawn(move || {
                let on = !matches!(engine::startup_tasks::task_state(), None | Some("Disabled"));
                h.state::<Ctx>().win.lock().unwrap_or_else(|e| e.into_inner()).boot_on = on;
                jobs::changed(&h);
            });
            if !background {
                open_window(app.handle(), first_page);
            }
            if runs_watcher() && !installer {
                let (h, paths, watch_exe) = (app.handle().clone(), paths.clone(), exe.clone());
                std::thread::spawn(move || {
                    let Some(_lock) = engine::watcher::watch_lock() else {
                        engine::activity_log::say("the watcher is already running");
                        return;
                    };
                    let hooks = Hooks { app: h, exe: watch_exe };
                    engine::watcher::watch_loop(&paths, &watch, &hooks, &cmd, logon);
                });
                if app_install::installed_here(&exe) {
                    let exe = exe.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(15)); // Windows lists the icon a moment after
                        engine::windows_system::tray_promote(&exe);
                    });
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Reality Hop could not start: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // closing the window keeps the tray and the watcher; only Quit ends the process. The installer copy just ends
    app.run(move |_, event| {
        if let RunEvent::ExitRequested { api, code: None, .. } = event
            && !installer
        {
            api.prevent_exit();
        }
    });
    std::process::ExitCode::SUCCESS
}
