//! The Windows user and machine environment (registry), the change broadcast, and known
//! folders (spec §9.4). In debug builds, `RPYENV_TEST_ENV_KEY` redirects both
//! environments to `HKCU\<key>\user` and `HKCU\<key>\machine` and skips the broadcast;
//! `RPYENV_TEST_DOCUMENTS` and `RPYENV_TEST_PROGRAM_FILES` replace the known folders
//! (plan M6a, R1). Tests always set them.

use std::io;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
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

/// The test override (debug builds only).
fn test_key() -> Option<String> {
    if cfg!(debug_assertions) {
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

/// A variable of `scope`, as stored (not expanded).
pub fn get(scope: Scope, name: &str) -> Option<Value> {
    let (root, sub) = location(scope);
    let (sub, name) = (wide(&sub), wide(name));
    // SAFETY: opens, sizes, reads and closes a key; buffers are locals of the given sizes.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(root, sub.as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }
        let (mut kind, mut size) = (0u32, 0u32);
        let ok = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        );
        if ok != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
            RegCloseKey(key);
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut bytes = (buf.len() * 2) as u32;
        let ok = RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            buf.as_mut_ptr().cast(),
            &mut bytes,
        );
        RegCloseKey(key);
        if ok != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(Value {
            text: String::from_utf16_lossy(&buf[..len]),
            expand: kind == REG_EXPAND_SZ,
        })
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
    if cfg!(debug_assertions) {
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

    #[test]
    fn a_value_round_trips_through_the_test_key_keeping_its_type() {
        let key = format!("Software\\rpyenv-test\\unit-{}", std::process::id());
        std::env::set_var("RPYENV_TEST_ENV_KEY", &key);
        assert!(get(Scope::User, "Path").is_none());
        let v = Value {
            text: r"%USERPROFILE%\x;C:\y".into(),
            expand: true,
        };
        set_user("Path", &v).unwrap();
        assert_eq!(get(Scope::User, "Path"), Some(v));
        assert!(get(Scope::Machine, "Path").is_none());
        let _ = std::process::Command::new("reg")
            .args(["delete", &format!("HKCU\\{key}"), "/f"])
            .output();
    }
}
