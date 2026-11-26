//! The window's state and actions (the port of realityhop.pyw's App): what each node table shows, the tiles, the status
//! line, and the actions behind every button. Actions run one at a time on a worker thread; the rest wait in order, each
//! once. The page asks for a snapshot after each change and draws it.

use engine::activity_log::say;
use engine::node_manager as nm;
use engine::paths::Paths;
use engine::router_ssh::RouterSsh;
use engine::settings::{self, Settings};
use engine::translations::tr;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// An action from the window, with its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub name: String,
    pub args: Value,
}

/// Everything the window knows.
#[derive(Default)]
pub struct Win {
    /// The nodes of each table: "pc" and "phone" (the router's), with their test results.
    pub lists: HashMap<String, Vec<Value>>,
    /// The PC tunnel: service, listening, api, now, exit.
    pub pcs: Option<Value>,
    /// The router's last full test (service, now, cfg) and when.
    pub router: Option<(Value, String)>,
    pub router_now: Option<Value>,
    /// Why the router cannot be reached, None when it can or nobody asked yet.
    pub router_away: Option<String>,
    pub rcore_text: String,
    pub pc_pending: bool,
    pub router_stale: bool,
    pub pending: Option<bool>,
    pub streak: HashMap<(String, String), u32>,
    pub status_text: String,
    pub status_state: String,
    pub busy: bool,
    pub started: Option<Instant>,
    pub current: Option<Job>,
    /// How long each action took last time (seconds), for the busy bar.
    pub took: HashMap<String, f64>,
    pub jobs: Vec<Job>,
    pub auto_on: bool,
    pub auto_fix: bool,
    pub next_auto: String,
    pub boot_on: bool,
    pub watch: engine::watcher::WatchState,
    pub update: Option<engine::app_install::Update>,
    /// The Candidates dialog.
    pub cand: crate::candidates::Cand,
    /// The setup wizard.
    pub wz: crate::wizard::Wz,
    /// The PC's sing-box under the mode line.
    pub core_text: String,
    /// A router build of sing-box to offer.
    pub rc_offer: Option<engine::router_actions::Offer>,
}

// The local time as HH:MM
fn clock() -> String {
    engine::local_time::now().format("%H:%M")
}

// time.time()
pub(crate) fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// A node's field as text
pub(crate) fn f(n: &Value, k: &str) -> String {
    n.get(k).map(engine::python_json::py_str).filter(|s| s != "None").unwrap_or_default()
}

// A field that is set (not missing, not null)
pub(crate) fn has(n: &Value, k: &str) -> bool {
    n.get(k).is_some_and(|v| !v.is_null())
}

// "{v} ms", or none when there is no time (ms)
pub(crate) fn ms(v: Option<&Value>, none: &str) -> String {
    match v.filter(|v| !v.is_null()) {
        Some(v) => format!("{} ms", engine::python_json::py_str(v)),
        None => none.to_string(),
    }
}

