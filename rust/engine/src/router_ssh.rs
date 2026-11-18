//! The router over SSH: the system's OpenSSH client with the setup's key and the pinned host key, used only while this
//! PC is on its own router's network (engine.ssh, router_here).

use crate::paths::Paths;
use crate::process;
use crate::router::{Error, Ssh};
use crate::settings::{self, Settings};
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long one router_here answer is kept.
const HERE_KEEP: Duration = Duration::from_secs(30);

/// The last router_here answer: when, the answer, and the MAC address seen ("" when none).
struct Here {
    at: Instant,
    answer: Result<(), String>,
    mac: String,
}

static HERE: Mutex<Option<Here>> = Mutex::new(None);

// The cached router_here answer, usable even after a thread panicked while holding it
fn here() -> MutexGuard<'static, Option<Here>> {
    HERE.lock().unwrap_or_else(|e| e.into_inner())
}

// The router MAC address saved in this-pc.json, "" when none
fn saved_mac(paths: &Paths) -> String {
    settings::this_pc(paths).get("router_mac").and_then(Value::as_str).unwrap_or_default().to_string()
}

// ssh with the key, the pinned host key and root@host (engine.ssh_base). The known_hosts path is quoted: unquoted,
// ssh takes C:\Program Files\... as two files, saves no host key and never notices a different router.
pub fn ssh_base(paths: &Paths, s: &Settings) -> Vec<OsString> {
    let key =
        if s.router_key.is_empty() { paths.router_key.clone().into_os_string() } else { s.router_key.clone().into() };
    let mut known = OsString::from("UserKnownHostsFile=\"");
    known.push(&paths.known_hosts);
    known.push("\"");
    let mut v: Vec<OsString> = vec![process::system_tool("ssh").into(), "-i".into(), key];
    for o in ["BatchMode=yes", "ConnectTimeout=10", "IdentitiesOnly=yes", "StrictHostKeyChecking=accept-new"] {
        v.extend(["-o".into(), o.into()]);
    }
    v.extend(["-o".into(), known, format!("root@{}", s.router_host).into()]);
    v
}

// The MAC address this PC sees for the router's address: None when port 22 does not answer, "" when it answers but is
// not on this PC's own network (no ARP entry) (engine.router_mac)
pub fn router_mac(host: &str) -> Result<Option<String>, String> {
    let open = (host, 22)
        .to_socket_addrs()
        .is_ok_and(|mut a| a.any(|a| TcpStream::connect_timeout(&a, Duration::from_secs(2)).is_ok()));
    if !open {
        return Ok(None);
    }
    let flag = if cfg!(windows) { "-a" } else { "-n" };
    let cmd: [OsString; 3] = [process::system_tool("arp").into(), flag.into(), host.into()];
    let out = process::run(&cmd, None, Duration::from_secs(10), None).map_err(|e| e.to_string())?.stdout;
    Ok(Some(if cfg!(windows) { crate::router::arp_mac_windows(&out) } else { crate::macos::mac_from_arp(&out) }))
}

// Ok when this PC is on the network of its own router: the router's address answers, with the MAC address it had
// when it was first reached. Checked before every SSH command (kept 30 seconds), so nothing goes to another network's
// router and a PC away from home does not wait for timeouts. The pinned SSH host key checks it again (engine.router_here)
pub fn router_here(paths: &Paths) -> Result<(), String> {
    let mut g = here();
    if let Some(h) = g.as_ref().filter(|h| h.at.elapsed() < HERE_KEEP) {
        return h.answer.clone();
    }
    let host = Settings::read(paths).map_err(|e| e.to_string())?.router_host;
    let mac = router_mac(&host)?;
    let saved = saved_mac(paths);
    let answer = match &mac {
        None => Err(format!("not on your router's network: {host} does not answer; router steps skipped")),
        Some(m) if !m.is_empty() && !saved.is_empty() && *m != saved => Err(format!(
            "a different router answers at {host}, not the one Reality Hop was set up with; router steps skipped. On a \
             new or reset router, use Settings → Change router."
        )),
        Some(_) => Ok(()),
    };
    *g = Some(Here { at: Instant::now(), answer: answer.clone(), mac: mac.unwrap_or_default() });
    answer
}

// Setting up a router (again): forget the old router's MAC address and SSH host key, so a new router is accepted
// (engine.router_forget)
pub fn router_forget(paths: &Paths) {
    let _ = settings::set_this_pc(paths, Map::from_iter([("router_mac".to_string(), Value::from(""))]));
    *here() = None;
    if let Ok(s) = Settings::read(paths) {
        let cmd: [OsString; 5] = [
            process::system_tool("ssh-keygen").into(),
            "-R".into(),
            s.router_host.into(),
            "-f".into(),
            paths.known_hosts.clone().into(),
        ];
        let _ = process::run(&cmd, None, Duration::from_secs(20), None); // fails when there is no known_hosts yet
    }
}

/// The real router: each command goes over SSH once router_here allows it.
pub struct RouterSsh<'a> {
    pub paths: &'a Paths,
}

impl RouterSsh<'_> {
    // One command on the router with optional bytes on stdin; exit code 255 is ssh's own failure (engine.ssh)
    fn exec(&mut self, cmd: &str, stdin: Option<&[u8]>, timeout: u64) -> Result<String, Error> {
        router_here(self.paths).map_err(Error::Failed)?;
        let s = Settings::read(self.paths).map_err(|e| Error::Failed(e.to_string()))?;
        let mut argv = ssh_base(self.paths, &s);
        argv.push(cmd.into());
        let r =
            process::run(&argv, stdin, Duration::from_secs(timeout), None).map_err(|e| Error::Failed(e.to_string()))?;
        if r.code == Some(255) {
            return Err(Error::Failed(if r.stderr.contains("IDENTIFICATION HAS CHANGED") {
                format!(
                    "a different router answers at {}, not the one Reality Hop was set up with; nothing was sent. On \
                     a new or reset router, use Settings → Change router.",
                    s.router_host
                )
            } else {
                "router unreachable over ssh".into()
            }));
        }
        // the MAC address is saved only after the pinned host key and our key let us in
        let mac = here().as_ref().map(|h| h.mac.clone()).unwrap_or_default();
        if !mac.is_empty() && saved_mac(self.paths).is_empty() {
            let _ = settings::set_this_pc(self.paths, Map::from_iter([("router_mac".to_string(), Value::from(mac))]));
        }
        Ok(r.stdout)
    }
}

impl Ssh for RouterSsh<'_> {
    // One command on the router (engine.ssh)
    fn run(&mut self, cmd: &str, stdin: Option<&str>, timeout: u64) -> Result<String, Error> {
        self.exec(cmd, stdin.map(str::as_bytes), timeout)
    }

    // One command with a file on stdin
    fn upload(&mut self, cmd: &str, data: &[u8], timeout: u64) -> Result<String, Error> {
        self.exec(cmd, Some(data), timeout)
    }
}
