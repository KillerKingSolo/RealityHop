//! Install, update and uninstall of Reality Hop itself (install.py), in 2.0's form: one RealityHop.exe, no program
//! folder to unpack. Installing copies the exe into C:\Program Files\RealityHop (or the folder picked in the setup)
//! with one UAC prompt, and gives that folder to this Windows user alone (with SYSTEM and administrators): Reality Hop
//! updates itself there later without asking again, and no other account can read the keys in it. An update renames
//! the running exe out of the way (RealityHop.old-<time>.exe, removed at the next start) and puts the new one in.
//! 1.7.x updates by running the new exe with --unpack-to <folder> and reading its exit code: unpack_to answers that.
//! macOS: RealityHop.app is dragged to Applications; an update swaps in the app from the release .dmg.

use crate::activity_log::say;
use crate::paths::Paths;
use crate::powershell::psq;
use crate::process::{run, system_tool};
use crate::router::Ssh;
use crate::settings::{self, Settings};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The Apps & features entry, under HKEY_CURRENT_USER.
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\RealityHop";
/// The window's id: Windows keeps its WebView2 profile in %LOCALAPPDATA%\<id> (tauri.conf.json identifier).
pub const WEBVIEW_ID: &str = "com.realityhop.app";
/// The program file's name on Windows.
pub const EXE_NAME: &str = "RealityHop.exe";
/// --unpack-to's exit codes, as 1.7.x's updater reads them.
pub const UNPACKED: u8 = 0;
pub const NO_WRITE_RIGHT: u8 = 5;
pub const NOT_UNPACKED: u8 = 1;

static OLD_APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.RealityHop\.old-(\d+)\.app$").unwrap());

// time.time() in whole seconds
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

// A path as text
fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

// The user's local app data folder (%LOCALAPPDATA%), or the home folder
fn local() -> PathBuf {
    let v = |k| std::env::var_os(k).filter(|v| !v.is_empty());
    v("LOCALAPPDATA").or_else(|| v("USERPROFILE")).or_else(|| v("HOME")).map(PathBuf::from).unwrap_or_default()
}

// macOS: ~/Library/Application Support/RealityHop, the data folder
pub fn mac_support() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support").join(crate::APP)
}

// The data folder of 1.0.x, and of a run before the setup installed Reality Hop (engine.OLD_DATA)
pub fn old_data() -> PathBuf {
    if cfg!(windows) { local().join(crate::APP) } else { mac_support() }
}

// Where 1.0.0 and 1.0.1 installed; moved on update (engine.LEGACY_DIR)
pub fn legacy_dir() -> PathBuf {
    local().join("Programs").join(crate::APP)
}

// C:\Program Files\RealityHop (engine.DEFAULT_DIR)
pub fn default_dir() -> PathBuf {
    let v = |k| std::env::var_os(k).filter(|v| !v.is_empty());
    let pf = v("ProgramW6432")
        .or_else(|| v("ProgramFiles"))
        .map_or_else(|| PathBuf::from(r"C:\Program Files"), PathBuf::from);
    pf.join(crate::APP)
}

// The folder Reality Hop is installed in: the one the setup picked (the Apps & features entry), else the default.
// macOS: the data folder (engine.install_dir)
pub fn install_dir() -> PathBuf {
    if !cfg!(windows) {
        return mac_support();
    }
    #[cfg(windows)]
    if let Ok(loc) = crate::windows_registry::Key::open(UNINSTALL_KEY, false).and_then(|k| k.string("InstallLocation"))
        && Path::new(&loc).join(EXE_NAME).is_file()
    {
        return PathBuf::from(loc);
    }
    default_dir()
}

// The installed RealityHop.exe; macOS: this app's program (engine.INSTALLED_EXE)
pub fn installed_exe(exe: &Path) -> PathBuf {
    if cfg!(windows) { install_dir().join(EXE_NAME) } else { exe.to_path_buf() }
}

// The data folder: the install folder once Reality Hop is installed there; before that, or for an install made by
// 1.0.x that was not moved yet, %LOCALAPPDATA%\RealityHop. macOS: Application Support (engine.data_dir)
pub fn data_dir() -> PathBuf {
    if !cfg!(windows) {
        return mac_support();
    }
    let (inst, old) = (install_dir(), old_data());
    let moved = inst.join("settings.json").exists() || !old.join("settings.json").exists();
    if inst.join(EXE_NAME).is_file() && moved { inst } else { old }
}

