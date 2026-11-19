//! Other programs on this PC: the process list, stopping a program by name, and the loopback ports (engine.procs,
//! pids, kill_name, port_owner, port_open, port_taken, cmdline, exe_path, stop_pid). Windows asks the system directly; macOS asks ps and lsof.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

// [(process id, program file name)] of every process
pub fn procs() -> Vec<(u32, String)> {
    #[cfg(windows)]
    return windows::procs();
    #[cfg(not(windows))]
    {
        let cmd = ["/bin/ps", "-axww", "-o", "pid=,stat=,comm="];
        let out = crate::process::run(&cmd, None, Duration::from_secs(15), None).map(|o| o.stdout).unwrap_or_default();
        crate::macos::procs_from_ps(&crate::macos::ps_rows(&out))
    }
}

// Process ids of every process with this program name, any letter case
pub fn pids(name: &str) -> Vec<u32> {
    procs().into_iter().filter(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(p, _)| p).collect()
}

// A process's full command line, when this user may read it. Windows only: None elsewhere
pub fn cmdline(pid: u32) -> Option<String> {
    #[cfg(windows)]
    return windows::cmdline(pid);
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

// The program file a process runs, when this user may read it. Windows only: None elsewhere
pub fn exe_path(pid: u32) -> Option<String> {
    #[cfg(windows)]
    return windows::exe_path(pid);
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

// Stop one process, when this user may
pub fn stop_pid(pid: u32) {
    #[cfg(windows)]
    windows::terminate(pid);
    #[cfg(not(windows))]
    {
        let cmd = ["/bin/kill".to_string(), "-KILL".into(), pid.to_string()];
        let _ = crate::process::run(&cmd, None, Duration::from_secs(15), None);
    }
}

// Stop every process with this program name that this user may stop (what taskkill /F /IM did)
pub fn kill_name(name: &str) {
    let ids = pids(name);
    #[cfg(windows)]
    ids.iter().for_each(|&p| windows::terminate(p));
    #[cfg(not(windows))]
    if !ids.is_empty() {
        let mut cmd = vec!["/bin/kill".to_string(), "-KILL".into()];
        cmd.extend(ids.iter().map(u32::to_string));
        let _ = crate::process::run(&cmd, None, Duration::from_secs(15), None); // gone already, or another user's
        std::thread::sleep(Duration::from_millis(100));
    }
}

// Program name of the process listening on a local TCP port, or None
pub fn port_owner(port: u16) -> Option<String> {
    #[cfg(windows)]
    {
        let pid = windows::listener_pid(port)?;
        procs().into_iter().find(|(p, _)| *p == pid).map(|(_, n)| n)
    }
    #[cfg(not(windows))]
    {
        let cmd =
            ["/usr/sbin/lsof".to_string(), "-nP".into(), format!("-iTCP:{port}"), "-sTCP:LISTEN".into(), "-Fc".into()];
        let out = crate::process::run(&cmd, None, Duration::from_secs(15), None).ok()?.stdout;
        crate::macos::lsof_command(&out)
    }
}

// True when something accepts connections on this loopback port
pub fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(500)).is_ok()
}

// True when another process holds this loopback port (the "running" locks, the tunnel's 2080). A bind test: no
// connection is made. On macOS the bind reuses the address, so a closed connection's TIME_WAIT does not count.
pub fn port_taken(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_err()
}

#[cfg(windows)]
mod windows {
    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
    use windows_sys::Win32::Foundation::UNICODE_STRING;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, TerminateProcess,
    };

    // A process's command line, asked of Windows directly (no WMI, no PowerShell)
    pub fn cmdline(pid: u32) -> Option<String> {
        // SAFETY: the handle is checked and closed; the buffer is 8-byte aligned and sized from the first call; the
        // string Windows writes lies inside the buffer and is read within its stated length
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut len = 0u32;
            NtQueryInformationProcess(h, ProcessCommandLineInformation, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(2)];
            let size = (buf.len() * 8) as u32;
            let st =
                NtQueryInformationProcess(h, ProcessCommandLineInformation, buf.as_mut_ptr().cast(), size, &mut len);
            CloseHandle(h);
            let us = &*buf.as_ptr().cast::<UNICODE_STRING>();
            if st < 0 || us.Buffer.is_null() {
                return None;
            }
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(us.Buffer, us.Length as usize / 2)))
        }
    }

    // The program file a process runs
    pub fn exe_path(pid: u32) -> Option<String> {
        let mut b = vec![0u16; 32768];
        let mut n = b.len() as u32;
        // SAFETY: the handle is checked and closed; Windows writes at most n characters into b
        let ok = unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, b.as_mut_ptr(), &mut n) != 0;
            CloseHandle(h);
            ok
        };
        ok.then(|| String::from_utf16_lossy(&b[..n as usize]))
    }

    // A snapshot of the process list
    pub fn procs() -> Vec<(u32, String)> {
        let mut out = Vec::new();
        // SAFETY: the snapshot handle is checked and closed; the entry is plain data with its size set
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        out
    }

    // Stop one process, when this user may
    pub fn terminate(pid: u32) {
        // SAFETY: the handle is checked before use and closed
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !h.is_null() {
                TerminateProcess(h, 1);
                CloseHandle(h);
            }
        }
    }

    // The process id listening on a local TCP port, from Windows' own table of listening sockets: IPv4, then IPv6
    pub fn listener_pid(port: u16) -> Option<u32> {
        // (address family, row size, where the port and the pid are in a row)
        for (family, row, at_port, at_pid) in [(2u32, 24usize, 8usize, 20usize), (23, 56, 20, 52)] {
            let mut size = 0u32;
            // SAFETY: the first call only reports the size; the second writes at most size bytes into buf
            let raw = unsafe {
                GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0);
                size += 4096; // room for sockets opened since
                let mut buf = vec![0u8; size as usize];
                if GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0)
                    != 0
                {
                    continue;
                }
                buf
            };
            let n = u32::from_le_bytes(raw[..4].try_into().ok()?) as usize;
            for r in raw[4..].chunks_exact(row).take(n) {
                if u16::from_be_bytes([r[at_port], r[at_port + 1]]) == port {
                    return Some(u32::from_le_bytes(r[at_pid..at_pid + 4].try_into().ok()?));
                }
            }
        }
        None
    }
}
