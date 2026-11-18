//! Downloads against a fake web server on a free loopback port. Router-only settings keep every retry off the PC
//! tunnel's port 2080, so the running tunnel is never used.

use engine::downloads::{self, GetError};
use engine::router::{Error, Ssh};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
const NODE: &str = "vless://0b5b3a8e-2b1c-4c1e-9f0a-1d2e3f4a5b6c@203.0.113.7:443?security=reality&flow=xtls-rprx-vision\
                    &sni=www.apple.com&pbk=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA#x";

/// A router that never answers.
struct NoRouter;

impl Ssh for NoRouter {
    // Every command fails as when the router is away
    fn run(&mut self, _: &str, _: Option<&str>, _: u64) -> Result<String, Error> {
        Err(Error::Failed("router unreachable over ssh".into()))
    }
}

// A data folder with router-only settings, and say() collected for the checks
fn temp_data(name: &str) -> engine::paths::Paths {
    let d = std::env::temp_dir().join(format!("rh-dl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("settings.json"), r#"{"router": 1, "pc_tunnel": 0}"#).unwrap();
    engine::activity_log::set_say(|m| SAID.lock().unwrap().push(m.to_string()));
    engine::paths::Paths::new(&d)
}

// Whether say() printed a line containing this
fn said(part: &str) -> bool {
    SAID.lock().unwrap().iter().any(|m| m.contains(part))
}

// The answer to one request: /ok echoes the Accept header, /feed is a node list, /slow sends a byte every 0.4 s
fn answer(s: &mut std::net::TcpStream, path: &str, accept: &str) {
    let head =
        |code: &str, len: usize| format!("HTTP/1.1 {code}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n");
    let mut feed = format!("junk\r\n{NODE}\r\nvless://no-reality-here\r\n").into_bytes();
    feed.extend_from_slice(b"\xff\n");
    let _ = match path {
        "/ok" => s.write_all(format!("{}{accept}", head("200 OK", accept.len())).as_bytes()),
        "/feed" => s.write_all(head("200 OK", feed.len()).as_bytes()).and_then(|_| s.write_all(&feed)),
        "/slow" => (|| {
            s.write_all(head("200 OK", 100).as_bytes())?;
            for _ in 0..100 {
                std::thread::sleep(Duration::from_millis(400));
                s.write_all(b"x")?;
            }
            Ok(())
        })(),
        _ => s.write_all(head("404 Not Found", 0).as_bytes()),
    };
}

// A fake web server; its base URL
fn server() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let (mut b, mut buf) = (Vec::new(), [0u8; 1024]);
                while !b.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => b.extend_from_slice(&buf[..n]),
                    }
                }
                let req = String::from_utf8_lossy(&b).into_owned();
                let path = req.split(' ').nth(1).unwrap_or_default().to_string();
                let accept = req.lines().find_map(|l| l.strip_prefix("accept: ").or(l.strip_prefix("Accept: ")));
                answer(&mut s, &path, accept.unwrap_or_default());
            });
        }
    });
    base
}

#[test]
fn a_download_with_its_headers() {
    let (p, base) = (temp_data("ok"), server());
    let b = downloads::gh_get(&p, &format!("{base}/ok"), 10, &[("Accept", "application/vnd.github+json")]);
    assert_eq!(b, Ok(b"application/vnd.github+json".to_vec()));
}

#[test]
fn http_errors_and_failures() {
    let (p, base) = (temp_data("err"), server());
    assert_eq!(downloads::gh_get(&p, &format!("{base}/missing"), 10, &[]), Err(GetError::Status(404)));
    let e = downloads::gh_get(&p, "http://rh-no-such-host.invalid/", 10, &[]).unwrap_err();
    assert!(matches!(e, GetError::NoName(_)), "{e:?}");
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let e = downloads::gh_get(&p, &format!("http://{closed}/"), 10, &[]).unwrap_err();
    assert!(matches!(e, GetError::Failed(_)), "{e:?}");
}

#[test]
fn a_trickling_download_is_stopped_at_six_times_the_timeout() {
    let (p, base) = (temp_data("slow"), server());
    let start = Instant::now();
    let e = downloads::gh_get(&p, &format!("{base}/slow"), 1, &[]).unwrap_err();
    assert_eq!(e, GetError::TooSlow(6));
    assert_eq!(e.to_string(), "download too slow, stopped after 6 s");
    assert!(start.elapsed() < Duration::from_secs(9));
}

#[test]
fn node_lists_then_the_saved_copy() {
    let (p, base) = (temp_data("feed"), server());
    let (good, bad) = (format!("{base}/feed"), format!("{base}/missing"));
    let none = downloads::feed_download_from(&p, &mut NoRouter, &[&bad]);
    assert_eq!(none, Err("no node list source works and there is no saved copy".into()));
    assert!(said("node list source failed (127.0.0.1): HTTP Error 404"));
    assert!(said("router download failed: router unreachable over ssh"));
    // only usable lines are kept, and saved for later
    assert_eq!(downloads::feed_download_from(&p, &mut NoRouter, &[&good]), Ok(NODE.to_string()));
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    assert_eq!(std::fs::read_to_string(&p.feed_cache).unwrap(), NODE.replace('\n', nl));
    assert_eq!(downloads::feed_download_from(&p, &mut NoRouter, &[&bad]), Ok(NODE.to_string()));
    assert!(said("every node list source failed; using the saved copy from 0 h ago"));
}
