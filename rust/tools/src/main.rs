//! rh-tools: Reality Hop's build and release tools.
//!   rh-tools check-backend                        refuse a release while the build backend is out of date
//!   rh-tools mac-app-test <dmg> <chip> <shots>    check a built Mac disk image on a real Mac
//!   rh-tools router-core [--verify | --newest | --update | --pin | --test CHANNEL | --plan]
//!                                                 the router build of sing-box (router-core/README.md)
//! Network: GitHub, crates.io, static.rust-lang.org and go.dev, read only. GITHUB_TOKEN, when set, goes to
//! api.github.com only.

mod check_backend;
mod mac_app_test;
mod router_core;

use serde_json::Value;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage: rh-tools check-backend | mac-app-test <dmg> <chip> <shots> | router-core [--verify | --newest | --update | --pin | --test CHANNEL | --plan]";

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let r = match a.first().map(String::as_str) {
        Some("check-backend") => check_backend::main(),
        Some("mac-app-test") => mac_app_test::main(&a[1..]),
        Some("router-core") => router_core::main(&a[1..]),
        _ => Err(USAGE.into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

// The repository: the first folder up from here that holds rust/Cargo.toml
pub fn root() -> Result<PathBuf, String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    here.ancestors()
        .find(|d| d.join("rust").join("Cargo.toml").is_file())
        .map(PathBuf::from)
        .ok_or_else(|| "run rh-tools inside the Reality Hop repository".into())
}

// A web page as text
pub fn get_text(url: &str) -> Result<String, String> {
    let cfg = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60)));
    let agent = ureq::Agent::new_with_config(cfg.build());
    let mut req =
        agent.get(url).header("User-Agent", "RealityHop-tools").header("Accept", "application/vnd.github+json");
    if url.starts_with("https://api.github.com/")
        && let Ok(t) = std::env::var("GITHUB_TOKEN")
        && !t.is_empty()
    {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = req.call().map_err(|e| format!("{url}: {e}"))?;
    resp.into_body().with_config().limit(20 << 20).read_to_string().map_err(|e| format!("{url}: {e}"))
}

// A web page as JSON
pub fn get_json(url: &str) -> Result<Value, String> {
    serde_json::from_str(&get_text(url)?).map_err(|e| format!("{url}: {e}"))
}

// Sort key for sing-box versions: 1.13.0-alpha.2 < 1.13.0-beta.1 < 1.13.0-rc.1 < 1.13.0 (engine::version::vkey)
pub fn vkey(tag: &str) -> Vec<u64> {
    let re = regex::Regex::new(r"^v?(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc)\.?(\d+)?)?").unwrap();
    let Some(m) = re.captures(tag) else { return vec![0] };
    let n = |i: usize| m.get(i).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
    let rank = match m.get(4).map(|x| x.as_str()) {
        Some("alpha") => 0,
        Some("beta") => 1,
        Some("rc") => 2,
        _ => 3,
    };
    vec![n(1), n(2), n(3), rank, n(5)]
}

// The first three numbers of a version: 1.12.0 > 1.11.9
pub fn ver(s: &str) -> Vec<u64> {
    regex::Regex::new(r"\d+").unwrap().find_iter(s).take(3).filter_map(|m| m.as_str().parse().ok()).collect()
}

// SHA-256 of bytes, as hex
pub fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

// A fresh folder in the temp folder, removed when dropped
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(prefix: &str) -> Result<TempDir, String> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let p = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sing_box_versions_sort_like_the_app() {
        let mut v = ["1.13.0", "1.13.0-rc.1", "1.13.0-alpha.2", "1.12.9", "1.13.0-beta.1", "v1.13.0-alpha.10"];
        v.sort_by_key(|t| vkey(t));
        assert_eq!(v, ["1.12.9", "1.13.0-alpha.2", "v1.13.0-alpha.10", "1.13.0-beta.1", "1.13.0-rc.1", "1.13.0"]);
        assert_eq!(vkey("nightly"), vec![0]);
    }

    #[test]
    fn first_three_numbers() {
        assert!(ver("v7.0.1") > ver("v7.0.0"));
        assert_eq!(ver("1.15.0-alpha.13"), vec![1, 15, 0]);
    }
}
