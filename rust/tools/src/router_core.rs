//! router-core: Reality Hop's router build of sing-box, the official source with everything the router does not use
//! left out (router-core/README.md). The same source, Go version and flags give the same file, so anyone can check a
//! release.
//!   rh-tools router-core                 build every channel in version.json for linux/arm64 into out/, with manifests
//!   rh-tools router-core --verify        the same, and fail unless each SHA-256 matches version.json
//!   rh-tools router-core --newest        print the newest sing-box (stable, preview) and Go versions as JSON
//!   rh-tools router-core --update        move version.json to the newest versions, build, write the new SHA-256
//!   rh-tools router-core --pin           build the versions in version.json and write their SHA-256 (after an overlay change)
//!   rh-tools router-core --test CHANNEL  build linux/amd64 and run it with the test configs (Linux; tproxy runs through sudo)
//!   rh-tools router-core --plan          CI: print go=<version> and new=yes|no (a newer sing-box or Go than version.json)
//! Needs git and the Go version in version.json on PATH.

use crate::{TempDir, get_json, root, sha256_hex, vkey};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const UPSTREAM: &str = "https://github.com/SagerNet/sing-box.git";
const TAGS: [&str; 2] = ["with_utls", "with_clash_api"]; // uTLS for Reality, the Clash API Reality Hop reads the router through
const CMD_KEEP: [&str; 8] = [
    "main.go",
    "cmd.go",
    "cmd_check.go",
    "cmd_run.go",
    "cmd_run_userns_linux.go",
    "cmd_run_userns_other.go",
    "cmd_netns_holder.go",
    "cmd_version.go",
];
const INCLUDE_KEEP: [&str; 2] = ["clashapi.go", "clashapi_stub.go"];
// box.go: no cache file (bbolt); exact text, so a changed upstream stops the build instead of guessing
const BOX_EDITS: [(&str, &str); 2] = [
    ("\t\"github.com/sagernet/sing-box/experimental/cachefile\"\n", ""),
    (
        "\t\tcacheFile := cachefile.New(ctx, logFactory.NewLogger(\"cache-file\"), common.PtrValueOrDefault(experimentalOptions.CacheFile))\n\t\tservice.MustRegister[adapter.CacheFile](ctx, cacheFile)\n\t\tinternalServices = append(internalServices, cacheFile)\n",
        "\t\treturn nil, E.New(\"cache file not in this build\")\n",
    ),
];
const ARCHES: [(&str, &str); 1] = [("arm64", "arm64")]; // router `uname -m` aarch64 -> GOARCH
const CHANNELS: [&str; 2] = ["stable", "preview"];

fn asset(arch: &str) -> String {
    format!("rh-router-core-linux-{arch}")
}

fn here() -> Result<PathBuf, String> {
    Ok(root()?.join("router-core"))
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn read_json(p: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))
}

fn write_json(p: &Path, v: &Value, newline: bool) -> Result<(), String> {
    let mut t = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    if newline {
        t.push('\n');
    }
    std::fs::write(p, t).map_err(|e| format!("{}: {e}", p.display()))
}

// Run a command, shown first; an error when it fails
fn run(cmd: &mut Command) -> Result<(), String> {
    let shown: Vec<String> =
        std::iter::once(cmd.get_program()).chain(cmd.get_args()).map(|a| a.to_string_lossy().into_owned()).collect();
    println!("+ {}", shown.join(" "));
    let st = cmd.status().map_err(|e| format!("{}: {e}", shown[0]))?;
    if st.success() { Ok(()) } else { Err(format!("{} failed ({st})", shown[0])) }
}

