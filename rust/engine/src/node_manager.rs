//! The node set in use: reading and writing it, finding and testing other nodes, replacing nodes, and deploying the
//! set to the PC tunnel and the router (engine.inuse, write_configs, push, deploy, scan, replace, rescue, router_sync).

use crate::activity_log::say;
use crate::data_files::{Newlines, jsave, read_json, read_text, write_text};
use crate::node_tests::{geolocate, node_test, router_test, tcp_probe};
use crate::paths::Paths;
use crate::router::{self, Ssh};
use crate::settings::{self, Settings, pc_only, router_only};
use crate::speed_test;
use md5::{Digest, Md5};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static PUSHED: AtomicBool = AtomicBool::new(true);
static API_SECRET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-fA-F]{16,128}$").unwrap());

/// One slot change: (tag, the server it held, the node now in it).
pub type Swap = (String, String, Value);

// time.time()
fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// A node's field as text, "" when missing
fn field(n: &Value, k: &str) -> String {
    n.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

// dict item assignment
fn set(n: &mut Value, k: &str, v: Value) {
    if let Some(m) = n.as_object_mut() {
        m.insert(k.into(), v);
    }
}

// dict.setdefault
fn set_default(n: &mut Value, k: &str, v: Value) {
    if let Some(m) = n.as_object_mut() {
        m.entry(k).or_insert(v);
    }
}

// A value's truth as Python sees it, false when missing
fn yes(n: &Value, k: &str) -> bool {
    n.get(k).is_some_and(settings::py_truthy)
}

// The settings, or the defaults when settings.json cannot be read
fn current(paths: &Paths) -> Settings {
    Settings::read(paths).unwrap_or_default()
}

// The servers of a node list
fn servers(nodes: &[Value]) -> Vec<String> {
    nodes.iter().map(|n| field(n, "server")).collect()
}

// Whether the last deploy reached the router (engine.PUSHED)
pub fn pushed() -> bool {
    PUSHED.load(Ordering::SeqCst)
}

// The vless nodes in one config file, either schema, as node objects (engine.nodes_of)
pub fn nodes_in(cfg: &Path) -> Result<Vec<Value>, String> {
    let list = crate::singbox_config::nodes_of(cfg).map_err(|e| e.to_string())?;
    Ok(list
        .into_iter()
        .map(|(tag, n)| {
            json!({"tag": tag, "server": n.server, "uuid": n.uuid, "sni": n.sni, "fp": n.fp, "pbk": n.pbk,
                "sid": n.sid, "tcp_ms": null})
        })
        .collect())
}

// The nodes in use. With router features off this PC goes by its own config (engine.inuse)
pub fn inuse(paths: &Paths) -> Result<Vec<Value>, String> {
    nodes_in(if pc_only(paths) { &paths.pc_cfg } else { &paths.router_cfg })
}

// Put the node set into both configs, each in its own schema; everything else in them stays (engine.write_configs)
pub fn write_configs(paths: &Paths, best: &[Value]) -> Result<(), String> {
    let pc = read_json(&paths.pc_cfg).map_err(|e| e.to_string())?;
    let r = read_json(&paths.router_cfg).map_err(|e| e.to_string())?;
    let nodes: Vec<_> = best.iter().map(router::node_of).collect();
    let (pc, r) = crate::singbox_config::write_configs(&pc, &r, &nodes);
    jsave(&paths.router_cfg, &r, Some(2)).map_err(|e| e.to_string())?;
    jsave(&paths.pc_cfg, &pc, Some(2)).map_err(|e| e.to_string())
}

// nodes.txt: the set in use, the spares and the failures, dated today (engine.write_txt)
pub fn write_txt(paths: &Paths, nodes: &[Value], best: &[Value]) -> Result<(), String> {
    let today = crate::local_time::now().format("%Y-%m-%d");
    let text = crate::node_tables::write_txt(nodes, best, &today);
    write_text(&paths.nodes_txt, &text, Newlines::Platform).map_err(|e| e.to_string())?;
    say("updated nodes.txt");
    Ok(())
}

// The working nodes, best first: at least the floor by latency, then untested, then the rest fastest first
// (engine.best_first)
pub fn best_first(paths: &Paths, nodes: &[Value]) -> Vec<Value> {
    let min = current(paths).min_mbps;
    let floor = crate::node_tests::floor_now(paths, min, now());
    let key = |n: &Value| {
        let f = |k: &str| n.get(k).and_then(Value::as_f64);
        speed_test::speed_rank(min, floor, f("mbps"), f("ms"))
    };
    let mut ok: Vec<Value> = nodes.iter().filter(|n| yes(n, "ok")).cloned().collect();
    ok.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
    ok
}

// The md5 of a file, "" when it cannot be read (engine.file_md5)
pub fn file_md5(path: &Path) -> String {
    std::fs::read(path).map(|b| Md5::digest(&b).iter().map(|x| format!("{x:02x}")).collect()).unwrap_or_default()
}

// Remember what the router runs and the router-config.json that matches it (engine.router_synced)
pub fn router_synced(paths: &Paths, router_md5: &str) {
    let kw = json!({"router_md5": router_md5, "router_local": file_md5(&paths.router_cfg)});
    let _ = settings::set_this_pc(paths, kw.as_object().cloned().unwrap_or_default());
}

// True when router-config.json has changes the router does not run yet (engine.router_stale)
pub fn router_stale(paths: &Paths, router_md5: &str) -> bool {
    let t = settings::this_pc(paths);
    let has = |k: &str, v: &str| t.get(k).and_then(Value::as_str) == Some(v);
    !router_md5.is_empty() && (!has("router_md5", router_md5) || !has("router_local", &file_md5(&paths.router_cfg)))
}

// True when the router answers over SSH; false with router features off (engine.router_up)
pub fn router_up(paths: &Paths, ssh: &mut impl Ssh) -> bool {
    !pc_only(paths)
        && ssh.run("echo up", None, router::SSH_TIMEOUT).is_ok_and(|o| crate::python_compat::strip(&o) == "up")
}

// Upload router-config.json and the device firewall rule, check the config on the router, restart its sing-box,
// install the self-repair. Copies of what goes to the router are kept in the Router folder (engine.push)
pub fn push(paths: &Paths, ssh: &mut impl Ssh) -> Result<String, String> {
    if pc_only(paths) {
        return Ok("router features are off: the router is not touched".into());
    }
    let devices = current(paths).devices;
    std::fs::create_dir_all(&paths.router_dir).map_err(|e| e.to_string())?;
    let nft = router::nft_script(router::NFT_SH, &devices);
    write_text(&paths.router_dir.join("nft-devices.sh"), &nft, Newlines::Lf).map_err(|e| e.to_string())?;
    std::fs::write(paths.router_dir.join("heal.sh"), router::HEAL_SH).map_err(|e| e.to_string())?;
    let cfg = read_json(&paths.router_cfg).map_err(|e| e.to_string())?;
    let used = servers(&inuse(paths).unwrap_or_default());
    let used: Vec<&str> = used.iter().map(String::as_str).collect();
    let vault = crate::vault::vault_nodes(paths, &used);
    let p = router::Push {
        router_cfg: &cfg,
        nft_template: router::NFT_SH,
        devices: &devices,
        heal_sh: router::HEAL_SH,
        vault: &vault,
    };
    let out = router::push(ssh, &p).map_err(|e| e.to_string())?;
    if let Some(md5) = &out.md5 {
        router_synced(paths, md5);
    }
    Ok(out.answer)
}

// pc-config.json -> the running PC tunnel (rh-core restart, about 2 s). Nothing in router-only mode, and nothing while
// stopped: the configs are used at the next Start (engine.pc_push)
pub fn pc_push(paths: &Paths) -> bool {
    if router_only(paths) {
        return true;
    }
    if !crate::pc_tunnel::core_mode(paths) {
        say("rh-core.exe is missing; run the setup again");
        return false;
    }
    if settings::stopped(paths) {
        say("Reality Hop is stopped: the tunnel stays off; the configs are saved and used at the next Start");
        return true;
    }
    crate::pc_tunnel::core_restart(paths)
}

// A node set that is not urgent for the PC tunnel: another PC's (or the router's self-repair's) choice, or a faster
// node for a slow one. A restart drops a VPN connected through the tunnel, and the nodes it runs still work: while PIA
// is connected the new set waits, and the PC tunnel takes it the moment PIA is not connected, at its next restart for
// another reason, or when PIA cannot reconnect. Without PIA, or with the tunnel down, at once
fn pc_push_when_free(paths: &Paths) {
    use crate::pc_tunnel::{core_mode, core_running};
    if core_mode(paths) && core_running(paths) && crate::watcher::pia_connected() {
        say(
            "PIA is connected through the PC tunnel: it keeps its working nodes for now and takes the new set when PIA is not connected or at its next restart",
        );
        return;
    }
    pc_push(paths);
}

// Deploy both configs: to the router (push) and to the PC tunnel (pc_push). A router that cannot be reached does not
// stop the PC tunnel restart. (router ok, router answer) (engine.deploy_files)
pub fn deploy_files(paths: &Paths, ssh: &mut impl Ssh) -> (bool, String) {
    deploy_files_how(paths, ssh, true)
}

// deploy_files; pc_now false: the PC tunnel takes the set when a restart costs nothing (pc_push_when_free)
fn deploy_files_how(paths: &Paths, ssh: &mut impl Ssh, pc_now: bool) -> (bool, String) {
    let out = push(paths, ssh).unwrap_or_else(|e| format!("router not updated: {e}"));
    say(&out);
    if pc_now {
        pc_push(paths);
    } else {
        pc_push_when_free(paths);
    }
    let ok = out.contains("sing-box service: running") || pc_only(paths);
    PUSHED.store(ok, Ordering::SeqCst);
    (ok, out)
}

// True when the running PC tunnel has a different node set than pc-config.json; None when it cannot tell
// (engine.pending_deploy)
pub fn pending_deploy(paths: &Paths) -> Option<bool> {
    let nodes = crate::singbox_config::nodes_of(&paths.pc_cfg).ok()?;
    let mark = crate::singbox_config::set_mark(nodes.iter().map(|(_, n)| n.server.as_str()));
    let api = crate::clash_api::ControlApi::pc(paths).ok()?;
    let v: Value = serde_json::from_str(&api.get("/proxies", Duration::from_secs(3)).ok()?).ok()?;
    Some(!v.get("proxies")?.as_object()?.contains_key(&mark))
}

// Write the node set everywhere, push it to the router, restart the PC tunnel on it. True when the router took it
// (engine.deploy)
pub fn deploy(paths: &Paths, ssh: &mut impl Ssh, best: &[Value], tested: Option<&[Value]>) -> Result<bool, String> {
    deploy_how(paths, ssh, best, tested, true)
}

// deploy; pc_now false: the PC tunnel takes the set when a restart costs nothing (pc_push_when_free)
fn deploy_how(
    paths: &Paths,
    ssh: &mut impl Ssh,
    best: &[Value],
    tested: Option<&[Value]>,
    pc_now: bool,
) -> Result<bool, String> {
    write_configs(paths, best)?;
    write_txt(paths, tested.unwrap_or(best), best)?;
    say(&format!("wrote {} nodes to both configs; deploying...", best.len()));
    Ok(deploy_files_how(paths, ssh, pc_now).0)
}

// The 443 ping: from this PC; when nothing at all answers (this PC is offline), from the router. In router-only mode
// always from the router, since this PC's traffic may go through its tunnel. The indexes that answered, fastest first
// (engine.probe)
pub fn probe(paths: &Paths, ssh: &mut impl Ssh, nodes: &mut [Value]) -> Result<Vec<usize>, String> {
    if router_only(paths) {
        return router::router_ping(ssh, nodes).map_err(|e| e.to_string());
    }
    let live = tcp_probe(nodes);
    if !live.is_empty() || nodes.len() < 5 || pc_only(paths) {
        return Ok(live);
    }
    say("nothing answers from this PC (no internet here?); pinging from the router instead...");
    router::router_ping(ssh, nodes).map_err(|e| e.to_string())
}

// The public node list as node objects (engine.feed)
fn feed(paths: &Paths, ssh: &mut impl Ssh) -> Result<Vec<Value>, String> {
    let nodes = crate::downloads::feed(paths, ssh)?;
    Ok(nodes.into_iter().map(|n| serde_json::to_value(n).unwrap_or_default()).collect())
}

// Test the pool (best 443 ping first) in batches of top, until `need` pass or the pool runs out: a short first batch
// must not settle for fewer nodes while more of the list could work. Every node tested, in order
fn test_until(
    paths: &Paths,
    ssh: &mut impl Ssh,
    pool: Vec<Value>,
    top: usize,
    need: usize,
) -> Result<Vec<Value>, String> {
    let mut tested: Vec<Value> = Vec::new();
    let mut rest = pool.into_iter().peekable();
    while rest.peek().is_some() {
        let mut batch: Vec<Value> = rest.by_ref().take(top.max(1)).collect();
        if !tested.is_empty() {
            let ok = tested.iter().filter(|n| yes(n, "ok")).count();
            say(&format!("{ok} of {need} found; testing the next {} candidates...", batch.len()));
        }
        router_test(paths, ssh, &mut batch, true)?;
        tested.extend(batch);
        if tested.iter().filter(|n| yes(n, "ok")).count() >= need {
            break;
        }
    }
    Ok(tested)
}

// Working nodes from the node list that are not in use, best first; also everything tested. Testing goes on past the
// first `top` until `need` work (engine.find_spares)
pub fn find_spares(
    paths: &Paths,
    ssh: &mut impl Ssh,
    top: usize,
    need: usize,
    exclude: &HashSet<String>,
) -> Result<(Vec<Value>, Vec<Value>), String> {
    let c = feed(paths, ssh)?;
    say(&format!("feed: {} candidates with plausible fronts", c.len()));
    let mut cand: Vec<Value> = c.into_iter().filter(|n| !exclude.contains(&field(n, "server"))).collect();
    let live = probe(paths, ssh, &mut cand)?;
    say(&format!(
        "answering on 443: {}, handshake-testing the {} fastest on this PC...",
        live.len(),
        top.min(live.len())
    ));
    let pool: Vec<Value> = live.iter().map(|&i| cand[i].clone()).collect();
    let test = test_until(paths, ssh, pool, top, need)?;
    Ok((best_first(paths, &test), test))
}

// The full ranking: the nodes in use plus the fastest candidates, all handshake-tested. Testing goes on past the first
// `top` candidates until SLOTS nodes pass (engine.scan)
pub fn scan(paths: &Paths, ssh: &mut impl Ssh, top: Option<usize>) -> Result<Vec<Value>, String> {
    let top = top.unwrap_or_else(|| current(paths).top.max(0) as usize);
    let mut all = inuse(paths)?;
    let k = all.len();
    let used: HashSet<String> = servers(&all).into_iter().collect();
    for n in all.iter_mut() {
        set(n, "inuse", json!(true));
    }
    let c: Vec<Value> = feed(paths, ssh)?.into_iter().filter(|n| !used.contains(&field(n, "server"))).collect();
    say(&format!("feed: {} candidates with plausible fronts", c.len()));
    all.extend(c);
    let live: Vec<usize> = probe(paths, ssh, &mut all)?.into_iter().filter(|&i| !yes(&all[i], "inuse")).collect();
    say(&format!("answering on 443: {}, handshake-testing the {top} fastest plus the {k} in use...", live.len()));
    let mut nodes: Vec<Value> = all[..k].to_vec();
    let first = live.len().min(top);
    nodes.extend(live[..first].iter().map(|&i| all[i].clone()));
    router_test(paths, ssh, &mut nodes, true)?;
    let ok = nodes.iter().filter(|n| yes(n, "ok")).count();
    if ok < SLOTS && first < live.len() {
        let pool: Vec<Value> = live[first..].iter().map(|&i| all[i].clone()).collect();
        say(&format!("{ok} of {SLOTS} found; testing the next {} candidates...", (live.len() - first).min(top)));
        let mut more = test_until(paths, ssh, pool, top, SLOTS - ok)?;
        nodes.append(&mut more);
    }
    for l in crate::node_tables::table(&nodes) {
        say(&l);
    }
    say(&format!("\n{} of {} pass", nodes.iter().filter(|n| yes(n, "ok")).count(), nodes.len()));
    Ok(nodes)
}

// Working nodes from the vault (nodes that passed a test before), tested again now, best first; also everything
// tested (engine.vault_spares)
pub fn vault_spares(
    paths: &Paths,
    ssh: &mut impl Ssh,
    top: usize,
    exclude: &HashSet<String>,
) -> Result<(Vec<Value>, Vec<Value>), String> {
    let ex: Vec<&str> = exclude.iter().map(String::as_str).collect();
    let mut test: Vec<Value> = crate::vault::vault_nodes(paths, &ex).into_iter().take(top).collect();
    if !test.is_empty() {
        say(&format!("testing {} nodes that worked before (vault)...", test.len()));
        router_test(paths, ssh, &mut test, true)?;
    }
    Ok((best_first(paths, &test), test))
}

/// The number of nodes in use the app aims for.
pub const SLOTS: usize = 6;

// Swap the in-use nodes at the given slots (0-based) for the best tested spares, from the online node list or (vault)
// from the nodes that worked before, fill empty slots up to SLOTS, then deploy. fast_only: only spares that reach the
// floor; with none, nothing changes (engine.replace)
pub fn replace(
    paths: &Paths,
    ssh: &mut impl Ssh,
    slots: &[usize],
    top: Option<usize>,
    vault: bool,
    fast_only: bool,
) -> Result<Vec<Swap>, String> {
    let top = top.unwrap_or_else(|| current(paths).top.max(0) as usize);
    if router_up(paths, ssh)
        && let Some(s) = router_sync(paths, ssh)?
    {
        return Err(format!("{s} Nothing replaced; check again and pick the node to fix."));
    }
    let mut cur = inuse(paths)?;
    let mut ex: HashSet<String> = servers(&cur).into_iter().collect();
    ex.extend(crate::clash_api::tunnel_fails(paths, now()).keys().cloned());
    let mut vault = vault;
    let need = slots.len() + SLOTS.saturating_sub(cur.len());
    let found = if vault { vault_spares(paths, ssh, top, &ex) } else { find_spares(paths, ssh, top, need, &ex) };
    let (mut spares, mut tested) = match found {
        Ok(f) => f,
        Err(e) if !vault => {
            say(&format!("node list unavailable ({e}); trying the nodes that worked before (vault)"));
            vault = true;
            vault_spares(paths, ssh, top, &ex)?
        }
        Err(e) => return Err(e),
    };
    if spares.is_empty() && !vault {
        say("no working spare in the node list; trying the nodes that worked before (vault)");
        vault = true;
        (spares, tested) = vault_spares(paths, ssh, top, &ex)?;
    }
    if fast_only {
        let floor = crate::node_tests::floor_now(paths, current(paths).min_mbps, now());
        spares.retain(|s| s.get("mbps").and_then(Value::as_f64).unwrap_or(0.0) >= floor as f64);
        if spares.is_empty() {
            say(&format!("no spare reaches {floor} Mbps; the slow nodes stay"));
            return Ok(Vec::new());
        }
    }
    if spares.is_empty() {
        let from = if vault { "among the nodes that worked before" } else { "in the online node list" };
        return Err(format!("no working spare found {from}; nothing changed"));
    }
    if spares.len() < need {
        say(&format!("only {} working spare(s) for {need} slot(s); replacing what can be replaced", spares.len()));
    }
    let mut swaps = Vec::new();
    let mut spares = spares.into_iter();
    // the given slots first; then empty slots up to SLOTS, after a scan that found fewer working nodes
    let mut todo: Vec<usize> = slots.iter().copied().filter(|&i| i < cur.len()).collect();
    todo.extend(cur.len()..SLOTS.max(cur.len()));
    for i in todo {
        let Some(s) = spares.next() else { break };
        let old = cur.get(i).map_or("(empty)".to_string(), |n| field(n, "server"));
        let mbps = s.get("mbps").filter(|m| !m.is_null()).map_or(String::new(), |m| format!(", {m} Mbps"));
        let ms = crate::python_json::py_str(s.get("ms").unwrap_or(&Value::Null));
        let place = s.get("where").map_or("?".to_string(), crate::python_json::py_str);
        say(&format!("vless-{}: {old} -> {}  ({ms}ms{mbps}, {place})", i + 1, field(&s, "server")));
        swaps.push((format!("vless-{}", i + 1), old, s.clone()));
        if i < cur.len() { cur[i] = s } else { cur.push(s) }
    }
    let kept: Vec<usize> = (0..cur.len()).filter(|&i| cur[i].get("ms").is_none_or(Value::is_null)).collect();
    if !kept.is_empty() && (paths.core.exists() || router_only(paths)) {
        // the same test the spares got, so the order below compares like with like
        let mut k: Vec<Value> = kept.iter().map(|&i| cur[i].clone()).collect();
        node_test(paths, ssh, &mut k, false)?;
        for (&i, n) in kept.iter().zip(k) {
            cur[i] = n;
        }
    }
    geolocate(paths, &mut cur);
    for n in cur.iter_mut() {
        set(n, "ok", json!(true));
    }
    let ms_key = |n: &Value| n.get("ms").and_then(Value::as_f64).filter(|m| *m != 0.0).unwrap_or(9e9);
    cur.sort_by(|a, b| ms_key(a).total_cmp(&ms_key(b))); // fastest first: vless-1 is the lowest ping
    let order: Vec<String> = cur
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let ms = n.get("ms").filter(|m| settings::py_truthy(m)).map_or("?".into(), crate::python_json::py_str);
            format!("vless-{} {} {ms} ms", i + 1, field(n, "server"))
        })
        .collect();
    say(&format!("slots ordered fastest first: {}", order.join(", ")));
    let in_set: HashSet<String> = servers(&cur).into_iter().collect();
    let mut all = cur.clone();
    all.extend(tested.into_iter().filter(|t| !in_set.contains(&field(t, "server"))));
    // fast_only is the speed swap: the slow nodes still work, so the PC tunnel switches when a restart costs nothing
    deploy_how(paths, ssh, &cur, Some(&all), !fast_only)?;
    Ok(swaps)
}

