//! sing-box's Clash API on 127.0.0.1:9090: the PC tunnel's control API, reached over loopback with its secret.

use crate::data_files::{jload, jsave, read_json};
use crate::paths::Paths;
use crate::router::TEST_URL;
use serde_json::{Map, Value};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Where the PC tunnel's control API listens.
pub const API_ADDR: &str = "127.0.0.1:9090";
/// How long a node that failed in the running tunnel stays out of the spares.
pub const FAIL_HOURS: f64 = 12.0;

/// What Python raises on an answer that is JSON but not an object.
#[derive(Debug, PartialEq, Eq)]
pub struct NotAnObject;

/// Why a control API call gave no answer.
#[derive(Debug)]
pub enum ApiError {
    /// The API answered with an HTTP error: its status and text.
    Http(u16, String),
    /// No answer: not running, refused or timed out.
    Io(io::Error),
}

impl From<io::Error> for ApiError {
    // A socket problem
    fn from(e: io::Error) -> Self {
        ApiError::Io(e)
    }
}

// A /delay answer as (ms, why not): {"delay": n} -> (n, ""), else (None, its message)
pub fn parse_delay(text: &str) -> Result<(Option<Value>, Value), NotAnObject> {
    let Ok(d) = serde_json::from_str::<Value>(text) else {
        return Ok((None, Value::from("no response")));
    };
    let Value::Object(m) = d else { return Err(NotAnObject) };
    Ok(match m.get("delay") {
        Some(v) => (Some(v.clone()), Value::from("")),
        None => (None, m.get("message").cloned().unwrap_or_else(|| Value::from("failed"))),
    })
}

// Status and body of a raw HTTP/1.x answer
fn parse_response(b: &[u8]) -> io::Result<(u16, String)> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "not an HTTP answer");
    let end = b.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(bad)?;
    let head = String::from_utf8_lossy(&b[..end]);
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or_else(bad)?;
    Ok((status, String::from_utf8_lossy(&b[end + 4..]).into_owned()))
}

/// A sing-box control API: its address and secret.
#[derive(Debug, Clone)]
pub struct ControlApi {
    pub addr: SocketAddr,
    pub secret: String,
}

impl ControlApi {
    // The PC tunnel's API, with the secret from pc-config.json
    pub fn pc(paths: &Paths) -> io::Result<ControlApi> {
        let secret = crate::router::secret(&read_json(&paths.pc_cfg)?);
        Ok(ControlApi { addr: API_ADDR.parse().expect("fixed address"), secret })
    }

    // GET a path; no proxy (engine.pc_get). HTTP/1.0, so Go's server answers without chunks and then closes.
    pub fn get(&self, path: &str, timeout: Duration) -> Result<String, ApiError> {
        let mut s = TcpStream::connect_timeout(&self.addr, timeout)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        let req =
            format!("GET {path} HTTP/1.0\r\nHost: {}\r\nAuthorization: Bearer {}\r\n\r\n", self.addr, self.secret);
        s.write_all(req.as_bytes())?;
        let mut b = Vec::new();
        s.read_to_end(&mut b)?;
        match parse_response(&b)? {
            (200..=299, body) => Ok(body),
            (code, body) => Err(ApiError::Http(code, body)),
        }
    }

    // One node's delay through the running tunnel as (ms, why not) (engine.pc_delay)
    pub fn delay(&self, tag: &str) -> Result<(Option<Value>, Value), NotAnObject> {
        match self.get(&format!("/proxies/{tag}/delay?url={TEST_URL}&timeout=8000"), Duration::from_secs(15)) {
            Ok(t) | Err(ApiError::Http(_, t)) => parse_delay(&t),
            Err(ApiError::Io(_)) => Ok((None, Value::from("no response"))),
        }
    }
}

