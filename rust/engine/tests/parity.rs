//! Parity with the Python code. tools/parity-dump.py runs the Python functions and writes rust/parity/<fn>.json:
//! {"fn": name, "source": file, "cases": [{"args": [...], "out": ...}]}. Phase 1 adds one test per function.

use serde_json::Value;
use std::path::PathBuf;

// The fixtures folder, rust/parity
fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("parity")
}

// Every fixture as (file stem, parsed JSON)
fn fixtures() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir()).expect("rust/parity is missing") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            let text = std::fs::read_to_string(&p).unwrap();
            let v = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            out.push((p.file_stem().unwrap().to_string_lossy().into_owned(), v));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// The recorded cases of one Python function, for the per-function tests
#[allow(dead_code)]
fn cases(name: &str) -> Vec<(Vec<Value>, Value)> {
    let (_, v) = fixtures().into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no fixture for {name}"));
    v["cases"].as_array().unwrap().iter().map(|c| (c["args"].as_array().unwrap().clone(), c["out"].clone())).collect()
}

#[test]
fn fixtures_are_well_formed() {
    let all = fixtures();
    assert!(!all.is_empty(), "no fixtures: run python tools/parity-dump.py");
    for (stem, v) in all {
        assert_eq!(v["fn"].as_str(), Some(stem.as_str()), "{stem}: fn does not match the file name");
        assert!(v["source"].as_str().is_some_and(|s| s.starts_with("src/")), "{stem}: no source file");
        let cases = v["cases"].as_array().unwrap_or_else(|| panic!("{stem}: no cases"));
        assert!(!cases.is_empty(), "{stem}: no cases");
        for c in cases {
            assert!(c["args"].is_array() && c.get("out").is_some(), "{stem}: a case needs args and out");
        }
    }
}