// Configs only, nothing deployed: each (slot, node) goes into both configs; a slot past the end fills the next free
// one (engine.swap_files)
pub fn swap_files(paths: &Paths, pairs: Vec<(usize, Value)>) -> Result<Vec<Swap>, String> {
    let mut cur = inuse(paths)?;
    let meta = crate::node_tables::known_where(&read_text(&paths.nodes_txt).unwrap_or_default());
    let mut out: Vec<(String, String, usize, Value)> = Vec::new();
    for (i, node) in pairs {
        if i < cur.len() {
            out.push((format!("vless-{}", i + 1), field(&cur[i], "server"), i, node.clone()));
            cur[i] = node;
        } else {
            out.push((format!("vless-{}", cur.len() + 1), "(empty)".into(), cur.len(), node.clone()));
            cur.push(node);
        }
    }
    for n in cur.iter_mut() {
        set_default(n, "ok", json!(true));
        let phone = n.get("phone_ms").cloned().unwrap_or(Value::Null);
        set_default(n, "ms", phone);
        if let Some((_, w)) = meta.iter().find(|(s, _)| *s == field(n, "server")) {
            set_default(n, "where", json!(w.place));
            set_default(n, "isp", json!(w.isp));
        }
    }
    write_configs(paths, &cur)?;
    write_txt(paths, &cur, &cur)?;
    let said: String = out.iter().map(|(t, o, _, n)| format!("; {t}: {o} -> {}", field(n, "server"))).collect();
    say(&format!("configs updated{said}"));
    // each node as the configs now hold it, unless a later pair took its slot again
    let slots: Vec<usize> = out.iter().map(|e| e.2).collect();
    Ok(out
        .into_iter()
        .enumerate()
        .map(|(k, (t, o, i, n))| if slots[k + 1..].contains(&i) { (t, o, n) } else { (t, o, cur[i].clone()) })
        .collect())
}

