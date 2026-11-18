//! The PowerShell and cmd scripts for Windows: scheduled tasks, folder rights, shortcuts, the elevated command line
//! and the console that adds this PC's key to the router.

use crate::powershell::psq;

/// Task Scheduler starts rh-core at boot under this name.
pub const BOOT_TASK: &str = "RealityHop tunnel";
/// Task Scheduler starts the watcher at logon under this name.
pub const LOGON_TASK: &str = "RealityHop watcher";

// DOMAIN\user as Task Scheduler wants it; just user without a domain
fn user_id(domain: &str, user: &str) -> String {
    format!("{domain}\\{user}").trim_start_matches('\\').to_string()
}

// The folder part of a Windows path
fn parent(path: &str) -> &str {
    path.rfind(['\\', '/']).map_or("", |i| &path[..i])
}

// PowerShell (needs admin) that creates the boot task, or points it at a new rh-core; a task switched off by Stop stays
// off. Normal priority: the Task Scheduler default (7) runs the tunnel below normal.
pub fn boot_task_ps(core: &str, cfg: &str, domain: &str, user: &str) -> String {
    format!(
        "$was = (Get-ScheduledTask -TaskName '{BOOT_TASK}' -ErrorAction SilentlyContinue).State; $a = New-ScheduledTaskAction -Execute {} -Argument {} -WorkingDirectory {}; $t = New-ScheduledTaskTrigger -AtStartup; $p = New-ScheduledTaskPrincipal -UserId {} -LogonType S4U -RunLevel Limited; $s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 5 -RestartCount 5 -RestartInterval (New-TimeSpan -Minutes 1) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable; Register-ScheduledTask -TaskName '{BOOT_TASK}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null; if ($was -eq 'Disabled') {{ Disable-ScheduledTask -TaskName '{BOOT_TASK}' | Out-Null }}",
        psq(core),
        psq(&format!("run -c \"{cfg}\"")),
        psq(parent(core)),
        psq(&user_id(domain, user))
    )
}

// The arguments after the program as one line, each with a space in quotes
fn task_args(cmd: &[&str]) -> String {
    cmd[1..]
        .iter()
        .map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.to_string() })
        .collect::<Vec<_>>()
        .join(" ")
}

// PowerShell (no admin) that registers the logon task for cmd as this user, normal priority; exits 0 when it is in place
pub fn logon_task_ps(cmd: &[&str], data: &str, domain: &str, user: &str) -> String {
    let args = task_args(cmd);
    let who = psq(&user_id(domain, user));
    format!(
        "try {{ $a = New-ScheduledTaskAction -Execute {} -Argument {} -WorkingDirectory {}; $t = New-ScheduledTaskTrigger -AtLogOn -User {who}; $p = New-ScheduledTaskPrincipal -UserId {who} -LogonType Interactive -RunLevel Limited; $s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -Priority 5 -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries; Register-ScheduledTask -TaskName {} -Action $a -Trigger $t -Principal $p -Settings $s -Force -ErrorAction Stop | Out-Null; exit 0 }} catch {{ exit 1 }}",
        psq(cmd[0]),
        psq(&args),
        psq(data),
        psq(LOGON_TASK)
    )
}

// PowerShell (needs admin, $d = the folder): only this Windows user, SYSTEM and administrators may open it. It holds
// the router SSH key, the node keys and the control API secret.
pub fn lock_ps(sid: &str) -> String {
    format!(
        "icacls $d /grant:r '*{sid}:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' /Q | Out-Null; icacls $d /inheritance:r /Q | Out-Null; icacls (Join-Path $d '*') /reset /T /C /Q | Out-Null"
    )
}

// PowerShell (needs admin): remove every scheduled task that starts the old sb-core.exe, then stop it
pub fn legacy_ps() -> &'static str {
    "Get-ScheduledTask | Where-Object { $_.Actions.Execute -like '*sb-core.exe*' } | Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue; Stop-Process -Name sb-core -Force -ErrorAction SilentlyContinue"
}

