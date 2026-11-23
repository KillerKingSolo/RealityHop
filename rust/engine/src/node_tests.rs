//! The node tests on this PC: the port 443 ping, the handshake test through a test copy of sing-box, the download
//! speed test, and where each node is (engine.ping443, tcp_probe, local_test, speed_test, geolocate, router_test).

use crate::activity_log::say;
use crate::paths::{EXE, Paths};
use crate::process::{run, system_tool};
use crate::processes_and_ports::{port_open, port_taken};
use crate::router::{self, Ssh};
use crate::settings::{self, Settings};
use crate::singbox_config::outbound19;
use crate::speed_test::{self, Speed};
use regex::Regex;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// CacheFly's speed test file (anycast), fetched through the node.
pub const SPEED_URL: &str = "https://cachefly.cachefly.net/100mb.test";
/// The handshake test fetches this through each node; it answers with the node's exit address.
pub const EXIT_URL: &str = "https://api.ipify.org";

static IPV4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\d+\.\d+\.\d+$").unwrap());

// time.time()
fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// A node's server address
fn server(n: &Value) -> String {
    n.get("server").and_then(Value::as_str).unwrap_or_default().to_string()
}

// dict item assignment: replace in place, or add at the end
fn set(n: &mut Value, k: &str, v: Value) {
    if let Some(m) = n.as_object_mut() {
        m.insert(k.into(), v);
    }
}

// f(0), f(1), ... f(count - 1) on up to `workers` threads at once; the results in order (ThreadPoolExecutor.map)
fn pool<T: Send>(workers: usize, count: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers.min(count) {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::SeqCst);
                    if k >= count {
                        break;
                    }
                    let r = f(k);
                    out.lock().unwrap_or_else(|e| e.into_inner())[k] = Some(r);
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().map(|r| r.expect("every item ran")).collect()
}

// TCP connect time to host:port in whole ms; None when it does not answer within timeout
pub fn connect_ms(host: &str, port: u16, timeout: Duration) -> Option<i64> {
    let addr = (host, port).to_socket_addrs().ok()?.next()?;
    let t = Instant::now();
    TcpStream::connect_timeout(&addr, timeout).ok()?;
    Some(t.elapsed().as_millis() as i64)
}

// TCP connect time to port 443 (engine.ping443)
pub fn ping443(host: &str, timeout: Duration) -> Option<i64> {
    connect_ms(host, 443, timeout)
}

// Set tcp_ms on every node (null when 443 never answered); the indexes of the ones that answered, fastest first.
// One 2 s ping for all, then the best of two more for the ones that answered (engine.tcp_probe)
pub fn tcp_probe(nodes: &mut [Value]) -> Vec<usize> {
    let servers: Vec<String> = nodes.iter().map(server).collect();
    let mut ms = pool(128, servers.len(), |i| ping443(&servers[i], Duration::from_secs(2)));
    let up: Vec<usize> = (0..ms.len()).filter(|&i| ms[i].is_some()).collect();
    let more =
        pool(128, up.len(), |k| (0..2).filter_map(|_| ping443(&servers[up[k]], Duration::from_millis(1500))).min());
    for (k, &i) in up.iter().enumerate() {
        if let (Some(a), Some(b)) = (ms[i], more[k]) {
            ms[i] = Some(a.min(b));
        }
    }
    for (n, m) in nodes.iter_mut().zip(&ms) {
        set(n, "tcp_ms", m.map_or(Value::Null, Value::from));
    }
    let mut out = up;
    out.sort_by_key(|&i| ms[i]);
    out
}

// The first port of k free loopback ports in a row for a test instance. A fixed range failed the whole test when
// another program, or a second test from the window and the watcher at once, held one of its ports (engine.free_base)
pub fn free_base(k: usize) -> Result<u16, String> {
    (23000..33000)
        .step_by(500)
        .find(|&base: &u16| !(0..k).any(|i| u16::try_from(usize::from(base) + i).map(port_taken).unwrap_or(true)))
        .ok_or_else(|| "no free local ports for the node test".into())
}

// rh-core under the name rh-test for the node tests: the tunnel checks look for rh-core by name, and stopping the
// tunnel must not stop a running test. A hard link, so the large program is on disk once; a copy where a link does
// not work. Made again whenever rh-core changes; rh-core itself when neither works (engine.test_exe)
pub fn test_exe(paths: &Paths) -> PathBuf {
    let t = paths.data.join(format!("rh-test{EXE}"));
    let Ok(b) = std::fs::metadata(&paths.core) else { return paths.core.clone() };
    let fresh = std::fs::metadata(&t).is_ok_and(|a| a.len() == b.len() && a.modified().ok() >= b.modified().ok());
    if fresh && same_file(&t, &paths.core) {
        return t;
    }
    if std::fs::remove_file(&t).is_err() && fresh {
        return t; // a current copy that runs now cannot go; it is linked next time
    }
    let _ = std::fs::create_dir_all(&paths.data);
    if std::fs::hard_link(&paths.core, &t).is_ok() || std::fs::copy(&paths.core, &t).is_ok() {
        return t;
    }
    paths.core.clone()
}

