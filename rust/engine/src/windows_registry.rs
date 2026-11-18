//! HKEY_CURRENT_USER through the Win32 registry calls (winreg). Only this user's own part of the registry: Reality Hop
//! never writes to the machine's.

#![cfg(windows)]

use std::io;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_DWORD, REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE,
    REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};

// Text as a NUL-terminated UTF-16 string
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// A Win32 status as a result
fn check(code: u32) -> io::Result<()> {
    if code == ERROR_SUCCESS { Ok(()) } else { Err(io::Error::from_raw_os_error(code as i32)) }
}

/// An open key under HKEY_CURRENT_USER, closed when dropped.
#[derive(Debug)]
pub struct Key(HKEY);

impl Drop for Key {
    // Close it
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    // Open HKCU\path; write also allows setting values
    pub fn open(path: &str, write: bool) -> io::Result<Key> {
        let mut h: HKEY = std::ptr::null_mut();
        let access = if write { KEY_READ | KEY_SET_VALUE } else { KEY_READ };
        check(unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, access, &mut h) })?;
        Ok(Key(h))
    }

    // Open HKCU\path for writing, made first when it is missing (winreg.CreateKey)
    pub fn create(path: &str) -> io::Result<Key> {
        let mut h: HKEY = std::ptr::null_mut();
        let p = wide(path);
        let access = KEY_READ | KEY_WRITE;
        let null = std::ptr::null();
        let r = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                p.as_ptr(),
                0,
                null,
                REG_OPTION_NON_VOLATILE,
                access,
                null as _,
                &mut h,
                std::ptr::null_mut(),
            )
        };
        check(r)?;
        Ok(Key(h))
    }

    // A value's type and raw bytes
    fn raw(&self, name: &str) -> io::Result<(u32, Vec<u8>)> {
        let n = wide(name);
        let (mut kind, mut size) = (0u32, 0u32);
        let null = std::ptr::null();
        check(unsafe { RegQueryValueExW(self.0, n.as_ptr(), null, &mut kind, std::ptr::null_mut(), &mut size) })?;
        let mut b = vec![0u8; size as usize];
        check(unsafe { RegQueryValueExW(self.0, n.as_ptr(), null, &mut kind, b.as_mut_ptr(), &mut size) })?;
        b.truncate(size as usize);
        Ok((kind, b))
    }

    // Whether the key has a value of this name
    pub fn has(&self, name: &str) -> bool {
        self.raw(name).is_ok()
    }

    // A text value (REG_SZ or REG_EXPAND_SZ, not expanded), as winreg.QueryValueEx returns it
    pub fn string(&self, name: &str) -> io::Result<String> {
        let (kind, b) = self.raw(name)?;
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a text value"));
        }
        let w: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
        Ok(String::from_utf16_lossy(&w[..end]))
    }

    // A number value (REG_DWORD)
    pub fn dword(&self, name: &str) -> io::Result<u32> {
        match self.raw(name)? {
            (REG_DWORD, b) if b.len() == 4 => Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "not a number value")),
        }
    }

    // Set a value from its type and bytes
    fn set_raw(&self, name: &str, kind: u32, b: &[u8]) -> io::Result<()> {
        let len = u32::try_from(b.len()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        check(unsafe { RegSetValueExW(self.0, wide(name).as_ptr(), 0, kind, b.as_ptr(), len) })
    }

    // Set a text value (REG_SZ)
    pub fn set_string(&self, name: &str, value: &str) -> io::Result<()> {
        let b: Vec<u8> = wide(value).iter().flat_map(|c| c.to_le_bytes()).collect();
        self.set_raw(name, REG_SZ, &b)
    }

    // Set a number value (REG_DWORD)
    pub fn set_dword(&self, name: &str, value: u32) -> io::Result<()> {
        self.set_raw(name, REG_DWORD, &value.to_le_bytes())
    }

    // Remove a value; a missing one is no error
    pub fn delete_value(&self, name: &str) -> io::Result<()> {
        match unsafe { RegDeleteValueW(self.0, wide(name).as_ptr()) } {
            ERROR_FILE_NOT_FOUND => Ok(()),
            c => check(c),
        }
    }

    // The names of its values
    pub fn value_names(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut buf = vec![0u16; 16384]; // a value name is at most 16383 characters
            let mut len = buf.len() as u32;
            let null = std::ptr::null_mut();
            let r = unsafe {
                RegEnumValueW(self.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), null, null as _, null)
            };
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            check(r)?;
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        Ok(out)
    }

    // The names of its subkeys
    pub fn subkeys(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 256]; // a key name is at most 255 characters
            let mut len = buf.len() as u32;
            let null = std::ptr::null_mut();
            let r = unsafe {
                RegEnumKeyExW(self.0, i, buf.as_mut_ptr(), &mut len, std::ptr::null(), null, null as _, null as _)
            };
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            check(r)?;
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        Ok(out)
    }
}

// Remove HKCU\path with every key under it; a missing key is no error
pub fn delete_tree(path: &str) {
    if let Ok(subs) = Key::open(path, false).and_then(|k| k.subkeys()) {
        for s in subs {
            delete_tree(&format!(r"{path}\{s}"));
        }
    }
    let _ = delete_key(path);
}

// Remove HKCU\path, which must have no subkeys (winreg.DeleteKey)
pub fn delete_key(path: &str) -> io::Result<()> {
    check(unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(path).as_ptr()) })
}
