//! Running other programs (sing-box, ssh, curl, PowerShell, piactl): every run has a time limit, cancel() stops
//! whatever runs, and no console window opens (engine.run).

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Set by cancel(): every run stops and new runs refuse to start, until reset_cancel().
static CANCEL: AtomicBool = AtomicBool::new(false);

/// What a finished program printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The exit code; None when it was ended by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Why a run gave no output.
#[derive(Debug)]
pub enum RunError {
    Cancelled,
    TimedOut { program: String, secs: u64 },
    Start(std::io::Error),
}

impl std::fmt::Display for RunError {
    // The message engine.run raised
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Cancelled => write!(f, "cancelled"),
            RunError::TimedOut { program, secs } => write!(f, "{program} took longer than {secs} s and was stopped"),
            RunError::Start(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

// Stop every running program and refuse new ones (the window's Cancel button)
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

// Allow runs again after cancel()
pub fn reset_cancel() {
    CANCEL.store(false, Ordering::SeqCst);
}

// Whether cancel() is in force
pub fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

// A Windows program by its full path in System32, so a same-named file in the Downloads folder never runs
// (engine.winexe); on macOS the first of /usr/bin, /usr/sbin, /bin, /sbin that has it
pub fn system_tool(name: &str) -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        // explorer.exe is in the Windows folder, not in System32
        if name == "explorer" {
            return root.join("explorer.exe");
        }
        let rel = match name {
            "powershell" => r"WindowsPowerShell\v1.0\powershell.exe".to_string(),
            "ssh" => r"OpenSSH\ssh.exe".to_string(),
            "ssh-keygen" => r"OpenSSH\ssh-keygen.exe".to_string(),
            n => format!("{n}.exe"),
        };
        return root.join("System32").join(rel);
    }
    ["/usr/bin", "/usr/sbin", "/bin", "/sbin"]
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

// Read a pipe to the end on its own thread; the bytes arrive on the channel
fn drain(mut r: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = r.read_to_end(&mut b);
        let _ = tx.send(b);
    });
    rx
}

// Run a program and wait for it, at most timeout. stdin goes as raw bytes: text mode would turn every newline into
// CR LF, which the router's shell chokes on.
pub fn run<S: AsRef<OsStr>>(
    cmd: &[S],
    stdin: Option<&[u8]>,
    timeout: Duration,
    cwd: Option<&Path>,
) -> Result<Output, RunError> {
    if cancelled() {
        return Err(RunError::Cancelled);
    }
    let mut c = Command::new(&cmd[0]);
    c.args(&cmd[1..]).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = c.spawn().map_err(RunError::Start)?;
    let (out, err) = (drain(child.stdout.take().unwrap()), drain(child.stderr.take().unwrap()));
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let data = data.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let start = Instant::now();
    let program = || Path::new(&cmd[0]).file_name().unwrap_or_default().to_string_lossy().into_owned();
    let status = loop {
        if let Ok(Some(s)) = child.try_wait() {
            break s;
        }
        if cancelled() || start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            if cancelled() {
                return Err(RunError::Cancelled);
            }
            return Err(RunError::TimedOut { program: program(), secs: timeout.as_secs() });
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    // A program it started in the background can hold the pipes open after it exits: wait for them only within the
    // time limit (at least a second, for output still in flight)
    let text = |rx: mpsc::Receiver<Vec<u8>>| match rx
        .recv_timeout(timeout.saturating_sub(start.elapsed()).max(Duration::from_secs(1)))
    {
        Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Some(String::new()),
        Err(mpsc::RecvTimeoutError::Timeout) => None,
    };
    let (stdout, stderr) = (text(out), text(err));
    if cancelled() {
        return Err(RunError::Cancelled);
    }
    match (stdout, stderr) {
        (Some(stdout), Some(stderr)) => Ok(Output { code: status.code(), stdout, stderr }),
        _ => Err(RunError::TimedOut { program: program(), secs: timeout.as_secs() }),
    }
}