// Node list candidates not in use and not known to fail, ranked by the 443 ping from this PC only; not tunnel-tested
// (engine.fastest_local)
pub fn fastest_local(
    paths: &Paths,
    ssh: &mut impl Ssh,
    k: usize,
    exclude: &HashSet<String>,
) -> Result<Vec<Value>, String> {
    let txt = read_text(&paths.nodes_txt).unwrap_or_default();
    let where_ = crate::node_tables::known_where(&txt);
    let mut skip = exclude.clone();
    skip.extend(crate::node_tables::known_failed(&txt));
    skip.extend(crate::clash_api::tunnel_fails(paths, now()).keys().cloned());
    let mut c: Vec<Value> = feed(paths, ssh)?.into_iter().filter(|n| !skip.contains(&field(n, "server"))).collect();
    let live = probe(paths, ssh, &mut c)?;
    Ok(live
        .into_iter()
        .take(k)
        .map(|i| {
            let mut n = c[i].clone();
            let w = where_
                .iter()
                .find(|(s, _)| *s == field(&n, "server"))
                .map_or("?".to_string(), |(_, w)| w.place.clone());
            set(&mut n, "where", json!(w));
            n
        })
        .collect())
}

// Files only, no router: the fastest 443-ping candidates go into the slots. Deploy later with deploy_files
// (engine.replace_local)
pub fn replace_local(paths: &Paths, ssh: &mut impl Ssh, slots: &[usize]) -> Result<Vec<Swap>, String> {
    let used: HashSet<String> = servers(&inuse(paths)?).into_iter().collect();
    let cands = fastest_local(paths, ssh, slots.len(), &used)?;
    if cands.is_empty() {
        return Err("no candidate answers on 443 from this PC; nothing changed".into());
    }
    swap_files(paths, slots.iter().copied().zip(cands).collect())
}