// Whether two paths are the same file on disk (hard links of one file)
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
        let id = |p: &std::path::Path| -> Option<(u32, u32, u32)> {
            let f = std::fs::File::open(p).ok()?;
            let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut i) } != 0;
            ok.then_some((i.dwVolumeSerialNumber, i.nFileIndexHigh, i.nFileIndexLow))
        };
        id(a).is_some_and(|x| id(b) == Some(x))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
}

// curl through the SOCKS port of a test instance, then the rest of the arguments
fn curl_via(port: usize, rest: &[&str]) -> Vec<OsString> {
    let mut cmd: Vec<OsString> = vec![system_tool("curl").into_os_string(), "-s".into()];
    if cfg!(windows) {
        cmd.push("--ssl-no-revoke".into()); // Schannel would ask the certificate's revocation server
    }
    cmd.extend(["--socks5-hostname".into(), format!("127.0.0.1:{port}").into()]);
    cmd.extend(rest.iter().map(OsString::from));
    cmd
}

// this-pc.json's fastest node speed of the last 24 hours as (when, Mbps); (0, 0) when there is none
fn top(paths: &Paths) -> (f64, f64) {
    let me = settings::this_pc(paths);
    let v: Vec<f64> =
        me.get("top_mbps").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_f64).collect();
    match v[..] {
        [t, m] => (t, m),
        _ => (0.0, 0.0),
    }
}

// The speed a node must reach now (engine.floor)
pub fn floor_now(paths: &Paths, min_mbps: i64, now: f64) -> i64 {
    speed_test::floor(min_mbps, Some(top(paths)), now)
}

// Keep the fastest node speed of the last 24 hours in this-pc.json, shared by the window and the watcher (engine.seen)
fn seen(paths: &Paths, m: Option<i64>, now: f64) {
    let (t, top) = top(paths);
    if let Some(m) = m.filter(|&m| m != 0)
        && (m as f64 > top || now - t > 86400.0)
    {
        let kw = json!({"top_mbps": [now, m]});
        let _ = settings::set_this_pc(paths, kw.as_object().cloned().unwrap_or_default());
    }
}

// Download speed through one test port, timed from the first byte, so the handshake and the node's latency do not
// count. At most secs; give_up stops after 2 s under a quarter of the floor. Some(0) when nothing came through the
// node; None when the test file answered with an HTTP error (engine.mbps)
pub fn mbps(port: usize, secs: u64, give_up: bool, floor: i64) -> Result<Option<i64>, String> {
    let limit = (floor.max(4) * 31250).to_string();
    let secs = secs.to_string();
    let mut rest = vec!["-o", if cfg!(windows) { "nul" } else { "/dev/null" }, "-m", &secs];
    if give_up {
        rest.extend(["--speed-time", "2", "--speed-limit", &limit]);
    }
    rest.extend(["-w", " %{http_code} %{size_download} %{time_total} %{time_starttransfer}", SPEED_URL]);
    let out = run(&curl_via(port, &rest), None, Duration::from_secs(20), None).map_err(|e| e.to_string())?;
    Ok(match speed_test::parse_mbps(&out.stdout) {
        Speed::Mbps(m) => Some(m),
        Speed::Unknown => None,
    })
}

