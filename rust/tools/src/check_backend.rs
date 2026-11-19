//! check-backend: refuses a release while the build backend is out of date. Checks the GitHub Actions in
//! .github/workflows (newest release, and the pinned commit is that tag's), the Rust toolchain in
//! rust/rust-toolchain.toml (newest stable), the exact (=) crate pins in rust/Cargo.toml and rust/*/Cargo.toml (newest
//! on crates.io), the tools the workflows install with cargo install, and that every crate in rust/Cargo.lock is at
//! the newest version its users allow (cargo update --dry-run). Runs first in the release workflow, and before a v*
//! tag is pushed (.githooks/pre-push). Exit code 1 = out of date.

use crate::{get_json, get_text, root, ver};
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

struct Report {
    problems: Vec<String>,
}

impl Report {
    fn line(&mut self, ok: bool, what: &str, detail: &str) {
        println!("{}  {what}: {detail}", if ok { "OK      " } else { "OUTDATED" });
        if !ok {
            self.problems.push(what.to_string());
        }
    }
}

fn read(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

// The workflow files, in name order
fn workflows(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = root.join(".github").join("workflows");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yml"))
        .collect();
    v.sort();
    Ok(v)
}

// Pinned to a commit, with the version it is in a comment: a moved tag cannot change what the release runs
fn actions(root: &Path, r: &mut Report) -> Result<(), String> {
    let re = Regex::new(r"uses:\s*([\w.-]+/[\w.-]+)@([\w.-]+)[ \t]*#[ \t]*(v[\w.-]+)").unwrap();
    let mut seen = HashSet::new();
    for wf in workflows(root)? {
        for c in re.captures_iter(&read(&wf)?) {
            let (repo, sha, tag) = (&c[1], &c[2], &c[3]);
            if !seen.insert((repo.to_string(), tag.to_string())) {
                continue;
            }
            let latest = get_json(&format!("https://api.github.com/repos/{repo}/releases/latest"))?;
            let latest = latest["tag_name"].as_str().unwrap_or_default().to_string();
            let real = get_json(&format!("https://api.github.com/repos/{repo}/commits/{tag}"))?;
            let real = real["sha"].as_str().unwrap_or_default();
            let moved = if real == sha { String::new() } else { format!(", but {tag} is commit {real}") };
            r.line(ver(tag) >= ver(&latest) && real == sha, repo, &format!("uses {tag}, newest {latest}{moved}"));
        }
    }
    Ok(())
}

fn toolchain(root: &Path, r: &mut Report) -> Result<(), String> {
    let f = read(&root.join("rust").join("rust-toolchain.toml"))?;
    let used = Regex::new(r#"channel\s*=\s*"([\d.]+)""#).unwrap().captures(&f).ok_or("no channel")?[1].to_string();
    let dist = get_text("https://static.rust-lang.org/dist/channel-rust-stable.toml")?;
    let re = Regex::new(r#"\[pkg\.rust\]\s*version\s*=\s*"([\d.]+)"#).unwrap();
    let stable = re.captures(&dist).ok_or("no stable version")?[1].to_string();
    r.line(
        ver(&used) >= ver(&stable),
        "Rust toolchain",
        &format!("rust-toolchain.toml uses {used}, newest stable {stable}"),
    );
    Ok(())
}

// The newest stable version of a crate on crates.io
fn newest_crate(name: &str) -> Result<String, String> {
    let v = get_json(&format!("https://crates.io/api/v1/crates/{name}"))?;
    Ok(v["crate"]["max_stable_version"].as_str().unwrap_or_default().to_string())
}

fn crates(root: &Path, r: &mut Report) -> Result<(), String> {
    let re = Regex::new(r#"(?m)^([\w-]+)\s*=\s*(?:\{[^}\n]*?version\s*=\s*)?"=([\w.-]+)""#).unwrap();
    let rust = root.join("rust");
    let mut files = vec![rust.join("Cargo.toml")];
    let mut members: Vec<PathBuf> = std::fs::read_dir(&rust)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path().join("Cargo.toml")))
        .filter(|p| p.is_file())
        .collect();
    members.sort();
    files.extend(members);
    let mut seen = HashSet::new();
    for f in files {
        for c in re.captures_iter(&read(&f)?) {
            let (name, pin) = (c[1].to_string(), c[2].to_string());
            if seen.insert(name.clone()) {
                let latest = newest_crate(&name)?;
                r.line(ver(&pin) >= ver(&latest), &name, &format!("pinned {pin}, newest {latest}"));
            }
        }
    }
    Ok(())
}

fn tools(root: &Path, r: &mut Report) -> Result<(), String> {
    let re = Regex::new(r"cargo install --locked ([\w-]+)@([\d.]+)").unwrap();
    for wf in workflows(root)? {
        let file = wf.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for c in re.captures_iter(&read(&wf)?) {
            let latest = newest_crate(&c[1])?;
            r.line(ver(&c[2]) >= ver(&latest), &c[1], &format!("{file} installs {}, newest {latest}", &c[2]));
        }
    }
    Ok(())
}

// What cargo update --dry-run --verbose would do: (moves, held back by a parent crate as (crate, available))
fn lock_changes(out: &str) -> (Vec<String>, Vec<(String, String)>) {
    let mv = Regex::new(r"(?m)^\s*(?:Updating|Adding|Removing|Downgrading)\s+(\S+ v\S+(?: -> v\S+)?)").unwrap();
    let held = Regex::new(r"(?m)^\s*Unchanged\s+(\S+ v\S+) \(available: (v[^),\s]+)").unwrap();
    (
        mv.captures_iter(out).map(|c| c[1].to_string()).collect(),
        held.captures_iter(out).map(|c| (c[1].to_string(), c[2].to_string())).collect(),
    )
}

// Every crate in Cargo.lock, direct or not, at the newest version its users allow. One held back by a parent crate's
// requirement is listed but allowed: only that parent's next release can move it
fn lockfile(root: &Path, r: &mut Report) -> Result<(), String> {
    let o = std::process::Command::new("cargo")
        .args(["update", "--dry-run", "--verbose"])
        .current_dir(root.join("rust"))
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    let err = String::from_utf8_lossy(&o.stderr);
    if !o.status.success() {
        return Err(err.trim().chars().rev().take(300).collect::<String>().chars().rev().collect());
    }
    let (moves, held) = lock_changes(&format!("{}{err}", String::from_utf8_lossy(&o.stdout)));
    for m in &moves {
        r.line(false, "Cargo.lock", &format!("{m} (run cargo update)"));
    }
    for (name, avail) in &held {
        println!("HELD      Cargo.lock: {name}, {avail} is out but a parent crate does not allow it yet");
    }
    if moves.is_empty() {
        r.line(
            true,
            "Cargo.lock",
            &format!("every crate at the newest version allowed ({} held by a parent crate)", held.len()),
        );
    }
    Ok(())
}

pub fn main() -> Result<(), String> {
    let root = root()?;
    let mut r = Report { problems: Vec::new() };
    type Check = fn(&Path, &mut Report) -> Result<(), String>;
    let checks: [(&str, Check); 5] =
        [("actions", actions), ("toolchain", toolchain), ("crates", crates), ("tools", tools), ("lockfile", lockfile)];
    for (name, check) in checks {
        if let Err(e) = check(&root, &mut r) {
            r.line(false, name, &format!("could not check ({e})"));
        }
    }
    if r.problems.is_empty() {
        println!("\nBackend up to date.");
        Ok(())
    } else {
        Err(format!("\nBackend out of date ({}). Update it before releasing.", r.problems.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_update_output_is_read() {
        let out = "    Updating crates.io index\n     Locking 1 package to latest compatible version\n    Updating itoa v1.0.10 -> v1.0.19\n   Unchanged toml v0.8.2 (available: v0.8.23)\n   Unchanged foo v1.0.0 (available: v1.2.0, requires Rust 1.101)\nwarning: not updating lockfile due to dry run\n";
        let (moves, held) = lock_changes(out);
        assert_eq!(moves, ["itoa v1.0.10 -> v1.0.19"]);
        assert_eq!(held, [("toml v0.8.2".into(), "v0.8.23".into()), ("foo v1.0.0".into(), "v1.2.0".into())]);
    }
}