// Whether two paths name the same file, as Windows compares them (install.same)
pub fn same(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
        let s = text(&p).replace('/', "\\");
        if cfg!(windows) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

// True when this copy runs from the install folder; on macOS always (install.installed_here)
pub fn installed_here(exe: &Path) -> bool {
    !cfg!(windows) || same(exe, &installed_exe(exe))
}

// True for a copy installed by 1.0.0 or 1.0.1 in %LOCALAPPDATA%\Programs (install.running_legacy)
pub fn running_legacy(exe: &Path) -> bool {
    cfg!(windows) && same(exe, &legacy_dir().join(EXE_NAME))
}

// True when RealityHop.exe runs from somewhere else (the Downloads folder): this run is the installer
// (install.needs_install)
pub fn needs_install(exe: &Path) -> bool {
    cfg!(windows) && !installed_here(exe) && !running_legacy(exe)
}

// The version in the Apps & features entry; macOS: this one (install.installed_version)
pub fn installed_version() -> Option<String> {
    if !cfg!(windows) {
        return Some(crate::VERSION.into());
    }
    #[cfg(windows)]
    return crate::windows_registry::Key::open(UNINSTALL_KEY, false).and_then(|k| k.string("DisplayVersion")).ok();
    #[cfg(not(windows))]
    None
}

// The Start menu shortcut
pub fn menu_lnk() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join(format!("{}.lnk", crate::NAME))
}

// The desktop icon's path; the Desktop folder may have moved, into OneDrive for example (install.desktop_lnk)
pub fn desktop_lnk() -> PathBuf {
    #[cfg(windows)]
    {
        let mut buf = [0u16; 260];
        // CSIDL_DESKTOPDIRECTORY
        let r = unsafe {
            windows_sys::Win32::UI::Shell::SHGetFolderPathW(
                std::ptr::null_mut(),
                0x10,
                std::ptr::null_mut(),
                0,
                buf.as_mut_ptr(),
            )
        };
        let end = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let desk = if r == 0 && end > 0 {
            PathBuf::from(String::from_utf16_lossy(&buf[..end]))
        } else {
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Desktop")
        };
        desk.join(format!("{}.lnk", crate::NAME))
    }
    #[cfg(not(windows))]
    PathBuf::new()
}

// Remove a folder: renamed first, so a copy still running from it keeps it until the next start
fn remove_dir(p: &Path) {
    let gone = if text(p).ends_with(".del") {
        p.to_path_buf()
    } else {
        let mut g = p.as_os_str().to_owned();
        g.push(format!(".{}.del", now()));
        PathBuf::from(g)
    };
    if gone == p || std::fs::rename(p, &gone).is_ok() {
        let _ = std::fs::remove_dir_all(&gone);
    }
}

// Left over from updates: old exes (a running exe can be renamed, not deleted), the program folders 1.x unpacked
// (app-X.Y.Z), the folder 1.0.0 and 1.0.1 installed to, and shortcuts under the name used up to 1.0.4. macOS: apps
// left by an update, once 10 minutes old (install.cleanup)
pub fn cleanup(exe: &Path) {
    if !cfg!(windows) {
        let Some(app) = crate::pia_split_tunnel::bundle_of(exe) else { return };
        let Some(d) = app.parent() else { return };
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let (name, p) = (e.file_name().to_string_lossy().into_owned(), e.path());
            let age = match OLD_APP.captures(&name) {
                Some(m) => now().saturating_sub(m[1].parse().unwrap_or(0)),
                None if name == "RealityHop.app.new" => e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map_or(0, |a| a.as_secs()),
                None => continue,
            };
            if age > 600 {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
        return;
    }
    for p in [crate::startup_tasks::startup_lnk(), menu_lnk(), desktop_lnk()] {
        crate::startup_tasks::lnk_migrate(&p);
    }
    let dir = install_dir();
    let old_exe = Regex::new(r"(?i)^RealityHop\.old-.*\.exe$").unwrap();
    let app = Regex::new(r"^app-").unwrap();
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let (name, p) = (e.file_name().to_string_lossy().into_owned(), e.path());
        if old_exe.is_match(&name) && p.is_file() {
            let _ = std::fs::remove_file(&p);
        } else if app.is_match(&name) && p.is_dir() {
            let young = name.ends_with(".part")
                && e.metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|a| a.as_secs() < 600);
            if !young {
                remove_dir(&p); // 2.0 runs from the one exe: every 1.x program folder can go
            }
        }
    }
    if installed_here(exe) && legacy_dir().is_dir() {
        let _ = std::fs::remove_dir_all(legacy_dir());
    }
}

// Put src at dst; a running copy there is renamed out of the way first (install.place)
pub fn place(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut new = dst.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    std::fs::copy(src, &new)?; // first, so a full disk leaves the current copy in place
    if dst.exists() {
        let stem = text(dst);
        let stem = stem.strip_suffix(".exe").unwrap_or(&stem);
        std::fs::rename(dst, format!("{stem}.old-{}.exe", now()))?;
    }
    std::fs::rename(&new, dst)
}

// This Windows user's SID (install.user_sid)
pub fn user_sid() -> Option<String> {
    let tool = system_tool("whoami");
    let r = run(
        &[tool.as_os_str(), "/user".as_ref(), "/fo".as_ref(), "csv".as_ref(), "/nh".as_ref()],
        None,
        Duration::from_secs(15),
        None,
    );
    crate::installer::user_sid(&r.ok()?.stdout)
}