// Download speed of the working nodes (test ports base + index). First up to 8 at once, 4 s: a node that reaches the
// floor even while sharing this PC's line passes, and one under an eighth of it is slow. Sharing is not fair (a
// nearby node takes more), so every node in between gets a test of its own, best first, until 6 pass. test(port,
// secs, give_up) runs one download test (engine.speed_test)
pub fn speed_test_with(
    paths: &Paths,
    nodes: &mut [Value],
    base: usize,
    min_mbps: i64,
    now: f64,
    test: impl Fn(usize, u64, bool) -> Result<Option<i64>, String> + Sync,
) -> Result<(), String> {
    let fl = || floor_now(paths, min_mbps, now);
    let ms = |i: usize| nodes[i].get("ms").and_then(Value::as_f64).unwrap_or(0.0);
    let mut ok: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].get("ok") == Some(&json!(true))).collect();
    ok.sort_by(|a, b| ms(*a).total_cmp(&ms(*b)));
    ok.truncate(24);
    let mut share = std::collections::HashMap::new();
    for group in ok.chunks(8) {
        let got = pool(group.len(), group.len(), |k| test(base + group[k], 4, false));
        for (&i, m) in group.iter().zip(got) {
            share.insert(i, m?);
        }
    }
    seen(paths, share.values().map(|m| m.unwrap_or(0)).max(), now);
    let (mut fast, mut alone) = (0, Vec::new());
    for &i in &ok {
        match share[&i] {
            Some(m) if m >= fl() || (m as f64) < fl() as f64 / 8.0 => {
                set(&mut nodes[i], "mbps", m.into());
                fast += usize::from(m >= fl());
            }
            _ => alone.push(i),
        }
    }
    alone.sort_by_key(|i| -share[i].unwrap_or(0));
    for i in alone {
        if fast >= 6 {
            break;
        }
        let m = test(base + i, 4, true)?;
        set(&mut nodes[i], "mbps", m.map_or(Value::Null, Value::from));
        seen(paths, m, now);
        fast += usize::from(m.unwrap_or(0) >= fl());
    }
    let each: Vec<String> = ok
        .iter()
        .map(|&i| {
            let m = nodes[i].get("mbps").and_then(Value::as_i64).map_or("?".into(), |m| m.to_string());
            format!("{} {m}", server(&nodes[i]))
        })
        .collect();
    say(&format!("speed: {} Mbps (need {})", each.join(", "), fl()));
    Ok(())
}

// speed_test_with on the real download test
pub fn speed_test(paths: &Paths, nodes: &mut [Value], base: usize, min_mbps: i64) -> Result<(), String> {
    let t = now();
    speed_test_with(paths, nodes, base, min_mbps, t, |port, secs, give_up| {
        mbps(port, secs, give_up, floor_now(paths, min_mbps, now()))
    })
}

// The test instance: killed when dropped, so an error or a cancel never leaves it running
struct Instance(std::process::Child);