// The traffic through the PC tunnel now, in KB/s over 10 s, from sing-box's running byte totals (/connections). Asks
// only the local API; sends nothing out. None when the API does not answer
pub fn tunnel_rate(paths: &Paths) -> Option<f64> {
    let api = ControlApi::pc(paths).ok()?;
    let total = || -> Option<f64> {
        let v: Value = serde_json::from_str(&api.get("/connections", Duration::from_secs(3)).ok()?).ok()?;
        Some(v.get("downloadTotal")?.as_f64()? + v.get("uploadTotal")?.as_f64()?)
    };
    let (a, t) = (total()?, Instant::now());
    std::thread::sleep(Duration::from_secs(10));
    let b = total()?;
    Some((b - a).max(0.0) / 1000.0 / t.elapsed().as_secs_f64())
}

/// What check_pc found besides each node's pc_ms and pc_why.
#[derive(Debug, Clone, PartialEq)]
pub struct PcCheck {
    /// The node the tunnel uses now (its tag).
    pub now: Option<Value>,
    /// The tunnel's exit address, when asked for.
    pub exit: Option<String>,
    /// The tunnel still runs an older node set: nothing was tested.
    pub stale: bool,
}

// Set a node's pc_ms and pc_why
fn set_pc(n: &mut Value, ms: Option<Value>, why: Value) {
    if let Some(o) = n.as_object_mut() {
        o.insert("pc_ms".into(), ms.unwrap_or(Value::Null));
        o.insert("pc_why".into(), why);
    }
}

// Fresh test of every node through the PC's running tunnel, one retry each. None when the API is down. When the
// tunnel still runs an older node set (files changed, no restart yet), nothing is tested: vless-N would still point at
// the old server. exit_ip also looks up the exit address through the tunnel (engine.check_pc)
pub fn check_pc(paths: &Paths, nodes: &mut [Value], exit_ip: bool, now: f64) -> Result<Option<PcCheck>, NotAnObject> {
    match ControlApi::pc(paths) {
        Ok(api) => check_pc_with(&api, paths, nodes, exit_ip, now),
        Err(_) => {
            nodes.iter_mut().for_each(|n| set_pc(n, None, Value::from("api down")));
            Ok(None)
        }
    }
}

// check_pc against a given API
pub fn check_pc_with(
    api: &ControlApi,
    paths: &Paths,
    nodes: &mut [Value],
    exit_ip: bool,
    now: f64,
) -> Result<Option<PcCheck>, NotAnObject> {
    let px = api
        .get("/proxies", Duration::from_secs(5))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("proxies").and_then(Value::as_object).cloned());
    let Some(px) = px else {
        nodes.iter_mut().for_each(|n| set_pc(n, None, Value::from("api down")));
        return Ok(None);
    };
    let using = px.get("auto").and_then(|a| a.get("now")).filter(|v| !v.is_null()).cloned();
    let mark = crate::singbox_config::set_mark(nodes.iter().map(|n| n["server"].as_str().unwrap_or_default()));
    if !px.contains_key(&mark) {
        nodes.iter_mut().for_each(|n| set_pc(n, None, Value::Null));
        return Ok(Some(PcCheck { now: using, exit: None, stale: true }));
    }
    let results: Vec<_> = std::thread::scope(|sc| {
        let jobs: Vec<_> = nodes
            .iter()
            .map(|n| {
                let tag = n["tag"].as_str().unwrap_or_default().to_string();
                sc.spawn(move || match api.delay(&tag)? {
                    (None, _) => api.delay(&tag),
                    ok => Ok(ok),
                })
            })
            .collect();
        jobs.into_iter().map(|j| j.join().unwrap_or(Ok((None, Value::from("no response"))))).collect()
    });
    for (n, r) in nodes.iter_mut().zip(results) {
        let (ms, why) = r?;
        set_pc(n, ms, why);
    }
    tunnel_fails_note(paths, nodes, now);
    Ok(Some(PcCheck { now: using, exit: if exit_ip { pc_exit() } else { None }, stale: false }))
}

