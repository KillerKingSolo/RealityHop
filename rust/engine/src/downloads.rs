//! Downloads from GitHub and its CDN mirrors: direct first, then through the PC tunnel, which gets past a block. TLS
//! stays end to end with the server; the trusted roots are Mozilla's list (engine.gh_get, feed_download, feed).

use crate::activity_log::{dbg, say};
use crate::paths::Paths;
use crate::processes_and_ports::port_open;
use crate::router::Ssh;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// The PC tunnel's mixed inbound also speaks HTTP proxy.
pub const TUNNEL_PROXY: &str = "http://127.0.0.1:2080";
/// After a name lookup fails, the tunnel goes first this long: each direct try would wait for DNS again.
const DNS_DOWN_FOR: Duration = Duration::from_secs(600);
/// A node list download is reused this long.
const FEED_KEEP: Duration = Duration::from_secs(900);
/// The largest download taken (a Reality Hop release is about 50 MB).
const MAX_BYTES: u64 = 512 * 1024 * 1024;
/// GitHub's API: 60 asks an hour per address without a login, shared by everyone behind the same VPN exit.
const API: &str = "https://api.github.com/";
/// An API answer this fresh (seconds) is used without asking GitHub again: without a login GitHub counts every ask,
/// a 304 too, so app restarts and dialogs in a row would use up the hour.
pub const API_FRESH: f64 = 900.0;

static DNS_DOWN: Mutex<Option<Instant>> = Mutex::new(None);
static FEED: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Why a download failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetError {
    /// The server answered with an HTTP error (404, rate limit): through the tunnel it answers the same.
    Status(u16),
    /// The server's name could not be looked up.
    NoName(String),
    /// The whole reply took longer than this many seconds.
    TooSlow(u64),
    /// Anything else: refused, reset, TLS.
    Failed(String),
}

impl std::fmt::Display for GetError {
    // The message for the log
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GetError::Status(c) => write!(f, "HTTP Error {c}"),
            GetError::TooSlow(s) => write!(f, "download too slow, stopped after {s} s"),
            GetError::NoName(e) | GetError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GetError {}

/// One answer: its HTTP status (304 for not modified), ETag and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Got {
    pub status: u16,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}

// A failed name lookup (Python's socket.gaierror): Windows' resolver errors, or the system resolver's message elsewhere
fn lookup_failed(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(11001..=11004) if cfg!(windows))
        || e.to_string().contains("failed to lookup address information")
}

// One try: connect and wait for the answer at most wait, the whole reply at most total; through proxy when given.
// The total limit matters: a filter that lets a few bytes through now and then could keep a download going for hours.
fn fetch(
    url: &str,
    headers: &[(&str, &str)],
    wait: Duration,
    total: Duration,
    proxy: Option<&str>,
) -> Result<Got, GetError> {
    let mut cfg = ureq::Agent::config_builder()
        .timeout_connect(Some(wait))
        .timeout_recv_response(Some(wait))
        .timeout_recv_body(Some(total));
    if let Some(p) = proxy {
        cfg = cfg.proxy(Some(ureq::Proxy::new(p).map_err(|e| GetError::Failed(e.to_string()))?));
    }
    let agent = ureq::Agent::new_with_config(cfg.build());
    let mut req = agent.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let map = |e: ureq::Error| match e {
        ureq::Error::StatusCode(c) => GetError::Status(c),
        ureq::Error::HostNotFound => GetError::NoName(format!("name lookup failed for {url}")),
        ureq::Error::Io(e) if lookup_failed(&e) => GetError::NoName(e.to_string()),
        ureq::Error::Timeout(ureq::Timeout::RecvBody) => GetError::TooSlow(total.as_secs()),
        e => GetError::Failed(e.to_string()),
    };
    let r = req.call().map_err(map)?;
    let status = r.status().as_u16();
    let etag = r.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body = r.into_body().with_config().limit(MAX_BYTES).read_to_vec().map_err(map)?;
    Ok(Got { status, etag, body })
}

// Whether a direct name lookup failed in the last 10 minutes
fn dns_down() -> bool {
    DNS_DOWN.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < DNS_DOWN_FOR)
}

