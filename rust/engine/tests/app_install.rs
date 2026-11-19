//! Reality Hop's own install and update steps that can run in a temp folder: the exe swap and 1.7.x's --unpack-to.
//! cleanup, move_data, register and uninstall act on the real install folder, data folder and Apps & features entry,
//! so they run only in the VM.

use engine::app_install::{EXE_NAME, NOT_UNPACKED, UNPACKED, place, rot13, same, unpack_to};
use std::path::PathBuf;

// An empty folder of this test's own
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// The names in a folder, sorted
fn names(d: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn an_update_renames_the_running_exe_out_of_the_way() {
    let d = dir("place");
    let (new, dst) = (d.join("download.exe"), d.join(EXE_NAME));
    std::fs::write(&new, b"2.0").unwrap();
    place(&new, &dst).unwrap();
    assert_eq!(std::fs::read(&dst).unwrap(), b"2.0");
    std::fs::write(&new, b"2.1").unwrap();
    place(&new, &dst).unwrap();
    assert_eq!(std::fs::read(&dst).unwrap(), b"2.1");
    let n = names(&d);
    assert_eq!(n.len(), 3, "{n:?}");
    assert_eq!((n[0].as_str(), n[2].as_str()), (EXE_NAME, "download.exe"));
    assert!(n[1].starts_with("RealityHop.old-") && n[1].ends_with(".exe"), "{n:?}");
    assert_eq!(std::fs::read(d.join(&n[1])).unwrap(), b"2.0", "the replaced copy is kept for the next start's cleanup");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn unpack_to_answers_1_7_updater_with_its_exit_codes() {
    let d = dir("unpack");
    let src = d.join("update.exe");
    std::fs::write(&src, b"new build").unwrap();
    let target = d.join("Program Files RealityHop");
    std::fs::create_dir_all(&target).unwrap();
    assert_eq!(unpack_to(&src, &target), UNPACKED);
    assert_eq!(std::fs::read(target.join(EXE_NAME)).unwrap(), b"new build");
    assert_eq!(unpack_to(&src, &d.join("no such folder")), NOT_UNPACKED);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn paths_compare_as_windows_does() {
    if cfg!(windows) {
        assert!(same(
            std::path::Path::new(r"C:\Program Files\RealityHop\RealityHop.exe"),
            std::path::Path::new("c:/program files/realityhop/REALITYHOP.EXE")
        ));
    }
    assert!(!same(std::path::Path::new("/a/b"), std::path::Path::new("/a/c")));
}

#[test]
fn userassist_names_are_rot13() {
    assert_eq!(rot13(r"{6Q809377}\ErnyvglUbc\ErnyvglUbc.rkr"), r"{6D809377}\RealityHop\RealityHop.exe");
    assert_eq!(rot13(&rot13("Reality Hop 2.0")), "Reality Hop 2.0");
}

#[cfg(windows)]
#[test]
fn registry_value_names_and_tree_removal_on_a_test_key() {
    use engine::windows_registry::{Key, delete_tree};
    let path = format!(r"Software\RealityHop-install-test-{}", std::process::id());
    let k = Key::create(&path).unwrap();
    k.set_string(r"C:\Program Files\RealityHop\RealityHop.exe.FriendlyAppName", "Reality Hop").unwrap();
    k.set_dword("Count", 3).unwrap();
    Key::create(&format!(r"{path}\a\b")).unwrap();
    let mut v = k.value_names().unwrap();
    v.sort();
    assert_eq!(v, [r"C:\Program Files\RealityHop\RealityHop.exe.FriendlyAppName", "Count"]);
    drop(k);
    delete_tree(&path);
    assert!(Key::open(&path, false).is_err(), "the key and every key under it are gone");
}

#[test]
fn an_app_on_the_disk_image_or_in_translocation_is_misplaced() {
    use engine::macos_system::misplaced;
    let exe = |b: &str| PathBuf::from(format!("{b}/RealityHop.app/Contents/MacOS/RealityHop"));
    assert!(misplaced(&exe("/Volumes/Reality Hop")));
    assert!(misplaced(&exe("/private/var/folders/x1/T/AppTranslocation/0A1B/d")));
    assert!(!misplaced(&exe("/Applications")));
    assert!(!misplaced(&exe("/Users/me/Applications")));
    assert!(!misplaced(&PathBuf::from("/Volumes/Data/realityhop/target/release/RealityHop")));
}

#[test]
fn login_item_arguments_read_back_as_written() {
    use engine::macos::{agent_plist, agent_plist_args};
    let args = ["/Applications/R&D <x>/RealityHop.app/Contents/MacOS/RealityHop", "--watch", "--logon"];
    let plist = agent_plist("com.realityhop.watcher", &args, "/Users/me/Library/Application Support/RealityHop");
    assert_eq!(agent_plist_args(&plist), args);
    assert!(agent_plist_args("<plist><dict></dict></plist>").is_empty());
}
