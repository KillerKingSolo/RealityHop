//! The public node lists (github.com/morpheusadam/v2ray-config, github.com/rtwo2/FastNodes): sources and the line
//! parser.

use crate::node::Node;
use crate::{python_compat, validate};
use std::collections::HashSet;

/// The node lists, merged; a server in both keeps the first list's line. morpheusadam's comes first: tested side by side,
/// its credentials worked wherever FastNodes' did, and on some servers where they did not. FastNodes publishes on its
/// "outputs" branch.
pub const FEEDS: [&str; 2] = [
    "https://raw.githubusercontent.com/morpheusadam/v2ray-config/main/subs/bundles/reality.txt",
    "https://raw.githubusercontent.com/rtwo2/FastNodes/outputs/sub/verified_tls.txt",
];
/// The same file on CDN mirrors, for when GitHub is blocked; {0} = owner/repo, {1} = path, {2} = branch.
pub const MIRRORS: [&str; 2] =
    ["https://cdn.jsdelivr.net/gh/{0}@{2}/{1}", "https://fastly.jsdelivr.net/gh/{0}@{2}/{1}"];
/// uTLS fingerprints a node may ask for; anything else becomes chrome.
pub const FPS: [&str; 10] =
    ["chrome", "firefox", "safari", "ios", "android", "edge", "360", "qq", "random", "randomized"];
/// The only SNIs (fronts) accepted.
pub const FRONTS: [&str; 15] = [
    "www.apple.com",
    "swdist.apple.com",
    "www.icloud.com",
    "www.cloudflare.com",
    "cloudflare.com",
    "www.amazon.com",
    "amazon.com",
    "yahoo.com",
    "www.yahoo.com",
    "cdnjs.com",
    "www.microsoft.com",
    "www.bing.com",
    "www.python.org",
    "www.nvidia.com",
    "www.amd.com",
];

// A raw.githubusercontent.com URL on the CDN mirrors; nothing for other hosts
pub fn mirrors(u: &str) -> Vec<String> {
    let Ok(s) = python_compat::urlsplit(u) else { return Vec::new() };
    if s.hostname().as_deref() != Some("raw.githubusercontent.com") {
        return Vec::new();
    }
    let mut path = s.path.as_str();
    if let Some(semi) = path.rfind('/').and_then(|i| path[i..].find(';').map(|j| i + j)) {
        path = &path[..semi]; // urlparse moves ;params of the last segment out of the path
    }
    let p: Vec<&str> = path.split('/').collect();
    let repo = p.get(1..3.min(p.len())).unwrap_or(&[]).join("/");
    let file = p.get(4..).unwrap_or(&[]).join("/");
    let branch = p.get(3).filter(|b| !b.is_empty()).copied().unwrap_or("main");
    MIRRORS.iter().map(|m| m.replace("{0}", &repo).replace("{1}", &file).replace("{2}", branch)).collect()
}

// A line worth keeping from a download
pub fn usable(l: &str) -> bool {
    l.starts_with("vless://") && l.contains("reality") && l.contains("vision")
}

// The candidates in a node list, one per server, first line wins (engine.feed)
pub fn parse(text: &str) -> Vec<Node> {
    let (mut seen, mut out) = (HashSet::new(), Vec::new());
    for l in python_compat::splitlines(text) {
        if usable(l)
            && let Some(n) = parse_line(python_compat::strip(l))
            && seen.insert(n.server.clone())
        {
            out.push(n);
        }
    }
    out
}

// One vless:// line as a node, or None when any check fails
fn parse_line(l: &str) -> Option<Node> {
    let u = python_compat::urlsplit(l).ok()?;
    if !u.netloc.is_ascii() {
        return None; // Python either raises or fails a check below for every non-ASCII netloc
    }
    let q = python_compat::parse_qs_first(&u.query);
    let get = |k: &str| q.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str());
    let (h, port) = (u.hostname().unwrap_or_default(), u.port().ok()?);
    let pbk = get("pbk").unwrap_or("");
    let sid = get("sid").unwrap_or("");
    let user = u.username().unwrap_or("");
    let ok = get("security") == Some("reality")
        && port == Some(443)
        && get("flow") == Some("xtls-rprx-vision")
        && get("sni").is_some_and(|s| FRONTS.contains(&s))
        && validate::public_ip(&h)
        && validate::pbk_ok(pbk)
        && validate::sid_ok(sid)
        && validate::uuid_ok(user);
    ok.then(|| Node {
        server: h,
        uuid: user.to_string(),
        pbk: pbk.to_string(),
        sid: sid.to_string(),
        sni: get("sni").unwrap_or_default().to_string(),
        fp: get("fp").filter(|f| FPS.contains(f)).unwrap_or("chrome").to_string(),
    })
}
