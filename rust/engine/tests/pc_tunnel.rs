//! The PC tunnel's parts that run without touching the running tunnel: unpacking an update with the system's tar,
//! trimming core.log and the port 2080 hint. Stop, start and restart act on every rh-core and on port 2080, so they
//! are tested in a VM only.

use engine::pc_tunnel;
use engine::process::system_tool;
use std::path::PathBuf;
use std::time::Duration;

// A fresh folder in the temp directory
fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-tunnel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn an_update_gives_up_only_the_sing_box_program() {
    let src = temp_dir("pack");
    let ver = "1.14.4";
    let (top, prog) = if cfg!(windows) {
        (format!("sing-box-{ver}-windows-amd64"), "sing-box.exe")
    } else {
        let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
        (format!("sing-box-{ver}-darwin-{arch}"), "sing-box")
    };
    std::fs::create_dir_all(src.join(&top)).unwrap();
    std::fs::write(src.join(&top).join(prog), b"the program").unwrap();
    std::fs::write(src.join(&top).join("LICENSE"), b"not wanted").unwrap();
    let name = if cfg!(windows) { format!("{top}.zip") } else { format!("{top}.tar.gz") };
    let archive = src.join(&name);
    let flags = if cfg!(windows) { "-a" } else { "-z" }; // bsdtar picks zip from the .zip name
    let cmd = [
        system_tool("tar").into_os_string(),
        flags.into(),
        "-cf".into(),
        archive.clone().into(),
        "-C".into(),
        src.clone().into(),
        top.clone().into(),
    ];
    let r = engine::process::run(&cmd, None, Duration::from_secs(60), None).unwrap();
    assert_eq!(r.code, Some(0), "{}", r.stderr);
    let out = temp_dir("unpack");
    let new = pc_tunnel::unpack(&out, &name, &std::fs::read(&archive).unwrap(), ver).unwrap();
    assert_eq!(new, out.join(format!("rh-core{}", engine::paths::EXE)));
    assert_eq!(std::fs::read(&new).unwrap(), b"the program");
    assert!(!out.join(&top).join("LICENSE").exists(), "nothing else is unpacked");
    let wrong = pc_tunnel::unpack(&temp_dir("wrong"), &name, &std::fs::read(&archive).unwrap(), "9.9.9");
    assert!(wrong.is_err());
    let _ = [src, out].map(std::fs::remove_dir_all);
}

#[test]
fn core_log_keeps_its_last_200_lines_past_256_kb() {
    let p = engine::paths::Paths::new(&temp_dir("log"));
    std::fs::create_dir_all(&p.logs).unwrap();
    let text: String = (0..3000).map(|i| format!("line {i} {}\n", "x".repeat(100))).collect();
    std::fs::write(&p.core_log, &text).unwrap();
    pc_tunnel::core_log_trim(&p);
    let kept = engine::data_files::read_text(&p.core_log).unwrap();
    assert_eq!(kept.lines().count(), 200);
    assert!(kept.starts_with("line 2800 ") && kept.ends_with(&format!("line 2999 {}\n", "x".repeat(100))));
    let small = std::fs::read(&p.core_log).unwrap();
    pc_tunnel::core_log_trim(&p); // under 256 KB: left alone
    assert_eq!(std::fs::read(&p.core_log).unwrap(), small);
}

#[test]
fn port_2080_hints() {
    assert_eq!(
        pc_tunnel::blocker_hint("nginx.exe"),
        "nginx.exe uses port 2080, so the PC tunnel cannot start. Close that program, then press Restart PC tunnel."
    );
    assert!(pc_tunnel::blocker_hint("SB-CORE.EXE").contains("old Reality Hop test build"));
}