// Run PowerShell, not elevated
fn powershell(ps: &str, secs: u64) -> Option<crate::process::Output> {
    let tool = system_tool("powershell");
    run(
        &[tool.as_os_str(), "-NoProfile".as_ref(), "-Command".as_ref(), ps.as_ref()],
        None,
        Duration::from_secs(secs),
        None,
    )
    .ok()
}

// The Start menu entry (also what Windows search finds) and the desktop icon, each as chosen in the setup or in
// Settings. macOS: the app in Applications is the entry (install.shortcuts)
pub fn shortcuts(paths: &Paths, target: &Path) {
    if !cfg!(windows) {
        return;
    }
    let s = Settings::read(paths).unwrap_or_default();
    for (on, path) in [(s.start_menu != 0, menu_lnk()), (s.desktop_icon != 0, desktop_lnk())] {
        crate::startup_tasks::lnk_migrate(&path);
        if on {
            powershell(&crate::windows_tasks::shortcut_ps(&text(&path), &text(target)), 30);
        } else if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

// The entry in Windows Settings > Apps and Control Panel > Uninstall a program (install.register)
pub fn register(d: &Path, e: &Path, ver: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let k = crate::windows_registry::Key::create(UNINSTALL_KEY)?;
        for (name, val) in crate::installer::entry(&text(d), &text(e), ver) {
            k.set_string(name, &val)?;
        }
        for name in ["NoModify", "NoRepair"] {
            k.set_dword(name, 1)?;
        }
    }
    let _ = (d, e, ver);
    Ok(())
}

// Up to 1.4.0 an update changed only the version in the entry, so an entry made by 1.0.x kept pointing at a folder
// that is gone. Rewrite it when it does not match this installed copy (install.entry_fix)
pub fn entry_fix(exe: &Path) {
    if !cfg!(windows) || !installed_here(exe) {
        return;
    }
    let (d, e) = (install_dir(), installed_exe(exe));
    #[cfg(windows)]
    if let Ok(k) = crate::windows_registry::Key::open(UNINSTALL_KEY, false)
        && crate::installer::entry(&text(&d), &text(&e), crate::VERSION)
            .iter()
            .all(|(n, v)| k.string(n).ok().as_ref() == Some(v))
    {
        return;
    }
    if register(&d, &e, crate::VERSION).is_ok() {
        say("Apps & features entry: repaired, it points at this copy again");
    }
}

// Move what 1.0.x (or the setup, before it had installed Reality Hop) kept in %LOCALAPPDATA%\RealityHop into dst, the
// install folder; the paths to use from now on. rh-core runs from there, so it is stopped; the caller starts it again
// (engine.move_data)
pub fn move_data(paths: &Paths, dst: &Path) -> Paths {
    let old = old_data();
    if !old.is_dir() || same(&old, dst) {
        return Paths::new(dst);
    }
    if crate::pc_tunnel::core_running(paths) {
        crate::pc_tunnel::core_stop(paths);
    }
    let _ = std::fs::create_dir_all(dst);
    for e in std::fs::read_dir(&old).into_iter().flatten().flatten() {
        let (s, d) = (e.path(), dst.join(e.file_name()));
        if s.is_dir() && d.is_dir() {
            let _ = copy_tree(&s, &d);
            let _ = std::fs::remove_dir_all(&s);
        } else {
            if d.is_file() {
                let _ = std::fs::remove_file(&d);
            }
            if std::fs::rename(&s, &d).is_err() {
                let _ = if s.is_dir() {
                    copy_tree(&s, &d).and_then(|_| std::fs::remove_dir_all(&s))
                } else {
                    std::fs::copy(&s, &d).map(|_| ())
                };
            }
        }
    }
    let _ = std::fs::remove_dir_all(&old);
    let moved = Paths::new(dst);
    // rh-core writes its log by full path
    if let Ok(mut c) = crate::data_files::read_json(&moved.pc_cfg)
        && c.get("log").is_some_and(Value::is_object)
    {
        c["log"]["output"] = Value::from(text(&moved.core_log));
        let _ = crate::data_files::jsave(&moved.pc_cfg, &c, Some(2));
    }
    say(&format!("data moved to {}", dst.display()));
    moved
}

// Copy a folder into another, merging (shutil.copytree dirs_exist_ok)
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_tree(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

// Stop every running watcher but this process (engine.watcher_kill)
pub fn watcher_kill() {
    let me = std::process::id();
    if cfg!(windows) {
        powershell(
            &format!(
                "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*--watch*' -and ($_.CommandLine -like '*engine.py*' -or $_.Name -eq 'RealityHop.exe') -and $_.ProcessId -ne {me} }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}"
            ),
            30,
        );
    } else {
        let rows = mac_cmdlines();
        let pids: Vec<u32> = rows
            .into_iter()
            .filter(|(p, c)| {
                *p != me
                    && c.split_whitespace().any(|w| w == "--watch")
                    && (c.contains("engine.py") || c.contains("RealityHop"))
            })
            .map(|(p, _)| p)
            .collect();
        mac_kill(&pids);
    }
    crate::watcher::kill_feeds();
}

// macOS: (process id, full command line) of every process (macos.cmdlines)
fn mac_cmdlines() -> Vec<(u32, String)> {
    let out = run(&["/bin/ps", "-axww", "-o", "pid=,command="], None, Duration::from_secs(15), None);
    crate::macos::ps_rows(&out.map(|o| o.stdout).unwrap_or_default())
}

// macOS: stop processes by id (macos.kill)
fn mac_kill(pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    let ids: Vec<String> = pids.iter().map(u32::to_string).collect();
    let args: Vec<&str> = ["/bin/kill", "-KILL"].into_iter().chain(ids.iter().map(String::as_str)).collect();
    let _ = run(&args, None, Duration::from_secs(15), None);
}

// Stop every other Reality Hop: watchers, and windows running from another copy of the exe (install.others_kill)
pub fn others_kill(exe: &Path) {
    watcher_kill();
    if cfg!(windows) {
        powershell(
            &format!(
                "Get-CimInstance Win32_Process -Filter \"Name='RealityHop.exe'\" | Where-Object {{ $_.ExecutablePath -ne {} }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}",
                psq(&text(exe))
            ),
            30,
        );
    }
}

// Stop every other Reality Hop process (a window left open, the watcher, a node test): a running copy keeps its folder
// from being removed. Their command lines (install.others_stop)
pub fn others_stop(exe: &Path) -> Vec<String> {
    let me = std::process::id();
    if !cfg!(windows) {
        let app = crate::pia_split_tunnel::bundle_of(exe).map_or("\0".to_string(), |b| text(&b));
        let rows: Vec<(u32, String)> = mac_cmdlines()
            .into_iter()
            .filter(|(p, c)| {
                let first = c.split(' ').next().unwrap_or_default();
                *p != me
                    && (c.starts_with(&format!("{app}/"))
                        || Path::new(first).file_name().is_some_and(|n| n == "rh-test")
                        || c.contains("realityhop.pyw")
                        || c.contains("engine.py --watch"))
            })
            .collect();
        mac_kill(&rows.iter().map(|(p, _)| *p).collect::<Vec<_>>());
        return rows.into_iter().filter(|(_, c)| !c.contains("rh-test")).map(|(_, c)| c).collect();
    }
    let ps = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ ($_.Name -in @('RealityHop.exe', 'rh-test.exe')) -and ($_.ProcessId -notin @({me})) }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue; if ($_.Name -eq 'RealityHop.exe') {{ $_.CommandLine }} }}"
    );
    let out = powershell(&ps, 30).map(|o| o.stdout).unwrap_or_default();
    out.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()
}

