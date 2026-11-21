//! The process runner, with harmless commands only (echo, a short wait on the loopback address).

use engine::process::{self, RunError};
use std::time::{Duration, Instant};

// A shell command line for this system
fn shell(line: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![process::system_tool("cmd").to_string_lossy().into_owned(), "/c".into(), line.into()]
    } else {
        vec!["/bin/sh".into(), "-c".into(), line.into()]
    }
}

// A command that waits about 5 seconds and does nothing else
fn wait_5s() -> Vec<String> {
    shell(if cfg!(windows) { "ping -n 6 127.0.0.1 >nul" } else { "sleep 5" })
}

#[test]
fn output_exit_code_and_stdin() {
    let r = process::run(&shell("echo hello&& exit 3"), None, Duration::from_secs(20), None).unwrap();
    assert_eq!((r.code, r.stdout.trim()), (Some(3), "hello"));
    let cat = if cfg!(windows) { shell("findstr x") } else { shell("cat") };
    let r = process::run(&cat, Some(b"a x\nb\n"), Duration::from_secs(20), None).unwrap();
    assert!(r.stdout.contains("a x"));
}

#[test]
fn time_limit_stops_the_program() {
    let start = Instant::now();
    let e = process::run(&wait_5s(), None, Duration::from_secs(1), None).unwrap_err();
    assert!(matches!(e, RunError::TimedOut { secs: 1, .. }), "{e}");
    assert!(e.to_string().ends_with("took longer than 1 s and was stopped"));
    assert!(start.elapsed() < Duration::from_secs(4));
}

#[test]
fn missing_program_is_a_start_error() {
    let e = process::run(&["rh-no-such-program-xyz"], None, Duration::from_secs(5), None).unwrap_err();
    assert!(matches!(e, RunError::Start(_)));
}

#[test]
fn system_tool_paths() {
    let p = process::system_tool("ssh");
    if cfg!(windows) {
        assert!(p.ends_with(r"System32\OpenSSH\ssh.exe"), "{}", p.display());
        assert!(process::system_tool("curl").ends_with(r"System32\curl.exe"));
        // the folder and link buttons start explorer: it must be the real file
        assert!(process::system_tool("explorer").is_file());
    }
}

#[test]
fn a_background_child_holding_the_output_cannot_outlast_the_time_limit() {
    // the shell exits at once; the wait it started keeps stdout open for about 5 seconds
    let line = if cfg!(windows) { "start /b ping -n 6 127.0.0.1" } else { "sleep 5 &" };
    let start = Instant::now();
    let e = process::run(&shell(line), None, Duration::from_secs(2), None).unwrap_err();
    assert!(matches!(e, RunError::TimedOut { secs: 2, .. }), "{e}");
    assert!(start.elapsed() < Duration::from_secs(4));
}