// subprocess.list2cmdline: one Windows command line from separate arguments
pub fn list2cmdline(args: &[&str]) -> String {
    let mut out = String::new();
    for arg in args {
        if !out.is_empty() {
            out.push(' ');
        }
        let quote = arg.is_empty() || arg.contains([' ', '\t']);
        if quote {
            out.push('"');
        }
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                _ => {
                    out.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                    out.push(c);
                }
            }
        }
        out.push_str(&"\\".repeat(if quote { backslashes * 2 } else { backslashes }));
        if quote {
            out.push('"');
        }
    }
    out
}

// The parameters for powershell.exe when a snippet runs as administrator (one UAC prompt)
pub fn elevated_params(ps: &str) -> String {
    list2cmdline(&["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", ps])
}

// PowerShell that makes one shortcut (.lnk) to target
pub fn shortcut_ps(lnk: &str, target: &str) -> String {
    format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.WorkingDirectory = {}; $s.IconLocation = {}; $s.Description = 'Reality Hop: get your VPN through networks that block VPNs'; $s.Save()",
        psq(lnk),
        psq(target),
        psq(parent(target)),
        psq(&format!("{target},0"))
    )
}

// The router command that adds a public key to its dropbear authorized_keys
pub fn add_key_remote(public_key: &str) -> String {
    let k = public_key.split_whitespace().nth(1).unwrap_or_default();
    format!(
        "mkdir -p /etc/dropbear; touch /etc/dropbear/authorized_keys; grep -q {k} /etc/dropbear/authorized_keys || echo {public_key} >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys; echo; echo Key added. Close this window."
    )
}

// add-router-key.cmd: a console where the person types the router password into ssh; Reality Hop never sees it.
// It runs from the data folder, with known_hosts by name: cmd cuts a full path at an &.
pub fn add_key_cmd(ssh_exe: &str, host: &str, public_key: &str) -> String {
    format!(
        "@echo off\r\nchcp 65001 >nul\r\ntitle Reality Hop: add key to router\r\necho Type the router admin password (the one for the GL.iNet web panel) and press Enter.\r\necho Nothing shows while you type. Reality Hop never sees it.\r\necho.\r\n\"{}\" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=known_hosts root@{host} \"{}\"\r\necho.\r\npause\r\n",
        ssh_exe.replace('%', "%%"),
        add_key_remote(public_key)
    )
}

// The router command that takes this PC's key off the router
pub fn remove_key_cmd(public_key: &str) -> String {
    let k = public_key.split_whitespace().nth(1).unwrap_or_default();
    format!("sed -i '\\|{k}|d' /etc/dropbear/authorized_keys; echo 'router: the key of this PC is removed'")
}

// PowerShell that makes the Startup folder shortcut for cmd: the fallback when Task Scheduler refuses the logon task
pub fn startup_lnk_ps(lnk: &str, cmd: &[&str], data: &str) -> String {
    format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.Arguments = {}; $s.WorkingDirectory = {}; $s.Save()",
        psq(lnk),
        psq(cmd[0]),
        psq(&task_args(cmd)),
        psq(data)
    )
}

// The boot task's state from schtasks /query /xml (the XML is the same in every Windows language): None when there is
// no task (engine.task_state)
pub fn task_state(code: Option<i32>, xml: &str) -> Option<&'static str> {
    match code {
        Some(0) if xml.contains("<Enabled>false</Enabled>") => Some("Disabled"),
        Some(0) => Some("Ready"),
        _ => None,
    }
}

// The boot task's priority from schtasks /query /xml: 7 (below normal) when the task sets none, None when there is no
// task (engine.boot_task_priority)
pub fn task_priority(code: Option<i32>, xml: &str) -> Option<i64> {
    static PRIORITY: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"<Priority>(\d+)</Priority>").unwrap());
    if code != Some(0) {
        return None;
    }
    Some(PRIORITY.captures(xml).and_then(|m| m[1].parse().ok()).unwrap_or(7))
}