fn output(cmd: &mut Command) -> Result<String, String> {
    let o = cmd.output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// The newest sing-box releases (stable; preview = the newest of all, as the PC's testing channel) and Go
fn newest() -> Result<Value, String> {
    let rels = get_json("https://api.github.com/repos/SagerNet/sing-box/releases?per_page=40")?;
    let rels: Vec<&Value> = rels.as_array().ok_or("no releases")?.iter().filter(|r| r["draft"] != true).collect();
    let tag = |r: &&Value| s(&r["tag_name"]);
    let stable = rels.iter().filter(|r| r["prerelease"] != true).max_by_key(|r| vkey(&tag(r))).ok_or("no stable")?;
    let preview = rels.iter().max_by_key(|r| vkey(&tag(r))).ok_or("no release")?;
    let go = get_json("https://go.dev/dl/?mode=json")?;
    let go = go.as_array().into_iter().flatten().find(|g| g["stable"] == true).ok_or("no stable Go")?;
    let strip_v = |t: String| t.strip_prefix('v').map(str::to_string).unwrap_or(t);
    Ok(json!({
        "go": s(&go["version"]).trim_start_matches("go"),
        "stable": strip_v(tag(stable)),
        "preview": strip_v(tag(preview)),
    }))
}

fn go_check(want: &str) -> Result<(), String> {
    let have = output(Command::new("go").args(["env", "GOVERSION"]))?;
    if have != format!("go{want}") {
        return Err(format!("Go {want} is needed (version.json), but PATH has {have}"));
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let p = e.map_err(|e| e.to_string())?.path();
        let dst = to.join(p.file_name().unwrap_or_default());
        if p.is_dir() {
            std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
            copy_tree(&p, &dst)?;
        } else {
            std::fs::copy(&p, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
    }
    Ok(())
}

// The official source at tag v<ver>, cut down to the router build; the source commit
fn prepare(ver: &str, d: &Path) -> Result<String, String> {
    run(Command::new("git")
        .args(["-c", "advice.detachedHead=false", "clone", "-q", "--depth", "1", "--branch"])
        .arg(format!("v{ver}"))
        .arg(UPSTREAM)
        .arg(d))?;
    let commit = output(Command::new("git").arg("-C").arg(d).args(["rev-parse", "HEAD"]))?;
    for (sub, keep) in [("cmd/sing-box", &CMD_KEEP[..]), ("include", &INCLUDE_KEEP[..]), ("transport/v2ray", &[][..])] {
        let dir = d.join(sub);
        for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let p = e.map_err(|e| e.to_string())?.path();
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            if name.ends_with(".go") && !keep.contains(&name.as_str()) {
                std::fs::remove_file(&p).map_err(|e| e.to_string())?;
            }
        }
    }
    copy_tree(&here()?.join("overlay"), d)?;
    let p = d.join("box.go");
    let mut t = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    for (old, new) in BOX_EDITS {
        if t.matches(old).count() != 1 {
            return Err(format!(
                "box.go in sing-box {ver} changed; update BOX_EDITS in rust/tools/src/router_core.rs:\n{old}"
            ));
        }
        t = t.replacen(old, new, 1);
    }
    std::fs::write(&p, t).map_err(|e| e.to_string())?;
    Ok(commit)
}

// Build sing-box ver for goos/goarch into out; (source commit, SHA-256)
fn build(ver: &str, goarch: &str, out: &Path) -> Result<(String, String), String> {
    let tmp = TempDir::new("rh-router-core")?;
    let src = tmp.0.join("src");
    let commit = prepare(ver, &src)?;
    // the official release flags (Makefile, release/LDFLAGS) and its non-feature tags (badlinkname, ...)
    let read = |f: &str| std::fs::read_to_string(src.join("release").join(f)).map_err(|e| format!("{f}: {e}"));
    let ld = read("LDFLAGS")?.trim().to_string();
    let others = read("DEFAULT_BUILD_TAGS_OTHERS")?;
    let extra = others.trim().split(',').filter(|t| !t.is_empty() && !t.starts_with("with_")).map(str::to_string);
    let tags: Vec<String> = TAGS.iter().map(|t| t.to_string()).chain(extra).collect();
    let out = std::path::absolute(out).map_err(|e| e.to_string())?;
    run(Command::new("go")
        .args(["build", "-trimpath", "-tags", &tags.join(",")])
        .arg("-ldflags")
        .arg(format!("-X 'github.com/sagernet/sing-box/constant.Version={ver}' {ld} -s -w -buildid="))
        .arg("-o")
        .arg(&out)
        .arg("./cmd/sing-box")
        .current_dir(&src)
        .envs([("CGO_ENABLED", "0"), ("GOOS", "linux"), ("GOARCH", goarch), ("GOTOOLCHAIN", "local")]))?;
    let b = std::fs::read(&out).map_err(|e| e.to_string())?;
    Ok((commit, sha256_hex(&b)))
}

// Every channel in v for every router arch into out/<channel>/ (the files of the rolling release
// router-core-<channel>), with a manifest each
fn build_all(v: &mut Value, verify: bool) -> Result<(), String> {
    go_check(&s(&v["go"]))?;
    let out_dir = here()?.join("out");
    let mut bad = Vec::new();
    for ch in CHANNELS {
        let dir = out_dir.join(ch);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let sb = s(&v[ch]["singbox"]);
        for (arch, goarch) in ARCHES {
            let out = dir.join(asset(arch));
            let (commit, sha) = build(&sb, goarch, &out)?;
            let size = std::fs::metadata(&out).map_or(0, |m| m.len());
            println!("{ch} {sb} {arch}: {sha} ({size} bytes)");
            let had = s(&v[ch]["sha256"][arch]);
            if verify && had != sha {
                bad.push(format!("{ch} {arch}: built {sha}, version.json has {had}"));
            }
            v[ch]["sha256"][arch] = json!(sha);
            let m = json!({"channel": ch, "singbox": sb, "go": v["go"], "source_commit": commit, "arch": arch,
                "asset": asset(arch), "sha256": sha, "size": size});
            write_json(&dir.join(format!("manifest-{arch}.json")), &m, false)?;
        }
    }
    if bad.is_empty() { Ok(()) } else { Err(format!("Not the expected build:\n{}", bad.join("\n"))) }
}

// Plain UDP DNS to 127.0.0.1:port; the IPv4 addresses in the answer
#[cfg_attr(not(unix), allow(dead_code))] // the test that uses it runs on Linux
fn dns_query(name: &str, port: u16) -> Result<Vec<String>, String> {
    let mut q: Vec<u8> = Vec::new();
    for p in name.split('.') {
        q.push(p.len() as u8);
        q.extend_from_slice(p.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    let mut msg = vec![0x52, 0x48, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    msg.extend_from_slice(&q);
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(std::time::Duration::from_secs(10))).map_err(|e| e.to_string())?;
    sock.send_to(&msg, ("127.0.0.1", port)).map_err(|e| e.to_string())?;
    let mut d = [0u8; 2048];
    let n = sock.recv(&mut d).map_err(|e| e.to_string())?;
    let d = &d[..n];
    let be16 = |i: usize| d.get(i..i + 2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]) as usize);
    let (mut i, mut ips) = (12 + q.len(), Vec::new());
    for _ in 0..be16(6) {
        // name (a 2-byte pointer), type, class, ttl, length
        let (t, len) = (be16(i + 2), be16(i + 10));
        i += 12;
        if t == 1
            && let Some(a) = d.get(i..i + 4)
        {
            ips.push(format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]));
        }
        i += len;
    }
    Ok(ips)
}

