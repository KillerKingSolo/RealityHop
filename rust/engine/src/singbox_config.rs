//! The sing-box configs (PC and router): building and changing them. Each function works on the parsed JSON and
//! keeps key order, so the files come out byte for byte as engine.py writes them.

use crate::node::Node;
use crate::python_compat;
use crate::settings::DNS_IPS;
use md5::{Digest, Md5};
use serde_json::{Value, json};

/// Web pages allowed to call the PC tunnel's control API: none (only itself).
pub const API_ORIGIN: &str = "http://127.0.0.1:9090";

// Tag of an unused direct outbound that names the node set; the Clash API lists it, so the running set is known
pub fn set_mark<'a>(servers: impl IntoIterator<Item = &'a str>) -> String {
    let joined = servers.into_iter().collect::<Vec<_>>().join(" ");
    let hex: String = Md5::digest(joined.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    format!("set-{}", &hex[..10])
}

// The vless outbound for a node; works on the router's sing-box 1.9 and on the PC's current sing-box
pub fn outbound19(n: &Node, tag: &str) -> Value {
    json!({"type": "vless", "tag": tag, "server": n.server, "server_port": 443, "uuid": n.uuid,
        "flow": "xtls-rprx-vision", "packet_encoding": "xudp",
        "tls": {"enabled": true, "server_name": n.sni, "utls": {"enabled": true, "fingerprint": n.fp},
            "reality": {"enabled": true, "public_key": n.pbk, "short_id": n.sid}}})
}

// A vless outbound back to a node with its tag (engine.from_outbound); None when a field is missing
pub fn from_outbound(o: &Value) -> Option<(String, Node)> {
    let t = &o["tls"];
    let s = |v: &Value| v.as_str().map(str::to_string);
    let node = Node {
        server: s(&o["server"])?,
        uuid: s(&o["uuid"])?,
        pbk: s(&t["reality"]["public_key"])?,
        sid: s(&t["reality"]["short_id"])?,
        sni: s(&t["server_name"])?,
        fp: s(&t["utls"]["fingerprint"])?,
    };
    Some((s(&o["tag"])?, node))
}

// Python truthiness of a JSON value
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

// router-config.json (the sing-box 1.9 schema GL.iNet's package runs) in the 1.12+ schema: DNS server types,
// sniffing and DNS hijack as route actions. Push picks the one the router's sing-box reads.
pub fn router_cfg_new(c: &Value) -> Value {
    let mut c = c.clone();
    let servers: Vec<Value> = c["dns"]["servers"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|s| match s.get("address").and_then(Value::as_str) {
            Some(a) if a.contains("://") => {
                let host = python_compat::urlsplit(a).ok().and_then(|u| u.hostname());
                json!({"type": "https", "tag": s["tag"], "server": host, "detour": s["detour"]})
            }
            _ => s,
        })
        .collect();
    c["dns"]["servers"] = Value::Array(servers);
    let mut rules = Vec::new();
    for i in c["inbounds"].as_array_mut().into_iter().flatten() {
        let Some(m) = i.as_object_mut() else { continue };
        let tag = m.get("tag").cloned().unwrap_or(Value::Null);
        if m.shift_remove("sniff").is_some_and(|v| truthy(&v)) {
            rules.push(json!({"inbound": [tag.clone()], "action": "sniff"}));
        }
        if m.shift_remove("override_address").is_some_and(|v| truthy(&v)) {
            m.shift_remove("override_port");
            rules.push(json!({"inbound": [tag], "action": "hijack-dns"}));
        }
    }
    let outs = c["outbounds"].as_array().cloned().unwrap_or_default();
    let dns_out: Vec<Value> = outs.iter().filter(|o| o["type"] == "dns").map(|o| o["tag"].clone()).collect();
    c["outbounds"] = Value::Array(outs.into_iter().filter(|o| o["type"] != "dns").collect());
    let old_rules = c["route"]["rules"].as_array().cloned().unwrap_or_default();
    rules.extend(old_rules.into_iter().filter(|r| !r.get("outbound").is_some_and(|o| dns_out.contains(o))));
    c["route"]["rules"] = Value::Array(rules);
    let final_ = c["dns"]["final"].clone();
    if let Some(route) = c["route"].as_object_mut() {
        route.entry("default_domain_resolver").or_insert(final_);
    }
    c
}

// The vless outbounds of a node set, tagged vless-1..N
fn vless_set(best: &[Node]) -> Vec<Value> {
    best.iter().enumerate().map(|(i, n)| outbound19(n, &format!("vless-{}", i + 1))).collect()
}

// The other outbounds, with every urltest group pointed at group (the fast nodes of the set) and the selector at
// the whole set
fn rest_with_groups(outs: &[Value], tags: &[Value], group: &[Value], keep: impl Fn(&Value) -> bool) -> Vec<Value> {
    let mut rest: Vec<Value> = outs.iter().filter(|o| o["type"] != "vless" && keep(o)).cloned().collect();
    for o in &mut rest {
        if o["type"] == "urltest" {
            o["outbounds"] = Value::Array(group.to_vec());
        } else if o["type"] == "selector" && o["tag"] == PICK {
            o["outbounds"] = Value::Array([Value::from("auto")].into_iter().chain(tags.iter().cloned()).collect());
        }
    }
    rest
}

/// The selector the PC tunnel routes through: on "auto" (the lowest-ping working node) except for a few minutes after
/// a PIA drop, when it can hold another node so PIA does not go back to the one that broke its connection.
pub const PICK: &str = "pick";

// Put the selector "pick" in front of the group "auto" and route through it (PC configs from before 2.1.1). True when
// the config changed; it takes effect at the tunnel's next start
pub fn pick_add(c: &mut Value) -> bool {
    let Some(outs) = c["outbounds"].as_array_mut() else { return false };
    if outs.iter().any(|o| o["tag"] == PICK) || !outs.iter().any(|o| o["tag"] == "auto" && o["type"] == "urltest") {
        return false;
    }
    let tags: Vec<Value> = outs.iter().filter(|o| o["type"] == "vless").map(|o| o["tag"].clone()).collect();
    let at = outs.iter().position(|o| o["tag"] == "auto").map_or(outs.len(), |i| i + 1);
    let all: Vec<Value> = [Value::from("auto")].into_iter().chain(tags).collect();
    outs.insert(at, json!({"type": "selector", "tag": PICK, "outbounds": all, "default": "auto"}));
    if c["route"]["final"] == "auto" {
        c["route"]["final"] = Value::from(PICK);
    }
    true
}

/// How often the PC tunnel's group re-tests its nodes: a node that stops passing traffic is dropped within a minute.
pub const RETEST: &str = "1m";
/// The PC tunnel's log level: "info" names the node each connection goes through, so a PIA drop can be traced to it.
pub const PC_LOG: &str = "info";

// The re-test interval and log level of PC configs from before 2.1.2; values changed by hand stay. mac: turn the
// interface watch back on where a 2.1.2 test build turned it off. Off did not help: sing-box watches the network
// either way. True when it changed
pub fn pc_tune(c: &mut Value, mac: bool) -> bool {
    let mut changed = false;
    for o in c["outbounds"].as_array_mut().into_iter().flatten() {
        if o["type"] == "urltest" && o["tag"] == "auto" && o["interval"] == "10m" {
            o["interval"] = RETEST.into();
            changed = true;
        }
    }
    if c["log"]["level"] == "warn" {
        c["log"]["level"] = PC_LOG.into();
        changed = true;
    }
    if mac && c["route"]["auto_detect_interface"] == false {
        c["route"]["auto_detect_interface"] = true.into();
        changed = true;
    }
    changed
}

// pick_add and pc_tune on pc-config.json; nothing when it is missing or already up to date. From the next tunnel start
pub fn pick_migrate(paths: &crate::paths::Paths) {
    let Ok(mut c) = crate::data_files::read_json(&paths.pc_cfg) else { return };
    let added = pick_add(&mut c);
    if pc_tune(&mut c, cfg!(target_os = "macos")) | added {
        let _ = crate::data_files::jsave(&paths.pc_cfg, &c, Some(2));
    }
}

// Put a node set into both configs (engine.write_configs): the vless outbounds first, then the rest, and on the PC
// the set-<md5> marker; everything else in them stays as is. fast: per node, whether it reaches the speed floor. The
// group "auto" picks by ping among the fast nodes only, so a slow node with a low ping is not used while a fast one
// works. The PC tunnel can still reach a slow node through its selector, as a backup (watcher settle); the router
// has no backup, so it keeps every node in the group unless at least 2 are fast
pub fn write_configs(pc: &Value, router: &Value, best: &[Node], fast: &[bool]) -> (Value, Value) {
    let vl = vless_set(best);
    let tags: Vec<Value> = vl.iter().map(|v| v["tag"].clone()).collect();
    let quick: Vec<Value> = tags
        .iter()
        .zip(fast.iter().chain(std::iter::repeat(&true)))
        .filter(|(_, f)| **f)
        .map(|(t, _)| t.clone())
        .collect();
    let pc_group = if quick.is_empty() { &tags } else { &quick };
    let r_group = if quick.len() < 2 { &tags } else { &quick };
    let mut r = router.clone();
    let outs = r["outbounds"].as_array().cloned().unwrap_or_default();
    r["outbounds"] =
        Value::Array(vl.iter().cloned().chain(rest_with_groups(&outs, &tags, r_group, |_| true)).collect());
    let mut p = pc.clone();
    let outs = p["outbounds"].as_array().cloned().unwrap_or_default();
    let not_mark = |o: &Value| !o["tag"].as_str().is_some_and(|t| t.starts_with("set-"));
    let mark = json!({"type": "direct", "tag": set_mark(best.iter().map(|n| n.server.as_str()))});
    p["outbounds"] =
        Value::Array(vl.into_iter().chain(rest_with_groups(&outs, &tags, pc_group, not_mark)).chain([mark]).collect());
    (p, r)
}

// The ip of a known name server
fn dns_ip(name: &str) -> Option<&'static str> {
    DNS_IPS.iter().find(|(k, _)| *k == name).map(|(_, ip)| *ip)
}

