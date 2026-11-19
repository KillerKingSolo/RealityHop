//! macOS: the router key script, and the update swap from a built release .dmg into a copy in a temp Applications
//! folder. The swap test runs only when RH_TEST_DMG names a built dmg (the release workflow sets it); the other checks
//! of the built app are in tools/mac-app-test.py.
#![cfg(target_os = "macos")]

use engine::app_install::{cleanup, mac_swap};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

// An empty folder of this test's own
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-mac-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn names(d: &Path) -> Vec<String> {
    std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

fn ok(c: &mut Command) -> bool {
    c.status().is_ok_and(|s| s.success())
}

#[test]
fn the_router_key_script_is_valid_shell() {
    let d = dir("key");
    let f = d.join("add-router-key.command");
    let remote = engine::windows_tasks::add_key_remote("ssh-ed25519 AAAA realityhop");
    std::fs::write(&f, engine::router_actions::key_command("/usr/bin/ssh", "192.168.8.1", &remote)).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(ok(Command::new("/bin/sh").arg("-n").arg(&f)));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_release_dmg_updates_an_app_in_place() {
    let Some(dmg) = std::env::var_os("RH_TEST_DMG") else {
        eprintln!("RH_TEST_DMG is not set: skipped");
        return;
    };
    let d = dir("update");
    let (apps, mnt) = (d.join("Applications"), d.join("mnt"));
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::create_dir_all(&mnt).unwrap();
    // the first copy, as a drag to Applications makes it
    let attach = ["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"];
    assert!(ok(Command::new("/usr/bin/hdiutil").args(attach).arg(&mnt).arg(&dmg)), "dmg mounts");
    let app = apps.join("RealityHop.app");
    let copied = ok(Command::new("/usr/bin/ditto").arg(mnt.join("RealityHop.app")).arg(&app));
    let _ = Command::new("/usr/bin/hdiutil").arg("detach").arg(&mnt).arg("-force").status();
    assert!(copied, "copy like a drag to Applications");
    let exe = app.join("Contents/MacOS/RealityHop");
    let before = std::fs::metadata(&exe).unwrap().ino();
    mac_swap(Path::new(&dmg), &exe).unwrap();
    assert_ne!(std::fs::metadata(&exe).unwrap().ino(), before, "app replaced");
    let olds: Vec<String> = names(&apps).into_iter().filter(|n| n.starts_with(".RealityHop.old-")).collect();
    assert_eq!(olds.len(), 1, "old copy kept until it has exited: {olds:?}");
    assert!(!apps.join("RealityHop.app.new").exists(), "no leftovers");
    assert!(ok(Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&app)), "signature");
    cleanup(&exe);
    assert!(apps.join(&olds[0]).exists(), "the old copy stays for 10 minutes");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    std::fs::rename(apps.join(&olds[0]), apps.join(format!(".RealityHop.old-{}.app", now - 3600))).unwrap();
    cleanup(&exe);
    assert!(!names(&apps).iter().any(|n| n.starts_with(".RealityHop.")), "old copy removed later");
    let _ = std::fs::remove_dir_all(&d);
}
