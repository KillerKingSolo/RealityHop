//! Every window script parses: one syntax error leaves the whole window blank. Checked with node --check; skipped on
//! a machine without Node.js, except in CI, where a missing node fails.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn every_ui_script_parses() {
    let ui = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../app/ui");
    let scripts: Vec<PathBuf> = std::fs::read_dir(&ui)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "js"))
        .collect();
    assert!(scripts.len() >= 5, "the window scripts are in {}", ui.display());
    if Command::new("node").arg("--version").output().is_err() {
        assert!(std::env::var_os("CI").is_none(), "CI needs Node.js to check the window scripts");
        eprintln!("skipped: no Node.js on this machine");
        return;
    }
    let bad: Vec<String> = scripts
        .iter()
        .filter_map(|p| {
            let o = Command::new("node").arg("--check").arg(p).output().unwrap();
            (!o.status.success()).then(|| format!("{}: {}", p.display(), String::from_utf8_lossy(&o.stderr)))
        })
        .collect();
    assert!(bad.is_empty(), "scripts that do not parse:\n{}", bad.join("\n"));
}