// Copy this exe into the install folder (one UAC prompt), give the folder to this Windows user alone, add the Start
// menu and Apps & features entries, move in the data kept in %LOCALAPPDATA%, and point the logon and boot starts at
// the new place. watcher(on) starts the watcher from there. The message, and the paths to use from now on
// (install.install)
pub fn install(
    paths: &Paths,
    target: Option<&Path>,
    exe: &Path,
    watcher: impl FnOnce(bool),
) -> Result<(String, Paths), String> {
    if !cfg!(windows) {
        return Ok(("nothing to install".into(), paths.clone()));
    }
    let sid = user_sid().ok_or("could not read this Windows user; not installed")?;
    let mut d = std::path::absolute(target.map_or_else(install_dir, Path::to_path_buf)).map_err(|e| e.to_string())?;
    if !d.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(crate::APP)) {
        d = d.join(crate::APP); // a parent folder was picked: Reality Hop gets its own folder in it
    }
    let e = d.join(EXE_NAME);
    let (copy, fresh) = (!same(exe, &e), !e.exists()); // the setup run again from the installed copy: nothing to copy
    others_kill(exe);
    let mut ps = format!(
        "$d = {}; $e = {}; New-Item -ItemType Directory -Force -Path $d | Out-Null; ",
        psq(&text(&d)),
        psq(&text(&e))
    );
    if copy {
        ps += &format!(
            "if (Test-Path -LiteralPath $e) {{ Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + [DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe') }}; Copy-Item -LiteralPath {} -Destination $e -Force; ",
            psq(&text(exe))
        );
    }
    ps += &crate::windows_tasks::lock_ps(&sid); // also lets Reality Hop update itself there later without a UAC prompt
    ps += "; ";
    ps += crate::windows_tasks::legacy_ps(); // a test build's sb-core would hold port 2080
    if crate::pc_tunnel::boot_task_exists() {
        let (domain, name) = crate::windows_system::user();
        let pc = d.join("PC");
        ps += "; ";
        ps += &crate::windows_tasks::boot_task_ps(
            &text(&pc.join("rh-core.exe")),
            &text(&pc.join("pc-config.json")),
            &domain,
            &name,
        );
    }
    say(&format!("installing to {}; answer the UAC prompt...", d.display()));
    if crate::windows_system::elevated(&ps) == crate::windows_system::DECLINED || !d.is_dir() {
        return Err("not installed (the UAC prompt was declined)".into());
    }
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if !e.exists() || (copy && size(&e) != size(exe)) {
        return Err("not installed (the copy failed)".into());
    }
    shortcuts(paths, &e);
    register(&d, &e, crate::VERSION).map_err(|e| e.to_string())?;
    if !running_legacy(exe) {
        let _ = std::fs::remove_dir_all(legacy_dir()); // else the new copy removes it on its first start
    }
    let paths = move_data(paths, &d);
    if fresh {
        let _ = settings::set_stopped(&paths, false); // an uninstall that kept the data left this on
    }
    if crate::startup_tasks::startup_on(&paths) {
        // the logon start now points at the install folder; starts the watcher from there
        crate::startup_tasks::startup(&paths, true, &[text(&e), "--watch".into()], watcher);
    }
    say(&format!("installed Reality Hop {} to {}", crate::VERSION, d.display()));
    Ok((format!("installed to {}", d.display()), paths))
}

