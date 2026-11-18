//! What Reality Hop does to the router besides pushing nodes: Reality Hop's own sing-box build for it (offered daily,
//! installed with rollback), and this PC's SSH key on it.

use crate::activity_log::say;
use crate::data_files::{Newlines, read_json, write_text};
use crate::paths::Paths;
use crate::process::{run, system_tool};
use crate::router::{self, Ssh};
use crate::settings::{self, Settings};
use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{64}$").unwrap());
static SB_VER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+\.\d+\.\d+(-[a-z]+\.?\d*)?$").unwrap());
static PUSHED_MD5: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^pushed-md5 ([0-9a-f]{32})$").unwrap());
static PUSHED_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n?pushed-md5 \S*").unwrap());

// time.time()
fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

// Merge one field into this-pc.json
fn mark(paths: &Paths, k: &str, v: Value) {
    let mut m = Map::new();
    m.insert(k.into(), v);
    let _ = settings::set_this_pc(paths, m);
}

// The router's sing-box channel, "stable" or "preview": its own setting, or the PC's (engine.router_channel)
pub fn router_channel(paths: &Paths) -> &'static str {
    let s = Settings::read(paths).unwrap_or_default();
    let ch = if s.router_singbox.is_empty() { s.singbox } else { s.router_singbox };
    if ch == "testing" { "preview" } else { "stable" }
}

// A GitHub API answer as JSON
fn gh_json(paths: &Paths, url: &str) -> Result<Value, String> {
    let b = crate::downloads::gh_get(paths, url, 30, &[]).map_err(|e| e.to_string())?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}

// Reality Hop's router build of sing-box on a channel: (manifest, binary asset). From the repo's rolling release
// router-core-<channel>, built by GitHub Actions from router-core/. The manifest and GitHub's digest must agree; the
// version goes into a shell script, so it must look like one (engine.router_core_release)
pub fn router_core_release(paths: &Paths, channel: &str) -> Result<(Value, Value), String> {
    let rel =
        gh_json(paths, &format!("https://api.github.com/repos/{}/releases/tags/router-core-{channel}", crate::REPO))?;
    let assets: Vec<Value> = rel.get("assets").and_then(Value::as_array).cloned().unwrap_or_default();
    let named = |n: &str| assets.iter().rev().find(|a| a.get("name").and_then(Value::as_str) == Some(n)).cloned();
    let manifest = named("manifest-arm64.json").ok_or("'manifest-arm64.json'")?;
    let m = gh_json(paths, manifest["browser_download_url"].as_str().unwrap_or_default())?;
    let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default();
    let a = m.get("asset").and_then(Value::as_str).and_then(named);
    let good = a
        .as_ref()
        .is_some_and(|a| a.get("digest").and_then(Value::as_str) == Some(&format!("sha256:{}", text("sha256"))))
        && SHA.is_match(text("sha256"))
        && text("channel") == channel
        && SB_VER.is_match(text("singbox"));
    match a {
        Some(a) if good => Ok((m, a)),
        _ => Err("the router build and its manifest disagree on GitHub; not offered".into()),
    }
}

// The newest Reality Hop router build on each channel: (stable, preview); None where unknown
// (engine.router_core_versions)
pub fn router_core_versions(paths: &Paths) -> (Option<String>, Option<String>) {
    let ver = |ch| router_core_release(paths, ch).ok().and_then(|(m, _)| m["singbox"].as_str().map(str::to_string));
    (ver("stable"), ver("preview"))
}

// What the router runs, kept in this-pc.json so the window shows it before the router answers
// (engine.router_core_state)
pub fn router_core_state(paths: &Paths, ssh: &mut impl Ssh) -> Result<router::CoreState, String> {
    let st = router::router_core_state(ssh).map_err(|e| e.to_string())?;
    mark(paths, "router_core", json!({"ver": st.ver, "mark": st.mark, "arch": st.arch}));
    Ok(st)
}

