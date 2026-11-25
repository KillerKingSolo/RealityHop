//! The commands behind the window's dialogs: node details, Settings (with the sing-box channels, the update check and
//! uninstall), the problem report (NodeDetails, SettingsDialog, ReportDialog).

use crate::jobs::{self, Ctx};
use crate::window_state::{Job, Mode};
use engine::settings::{self, Settings};
use engine::translations::{LANGS, tr};
use serde_json::{Map, Value, json};
use tauri::{AppHandle, State};

// "{v} ms", or none
fn ms(v: Option<&Value>, none: &str) -> String {
    match v.filter(|v| !v.is_null()) {
        Some(v) => format!("{} ms", engine::python_json::py_str(v)),
        None => none.to_string(),
    }
}

// A field as text, "" when missing
fn f(n: &Value, k: &str) -> String {
    n.get(k).filter(|v| !v.is_null()).map(engine::python_json::py_str).unwrap_or_default()
}

// One node: where it is, how it tests, how often it passed, and its tunnel ping over the last 24 hours (NodeDetails)
#[tauri::command]
pub fn node_details(ctx: State<Ctx>, key: String, tag: String) -> Result<Value, String> {
    let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    let n = w.lists.get(&key).and_then(|l| l.iter().find(|n| f(n, "tag") == tag)).cloned().ok_or("no such node")?;
    let (state, sentence, _) = w.facts(&key, &n);
    drop(w);
    let vault = engine::vault::load(&ctx.paths);
    let v = vault.get(&f(&n, "server")).cloned().unwrap_or(Value::Null);
    let or = |a: String, b: String| {
        if !a.is_empty() && a != "?" {
            a
        } else if !b.is_empty() {
            b
        } else {
            "?".into()
        }
    };
    let mut rows = vec![
        ("Location", or(f(&n, "where"), f(&v, "where"))),
        ("Host", or(f(&v, "isp"), f(&n, "isp"))),
        ("Front site (SNI)", or(f(&n, "sni"), String::new())),
        ("443 ping from this PC", ms(n.get("tcp_ms"), &tr("No answer", &[]))),
        ("Through the PC tunnel", ms(n.get("pc_ms"), &or(f(&n, "pc_why"), "-".into()))),
    ];
    if Mode::of(&ctx.paths).router {
        rows.push(("Through the router tunnel", ms(n.get("phone_ms"), &or(f(&n, "phone_why"), "-".into()))));
    }
    let last: String =
        if f(&v, "last_ok").is_empty() { tr("Never", &[]) } else { f(&v, "last_ok") }.chars().take(16).collect();
    let passes = v.get("passes").and_then(Value::as_i64).unwrap_or(0).to_string();
    rows.push(("Passed tests", tr("{n}x, last {at}", &[("n", passes), ("at", last)])));
    rows.push(("Best tested", ms(v.get("ms"), "-")));
    let rows: Vec<Value> = rows.into_iter().map(|(k, val)| json!([tr(k, &[]), val])).collect();
    let cut =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64() - 86400.0;
    let h = engine::history::load(&ctx.paths.history);
    let pings: Vec<Value> = h["pings"][f(&n, "server")]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p.get(0).and_then(Value::as_f64).is_some_and(|t| t > cut))
        .cloned()
        .collect();
    let slow = Settings::read(&ctx.paths).map_or(200, |s| s.slow_ms);
    Ok(json!({"title": format!("{}  ·  {}", f(&n, "tag"), f(&n, "server")), "state": state, "sentence": sentence,
        "rows": rows, "pings": pings, "slow": slow}))
}

// What Settings shows (SettingsDialog)
#[tauri::command]
pub fn settings_get(ctx: State<Ctx>) -> Value {
    let p = &ctx.paths;
    let s = Settings::read(p).unwrap_or_default();
    let mode = Mode::of(p);
    let langs: Vec<Value> = LANGS.iter().map(|(c, n)| json!([c, n])).collect();
    let lang = if LANGS.iter().any(|(c, _)| *c == s.lang) {
        s.lang.clone()
    } else {
        engine::translations::system_lang().into()
    };
    let rc = settings::this_pc(p).get("router_core").cloned().unwrap_or(Value::Null);
    json!({
        "values": s, "lang": lang, "langs": langs, "router": mode.router, "ronly": mode.ronly, "mac": !cfg!(windows),
        "version": format!("{} {}", engine::NAME, engine::version_label()), "repo": engine::REPO,
        "pc_now": if mode.ronly { None } else { engine::pc_tunnel::core_version(p) },
        "rc_now": rc, "defaults": Settings::default(),
    })
}

