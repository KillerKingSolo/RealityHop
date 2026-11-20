//! Installing and updating on Windows: the Apps & features entry, the user's SID and GitHub's release answers.

use crate::python_compat::strip;
use crate::version::vtuple;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// The release file this copy updates from on Windows.
pub const ASSET: &str = "RealityHop.exe";

// The Apps & features entry (HKCU ...\Uninstall\RealityHop) for the program in folder d, exe e; names in write order
pub fn entry(d: &str, e: &str, ver: &str) -> Vec<(&'static str, String)> {
    vec![
        ("DisplayName", crate::NAME.into()),
        ("DisplayVersion", ver.into()),
        ("Publisher", crate::NAME.into()),
        ("DisplayIcon", e.into()),
        ("InstallLocation", d.into()),
        ("UninstallString", format!("\"{e}\" --uninstall")),
        ("URLInfoAbout", format!("https://github.com/{}", crate::REPO)),
    ]
}

static SID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"S-1-[\d-]+").unwrap());

// This Windows user's SID from `whoami /user /fo csv /nh`
pub fn user_sid(whoami: &str) -> Option<String> {
    SID.find(whoami).map(|m| m.as_str().to_string())
}

/// The newest release, as the update check needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Latest {
    pub ver: String,
    pub asset: Value,
    /// The SHA-256 printed in the release notes, or "".
    pub sha: String,
    pub notes: String,
}

// The text before the first "\n---" line
fn above_rule(body: &str) -> &str {
    body.split("\n---").next().unwrap_or_default()
}

// GitHub's releases/latest answer as the update: None when it has no file named asset
pub fn latest(release: &Value, asset: &str) -> Option<Latest> {
    let a = release["assets"].as_array()?.iter().rev().find(|a| a["name"] == asset)?.clone();
    let body = release["body"].as_str().unwrap_or_default();
    let re = Regex::new(&format!(r"SHA-256 of {}[^0-9a-fA-F]*([0-9a-fA-F]{{64}})", regex::escape(asset))).unwrap();
    Some(Latest {
        ver: release["tag_name"].as_str().unwrap_or_default().trim_start_matches('v').into(),
        asset: a,
        sha: re.captures(body).map(|m| m[1].to_string()).unwrap_or_default(),
        notes: above_rule(body).chars().take(1500).collect(),
    })
}

static APP_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v\d+\.\d+\.\d+$").unwrap());

// (version, date, notes) of every app release newer than since, newest first; drafts, prereleases and the router
// builds (router-core-*) left out
pub fn changes(releases: &Value, since: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = releases
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] != true && r["prerelease"] != true)
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            APP_TAG.is_match(tag).then(|| {
                let date: String = r["published_at"].as_str().unwrap_or_default().chars().take(10).collect();
                (
                    tag.trim_start_matches('v').to_string(),
                    date,
                    strip(above_rule(r["body"].as_str().unwrap_or_default())).to_string(),
                )
            })
        })
        .filter(|c| vtuple(&c.0) > vtuple(since))
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(vtuple(&c.0)));
    out
}

static CL_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^## (\d+\.\d+\.\d+)(?: \((\d{4}-\d\d-\d\d)\))?\s*$").unwrap());

// (version, date, notes) from CHANGELOG.md for every version after since, up to and with upto, newest first. Without
// since: only upto
pub fn changelog(md: &str, since: Option<&str>, upto: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for line in md.lines() {
        if let Some(c) = CL_HEAD.captures(line) {
            out.push((c[1].to_string(), c.get(2).map_or("", |d| d.as_str()).to_string(), String::new()));
        } else if line.starts_with("## ") {
            out.push((String::new(), String::new(), String::new())); // a heading of another kind ends the last version
        } else if let Some(v) = out.last_mut() {
            v.2.push_str(line);
            v.2.push('\n');
        }
    }
    let keep = |v: &str| match since {
        Some(s) => vtuple(v) > vtuple(s) && vtuple(v) <= vtuple(upto),
        None => v == upto,
    };
    out.into_iter().filter(|c| !c.0.is_empty() && keep(&c.0)).map(|(v, d, n)| (v, d, n.trim().to_string())).collect()
}