// Switch one config to the name server want (quad9 or cloudflare); a server set by hand stays. True when it changed.
pub fn dns_apply(c: &mut Value, want: &str) -> bool {
    let Some(ip) = dns_ip(want) else { return false };
    let Some(old) = c["dns"]["final"].as_str().map(str::to_string) else { return false };
    if dns_ip(&old).is_none() {
        return false;
    }
    let Some(srv) = c["dns"]["servers"].as_array_mut().and_then(|s| s.iter_mut().find(|x| x["tag"] == old.as_str()))
    else {
        return false;
    };
    let srv = srv.as_object_mut().unwrap();
    // the address in use. Cloudflare was 1.1.1.1 before 2.1.2; now 1.1.1.2, which also blocks malware
    let old_ip = srv
        .get("server")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            srv.get("address").and_then(Value::as_str).and_then(|a| python_compat::urlsplit(a).ok()?.hostname())
        })
        .unwrap_or_default();
    if old == want && !(want == "cloudflare" && old_ip == "1.1.1.1") {
        return false;
    }
    srv.insert("tag".into(), want.into());
    if srv.contains_key("server") {
        srv.insert("server".into(), ip.into());
    }
    if srv.contains_key("address") {
        srv.insert("address".into(), format!("https://{ip}/dns-query").into());
    }
    c["dns"]["final"] = want.into();
    if c["route"]["default_domain_resolver"] == old.as_str() {
        c["route"]["default_domain_resolver"] = want.into();
    }
    for i in c["inbounds"].as_array_mut().into_iter().flatten() {
        if i["override_address"] == old_ip.as_str() {
            i["override_address"] = ip.into();
        }
    }
    true
}

