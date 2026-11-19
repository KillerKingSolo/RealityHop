//! The Candidates dialog: nodes from the public node list that could replace yours. Ping them from this PC, test the
//! fastest, then put picked ones into slots (Candidates).

use crate::window_state::{Mode, Worker, f, has, ms, now};
use engine::activity_log::say;
use engine::node_manager as nm;
use engine::router_ssh::RouterSsh;
use engine::settings::Settings;
use engine::translations::tr;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;

/// What the dialog shows; nodes are the candidates with their results.
#[derive(Default)]
pub struct Cand {
    pub open: bool,
    pub nodes: Vec<Value>,
    pub msg: String,
}

// ok is True / False / None, as Python tells them apart
fn is(n: &Value, b: bool) -> bool {
    n.get("ok") == Some(&Value::Bool(b))
}

fn untested(n: &Value) -> bool {
    !has(n, "ok")
}

impl Worker<'_> {
    // The slot count: the nodes in the main table, or 6 (slots)
    fn slots(&self) -> usize {
        let main = Mode::of(self.paths).main();
        match self.w().lists.get(main).map_or(0, Vec::len) {
            0 => 6,
            n => n,
        }
    }

    fn set_msg(&self, msg: String) {
        self.w().cand.msg = msg;
    }

    // Download the node list and ping 443 from this PC; nodes that failed before go last (ping)
    pub fn cand_ping(&self) -> Result<(), String> {
        let mut ssh = RouterSsh { paths: self.paths };
        say("candidates: downloading the node list...");
        let used: HashSet<String> = {
            let w = self.w();
            ["phone", "pc"].iter().flat_map(|k| w.lists.get(*k).into_iter().flatten().map(|n| f(n, "server"))).collect()
        };
        let feed = engine::downloads::feed(self.paths, &mut ssh)?;
        let mut c: Vec<Value> = feed
            .into_iter()
            .map(|n| serde_json::to_value(n).unwrap_or_default())
            .filter(|n| !used.contains(&f(n, "server")))
            .collect();
        say(&format!("candidates: {} with plausible fronts; pinging 443 from this PC...", c.len()));
        let txt = engine::data_files::read_text(&self.paths.nodes_txt).unwrap_or_default();
        let place = engine::node_tables::known_where(&txt);
        let failed: HashSet<String> = engine::node_tables::known_failed(&txt).into_iter().collect();
        let dead = engine::clash_api::tunnel_fails(self.paths, now());
        for n in c.iter_mut() {
            let s = f(n, "server");
            n["where"] = json!(place.iter().find(|(x, _)| *x == s).map_or("?".to_string(), |(_, w)| w.place.clone()));
            if let Some(at) = dead.get(&s).and_then(Value::as_f64) {
                let at = engine::local_time::at(at as i64).format("%H:%M");
                n["ok"] = json!(false);
                n["why"] = json!(tr("failed in your PC tunnel at {at}", &[("at", at)]));
                n["ms"] = Value::Null;
                n["known"] = json!(true);
            } else if failed.contains(&s) {
                n["ok"] = json!(false);
                n["why"] = json!(tr("failed the last scan", &[]));
                n["ms"] = Value::Null;
                n["known"] = json!(true);
            }
        }
        let total = c.len();
        let live = nm::probe(self.paths, &mut ssh, &mut c)?;
        let mut nodes: Vec<Value> = live.into_iter().map(|i| c[i].clone()).collect();
        nodes.sort_by_key(|n| is(n, false)); // stable: fastest 443 first within each group
        let msg = tr(
            "{ok} of {n} candidates answer on 443 from this PC. None is tunnel-tested yet.",
            &[("ok", nodes.len().to_string()), ("n", total.to_string())],
        );
        let mut w = self.w();
        w.cand.nodes = nodes;
        w.cand.msg = msg;
        Ok(())
    }

    // Test the fastest untested candidates until a full set works or none are left (test)
    pub fn cand_test(&self) -> Result<(), String> {
        let mut nodes = std::mem::take(&mut self.w().cand.nodes);
        let r = self.test_nodes(&mut nodes);
        self.w().cand.nodes = nodes;
        r
    }

    fn test_nodes(&self, nodes: &mut [Value]) -> Result<(), String> {
        if nodes.is_empty() {
            self.set_msg(tr("Nothing to test; ping first.", &[]));
            return Ok(());
        }
        let s = Settings::read(self.paths).unwrap_or_default();
        let (k, top_n) = (self.slots(), s.top.max(1) as usize);
        let mut ssh = RouterSsh { paths: self.paths };
        let mut top: Vec<usize> = Vec::new();
        loop {
            let batch: Vec<usize> =
                (0..nodes.len()).filter(|i| untested(&nodes[*i]) && !top.contains(i)).take(top_n).collect();
            if batch.is_empty() {
                break;
            }
            let more = if top.is_empty() { "" } else { " untested" };
            say(&format!(
                "candidates: this PC is testing the {} fastest{more}, 5 at a time (about 20 seconds)...",
                batch.len()
            ));
            let mut b: Vec<Value> = batch.iter().map(|i| nodes[*i].clone()).collect();
            let r = engine::node_tests::router_test(self.paths, &mut ssh, &mut b, true);
            for (i, n) in batch.iter().zip(b) {
                nodes[*i] = n;
            }
            r?;
            top.extend(batch);
            if engine::process::cancelled() || nm::best_first(self.paths, nodes).len() >= k {
                break;
            }
        }
        let tested: Vec<Value> = top.iter().map(|i| nodes[*i].clone()).collect();
        let ok = nm::best_first(self.paths, &tested);
        let floor = engine::node_tests::floor_now(self.paths, s.min_mbps, now());
        let num = |n: &Value, k: &str| n.get(k).and_then(Value::as_f64);
        let key = |n: &Value| {
            let rank = if is(n, true) {
                engine::speed_test::speed_rank(s.min_mbps, floor, num(n, "mbps"), num(n, "ms"))
            } else {
                (9, 9e9)
            };
            (!is(n, true), rank, num(n, "tcp_ms").unwrap_or(f64::INFINITY))
        };
        nodes.sort_by(|a, b| {
            let (x, y) = (key(a), key(b));
            x.0.cmp(&y.0).then(x.1.0.cmp(&y.1.0)).then(x.1.1.total_cmp(&y.1.1)).then(x.2.total_cmp(&y.2))
        });
        let mut msg =
            tr("{ok} of {n} tested candidates work.", &[("ok", ok.len().to_string()), ("n", top.len().to_string())]);
        msg += " ";
        msg += &match ok.first() {
            Some(b) => tr(
                "Best: {server} at {ms} ms ({where}).",
                &[
                    ("server", f(b, "server")),
                    ("ms", f(b, "ms")),
                    ("where", if has(b, "where") { f(b, "where") } else { "?".into() }),
                ],
            ),
            None => tr("Ping again later or widen the test.", &[]),
        };
        self.set_msg(msg);
        Ok(())
    }

    // The running PC tunnel fetches through each node in its slots, as Check now does (test_tunnel)
    pub fn cand_test_tunnel(&self) {
        self.check_local("pc");
        let w = self.w();
        let nodes = w.lists.get("pc").cloned().unwrap_or_default();
        let dead: Vec<String> = nodes.iter().filter(|n| !has(n, "pc_ms")).map(|n| f(n, "tag")).collect();
        let api = w.pcs.as_ref().is_some_and(|p| p["api"] == true);
        let pending = w.pc_pending;
        drop(w);
        let msg = if !api {
            tr("The PC tunnel is not running. Press Restart PC tunnel, then test again.", &[])
        } else if pending {
            tr("The PC tunnel still runs the previous nodes. Press Restart PC tunnel, then test again.", &[])
        } else {
            let mut m = tr(
                "PC tunnel: {ok} of {n} nodes work.",
                &[("ok", (nodes.len() - dead.len()).to_string()), ("n", nodes.len().to_string())],
            );
            if !dead.is_empty() {
                m += &format!(" {}", tr("Not working: {tags}.", &[("tags", dead.join(", "))]));
            }
            m
        };
        self.set_msg(msg);
    }

    // Write the pairs, deploy, check the PC tunnel; the slots whose new node failed there (deploy)
    fn cand_deploy(&self, pairs: Vec<(usize, Value)>) -> Result<Vec<usize>, String> {
        let mut ssh = RouterSsh { paths: self.paths };
        let swaps = nm::swap_files(self.paths, pairs.clone())?;
        let used: HashSet<String> = pairs.iter().map(|(_, n)| f(n, "server")).collect();
        self.w().cand.nodes.retain(|n| !used.contains(&f(n, "server")));
        nm::deploy_files(self.paths, &mut ssh);
        let mode = Mode::of(self.paths);
        if !mode.ronly {
            engine::clash_api::wait_api(self.paths, Duration::from_secs(20)); // the check must not run while it starts
        }
        let s: Vec<String> = swaps
            .iter()
            .map(|(tag, old, new)| {
                let how = if is(new, true) {
                    tr("({ms} ms, tested).", &[("ms", f(new, "ms"))])
                } else {
                    tr("(not tunnel-tested).", &[])
                };
                format!("{tag}: {old} → {} {how}", f(new, "server"))
            })
            .collect();
        self.after_deploy(&s.join(" "));
        if mode.ronly {
            return Ok(Vec::new());
        }
        let w = self.w();
        let pc = w.lists.get("pc").cloned().unwrap_or_default();
        let by = |s: &str| pc.iter().find(|n| f(n, "server") == s).cloned().unwrap_or(json!({}));
        Ok(pairs
            .iter()
            .filter(|(_, n)| {
                let m = by(&f(n, "server"));
                !has(&m, "pc_ms") && has(&m, "pc_why") && f(&m, "pc_why") != "api down"
            })
            .map(|(i, _)| *i)
            .collect())
    }

    // The nodes are in: the dialog closes either way; a slot that failed shows in the main window (finish)
    fn cand_finish(&self, dead: Vec<usize>) {
        let mut w = self.w();
        if !dead.is_empty() {
            let slots: Vec<String> = dead.iter().map(|i| format!("vless-{}", i + 1)).collect();
            w.status_text = tr(
                "{slots} did not work through the PC tunnel after the restart. Open Candidates and put another node there.",
                &[("slots", slots.join(", "))],
            );
            w.status_state = "warn".into();
        }
        w.cand.open = false;
    }

    // Picked candidates into their slots: (slot, server) (put_all, put_one)
    pub fn cand_put(&self, picks: &[(usize, String)]) -> Result<(), String> {
        let pairs: Vec<(usize, Value)> = {
            let w = self.w();
            picks
                .iter()
                .filter_map(|(i, s)| w.cand.nodes.iter().find(|n| f(n, "server") == *s).map(|n| (*i, n.clone())))
                .collect()
        };
        if pairs.is_empty() {
            return Err("the picked candidates are gone; ping again".into());
        }
        let dead = self.cand_deploy(pairs)?;
        self.cand_finish(dead);
        Ok(())
    }

    // Only tunnel-tested working nodes go in, the fast ones first; dead slots then the slowest are replaced (do_best)
    pub fn cand_best(&self) -> Result<(), String> {
        let k = self.slots();
        let tested_ok = |wk: &Self| nm::best_first(wk.paths, &wk.w().cand.nodes);
        if tested_ok(self).len() < k {
            self.cand_test()?;
        }
        let mut ok = tested_ok(self);
        if ok.is_empty() {
            self.set_msg(tr("No candidate passed the test; nothing changed. Ping again later.", &[]));
            return Ok(());
        }
        let s = Settings::read(self.paths).unwrap_or_default();
        let floor = engine::node_tests::floor_now(self.paths, s.min_mbps, now()) as f64;
        let fast: Vec<Value> =
            ok.iter().filter(|n| n.get("mbps").and_then(Value::as_f64).unwrap_or(0.0) >= floor).cloned().collect();
        if !fast.is_empty() {
            ok = fast; // fewer fast ones replace fewer slots: the slowest
        }
        let cur = self.w().lists.get(Mode::of(self.paths).main()).cloned().unwrap_or_default();
        let speed = |i: usize| {
            let n = cur.get(i);
            let ms = n.and_then(|n| {
                ["pc_ms", "phone_ms"].iter().find_map(|k| n.get(*k).filter(|v| engine::settings::py_truthy(v)))
            });
            let v = ms.and_then(Value::as_f64);
            (v.is_some(), -v.unwrap_or(0.0))
        };
        let mut order: Vec<usize> = (0..k).collect();
        order.sort_by(|a, b| {
            let (x, y) = (speed(*a), speed(*b));
            x.0.cmp(&y.0).then(x.1.total_cmp(&y.1))
        });
        let mut slots: Vec<usize> = order.into_iter().take(ok.len()).collect();
        slots.sort();
        let mut dead = self.cand_deploy(slots.into_iter().zip(ok).collect())?;
        if !dead.is_empty() {
            let spares = tested_ok(self);
            if !spares.is_empty() {
                dead = self.cand_deploy(dead.into_iter().zip(spares).collect())?;
            }
        }
        self.cand_finish(dead);
        Ok(())
    }
}

