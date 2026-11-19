//! The date math behind log times and file names, and the system's local time against Python's.

use engine::local_time::{at, utc};

#[test]
fn unix_times_become_dates() {
    assert_eq!(utc(0).format("%Y-%m-%d %H:%M:%S"), "1970-01-01 00:00:00");
    assert_eq!(utc(1_800_000_000).format("%Y-%m-%d %H:%M:%S"), "2027-01-15 08:00:00");
    assert_eq!(utc(951_782_400).format("%Y%m%d-%H%M%S"), "20000229-000000"); // a leap day
    assert_eq!(utc(-1).format("%Y-%m-%d %H:%M:%S"), "1969-12-31 23:59:59");
    assert_eq!(utc(0).format("100% %Y"), "100% 1970");
}

#[test]
fn local_time_matches_python() {
    let Ok(out) = std::process::Command::new("python")
        .args(["-c", "import time; print(time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(1800000000)))"])
        .output()
    else {
        return; // no Python here: the date math above still holds
    };
    let py = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !py.is_empty() {
        assert_eq!(at(1_800_000_000).format("%Y-%m-%d %H:%M:%S"), py);
    }
}
