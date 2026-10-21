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
    set_value(root, &sub, name, value)
}

/// Writes one string value, creating the key if needed.
fn set_value(root: HKEY, sub: &str, name: &str, value: &Value) -> io::Result<()> {
    let (sub, name, data) = (wide(sub), wide(name), wide(&value.text));
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

/// `%ProgramFiles%`, where an all-users install lives. The variable Windows sets in every
/// process comes first: the known folder loads the shell, which the all-users check on a
/// per-user install's commands shouldn't pay for (final review M5).
pub fn program_files() -> Option<PathBuf> {
    override_dir("RPYENV_TEST_PROGRAM_FILES")
        .or_else(|| {
            std::env::var_os("ProgramFiles")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_ProgramFiles))
}

/// What Windows PowerShell 5.1 does with scripts for this user: the effective execution
/// policy, and whether Group Policy set it (M6b Sandbox S1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPolicy {
    pub name: &'static str,
    pub by_group_policy: bool,
}

impl ScriptPolicy {
    /// A local, unsigned script, such as a profile, doesn't run.
    pub fn blocks_local_scripts(&self) -> bool {
        matches!(self.name, "Restricted" | "AllSigned")
    }
}

const POLICIES: [&str; 5] = [
    "Restricted",
    "AllSigned",
    "RemoteSigned",
    "Unrestricted",
    "Bypass",
];

/// A stored policy as PowerShell spells it. `Default` is the system's default; `Undefined`,
/// and anything else, defers to the next scope.
fn policy_name(s: &str, default: &'static str) -> Option<&'static str> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("Default") {
        return Some(default);
    }
    POLICIES.iter().copied().find(|p| p.eq_ignore_ascii_case(s))
}

/// Group Policy's "Turn on Script Execution": `EnableScripts` 0 means `Restricted`, 1 takes
/// `ExecutionPolicy`.
fn group_policy(enable: Option<u32>, policy: Option<&str>) -> Option<&'static str> {
    match enable? {
        0 => Some("Restricted"),
        _ => policy.and_then(|p| policy_name(p, "Restricted")),
    }
}

/// The scopes in PowerShell's order: machine and user Group Policy, then the CurrentUser and
/// LocalMachine settings, then the system's default (`Restricted` on client Windows,
/// `RemoteSigned` on Windows Server). The Process scope is left out: it lasts one process
/// tree, not the shells a user opens later.
pub fn effective_policy(
    machine_gpo: (Option<u32>, Option<&str>),
    user_gpo: (Option<u32>, Option<&str>),
    current_user: Option<&str>,
    local_machine: Option<&str>,
    server: bool,
) -> ScriptPolicy {
    let default = if server { "RemoteSigned" } else { "Restricted" };
    let gpo = |(e, p)| group_policy(e, p);
    if let Some(name) = gpo(machine_gpo).or_else(|| gpo(user_gpo)) {
        return ScriptPolicy {
            name,
            by_group_policy: true,
        };
    }
    let name = current_user
        .and_then(|p| policy_name(p, default))
        .or_else(|| local_machine.and_then(|p| policy_name(p, default)))
        .unwrap_or(default);
    ScriptPolicy {
        name,
        by_group_policy: false,
    }
}

enum Raw {
    Text(String),
    Dword(u32),
}