impl Worker<'_> {
    // Push the device list to the router (DevicesDialog.push)
    pub fn devices_push(&self) -> Result<(), String> {
        let out = nm::push(self.paths, &mut RouterSsh { paths: self.paths })?;
        say(&out);
        let ok = out.contains("sing-box service: running");
        let d = Settings::read(self.paths).unwrap_or_default().devices;
        let (text, state) = if !ok {
            (tr("Router NOT updated; see the activity log.", &[]), "bad")
        } else if engine::router::all_devices(&d) {
            (tr("Router updated: every device on the main network goes through the tunnel.", &[]), "ok")
        } else {
            let who = engine::router::devices(&d).join(", ");
            let who = if who.is_empty() { tr("no devices", &[]) } else { who };
            (tr("Router updated: {who} go through the tunnel.", &[("who", who)]), "ok")
        };
        let mut w = self.w();
        w.status_text = text;
        w.status_state = state.into();
        Ok(())
    }
}

// The dialog's rows and message, in the person's language (render)
pub fn view(paths: &engine::paths::Paths, w: &crate::window_state::Win) -> Value {
    let mode = Mode::of(paths);
    let rows: Vec<Value> = w
        .cand
        .nodes
        .iter()
        .map(|n| {
            let (state, res) = if is(n, true) {
                ("ok", tr("works, {ms} ms in the test, exit {ip}", &[("ms", f(n, "ms")), ("ip", f(n, "exit"))]))
            } else if is(n, false) {
                let why = if has(n, "why") { f(n, "why") } else { "?".into() };
                (
                    "bad",
                    if n.get("known") == Some(&json!(true)) {
                        why
                    } else {
                        tr("failed the test: {why}", &[("why", why)])
                    },
                )
            } else {
                ("warn", tr("answers on 443; not tunnel-tested", &[]))
            };
            json!({
                "server": f(n, "server"), "front": f(n, "sni"),
                "where": if has(n, "where") { f(n, "where") } else { "?".into() },
                "tcp": ms(n.get("tcp_ms"), &tr("no answer", &[])), "tunnel": ms(n.get("ms"), ""),
                "result": res, "state": state, "ok": is(n, true),
            })
        })
        .collect();
    let main = w.lists.get(mode.main()).map_or(0, Vec::len);
    json!({
        "open": w.cand.open, "msg": w.cand.msg, "rows": rows, "busy": w.busy,
        "slots": if main == 0 { 6 } else { main },
        "ronly": mode.ronly, "router": mode.router, "pc_only": engine::settings::pc_only(paths),
    })
}
