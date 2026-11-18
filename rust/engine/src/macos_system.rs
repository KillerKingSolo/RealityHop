//! The macOS side that calls the system: login items on disk and signing sing-box with Reality Hop's id (macos.py).

use std::path::{Path, PathBuf};
use std::time::Duration;

/// rh-core's signing id. It starts with the app's own id (com.realityhop.app), so PIA's rule for the app covers it.
pub const CORE_ID: &str = "com.realityhop.app.core";

// ~/Library/LaunchAgents
pub fn agents_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library").join("LaunchAgents")
}

// The login item's plist file (macos.agent_path)
pub fn agent_path(label: &str) -> PathBuf {
    agents_dir().join(format!("{label}.plist"))
}

// Whether the login item exists (macos.agent_exists)
pub fn agent_exists(label: &str) -> bool {
    agent_path(label).is_file()
}

// Sign sing-box ad hoc (no Apple ID) with Reality Hop's own id. PIA's Mac split tunnel matches a program by its
// signing id, and an id that starts with an app's id counts as that app: setting RealityHop.app to Bypass VPN then
// covers rh-core and its copy rh-test too. A copy is signed and swapped in, so a copy that runs now keeps running.
// True when the id is in place; say hears why not (macos.core_sign)
pub fn core_sign(path: &Path, say: impl Fn(&str)) -> bool {
    let run = |args: &[&std::ffi::OsStr], secs| crate::process::run(args, None, Duration::from_secs(secs), None);
    let codesign = std::ffi::OsStr::new("/usr/bin/codesign");
    let shown = run(&[codesign, "-dv".as_ref(), path.as_os_str()], 30);
    if shown.is_ok_and(|r| r.stderr.contains(&format!("Identifier={CORE_ID}\n"))) {
        return true;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".sign");
    let tmp = PathBuf::from(tmp);
    if let Err(e) = std::fs::copy(path, &tmp) {
        say(&format!("rh-core: codesign failed: {e}"));
        return false;
    }
    let args = ["--force", "--sign", "-", "--identifier", CORE_ID].map(std::ffi::OsStr::new);
    let r = run(&[&[codesign], &args[..], &[tmp.as_os_str()]].concat(), 60);
    let failed = match &r {
        Ok(o) if o.code == Some(0) => None,
        Ok(o) => Some(crate::python_compat::strip(if o.stderr.is_empty() { &o.stdout } else { &o.stderr }).to_string()),
        Err(e) => Some(e.to_string()),
    };
    if let Some(why) = failed {
        let _ = std::fs::remove_file(&tmp);
        say(&format!("rh-core: codesign failed: {}", why.chars().take(300).collect::<String>()));
        return false;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        say(&format!("rh-core: codesign failed: {e}"));
        return false;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    say(&format!("{name}: signed as {CORE_ID}, so PIA's Reality Hop rule covers it"));
    true
}

// Write a login item that starts args once at every login, in this user's session. launchd reads it at the next
// login; whatever must run now is started by the caller (macos.agent_write)
pub fn agent_write(label: &str, args: &[String], cwd: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(agents_dir())?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let path = agent_path(label);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, crate::macos::agent_plist(label, &args, &cwd.to_string_lossy()))?;
    std::fs::rename(&tmp, &path)
}

// Remove the login item. Not unloaded from launchd: that would also stop what it started at this login
// (macos.agent_remove)
pub fn agent_remove(label: &str) {
    let _ = std::fs::remove_file(agent_path(label));
}

// This user's id, as launchctl's gui/<uid> domain wants it
fn uid() -> &'static str {
    static UID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    UID.get_or_init(|| {
        let r = crate::process::run(&["/usr/bin/id", "-u"], None, Duration::from_secs(15), None);
        r.map(|o| crate::python_compat::strip(&o.stdout).to_string()).unwrap_or_default()
    })
}

// launchctl's output, or None when it could not run
fn launchctl(args: &[&str]) -> Option<String> {
    let cmd: Vec<&str> = [&["/bin/launchctl"], args].concat();
    crate::process::run(&cmd, None, Duration::from_secs(15), None).ok().map(|o| o.stdout)
}

// False after agent_enable(label, false) (Stop): launchd then skips it at login, until Start (macos.agent_enabled)
pub fn agent_enabled(label: &str) -> bool {
    let re = regex::Regex::new(&format!(r#""{}"\s*=>\s*(disabled|true)"#, regex::escape(label))).unwrap();
    !launchctl(&["print-disabled", &format!("gui/{}", uid())]).is_some_and(|out| re.is_match(&out))
}

// Switch a login item on or off for the next logins; true when it took (macos.agent_enable)
pub fn agent_enable(label: &str, on: bool) -> bool {
    launchctl(&[if on { "enable" } else { "disable" }, &format!("gui/{}/{label}", uid())]);
    agent_enabled(label) == on
}

// Only this user may open the data folder: it holds the router SSH key, the node keys and the control API secret
// (macos.lockdown)
pub fn lockdown(d: &Path) {
    let _ = std::fs::create_dir_all(d);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
}