/// One value, text or DWORD, from the 64-bit view; `None` when absent or unreadable.
fn read_raw(root: HKEY, sub: &str, name: &str) -> Option<Raw> {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, REG_DWORD, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let (sub, name) = (wide(sub), wide(name));
    let mut buf = vec![0u16; 512];
    let (mut kind, mut bytes) = (0u32, (buf.len() * 2) as u32);
    // SAFETY: reads one value into a local buffer of the size given.
    let rc = unsafe {
        RegGetValueW(
            root,
            sub.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            &mut kind,
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    if kind == REG_DWORD {
        return Some(Raw::Dword(u32::from(buf[0]) | (u32::from(buf[1]) << 16)));
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(Raw::Text(String::from_utf16_lossy(&buf[..len])))
}

/// Where Windows PowerShell keeps its CurrentUser and LocalMachine policies.
const PS: &str = r"SOFTWARE\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell";

/// The CurrentUser policy's key: `HKCU\<key>\ps-user` under the test override.
fn current_user_policy_key() -> String {
    match test_key() {
        Some(k) => format!("{k}\\ps-user"),
        None => PS.into(),
    }
}

/// Windows PowerShell's CurrentUser policy as stored (what `Set-ExecutionPolicy -Scope
/// CurrentUser` writes), if any.
pub fn current_user_policy() -> Option<String> {
    match read_raw(
        HKEY_CURRENT_USER,
        &current_user_policy_key(),
        "ExecutionPolicy",
    ) {
        Some(Raw::Text(t)) => Some(t),
        _ => None,
    }
}

/// Windows PowerShell's LocalMachine policy as stored (`Set-ExecutionPolicy -Scope
/// LocalMachine`, an administrator's), if any: `HKCU\<key>\ps-machine` under the test
/// override.
pub fn local_machine_policy() -> Option<String> {
    let raw = match test_key() {
        Some(k) => read_raw(
            HKEY_CURRENT_USER,
            &format!("{k}\\ps-machine"),
            "ExecutionPolicy",
        ),
        None => read_raw(HKEY_LOCAL_MACHINE, PS, "ExecutionPolicy"),
    };
    match raw {
        Some(Raw::Text(t)) => Some(t),
        _ => None,
    }
}

/// Sets Windows PowerShell's CurrentUser policy, or with `None` removes it (back to
/// `Undefined`), as `Set-ExecutionPolicy -Scope CurrentUser` does.
pub fn set_current_user_policy(policy: Option<&str>) -> io::Result<()> {
    let sub = current_user_policy_key();
    let Some(policy) = policy else {
        use windows_sys::Win32::System::Registry::RegDeleteKeyValueW;
        let (sub, name) = (wide(&sub), wide("ExecutionPolicy"));
        // SAFETY: deletes one value; both strings are NUL-terminated locals.
        let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, sub.as_ptr(), name.as_ptr()) };
        return match rc {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            rc => Err(io::Error::from_raw_os_error(rc as i32)),
        };
    };
    set_value(
        HKEY_CURRENT_USER,
        &sub,
        "ExecutionPolicy",
        &Value {
            text: policy.into(),
            expand: false,
        },
    )
}

/// Windows PowerShell 5.1's effective policy for this user, read from the registry. Under
/// the test override, each key is `HKCU\<key>\<label>` (`gpo-machine`, `gpo-user`,
/// `ps-user`, `ps-machine`, `product`), holding the same value names.
pub fn windows_powershell_policy() -> ScriptPolicy {
    const GPO: &str = r"SOFTWARE\Policies\Microsoft\Windows\PowerShell";
    const PRODUCT: &str = r"SYSTEM\CurrentControlSet\Control\ProductOptions";
    let read = |label: &str, root: HKEY, sub: &str, name: &str| match test_key() {
        Some(k) => read_raw(HKEY_CURRENT_USER, &format!("{k}\\{label}"), name),
        None => read_raw(root, sub, name),
    };
    let text = |label, root, sub, name| match read(label, root, sub, name) {
        Some(Raw::Text(t)) => Some(t),
        _ => None,
    };
    let dword = |label, root, sub, name| match read(label, root, sub, name) {
        Some(Raw::Dword(d)) => Some(d),
        _ => None,
    };
    let (hklm, hkcu) = (HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER);
    let gm = (
        dword("gpo-machine", hklm, GPO, "EnableScripts"),
        text("gpo-machine", hklm, GPO, "ExecutionPolicy"),
    );
    let gu = (
        dword("gpo-user", hkcu, GPO, "EnableScripts"),
        text("gpo-user", hkcu, GPO, "ExecutionPolicy"),
    );
    let server = text("product", hklm, PRODUCT, "ProductType")
        .is_some_and(|t| !t.eq_ignore_ascii_case("WinNT"));
    effective_policy(
        (gm.0, gm.1.as_deref()),
        (gu.0, gu.1.as_deref()),
        text("ps-user", hkcu, PS, "ExecutionPolicy").as_deref(),
        text("ps-machine", hklm, PS, "ExecutionPolicy").as_deref(),
        server,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eff(
        mg: (Option<u32>, Option<&str>),
        ug: (Option<u32>, Option<&str>),
        cu: Option<&str>,
        lm: Option<&str>,
        server: bool,
    ) -> (&'static str, bool) {
        let p = effective_policy(mg, ug, cu, lm, server);
        (p.name, p.by_group_policy)
    }

    /// Sandbox S1: PowerShell's scope order, and the system default when nothing is set.
    #[test]
    fn the_effective_policy_follows_powershells_order() {
        let none = (None, None);
        assert_eq!(eff(none, none, None, None, false), ("Restricted", false));
        assert_eq!(eff(none, none, None, None, true), ("RemoteSigned", false));
        assert_eq!(
            eff(none, none, None, Some("Unrestricted"), false),
            ("Unrestricted", false)
        );
        assert_eq!(
            eff(none, none, Some("remotesigned"), Some("AllSigned"), false),
            ("RemoteSigned", false),
            "CurrentUser before LocalMachine, in any case"
        );
        assert_eq!(
            eff(none, none, Some("Undefined"), Some("Bypass"), false),
            ("Bypass", false),
            "Undefined defers"
        );
        assert_eq!(
            eff(none, none, Some("Default"), None, true),
            ("RemoteSigned", false),
            "Default is the system's"
        );
        assert_eq!(
            eff(
                none,
                (Some(1), Some("AllSigned")),
                Some("Bypass"),
                None,
                false
            ),
            ("AllSigned", true),
            "user Group Policy before CurrentUser"
        );
        assert_eq!(
            eff(
                (Some(0), Some("Unrestricted")),
                (Some(1), Some("RemoteSigned")),
                None,
                None,
                true
            ),
            ("Restricted", true),
            "machine Group Policy first; EnableScripts 0 is Restricted"
        );
        assert_eq!(
            eff((None, Some("AllSigned")), none, None, None, true),
            ("RemoteSigned", false),
            "no EnableScripts, no Group Policy"
        );
        let blocks = |name| {
            ScriptPolicy {
                name,
                by_group_policy: false,
            }
            .blocks_local_scripts()
        };
        assert!(blocks("Restricted") && blocks("AllSigned"));
        assert!(!blocks("RemoteSigned") && !blocks("Unrestricted") && !blocks("Bypass"));
    }

    /// The only test here that sets `RPYENV_TEST_ENV_KEY` (tests share the process).
    #[test]
    fn a_value_round_trips_through_the_test_key_keeping_its_type() {
        let key = format!("Software\\rpyenv-test-unit-{}", std::process::id());
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