// One chosen node into one slot (0-based) of both configs; deployed unless deploy_now is false (engine.put)
pub fn put(paths: &Paths, ssh: &mut impl Ssh, slot: usize, node: Value, deploy_now: bool) -> Result<Swap, String> {
    let sw = swap_files(paths, vec![(slot, node)])?.remove(0);
    if deploy_now {
        deploy_files(paths, ssh);
    }
    Ok(sw)
}

// The k best of tested nodes into use (engine.apply)
pub fn apply(paths: &Paths, ssh: &mut impl Ssh, nodes: &[Value], k: usize) -> Result<(), String> {
    let best: Vec<Value> = best_first(paths, nodes).into_iter().take(k).collect();
    if best.len() < 2 {
        return Err("fewer than 2 passing nodes, not applying".into());
    }
    deploy(paths, ssh, &best, Some(nodes)).map(|_| ())
}

// Both live checks on the nodes in use: (router side, PC side) (engine.check_all)
pub fn check_all(
    paths: &Paths,
    ssh: &mut impl Ssh,
    nodes: &mut [Value],
) -> Result<(router::PhoneCheck, Option<crate::clash_api::PcCheck>), String> {
    geolocate(paths, nodes);
    let secret = router::secret(&read_json(&paths.router_cfg).map_err(|e| e.to_string())?);
    let ph = router::check_phone(ssh, nodes, &secret).map_err(|e| e.to_string())?;
    let pc = crate::clash_api::check_pc(paths, nodes, true, now()).map_err(|_| "not a JSON object".to_string())?;
    Ok((ph, pc))
}

