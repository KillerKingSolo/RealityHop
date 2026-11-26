//! The setup, one step at a time: install and first nodes, an optional GL.iNet router, the VPN app, background start
//! (Wizard). First the language and, at the first setup, the France question (pick_language, ask_france).

use crate::jobs::{self, Ctx};
use crate::window_state::{Job, Worker};
use engine::activity_log::say;
use engine::paths::Paths;
use engine::router::Ssh;
use engine::router_ssh::RouterSsh;
use engine::settings::{self, Settings};
use engine::translations::{LANGS, tr};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use tauri::{AppHandle, State};

/// The steps, by key and English name.
pub const STEPS: [(&str, &str); 8] = [
    ("welcome", "Welcome"),
    ("install", "Install"),
    ("router", "Router?"),
    ("ssh", "Router: connect"),
    ("box", "Router: sing-box"),
    ("devices", "Router: devices"),
    ("vpn", "VPN app"),
    ("finish", "Finish"),
];

/// What the wizard knows: the steps done, the last note, the router's clients.
#[derive(Default)]
pub struct Wz {
    pub ok: HashSet<String>,
    pub note: Option<(String, String)>,
    pub rows: Vec<(String, String)>,
    /// The installed copy to go on in, after this copy installed Reality Hop.
    pub relaunch: Option<std::path::PathBuf>,
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn save(paths: &Paths, v: Value) -> Result<Settings, String> {
    Settings::save(paths, &obj(v)).map_err(|e| e.to_string())
}

fn exe() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_default()
}

// This copy installs itself: a release build that does not run from the install folder
fn installs() -> bool {
    !cfg!(debug_assertions) && engine::app_install::needs_install(&exe())
}

// What the pages need
#[tauri::command]
pub fn wz_info(ctx: State<Ctx>, router_only: bool) -> Value {
    let p = &ctx.paths;
    let s = Settings::read(p).unwrap_or_default();
    {
        let mut w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
        if engine::setup_steps::is_setup(p) && engine::singbox_config::nodes_of(&p.pc_cfg).is_ok_and(|n| !n.is_empty())
        {
            w.wz.ok.insert("install".into()); // running the wizard again: the PC part is already done
        }
    }
    let mac = !cfg!(windows);
    let frozen = !cfg!(debug_assertions);
    let installed_exe = engine::app_install::installed_exe(&exe());
    let split = if engine::pia_split_tunnel::pia_bypass(p, &[], true) {
        None
    } else if mac {
        Some(tr("Settings → Split Tunnel → Add Application → Applications → Reality Hop → Bypass VPN.", &[]))
    } else {
        Some(tr(
            "Only if Split Tunnel has All Other Apps on Use VPN: Settings → Split Tunnel, Add Application {core}, set it to Bypass VPN. Without it the VPN cannot connect.",
            &[("core", p.core.display().to_string())],
        ))
    };
    json!({
        "steps": STEPS.iter().map(|(k, n)| [k.to_string(), tr(n, &[])]).collect::<Vec<_>>(),
        "use_router": router_only || !settings::pc_only(p),
        "start": if router_only { "router" } else { "welcome" },
        "mac": mac, "frozen": frozen,
        "first_install": frozen && !mac && !installed_exe.is_file(),
        "install_dir": engine::app_install::install_dir().display().to_string(),
        "beta": s.singbox == "testing", "start_menu": s.start_menu != 0, "desktop": s.desktop_icon != 0,
        "top": s.top, "host": s.router_host,
        "all_dev": engine::router::all_devices(&s.devices), "devices": engine::router::devices(&s.devices),
        "split": split, "ronly": settings::router_only(p),
        "auto_update": s.auto_update != 0, "backend_files": s.backend_files != 0,
        "version": engine::version_label(), "repo": engine::REPO,
        "ask_lang": s.lang.is_empty(), "ask_france": s.dns.is_empty() && !engine::setup_steps::is_setup(p),
        "langs": LANGS, "sys_lang": engine::translations::system_lang(),
    })
}

// The steps done, the note, the router's clients, and whether an action runs
#[tauri::command]
pub fn wz_view(ctx: State<Ctx>) -> Value {
    let w = ctx.win.lock().unwrap_or_else(|e| e.into_inner());
    json!({"ok": w.wz.ok, "note": w.wz.note, "rows": w.wz.rows, "busy": w.busy, "prog": crate::window_state::prog(&w), "relaunch": w.wz.relaunch.is_some()})
}

// A new page: the last page's note goes
#[tauri::command]
pub fn wz_note_clear(ctx: State<Ctx>) {
    ctx.win.lock().unwrap_or_else(|e| e.into_inner()).wz.note = None;
}

