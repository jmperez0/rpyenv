//! The Windows user and machine environment (registry), the change broadcast, and known
//! folders (spec §9.4). In debug builds, `RPYENV_TEST_ENV_KEY` redirects both
//! environments to `HKCU\<key>\user` and `HKCU\<key>\machine` and skips the broadcast;
//! `RPYENV_TEST_DOCUMENTS` and `RPYENV_TEST_PROGRAM_FILES` replace the known folders
//! (plan M6a, R1), and in unit-test builds of any profile. Tests always set them.

use std::io;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_EXPAND_SZ,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    Machine,
}

/// A registry string, and whether it's `REG_EXPAND_SZ` (its `%VARS%` expand when read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub text: String,
    pub expand: bool,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The test override (debug and unit-test builds only).
fn test_key() -> Option<String> {
    if cfg!(any(debug_assertions, test)) {
        std::env::var("RPYENV_TEST_ENV_KEY")
            .ok()
            .filter(|k| !k.is_empty())
    } else {
        None
    }
}

fn location(scope: Scope) -> (HKEY, String) {
    match (test_key(), scope) {
        (Some(k), Scope::User) => (HKEY_CURRENT_USER, format!("{k}\\user")),
        (Some(k), Scope::Machine) => (HKEY_CURRENT_USER, format!("{k}\\machine")),
        (None, Scope::User) => (HKEY_CURRENT_USER, "Environment".into()),
        (None, Scope::Machine) => (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment".into(),
        ),
    }
}

/// A variable of `scope`, as stored (not expanded); `Ok(None)` only when it doesn't exist.
/// Any other failure is an error, so a caller never mistakes an unreadable `Path` for an
/// empty one and writes over it (final review I5).
pub fn get(scope: Scope, name: &str) -> io::Result<Option<Value>> {
    let (root, sub) = location(scope);
    let (sub, name) = (wide(&sub), wide(name));
    let fail = |rc: u32| Err(io::Error::from_raw_os_error(rc as i32));
    // SAFETY: opens, sizes, reads and closes a key; buffers are locals of the given sizes.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        match RegOpenKeyExW(root, sub.as_ptr(), 0, KEY_READ, &mut key) {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            rc => return fail(rc),
        }
        let mut size = 0u32;
        // The value can grow between sizing and reading (ERROR_MORE_DATA): size again.
        for _ in 0..4 {
            let mut kind = 0u32;
            let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
            let mut bytes = (buf.len() * 2) as u32;
            let rc = RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr().cast(),
                &mut bytes,
            );
            if rc == ERROR_MORE_DATA {
                size = bytes;
                continue;
            }
            RegCloseKey(key);
            return match rc {
                ERROR_SUCCESS if kind == REG_SZ || kind == REG_EXPAND_SZ => {
                    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    Ok(Some(Value {
                        text: String::from_utf16_lossy(&buf[..len]),
                        expand: kind == REG_EXPAND_SZ,
                    }))
                }
                ERROR_SUCCESS => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "it isn't stored as text",
                )),
                ERROR_FILE_NOT_FOUND => Ok(None),
                rc => fail(rc),
            };
        }
        RegCloseKey(key);
        fail(ERROR_MORE_DATA)
    }
}

/// Sets a user variable, creating the key if needed (it only lacks it under the test key).
pub fn set_user(name: &str, value: &Value) -> io::Result<()> {
    let (root, sub) = location(Scope::User);
    let (sub, name, data) = (wide(&sub), wide(name), wide(&value.text));
    // SAFETY: creates or opens a key, writes a NUL-terminated UTF-16 string of the size
    // given, and closes the key.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let rc = RegCreateKeyExW(
            root,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        let kind = if value.expand { REG_EXPAND_SZ } else { REG_SZ };
        let rc = RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            kind,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        );
        RegCloseKey(key);
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
    }
    Ok(())
}

/// Tells running programs (Explorer, new terminals) that the environment changed. Skipped
/// under the test override.
pub fn broadcast() {
    if test_key().is_some() {
        return;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let what = wide("Environment");
    let mut result = 0usize;
    // SAFETY: a broadcast with a NUL-terminated string that outlives the call.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            what.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

/// `%VAR%`s replaced as Windows does, for comparing entries.
pub fn expand(s: &str) -> String {
    let src = wide(s);
    // SAFETY: sizes, then fills, a local buffer.
    unsafe {
        let n = ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0);
        if n == 0 {
            return s.to_string();
        }
        let mut buf = vec![0u16; n as usize];
        let n = ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), n);
        String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)])
    }
}

fn override_dir(var: &str) -> Option<PathBuf> {
    if cfg!(any(debug_assertions, test)) {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    } else {
        None
    }
}

fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    // SAFETY: the shell allocates the string; it's read, then freed once.
    unsafe {
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p);
        if hr != 0 || p.is_null() {
            return None;
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p.cast());
        Some(PathBuf::from(s))
    }
}

/// The user's Documents folder, where PowerShell keeps profiles (OneDrive-redirected or not).
pub fn documents() -> Option<PathBuf> {
    override_dir("RPYENV_TEST_DOCUMENTS")
        .or_else(|| known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Documents))
}

/// `%ProgramFiles%`, where an all-users install lives.
pub fn program_files() -> Option<PathBuf> {
    override_dir("RPYENV_TEST_PROGRAM_FILES")
        .or_else(|| known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_ProgramFiles))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only test here that sets `RPYENV_TEST_ENV_KEY` (tests share the process).
    #[test]
    fn a_value_round_trips_through_the_test_key_keeping_its_type() {
        let key = format!("Software\\rpyenv-test\\unit-{}", std::process::id());
        std::env::set_var("RPYENV_TEST_ENV_KEY", &key);
        // `cargo test --release` has no debug assertions: the override must hold there
        // too, or the writes below would reach the real user `Path`.
        assert_eq!(location(Scope::User).1, format!("{key}\\user"));
        assert!(get(Scope::User, "Path").unwrap().is_none());
        let v = Value {
            text: r"%USERPROFILE%\x;C:\y".into(),
            expand: true,
        };
        set_user("Path", &v).unwrap();
        assert_eq!(get(Scope::User, "Path").unwrap(), Some(v));
        assert!(get(Scope::Machine, "Path").unwrap().is_none());
        let _ = std::process::Command::new("reg")
            .args(["delete", &format!("HKCU\\{key}"), "/f"])
            .output();
    }
}