// Linux CI: the amd64 build must accept both test configs, run the router config (tproxy: root, through sudo),
// answer on its API and resolve through the DNS inbound. The test nodes are fake, so DoH goes out directly here
#[cfg(unix)]
fn test(ch: &str) -> Result<(), String> {
    use std::time::{Duration, Instant};
    let here = here()?;
    let v = read_json(&here.join("version.json"))?;
    go_check(&s(&v["go"]))?;
    let out_dir = here.join("out");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let exe = out_dir.join("test-amd64");
    build(&s(&v[ch]["singbox"]), "amd64", &exe)?;
    for cfg in ["test-router.json", "test-nodes.json"] {
        run(Command::new(&exe).args(["check", "-c"]).arg(here.join(cfg)))?;
    }
    // the node test's config has no DNS section: it must start too (check alone passed a build without local DNS)
    let mut p =
        Command::new(&exe).args(["run", "-c"]).arg(here.join("test-nodes.json")).spawn().map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs(3));
    if !matches!(p.try_wait(), Ok(None)) {
        return Err("the node test config did not start".into());
    }
    let _ = p.kill();
    let _ = p.wait();
    let mut c = read_json(&here.join("test-router.json"))?;
    if let Some(srv) = c.pointer_mut("/dns/servers/0").and_then(Value::as_object_mut) {
        srv.shift_remove("detour");
    }
    c["route"]["final"] = json!("direct");
    let cfg = out_dir.join("test-run.json");
    write_json(&cfg, &c, false)?;
    let root_user = unsafe { libc::geteuid() } == 0;
    let sudo = |cmd: &str| {
        if root_user {
            Command::new(cmd)
        } else {
            let mut c = Command::new("sudo");
            c.args(["-n", cmd]);
            c
        }
    };
    let exe_s = exe.to_string_lossy().into_owned();
    let mut p = sudo(&exe_s).args(["run", "-c"]).arg(&cfg).spawn().map_err(|e| e.to_string())?;
    let r = (|| -> Result<(), String> {
        let end = Instant::now() + Duration::from_secs(15);
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(2))).build(),
        );
        loop {
            let got = agent.get("http://127.0.0.1:9090/version").header("Authorization", "Bearer test").call();
            if let Ok(resp) = got {
                println!("API: {}", resp.into_body().read_to_string().unwrap_or_default());
                break;
            }
            if !matches!(p.try_wait(), Ok(None)) || Instant::now() > end {
                return Err("the router test config did not start".into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let ips = dns_query("example.com", 5354)?;
        println!("DNS through the hijack and Quad9 DoH: {ips:?}");
        if ips.is_empty() { Err("no DNS answer through the DNS inbound".into()) } else { Ok(()) }
    })();
    let _ = sudo("pkill").args(["-f", "test-amd64 run"]).status();
    let _ = p.wait();
    r
}