impl Drop for Instance {
    // Stop it and wait for it
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// The handshake test on this PC: one sing-box instance, one port per node, 10 fetches at a time. speed: then the
// download test (engine.local_test)
pub fn local_test(paths: &Paths, nodes: &mut [Value], speed: bool) -> Result<(), String> {
    if !paths.core.exists() {
        let name = paths.core.file_name().unwrap_or_default().to_string_lossy();
        return Err(format!("{name} is missing; run the setup again (Settings > Run setup again)"));
    }
    if nodes.is_empty() {
        return Ok(());
    }
    for n in nodes.iter_mut() {
        if let Some(m) = n.as_object_mut() {
            m.shift_remove("ok");
        }
    }
    let base = usize::from(free_base(nodes.len())?);
    // macOS: PIA's kill switch would block the test connections
    let bypass = !cfg!(windows) && {
        let servers: Vec<String> = nodes.iter().map(server).collect();
        crate::pia_split_tunnel::pia_bypass(paths, &servers, true)
    };
    let r = local_test_on(paths, nodes, speed, base);
    if bypass {
        crate::pia_split_tunnel::pia_bypass(paths, &[], true);
    }
    r
}

// local_test with its ports from base
fn local_test_on(paths: &Paths, nodes: &mut [Value], speed: bool, base: usize) -> Result<(), String> {
    let d = crate::data_files::TempDir::new("rh-test").map_err(|e| e.to_string())?;
    let count = nodes.len();
    let log_path = d.path().join("log");
    let inbounds: Vec<Value> = (0..count)
        .map(|i| json!({"type": "mixed", "tag": format!("in-{i}"), "listen": "127.0.0.1", "listen_port": base + i}))
        .collect();
    let outbounds: Vec<Value> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| outbound19(&router::node_of(n), &format!("n-{i}")))
        .chain([json!({"type": "direct", "tag": "direct"})])
        .collect();
    let rules: Vec<Value> =
        (0..count).map(|i| json!({"inbound": [format!("in-{i}")], "outbound": format!("n-{i}")})).collect();
    let mut cfg = json!({"log": {"level": "error", "output": log_path.to_string_lossy()}, "inbounds": inbounds,
        "outbounds": outbounds, "route": {"rules": rules, "final": "direct", "auto_detect_interface": true}});
    if crate::pc_tunnel::store_ok(paths) {
        cfg["certificate"] = json!({"store": crate::pc_tunnel::CERT_STORE});
    }
    let cfg_path = d.path().join("c.json");
    std::fs::write(&cfg_path, crate::python_json::dumps(&cfg, None)).map_err(|e| e.to_string())?;
    let mut c = std::process::Command::new(test_exe(paths));
    c.args(["run".as_ref(), "-c".as_ref(), cfg_path.as_os_str()]);
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let r = (|| {
        let mut p = Instance(c.spawn().map_err(|_| "the local test instance did not start".to_string())?);
        // listening within a moment; a slow PC gets longer instead of failing every node
        let end = Instant::now() + Duration::from_secs(10);
        let last = u16::try_from(base + count - 1).map_err(|e| e.to_string())?;
        while !port_open(last) {
            if !matches!(p.0.try_wait(), Ok(None)) || Instant::now() > end {
                return Err("the local test instance did not start".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let got = pool(10, count, |i| {
            let out = run(
                &curl_via(base + i, &["-m", "10", "-w", " %{time_total}", EXIT_URL]),
                None,
                Duration::from_secs(20),
                None,
            )
            .map_err(|e| e.to_string())?;
            let parts: Vec<&str> = out.stdout.split(crate::python_compat::is_space).filter(|w| !w.is_empty()).collect();
            let secs = parts.get(1).and_then(|t| t.parse::<f64>().ok()).filter(|t| t.is_finite());
            Ok::<_, String>(match (parts.len(), secs) {
                (2, Some(t)) if IPV4.is_match(parts[0]) => Some((parts[0].to_string(), (t * 1000.0) as i64)),
                _ => None,
            })
        });
        let mut first_err = None;
        for (n, r) in nodes.iter_mut().zip(got) {
            match r {
                Ok(Some((exit, ms))) => {
                    set(n, "ok", json!(true));
                    set(n, "exit", json!(exit));
                    set(n, "ms", json!(ms));
                }
                Ok(None) => {
                    set(n, "ok", json!(false));
                    set(n, "exit", json!("-"));
                    set(n, "ms", Value::Null);
                }
                Err(e) => first_err = first_err.or(Some(e)),
            }
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        let min_mbps = Settings::read(paths).unwrap_or_default().min_mbps;
        if speed && min_mbps != 0 {
            speed_test(paths, nodes, base, min_mbps)?;
        }
        drop(p);
        Ok(())
    })();
    r?;
    let log = std::fs::read(&log_path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    for (i, n) in nodes.iter_mut().enumerate() {
        if n.get("ok") != Some(&json!(true)) {
            set(n, "ok", json!(false));
            set(n, "exit", json!("-"));
            set(n, "ms", Value::Null);
            set(n, "why", json!(router::why_failed(&log, &i.to_string())));
        }
        if let Some(m) = n.as_object_mut() {
            m.entry("where").or_insert("?".into());
        }
    }
    Ok(())
}

// City and ISP for each node: known ones from the vault, the rest from ip-api, asked through the PC tunnel (in
// router-only mode the router tunnels this PC instead) (engine.geolocate)
pub fn geolocate(paths: &Paths, nodes: &mut [Value]) {
    let vault = crate::vault::load(paths);
    crate::node_location::geolocate(nodes, &vault, !settings::router_only(paths), |args, body| {
        let mut cmd = vec![system_tool("curl").into_os_string()];
        cmd.extend(args.iter().map(OsString::from));
        run(&cmd, Some(body.as_bytes()), Duration::from_secs(30), None).ok().map(|o| o.stdout)
    });
}

// The handshake test where it measures the real path: on this PC with rh-core, or on the router in router-only mode
// (and when rh-core is missing). The router has little memory, so it tests 20 nodes per run. speed: also the
// download test (this PC only) (engine.node_test)
pub fn node_test(paths: &Paths, ssh: &mut impl Ssh, nodes: &mut [Value], speed: bool) -> Result<(), String> {
    if settings::router_only(paths) || !(paths.core.exists() || settings::pc_only(paths)) {
        for chunk in nodes.chunks_mut(20) {
            router::handshake(ssh, chunk).map_err(|e| e.to_string())?;
        }
        Ok(())
    } else {
        local_test(paths, nodes, speed)
    }
}

// The node test with geolocation alongside; the results go into the vault (engine.router_test)
pub fn router_test(paths: &Paths, ssh: &mut impl Ssh, nodes: &mut [Value], speed: bool) -> Result<(), String> {
    let mut geo = nodes.to_vec();
    let r = std::thread::scope(|s| {
        let g = s.spawn(|| geolocate(paths, &mut geo));
        let r = node_test(paths, ssh, nodes, speed);
        let _ = g.join();
        r
    });
    for (n, g) in nodes.iter_mut().zip(&geo) {
        for k in ["where", "isp"] {
            if let Some(v) = g.get(k) {
                set(n, k, v.clone());
            }
        }
    }
    r?;
    crate::vault::record_file(paths, nodes).map_err(|e| e.to_string())
}