// The two pc-config.json fixes engine.api_lockdown makes (it then checks the result with rh-core before saving):
// close the control API to web pages, and use sing-box's own Mozilla root list when rh-core is 1.12.0 or newer
pub fn api_lockdown(c: &mut Value, store_ok: bool) -> Vec<&'static str> {
    let mut fix = Vec::new();
    let Some(api) = c.pointer_mut("/experimental/clash_api").and_then(Value::as_object_mut) else { return fix };
    if api.get("access_control_allow_origin") != Some(&json!([API_ORIGIN])) {
        api.insert("access_control_allow_origin".into(), json!([API_ORIGIN]));
        api.insert("access_control_allow_private_network".into(), false.into());
        fix.push("control API: closed to web pages");
    }
    let store = json!({"store": "mozilla"});
    if store_ok && c.get("certificate") != Some(&store) {
        if let Some(m) = c.as_object_mut() {
            m.insert("certificate".into(), store);
        }
        fix.push("rh-core: checks certificates against its own Mozilla root list");
    }
    fix
}

// The vless nodes in one config file, either schema, with their tags (engine.nodes_of)
pub fn nodes_of(cfg: &std::path::Path) -> std::io::Result<Vec<(String, Node)>> {
    let c = crate::data_files::read_json(cfg)?;
    Ok(c["outbounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["type"] == "vless")
        .filter_map(from_outbound)
        .collect())
}