#[cfg(not(unix))]
fn test(_: &str) -> Result<(), String> {
    Err("--test runs on Linux".into())
}

pub fn main(a: &[String]) -> Result<(), String> {
    let first = a.first().map(String::as_str);
    match first {
        Some("--newest") => {
            println!("{}", newest()?);
            return Ok(());
        }
        Some("--test") => return test(a.get(1).map_or("stable", String::as_str)),
        None | Some("--verify" | "--update" | "--pin" | "--plan") => {}
        Some(x) => return Err(format!("unknown option {x}")),
    }
    let path = here()?.join("version.json");
    let mut v = read_json(&path)?;
    let is_new = |n: &Value, v: &Value| n["go"] != v["go"] || CHANNELS.iter().any(|ch| n[*ch] != v[*ch]["singbox"]);
    if first == Some("--plan") {
        let n = newest()?;
        let new = is_new(&n, &v);
        println!("go={}\nnew={}", s(if new { &n["go"] } else { &v["go"] }), if new { "yes" } else { "no" });
        return Ok(());
    }
    if first == Some("--update") {
        let n = newest()?;
        if !is_new(&n, &v) {
            println!("already newest");
            return Ok(());
        }
        v["go"] = n["go"].clone();
        for ch in CHANNELS {
            v[ch] = json!({"singbox": n[ch], "sha256": {}});
        }
    }
    build_all(&mut v, first == Some("--verify"))?;
    if matches!(first, Some("--update" | "--pin")) {
        write_json(&path, &v, true)?;
        println!("version.json written");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dns_answer_is_read() {
        // a fake server on a free port answers example.com with 93.184.215.14
        let srv = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut b = [0u8; 512];
            let (n, from) = srv.recv_from(&mut b).unwrap();
            let mut r = b[..n].to_vec();
            r[2] = 0x81;
            r[3] = 0x80;
            r[7] = 1; // one answer
            r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 215, 14]);
            srv.send_to(&r, from).unwrap();
        });
        assert_eq!(dns_query("example.com", port).unwrap(), ["93.184.215.14"]);
        h.join().unwrap();
    }

    #[test]
    fn box_edits_are_exact_lines() {
        for (old, _) in BOX_EDITS {
            assert!(old.ends_with('\n') && old.starts_with('\t'));
        }
    }
}
