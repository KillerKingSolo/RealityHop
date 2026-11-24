//! Reading and writing the data folder's files exactly as engine.py does: JSON in Python's format, written
//! atomically (the window and the watcher both write), with the platform's line endings.

use crate::python_json::dumps;
use serde_json::{Map, Value};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How newlines go to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newlines {
    /// As Python's text mode writes them: \r\n on Windows, \n elsewhere. The data files and configs.
    Platform,
    /// Always \n: scripts that go to the router.
    Lf,
}

// text with \n turned into the platform's line ending
fn on_disk(text: &str, nl: Newlines) -> String {
    if nl == Newlines::Platform && cfg!(windows) { text.replace('\n', "\r\n") } else { text.to_string() }
}

// The temporary file next to path that a write goes through: {path}.{pid}-{thread}.tmp
fn temp_path(path: &Path) -> PathBuf {
    let mut t = path.as_os_str().to_owned();
    t.push(format!(".{}-{:?}.tmp", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
    PathBuf::from(t)
}

// Write text atomically: to a temporary file, then rename over path. Windows refuses the rename for a moment while
// the other process has the file open, so it retries for a second (engine.jsave).
pub fn write_text(path: &Path, text: &str, nl: Newlines) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_path(path);
    let wrote = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(on_disk(text, nl).as_bytes())?;
        f.sync_all()
    })();
    if let Err(e) = wrote {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    for i in 0..20 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && i < 19 => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
    unreachable!()
}

// Files in the data folder that nothing needs any more, removed at the app's start: unfinished writes (.tmp) of a
// process that was stopped, the node test's rh-test, the router key console script, and 1.x's tray-window.json.
// Only when 10 minutes old, so a write, a test or a console of a running copy is never cut off
pub fn leftovers_remove(data: &Path) {
    let names = ["rh-test.exe", "rh-test", "add-router-key.cmd", "add-router-key.command", "tray-window.json"];
    for e in std::fs::read_dir(data).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|a| a.as_secs() >= 600);
        if old && e.path().is_file() && (name.ends_with(".tmp") || names.contains(&name.as_str())) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

// Write JSON as engine.jsave does (json.dump with the given indent, atomically)
pub fn jsave(path: &Path, data: &Value, indent: Option<usize>) -> io::Result<()> {
    write_text(path, &dumps(data, indent), Newlines::Platform)
}

// A file's text as Python's text mode reads it: \r\n and \r become \n; invalid UTF-8 replaced
pub fn read_text(path: &Path) -> io::Result<String> {
    let b = std::fs::read(path)?;
    Ok(String::from_utf8_lossy(&b).replace("\r\n", "\n").replace('\r', "\n"))
}

// A JSON object from a file; empty when the file is missing, damaged or holds something else (engine.jload)
pub fn jload(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

// A JSON value from a file, with the error when it cannot be read (configs: a broken one must not pass silently)
pub fn read_json(path: &Path) -> io::Result<Value> {
    serde_json::from_slice(&std::fs::read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// A new, empty folder of its own in the system's temp folder (readable by this user only on macOS), removed with
/// everything in it when dropped (tempfile.TemporaryDirectory).
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    // Make one; the name starts with prefix
    pub fn new(prefix: &str) -> io::Result<TempDir> {
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        #[cfg(unix)]
        let b = {
            use std::os::unix::fs::DirBuilderExt;
            let mut b = std::fs::DirBuilder::new();
            b.mode(0o700);
            b
        };
        #[cfg(not(unix))]
        let b = std::fs::DirBuilder::new();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        loop {
            let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{n}", std::process::id()));
            match b.create(&p) {
                Ok(()) => return Ok(TempDir(p)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    // Where it is
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    // Remove it with everything in it
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