// True when the daily look for a new router build is due: router features on and the tunnel pushed once
// (engine.router_core_due)
pub fn router_core_due(paths: &Paths) -> bool {
    let t = settings::this_pc(paths);
    let checked = t.get("rc_checked").and_then(Value::as_f64).unwrap_or(0.0);
    !settings::pc_only(paths) && t.get("router_md5").is_some_and(settings::py_truthy) && now() - checked > 24.0 * 3600.0
}

/// A router build to offer: what the router runs, the build's manifest and its asset.
pub type Offer = (router::CoreState, Value, Value);

// The build to offer the router when it does not run it yet. Not for a router other than arm64, and not again within
// 3 days for a build the person put off (Later) (engine.router_core_offer)
pub fn router_core_offer(paths: &Paths, ssh: &mut impl Ssh) -> Result<Option<Offer>, String> {
    mark(paths, "rc_checked", json!(now()));
    let st = router_core_state(paths, ssh)?;
    if st.ver.is_none() || st.arch.is_none() {
        return Ok(None);
    }
    let (m, a) = router_core_release(paths, router_channel(paths))?;
    let sha = m["sha256"].as_str().unwrap_or_default();
    if st.mark.as_ref().is_some_and(|k| k.get(2).map(String::as_str) == Some(sha)) {
        return Ok(None);
    }
    let later = settings::this_pc(paths).get("rc_later").cloned().unwrap_or_default();
    let at = later.get("at").and_then(Value::as_f64).unwrap_or(0.0);
    if later.get("sha").and_then(Value::as_str) == Some(sha) && now() - at < 3.0 * 86400.0 {
        return Ok(None);
    }
    Ok(Some((st, m, a)))
}

// The person put this build off (Later) (engine.router_core_later)
pub fn router_core_later(paths: &Paths, m: &Value) {
    mark(paths, "rc_later", json!({"sha": m["sha256"], "at": now()}));
}

// Download the router build, check its SHA-256, upload it into the router's RAM and swap it in; the router rolls back
// by itself when the new build does not start. (installed, the router's answer) (engine.router_core_install)
pub fn router_core_install(paths: &Paths, ssh: &mut impl Ssh, m: &Value, a: &Value) -> Result<(bool, String), String> {
    let text = |k: &str| m[k].as_str().unwrap_or_default().to_string();
    let (ver, channel, sha) = (text("singbox"), text("channel"), text("sha256"));
    let size = a["size"].as_u64().unwrap_or(0) / 1_000_000;
    say(&format!("downloading the router's sing-box {ver} ({size} MB) from github.com/{}...", crate::REPO));
    let url = a["browser_download_url"].as_str().unwrap_or_default();
    let b = crate::downloads::gh_get(paths, url, 180, &[]).map_err(|e| e.to_string())?;
    let got: String = Sha256::digest(&b).iter().map(|x| format!("{x:02x}")).collect();
    if got != sha {
        return Err("the router build does not match its checksum; not installed".into());
    }
    say("router: uploading the new sing-box into RAM (nothing changes yet)...");
    ssh.upload("cat > /tmp/rh-core.new", &b, 300).map_err(|e| e.to_string())?;
    say("router: checking the new build, then swapping it in (the tunnel drops for about 10 s)...");
    let cfg = read_json(&paths.router_cfg).map_err(|e| e.to_string())?;
    let manifest = router::Manifest { channel: &channel, singbox: &ver, sha256: &sha };
    let script = router::rc_script(&manifest, &cfg, &router::secret(&cfg));
    let out = ssh.run("sh -s", Some(&script), 240).map_err(|e| e.to_string())?;
    let out = crate::python_compat::strip(&out).to_string();
    if let Some(md5) = PUSHED_MD5.captures(&out) {
        crate::node_manager::router_synced(paths, &md5[1]);
    }
    let ok = out.contains("ROUTER BUILD: installed");
    let _ = router_core_state(paths, ssh);
    let event = format!("router sing-box {ver} ({channel}) {}", if ok { "installed" } else { "not installed" });
    crate::history::record(&paths.history, &[], Some(("restart", &event)), now());
    Ok((ok, PUSHED_LINE.replace_all(&out, "").into_owned()))
}

