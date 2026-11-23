//! The work turn the window's actions and the watcher's checks share. A file of its own: cancel() reaches every run
//! in the process, so no other test may run next to this one.

use engine::process::{self, RunError};
use std::time::{Duration, Instant};

#[test]
fn one_holder_at_a_time_and_a_cancel_ends_with_its_holder() {
    let echo: Vec<String> = if cfg!(windows) {
        vec![process::system_tool("cmd").to_string_lossy().into_owned(), "/c".into(), "echo x".into()]
    } else {
        vec!["/bin/sh".into(), "-c".into(), "echo x".into()]
    };
    let turn = process::work();
    assert!(process::try_work().is_none(), "a second holder got the turn");
    process::cancel();
    let e = process::run(&echo, None, Duration::from_secs(5), None).unwrap_err();
    assert!(matches!(e, RunError::Cancelled), "{e}");
    drop(turn);
    assert!(!process::cancelled(), "the cancel outlived its holder");
    assert!(process::run(&echo, None, Duration::from_secs(5), None).is_ok());
    // the next holder waits until the turn is free
    let turn = process::try_work().expect("the turn is free again");
    let h = std::thread::spawn(|| {
        let start = Instant::now();
        let _t = process::work();
        start.elapsed()
    });
    std::thread::sleep(Duration::from_millis(400));
    drop(turn);
    assert!(h.join().unwrap() >= Duration::from_millis(300), "it did not wait for the holder");
}
