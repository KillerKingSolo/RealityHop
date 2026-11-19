//! The process list, stopping by name and the port checks. Only this test's own listeners and its own uniquely named
//! copy of a harmless program are touched.

use engine::macos;
use engine::processes_and_ports as pp;
use std::net::TcpListener;
use std::time::{Duration, Instant};

// This test program's own file name
fn me() -> String {
    std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn this_process_is_in_the_list() {
    let id = std::process::id();
    assert!(pp::procs().iter().any(|(p, n)| *p == id && *n == me()));
    assert!(pp::pids(&me().to_uppercase()).contains(&id), "any letter case");
}

// The name a port's owner shows: macOS keeps at most 31 characters of a process name, and test programs are longer
fn owner_name() -> String {
    if cfg!(windows) { me() } else { me().chars().take(31).collect() }
}

#[test]
fn command_line_path_and_stop_of_one_process() {
    let src = if cfg!(windows) { engine::process::system_tool("PING") } else { "/bin/sleep".into() };
    let name = format!("rh-cmd-test-{}{}", std::process::id(), engine::paths::EXE);
    let copy = std::env::temp_dir().join(&name);
    std::fs::copy(&src, &copy).unwrap();
    let args: &[&str] = if cfg!(windows) { &["-n", "30", "127.0.0.1"] } else { &["30"] };
    let mut child = std::process::Command::new(&copy).args(args).stdout(std::process::Stdio::null()).spawn().unwrap();
    if cfg!(windows) {
        let c = pp::cmdline(child.id()).unwrap();
        assert!(c.contains(&name) && c.ends_with("-n 30 127.0.0.1"), "{c}");
        assert!(pp::exe_path(child.id()).unwrap().eq_ignore_ascii_case(&copy.to_string_lossy()));
        assert_eq!(pp::cmdline(0), None, "no such process");
    } else {
        assert_eq!(pp::cmdline(child.id()), None);
    }
    let start = Instant::now();
    pp::stop_pid(child.id());
    assert!(!child.wait().unwrap().success() && start.elapsed() < Duration::from_secs(10));
    let _ = std::fs::remove_file(&copy);
}

#[test]
fn ports_taken_open_and_owned() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    assert!(pp::port_taken(port) && pp::port_open(port));
    assert_eq!(pp::port_owner(port), Some(owner_name()));
    let l6 = TcpListener::bind("[::1]:0").unwrap(); // the owner is found in the IPv6 table too
    assert_eq!(pp::port_owner(l6.local_addr().unwrap().port()), Some(owner_name()));
    drop(l);
    assert!(!pp::port_taken(port) && !pp::port_open(port));
    assert_eq!(pp::port_owner(port), None);
}

#[test]
fn kill_by_name_stops_only_that_program() {
    let src = if cfg!(windows) { engine::process::system_tool("PING") } else { "/bin/sleep".into() };
    let name = format!("rh-kill-test-{}{}", std::process::id(), engine::paths::EXE);
    let copy = std::env::temp_dir().join(&name);
    std::fs::copy(&src, &copy).unwrap();
    let args: &[&str] = if cfg!(windows) { &["-n", "30", "127.0.0.1"] } else { &["30"] };
    let mut child = std::process::Command::new(&copy)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while pp::pids(&name).is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(pp::pids(&name), [child.id()]);
    pp::kill_name(&name);
    let status = child.wait().unwrap();
    assert!(!status.success() && start.elapsed() < Duration::from_secs(10));
    assert!(pp::pids(&name).is_empty());
    let _ = std::fs::remove_file(&copy);
}

#[test]
fn macos_ps_and_lsof_text() {
    let out = "    1 Ss   /sbin/launchd\n  412 S    /Applications/Reality Hop.app/Contents/MacOS/rh-core\n\
               77 Z    (sing-box)\n   x S /bin/no\n  9 R\n 13 S\u{1f}  /usr/bin/ssh\n";
    let rows = macos::ps_rows(out);
    assert_eq!(rows[1], (412, "S    /Applications/Reality Hop.app/Contents/MacOS/rh-core".to_string()));
    assert_eq!(rows.len(), 5, "the line without a number is skipped");
    let names: Vec<(u32, String)> = macos::procs_from_ps(&rows);
    let want = [(1, "launchd"), (412, "rh-core"), (13, "ssh")].map(|(p, n)| (p, n.to_string()));
    assert_eq!(names, want, "the zombie and the line without a program are left out");
    assert_eq!(macos::lsof_command("p412\ncrh-core\nf7\n"), Some("rh-core".into()));
    assert_eq!(macos::lsof_command(""), None);
}