// A whole download from GitHub: direct first, then through the PC tunnel, which gets past a block. The retry also
// covers a download that stalls partway, as DPI filters do. API answers go through api_cached. timeout is in seconds
// (engine.gh_get)
pub fn gh_get(paths: &Paths, url: &str, timeout: u64, headers: &[(&str, &str)]) -> Result<Vec<u8>, GetError> {
    if !url.starts_with(API) {
        return gh_got(paths, url, timeout, headers).map(|g| g.body);
    }
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    api_cached(&paths.data.join("github-cache.json"), url, now, |etag| {
        let h: Vec<(&str, &str)> = headers.iter().copied().chain(etag.map(|e| ("If-None-Match", e))).collect();
        gh_got(paths, url, timeout, &h)
    })
}

// A GitHub API answer through the cache in file (url -> ETag, body, time). An answer under API_FRESH old is used as
// is. Else GitHub is asked with the kept ETag: a 304 (not modified) gives the kept answer without sending it again.
// When GitHub refuses (403 or 429: the hourly limit is used up), the kept answer is used too (engine.api_cached)
pub fn api_cached(
    file: &Path,
    url: &str,
    now: f64,
    get: impl FnOnce(Option<&str>) -> Result<Got, GetError>,
) -> Result<Vec<u8>, GetError> {
    let mut all = crate::data_files::jload(file);
    let kept = all.get(url).cloned().unwrap_or(Value::Null);
    let body = kept["body"].as_str().map(|b| b.as_bytes().to_vec());
    let at = kept["at"].as_f64().unwrap_or(0.0);
    if let Some(b) = &body
        && (0.0..API_FRESH).contains(&(now - at))
    {
        return Ok(b.clone());
    }
    let etag = body.as_ref().and(kept["etag"].as_str());
    let mut keep = |etag: &str, text: &str| {
        all.insert(url.to_string(), json!({"etag": etag, "body": text, "at": now}));
        let _ = crate::data_files::jsave(file, &Value::Object(all.clone()), None);
    };
    match get(etag) {
        Ok(g) if g.status == 304 => {
            let b = body.ok_or_else(|| GetError::Failed("304 without a kept answer".into()))?;
            keep(etag.unwrap_or_default(), &String::from_utf8_lossy(&b));
            Ok(b)
        }
        Ok(g) => {
            if let (Some(e), Ok(text)) = (&g.etag, std::str::from_utf8(&g.body)) {
                keep(e, text);
            }
            Ok(g.body)
        }
        Err(GetError::Status(c @ (403 | 429))) if body.is_some() => {
            say(&format!("GitHub refused (HTTP {c}, its hourly limit); using its last answer"));
            Ok(body.unwrap_or_default())
        }
        Err(e) => Err(e),
    }
}

// gh_get without the API cache: the answer with its status and ETag
fn gh_got(paths: &Paths, url: &str, timeout: u64, headers: &[(&str, &str)]) -> Result<Got, GetError> {
    let (wait, total) = (Duration::from_secs(timeout), Duration::from_secs(timeout * 6));
    let tunnel_up = || !crate::settings::router_only(paths) && port_open(2080);
    let tunnel = || {
        let t = Instant::now();
        let r = fetch(url, headers, wait * 3, total, Some(TUNNEL_PROXY));
        dbg(|| match &r {
            Ok(g) => format!("download via tunnel ok {} B {:.1} s {url}", g.body.len(), t.elapsed().as_secs_f64()),
            Err(e) => format!("download via tunnel FAILED {:.1} s: {e} {url}", t.elapsed().as_secs_f64()),
        });
        r
    };
    if dns_down() && tunnel_up() {
        dbg(|| format!("download: name lookups failed lately, the tunnel goes first {url}"));
        match tunnel() {
            Err(GetError::Status(c)) => return Err(GetError::Status(c)),
            Err(_) => {} // the tunnel is down too: direct after all
            ok => return ok,
        }
    }
    let t = Instant::now();
    match fetch(url, headers, wait, total, None) {
        Ok(g) => {
            dbg(|| format!("download direct ok {} B {:.1} s {url}", g.body.len(), t.elapsed().as_secs_f64()));
            Ok(g)
        }
        Err(GetError::Status(c)) => Err(GetError::Status(c)),
        Err(e) => {
            dbg(|| format!("download direct FAILED {:.1} s: {e} {url}", t.elapsed().as_secs_f64()));
            if matches!(e, GetError::NoName(_)) {
                *DNS_DOWN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            }
            if !tunnel_up() {
                return Err(e);
            }
            say(&format!("direct download failed ({e}); retrying through the tunnel"));
            tunnel()
        }
    }
}

