//! The router side (GL.iNet, OpenWrt): the shell scripts sent over SSH and the reading of their answers. The SSH
//! transport is passed in, so the parity tests replay recorded router answers.

use crate::node::Node;
use crate::python_compat::{splitlines, strip};
use crate::python_json::dumps;
use crate::singbox_config::{outbound19, router_cfg_new};
use crate::{clash_api, validate};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

/// The Clash API address on the router and the PC.
pub const API: &str = "http://127.0.0.1:9090";
/// The URL the delay tests fetch.
pub const TEST_URL: &str = "https://www.gstatic.com/generate_204";
/// On the router: "<channel> <version> <sha256>" of the Reality Hop build it runs.
pub const RC_MARK: &str = "/etc/sing-box/rh-core";
/// The default time limit of one SSH command, in seconds.
pub const SSH_TIMEOUT: u64 = 240;

/// Why a router step failed, as the Python exception it mirrors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// RuntimeError: the router or the step failed, with a message for the person.
    Failed(String),
    /// ValueError: the router's answer could not be read.
    BadAnswer(String),
}

impl std::fmt::Display for Error {
    // The message, as str() of the Python exception gives it
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Failed(s) | Error::BadAnswer(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

/// The router's self-repair script (resources/router/heal.sh), installed with every push.
pub const HEAL_SH: &str = include_str!("../../resources/router/heal.sh");
/// The router's firewall include that diverts the chosen devices (resources/router/nft-devices.sh).
pub const NFT_SH: &str = include_str!("../../resources/router/nft-devices.sh");

/// Runs one command on the router over SSH, with optional script text on stdin.
pub trait Ssh {
    fn run(&mut self, cmd: &str, stdin: Option<&str>, timeout: u64) -> Result<String, Error>;

    // One command with raw bytes on stdin: a file upload. A transport that cannot send files says so
    fn upload(&mut self, cmd: &str, data: &[u8], timeout: u64) -> Result<String, Error> {
        let _ = (cmd, data, timeout);
        Err(Error::Failed("this connection cannot send files".into()))
    }
}

// str.split(): words between runs of whitespace
fn words(s: &str) -> Vec<&str> {
    s.split(crate::python_compat::is_space).filter(|w| !w.is_empty()).collect()
}

// A node object's fields as a Node (missing fields as "")
pub fn node_of(n: &Value) -> Node {
    let f = |k: &str| n.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    Node { server: f("server"), uuid: f("uuid"), pbk: f("pbk"), sid: f("sid"), sni: f("sni"), fp: f("fp") }
}

// dict.update: replace a key in place, or add it at the end
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

static ERRS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)reality verification failed|certificate is valid for [a-z0-9.*-]+|connection refused|i/o timeout|connection reset|EOF")
        .unwrap()
});
static DEAD_NODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"outbound\[(\d+)\]|n-(\d+)").unwrap());

// Why test node i failed, from the test instance's log: the first known error on a line about its port or its
// outbound, else "no-response"
pub fn why_failed(log: &str, i: &str) -> String {
    let (a, b) = (format!("[in-{i}]"), format!("[n-{i}]"));
    splitlines(log)
        .into_iter()
        .filter(|l| l.contains(&a) || l.contains(&b))
        .find_map(|l| ERRS.find(l).map(|m| m.as_str().to_lowercase()))
        .unwrap_or_else(|| "no-response".into())
}