// The newest sing-box on each channel, for the PC and the router (sb_ask); slow when GitHub is blocked
#[tauri::command]
pub async fn sb_versions(app: AppHandle) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = tauri::Manager::state::<Ctx>(&app);
        let mode = Mode::of(&ctx.paths);
        let (stable, testing) =
            if mode.ronly { (None, None) } else { engine::pc_tunnel::core_versions(&ctx.paths).unwrap_or_default() };
        let (rs, rp) =
            if mode.router { engine::router_actions::router_core_versions(&ctx.paths) } else { (None, None) };
        json!({"pc": {"stable": stable, "testing": testing}, "router": {"stable": rs, "preview": rp}})
    })
    .await
    .unwrap_or(Value::Null)
}

// Save Settings; what to tell the person and whether the window lays itself out again (SettingsDialog.save)
#[tauri::command]
pub fn settings_save(
    app: AppHandle,
    ctx: State<Ctx>,
    changes: Map<String, Value>,
    router_only: bool,
) -> Result<Value, String> {
    let p = &ctx.paths;
    let before = Settings::read(p).map_err(|e| e.to_string())?;
    let was_ronly = settings::router_only(p);
    let rc_was = engine::router_actions::router_channel(p);
    let mut c = changes.clone();
    c.insert("pc_tunnel".into(), json!(i64::from(!router_only)));
    let mut d = Settings::save(p, &c).map_err(|e| e.to_string())?;
    let mut msgs: Vec<String> = Vec::new();
    let mut ask_router = false;
    if router_only && d.router == 0 {
        d = Settings::save(p, json!({"pc_tunnel": 1}).as_object().unwrap()).map_err(|e| e.to_string())?;
        msgs.push(tr("Router only needs router features on; the tunnel stays on this PC.", &[]));
    }
    if settings::router_only(p) != was_ronly {
        if settings::router_only(p) {
            engine::pc_tunnel::core_stop(p);
            if engine::pc_tunnel::boot_task_here(p) {
                engine::startup_tasks::boot_task(p, false); // nothing to start at boot any more (one admin prompt)
            }
            msgs.push(tr("The tunnel on this PC is off now. If your VPN app uses the SOCKS5 proxy 127.0.0.1:2080, turn that off (PIA: Settings → Multi-Hop, untick \"Multi-hop and Obfuscation\"). To tunnel this PC through the router, tick it in Router > Devices, or choose every device.", &[]));
        } else {
            engine::pc_tunnel::core_restart(p);
        }
    }
    if d.lang != before.lang {
        engine::translations::use_lang(&d.lang);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    if (d.start_menu, d.desktop_icon) != (before.start_menu, before.desktop_icon)
        && engine::app_install::installed_here(&exe)
        && !cfg!(debug_assertions)
    {
        engine::app_install::shortcuts(p, &exe);
    }
    let key = if d.router_key.is_empty() { p.router_key.clone() } else { std::path::PathBuf::from(&d.router_key) };
    if d.router != 0 && before.router == 0 && !key.exists() {
        Settings::save(p, json!({"router": 0}).as_object().unwrap()).map_err(|e| e.to_string())?;
        ask_router = true; // the page asks whether to open the router steps of the setup
    }
    engine::activity_log::say(&format!(
        "settings saved: {}",
        serde_json::to_string(&Settings::read(p).unwrap_or_default()).unwrap_or_default()
    ));
    if d.singbox != before.singbox {
        engine::activity_log::say(&format!("sing-box channel: {}; installing that build now...", d.singbox));
        jobs::work(&app, Job { name: "core_update".into(), args: Value::Null });
    }
    if d.router != 0 && engine::router_actions::router_channel(p) != rc_was {
        let h = app.clone(); // the router's channel changed: offer that build now
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            crate::updates::maybe_router_core(&h, true);
        });
    }
    jobs::changed(&app);
    Ok(json!({"ask_router": ask_router, "messages": msgs, "relayout": d.lang != before.lang}))
}

// Ask GitHub for a newer Reality Hop: its version, or None (check_updates)
#[tauri::command]
pub async fn check_updates(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = tauri::Manager::state::<Ctx>(&app);
        let up = engine::app_install::check_update(&ctx.paths)?;
        let ver = up.as_ref().map(|u| u.latest.ver.clone());
        ctx.win.lock().unwrap_or_else(|e| e.into_inner()).update = up;
        jobs::changed(&app);
        Ok(ver)
    })
    .await
    .map_err(|e| e.to_string())?
}