// True for an install made by 1.0.x: the program is in its folder, the data still in %LOCALAPPDATA% (install.needs_move)
pub fn needs_move(paths: &Paths, exe: &Path) -> bool {
    cfg!(windows) && installed_here(exe) && same(&paths.data, &old_data())
}

// Move such an install's data into the program folder. One UAC prompt: the folder goes to this Windows user alone and
// the boot task is pointed at the new rh-core. rh-core starts again from there and the logon start follows; the caller
// starts the watcher again. stop() runs once the prompt is answered, before anything moves. The paths to use from now
// on (install.move_here)
pub fn move_here(paths: &Paths, exe: &Path, stop: impl FnOnce()) -> Result<Paths, String> {
    let sid = user_sid().ok_or("could not read this Windows user")?;
    let (d, watcher) = (install_dir(), crate::startup_tasks::startup_on(paths));
    let mut ps = format!("$d = {}; ", psq(&text(&d))) + &crate::windows_tasks::lock_ps(&sid);
    if crate::pc_tunnel::boot_task_exists() {
        let (domain, name) = crate::windows_system::user();
        let pc = d.join("PC");
        ps += "; ";
        ps += &crate::windows_tasks::boot_task_ps(
            &text(&pc.join("rh-core.exe")),
            &text(&pc.join("pc-config.json")),
            &domain,
            &name,
        );
    }
    if crate::windows_system::elevated(&ps) == crate::windows_system::DECLINED {
        return Err("the admin prompt was declined".into());
    }
    stop();
    others_kill(exe);
    let paths = move_data(paths, &d);
    if !settings::stopped(&paths) {
        crate::pc_tunnel::core_restart(&paths);
    }
    if watcher {
        crate::startup_tasks::startup(&paths, true, &[text(exe), "--watch".into()], |_| {});
    }
    Ok(paths)
}

// The release file this copy updates from: RealityHop.exe, or the .dmg for this Mac's chip
pub fn asset() -> &'static str {
    if cfg!(windows) {
        crate::installer::ASSET
    } else if cfg!(target_arch = "aarch64") {
        "RealityHop-mac-apple-silicon.dmg"
    } else {
        "RealityHop-mac-intel.dmg"
    }
}

// A GitHub API answer as JSON
fn gh_json(paths: &Paths, url: &str) -> Result<Value, String> {
    let b = crate::downloads::gh_get(paths, url, 20, &[("Accept", "application/vnd.github+json")])
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}

/// A newer release, with the notes of every version since this one.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub latest: crate::installer::Latest,
    /// (version, date, notes), newest first.
    pub changes: Vec<(String, String, String)>,
}

// The newer release, or None (install.check_update)
pub fn check_update(paths: &Paths) -> Result<Option<Update>, String> {
    let r = gh_json(paths, &format!("https://api.github.com/repos/{}/releases/latest", crate::REPO))?;
    let Some(latest) = crate::installer::latest(&r, asset()) else { return Ok(None) };
    if crate::version::vtuple(&latest.ver) <= crate::version::vtuple(crate::VERSION) {
        return Ok(None);
    }
    let all = gh_json(paths, &format!("https://api.github.com/repos/{}/releases?per_page=30", crate::REPO));
    let changes = match all {
        Ok(v) => crate::installer::changes(&v, crate::VERSION),
        Err(_) => vec![(latest.ver.clone(), String::new(), latest.notes.clone())],
    };
    Ok(Some(Update { latest, changes }))
}