// The script of one handshake round: one sing-box with a port per node, then 5 fetches at a time
fn handshake_script(nodes: &[Value], todo: &[usize]) -> String {
    let inbounds: Vec<Value> = todo
        .iter()
        .map(|i| json!({"type": "mixed", "tag": format!("in-{i}"), "listen": "127.0.0.1", "listen_port": 21000 + i}))
        .collect();
    let outbounds: Vec<Value> = todo
        .iter()
        .map(|&i| outbound19(&node_of(&nodes[i]), &format!("n-{i}")))
        .chain([json!({"type": "direct", "tag": "direct"})])
        .collect();
    let rules: Vec<Value> = [json!({"ip_is_private": true, "outbound": "direct"})]
        .into_iter()
        .chain(todo.iter().map(|i| json!({"inbound": [format!("in-{i}")], "outbound": format!("n-{i}")})))
        .collect();
    let cfg = json!({"log": {"level": "error"}, "inbounds": inbounds, "outbounds": outbounds,
        "route": {"rules": rules, "final": "direct", "auto_detect_interface": true}});
    let mut fetch = String::new();
    for (k, i) in todo.iter().enumerate() {
        fetch += &format!(
            "( T=$(curl -s --socks5-hostname 127.0.0.1:{p} --max-time 10 -o out-{i} -w '%{{time_total}}' https://api.ipify.org 2>/dev/null); IP=$(cat out-{i} 2>/dev/null); if [ -n \"$IP\" ]; then echo \"nt-{i} PASS $IP $T\"; else echo \"nt-{i} FAIL\"; fi ) &\nW=\"$W $!\"\n",
            p = 21000 + i
        );
        if k % 5 == 4 {
            fetch += "wait $W\nW=\n";
        }
    }
    format!(
        "rm -rf /tmp/nt; mkdir -p /tmp/nt; cd /tmp/nt\ncat > all.json <<'SBEOF'\n{}\nSBEOF\nsing-box run -c all.json > log 2>&1 &\nP=$!\nsleep 2\nif ! kill -0 $P 2>/dev/null; then echo DEAD; sed \"s/\\x1b\\[[0-9;]*m//g\" log | head -5; exit; fi\n{fetch}[ -n \"$W\" ] && wait $W\nkill $P 2>/dev/null\necho ---LOG---\nsed \"s/\\x1b\\[[0-9;]*m//g\" log | tail -300\nrm -rf /tmp/nt\n",
        dumps(&cfg, None)
    )
}

// Real REALITY handshake + HTTPS fetch through each node, all at once on the router, each timed end to end. A node
// that stops the test instance from starting is dropped and the rest retried, up to 4 rounds (engine.handshake).
pub fn handshake(ssh: &mut impl Ssh, nodes: &mut [Value]) -> Result<(), Error> {
    for n in nodes.iter_mut() {
        if let Some(m) = n.as_object_mut() {
            m.shift_remove("ok");
        }
    }
    let mut todo: Vec<usize> = (0..nodes.len()).collect();
    for _ in 0..4 {
        if todo.is_empty() {
            break;
        }
        let out = ssh.run("sh -s", Some(&handshake_script(nodes, &todo)), SSH_TIMEOUT)?;
        if out.starts_with("DEAD") {
            let Some(bad) = DEAD_NODE.captures(&out) else {
                let rest: String = words(&out)[1..].join(" ").chars().take(200).collect();
                return Err(Error::Failed(format!("router test instance did not start: {rest}")));
            };
            let k = match bad.get(1) {
                Some(g) => g.as_str().parse::<usize>().map_err(|e| Error::BadAnswer(e.to_string()))?,
                None => {
                    let i: usize = bad[2].parse().map_err(|_| Error::BadAnswer("bad node number".into()))?;
                    todo.iter().position(|&x| x == i).ok_or_else(|| Error::BadAnswer(format!("{i} is not in list")))?
                }
            };
            if k >= todo.len() {
                return Err(Error::BadAnswer("pop index out of range".into()));
            }
            let n = &mut nodes[todo.remove(k)];
            for (key, v) in [
                ("ok", json!(false)),
                ("exit", json!("-")),
                ("ms", Value::Null),
                ("why", json!("rejected by sing-box: bad node parameters")),
            ] {
                set(n, key, v);
            }
            continue;
        }
        let (res, log) = out.split_once("---LOG---").unwrap_or((&out, ""));
        for line in splitlines(strip(res)) {
            let p = words(line);
            if p.len() < 2 || !p[0].starts_with("nt-") {
                continue;
            }
            // A line the router garbled is skipped; that node ends as "no result". (1.7.x stopped the whole test.)
            let i = &p[0][3..];
            let Some(idx) = i.parse::<usize>().ok().filter(|&k| k < nodes.len()) else { continue };
            let ok = p[1] == "PASS";
            // curl prints the time with a decimal comma in some router locales (1,5), as router_ping allows
            let secs = p.get(3).and_then(|t| t.replace(',', ".").parse::<f64>().ok()).filter(|t| t.is_finite());
            let (exit, ms) = match secs {
                Some(t) if ok => (json!(p[2]), json!((t * 1000.0) as i64)),
                _ => (json!("-"), Value::Null),
            };
            let n = &mut nodes[idx];
            set(n, "ok", json!(ok));
            set(n, "exit", exit);
            set(n, "ms", ms);
            if !ok {
                set(n, "why", json!(why_failed(log, i)));
            }
        }
        break;
    }
    for n in nodes.iter_mut() {
        set_default(n, "ok", json!(false));
        set_default(n, "exit", json!("-"));
        set_default(n, "ms", Value::Null);
        set_default(n, "why", json!("no result"));
    }
    Ok(())
}