// Remove Reality Hop from this PC; the lines for the person. The process ends once they have read them (uninstall)
#[tauri::command]
pub fn uninstall(ctx: State<Ctx>, keep: bool, router: bool) -> Result<Vec<String>, String> {
    if cfg!(debug_assertions) {
        return Err("a development build does not uninstall".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut ssh = engine::router_ssh::RouterSsh { paths: &ctx.paths };
    Ok(engine::app_install::uninstall(&ctx.paths, &mut ssh, keep, router, &exe))
}

// End the process (after uninstall)
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

// Forget the router: a new router, a reset router or a new address (Change router)
#[tauri::command]
pub fn router_forget(ctx: State<Ctx>) {
    engine::router_ssh::router_forget(&ctx.paths);
}

// Make a redacted report; its file name and text. Nothing is sent (ReportDialog.make)
#[tauri::command]
pub async fn report_make(app: AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = tauri::Manager::state::<Ctx>(&app);
        let mut ssh = engine::router_ssh::RouterSsh { paths: &ctx.paths };
        let (path, text) = engine::problem_report_file::make(&ctx.paths, &mut ssh)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(json!({"name": name, "text": text}))
    })
    .await
    .map_err(|e| e.to_string())?
}

// A new GitHub issue with the report's summary filled in, and the reports folder to drag the file from
// (ReportDialog.issue)
#[tauri::command]
pub fn report_issue(ctx: State<Ctx>, name: String) -> Result<(), String> {
    if !name.starts_with("report-") || !name.ends_with(".txt") || name.contains(['/', '\\']) || name.contains("..") {
        return Err("not a report".into());
    }
    let path = ctx.paths.reports.join(&name);
    let text = engine::data_files::read_text(&path).map_err(|e| e.to_string())?;
    let url = engine::problem_report::issue_url(&name, &text);
    let tool =
        if cfg!(windows) { engine::process::system_tool("explorer").into_os_string() } else { "/usr/bin/open".into() };
    std::process::Command::new(&tool).arg(url).spawn().map_err(|e| e.to_string())?;
    std::process::Command::new(&tool).arg(&ctx.paths.reports).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// The devices the router tunnels and the router's DHCP leases (DevicesDialog.load)
#[tauri::command]
pub async fn devices_get(app: AppHandle) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = tauri::Manager::state::<Ctx>(&app);
        let s = Settings::read(&ctx.paths).unwrap_or_default();
        let mut ssh = engine::router_ssh::RouterSsh { paths: &ctx.paths };
        let (rows, err) = match engine::router::router_clients(&mut ssh) {
            Ok(r) => (r, String::new()),
            Err(e) => (
                Vec::new(),
                tr("Could not read the router ({e}); you can still add addresses by hand.", &[("e", e.to_string())]),
            ),
        };
        json!({
            "picked": engine::router::devices(&s.devices), "all": engine::router::all_devices(&s.devices),
            "rows": rows, "err": err, "host": s.router_host,
        })
    })
    .await
    .unwrap_or(Value::Null)
}

// A local network address like 192.168.8.20
#[tauri::command]
pub fn lan_ip(ip: String) -> bool {
    engine::validate::lan_ip(&ip)
}

// Keep the device list and push it to the router (DevicesDialog.save)
#[tauri::command]
pub fn devices_save(app: AppHandle, ctx: State<Ctx>, ips: Vec<String>) -> Result<(), String> {
    let ips: Vec<String> = ips.into_iter().filter(|x| x == "all" || engine::validate::lan_ip(x)).collect();
    let mut c = Map::new();
    c.insert("devices".into(), json!(ips.join(", ")));
    Settings::save(&ctx.paths, &c).map_err(|e| e.to_string())?;
    jobs::work(&app, Job { name: "devices_push".into(), args: Value::Null });
    Ok(())
}

// The router's web panel in the browser: only the router address from Settings
#[tauri::command]
pub fn open_router_panel(ctx: State<Ctx>) -> Result<(), String> {
    let host = Settings::read(&ctx.paths).unwrap_or_default().router_host;
    if !engine::validate::lan_ip(&host) {
        return Err("the router address in Settings is not a local address".into());
    }
    let tool =
        if cfg!(windows) { engine::process::system_tool("explorer").into_os_string() } else { "/usr/bin/open".into() };
    std::process::Command::new(tool).arg(format!("http://{host}")).spawn().map(|_| ()).map_err(|e| e.to_string())
}