// A node that fails in the running tunnel can still pass the candidate test. It stays out of the spares for
// FAIL_HOURS, or until it works in the tunnel again (engine.tunnel_fails_note)
pub fn tunnel_fails_note(paths: &Paths, nodes: &[Value], now: f64) {
    if !nodes.iter().any(|n| n.get("pc_ms").is_some_and(|v| !v.is_null())) {
        return; // all dead: more likely this PC's own network than six bad nodes
    }
    let mut f = jload(&paths.tunnel_fails);
    for n in nodes {
        let server = n["server"].as_str().unwrap_or_default();
        if n.get("pc_ms").is_none_or(Value::is_null) {
            f.insert(server.to_string(), Value::from(now));
        } else {
            f.shift_remove(server);
        }
    }
    let kept: Map<String, Value> =
        f.into_iter().filter(|(_, t)| t.as_f64().is_some_and(|t| now - t < FAIL_HOURS * 3600.0)).collect();
    let _ = jsave(&paths.tunnel_fails, &Value::Object(kept), None);
}

// server -> time it last failed in the PC tunnel or broke PIA's connection, within FAIL_HOURS (engine.tunnel_fails)
pub fn tunnel_fails(paths: &Paths, now: f64) -> Map<String, Value> {
    jload(&paths.tunnel_fails)
        .into_iter()
        .chain(jload(&paths.pia_breaks))
        .filter(|(_, t)| t.is_number() && t.as_f64().is_some_and(|t| now - t < FAIL_HOURS * 3600.0))
        .collect()
}

// A node that broke PIA's connection again and again: out of the spares for FAIL_HOURS. Kept apart from
// tunnel-fails.json, since it passes the short tests that clear a node from there
pub fn pia_break_note(paths: &Paths, server: &str, now: f64) {
    let mut f = jload(&paths.pia_breaks);
    f.insert(server.to_string(), Value::from(now));
    let kept: Map<String, Value> =
        f.into_iter().filter(|(_, t)| t.as_f64().is_some_and(|t| now - t < FAIL_HOURS * 3600.0)).collect();
    let _ = jsave(&paths.pia_breaks, &Value::Object(kept), None);
}

// The node the PC tunnel sends traffic through now (its address), from the local control API. None when the API does
// not answer or the tunnel still runs an older set than pc-config.json
pub fn tunnel_node(paths: &Paths) -> Option<String> {
    let api = ControlApi::pc(paths).ok()?;
    let v: Value = serde_json::from_str(&api.get("/proxies", Duration::from_secs(3)).ok()?).ok()?;
    let px = v.get("proxies")?.as_object()?;
    let list = crate::singbox_config::nodes_of(&paths.pc_cfg).ok()?;
    let mark = crate::singbox_config::set_mark(list.iter().map(|(_, n)| n.server.as_str()));
    if !px.contains_key(&mark) {
        return None;
    }
    let tag = px.get("auto")?.get("now")?.as_str()?;
    list.into_iter().find(|(t, _)| t == tag).map(|(_, n)| n.server)
}

// The PC tunnel's exit address, from api.ipify.org through the tunnel's SOCKS port (engine.pc_exit)
pub fn pc_exit() -> Option<String> {
    exit_ip(2080)
}

// The exit address of the SOCKS proxy on a loopback port
pub fn exit_ip(port: u16) -> Option<String> {
    let proxy = format!("127.0.0.1:{port}");
    let curl = crate::process::system_tool("curl").into_os_string();
    let mut cmd: Vec<std::ffi::OsString> = vec![curl, "-s".into()];
    if cfg!(windows) {
        cmd.push("--ssl-no-revoke".into()); // Schannel would ask the certificate's revocation server
    }
    for a in ["-m", "10", "--socks5-hostname", &proxy, "https://api.ipify.org"] {
        cmd.push(a.into());
    }
    let out = crate::process::run(&cmd, None, Duration::from_secs(240), None).ok()?.stdout;
    Some(crate::python_compat::strip(&out).to_string()).filter(|s| !s.is_empty())
}

// Wait up to limit until the PC tunnel's control API answers; true when it does (engine.wait_api)
pub fn wait_api(paths: &Paths, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if ControlApi::pc(paths).is_ok_and(|api| api.get("/proxies", Duration::from_secs(3)).is_ok()) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    false
}
