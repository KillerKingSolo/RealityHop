//! Anonymous problem reports: the logs and a setup summary with everything personal replaced by neutral tokens.
//! Nothing is sent anywhere; the person attaches the file to a GitHub issue themselves.

use crate::node_feed::FRONTS;
use crate::python_compat::{ipv4, is_link_local, is_loopback, is_private, strip};
use fancy_regex::{Captures, Regex};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Public DNS resolvers say nothing about anyone.
const KEEP_IPS: [&str; 5] = ["0.0.0.0", "9.9.9.9", "149.112.112.112", "1.1.1.1", "8.8.8.8"];
/// The sites Reality Hop itself uses (plus the fronts).
const KEEP_HOSTS: [&str; 10] = [
    "api.ipify.org",
    "www.gstatic.com",
    "ip-api.com",
    "api.github.com",
    "github.com",
    "raw.githubusercontent.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "cdn.jsdelivr.net",
    "fastly.jsdelivr.net",
];
/// File name endings that end no website's name.
const KEEP_EXT: [&str; 11] = ["json", "pyw", "exe", "log", "txt", "ico", "cmd", "lnk", "tmp", "dll", "old"];
/// Lines per log in a report.
pub const LINES: usize = 400;

// A pattern that is known to compile
fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

static KEEP_FILES: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^(?:(engine|install|report|version|check-backend)\.py|(heal|nft-devices)\.sh|realityhop-app\.zip|sing-box-[\w.-]+\.(zip|tar\.gz)|realityhop\.exe\.new|(readme|changelog)\.md|realityhop\.app|realityhop-mac-[\w-]+\.dmg|com\.realityhop\.\w+(\.plist)?|add-router-key\.command|macos\.py)$",
    )
});
static TZ: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^[+-]\d{4} (?=\d{4}-\d\d-\d\d)"));
// A profile folder followed by more path: the whole name up to the next \ or /, spaces included (1.7.x stopped at
// the first space, so "C:\Users\Jo Smith\..." kept "Smith"). Names never hold : * ? " < > |.
static WIN_HOME_DIR: LazyLock<Regex> = LazyLock::new(|| re(r#"\b([A-Za-z]:\\Users\\)[^\\":*?<>|\r\n]{1,64}(?=\\)"#));
static MAC_HOME_DIR: LazyLock<Regex> = LazyLock::new(|| re(r#"(?<![\w.])/Users/(?!Shared/)[^/":\r\n]{1,64}(?=/)"#));
// A profile folder at the end of a path: up to the first space
static WIN_HOME: LazyLock<Regex> = LazyLock::new(|| re(r#"\b[A-Za-z]:\\Users\\[^\\\s"']+"#));
static MAC_HOME: LazyLock<Regex> = LazyLock::new(|| re(r#"(?<![\w.])/Users/(?!Shared/)[^/\s"']+"#));
static SECRET_KV: LazyLock<Regex> =
    LazyLock::new(|| re(r#"(?i)\b(sid|short_id|token|secret|password|bearer)([=:\s]+)[^\s&"']+"#));
static EMAIL: LazyLock<Regex> = LazyLock::new(|| re(r"[\w.+-]+@[\w-]+\.[\w.-]+"));
static HOST: LazyLock<Regex> = LazyLock::new(|| re(r"(?<![\w-])(?:[A-Za-z0-9-]+\.)+[A-Za-z]{2,}(?![\w-])"));
static UUID: LazyLock<Regex> =
    LazyLock::new(|| re(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"));
static MAC: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:[0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}\b"));
static MAC_DOTS: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:[0-9a-fA-F]{4}\.){2}[0-9a-fA-F]{4}\b"));
static IPV6: LazyLock<Regex> = LazyLock::new(|| re(r"(?<![\w:])(?:[0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}(?![\w:])"));
static IPV4: LazyLock<Regex> = LazyLock::new(|| re(r"(?<![\d.])\d{1,3}(?:\.\d{1,3}){3}(?!\d|\.\d)"));
// A REALITY public key, also one that starts or ends with - or _ (1.7.x's \b missed those)
static KEY: LazyLock<Regex> = LazyLock::new(|| re(r"(?<![A-Za-z0-9_-])[A-Za-z0-9_-]{43}(?![A-Za-z0-9_-])"));
static HEX: LazyLock<Regex> = LazyLock::new(|| re(r"\b[0-9a-fA-F]{32,64}\b"));

// re.sub with a function: every match replaced by f(match)
fn sub(r: &Regex, text: &str, mut f: impl FnMut(&Captures<'_, str>) -> String) -> String {
    let (mut out, mut last) = (String::with_capacity(text.len()), 0);
    for c in r.captures_iter(text).flatten() {
        let m = c.get(0).unwrap();
        out.push_str(&text[last..m.start()]);
        out.push_str(&f(&c));
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

/// Who and what to take out of a report.
pub struct Identity<'a> {
    /// The Clash API secrets of both configs.
    pub secrets: Vec<String>,
    /// The node addresses in both configs and the vault.
    pub nodes: HashSet<String>,
    /// The home folder, the user name, the computer name and the domain.
    pub home: &'a str,
    pub user: Option<&'a str>,
    pub computer: Option<&'a str>,
    pub domain: Option<&'a str>,
    /// The path separator of this system.
    pub sep: char,
}

/// Replaces personal data with tokens: node-N (a public node), lan-N (an address on the local network), ip-N (any
/// other internet address), host-N (a website), ipv6-N, mac-N, <user>, <pc>, <email>, <uuid>, <key>, <secret>.
/// The same value always gets the same token within one report.
pub struct Scrubber {
    exact: Vec<(Regex, String)>,
    nodes: HashSet<String>,
    maps: HashMap<&'static str, HashMap<String, String>>,
}

impl Scrubber {
    // A scrubber for one report (report.Scrubber)
    pub fn new(id: &Identity) -> Scrubber {
        let mut exact: Vec<(String, String)> = Vec::new();
        let mut set = |k: &str, v: &str, overwrite: bool| match exact.iter_mut().find(|(x, _)| x == k) {
            Some(e) if overwrite => e.1 = v.to_string(),
            Some(_) => {}
            None => exact.push((k.to_string(), v.to_string())),
        };
        for s in id.secrets.iter().filter(|s| !s.is_empty()) {
            set(s, "<secret>", true);
        }
        let (dir, base) = id.home.rsplit_once(['\\', '/']).unwrap_or(("", id.home));
        set(id.home, &format!("{dir}{}<user>", id.sep), true);
        for (v, t) in [(id.user, "<user>"), (Some(base), "<user>"), (id.computer, "<pc>"), (id.domain, "<pc>")] {
            if let Some(v) = v
                && v.chars().count() > 2
                && !["user", "admin", "administrator", "windows"].contains(&v.to_lowercase().as_str())
            {
                set(v, t, false);
            }
        }
        exact.sort_by_key(|(k, _)| std::cmp::Reverse(k.chars().count()));
        let exact = exact
            .into_iter()
            .map(|(k, v)| (re(&format!("(?i)(?<![A-Za-z0-9]){}(?![A-Za-z0-9])", fancy_regex::escape(&k))), v))
            .collect();
        Scrubber { exact, nodes: id.nodes.clone(), maps: HashMap::new() }
    }

    // The token of a value of one kind: kind-1, kind-2, ... in order of first sight
    fn tok(&mut self, kind: &'static str, value: &str) -> String {
        let m = self.maps.entry(kind).or_default();
        let n = m.len() + 1;
        m.entry(value.to_string()).or_insert_with(|| format!("{kind}-{n}")).clone()
    }

    // A website's name, unless Reality Hop uses it or it is a file name
    fn host(&mut self, found: &str) -> String {
        let h = found.to_lowercase();
        let ext = h.rsplit_once('.').map_or("", |(_, e)| e);
        let keep = KEEP_HOSTS.contains(&h.as_str())
            || FRONTS.contains(&h.as_str())
            || KEEP_EXT.contains(&ext)
            || KEEP_FILES.is_match(&h).unwrap_or(false);
        if keep { found.to_string() } else { self.tok("host", &h) }
    }

    // An IPv4 address as node-N, lan-N or ip-N; loopback and public resolvers stay
    fn ipv4(&mut self, ip: &str) -> String {
        let Some(a) = ipv4(ip) else { return ip.to_string() };
        if is_loopback(a) || KEEP_IPS.contains(&ip) {
            return ip.to_string();
        }
        if self.nodes.contains(ip) {
            return self.tok("node", ip);
        }
        self.tok(if is_private(a) || is_link_local(a) { "lan" } else { "ip" }, ip)
    }

    // One text with everything personal replaced
    pub fn scrub(&mut self, text: &str) -> String {
        let mut text = text.to_string();
        for (r, v) in &self.exact {
            text = sub(r, &text, |_| v.clone());
        }
        text = sub(&TZ, &text, |_| String::new());
        text = sub(&WIN_HOME_DIR, &text, |c| format!("{}<user>", &c[1]));
        text = sub(&WIN_HOME, &text, |c| format!("{}<user>", c[0].chars().take(9).collect::<String>()));
        text = sub(&MAC_HOME_DIR, &text, |_| "/Users/<user>".into());
        text = sub(&MAC_HOME, &text, |_| "/Users/<user>".into());
        text = sub(&SECRET_KV, &text, |c| format!("{}{}<secret>", &c[1], &c[2]));
        text = sub(&EMAIL, &text, |_| "<email>".into());
        text = sub(&HOST, &text, |c| self.host(&c[0]));
        text = sub(&UUID, &text, |_| "<uuid>".into());
        text = sub(&MAC, &text, |c| self.tok("mac", &c[0].to_lowercase()));
        text = sub(&MAC_DOTS, &text, |c| self.tok("mac", &c[0].to_lowercase()));
        text = sub(&IPV6, &text, |c| {
            let m = &c[0];
            if m.matches(':').count() >= 4 || m.contains("::") { self.tok("ipv6", m) } else { m.to_string() }
        });
        text = sub(&IPV4, &text, |c| self.ipv4(&c[0]));
        text = sub(&KEY, &text, |_| "<key>".into());
        sub(&HEX, &text, |_| "<secret>".into())
    }
}

static CORE_LINE: LazyLock<Regex> = LazyLock::new(|| re(r"^(\d{4}-\d\d-\d\d) (\d\d:\d\d:\d\d) (.*)"));
static CONN_ID: LazyLock<Regex> = LazyLock::new(|| re(r"\[\d+ [\d.]+(?:[mµn]?s|m\d+(?:\.\d+)?s)+\] "));
static LOCAL_PORT: LazyLock<Regex> = LazyLock::new(|| re(r":\d{2,5}->"));

// sing-box repeats the same error many times: drop connection ids and local ports, then show each message once,
// newest last, with how often and since when (report.core_lines)
pub fn core_lines(lines: &[String]) -> Vec<String> {
    let mut seen: Vec<(String, (String, String, u64))> = Vec::new();
    for l in lines {
        let Ok(Some(m)) = CORE_LINE.captures(l.as_str()) else { continue };
        let at = format!("{} {}", &m[1], &m[2]);
        let msg = sub(&CONN_ID, &m[3], |_| String::new());
        let msg = sub(&LOCAL_PORT, &msg, |_| "->".into());
        let prev = seen.iter().position(|(x, _)| *x == msg).map(|i| seen.remove(i).1); // a repeat moves to the end
        let (first, n) = prev.map_or((at.clone(), 0), |(f, _, n)| (f, n));
        seen.push((msg, (first, at, n + 1)));
    }
    seen.into_iter()
        .map(|(msg, (first, last, n))| {
            let more = if n > 1 {
                format!("   (x{n}, first {})", first.chars().skip(11).collect::<String>())
            } else {
                String::new()
            };
            format!("{last}  {msg}{more}")
        })
        .collect()
}

// urllib.parse.quote_plus: letters, digits and _.-~ stay, space becomes +, the rest %XX of its UTF-8 bytes
fn quote_plus(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// A new GitHub issue with the report's summary filled in; the person attaches the report file (report.issue_url)
pub fn issue_url(path: &str, text: &str) -> String {
    let head = strip(text.split("\nWatcher log (").next().unwrap_or_default());
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let body = format!(
        "**What happened**\n\n\n**What you expected**\n\n\n**Summary (redacted)**\n```\n{}\n```\n\n**Report:** drag `{name}` from the anonymous-logs folder into this box.\n",
        head.chars().take(3000).collect::<String>()
    );
    format!(
        "https://github.com/{}/issues/new?labels=bug&title={}&body={}",
        crate::REPO,
        quote_plus("Problem: "),
        quote_plus(&body)
    )
}