// Download the release file and check its SHA-256 against the release (GitHub's asset digest, or the checksum printed
// in the release notes); the file in the data folder
fn download(paths: &Paths, rel: &crate::installer::Latest, name: &str) -> Result<PathBuf, String> {
    let a = &rel.asset;
    std::fs::create_dir_all(&paths.data).map_err(|e| e.to_string())?;
    let tmp = paths.data.join(name);
    say(&format!("downloading Reality Hop {} ({} MB)...", rel.ver, a["size"].as_u64().unwrap_or(0) / 1_000_000));
    let b = crate::downloads::gh_get(paths, a["browser_download_url"].as_str().unwrap_or_default(), 180, &[])
        .map_err(|e| e.to_string())?;
    let digest = a["digest"].as_str().unwrap_or_default();
    let want = digest.strip_prefix("sha256:").unwrap_or(digest);
    let want = if want.is_empty() { rel.sha.as_str() } else { want };
    let got: String = Sha256::digest(&b).iter().map(|x| format!("{x:02x}")).collect();
    if want.is_empty() || !got.eq_ignore_ascii_case(want) {
        return Err("the download does not match the checksum published with the release; not installed".into());
    }
    std::fs::write(&tmp, &b).map_err(|e| e.to_string())?;
    Ok(tmp)
}

// Put a checked release in where this copy runs: Windows renames the running exe out of the way and puts the new one
// in (one UAC prompt when the folder lacks the right); macOS swaps in the app from the .dmg. The caller starts again
// from the same path (install.apply_update)
pub fn apply_update(paths: &Paths, rel: &crate::installer::Latest, exe: &Path) -> Result<(), String> {
    if !cfg!(windows) {
        return mac_update(paths, rel, exe);
    }
    let tmp = download(paths, rel, "update.exe")?;
    others_stop(exe); // the caller starts them again
    let r = place(&tmp, exe);
    if let Err(e) = r {
        if e.kind() != std::io::ErrorKind::PermissionDenied {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        // a folder without the modify right: one UAC prompt instead
        crate::windows_system::elevated(&format!(
            "$e = {}; Rename-Item -LiteralPath $e -NewName ('RealityHop.old-' + [DateTimeOffset]::Now.ToUnixTimeSeconds() + '.exe'); Copy-Item -LiteralPath {} -Destination $e -Force",
            psq(&text(exe)),
            psq(&text(&tmp))
        ));
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
        if size(exe).is_none() || size(exe) != size(&tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err("the update could not be copied (UAC declined); the current version keeps running".into());
        }
    }
    let _ = std::fs::remove_file(&tmp);
    if let Some(d) = exe.parent() {
        let _ = register(d, exe, &rel.ver);
    }
    say(&format!("Reality Hop {} installed", rel.ver));
    Ok(())
}

// macOS: mount the release .dmg read-only, copy its RealityHop.app next to this one, then swap the two
// (macos.apply_update)
fn mac_update(paths: &Paths, rel: &crate::installer::Latest, exe: &Path) -> Result<(), String> {
    let app = crate::pia_split_tunnel::bundle_of(exe).ok_or("not running from RealityHop.app; not updated")?;
    let dir = app.parent().ok_or("not running from RealityHop.app; not updated")?;
    let probe = dir.join(format!(".rh-write-{}", std::process::id()));
    if std::fs::write(&probe, b"").is_err() {
        return Err(format!(
            "no write right to {}: download {} from GitHub and drag Reality Hop to Applications again",
            dir.display(),
            asset()
        ));
    }
    let _ = std::fs::remove_file(&probe);
    let tmp = download(paths, rel, "update.dmg")?;
    let r = mac_swap(&tmp, exe);
    let _ = std::fs::remove_file(&tmp);
    r?;
    say(&format!("Reality Hop {} installed", rel.ver));
    Ok(())
}

// Copy RealityHop.app out of a checked release .dmg next to the running app, then swap the two. The old one stays as
// .RealityHop.old-<time>.app until cleanup() finds it 10 minutes old
pub fn mac_swap(dmg: &Path, exe: &Path) -> Result<(), String> {
    let app = crate::pia_split_tunnel::bundle_of(exe).ok_or("not running from RealityHop.app; not updated")?;
    let dir = app.parent().ok_or("not running from RealityHop.app; not updated")?;
    let mnt = crate::data_files::TempDir::new("rh-update").map_err(|e| e.to_string())?;
    let mut new = app.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    let r = (|| -> Result<(), String> {
        let m = text(mnt.path());
        let t = text(dmg);
        let o = run(
            &["/usr/bin/hdiutil", "attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", &m, &t],
            None,
            Duration::from_secs(120),
            None,
        )
        .map_err(|e| e.to_string())?;
        if o.code != Some(0) {
            let why = if o.stderr.is_empty() { o.stdout } else { o.stderr };
            return Err(format!(
                "the update could not be opened: {}",
                crate::python_compat::strip(&why).chars().take(200).collect::<String>()
            ));
        }
        let src = mnt.path().join("RealityHop.app");
        let copied = (|| -> Result<(), String> {
            if !src.join("Contents/MacOS/RealityHop").is_file() {
                return Err("the update holds no RealityHop.app; not installed".into());
            }
            let _ = std::fs::remove_dir_all(&new);
            let o = run(
                &[Path::new("/usr/bin/ditto").as_os_str(), src.as_os_str(), new.as_os_str()],
                None,
                Duration::from_secs(300),
                None,
            )
            .map_err(|e| e.to_string())?;
            if o.code != Some(0) || !new.join("Contents/MacOS/RealityHop").is_file() {
                let _ = std::fs::remove_dir_all(&new);
                let why = if o.stderr.is_empty() { o.stdout } else { o.stderr };
                return Err(format!(
                    "the update could not be copied: {}",
                    crate::python_compat::strip(&why).chars().take(200).collect::<String>()
                ));
            }
            Ok(())
        })();
        let _ = run(&["/usr/bin/hdiutil", "detach", &m, "-force"], None, Duration::from_secs(60), None);
        copied
    })();
    r?;
    others_stop(exe);
    let old = dir.join(format!(".RealityHop.old-{}.app", now()));
    std::fs::rename(&app, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&new, &app) {
        let _ = std::fs::rename(&old, &app); // put the running version back
        return Err(e.to_string());
    }
    Ok(())
}

// --unpack-to <folder>, as 1.7.x's updater runs a downloaded RealityHop.exe: this exe goes into the folder as
// RealityHop.exe (the running one is renamed out of the way). 0 done, 5 no write right, 1 failed
pub fn unpack_to(exe: &Path, dir: &Path) -> u8 {
    match place(exe, &dir.join(EXE_NAME)) {
        Ok(()) => UNPACKED,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => NO_WRITE_RIGHT,
        Err(_) => NOT_UNPACKED,
    }
}

// What Windows keeps about Reality Hop in this user's registry: its tray icon entry, the Start menu tile cache, the
// program name cache and the usage count. Windows' background activity record (HKLM) needs SYSTEM rights; it stays
// (install.traces_remove)
pub fn traces_remove() {
    #[cfg(windows)]
    {
        use crate::windows_registry::{Key, delete_tree};
        let tail = format!("\\{0}\\{0}.exe", crate::APP).to_lowercase();
        let app = crate::APP.to_lowercase();
        let keys = |p: &str| Key::open(p, false).and_then(|k| k.subkeys()).unwrap_or_default();
        let base = r"Control Panel\NotifyIconSettings";
        for s in keys(base) {
            let p = format!(r"{base}\{s}");
            if Key::open(&p, false)
                .and_then(|k| k.string("ExecutablePath"))
                .is_ok_and(|v| v.to_lowercase().ends_with(&tail))
            {
                delete_tree(&p);
            }
        }
        let base = r"Software\Microsoft\Windows\CurrentVersion\Start\TileProperties";
        for s in keys(base) {
            for t in keys(&format!(r"{base}\{s}")) {
                if t.to_lowercase() == app {
                    delete_tree(&format!(r"{base}\{s}\{t}"));
                }
            }
        }
        let rm_values = |p: &str, matches: &dyn Fn(&str) -> bool| {
            if let Ok(k) = Key::open(p, true) {
                for n in k.value_names().unwrap_or_default().into_iter().filter(|n| matches(n)) {
                    let _ = k.delete_value(&n);
                }
            }
        };
        rm_values(r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache", &|n| {
            n.to_lowercase().contains(&tail)
        });
        let base = r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist";
        for g in keys(base) {
            rm_values(&format!(r"{base}\{g}\Count"), &|n| rot13(n).to_lowercase().contains(&app));
        }
    }
}

