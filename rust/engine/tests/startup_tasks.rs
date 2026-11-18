//! Boot and logon starts: reading schtasks' XML, the Startup shortcut script, the shortcut rename, and the registry
//! calls (on a test key of their own, removed afterward).

use engine::startup_tasks::lnk_migrate;
use engine::windows_tasks::{startup_lnk_ps, task_priority, task_state};

#[test]
fn schtasks_xml_gives_the_state_and_priority() {
    let xml = "<Settings>\r\n  <Enabled>false</Enabled>\r\n  <Priority>5</Priority>\r\n</Settings>";
    assert_eq!(task_state(Some(0), xml), Some("Disabled"));
    assert_eq!(task_state(Some(0), "<Enabled>true</Enabled>"), Some("Ready"));
    assert_eq!(task_state(Some(0), ""), Some("Ready"));
    assert_eq!(task_state(Some(1), xml), None, "no task");
    assert_eq!(task_state(None, xml), None, "schtasks did not run");
    assert_eq!(task_priority(Some(0), xml), Some(5));
    assert_eq!(task_priority(Some(0), "<Settings/>"), Some(7), "no priority set: Task Scheduler's default");
    assert_eq!(task_priority(Some(1), xml), None);
}

#[test]
fn the_startup_shortcut_script_quotes_every_part() {
    let ps = startup_lnk_ps(
        r"C:\Users\O'Brien\Startup\Reality Hop.lnk",
        &[r"C:\Program Files\RealityHop\RealityHop.exe", "--watch", "--data", r"C:\My Data", "--logon"],
        r"C:\Program Files\RealityHop",
    );
    assert_eq!(
        ps,
        r#"$s = (New-Object -ComObject WScript.Shell).CreateShortcut('C:\Users\O''Brien\Startup\Reality Hop.lnk'); $s.TargetPath = 'C:\Program Files\RealityHop\RealityHop.exe'; $s.Arguments = '--watch --data "C:\My Data" --logon'; $s.WorkingDirectory = 'C:\Program Files\RealityHop'; $s.Save()"#
    );
}

#[test]
fn the_old_shortcut_name_is_moved_or_dropped() {
    let d = std::env::temp_dir().join(format!("rh-lnk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let (old, new) = (d.join("RealityHop.lnk"), d.join("Reality Hop.lnk"));
    std::fs::write(&old, "old").unwrap();
    lnk_migrate(&new);
    assert!(!old.exists());
    assert_eq!(std::fs::read_to_string(&new).unwrap(), "old", "renamed when the new name is free");
    std::fs::write(&old, "older").unwrap();
    lnk_migrate(&new);
    assert!(!old.exists());
    assert_eq!(std::fs::read_to_string(&new).unwrap(), "old", "dropped when the new name exists");
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(windows)]
#[test]
fn registry_values_round_trip_on_a_test_key() {
    use engine::windows_registry::{Key, delete_key};
    let path = format!(r"Software\RealityHop-engine-test-{}", std::process::id());
    struct Gone<'a>(&'a str);
    impl Drop for Gone<'_> {
        fn drop(&mut self) {
            let _ = delete_key(&format!(r"{}\sub", self.0));
            let _ = delete_key(self.0);
        }
    }
    let _gone = Gone(&path);
    assert!(Key::open(&path, false).is_err());
    let k = Key::create(&path).unwrap();
    k.set_string("InstallLocation", r"C:\Program Files\Réality Hop").unwrap();
    k.set_dword("NoModify", 1).unwrap();
    Key::create(&format!(r"{path}\sub")).unwrap();
    let r = Key::open(&path, false).unwrap();
    assert_eq!(r.string("InstallLocation").unwrap(), r"C:\Program Files\Réality Hop");
    assert_eq!(r.dword("NoModify").unwrap(), 1);
    assert!(r.string("NoModify").is_err(), "a number is not text");
    assert!(r.has("NoModify") && !r.has("Missing"));
    assert_eq!(r.subkeys().unwrap(), ["sub"]);
    k.delete_value("NoModify").unwrap();
    k.delete_value("NoModify").unwrap();
    assert!(!r.has("NoModify"));
    drop((k, r));
    assert!(delete_key(&path).is_err(), "a key with a subkey stays");
    delete_key(&format!(r"{path}\sub")).unwrap();
    delete_key(&path).unwrap();
    assert!(Key::open(&path, false).is_err());
}

// Reads (never changes) this PC's boot task through schtasks
#[test]
#[ignore = "reads this PC's scheduled tasks"]
fn live_boot_task_read_only() {
    let exists = engine::pc_tunnel::boot_task_exists();
    let (state, priority) = (engine::startup_tasks::task_state(), engine::startup_tasks::boot_task_priority());
    println!("boot task: exists {exists}, state {state:?}, priority {priority:?}");
    assert_eq!(exists, state.is_some());
}