// 443 connect time measured on the router, 32 at a time; sets tcp_ms on every node and returns the indexes of the
// ones that answered, fastest first (engine.router_ping)
pub fn router_ping(ssh: &mut impl Ssh, nodes: &mut [Value]) -> Result<Vec<usize>, Error> {
    let mut body = String::new();
    for (k, n) in nodes.iter().enumerate() {
        body += &format!(
            "( echo \"{k} $(curl -sk -o /dev/null --connect-timeout 2 -m 3 -w '%{{time_connect}}' https://{}/)\" ) &\nW=\"$W $!\"\n",
            node_of(n).server
        );
        if k % 32 == 31 {
            body += "wait $W\nW=\n";
        }
    }
    let out = ssh.run("sh -s", Some(&(body + "[ -n \"$W\" ] && wait $W\n")), SSH_TIMEOUT)?;
    let mut res: Vec<(usize, Option<i64>)> = Vec::new();
    for l in splitlines(&out) {
        let p = words(l);
        if p.len() == 2 && p[0].bytes().all(|b| b.is_ascii_digit()) {
            let Ok(t) = p[1].replace(',', ".").parse::<f64>() else { continue };
            let k: usize = p[0].parse().unwrap_or(usize::MAX);
            res.retain(|(x, _)| *x != k);
            res.push((k, if t > 0.0 { Some((t * 1000.0) as i64) } else { None }));
        }
    }
    for (k, n) in nodes.iter_mut().enumerate() {
        let ms = res.iter().find(|(x, _)| *x == k).and_then(|(_, m)| *m);
        set(n, "tcp_ms", ms.map_or(Value::Null, Value::from));
    }
    let mut live: Vec<usize> = (0..nodes.len()).filter(|&k| !nodes[k]["tcp_ms"].is_null()).collect();
    live.sort_by_key(|&k| nodes[k]["tcp_ms"].as_i64());
    Ok(live)
}

/// The router side of a live check (engine.check_phone).
#[derive(Debug, Clone, PartialEq)]
pub struct PhoneCheck {
    pub service: bool,
    pub now: Option<Value>,
    pub cfg: String,
}

