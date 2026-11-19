//! The activity log: say() goes to the window's log pane or the watcher's log file, dbg() to a test build's
//! logs/debug.log. Log files roll over at 1 MB, as Python's RotatingFileHandler did (engine.say, dbg, file_log).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

/// Test builds only: built with REALITYHOP_DEBUG set, they write logs/debug.log.
pub const DEBUG: bool = option_env!("REALITYHOP_DEBUG").is_some();
/// A log file rolls over to <name>.log.1 at this size, so each log keeps at most 2 files.
const MAX_BYTES: u64 = 1_000_000;

type Sink = Box<dyn Fn(&str) + Send + Sync>;
static SAY: RwLock<Option<Sink>> = RwLock::new(None);
static DEBUG_LOG: OnceLock<FileLog> = OnceLock::new();

// Where say() writes from now on: the window's log pane, the watcher's log file
pub fn set_say(sink: impl Fn(&str) + Send + Sync + 'static) {
    *SAY.write().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(sink));
}

// Whether something takes say() already (the app's window and log)
pub fn has_say() -> bool {
    SAY.read().unwrap_or_else(|e| e.into_inner()).is_some()
}

// One line for the person; printed when nothing else takes it
pub fn say(msg: &str) {
    match SAY.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(f) => f(msg),
        None => println!("{msg}"),
    }
}

// Test builds: open logs/debug.log in this folder for dbg()
pub fn set_debug_dir(logs: &Path) {
    if DEBUG {
        let _ = DEBUG_LOG.set(FileLog::new(logs, "debug"));
    }
}

// Test builds: details for debug.log with the thread and the calling line; release builds do nothing (engine.dbg)
#[track_caller]
pub fn dbg(msg: impl FnOnce() -> String) {
    if let (true, Some(log)) = (DEBUG, DEBUG_LOG.get()) {
        let at = std::panic::Location::caller();
        let thread = std::thread::current().name().unwrap_or("thread").to_string();
        log.write(&format!("[{thread}] {}:{}: {}", at.file(), at.line(), msg()));
    }
}

/// An activity log file in the logs folder.
#[derive(Debug)]
pub struct FileLog {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileLog {
    // logs/<name>.log
    pub fn new(logs: &Path, name: &str) -> FileLog {
        FileLog { path: logs.join(format!("{name}.log")), lock: Mutex::new(()) }
    }

    // Append one line with the local time ("2026-11-18 14:03:22  message"); a line that would pass 1 MB first moves
    // the file to <name>.log.1. A failure only costs the line.
    pub fn write(&self, msg: &str) {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let nl = if cfg!(windows) { "\r\n" } else { "\n" };
        let line = format!("{}  {}{nl}", crate::local_time::now().format("%Y-%m-%d %H:%M:%S"), msg.replace('\n', nl));
        let size = std::fs::metadata(&self.path).map_or(0, |m| m.len());
        if size > 0 && size + line.len() as u64 >= MAX_BYTES {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&self.path, &old);
        }
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
