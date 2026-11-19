//! The local date and time, from the system's own time zone rules (Windows: SystemTimeToTzSpecificLocalTime, macOS:
//! localtime_r), for log lines, file names and the times the window shows.

use std::time::{SystemTime, UNIX_EPOCH};

/// A local date and time, to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

// The UTC date of a day count since 1970-01-01 (Howard Hinnant's civil_from_days)
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

// The UTC time of a Unix time
pub fn utc(t: i64) -> Local {
    let (year, month, day) = civil(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400) as u32;
    Local { year, month, day, hour: s / 3600, minute: s / 60 % 60, second: s % 60 }
}

// The local time of a Unix time
pub fn at(t: i64) -> Local {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
        let u = utc(t);
        let st = SYSTEMTIME {
            wYear: u.year as u16,
            wMonth: u.month as u16,
            wDayOfWeek: 0,
            wDay: u.day as u16,
            wHour: u.hour as u16,
            wMinute: u.minute as u16,
            wSecond: u.second as u16,
            wMilliseconds: 0,
        };
        let mut l: SYSTEMTIME = unsafe { std::mem::zeroed() };
        if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &st, &mut l) } == 0 {
            return u;
        }
        Local {
            year: i64::from(l.wYear),
            month: u32::from(l.wMonth),
            day: u32::from(l.wDay),
            hour: u32::from(l.wHour),
            minute: u32::from(l.wMinute),
            second: u32::from(l.wSecond),
        }
    }
    #[cfg(unix)]
    {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&tt, &mut tm) }.is_null() {
            return utc(t);
        }
        Local {
            year: i64::from(tm.tm_year) + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }
}

// The local time now
pub fn now() -> Local {
    at(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64)
}

impl Local {
    // As strftime writes it, for %Y %m %d %H %M %S and plain text
    pub fn format(&self, pattern: &str) -> String {
        let mut out = String::new();
        let mut it = pattern.chars();
        while let Some(c) = it.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('Y') => out += &format!("{:04}", self.year),
                Some('m') => out += &format!("{:02}", self.month),
                Some('d') => out += &format!("{:02}", self.day),
                Some('H') => out += &format!("{:02}", self.hour),
                Some('M') => out += &format!("{:02}", self.minute),
                Some('S') => out += &format!("{:02}", self.second),
                Some(o) => {
                    out.push('%');
                    out.push(o);
                }
                None => out.push('%'),
            }
        }
        out
    }
}