// The site a node's traffic poses as, by name: www.cloudflare.com -> Cloudflare (brand)
pub fn brand(sni: &str) -> String {
    let parts: Vec<&str> = sni.split('.').collect();
    if parts.len() < 2 {
        return if sni.is_empty() { "?".into() } else { sni.into() };
    }
    let p = parts[parts.len() - 2];
    match p {
        "icloud" => "Apple iCloud".into(),
        "cdnjs" => "cdnjs (Cloudflare)".into(),
        "amd" => "AMD".into(),
        "nvidia" => "NVIDIA".into(),
        _ => {
            let mut c = p.chars();
            c.next().map(|x| x.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// The modes the window is laid out for.
pub struct Mode {
    pub router: bool,
    pub ronly: bool,
}

impl Mode {
    // From the settings
    pub fn of(paths: &Paths) -> Mode {
        Mode { router: !settings::pc_only(paths), ronly: settings::router_only(paths) }
    }

    // The node tables shown: (key, label, file)
    pub fn lists(&self) -> Vec<(&'static str, String, &'static str)> {
        let mut v = Vec::new();
        if !self.ronly {
            v.push(("pc", tr("PC tunnel", &[]), "pc-config.json"));
        }
        if self.router {
            v.push(("phone", tr("Router devices", &[]), "router-config.json"));
        }
        v
    }

    // The table the slot numbers come from
    pub fn main(&self) -> &'static str {
        if self.router { "phone" } else { "pc" }
    }

    // What to press after a change that was not deployed
    pub fn push_hint(&self) -> String {
        tr(
            if self.ronly {
                "Use Push to router when you are ready."
            } else if self.router {
                "Use Push to router and Restart tunnel when you are ready."
            } else {
                "Press Restart tunnel when you are ready."
            },
            &[],
        )
    }
}

impl Win {
    // A new window state
    pub fn new() -> Win {
        Win {
            status_text: tr("Reading the configs and testing from this PC...", &[]),
            status_state: "unk".into(),
            auto_on: true,
            auto_fix: true,
            ..Default::default()
        }
    }

    // The list of one table
    fn list(&self, key: &str) -> &[Value] {
        self.lists.get(key).map_or(&[], Vec::as_slice)
    }

    // (state, sentence, short text for the table) for one row: router result, else PC tunnel result, else plain 443
    // reachability (facts)
    pub fn facts(&self, key: &str, n: &Value) -> (&'static str, String, String) {
        let dead = tr("Dead", &[]);
        if key == "phone" && has(n, "phone_why") {
            return if has(n, "phone_ms") {
                (
                    "ok",
                    tr("Working, {ms} ms through the router tunnel", &[("ms", f(n, "phone_ms"))]),
                    tr("Working", &[]),
                )
            } else {
                let why = f(n, "phone_why");
                ("bad", tr("NOT WORKING on the router: {why}", &[("why", why.clone())]), format!("{dead}  ·  {why}"))
            };
        }
        let pc = if key == "pc" { Some(n) } else { self.list("pc").iter().find(|m| f(m, "server") == f(n, "server")) };
        if let Some(pc) = pc.filter(|p| has(p, "pc_why")) {
            return if has(pc, "pc_ms") {
                ("ok", tr("Working, {ms} ms through the PC tunnel", &[("ms", f(pc, "pc_ms"))]), tr("Working", &[]))
            } else {
                let why = f(pc, "pc_why");
                (
                    "bad",
                    tr("NOT WORKING: the PC tunnel could not fetch through it ({why})", &[("why", why.clone())]),
                    format!("{dead}  ·  {why}"),
                )
            };
        }
        if !has(n, "tcp_ms") {
            return ("bad", tr("Not answering on 443 from this PC", &[]), tr("Not answering on 443", &[]));
        }
        let ms = f(n, "tcp_ms");
        if key == "pc" && self.pc_pending {
            return (
                "warn",
                tr("Answers on 443 in {ms} ms; the PC tunnel loads it on the next restart", &[("ms", ms)]),
                tr("Waiting for PC restart", &[]),
            );
        }
        (
            "warn",
            tr("Answers on 443 in {ms} ms; not tunnel-tested yet", &[("ms", ms)]),
            tr("Reachable  ·  not tunnel-tested", &[]),
        )
    }

    // Count failures in a row per node (count)
    fn count(&mut self, key: &str) {
        let states: Vec<(String, bool)> =
            self.list(key).iter().map(|n| (f(n, "server"), self.facts(key, n).0 == "bad")).collect();
        for (s, bad) in states {
            let e = self.streak.entry((key.into(), s)).or_insert(0);
            *e = if bad { *e + 1 } else { 0 };
        }
    }

    // The plain-language status line (judge)
    pub fn judge(&mut self, mode: &Mode) {
        let (mut msgs, mut state) = (Vec::new(), "ok");
        for (key, label, _) in mode.lists() {
            let nodes = self.list(key);
            if nodes.is_empty() {
                continue;
            }
            let states: Vec<&str> = nodes.iter().map(|n| self.facts(key, n).0).collect();
            let bad: Vec<&Value> = nodes.iter().zip(&states).filter(|(_, s)| **s == "bad").map(|(n, _)| n).collect();
            if !bad.is_empty() {
                let names =
                    bad.iter().map(|n| format!("{} ({})", f(n, "tag"), f(n, "server"))).collect::<Vec<_>>().join(", ");
                let kw = [("label", label.clone()), ("names", names)];
                let mut m = tr(
                    if bad.len() == 1 {
                        "{label}: {names} is not working."
                    } else {
                        "{label}: {names} are not working."
                    },
                    &kw,
                );
                if bad.len() == nodes.len() {
                    m += &format!(" {}", tr("ALL nodes are down: press Rescue.", &[]));
                }
                msgs.push(m);
                state = "bad";
            } else if states.iter().all(|s| *s == "ok") {
                msgs.push(tr(
                    "{label}: all {n} nodes are working.",
                    &[("label", label), ("n", nodes.len().to_string())],
                ));
            } else {
                msgs.push(tr(
                    "{label}: all {n} nodes answer on 443; no tunnel test yet.",
                    &[("label", label), ("n", nodes.len().to_string())],
                ));
                if state == "ok" {
                    state = "warn";
                }
            }
        }
        let servers = |k: &str| self.list(k).iter().map(|n| f(n, "server")).collect::<Vec<_>>();
        if mode.router
            && !self.list("pc").is_empty()
            && !self.list("phone").is_empty()
            && servers("pc") != servers("phone")
        {
            msgs.push(tr("The PC and router lists differ; a replace makes them the same.", &[]));
        }
        let pending = self.pending == Some(true);
        if pending && self.router_stale {
            msgs.push(format!("{} {}", tr("Both configs are newer than what is running.", &[]), mode.push_hint()));
        } else if pending {
            msgs.push(tr("The PC tunnel runs an older node set than pc-config.json: press Restart tunnel.", &[]));
        } else if self.router_stale {
            msgs.push(tr("The router runs an older config than router-config.json: press Push to router.", &[]));
        } else if self.pc_pending {
            msgs.push(tr("New nodes are waiting for a PC tunnel restart.", &[]));
        }
        self.status_text = if msgs.is_empty() { tr("Nothing checked yet.", &[]) } else { msgs.join(" ") };
        self.status_state = state.into();
    }

    // The tray ring of the window: one segment per node, center the overall state (set_icon)
    pub fn segs(&self, mode: &Mode, slow_ms: f64) -> Vec<String> {
        let key = if mode.ronly { "phone" } else { "pc" };
        self.list(key)
            .iter()
            .map(|n| {
                let st = self.facts(key, n).0;
                let t = n.get(format!("{key}_ms")).and_then(Value::as_f64).unwrap_or(0.0);
                if st == "ok" && t > slow_ms {
                    "warn".into()
                } else if st == "ok" || st == "bad" {
                    st.into()
                } else {
                    "unk".into()
                }
            })
            .collect()
    }
}

/// The window's state, shared by the commands and the worker.
pub type Shared = Mutex<Win>;

// What the PC tunnel is doing right now, without SSH (engine.pc_status)
pub fn pc_status(paths: &Paths) -> Value {
    let service = engine::pc_tunnel::core_running(paths);
    let listening =
        std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], 2080).into(), Duration::from_secs(1)).is_ok();
    // the node traffic goes through: a pin or a hold on the selector first, else the group's pick
    let (now, api) = match engine::clash_api::proxies(paths) {
        Some(px) => (engine::clash_api::now_tag(&px).map_or(Value::Null, Value::from), true),
        None => (Value::Null, false),
    };
    json!({"service": service, "listening": listening, "api": api, "now": now, "exit": null})
}