// Fresh test of every in-use node through the router's live tunnel; sets phone_ms / phone_why on each node
pub fn check_phone(ssh: &mut impl Ssh, nodes: &mut [Value], secret: &str) -> Result<PhoneCheck, Error> {
    let tags: Vec<String> = nodes.iter().map(|n| n["tag"].as_str().unwrap_or_default().to_string()).collect();
    let delay = format!("curl -s -m 12 -H \"$H\" \"{API}/proxies/$t/delay?url={TEST_URL}&timeout=8000\"");
    let script = format!(
        "H=\"Authorization: Bearer {secret}\"\necho \"svc $(pgrep -f \"/etc/sing-box/config[.]json\" | head -1)\"\nfor t in {}; do ( r=$({delay}); case \"$r\" in *delay*) ;; *) r=$({delay});; esac; echo \"$t $r\" ) & done; wait\necho \"now $(curl -s -m 5 -H \"$H\" {API}/proxies/auto)\"\necho \"cfg $(md5sum /etc/sing-box/config.json 2>/dev/null | cut -c1-32)\"\n",
        tags.join(" ")
    );
    let out = ssh.run("sh -s", Some(&script), SSH_TIMEOUT)?;
    let mut res: Map<String, Value> = Map::new();
    for l in splitlines(&out).into_iter().filter(|l| !strip(l).is_empty()) {
        let (k, v) = l.split_once(' ').unwrap_or((l, ""));
        res.insert(k.into(), v.into());
    }
    let get = |k: &str| res.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    for (n, tag) in nodes.iter_mut().zip(&tags) {
        let (ms, why) = clash_api::parse_delay(&get(tag)).map_err(|_| Error::BadAnswer("not a JSON object".into()))?;
        set(n, "phone_ms", ms.unwrap_or(Value::Null));
        set(n, "phone_why", why);
    }
    let now = match serde_json::from_str::<Value>(&get("now")) {
        Ok(Value::Object(m)) => m.get("now").cloned(),
        Ok(_) => return Err(Error::BadAnswer("not a JSON object".into())),
        Err(_) => None,
    };
    Ok(PhoneCheck { service: !strip(&get("svc")).is_empty(), now, cfg: strip(&get("cfg")).to_string() })
}

// The node the router tunnel uses now (its tag), or None (engine.router_now)
// A pin (the selector "pick" on a node) comes first; the group "auto" decides otherwise
pub fn router_now(ssh: &mut impl Ssh, secret: &str) -> Option<Value> {
    let h = format!("-H \"Authorization: Bearer {secret}\"");
    let script = format!("curl -s -m 5 {h} {API}/proxies/pick; echo; curl -s -m 5 {h} {API}/proxies/auto\n");
    let out = ssh.run("sh -s", Some(&script), 20).ok()?;
    let answers: Vec<Value> = out.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect();
    let of = |t: &str| answers.iter().find(|v| v["type"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(t)));
    if let Some(n) = of("Selector").map(|v| &v["now"]).filter(|n| n.as_str().is_some_and(|t| t != "auto")) {
        return Some(n.clone());
    }
    let selector = |v: &Value| v["type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("Selector"));
    match answers.iter().find(|v| v.get("now").is_some() && !selector(v))? {
        // a router with no selector answers "not found" first
        Value::Object(m) => Some(m.get("now").cloned().unwrap_or(Value::Null)),
        _ => None,
    }
}

// Point the router tunnel's selector at a node tag, or at "auto" to end a pin. False when the router config has no
// selector yet (push first) or the API refuses
pub fn router_pick(ssh: &mut impl Ssh, secret: &str, tag: &str) -> bool {
    let body = json!({ "name": tag }).to_string();
    let script = format!(
        "curl -s -o /dev/null -w '%{{http_code}}' -m 5 -X PUT -H \"Authorization: Bearer {secret}\" -d '{body}' {API}/proxies/pick\n"
    );
    ssh.run("sh -s", Some(&script), 20).is_ok_and(|o| o.trim() == "204")
}

// The node lists downloaded and pre-filtered by the router, for when this PC cannot reach GitHub (engine.router_feed)
pub fn router_feed(ssh: &mut impl Ssh) -> Result<String, Error> {
    let cmd: Vec<String> = crate::node_feed::FEEDS
        .iter()
        .map(|u| format!("curl -s -m 40 '{u}' | grep '^vless://' | grep reality | grep vision"))
        .collect();
    let out = ssh.run(&cmd.join(" ; "), None, SSH_TIMEOUT)?;
    if !out.contains("vless://") {
        return Err(Error::Failed("the router could not download the node lists either".into()));
    }
    Ok(out)
}

/// Model, firmware, sing-box version and helper tools on the router.
#[derive(Debug, Clone, PartialEq)]
pub struct RouterInfo {
    pub model: String,
    pub firmware: String,
    pub singbox: Option<String>,
    /// A sing-box newer than 1.14 that is not Reality Hop's own build.
    pub too_new: bool,
    pub tools: Vec<String>,
}

static VER3: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

// What the router is and runs (engine.router_info)
pub fn router_info(ssh: &mut impl Ssh) -> Result<RouterInfo, Error> {
    let out = ssh.run(
        &format!("cat /tmp/sysinfo/model 2>/dev/null; echo \"|\"; cat /etc/glversion 2>/dev/null; echo \"|\"; sing-box version 2>/dev/null | head -1; echo \"|\"; for t in jq curl; do command -v $t >/dev/null && echo $t; done; echo \"|\"; cat {RC_MARK} 2>/dev/null"),
        None,
        30,
    )?;
    let mut p: Vec<&str> = out.split('|').collect();
    p.resize(p.len().max(5), "");
    let or_q = |s: &str| if strip(s).is_empty() { "?".to_string() } else { strip(s).to_string() };
    let m = VER3.captures(p[2]);
    let num = |i: usize| m.as_ref().and_then(|m| m[i].parse::<u64>().ok()).unwrap_or(0);
    let mut tools: Vec<String> = words(p[3]).into_iter().map(str::to_string).collect();
    tools.sort();
    tools.dedup();
    Ok(RouterInfo {
        model: or_q(p[0]),
        firmware: or_q(p[1]),
        singbox: m.as_ref().map(|m| m[0].to_string()),
        too_new: m.is_some() && strip(p[4]).is_empty() && (num(1), num(2)) >= (1, 15),
        tools,
    })
}

/// What the router runs: its sing-box version, the mark of a Reality Hop build, and its build architecture.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreState {
    pub ver: Option<String>,
    pub mark: Option<Vec<String>>,
    pub arch: Option<String>,
}