// Bytes as text with invalid UTF-8 left out (Python's decode('utf-8', 'ignore'))
fn utf8_ignore(b: &[u8]) -> String {
    b.utf8_chunks().map(|c| c.valid()).collect()
}

// One node list: GitHub, then its CDN mirrors; only the usable lines. "" when every source failed
fn one_feed(paths: &Paths, u: &str) -> String {
    for src in std::iter::once(u.to_string()).chain(crate::node_feed::mirrors(u)) {
        let t = Instant::now();
        match gh_get(paths, &src, 20, &[]) {
            Ok(b) => {
                let text = utf8_ignore(&b);
                let raw: Vec<&str> = crate::python_compat::splitlines(&text)
                    .into_iter()
                    .filter(|l| crate::node_feed::usable(l))
                    .collect();
                if !raw.is_empty() {
                    return raw.join("\n");
                }
            }
            Err(e) => {
                let host = crate::python_compat::urlsplit(&src).ok().and_then(|s| s.hostname()).unwrap_or_default();
                say(&format!("node list source failed ({host}): {e}"));
                if matches!(e, GetError::NoName(_)) && t.elapsed() > Duration::from_secs(10) {
                    // each would wait the same 30 s; a DNS block answers at once, so the mirrors still get their turn
                    say(
                        "name lookups get no answer on this PC (DNS down, a VPN still connecting?); skipping the other sources",
                    );
                    break;
                }
            }
        }
    }
    String::new()
}

// The node lists from GitHub (direct, then through the tunnel), the CDN mirrors, the router, then the last good copy.
// Only the usable lines are kept (about a quarter of the 18 MB) (engine.feed_download)
pub fn feed_download(paths: &Paths, ssh: &mut impl Ssh) -> Result<String, String> {
    feed_download_from(paths, ssh, &crate::node_feed::FEEDS)
}

// feed_download from these node lists
pub fn feed_download_from(paths: &Paths, ssh: &mut impl Ssh, feeds: &[&str]) -> Result<String, String> {
    let parts: Vec<String> = std::thread::scope(|s| {
        let jobs: Vec<_> = feeds.iter().map(|u| s.spawn(move || one_feed(paths, u))).collect();
        jobs.into_iter().map(|j| j.join().unwrap_or_default()).collect()
    });
    let mut raw = parts.join("\n");
    if !raw.contains("vless://") && !crate::settings::pc_only(paths) {
        say("this PC could not download the node lists; asking the router to download them...");
        match crate::router::router_feed(ssh) {
            Ok(r) => raw = r,
            Err(crate::router::Error::Failed(e) | crate::router::Error::BadAnswer(e)) => {
                say(&format!("router download failed: {e}"))
            }
        }
    }
    if raw.contains("vless://") {
        let _ = crate::data_files::write_text(&paths.feed_cache, &raw, crate::data_files::Newlines::Platform);
        return Ok(raw);
    }
    let none = || "no node list source works and there is no saved copy".to_string();
    let modified = std::fs::metadata(&paths.feed_cache).and_then(|m| m.modified()).map_err(|_| none())?;
    let raw = crate::data_files::read_text(&paths.feed_cache).map_err(|_| none())?;
    let age = SystemTime::now().duration_since(modified).unwrap_or_default().as_secs_f64() / 3600.0;
    say(&format!("every node list source failed; using the saved copy from {age:.0} h ago"));
    Ok(raw)
}

// Candidates from the public node lists, merged, one per server; the download is reused for 15 minutes (engine.feed)
pub fn feed(paths: &Paths, ssh: &mut impl Ssh) -> Result<Vec<crate::node::Node>, String> {
    let mut g = FEED.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_none_or(|(t, _)| t.elapsed() > FEED_KEEP) {
        *g = Some((Instant::now(), feed_download(paths, ssh)?));
    }
    Ok(crate::node_feed::parse(g.as_ref().map_or("", |(_, raw)| raw)))
}

// Drop the node list download once it is older than its 15 minutes: it is several MB, and a new download is due
// anyway (engine.memory_trim)
pub fn feed_forget_stale() {
    let mut g = FEED.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|(t, _)| t.elapsed() > FEED_KEEP) {
        *g = None;
    }
}
