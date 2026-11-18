//! The macOS side that is plain text: login item plists (launchd), local DNS apps, the router's MAC address, and
//! the process list and port owners as ps and lsof print them.

use regex::Regex;
use std::sync::LazyLock;

/// launchd labels of the tunnel and the watcher login items.
pub const TUNNEL: &str = "com.realityhop.tunnel";
pub const WATCHER: &str = "com.realityhop.watcher";

// Text inside a plist <string> or <key>, as Python's plistlib escapes it
fn xml(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// A login item that starts args once at every login, in this user's session; the plist exactly as plistlib writes it
pub fn agent_plist(label: &str, args: &[&str], cwd: &str) -> String {
    let strings: String = args.iter().map(|a| format!("\t\t<string>{}</string>\n", xml(a))).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key>\n\t<string>{}</string>\n\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n\t<key>ProcessType</key>\n\t<string>Interactive</string>\n\t<key>ProgramArguments</key>\n\t<array>\n{strings}\t</array>\n\t<key>RunAtLoad</key>\n\t<true/>\n\t<key>WorkingDirectory</key>\n\t<string>{}</string>\n</dict>\n</plist>\n",
        xml(label),
        xml(cwd)
    )
}

static NAMESERVER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"nameserver\[\d+\]\s*:\s*(\S+)").unwrap());

// The DNS servers in `scutil --dns` output that are a local DNS app: loopback, or 192.0.2.0/24 that such apps use
pub fn dns_proxies(scutil: &str) -> Vec<String> {
    let mut found: Vec<String> = NAMESERVER
        .captures_iter(scutil)
        .map(|c| c[1].to_string())
        .filter(|a| a.starts_with("127.") || a.starts_with("192.0.2.") || a == "::1")
        .collect();
    found.sort();
    found.dedup();
    found
}

static ARP_MAC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bat ((?:[0-9a-f]{1,2}:){5}[0-9a-f]{1,2})\b").unwrap());

// The router's MAC address in macOS `arp -n` output, in the aa-bb-cc-dd-ee-ff form Windows prints (macOS drops
// leading zeros)
pub fn mac_from_arp(out: &str) -> String {
    let low = out.to_lowercase();
    ARP_MAC
        .captures(&low)
        .map_or(String::new(), |c| c[1].split(':').map(|x| format!("{x:0>2}")).collect::<Vec<_>>().join("-"))
}

// [(pid, field)] from `ps -axww -o pid=,<fmt>=`; field is the rest of the line, spaces kept (macos.ps)
pub fn ps_rows(out: &str) -> Vec<(u32, String)> {
    let strip = crate::python_compat::strip;
    crate::python_compat::splitlines(out)
        .into_iter()
        .filter_map(|l| {
            let l = strip(l);
            let (pid, rest) = l.split_once(' ').unwrap_or((l, ""));
            let ok = !pid.is_empty() && pid.bytes().all(|c| c.is_ascii_digit());
            ok.then(|| Some((pid.parse().ok()?, strip(rest).to_string()))).flatten()
        })
        .collect()
}

// [(pid, program name)] from `ps -o pid=,stat=,comm=` rows; zombies (state Z) left out. comm is the full path
// (macos.procs)
pub fn procs_from_ps(rows: &[(u32, String)]) -> Vec<(u32, String)> {
    rows.iter()
        .filter(|(_, c)| !c.starts_with('Z') && c.contains(' '))
        .filter_map(|(p, c)| {
            // c.split(None, 1)[1]: what follows the state, leading whitespace dropped
            let comm = c[c.find(crate::python_compat::is_space)?..].trim_start_matches(crate::python_compat::is_space);
            Some((*p, comm.rsplit('/').next().unwrap_or_default().to_string()))
        })
        .collect()
}

// The program name in `lsof -Fc` output: the first line that starts with c (macos.port_owner)
pub fn lsof_command(out: &str) -> Option<String> {
    crate::python_compat::splitlines(out).into_iter().find_map(|l| l.strip_prefix('c').map(str::to_string))
}