static VERSION_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"version (\S+)").unwrap());

// The router's sing-box (engine.router_core_state; the caller keeps it in this-pc.json)
pub fn router_core_state(ssh: &mut impl Ssh) -> Result<CoreState, Error> {
    let out = ssh.run(
        &format!("sing-box version 2>/dev/null | head -1; echo \"|\"; cat {RC_MARK} 2>/dev/null; echo \"|\"; uname -m"),
        None,
        30,
    )?;
    let mut p: Vec<&str> = out.split('|').collect();
    p.resize(p.len().max(3), "");
    let mk: Vec<String> = words(p[1]).into_iter().map(str::to_string).collect();
    Ok(CoreState {
        ver: VERSION_WORD.captures(p[0]).map(|m| m[1].to_string()),
        mark: (mk.len() == 3).then_some(mk),
        arch: (strip(p[2]) == "aarch64").then(|| "arm64".to_string()),
    })
}

// (ip, name) of each DHCP lease on the router, by address (engine.router_clients)
pub fn router_clients(ssh: &mut impl Ssh) -> Result<Vec<(String, String)>, Error> {
    let out = ssh.run("cat /tmp/dhcp.leases 2>/dev/null", None, 30)?;
    let mut list: Vec<(String, String)> = splitlines(&out)
        .into_iter()
        .map(words)
        .filter(|p| p.len() >= 4 && is_dotted(p[2]))
        .map(|p| (p[2].to_string(), if p[3] == "*" { "(no name)".to_string() } else { p[3].to_string() }))
        .collect();
    let key = |ip: &str| ip.split('.').map(|k| k.parse::<u128>().unwrap_or(u128::MAX)).collect::<Vec<_>>();
    list.sort_by_key(|(ip, _)| key(ip));
    Ok(list)
}

// \d+\.\d+\.\d+\.\d+ in full
fn is_dotted(s: &str) -> bool {
    let p: Vec<&str> = s.split('.').collect();
    p.len() == 4 && p.iter().all(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()))
}

// Turn the router tunnel off: sing-box disabled, the device diversion removed, the self-repair cron line gone
pub fn router_off(ssh: &mut impl Ssh) -> Result<String, Error> {
    let out = ssh.run("uci set sing-box.main.enabled=0; uci commit sing-box; /etc/init.d/sing-box stop; sed -i '/heal.sh/d' /etc/crontabs/root; /etc/init.d/cron restart; fw4 reload >/dev/null 2>&1; echo 'router tunnel: off'", None, 60)?;
    Ok(strip(&out).to_string())
}