// The language, asked once at the first start (pick_language)
#[tauri::command]
pub fn wz_lang(ctx: State<Ctx>, code: String) -> Result<(), String> {
    save(&ctx.paths, json!({"lang": code}))?;
    engine::translations::use_lang(&code);
    Ok(())
}

// In France, Cloudflare instead of Quad9 as the name server (ask_france)
#[tauri::command]
pub fn wz_france(ctx: State<Ctx>, dns: String) -> Result<(), String> {
    let dns = if dns == "cloudflare" { "cloudflare" } else { "quad9" };
    save(&ctx.paths, json!({"dns": dns}))?;
    engine::setup_steps::dns_apply(&ctx.paths);
    Ok(())
}

// This PC and the router, the router only, or this PC only (page_router.pick)
#[tauri::command]
pub fn wz_mode(ctx: State<Ctx>, mode: String) -> Result<(), String> {
    let p = &ctx.paths;
    if mode == "pc" {
        save(p, json!({"router": 0, "pc_tunnel": 1}))?;
    } else {
        save(p, json!({"pc_tunnel": i64::from(mode != "router")}))?;
    }
    if mode == "router" {
        let p = p.clone(); // the install step started the PC tunnel; the router does the work from now on
        std::thread::spawn(move || engine::pc_tunnel::core_stop(&p));
    }
    Ok(())
}

// The router address, checked and kept (save_host)
fn save_host(p: &Paths, h: &str) -> Result<String, String> {
    let h = h.trim();
    if !engine::validate::host_ok(h) {
        return Err("the router address looks wrong; use something like 192.168.8.1".into());
    }
    save(p, json!({"router_host": h}))?;
    Ok(h.to_string())
}

// A console window where the person types the router password once; the router then trusts Reality Hop's key
// (add_key)
#[tauri::command]
pub fn wz_add_key(ctx: State<Ctx>, host: String) -> Result<String, String> {
    let host = save_host(&ctx.paths, &host)?;
    engine::router_ssh::router_forget(&ctx.paths); // this is the router now, also when it is a new one
    engine::router_actions::add_key_console(&ctx.paths, &host)?;
    Ok(tr("Type the router password in the black window, then click Test connection.", &[]))
}

// A private key the router already accepts (own_key)
#[tauri::command]
pub fn wz_own_key(ctx: State<Ctx>, path: String) -> Result<String, String> {
    let f = std::path::PathBuf::from(path.trim().trim_matches('"'));
    if !f.is_absolute() || !f.is_file() {
        return Err(tr("Something failed: {e}", &[("e", "no such file".into())]));
    }
    save(&ctx.paths, json!({"router_key": f.display().to_string()}))?;
    let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(tr("Using {file}. Click Test connection.", &[("file", name)]))
}

// Check PIA: its settings and the split tunnel; (text, state)
#[tauri::command]
pub fn wz_check_pia(ctx: State<Ctx>) -> (String, String) {
    let text = engine::setup_steps::vpn_check(&ctx.paths);
    let st = engine::setup_steps::vpn_state(&ctx.paths);
    (text, if st.0 && st.1.is_empty() { "ok" } else { "warn" }.into())
}