// The nodes of a config with what the last check knew about them
fn fresh_list(paths: &Paths, cfg: &std::path::Path, prev: &[Value]) -> Vec<Value> {
    let txt = engine::data_files::read_text(&paths.nodes_txt).unwrap_or_default();
    let known = engine::node_tables::known_where(&txt);
    let mut nodes = nm::nodes_in(cfg).unwrap_or_default();
    for n in nodes.iter_mut() {
        let p = prev.iter().find(|p| f(p, "server") == f(n, "server")).cloned().unwrap_or(Value::Null);
        let place = p.get("where").filter(|w| !w.is_null() && *w != "").cloned().unwrap_or_else(|| {
            json!(known.iter().find(|(s, _)| *s == f(n, "server")).map_or("?".to_string(), |(_, w)| w.place.clone()))
        });
        n["where"] = place;
        for k in ["pc_ms", "pc_why", "phone_ms", "phone_why"] {
            n[k] = p.get(k).cloned().unwrap_or(Value::Null);
        }
    }
    nodes
}

/// Runs the window's checks and actions; holds what the worker needs.
pub struct Worker<'a> {
    pub paths: &'a Paths,
    pub win: &'a Shared,
}

impl Worker<'_> {
    // The window state, poisoned or not
    pub(crate) fn w(&self) -> std::sync::MutexGuard<'_, Win> {
        self.win.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Set the status line
    fn status(&self, text: String, state: &str) {
        let mut w = self.w();
        w.status_text = engine::translations::cap_first(&text);
        w.status_state = state.into();
    }

    // Everything this PC can know on its own: the two configs, 443 reachability, the PC tunnel's own view (check_local)
    pub fn check_local(&self, which: &str) {
        let mode = Mode::of(self.paths);
        for (key, _, rel) in mode.lists() {
            if which != key && which != "both" {
                continue;
            }
            let cfg = if key == "pc" { &self.paths.pc_cfg } else { &self.paths.router_cfg };
            let prev = self.w().list(key).to_vec();
            let mut nodes = fresh_list(self.paths, cfg, &prev);
            let en = if key == "pc" { "PC tunnel" } else { "Router devices" };
            say(&format!("{en}: {} nodes in {rel}; testing 443 from this PC...", nodes.len()));
            engine::node_tests::tcp_probe(&mut nodes);
            self.w().lists.insert(key.into(), nodes);
        }
        if (which == "pc" || which == "both") && !mode.ronly {
            say("PC: asking the PC tunnel...");
            let mut pcs = pc_status(self.paths);
            let listening = pcs["listening"] == true;
            let ext = std::thread::spawn(move || if listening { engine::clash_api::pc_exit() } else { None });
            let mut list = self.w().list("pc").to_vec();
            if pcs["api"] == true {
                match engine::clash_api::check_pc(self.paths, &mut list, false, now()) {
                    Ok(Some(r)) if r.stale => {
                        self.w().pc_pending = true;
                        say("PC: the tunnel still runs the previous node set; restart it to test the new one");
                    }
                    Ok(Some(_)) => {
                        self.w().pc_pending = false;
                        let pings: Vec<(String, Option<i64>)> =
                            list.iter().map(|n| (f(n, "server"), n.get("pc_ms").and_then(Value::as_i64))).collect();
                        engine::history::record(&self.paths.history, &pings, None, now());
                    }
                    _ => {}
                }
            } else {
                for n in list.iter_mut() {
                    n["pc_ms"] = Value::Null;
                    n["pc_why"] = Value::Null;
                }
            }
            pcs["exit"] = ext.join().ok().flatten().map_or(Value::Null, Value::from);
            let mut w = self.w();
            w.lists.insert("pc".into(), list);
            w.pcs = Some(pcs);
        }
        let pending = nm::pending_deploy(self.paths);
        let mut w = self.w();
        w.pending = pending;
        for key in ["pc", "phone"] {
            if which == key || which == "both" {
                w.count(key);
            }
        }
        w.judge(&mode);
    }

    // Remember whether the router answers on this network (router_seen)
    fn router_seen(&self) {
        self.w().router_away = engine::router_ssh::router_here(self.paths).err();
    }

    // Check now: the router's nodes first when another PC changed them, then everything this PC can see; in router-only
    // mode also the router's own test (first_check)
    pub fn first_check(&self) -> Result<(), String> {
        let mode = Mode::of(self.paths);
        let mut ssh = RouterSsh { paths: self.paths };
        if mode.router {
            self.router_seen();
            if nm::router_up(self.paths, &mut ssh) {
                engine::watcher::follow_router(self.paths, &mut ssh)?;
                let secret =
                    engine::router::secret(&engine::data_files::read_json(&self.paths.router_cfg).unwrap_or_default());
                self.w().router_now = engine::router::router_now(&mut ssh, &secret);
            }
        }
        self.check_local("both");
        if mode.ronly {
            self.check_router()?;
        }
        Ok(())
    }

    // The router's own tunnel tests every node over SSH; its config is compared with router-config.json (check_router)
    pub fn check_router(&self) -> Result<(), String> {
        let mut ssh = RouterSsh { paths: self.paths };
        self.router_seen();
        if engine::watcher::follow_router(self.paths, &mut ssh)?.is_some() {
            self.check_local("both");
        }
        let mut nodes = self.w().list("phone").to_vec();
        if nodes.is_empty() {
            nodes = nm::nodes_in(&self.paths.router_cfg)?;
        }
        say("router: asking the router tunnel to test every node...");
        let mut geo = nodes.clone();
        let paths = self.paths;
        let r = std::thread::scope(|s| {
            let g = s.spawn(|| engine::node_tests::geolocate(paths, &mut geo));
            let secret = engine::router::secret(&engine::data_files::read_json(&paths.router_cfg).unwrap_or_default());
            let r = engine::router::check_phone(&mut ssh, &mut nodes, &secret);
            let _ = g.join();
            r
        });
        let r = r.map_err(|e| e.to_string())?;
        for (n, g) in nodes.iter_mut().zip(&geo) {
            for k in ["where", "isp"] {
                if let Some(v) = g.get(k) {
                    n[k] = v.clone();
                }
            }
        }
        let mode = Mode::of(self.paths);
        let stale = nm::router_stale(self.paths, &r.cfg);
        let mut w = self.w();
        w.router = Some((json!({"service": r.service, "now": r.now, "cfg": r.cfg}), clock()));
        w.router_stale = stale;
        w.lists.insert("phone".into(), nodes);
        w.count("phone");
        w.judge(&mode);
        Ok(())
    }

    // After a push the router picks its node a few seconds later: ask now (read_router_now)
    fn read_router_now(&self) {
        if !Mode::of(self.paths).router {
            return;
        }
        let secret = engine::router::secret(&engine::data_files::read_json(&self.paths.router_cfg).unwrap_or_default());
        self.w().router_now = engine::router::router_now(&mut RouterSsh { paths: self.paths }, &secret);
    }

    // Forget the router's results: its nodes changed (after_files, after_deploy)
    fn forget_router(&self) {
        let mut w = self.w();
        w.router = None;
        w.router_now = None;
        for n in w.lists.entry("phone".into()).or_default().iter_mut() {
            n["phone_ms"] = Value::Null;
            n["phone_why"] = Value::Null;
        }
    }

    // Configs changed, nothing deployed (after_files)
    fn after_files(&self, sentence: &str) {
        self.forget_router();
        self.check_local("both");
        let mode = Mode::of(self.paths);
        self.status(format!("{sentence} {} {}", tr("Nothing deployed yet.", &[]), mode.push_hint()), "warn");
    }

    // Configs deployed (after_deploy)
    pub(crate) fn after_deploy(&self, sentence: &str) {
        self.forget_router();
        {
            let mut w = self.w();
            w.router_stale = !nm::pushed();
            w.pc_pending = true;
        }
        self.check_local("both");
        let mode = Mode::of(self.paths);
        if !nm::pushed() {
            let t = if mode.ronly {
                "Router NOT updated (not reachable)."
            } else {
                "PC tunnel updated; router NOT updated (not reachable)."
            };
            return self.status(format!("{sentence} {}", tr(t, &[])), "warn");
        }
        self.read_router_now();
        let t = if !mode.router {
            "PC tunnel updated."
        } else if mode.ronly {
            "Router updated."
        } else {
            "Router and PC tunnel updated."
        };
        self.status(format!("{sentence} {}", tr(t, &[])), "ok");
    }

    // Fix one node: test the online list (online), the vault (vault), or the fastest 443 ping untested (quick)
    // (do_replace)
    pub fn fix(&self, slot: usize, how: &str) -> Result<(), String> {
        let mut ssh = RouterSsh { paths: self.paths };
        if how == "quick" {
            let swaps = nm::replace_local(self.paths, &mut ssh, &[slot])?;
            let s: Vec<String> = swaps
                .iter()
                .map(|(t, o, n)| {
                    tr(
                        "{tag} was replaced in the configs: {old} -> {new} (443 ping {ms} ms, not tunnel-tested).",
                        &[("tag", t.clone()), ("old", o.clone()), ("new", f(n, "server")), ("ms", f(n, "tcp_ms"))],
                    )
                })
                .collect();
            self.after_files(&s.join(" "));
        } else {
            let swaps = nm::replace(self.paths, &mut ssh, &[slot], None, how == "vault", false, true)?;
            if swaps.is_empty() {
                let t = tr(
                    "vless-{n} was kept: it tests at least as well as every spare.",
                    &[("n", (slot + 1).to_string())],
                );
                self.status(t, "ok");
                return Ok(());
            }
            let s: Vec<String> = swaps
                .iter()
                .map(|(t, o, n)| {
                    let place = if has(n, "where") { f(n, "where") } else { "?".into() };
                    tr(
                        "{tag} was replaced: {old} -> {new} ({ms} ms, {where}).",
                        &[
                            ("tag", t.clone()),
                            ("old", o.clone()),
                            ("new", f(n, "server")),
                            ("ms", f(n, "ms")),
                            ("where", place),
                        ],
                    )
                })
                .collect();
            self.after_deploy(&s.join(" "));
        }
        Ok(())
    }

    // Scan, test and put the best 6 everywhere (do_replace_all)
    pub fn replace_all(&self) -> Result<(), String> {
        let mut ssh = RouterSsh { paths: self.paths };
        let nodes = nm::scan(self.paths, &mut ssh, None)?;
        nm::apply(self.paths, &mut ssh, &nodes, nm::SLOTS)?;
        self.after_deploy(&tr("All slots were rewritten with the 6 best nodes from the scan.", &[]));
        Ok(())
    }

    // Rescue from the vault (do_rescue)
    pub fn rescue(&self) -> Result<(), String> {
        let sentence = nm::rescue(self.paths, &mut RouterSsh { paths: self.paths }, nm::SLOTS)?;
        let ok = sentence.starts_with("Rescue done");
        self.forget_router();
        {
            let mut w = self.w();
            w.router_stale = !ok;
            w.pc_pending = true;
        }
        self.check_local("both");
        if ok {
            self.read_router_now();
        }
        self.status(sentence.clone(), if ok && !sentence.contains("FAILED") { "ok" } else { "warn" });
        Ok(())
    }

    // Push to router (do_push_phone)
    pub fn push_router(&self) -> Result<(), String> {
        let out = nm::push(self.paths, &mut RouterSsh { paths: self.paths })?;
        say(&out);
        let ok = out.contains("sing-box service: running");
        self.forget_router();
        self.w().router_stale = !ok;
        self.check_local("phone");
        if ok {
            self.read_router_now();
        }
        let t = if ok {
            "Router updated and running."
        } else {
            "Router NOT updated (see the activity log). router-config.json stays ready for the next try."
        };
        self.status(tr(t, &[]), if ok { "ok" } else { "bad" });
        Ok(())
    }

    // Pin the PC tunnel ("pc"), the router ("router") or both ("both") to the node at server, or with None let them
    // pick automatically again. A pin holds until that node fails (the watcher and the router's heal.sh pin check)
    pub fn pin(&self, who: &str, server: Option<&str>) -> Result<(), String> {
        use engine::clash_api::{pick, set_pin, tag_of};
        let mode = Mode::of(self.paths);
        let mut done = Vec::new();
        if (who == "pc" || who == "both") && !mode.ronly {
            let tag = match server {
                Some(s) => tag_of(&self.paths.pc_cfg, s)
                    .ok_or_else(|| tr("{server} is not in the PC tunnel's slots.", &[("server", s.into())]))?,
                None => "auto".into(),
            };
            if !pick(self.paths, &tag) {
                return Err(tr("The PC tunnel did not take the change. Restart the PC tunnel, then try again.", &[]));
            }
            set_pin(self.paths, "pc", server);
            done.push(tr("PC", &[]));
            self.check_local("pc");
        }
        if (who == "router" || who == "both") && mode.router {
            let mut ssh = RouterSsh { paths: self.paths };
            let mut cfg = engine::data_files::read_json(&self.paths.router_cfg).map_err(|e| e.to_string())?;
            let secret = engine::router::secret(&cfg);
            let tag = match server {
                Some(s) => tag_of(&self.paths.router_cfg, s)
                    .ok_or_else(|| tr("{server} is not in the router's slots.", &[("server", s.into())]))?,
                None => "auto".into(),
            };
            // no selector on the router and no pin wanted: it picks automatically already
            if !engine::router::router_pick(&mut ssh, &secret, &tag) && server.is_some() {
                // the router's config has no selector yet: add it and push, then pin
                if engine::singbox_config::pick_add(&mut cfg) {
                    engine::data_files::jsave(&self.paths.router_cfg, &cfg, Some(2)).map_err(|e| e.to_string())?;
                }
                say("router: adding the node switch to its config and pushing it...");
                self.push_router()?;
                if !engine::router::router_pick(&mut ssh, &secret, &tag) {
                    return Err(tr("The router did not take the change; see the activity log.", &[]));
                }
            }
            done.push(tr("Router", &[]));
            self.read_router_now();
        }
        let who = done.join(" & ");
        let text = match server {
            Some(s) => tr("{who} now uses {server}, until it fails.", &[("who", who), ("server", s.into())]),
            None => tr("{who} picks its node automatically again.", &[("who", who)]),
        };
        say(&text);
        self.status(text, "ok");
        Ok(())
    }

    // Restart the PC tunnel on the configs (pc_restart)
    pub fn restart_pc(&self) {
        let live = nm::pc_push(self.paths);
        self.w().pc_pending = !live;
        self.check_local("pc");
        let (t, s) = if live {
            ("PC tunnel restarted.", "ok")
        } else {
            ("PC tunnel did not come back; see the activity log and logs/core.log.", "bad")
        };
        self.status(tr(t, &[]), s);
        engine::history::record(
            &self.paths.history,
            &[],
            Some(("restart", "PC tunnel restarted from the window")),
            now(),
        );
    }

    // Stop or Start (do_power); cmd starts the watcher
    pub fn power(&self, stop: bool, cmd: &[String], watcher: impl FnOnce(bool)) {
        let res = if stop {
            engine::startup_tasks::manager_stop(self.paths, cmd, watcher)
        } else {
            engine::startup_tasks::manager_start(self.paths, cmd, watcher)
        };
        self.w().boot_on = !matches!(engine::startup_tasks::task_state_here(self.paths), None | Some("Disabled"));
        if !stop {
            self.check_local("pc");
        }
        let bad = res.iter().any(|l| {
            l.contains("NOT")
                || l.split_once(':').is_some_and(|(_, r)| r.chars().take(12).collect::<String>().contains("not"))
                || l.contains("still")
        });
        let head = tr(if stop { "Reality Hop is stopped." } else { "Reality Hop is running." }, &[]);
        let lines: Vec<String> = res.iter().map(|l| engine::translations::cap_first(&tr(l, &[]))).collect();
        self.status(format!("{head} {}", lines.join(" ")), if stop || bad { "warn" } else { "ok" });
    }

    // The automatic check while the window is open: with auto-repair the watcher's own check, else a check that flags
    // nodes failing twice in a row (auto_run)
    pub fn auto_run(&self) -> Result<(), String> {
        if self.w().auto_fix {
            let mut st = std::mem::take(&mut self.w().watch);
            let out = engine::watcher::watch_cycle(self.paths, &mut RouterSsh { paths: self.paths }, &mut st);
            self.w().watch = st;
            let out = out?;
            self.forget_router();
            self.check_local("both");
            if let Some(o) = out {
                self.status(format!("{} {o}", tr("Auto-repair:", &[])), "ok");
            }
            return Ok(());
        }
        self.check_local("both");
        let mode = Mode::of(self.paths);
        let w = self.w();
        let due: Vec<String> = w
            .list(mode.main())
            .iter()
            .filter(|n| {
                let s = f(n, "server");
                let k = |l: &str| w.streak.get(&(l.to_string(), s.clone())).copied().unwrap_or(0);
                k("phone").max(k("pc")) >= 2
            })
            .map(|n| f(n, "tag"))
            .collect();
        drop(w);
        if !due.is_empty() {
            self.status(
                tr(
                    "{tags} failed two checks in a row. Replace with the button, or tick auto-repair.",
                    &[("tags", due.join(", "))],
                ),
                "bad",
            );
        }
        Ok(())
    }
}