// ROT13, which UserAssist's value names are written in
pub fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => c,
        })
        .collect()
}

// Remove Reality Hop from this PC: tunnel, watcher, boot start, shortcuts, Apps & features entry, the program and
// (unless keep_data) the data folder. router_too switches the router tunnel off first. The folders go once this
// process has exited. Lines for the person (install.uninstall)
pub fn uninstall(paths: &Paths, ssh: &mut impl Ssh, keep_data: bool, router_too: bool, exe: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let _ = settings::set_stopped(paths, true); // nothing may restart the tunnel while it is being removed
    if router_too && !settings::pc_only(paths) {
        let r = (|| -> Result<(), String> {
            out.push(crate::router::router_off(ssh).map_err(|e| e.to_string())?);
            if !keep_data {
                out.push(crate::router_actions::router_key_remove(paths, ssh)?); // its private half is about to go
            }
            Ok(())
        })();
        if let Err(e) = r {
            out.push(format!("router: could not switch it off ({e}); do it later in the GL.iNet panel"));
        }
    }
    crate::startup_tasks::startup(paths, false, &[], |_| watcher_kill());
    others_stop(exe);
    crate::pc_tunnel::core_stop(paths);
    out.push("PC tunnel, background watcher and open windows: stopped".into());
    let me = settings::this_pc(paths);
    if ["pia_ips", "pia_app", "pia_apps"].iter().any(|k| me.get(*k).is_some_and(settings::py_truthy)) {
        out.push(if crate::pia_split_tunnel::pia_bypass(paths, &[], false) {
            "PIA split tunnel: Reality Hop's rules removed".into()
        } else {
            let what = if cfg!(windows) { "rh-core.exe and rh-test.exe" } else { "Reality Hop and its node addresses" };
            format!("PIA split tunnel: remove {what} by hand (Settings → Split Tunnel)")
        });
    }
    let proxy_off =
        "VPN app: turn its SOCKS5 proxy off (PIA: Settings > Multi-Hop, untick \"Multi-hop and Obfuscation\")";
    if !cfg!(windows) {
        crate::startup_tasks::boot_task(paths, false);
        out.push("login start: removed".into());
        out.push(proxy_off.into());
        let app = crate::pia_split_tunnel::bundle_of(exe);
        let mut gone: Vec<PathBuf> = app.iter().cloned().collect();
        if !keep_data {
            gone.push(paths.data.clone());
        }
        remove_later(&gone);
        let what = if app.is_some() { "the app" } else { "the program (not in an app: left in place)" };
        out.push(format!(
            "{what}{}: removed when you close this message",
            if keep_data { "" } else { " and data folder" }
        ));
        return out;
    }
    if crate::pc_tunnel::boot_task_exists() {
        out.push(if crate::startup_tasks::boot_task(paths, false) {
            "boot start: removed".into()
        } else {
            format!(
                "boot start: NOT removed (UAC declined); remove the task \"{}\" in Task Scheduler",
                crate::pc_tunnel::BOOT_TASK
            )
        });
    }
    for f in [menu_lnk(), crate::startup_tasks::startup_lnk(), desktop_lnk()] {
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_file(f.with_file_name(format!("{}.lnk", crate::APP))); // the name used up to 1.0.4
    }
    #[cfg(windows)]
    let _ = crate::windows_registry::delete_key(UNINSTALL_KEY);
    traces_remove();
    out.push(proxy_off.into());
    let inst = install_dir();
    // the window's WebView2 profile goes too: it holds only the window's cache
    let mut rm: Vec<String> = [legacy_dir(), local().join(WEBVIEW_ID)]
        .into_iter()
        .chain((!keep_data).then(|| paths.data.clone()))
        .map(|d| format!("Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue", psq(&text(&d))))
        .collect();
    if keep_data && same(&paths.data, &inst) {
        // the data stays in the program folder: only the program goes
        let i = psq(&text(&inst));
        rm.push(format!(
            "Get-ChildItem -LiteralPath {i} -File -Filter 'RealityHop*.exe' | Remove-Item -Force; Get-ChildItem -LiteralPath {i} -Directory -Filter 'app-*' | Remove-Item -Recurse -Force"
        ));
    } else {
        rm.push(format!(
            "Remove-Item -LiteralPath {} -Recurse -Force -ErrorAction SilentlyContinue",
            psq(&text(&inst))
        ));
    }
    // this process runs from the install folder: a separate shell removes it once this one and the window's WebView2
    // processes have exited, and tries again for a few seconds while antivirus still holds a file
    let script = format!(
        "Wait-Process -Id {} -ErrorAction SilentlyContinue; Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -like '*{WEBVIEW_ID}*' }} | ForEach-Object {{ Wait-Process -Id $_.ProcessId -Timeout 30 -ErrorAction SilentlyContinue }}; Start-Sleep 1; foreach ($i in 1..15) {{ {}; if (-not (Test-Path -LiteralPath {})) {{ break }}; Start-Sleep 2 }}",
        std::process::id(),
        rm.join("; "),
        psq(&text(&installed_exe(exe)))
    );
    let mut c = std::process::Command::new(system_tool("powershell"));
    c.args(["-NoProfile", "-Command", &script]);
    // not the install folder: PowerShell does not remove the folder it stands in
    c.current_dir(system_tool("powershell").parent().unwrap_or(Path::new("/")));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = c.spawn();
    out.push(format!(
        "program folder{}: removed when you close this message",
        if keep_data { "" } else { " and data folder" }
    ));
    out
}

// macOS: delete these folders once this process has exited (the app runs from one of them). Only RealityHop.app and
// the Reality Hop data folder are accepted (macos.remove_later)
pub fn remove_later(gone: &[PathBuf]) {
    let ok: Vec<String> = gone
        .iter()
        .filter(|p| p.file_name().is_some_and(|n| n == "RealityHop.app") || same(p, &mac_support()))
        .map(|p| text(p))
        .collect();
    if ok.is_empty() {
        return;
    }
    let mut c = std::process::Command::new("/bin/sh");
    c.args([
        "-c",
        "while kill -0 \"$0\" 2>/dev/null; do sleep 1; done; sleep 1; rm -rf -- \"$@\"",
        &std::process::id().to_string(),
    ]);
    c.args(&ok);
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let _ = c.spawn();
}
