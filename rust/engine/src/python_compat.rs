//! Python behavior the engine depends on, copied exactly: str.splitlines, str.strip, urllib.parse (urlsplit, the
//! netloc fields, parse_qs) and ipaddress (strict IPv4 parsing, is_global). The parity tests hold these to Python 3.15.

use std::net::Ipv4Addr;

// Python's str.isspace() for one character (wider than char::is_whitespace: it includes \x1c-\x1f)
pub fn is_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | '\x1c'..='\x1f' | ' ' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}'
        | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

// str.strip()
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

// str.splitlines(): splits on \n, \r, \r\n, \v, \f, \x1c, \x1d, \x1e, \x85,  ,  ; no empty last piece
pub fn splitlines(s: &str) -> Vec<&str> {
    let (mut out, mut start, mut it) = (Vec::new(), 0, s.char_indices().peekable());
    while let Some((i, c)) = it.next() {
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && it.peek().is_some_and(|&(_, n)| n == '\n') {
                it.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// urllib.parse.urlsplit; Err is the ValueError Python raises.
#[derive(Debug, Default, PartialEq)]
pub struct Split {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

// urlsplit(url): scheme, netloc, path, query, fragment
pub fn urlsplit(url: &str) -> Result<Split, &'static str> {
    let url: String =
        url.trim_start_matches(|c: char| c <= ' ').chars().filter(|c| !matches!(c, '\t' | '\r' | '\n')).collect();
    let mut r = Split::default();
    let mut rest = url.as_str();
    if let Some(i) = rest.find(':')
        && i > 0
        && rest.as_bytes()[0].is_ascii_alphabetic()
        && rest[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
    {
        r.scheme = rest[..i].to_ascii_lowercase();
        rest = &rest[i + 1..];
    }
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        r.netloc = after[..end].to_string();
        rest = &after[end..];
        if r.netloc.contains('[') != r.netloc.contains(']') {
            return Err("Invalid IPv6 URL");
        }
    }
    if let Some((a, f)) = rest.split_once('#') {
        r.fragment = f.to_string();
        rest = a;
    }
    if let Some((a, q)) = rest.split_once('?') {
        r.query = q.to_string();
        rest = a;
    }
    r.path = rest.to_string();
    Ok(r)
}

impl Split {
    // The part after the last '@'
    fn hostinfo(&self) -> &str {
        self.netloc.rsplit_once('@').map_or(self.netloc.as_str(), |(_, h)| h)
    }
    // (hostname as written, port text) as in _hostinfo
    fn host_port(&self) -> (&str, Option<&str>) {
        let h = self.hostinfo();
        let (host, port) = match h.split_once('[') {
            Some((_, b)) => {
                let (host, after) = b.split_once(']').unwrap_or((b, ""));
                (host, after.split_once(':').map_or("", |(_, p)| p))
            }
            None => h.split_once(':').unwrap_or((h, "")),
        };
        (host, if port.is_empty() { None } else { Some(port) })
    }
    /// .hostname: lowercased, None when empty.
    pub fn hostname(&self) -> Option<String> {
        let (h, _) = self.host_port();
        if h.is_empty() {
            return None;
        }
        Some(match h.split_once('%') {
            Some((a, zone)) => format!("{}%{zone}", a.to_lowercase()),
            None => h.to_lowercase(),
        })
    }
    /// .port: Err when it is not ASCII digits or out of 0-65535.
    pub fn port(&self) -> Result<Option<u16>, &'static str> {
        match self.host_port().1 {
            None => Ok(None),
            Some(p) if !p.bytes().all(|b| b.is_ascii_digit()) => Err("Port could not be cast to integer value"),
            Some(p) => {
                let digits = p.trim_start_matches('0');
                match digits.len() {
                    0 => Some(0),
                    1..=5 => digits.parse::<u32>().ok().filter(|&n| n <= 65535).map(|n| n as u16),
                    _ => None,
                }
                .map(Some)
                .ok_or("Port out of range 0-65535")
            }
        }
    }
    /// .username: None without '@'.
    pub fn username(&self) -> Option<&str> {
        self.netloc.rsplit_once('@').map(|(u, _)| u.split_once(':').map_or(u, |(n, _)| n))
    }
}

// unquote_plus with utf-8 and errors='replace'
pub fn unquote_plus(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// parse_qs(q) with the first value of each key, blank values and fields without '=' dropped
pub fn parse_qs_first(q: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for field in q.split('&') {
        if let Some((k, v)) = field.split_once('=')
            && !v.is_empty()
        {
            let k = unquote_plus(k);
            if !out.iter().any(|(x, _)| *x == k) {
                out.push((k, unquote_plus(v)));
            }
        }
    }
    out
}

// ipaddress.IPv4Address(s) for a str: four ASCII octets, no leading zeros, each 0-255
pub fn ipv4(s: &str) -> Option<Ipv4Addr> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut o = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0'))
        {
            return None;
        }
        o[i] = p.parse::<u16>().ok().filter(|&n| n <= 255)? as u8;
    }
    Some(Ipv4Addr::from(o))
}

// Whether a is inside net/len
fn within(a: Ipv4Addr, net: [u8; 4], len: u32) -> bool {
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    u32::from(a) & mask == u32::from(Ipv4Addr::from(net)) & mask
}

// IPv4Address.is_global (Python 3.13+ table: not 100.64.0.0/10 and not is_private)
pub fn is_global(a: Ipv4Addr) -> bool {
    const PRIVATE: [([u8; 4], u32); 14] = [
        ([0, 0, 0, 0], 8),
        ([10, 0, 0, 0], 8),
        ([127, 0, 0, 0], 8),
        ([169, 254, 0, 0], 16),
        ([172, 16, 0, 0], 12),
        ([192, 0, 0, 0], 24),
        ([192, 0, 0, 170], 31),
        ([192, 0, 2, 0], 24),
        ([192, 168, 0, 0], 16),
        ([198, 18, 0, 0], 15),
        ([198, 51, 100, 0], 24),
        ([203, 0, 113, 0], 24),
        ([240, 0, 0, 0], 4),
        ([255, 255, 255, 255], 32),
    ];
    const EXCEPT: [([u8; 4], u32); 2] = [([192, 0, 0, 9], 32), ([192, 0, 0, 10], 32)];
    let private = PRIVATE.iter().any(|&(n, l)| within(a, n, l)) && !EXCEPT.iter().any(|&(n, l)| within(a, n, l));
    !within(a, [100, 64, 0, 0], 10) && !private
}

// Whether a is in 10/8, 172.16/12 or 192.168/16 (engine.LAN_NETS)
pub fn in_lan_nets(a: Ipv4Addr) -> bool {
    within(a, [10, 0, 0, 0], 8) || within(a, [172, 16, 0, 0], 12) || within(a, [192, 168, 0, 0], 16)
}