// Install sing-box, jq and curl from the router's package feed (GL.iNet: same as APPLICATIONS > Plug-ins)
pub fn router_install(ssh: &mut impl Ssh) -> Result<String, Error> {
    let out = ssh.run("opkg update >/dev/null 2>&1; opkg install sing-box jq curl 2>&1 | tail -4; sing-box version 2>/dev/null | head -1", None, 600)?;
    Ok(strip(&out).to_string())
}

// The LAN addresses that go through the router tunnel, from Settings 'devices'
pub fn devices(setting: &str) -> Vec<String> {
    setting
        .split(|c: char| crate::python_compat::is_space(c) || c == ',' || c == ';')
        .filter(|x| validate::lan_ip(x))
        .map(str::to_string)
        .collect()
}

// True when the router tunnels every device on its main network
pub fn all_devices(setting: &str) -> bool {
    strip(setting).to_lowercase() == "all"
}

// The router's firewall include (resources/router/nft-devices.sh) filled in for the chosen devices
pub fn nft_script(template: &str, devices_setting: &str) -> String {
    let body = template.replace('\r', "");
    if all_devices(devices_setting) {
        return body
            .replace("@ELEMENTS@", "")
            .replace("@WHO@", "iifname \"br-lan\" meta nfproto ipv4")
            .replace("@V6@", "    iifname \"br-lan\" oifname != \"br-lan\" meta nfproto ipv6 counter reject");
    }
    let ips = devices(devices_setting);
    let elements = if ips.is_empty() { String::new() } else { format!("elements = {{ {} }}", ips.join(", ")) };
    body.replace("@ELEMENTS@", &elements).replace("@WHO@", "ip saddr @tunneled").replace("@V6@", "")
}

/// Before 1.12 the router's sing-box reads the old config schema.
pub const OLD_SB: &str = r"sing-box version 2>/dev/null | head -1 | grep -qE 'version 1\.([0-9]|1[01])\.'";

// Shell lines that leave router-config.json in /tmp/sb-new.json, in the schema the router's sing-box reads
pub fn router_cfg_sh(c: &Value, sb: &str) -> String {
    format!(
        "cat > /tmp/sb-new.json <<'SBEOF'\n{}\nSBEOF\ncat > /tmp/sb-new12.json <<'SBEOF'\n{}\nSBEOF\nif {}; then rm -f /tmp/sb-new12.json; else mv /tmp/sb-new12.json /tmp/sb-new.json; fi\n",
        dumps(c, Some(2)),
        dumps(&router_cfg_new(c), Some(2)),
        OLD_SB.replacen("sing-box", sb, 1)
    )
}

/// The part of a push that runs after the config check (engine.PUSH_SH).
pub const PUSH_SH: &str = include_str!("router_push.sh");

static PUSHED_MD5: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^pushed-md5 ([0-9a-f]{32})$").unwrap());
static PUSHED_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n?pushed-md5 \S*").unwrap());

/// What a push needs: the router config, the firewall include and the Settings 'devices' value.
pub struct Push<'a> {
    pub router_cfg: &'a Value,
    pub nft_template: &'a str,
    pub devices: &'a str,
    pub heal_sh: &'a str,
    /// The vault nodes not in use, most recently verified first (vault::nodes).
    pub vault: &'a [Value],
}

/// The router's answer to a push, and the md5 of the config it now runs (when the push went through).
#[derive(Debug, Clone, PartialEq)]
pub struct Pushed {
    pub answer: String,
    pub md5: Option<String>,
}