// Go on in the installed copy, at the router step: this copy came from the Downloads folder
#[tauri::command]
pub fn wz_relaunch(app: AppHandle, ctx: State<Ctx>) -> Result<(), String> {
    let e = ctx.win.lock().unwrap_or_else(|e| e.into_inner()).wz.relaunch.clone().ok_or("nothing to open")?;
    std::process::Command::new(e)
        .args(["--setup", "--step", "router", "--after-update"])
        .spawn()
        .map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

// Start a wizard action
#[tauri::command]
pub fn wz_act(app: AppHandle, ctx: State<Ctx>, name: String, args: Option<Value>) {
    ctx.win.lock().unwrap_or_else(|e| e.into_inner()).wz.note = None;
    jobs::work(&app, Job { name, args: args.unwrap_or(Value::Null) });
}

impl Worker<'_> {
    fn note(&self, text: String, state: &str) {
        self.w().wz.note = Some((text, state.into()));
    }

    fn done_ok(&self, gate: &str, text: String) {
        self.w().wz.ok.insert(gate.into());
        self.note(text, "ok");
    }

    // A wizard action; a failure shows on the page (work)
    pub fn wizard(&self, name: &str, args: &Value, cmd: &[String]) -> Result<(), String> {
        let r = match name {
            "wz_install" => self.wz_install(args),
            "wz_test_ssh" => self.wz_test_ssh(args["host"].as_str().unwrap_or_default()),
            "wz_check_box" => self.wz_check_box(),
            "wz_box_install" => self.wz_box_install(),
            "wz_load_devices" => self.wz_load_devices(),
            "wz_push_devices" => self.wz_push_devices(args),
            "wz_finish" => self.wz_finish(args, cmd),
            other => Err(format!("unknown action: {other}")),
        };
        if let Err(e) = &r {
            self.note(tr("Something failed: {e}", &[("e", e.clone())]), "bad");
        }
        r
    }

    // Install: the program (a release run from elsewhere), sing-box, the first nodes, the tunnel (do_install)
    fn wz_install(&self, a: &Value) -> Result<(), String> {
        let channel = if a["testing"] == true { "testing" } else { "stable" };
        let folder = a["folder"].as_str().map(str::trim).filter(|f| !f.is_empty()).map(std::path::PathBuf::from);
        if folder.as_ref().is_some_and(|f| !f.is_absolute()) {
            return Err(r"the install folder must be a full path, like C:\Program Files\RealityHop".into());
        }
        save(
            self.paths,
            json!({"singbox": channel, "start_menu": i64::from(a["menu"] == true), "desktop_icon": i64::from(a["desk"] == true)}),
        )?;
        let mut paths = self.paths.clone();
        if installs() {
            let (msg, np) = engine::app_install::install(self.paths, folder.as_deref(), &exe(), |_| {})?;
            paths = np;
            jobs::data_moved(self.paths, &paths);
            say(&msg);
        }
        let t = settings::this_pc(&paths);
        if paths.core.exists() && t.get("core_channel").and_then(Value::as_str) != Some(channel) {
            engine::pc_tunnel::core_update(&paths, false)?; // running the setup again with the other channel
        }
        let res = engine::setup_steps::setup_pc(&paths, &mut RouterSsh { paths: &paths })?;
        if installs() {
            self.w().wz.relaunch = Some(engine::app_install::installed_exe(&exe()));
        }
        let bad: Vec<&String> = res.iter().filter(|l| l.contains("NOT") || l.contains("did not come up")).collect();
        if bad.is_empty() {
            self.done_ok("install", tr("Installed. The tunnel is running. Press Next.", &[]));
        } else {
            let text: Vec<&str> = bad.iter().map(|s| s.as_str()).collect();
            let text = text.join(" ");
            self.note(format!("{} {text}", tr("Installed with a problem:", &[])), "warn");
            if !text.contains("did not come up") {
                self.w().wz.ok.insert("install".into());
            }
        }
        Ok(())
    }

    // Test the SSH connection (test_ssh)
    fn wz_test_ssh(&self, host: &str) -> Result<(), String> {
        save_host(self.paths, host)?;
        engine::router_ssh::router_forget(self.paths);
        let mut ssh = RouterSsh { paths: self.paths };
        if ssh.run("echo up", None, 30).map(|o| o.trim().to_string()).ok().as_deref() != Some("up") {
            return Err("the router did not answer over SSH; add the key first, and check the address".into());
        }
        let i = engine::router::router_info(&mut ssh).map_err(|e| e.to_string())?;
        let bx = i.singbox.unwrap_or_else(|| tr("Not installed", &[]));
        self.done_ok(
            "ssh",
            tr(
                "Connected: {model}, firmware {fw}, sing-box {box}. Press Next.",
                &[("model", i.model), ("fw", i.firmware), ("box", bx)],
            ),
        );
        Ok(())
    }

    // sing-box, jq and curl on the router (check_box)
    fn wz_check_box(&self) -> Result<(), String> {
        let i = engine::router::router_info(&mut RouterSsh { paths: self.paths }).map_err(|e| e.to_string())?;
        let miss: Vec<&str> = ["jq", "curl"].into_iter().filter(|t| !i.tools.iter().any(|x| x == t)).collect();
        match &i.singbox {
            Some(v) if miss.is_empty() && i.too_new => {
                self.w().wz.ok.insert("box".into());
                self.note(tr("sing-box {ver} is installed. Reality Hop's router config is written for sing-box 1.9 to 1.14; this newer version may reject it. If Push to router fails, please open an issue on GitHub.", &[("ver", v.clone())]), "warn");
            }
            Some(v) if miss.is_empty() => self.done_ok(
                "box",
                tr(
                    "{model}: sing-box {ver}, jq and curl are installed. Press Next.",
                    &[("model", i.model.clone()), ("ver", v.clone())],
                ),
            ),
            _ => {
                let what = match &i.singbox {
                    None => tr("sing-box is not installed", &[]),
                    Some(v) => format!("sing-box {v}"),
                };
                let miss = if miss.is_empty() {
                    String::new()
                } else {
                    format!("; {}", tr("Missing {tools}", &[("tools", miss.join(", "))]))
                };
                self.note(
                    format!("{}: {what}{miss}. {}", i.model, tr("Click Install sing-box on the router.", &[])),
                    "warn",
                );
            }
        }
        Ok(())
    }

    fn wz_box_install(&self) -> Result<(), String> {
        say("router: installing sing-box, jq and curl (a few minutes on a slow connection)...");
        say(&engine::router::router_install(&mut RouterSsh { paths: self.paths }).map_err(|e| e.to_string())?);
        self.wz_check_box()
    }

    // The router's DHCP clients (load_devices)
    fn wz_load_devices(&self) -> Result<(), String> {
        let rows = engine::router::router_clients(&mut RouterSsh { paths: self.paths }).map_err(|e| e.to_string())?;
        self.w().wz.rows = rows;
        Ok(())
    }

    // The chosen devices into Settings and onto the router; in router-only mode first nodes from the router's side
    // (push_devices)
    fn wz_push_devices(&self, a: &Value) -> Result<(), String> {
        let every = a["every"] == true;
        let extra: Vec<String> = a["extra"]
            .as_str()
            .unwrap_or_default()
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect();
        let bad: Vec<&str> = extra.iter().filter(|x| !engine::validate::lan_ip(x)).map(String::as_str).collect();
        if !bad.is_empty() {
            return Err(format!("not an address: {}", bad.join(", ")));
        }
        let shown: HashSet<String> = self.w().wz.rows.iter().map(|r| r.0.clone()).collect();
        let picked: HashSet<String> =
            a["picked"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect();
        let ips: Vec<String> = if every {
            vec!["all".into()]
        } else {
            let mut set: Vec<String> = extra
                .iter()
                .cloned()
                .chain(picked.intersection(&shown).cloned())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            set.sort();
            set
        };
        if ips.is_empty() {
            return Err("pick at least one device, or every device".into());
        }
        save(self.paths, json!({"devices": ips.join(", "), "router": 1}))?;
        let mut ssh = RouterSsh { paths: self.paths };
        if settings::router_only(self.paths)
            && engine::singbox_config::nodes_of(&self.paths.router_cfg).map_or(true, |n| n.is_empty())
        {
            say("router: finding the first nodes (tested on the router)...");
            let nodes = engine::node_manager::scan(self.paths, &mut ssh, None)?;
            engine::node_manager::apply(self.paths, &mut ssh, &nodes, engine::node_manager::SLOTS)?;
        }
        let who = if every { "every device on the main network".to_string() } else { ips.join(", ") };
        say(&format!("router: pushing the config for {who}..."));
        let who = if every { tr("every device on the main network", &[]) } else { who };
        let out = engine::node_manager::push(self.paths, &mut ssh)?;
        say(&out);
        if !out.contains("sing-box service: running") {
            return Err("the router did not take the config; see the log above".into());
        }
        self.done_ok("devices", tr("The router tunnel is running for {who}. Press Next.", &[("who", who)]));
        Ok(())
    }

    // Background start, boot start, updates, backend files; then the window (finish)
    fn wz_finish(&self, a: &Value, cmd: &[String]) -> Result<(), String> {
        save(
            self.paths,
            json!({"auto_update": i64::from(a["upd"] == true), "backend_files": i64::from(a["files"] == true)}),
        )?;
        engine::startup_tasks::startup(self.paths, a["bg"] == true, cmd, |_| {});
        if settings::router_only(self.paths) {
            if engine::pc_tunnel::boot_task_here(self.paths) {
                engine::startup_tasks::boot_task(self.paths, false); // no PC tunnel to start at boot any more
            }
        } else if a["boot"] == true && !engine::pc_tunnel::boot_task_here(self.paths) {
            engine::startup_tasks::boot_task(self.paths, true);
        }
        self.w().boot_on = !matches!(engine::startup_tasks::task_state_here(self.paths), None | Some("Disabled")); // the window's switch
        let mut done = obj(json!({"setup_done": true, "stopped": false}));
        if !settings::this_pc(self.paths).contains_key("seen_ver") {
            done.insert("seen_ver".into(), engine::VERSION.into()); // a new install: no "What's new" for this version
        }
        settings::set_this_pc(self.paths, done).map_err(|e| e.to_string())?;
        self.w().wz.ok.insert("finish".into());
        Ok(())
    }
}