// What the page draws: every part of the main window
pub fn snapshot(paths: &Paths, w: &Win) -> Value {
    let mode = Mode::of(paths);
    let s = Settings::read(paths).unwrap_or_default();
    let server = |lst: &str, tag: Option<&Value>| -> Option<String> {
        let tag = tag.and_then(Value::as_str)?;
        w.list(lst).iter().find(|n| f(n, "tag") == tag).map(|n| f(n, "server"))
    };
    let pc_srv = server("pc", w.pcs.as_ref().and_then(|p| p.get("now")));
    let r_now = w.router.as_ref().and_then(|r| r.0.get("now").cloned()).or_else(|| w.router_now.clone());
    let r_srv = server("phone", r_now.as_ref());
    let vault = engine::vault::load(paths);
    // the hosting company: the node's own, else the vault's
    let host = |n: &Value| {
        let v = vault.get(&f(n, "server")).map(|v| f(v, "isp")).unwrap_or_default();
        [f(n, "isp"), v].into_iter().find(|h| !h.is_empty() && h != "?").unwrap_or_else(|| "?".into())
    };
    let mut lists = Vec::new();
    for (key, label, file) in mode.lists() {
        let rows: Vec<Value> = w
            .list(key)
            .iter()
            .map(|n| {
                let (state, _, text) = w.facts(key, n);
                let mut tun = if key == "phone" && has(n, "phone_why") { n.get("phone_ms").cloned() } else { None };
                if tun.as_ref().is_none_or(Value::is_null) {
                    let pc = if key == "pc" {
                        Some(n)
                    } else {
                        w.list("pc").iter().find(|m| f(m, "server") == f(n, "server"))
                    };
                    tun = pc.filter(|p| has(p, "pc_why")).and_then(|p| p.get("pc_ms").cloned());
                }
                // a working node over the slow limit is orange, as in the tray ring, which judges by the PC tunnel (by
                // the router in router-only mode)
                let slow = state == "ok"
                    && (key == "pc" || mode.ronly)
                    && tun.as_ref().and_then(Value::as_f64).is_some_and(|m| m > s.slow_ms as f64);
                let (state, text) = if slow {
                    ("warn", tr("Working, but slow ({ms}ms+)", &[("ms", s.slow_ms.to_string())]))
                } else {
                    (state, text)
                };
                let mut active = Vec::new();
                if pc_srv.as_deref() == Some(f(n, "server").as_str()) {
                    active.push(tr("PC", &[]));
                }
                if r_srv.as_deref() == Some(f(n, "server").as_str()) {
                    active.push(tr("Router", &[]));
                }
                let mark = match state {
                    "ok" => "●  ",
                    "warn" => "◐  ",
                    "bad" => "✕  ",
                    _ => "○  ",
                };
                let using = if active.is_empty() {
                    String::new()
                } else {
                    format!("   ·   {}", tr("In use by {who}", &[("who", active.join(" & "))]))
                };
                json!({
                    "tag": f(n, "tag"), "server": f(n, "server"), "front": brand(&f(n, "sni")),
                    "where": if has(n, "where") { f(n, "where") } else { "?".into() }, "host": host(n),
                    "tcp": ms(n.get("tcp_ms"), &tr("None", &[])), "tunnel": ms(tun.as_ref(), "-"),
                    "state": if state == "bad" && !has(n, "tcp_ms") { "broken" } else { state },
                    "text": format!("{mark}{text}{using}"),
                    "lead": format!("{mark}{text}"),
                    "pc": pc_srv.as_deref() == Some(f(n, "server").as_str()),
                    "router": r_srv.as_deref() == Some(f(n, "server").as_str()),
                    "node": n,
                })
            })
            .collect();
        let stale = if key == "pc" { w.pending == Some(true) } else { w.router_stale };
        let mut note = Vec::new();
        if key == "phone" && w.router_away.is_some() {
            note.push(tr("Router not connected", &[]));
        }
        if stale {
            note.push(tr("Config newer than what runs", &[]));
        }
        lists.push(json!({"key": key, "label": label, "file": file, "note": note.join("  ·  "), "rows": rows}));
    }
    let mut tiles = Vec::new();
    let tile = |key: &str, label: &str, value: String, color: &str, sub: String, act: Option<&str>| json!({"key": key, "label": tr(label, &[]), "value": value, "color": color, "sub": sub, "act": act});
    if !mode.ronly {
        tiles.push(match &w.pcs {
            None => tile("pc", "PC TUNNEL", "-".into(), "unk", tr("Not checked yet", &[]), None),
            Some(p) if p["api"] == true || p["listening"] == true => {
                let old = p["api"] != true || w.pc_pending;
                let now = if p["now"].is_null() { "?".into() } else { f(p, "now") };
                let exit = if p["exit"].is_null() { "?".into() } else { f(p, "exit") };
                tile(
                    "pc",
                    "PC TUNNEL",
                    tr(if old { "Needs restart" } else { "Running" }, &[]),
                    if old { "warn" } else { "ok" },
                    format!("{now}  ·  {}", tr("Exit {ip}", &[("ip", exit)])),
                    if old { Some("restart_pc") } else { None },
                )
            }
            Some(_) => tile("pc", "PC TUNNEL", tr("Down", &[]), "bad", tr("Nothing on 127.0.0.1:2080", &[]), None),
        });
    }
    if mode.router {
        tiles.push(if w.router_away.is_some() {
            tile(
                "router",
                "ROUTER TUNNEL",
                tr("Not connected", &[]),
                "warn",
                tr("Not on the router's network; press Test on router", &[]),
                Some("test_router"),
            )
        } else {
            match &w.router {
                None => tile(
                    "router",
                    "ROUTER TUNNEL",
                    "-".into(),
                    "unk",
                    tr("Press Test on router", &[]),
                    Some("test_router"),
                ),
                Some((r, at)) if r["service"] == true => {
                    let now = if r["now"].is_null() { "?".into() } else { f(r, "now") };
                    tile(
                        "router",
                        "ROUTER TUNNEL",
                        tr(if w.router_stale { "Older config" } else { "Running" }, &[]),
                        if w.router_stale { "warn" } else { "ok" },
                        format!("{now}  ·  {}", tr("Checked {at}", &[("at", at.clone())])),
                        None,
                    )
                }
                Some((_, at)) => tile(
                    "router",
                    "ROUTER TUNNEL",
                    tr("Stopped", &[]),
                    "bad",
                    tr("Checked {at}; press Push to router", &[("at", at.clone())]),
                    Some("push_router"),
                ),
            }
        });
    }
    let key = if !mode.ronly && (w.list("pc").iter().any(|n| has(n, "pc_why")) || w.router.is_none()) {
        "pc"
    } else {
        "phone"
    };
    let nodes = w.list(key);
    let states: Vec<&str> = nodes.iter().map(|n| w.facts(key, n).0).collect();
    let (good, bad) = (states.iter().filter(|s| **s == "ok").count(), states.iter().filter(|s| **s == "bad").count());
    tiles.push(if nodes.is_empty() {
        tile("nodes", "NODES WORKING", "-".into(), "unk", String::new(), None)
    } else {
        let tested = good > 0 || bad > 0;
        // out of all 6 slots: an empty slot counts as a dead node
        let total = nodes.len().max(engine::node_manager::SLOTS);
        let value =
            if tested { format!("{good} / {total}") } else { tr("{n} reachable", &[("n", nodes.len().to_string())]) };
        let color = if bad == nodes.len() {
            "bad"
        } else if bad > 0 || good == 0 || (tested && good < total) {
            "warn"
        } else {
            "ok"
        };
        let sub = if !tested {
            tr("443 ping only", &[])
        } else if key == "phone" {
            tr("Router tunnel test", &[])
        } else {
            tr("PC tunnel test", &[])
        };
        tile("nodes", "NODES WORKING", value, color, sub, None)
    });
    let on = !settings::stopped(paths) && engine::startup_tasks::startup_on(paths);
    tiles.push(tile(
        "watch",
        "BACKGROUND",
        tr(if on { "On" } else { "Off" }, &[]),
        if on { "ok" } else { "unk" },
        tr(if on { "Hourly checks wait while this window is open" } else { "Tick Run in background at logon" }, &[]),
        None,
    ));
    let pill = match w.status_state.as_str() {
        "ok" => "All good",
        "warn" => "Attention",
        "bad" => "Problem",
        _ => "Checking",
    };
    let h = engine::history::load(&paths.history);
    let cut = now() - 86400.0;
    let events: Vec<Value> = h["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e.get(0).and_then(Value::as_f64).is_some_and(|t| t > cut))
        .cloned()
        .collect();
    json!({
        "mode": {"router": mode.router, "ronly": mode.ronly, "mac": !cfg!(windows), "backend": s.backend_files != 0},
        "version": engine::version_label(), "release": engine::VERSION,
        "core_mode": engine::pc_tunnel::core_mode(paths),
        "core": w.core_text, "rcore": w.rcore_text, "router_away": w.router_away,
        "stopped": settings::stopped(paths), "startup_on": engine::startup_tasks::startup_on(paths), "boot_on": w.boot_on,
        "check_min": s.check_min, "auto_on": w.auto_on, "auto_fix": w.auto_fix, "next_auto": w.next_auto,
        "status": {"text": w.status_text, "state": w.status_state, "pill": tr(pill, &[])},
        "tiles": tiles, "lists": lists, "events": events, "now": now(),
        "down": engine::history::drop_lengths(&events),
        "busy": w.busy, "prog": prog(w), "update": w.update.as_ref().map(|u| u.latest.ver.clone()),
        "main": mode.main(),
    })
}

// The last check's results, kept in last.json, so the window shows them at once (App.__init__)
pub fn load_last(paths: &Paths, w: &mut Win) {
    let last = engine::data_files::jload(&paths.last);
    if let Some(t) = last.get("took").and_then(Value::as_object) {
        w.took = t.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect();
    }
    let Some(lists) = last.get("lists").and_then(Value::as_object) else { return };
    for (k, cfg) in [("pc", &paths.pc_cfg), ("phone", &paths.router_cfg)] {
        let saved: Vec<Value> = lists.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let now: Vec<String> = nm::nodes_in(cfg).unwrap_or_default().iter().map(|n| f(n, "server")).collect();
        if !saved.is_empty() && saved.iter().map(|n| f(n, "server")).collect::<Vec<_>>() != now {
            return; // the nodes changed since: the saved results are not theirs
        }
        w.lists.insert(k.into(), saved);
    }
    let mode = Mode::of(paths);
    w.pcs = last.get("pcs").filter(|p| p.is_object() && !mode.ronly).cloned();
    if mode.ronly {
        w.lists.insert("pc".into(), vec![]);
    }
    w.router = last
        .get("router")
        .and_then(Value::as_array)
        .and_then(|r| Some((r.first()?.clone(), r.get(1)?.as_str()?.to_string())));
    w.judge(&mode);
    let at = last.get("at").and_then(Value::as_str).unwrap_or_default();
    w.status_text =
        tr("Last check {at}: {status} Checking again now...", &[("at", at.into()), ("status", w.status_text.clone())]);
}

// The busy bar: seconds since the running action started and how long it took last time; null while it waits
pub fn prog(w: &Win) -> Value {
    match (w.busy, w.started, &w.current) {
        (true, Some(s), Some(j)) => {
            json!({"el": s.elapsed().as_secs_f64(), "exp": w.took.get(&j.name).copied().unwrap_or(30.0)})
        }
        _ => Value::Null,
    }
}

// Keep the results in last.json (done)
pub fn save_last(paths: &Paths, w: &Win) {
    let mut m = Map::new();
    m.insert("lists".into(), json!({"pc": w.list("pc"), "phone": w.list("phone")}));
    m.insert("pcs".into(), w.pcs.clone().unwrap_or(Value::Null));
    m.insert("router".into(), w.router.as_ref().map_or(Value::Null, |(r, at)| json!([r, at])));
    m.insert("at".into(), json!(clock()));
    m.insert("took".into(), json!(w.took));
    let _ = engine::data_files::jsave(&paths.last, &Value::Object(m), None);
}