// Upload router-config.json and the device firewall rule, check the config on the router, restart its sing-box and
// install the self-repair (engine.push; the caller saves the md5 and the local copies)
pub fn push(ssh: &mut impl Ssh, p: &Push) -> Result<Pushed, Error> {
    let who = if all_devices(p.devices) { "all".to_string() } else { devices(p.devices).join(" ") };
    let script = format!(
        "{}sing-box check -c /tmp/sb-new.json || {{ echo 'CONFIG REJECTED, nothing deployed'; exit 1; }}\ncat > /tmp/nft-devices.sh <<'NFTEOF'\n{}NFTEOF\n{PUSH_SH}echo '{who}' > /etc/sing-box/rh-devices\necho \"pushed-md5 $(md5sum /etc/sing-box/config.json | cut -c1-32)\"\n",
        router_cfg_sh(p.router_cfg, "sing-box"),
        nft_script(p.nft_template, p.devices)
    );
    let raw = ssh.run("sh -s", Some(&script), SSH_TIMEOUT)?;
    let mut out = strip(&raw).to_string();
    if out.is_empty() {
        out = "no answer from the router".into();
    }
    let md5 = PUSHED_MD5.captures(&out).filter(|_| !out.contains("CONFIG REJECTED")).map(|m| m[1].to_string());
    let mut answer = PUSHED_LINE.replace_all(&out, "").into_owned();
    if answer.contains("sing-box service: running") {
        match push_heal(ssh, p.vault, p.heal_sh) {
            Ok(h) => answer += &format!("\n{h}"),
            Err(Error::Failed(e) | Error::BadAnswer(e)) => answer += &format!("\nself-repair: not installed ({e})"),
        }
    }
    Ok(Pushed { answer, md5 })
}

// Router self-repair: heal.sh, up to 60 vault nodes (router schema, no tags) and a 30-minute cron line
pub fn push_heal(ssh: &mut impl Ssh, vault: &[Value], heal_sh: &str) -> Result<String, Error> {
    let list: Vec<Value> = vault
        .iter()
        .take(60)
        .map(|n| {
            let mut o = outbound19(&node_of(n), "");
            o.as_object_mut().unwrap().shift_remove("tag");
            o
        })
        .collect();
    let script = format!(
        "cat > /etc/sing-box/vault.json <<'VEOF'\n{}\nVEOF\ncat > /etc/sing-box/heal.sh <<'HEOF'\n{}HEOF\nchmod +x /etc/sing-box/heal.sh\nsed -i '/heal.sh/d' /etc/crontabs/root; {{ echo '*/30 * * * * /etc/sing-box/heal.sh' >> /etc/crontabs/root; echo '*/2 * * * * /etc/sing-box/heal.sh pin' >> /etc/crontabs/root; /etc/init.d/cron restart; }}\necho \"self-repair: installed, {} vault nodes on the router\"\n",
        dumps(&Value::Array(list.clone()), None),
        heal_sh.replace('\r', ""),
        list.len()
    );
    Ok(strip(&ssh.run("sh -s", Some(&script), SSH_TIMEOUT)?).to_string())
}

/// A Reality Hop router build, as its manifest-arm64.json describes it.
pub struct Manifest<'a> {
    pub channel: &'a str,
    pub singbox: &'a str,
    pub sha256: &'a str,
}

/// The install script of a router build (engine.RC_INSTALL).
pub const RC_INSTALL: &str = include_str!("router_core_install.sh");

// The script that swaps a router build in, with rollback (engine.rc_script)
pub fn rc_script(m: &Manifest, router_cfg: &Value, secret: &str) -> String {
    RC_INSTALL
        .replace("@CFG@", &router_cfg_sh(router_cfg, "$N"))
        .replace("@SHA@", m.sha256)
        .replace("@VER@", m.singbox)
        .replace("@API@", secret)
        .replace("@CH@", m.channel)
        .replace("@MARK@", RC_MARK)
}

// A config's Clash API secret, or "" (engine.secret)
pub fn secret(cfg: &Value) -> String {
    cfg.pointer("/experimental/clash_api/secret").and_then(Value::as_str).unwrap_or_default().to_string()
}

static ARP_WINDOWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([0-9a-f]{2}(?:-[0-9a-f]{2}){5})\b").unwrap());

// The router's MAC address in Windows `arp -a <host>` output, or "" when it has no entry (engine.router_mac)
pub fn arp_mac_windows(out: &str) -> String {
    ARP_WINDOWS.captures(&out.to_lowercase()).map_or(String::new(), |c| c[1].to_string())
}
