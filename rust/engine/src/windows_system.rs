//! The Windows calls that change the system: PowerShell as administrator (one UAC prompt), and the tray icon's place
//! on the taskbar.

use std::path::Path;

/// elevated's answer when the UAC prompt was declined, or there is no administrator account.
pub const DECLINED: u32 = 2;

// DOMAIN\user of the person running Reality Hop, from the environment, as Task Scheduler wants it: (domain, user)
pub fn user() -> (String, String) {
    let v = |k: &str| std::env::var(k).unwrap_or_default();
    (v("USERDOMAIN"), v("USERNAME"))
}

// Run a PowerShell snippet as administrator (one UAC prompt) and wait up to 5 minutes. Its exit code; DECLINED when
// the prompt was declined. The snippet goes in as plain text, readable in the process list. Never needed on macOS
// (engine.elevated)
pub fn elevated(ps: &str) -> u32 {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        use windows_sys::Win32::UI::Shell::{
            SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
        };
        let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
        let verb = wide("runas".as_ref());
        let file = wide(crate::process::system_tool("powershell").as_os_str());
        let params = wide(crate::windows_tasks::elevated_params(ps).as_ref());
        let mut e = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: params.as_ptr(),
            nShow: 0, // SW_HIDE
            ..Default::default()
        };
        if unsafe { ShellExecuteExW(&mut e) } == 0 || e.hProcess.is_null() {
            return DECLINED;
        }
        let mut code = 0u32;
        unsafe {
            WaitForSingleObject(e.hProcess, 300_000);
            GetExitCodeProcess(e.hProcess, &mut code);
            CloseHandle(e.hProcess);
        }
        code
    }
    #[cfg(not(windows))]
    {
        let _ = ps;
        DECLINED
    }
}

// A Windows path as os.path.normcase makes it: lower case, \ as the separator
fn normcase(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

// Windows 11 puts a new tray icon in the hidden overflow (^). Show Reality Hop's on the taskbar once, unless the
// person already chose (Windows Settings > Personalization > Taskbar > Other system tray icons); their choice wins
// (engine.tray_promote)
pub fn tray_promote(exe: &Path) {
    #[cfg(windows)]
    {
        use crate::windows_registry::Key;
        let exe = normcase(&exe.to_string_lossy());
        let parts: Vec<&str> = exe.split('\\').collect();
        let tail = format!("\\{}", parts[parts.len().saturating_sub(2)..].join("\\"));
        let root = r"Control Panel\NotifyIconSettings";
        let Ok(subkeys) = Key::open(root, false).and_then(|k| k.subkeys()) else { return };
        for name in subkeys {
            let Ok(k) = Key::open(&format!("{root}\\{name}"), true) else { return };
            if !k.string("ExecutablePath").is_ok_and(|p| normcase(&p).ends_with(&tail)) {
                continue;
            }
            if !k.has("IsPromoted") && k.set_dword("IsPromoted", 1).is_ok() {
                crate::activity_log::say("tray icon: shown on the taskbar");
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (exe, normcase);
}