// All nodes dead: test the vault, put the best k in both files, deploy both sides. Needs no internet on this PC. Too
// few working vault nodes: the node list is scanned too. Nothing can test (no rh-core, router unreachable): the most
// recently verified vault nodes go in untested. A sentence for the window (engine.rescue)
pub fn rescue(paths: &Paths, ssh: &mut impl Ssh, k: usize) -> Result<String, String> {
    let cur = inuse(paths)?;
    let mut pool: Map<String, Value> = Map::new();
    for n in crate::vault::vault_nodes(paths, &[]) {
        pool.insert(field(&n, "server"), n);
    }
    for n in &cur {
        pool.entry(field(n, "server")).or_insert_with(|| n.clone());
    }
    let core = paths.core.exists();
    if !core && !router_up(paths, ssh) {
        let used: HashSet<String> = servers(&cur).into_iter().collect();
        let mut best: Vec<Value> =
            pool.values().filter(|n| !used.contains(&field(n, "server"))).take(k).cloned().collect();
        if best.is_empty() {
            best = pool.values().take(k).cloned().collect();
        }
        if best.is_empty() {
            return Err("the vault is empty and nothing can test nodes; nothing to fall back on".into());
        }
        write_configs(paths, &best)?;
        return Ok(format!(
            "Nothing could test nodes (no rh-core, router unreachable): the {} most recently verified vault nodes went into the configs untested. Run the setup again.",
            best.len()
        ));
    }
    let top = current(paths).top.max(0) as usize;
    let mut test: Vec<Value> = pool.values().take(top).cloned().collect();
    say(&format!("rescue: {} is testing {} vault nodes...", if core { "this PC" } else { "the router" }, test.len()));
    router_test(paths, ssh, &mut test, true)?;
    let mut ok = best_first(paths, &test);
    say(&format!("rescue: {} of {} vault nodes work", ok.len(), test.len()));
    if ok.len() < k {
        say("rescue: too few, scanning the feed for more...");
        match find_spares(paths, ssh, top, k - ok.len(), &servers(&test).into_iter().collect()) {
            Ok((more, _)) => ok.extend(more),
            Err(e) => say(&format!("rescue: feed scan failed ({e}); using what works")),
        }
    }
    if ok.is_empty() {
        return Err("no vault node and no feed node works right now; nothing changed".into());
    }
    let mut best = best_first(paths, &ok);
    best.truncate(k);
    let router_ok = deploy(paths, ssh, &best, Some(&test))?;
    let where_ = if pc_only(paths) {
        "the PC tunnel"
    } else if router_ok {
        "the PC tunnel and on the router"
    } else {
        "the PC tunnel (router push FAILED, press Push to router later)"
    };
    let ms = crate::python_json::py_str(best[0].get("ms").unwrap_or(&Value::Null));
    Ok(format!("Rescue done: {} working nodes (best {ms} ms) are in {where_}.", best.len()))
}

