//! Log files: Python logging's line format and line endings, and the rollover at 1 MB.

use engine::activity_log::FileLog;

#[test]
fn lines_and_rollover() {
    let d = std::env::temp_dir().join(format!("rh-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let log = FileLog::new(&d, "watch");
    log.write("PC tunnel restarted (task)");
    let text = std::fs::read_to_string(d.join("watch.log")).unwrap();
    let nl = if cfg!(windows) { "\r\n" } else { "\n" };
    let re =
        regex::Regex::new(&format!(r"^\d{{4}}-\d\d-\d\d \d\d:\d\d:\d\d  PC tunnel restarted \(task\){nl}$")).unwrap();
    assert!(re.is_match(&text), "{text:?}");
    // a line that would pass 1 MB moves the file to watch.log.1 first
    let big = "x".repeat(600_000);
    log.write(&big);
    log.write(&"y".repeat(600_000));
    log.write("after");
    let old = std::fs::read_to_string(d.join("watch.log.1")).unwrap();
    assert!(old.contains("PC tunnel restarted") && old.contains(&big));
    let new = std::fs::read_to_string(d.join("watch.log")).unwrap();
    assert!(new.ends_with(&format!("  after{nl}")) && new.contains('y') && !new.contains('x'));
    let _ = std::fs::remove_dir_all(&d);
}