// An SSH key for the router, made with the system's own ssh-keygen; the public key line (engine.make_key)
pub fn make_key(paths: &Paths) -> Result<String, String> {
    if !paths.router_key.exists() {
        std::fs::create_dir_all(&paths.data).map_err(|e| e.to_string())?;
        let tool = system_tool("ssh-keygen");
        let args = [tool.as_os_str(), "-q".as_ref(), "-t".as_ref(), "ed25519".as_ref(), "-N".as_ref(), "".as_ref()];
        let args =
            [&args[..], &["-C".as_ref(), "realityhop".as_ref(), "-f".as_ref(), paths.router_key.as_os_str()]].concat();
        let r = run(&args, None, Duration::from_secs(30), None).map_err(|e| e.to_string())?;
        if r.code != Some(0) {
            let why = crate::python_compat::strip(if r.stderr.is_empty() { &r.stdout } else { &r.stderr }).to_string();
            return Err(format!("ssh-keygen failed: {}", why.chars().take(200).collect::<String>()));
        }
    }
    let mut pubkey = paths.router_key.as_os_str().to_owned();
    pubkey.push(".pub");
    let text = std::fs::read_to_string(&pubkey).map_err(|e| e.to_string())?;
    Ok(crate::python_compat::strip(&text).to_string())
}

// macOS: add-router-key.command, the Terminal script where the person types the router password into ssh
pub fn key_command(ssh: &str, host: &str, remote: &str) -> String {
    format!(
        "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\nprintf \"\\033]0;Reality Hop: add key to router\\007\"\necho \"Type the router admin password (the one for the GL.iNet web panel) and press Enter.\"\necho \"Nothing shows while you type. Reality Hop never sees it.\"\necho\n{} -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=known_hosts root@{host} {}\necho\nprintf \"Press Enter to close. \"\nread _\n",
        crate::powershell::shq(ssh),
        crate::powershell::shq(remote)
    )
}

// Open a console that adds this PC's key to the router. The person types the router password into ssh there; Reality
// Hop never sees it. It runs from the data folder, with known_hosts by name: cmd cuts a full path at an &, and ssh
// takes a quoted path with a space in it as two files (engine.add_key_console)
pub fn add_key_console(paths: &Paths, host: &str) -> Result<(), String> {
    let public = make_key(paths)?;
    let ssh = system_tool("ssh").to_string_lossy().into_owned();
    if cfg!(windows) {
        let bat = paths.data.join("add-router-key.cmd");
        let text = crate::windows_tasks::add_key_cmd(&ssh, host, &public);
        // as Python's text mode writes it: each \r\n becomes \r\r\n, which cmd reads the same
        write_text(&bat, &text, Newlines::Platform).map_err(|e| e.to_string())?;
        let mut c = std::process::Command::new(system_tool("cmd"));
        c.args(["/c", r".\add-router-key.cmd"]).current_dir(&paths.data);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
        }
        c.spawn().map_err(|e| e.to_string())?;
    } else {
        let f = paths.data.join("add-router-key.command");
        let remote = crate::windows_tasks::add_key_remote(&public);
        std::fs::write(&f, key_command(&ssh, host, &remote)).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o700));
        }
        let mut c = std::process::Command::new("/usr/bin/open");
        c.args(["-a".as_ref(), "Terminal".as_ref(), f.as_os_str()]);
        c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        c.spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Take this PC's key off the router (uninstall). Only the key Reality Hop made; other PCs keep theirs
// (engine.router_key_remove)
pub fn router_key_remove(paths: &Paths, ssh: &mut impl Ssh) -> Result<String, String> {
    if !Settings::read(paths).map_err(|e| e.to_string())?.router_key.is_empty() {
        return Ok("router: your own key file was used, so it stays on the router".into());
    }
    let mut pubkey = paths.router_key.as_os_str().to_owned();
    pubkey.push(".pub");
    let public = std::fs::read_to_string(&pubkey).map_err(|e| e.to_string())?;
    if public.split_whitespace().nth(1).is_none() {
        return Err("list index out of range".into());
    }
    let out = ssh.run(&crate::windows_tasks::remove_key_cmd(&public), None, 30).map_err(|e| e.to_string())?;
    Ok(crate::python_compat::strip(&out).to_string())
}