// Several PCs can share one router, and the router runs what was pushed last. When its config changed since this PC
// last pushed or looked (another PC pushed, or the router's self-repair swapped nodes), this PC takes the router's
// nodes and device list instead of pushing its own over them. A sentence when the nodes changed (engine.router_sync)
pub fn router_sync(paths: &Paths, ssh: &mut impl Ssh) -> Result<Option<String>, String> {
    let out = ssh
        .run(
            "md5sum /etc/sing-box/config.json 2>/dev/null | cut -c1-32; echo ---; cat /etc/sing-box/rh-devices 2>/dev/null; echo; echo ---; cat /etc/sing-box/heal.state 2>/dev/null; echo; echo ---; cat /etc/sing-box/config.json 2>/dev/null",
            None,
            router::SSH_TIMEOUT,
        )
        .map_err(|e| e.to_string())?;
    let mut parts: Vec<String> = out.splitn(4, "---\n").map(|p| crate::python_compat::strip(p).to_string()).collect();
    parts.resize(4, String::new());
    let (md5, devs, heal, cfg) = (&parts[0], &parts[1], &parts[2], &parts[3]);
    if md5.is_empty() || settings::this_pc(paths).get("router_md5").and_then(Value::as_str) == Some(md5.as_str()) {
        return Ok(None);
    }
    let Ok(Value::Object(r)) = serde_json::from_str::<Value>(cfg) else { return Ok(None) };
    let Some(outs) = r.get("outbounds").and_then(Value::as_array) else { return Ok(None) };
    let mut nodes: Vec<Value> = Vec::new();
    for o in outs.iter().filter(|o| o.get("type") == Some(&json!("vless"))) {
        let Some((tag, n)) = crate::singbox_config::from_outbound(o) else { return Ok(None) };
        nodes.push(json!({"tag": tag, "server": n.server, "uuid": n.uuid, "sni": n.sni, "fp": n.fp, "pbk": n.pbk,
            "sid": n.sid, "tcp_ms": null}));
    }
    let Some(api) = r.get("experimental").and_then(|e| e.get("clash_api")).and_then(|c| c.get("secret")) else {
        return Ok(None);
    };
    let bad = nodes.is_empty() || !nodes.iter().all(|n| crate::validate::node_ok(&router::node_of(n)));
    if bad || !API_SECRET.is_match(&crate::python_json::py_str(api)) {
        say("the router's config has nodes Reality Hop would not use; not following it. Push to router replaces it.");
        return Ok(None);
    }
    let before = servers(&inuse(paths)?);
    let meta = crate::vault::load(paths);
    for n in nodes.iter_mut() {
        set(n, "ms", Value::Null);
        // a node another PC found is not in this PC's vault: no latency or place known yet
        if let Some(e) = meta.get(&field(n, "server")).and_then(Value::as_object) {
            for (k, v) in
                e.iter().filter(|(k, v)| ["where", "isp", "fp", "ms"].contains(&k.as_str()) && settings::py_truthy(v))
            {
                set(n, k, v.clone());
            }
        }
    }
    // only its secret: so this PC can still test it
    let mut c = read_json(&paths.router_cfg).map_err(|e| e.to_string())?;
    let slot = c
        .get_mut("experimental")
        .and_then(|e| e.get_mut("clash_api"))
        .and_then(Value::as_object_mut)
        .ok_or("'experimental'")?;
    slot.insert("secret".into(), api.clone());
    jsave(&paths.router_cfg, &c, Some(2)).map_err(|e| e.to_string())?;
    write_configs(paths, &nodes)?;
    write_txt(paths, &nodes, &nodes)?;
    if devs == "all" || (!devs.is_empty() && devs.split_whitespace().all(crate::validate::lan_ip)) {
        let mut m = Map::new();
        m.insert("devices".into(), json!(devs));
        Settings::save(paths, &m).map_err(|e| e.to_string())?;
    }
    router_synced(paths, md5);
    if !heal.is_empty() {
        ssh.run("rm -f /etc/sing-box/heal.state", None, router::SSH_TIMEOUT).map_err(|e| e.to_string())?;
    }
    if servers(&nodes) == before {
        return Ok(None);
    }
    pc_push_when_free(paths);
    let who = if heal.is_empty() {
        "Another PC changed the router's nodes"
    } else {
        "The router repaired itself with vault nodes"
    };
    Ok(Some(format!("{who} ({}); this PC follows them now.", servers(&nodes).join(", "))))
}
