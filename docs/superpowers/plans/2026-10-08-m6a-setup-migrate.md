# M6a Setup and Migrate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Windows, `pyenv setup` prepares a user's environment, `pyenv migrate` takes over a pyenv-win install (and its pyenv-win-venv envs), `--restore` undoes that, `pyenv init --install` edits the PowerShell profiles, an all-users install sets each user up on first use, and the host architecture is read at run time.

**Architecture:**
- **Two new layers in `rpyenv-core`:**
  - `pathlist` (pure `PATH`-string editing, tested on every OS);
  - `winenv` (Windows: the user and machine environment in the registry, the `WM_SETTINGCHANGE` broadcast, known folders), with debug-only test overrides so tests never touch the real registry or Documents folder.
- **`pyenv` gains:**
  - `commands/pwsh_profile.rs` (adding and removing the profile line);
  - `commands/setup_win.rs`;
  - `commands/migrate_win.rs`, which records what it did in a manifest that `--restore` replays backwards.

**Tech Stack:** Rust 1.96, `windows-sys` 0.61 (`Win32_System_Registry`, `Win32_UI_Shell`, `Win32_System_Com`, `Win32_UI_WindowsAndMessaging`).

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md` §9.4 ("`pyenv setup`", "`pyenv migrate`", "Uninstall", "`PATH` order") and §7 (`init --install`). Research: pyenv-win's `install-pyenv-win.ps1` (local clone `C:\tmp\pyenv-win-856ed5a`), `docs/parity/pyenv-virtualenv-m4-harness-and-windows.md` §2.4 (pyenv-win-venv).

## Global Constraints

- **User decisions (2026-10-08):**
  1. M6 is split: **M6a** is these commands (this plan); **M6b** is the MSI, the Linux tarball and install script, the release workflow, and code signing.
  2. `migrate` **links pyenv-win-venv envs in** with junctions, and `--restore` removes them.
- **Spec §9.4 `setup`:**
  - creates `PYENV_ROOT`;
  - puts `shims` at the front of the user `PATH`;
  - runs rehash;
  - adds the PowerShell line ``iex ((pyenv init - pwsh) -join "`n")`` to the Windows PowerShell 5.1 and PowerShell 7 profiles, found where PowerShell finds them (OneDrive-redirected Documents included), leaving alone a profile that already mentions pyenv;
  - warns about a `python.exe` on the machine `PATH`;
  - for an all-users install, runs on any `pyenv` command that finds it hasn't run yet for this user, and prints a hint when it finds pyenv-win.
- **Spec §9.4 `migrate`:**
  - removes pyenv-win's `bin` from the user `PATH`;
  - moves `bin\pyenv.ps1`, `bin\pyenv.bat` and `bin\pyenv` into a backup folder inside `PYENV_ROOT`;
  - runs rehash;
  - adds the PowerShell profile line.

  `pyenv migrate --restore` undoes it.
- **pyenv-win's installer facts (install-pyenv-win.ps1:29-40, 129-140):**
  - `PATH` entries are the absolute `…\.pyenv\pyenv-win\bin` and `…\pyenv-win\shims`, placed first;
  - `PYENV`, `PYENV_ROOT` and `PYENV_HOME` are `…\pyenv-win\`, with a trailing backslash.

  rpyenv's Windows root is the same folder.
- **pyenv-win-venv facts:** envs are plain venvs in `%USERPROFILE%\.pyenv-win-venv\envs\<name>`. `pyvenv.cfg`'s `home` is the base version's folder.
- **Safety (carried from M4b):**
  - never delete or replace anything rpyenv didn't make;
  - never follow a junction when deleting;
  - names pass `crate::install::is_safe_win_segment`;
  - a junction is removed with `std::fs::remove_dir` only.
- **Tests never write the real registry, profiles or Documents** (memory: "never write real startup files"). The debug-only overrides (`cfg(debug_assertions)`) are:
  - `RPYENV_TEST_ENV_KEY` (an `HKCU` subkey standing in for both environments);
  - `RPYENV_TEST_DOCUMENTS`;
  - `RPYENV_TEST_PROGRAM_FILES`.

  `Fixture::command` (crates/pyenv/tests/common) always sets all three and deletes the key when dropped.
- **`setup` and `migrate` exist on Windows only** (`WIN_ONLY`). Linux keeps `init --install`.
- **Running tests:**
  - Windows: `cargo build --workspace`, then `cargo test --workspace --no-fail-fast`.
  - Linux (WSL runner, renamed for this branch): `cargo test --workspace` (pure modules only).
- **Commits:** a one-line `-m`. No attribution lines.

## Review Focus

These are the five input classes or failure modes most likely to bite a user, none exercised by a task's main tests. Each has a pinning test in its owning task.

1. **A user `PATH` holding `%USERPROFILE%`-style entries, a trailing backslash, different case, or stored as `REG_EXPAND_SZ`.** Expected: `setup` recognizes its `shims` entry however it's spelled, keeps the value type, and never duplicates the entry. Pinned in Task 1 (`pathlist` tests) and Task 3 (`setup_keeps_expand_sz_and_does_not_duplicate`).
2. **Running `setup` or `migrate` twice.** Expected: the second run changes nothing and says so. Pinned in Tasks 3 and 5.
3. **`--restore` after the user edited a profile or added a file to `bin`.** Expected: `--restore` removes only the line it added, never overwrites a `bin` file that exists again, and removes only the junctions it made, still pointing where it pointed them. Pinned in Task 5 (`restore_touches_only_what_migrate_did`).
4. **A pyenv-win-venv env whose name collides with an installed version, or whose base isn't installed.** Expected: it's skipped with a message, and nothing is overwritten. Pinned in Task 5.
5. **Uninstalling a base version after migrate linked a pyenv-win-venv env into its `envs`.** Expected: the junction goes and the env's files in `.pyenv-win-venv` stay. Pinned in Task 5 (`uninstalling_the_base_keeps_a_linked_venv_env`).

## Decisions (rulings made while planning)

- **R1, the tests' isolation.** All registry and known-folder access goes through `rpyenv_core::winenv`. In debug builds an override redirects it to `HKCU\<RPYENV_TEST_ENV_KEY>\user` (or `\machine`) and to the override folders, and skips the broadcast. The test fixture always sets the overrides, so a test can't reach the real environment.
- **R2, what `init --install` does on Windows.** For `pwsh` it adds the profile line (the same code as `setup`). Every other shell keeps today's refusal: cmd has no integration, and Git Bash startup files are out of scope.
- **R3, `setup` leaves `PYENV`, `PYENV_ROOT` and `PYENV_HOME` alone.** rpyenv's default root is pyenv-win's folder, so no variable is needed, and one the user set stays theirs.
- **R4, the manifest.** `migrate` writes `PYENV_ROOT\.rpyenv-migrate\manifest.txt`, one action per line, appended as each step succeeds, so a half-finished migrate can still be restored. The bin files go to `PYENV_ROOT\.rpyenv-migrate\bin\`.
- **R5, `--restore` removes the profile line `migrate` added.** pyenv-win has no `pyenv init`, so the line would fail in every new PowerShell. `--restore` asks the user to run pyenv-win's `pyenv rehash` to bring back its `.bat` shims.
- **R6, an all-users install is recognized by `current_exe()` living under Program Files.** The marker is `PYENV_ROOT\.rpyenv-setup`. On first run the setup is quiet apart from one stderr line, and it's skipped for `pyenv setup` itself.
- **R7, the host architecture is the native machine** (`IsWow64Process2`). That's what pyenv-win effectively sees, because its `cscript` runs natively: an ARM64 host gets `-arm64` even from an x64 rpyenv, and an x64 host gets `""` even from an x86 rpyenv. `PYENV_FORCE_ARCH` still wins.

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/rpyenv-core/src/pathlist.rs` | create | split, join, `put_first`, `without` (pure) |
| `crates/rpyenv-core/src/winenv.rs` | create (Windows) | registry environment, broadcast, expand, known folders, test overrides |
| `crates/rpyenv-core/src/ctx.rs` | modify | `suffix_for_machine` (pure) + run-time `host_arch_suffix` |
| `crates/pyenv/src/commands/pwsh_profile.rs` | create | the profile paths, `add`, `remove` |
| `crates/pyenv/src/commands/setup_win.rs` | create | `pyenv setup`, `first_run` |
| `crates/pyenv/src/commands/migrate_win.rs` | create | `pyenv migrate` / `--restore` |
| `crates/pyenv/src/commands/init_win.rs` | modify | `--install` for pwsh |
| `crates/pyenv/src/commands/mod.rs`, `lib.rs`, `help.rs` | modify | registration, first run, help topics |
| `crates/pyenv/tests/common/mod.rs` | modify | overrides + key cleanup |
| `crates/pyenv/tests/cli_setup_win.rs`, `cli_migrate_win.rs` | create | tests |
| spec §9.4/§7, README, `parity/golden/windows/commands.stdout` | modify | as built |

---

### Task 1: `pathlist`, `winenv`, and the test fixture's isolation

**Files:**
- Create: `crates/rpyenv-core/src/pathlist.rs`, `crates/rpyenv-core/src/winenv.rs`
- Modify: `crates/rpyenv-core/src/lib.rs`, `crates/rpyenv-core/Cargo.toml` (features `Win32_System_Registry`, `Win32_UI_Shell`, `Win32_System_Com`)
- Modify: `crates/pyenv/tests/common/mod.rs`

**Interfaces:**
- Produces, in `pathlist`:
  - `split(value: &str) -> Vec<String>`;
  - `join(entries: &[String]) -> String`;
  - `same(a: &str, b: &str) -> bool` (no trailing `\` or `/`, case-insensitive);
  - `put_first(value: &str, entry: &str, expand: &dyn Fn(&str) -> String) -> Option<String>` (`None` when the entry is already first and alone);
  - `without(value: &str, drop: &dyn Fn(&str) -> bool) -> (String, Vec<String>)` (the new value, and the removed entries as written).
- Produces, in `winenv` (Windows):
  - `enum Scope { User, Machine }`;
  - `struct Value { text: String, expand: bool }`;
  - `get(Scope, &str) -> Option<Value>`;
  - `set_user(&str, &Value) -> io::Result<()>`;
  - `broadcast()`;
  - `expand(&str) -> String`;
  - `documents() -> Option<PathBuf>`;
  - `program_files() -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing `pathlist` tests.** Create `pathlist.rs` with only:

```rust
//! A Windows `PATH` value as entries (spec §9.4): pure, so its tests run on every OS.

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn split_and_join_drop_empty_entries() {
        assert_eq!(split(r"a;;b;"), vec!["a", "b"]);
        assert_eq!(join(&split(r"a;b")), "a;b");
        assert!(split("").is_empty());
    }

    #[test]
    fn entries_compare_without_case_or_a_trailing_separator() {
        assert!(same(r"C:\Root\Shims\", r"c:\root\shims"));
        assert!(same("C:/x/", r"C:/x"));
        assert!(!same(r"C:\root\shims2", r"C:\root\shims"));
    }

    /// Review focus 1: however the entry is spelled, it ends up first and only once.
    #[test]
    fn put_first_moves_the_entry_and_removes_its_other_spellings() {
        let expand = |s: &str| s.replace("%USERPROFILE%", r"C:\Users\me");
        let shims = r"C:\Users\me\.pyenv\pyenv-win\shims";
        let v = r"C:\a;%USERPROFILE%\.pyenv\pyenv-win\Shims\;C:\b";
        assert_eq!(
            put_first(v, shims, &expand).unwrap(),
            format!(r"{shims};C:\a;C:\b")
        );
        assert_eq!(put_first(&format!(r"{shims};C:\a"), shims, &expand), None);
        assert_eq!(put_first("", shims, &id).unwrap(), shims);
    }

    #[test]
    fn without_returns_what_it_removed_as_written() {
        let (v, gone) = without(r"C:\x\bin;C:\a;c:\X\BIN\", &|e| same(e, r"C:\x\bin"));
        assert_eq!(v, r"C:\a");
        assert_eq!(gone, vec![r"C:\x\bin".to_string(), r"c:\X\BIN\".to_string()]);
    }
}
```

Add `pub mod pathlist;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rpyenv-core --lib pathlist::`
Expected: compile errors (`split`, `join`, `same`, `put_first`, `without` not found).

- [ ] **Step 3: Implement `pathlist`** above the tests:

```rust
/// The entries of `value`, in order, without empty ones.
pub fn split(value: &str) -> Vec<String> {
    value
        .split(';')
        .filter(|e| !e.trim().is_empty())
        .map(str::to_string)
        .collect()
}

pub fn join(entries: &[String]) -> String {
    entries.join(";")
}

/// The same folder: compared without case and without a trailing `\` or `/`.
pub fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

/// `value` with `entry` first and only there; `expand` turns an entry as written
/// (`%USERPROFILE%\…`) into the folder it names. `None` when it's already so.
pub fn put_first(value: &str, entry: &str, expand: &dyn Fn(&str) -> String) -> Option<String> {
    let entries = split(value);
    let rest: Vec<String> = entries
        .iter()
        .filter(|e| !same(&expand(e), entry))
        .cloned()
        .collect();
    let already = entries.first().is_some_and(|e| same(&expand(e), entry))
        && rest.len() + 1 == entries.len();
    if already {
        return None;
    }
    let mut out = vec![entry.to_string()];
    out.extend(rest);
    Some(join(&out))
}

/// `value` without the entries `drop` matches, and those entries as written.
pub fn without(value: &str, drop: &dyn Fn(&str) -> bool) -> (String, Vec<String>) {
    let (gone, kept): (Vec<String>, Vec<String>) = split(value).into_iter().partition(|e| drop(e));
    (join(&kept), gone)
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rpyenv-core --lib pathlist::`
Expected: 4 passed.

- [ ] **Step 5: Write the failing `winenv` test.** In `crates/rpyenv-core/src/winenv.rs`:

```rust
//! The Windows user and machine environment (registry), the change broadcast, and known
//! folders (spec §9.4). In debug builds, `RPYENV_TEST_ENV_KEY` redirects both
//! environments to `HKCU\<key>\user` and `HKCU\<key>\machine` and skips the broadcast;
//! `RPYENV_TEST_DOCUMENTS` and `RPYENV_TEST_PROGRAM_FILES` replace the known folders
//! (plan M6a, R1). Tests always set them.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_round_trips_through_the_test_key_keeping_its_type() {
        let key = format!("Software\\rpyenv-test\\unit-{}", std::process::id());
        std::env::set_var("RPYENV_TEST_ENV_KEY", &key);
        assert!(get(Scope::User, "Path").is_none());
        let v = Value { text: r"%USERPROFILE%\x;C:\y".into(), expand: true };
        set_user("Path", &v).unwrap();
        assert_eq!(get(Scope::User, "Path"), Some(v));
        assert!(get(Scope::Machine, "Path").is_none());
        let _ = std::process::Command::new("reg")
            .args(["delete", &format!("HKCU\\{key}"), "/f"])
            .output();
    }
}
```

Add `#[cfg(windows)] pub mod winenv;` to `lib.rs`.

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p rpyenv-core --lib winenv::`
Expected: compile errors (`get`, `set_user`, `Scope`, `Value` not found).

- [ ] **Step 7: Implement `winenv`** above the tests:

```rust
use std::io;
use std::os::windows::ffi::OsStrExt;
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
        std::env::var("RPYENV_TEST_ENV_KEY").ok().filter(|k| !k.is_empty())
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
        let ok = RegQueryValueExW(key, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut size);
        if ok != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
            RegCloseKey(key);
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut bytes = (buf.len() * 2) as u32;
        let ok = RegQueryValueExW(key, name.as_ptr(), std::ptr::null(), &mut kind, buf.as_mut_ptr().cast(), &mut bytes);
        RegCloseKey(key);
        if ok != ERROR_SUCCESS {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(Value { text: String::from_utf16_lossy(&buf[..len]), expand: kind == REG_EXPAND_SZ })
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
        let rc = RegCreateKeyExW(root, sub.as_ptr(), 0, std::ptr::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, std::ptr::null(), &mut key, std::ptr::null_mut());
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        let kind = if value.expand { REG_EXPAND_SZ } else { REG_SZ };
        let rc = RegSetValueExW(key, name.as_ptr(), 0, kind, data.as_ptr().cast(), (data.len() * 2) as u32);
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
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 5000, &mut result);
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
        std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
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

#[allow(dead_code)]
fn _uses(_: &std::ffi::OsStr) -> Option<Vec<u16>> {
    None
}
```

Remove the `_uses` helper and the `OsStrExt` import if clippy reports them unused. They're only there in case the executor needs `encode_wide`.

Add the three features to `rpyenv-core`'s `windows-sys` list.

- [ ] **Step 8: Isolate the CLI fixture.** In `crates/pyenv/tests/common/mod.rs`, `Fixture` gains a `test_key: String` field, set in `new()` to `format!("Software\\rpyenv-test\\{}", <the tempdir's file name>)`. `command()` gains:

```rust
        // Plan M6a, R1: setup, migrate and init --install never reach the real registry,
        // profiles or Program Files from a test.
        cmd.env("RPYENV_TEST_ENV_KEY", &self.test_key)
            .env("RPYENV_TEST_DOCUMENTS", self.base.join("Documents"))
            .env("RPYENV_TEST_PROGRAM_FILES", self.base.join("Program Files"));
```

Add a `Drop`:

```rust
impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = std::process::Command::new("reg")
            .args(["delete", &format!("HKCU\\{}", self.test_key), "/f"])
            .output();
    }
}
```

- [ ] **Step 9: Run them to verify they pass**

Run: `cargo test -p rpyenv-core --lib winenv:: pathlist::`, then `cargo test --workspace --no-fail-fast`.
Expected: 5 passed, and the suite is green.

- [ ] **Step 10: Commit:** `git commit -m "PATH-list editing and the Windows environment layer, with test overrides that keep tests off the real registry and profiles"`.

---

### Task 2: The PowerShell profile line and `pyenv init --install` on Windows

**Files:**
- Create: `crates/pyenv/src/commands/pwsh_profile.rs`
- Modify: `crates/pyenv/src/commands/mod.rs` (`pub mod pwsh_profile;`), `crates/pyenv/src/commands/init_win.rs`
- Test: `crates/pyenv/tests/cli_init_win.rs`

**Interfaces:**
- Produces:
  - `pwsh_profile::LINE: &str`;
  - `pwsh_profile::paths(documents: &Path) -> [PathBuf; 2]`;
  - `enum Added { Added, Mentions }`;
  - `add(&Path) -> io::Result<Added>`;
  - `remove(&Path) -> io::Result<bool>`;
  - `install_all() -> Output` (adds the line to both profiles and reports it).

- [ ] **Step 1: Write the failing unit tests.** Create `pwsh_profile.rs`:

```rust
//! The PowerShell profile line (spec §7, §9.4): ``iex ((pyenv init - pwsh) -join "`n")``,
//! in the Windows PowerShell 5.1 and PowerShell 7 profiles under Documents.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_profiles_under_documents() {
        let [p5, p7] = paths(Path::new(r"D:\Docs"));
        assert_eq!(p5, Path::new(r"D:\Docs\WindowsPowerShell\Microsoft.PowerShell_profile.ps1"));
        assert_eq!(p7, Path::new(r"D:\Docs\PowerShell\Microsoft.PowerShell_profile.ps1"));
    }

    #[test]
    fn add_creates_appends_once_and_leaves_a_pyenv_profile_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let new = tmp.path().join("a").join("p.ps1");
        assert_eq!(add(&new).unwrap(), Added::Added);
        assert_eq!(std::fs::read_to_string(&new).unwrap(), format!("{LINE}\r\n"));
        assert_eq!(add(&new).unwrap(), Added::Mentions, "already mentions pyenv");
        let other = tmp.path().join("o.ps1");
        std::fs::write(&other, "Set-Alias ll ls").unwrap();
        assert_eq!(add(&other).unwrap(), Added::Added);
        assert_eq!(std::fs::read_to_string(&other).unwrap(), format!("Set-Alias ll ls\r\n{LINE}\r\n"));
        let mine = tmp.path().join("m.ps1");
        std::fs::write(&mine, "# my PYENV setup\r\n").unwrap();
        assert_eq!(add(&mine).unwrap(), Added::Mentions);
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "# my PYENV setup\r\n");
    }

    #[test]
    fn remove_takes_out_only_the_exact_line() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("p.ps1");
        std::fs::write(&f, format!("a\r\n{LINE}\r\n# pyenv notes\r\nb\r\n")).unwrap();
        assert!(remove(&f).unwrap());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "a\r\n# pyenv notes\r\nb\r\n");
        assert!(!remove(&f).unwrap());
        assert!(!remove(&tmp.path().join("missing.ps1")).unwrap());
    }
}
```

Add `pub mod pwsh_profile;` to `commands/mod.rs`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p pyenv --lib pwsh_profile::`
Expected: compile errors.

- [ ] **Step 3: Implement** above the tests:

```rust
use crate::Output;
use std::io;
use std::path::{Path, PathBuf};

pub const LINE: &str = "iex ((pyenv init - pwsh) -join \"`n\")";

/// The current-user, current-host profiles of Windows PowerShell 5.1 and PowerShell 7.
pub fn paths(documents: &Path) -> [PathBuf; 2] {
    [
        documents.join("WindowsPowerShell").join("Microsoft.PowerShell_profile.ps1"),
        documents.join("PowerShell").join("Microsoft.PowerShell_profile.ps1"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Added,
    /// The profile already mentions pyenv: left alone, as `pyenv init --install` does.
    Mentions,
}

/// Appends `LINE` to `file` (created with its folder if missing), unless it already
/// mentions pyenv in any case.
pub fn add(file: &Path) -> io::Result<Added> {
    let text = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    if text.to_ascii_lowercase().windows(5).any(|w| w == b"pyenv") {
        return Ok(Added::Mentions);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = text;
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(LINE.as_bytes());
    out.extend_from_slice(b"\r\n");
    std::fs::write(file, out)?;
    Ok(Added::Added)
}

/// Removes every line that is exactly `LINE`; true when there was one.
pub fn remove(file: &Path) -> io::Result<bool> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut removed = false;
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == LINE {
            removed = true;
        } else {
            out.push_str(line);
        }
    }
    if removed {
        std::fs::write(file, out)?;
    }
    Ok(removed)
}

/// Adds the line to both profiles and reports each (`init --install pwsh`, `setup`).
#[cfg(windows)]
pub fn install_all() -> Output {
    let mut o = Output::new();
    let Some(docs) = rpyenv_core::winenv::documents() else {
        return Output::error("pyenv: cannot find your Documents folder for the PowerShell profiles");
    };
    for p in paths(&docs) {
        match add(&p) {
            Ok(Added::Added) => o.out(format!("pyenv: added the PowerShell line to {}", p.display())),
            Ok(Added::Mentions) => o.out(format!("pyenv: {} already mentions pyenv; left as it is", p.display())),
            Err(e) => o.err(format!("pyenv: {}: {}", p.display(), rpyenv_core::launch::io_reason(&e))),
        }
    }
    if !o.stderr.is_empty() {
        o.code = 1;
    }
    o
}
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `cargo test -p pyenv --lib pwsh_profile::`
Expected: 3 passed.

- [ ] **Step 5: Write the failing CLI test.** In `crates/pyenv/tests/cli_init_win.rs`, replace the test pinning "Decision 2: no startup-file editing on Windows before M6" with:

```rust
/// M6a (plan R2): `init --install pwsh` adds the profile line to both profiles; cmd still
/// refuses.
#[test]
fn init_install_pwsh_adds_the_profile_line() {
    let f = Fixture::new();
    let r = f.pyenv(&["init", "--install", "pwsh"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    for sub in ["WindowsPowerShell", "PowerShell"] {
        let p = f.base.join("Documents").join(sub).join("Microsoft.PowerShell_profile.ps1");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "iex ((pyenv init - pwsh) -join \"`n\")\r\n");
    }
    let r = f.pyenv(&["init", "--install", "cmd"]);
    assert_ne!(r.code, 0);
}
```

- [ ] **Step 6: Run it to verify it fails** (`cargo test -p pyenv --test cli_init_win -- init_install_pwsh`): today's refusal makes the exit code non-zero.

- [ ] **Step 7: Implement.** In `init_win.rs`, replace the `Mode::Install` arm:

```rust
        // Plan M6a, R2: PowerShell gets the profile line; other shells have no startup
        // file rpyenv edits.
        Mode::Install if family == Family::Pwsh => crate::commands::pwsh_profile::install_all(),
        Mode::Install => Output::error(format!(
            "pyenv: cannot automatically configure startup files for {shell}"
        )),
```

- [ ] **Step 8: Run to verify it passes**, plus `cargo test -p pyenv --test cli_init_win`.

- [ ] **Step 9: Commit:** `git commit -m "init --install pwsh adds the PowerShell line to the 5.1 and 7 profiles, leaving a profile that mentions pyenv alone"`.

---

### Task 3: `pyenv setup`

**Files:**
- Create: `crates/pyenv/src/commands/setup_win.rs`, `crates/pyenv/tests/cli_setup_win.rs`
- Modify: `crates/pyenv/src/commands/mod.rs` (`WIN_ONLY` gains `("setup", setup_win::setup)`), `crates/pyenv/src/help.rs` (topic `setup`)

**Interfaces:**
- Consumes: Task 1's `pathlist::put_first` and `winenv::{get, set_user, broadcast, expand, Scope, Value}`; Task 2's `pwsh_profile::install_all`.
- Produces:
  - `setup_win::setup(&Ctx, &[&str]) -> Output`;
  - `setup_win::run(&Ctx) -> Output`;
  - `setup_win::MARKER = ".rpyenv-setup"`;
  - `setup_win::pyenv_win_bin(&Ctx) -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests** (`cli_setup_win.rs`, `#![cfg(windows)]`). The fixture's user environment is `HKCU\<f.test_key>\user`; tests seed and read it with `reg`:

```rust
#![cfg(windows)]
mod common;
use common::*;

fn reg_set(f: &Fixture, scope: &str, name: &str, kind: &str, value: &str) {
    let out = std::process::Command::new("reg")
        .args(["add", &format!("HKCU\\{}\\{scope}", f.test_key), "/v", name, "/t", kind, "/d", value, "/f"])
        .output()
        .unwrap();
    assert!(out.status.success());
}

fn reg_get(f: &Fixture, scope: &str, name: &str) -> String {
    let out = std::process::Command::new("reg")
        .args(["query", &format!("HKCU\\{}\\{scope}", f.test_key), "/v", name])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn setup_puts_shims_first_adds_the_profile_line_and_marks_the_root() {
    let f = Fixture::new();
    reg_set(&f, "user", "Path", "REG_EXPAND_SZ", r"C:\Tools");
    let r = f.pyenv(&["setup"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let shims = f.root.join("shims");
    assert!(reg_get(&f, "user", "Path").contains(&format!("{};C:\\Tools", shims.display())));
    assert!(f.root.join(".rpyenv-setup").is_file());
    let p7 = f.base.join("Documents").join("PowerShell").join("Microsoft.PowerShell_profile.ps1");
    assert!(std::fs::read_to_string(p7).unwrap().contains("pyenv init - pwsh"));
    assert!(r.stdout.contains("open a new terminal"), "{}", r.stdout);
}

/// Review focus 1 and 2: a REG_EXPAND_SZ value keeps its type, the entry isn't duplicated,
/// and a second run changes nothing.
#[test]
fn setup_keeps_expand_sz_and_does_not_duplicate() {
    let f = Fixture::new();
    let shims = f.root.join("shims");
    let spelled = format!("{}\\", shims.display()).to_uppercase();
    reg_set(&f, "user", "Path", "REG_EXPAND_SZ", &format!(r"C:\Tools;{spelled}"));
    assert_eq!(f.pyenv(&["setup"]).code, 0);
    let after = reg_get(&f, "user", "Path");
    assert!(after.contains("REG_EXPAND_SZ"), "{after}");
    assert_eq!(after.to_lowercase().matches(&shims.display().to_string().to_lowercase()).count(), 1, "{after}");
    let again = f.pyenv(&["setup"]);
    assert!(again.stdout.contains("already first"), "{}", again.stdout);
    assert_eq!(reg_get(&f, "user", "Path"), after);
}

/// Spec §9.4 "PATH order": a python.exe on the machine PATH is named.
#[test]
fn setup_warns_about_a_python_on_the_machine_path() {
    let f = Fixture::new();
    let sys_py = f.base.join("SysPython");
    std::fs::create_dir_all(&sys_py).unwrap();
    std::fs::write(sys_py.join("python.exe"), b"").unwrap();
    reg_set(&f, "machine", "Path", "REG_SZ", &sys_py.display().to_string());
    let r = f.pyenv(&["setup"]);
    assert!(r.stderr.contains(&sys_py.join("python.exe").display().to_string()), "{}", r.stderr);
}

/// Spec §9.4: pyenv-win's own `bin` in this root gets a hint to migrate.
#[test]
fn setup_hints_at_migrate_when_pyenv_win_is_there() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("bin")).unwrap();
    std::fs::write(f.root.join("bin").join("pyenv.ps1"), b"").unwrap();
    let r = f.pyenv(&["setup"]);
    assert!(r.stderr.contains("pyenv migrate"), "{}", r.stderr);
}
```

Make `Fixture::test_key` `pub`.

- [ ] **Step 2: Run them to verify they fail:** unknown command `setup`.

- [ ] **Step 3: Implement** `setup_win.rs`:

```rust
//! `pyenv setup` (rpyenv-only, spec §9.4): prepares one user's environment.

use crate::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{pathlist, winenv};
use std::path::PathBuf;

pub const MARKER: &str = ".rpyenv-setup";

pub const HELP: &str = "Usage: pyenv setup

Prepares your Windows user for rpyenv: creates PYENV_ROOT, puts its shims folder first
on your user PATH, rehashes, and adds the PowerShell line to your profiles. It's safe to
run again.
";

pub fn setup(ctx: &Ctx, args: &[&str]) -> Output {
    match args {
        [] => run(ctx),
        ["--help"] => {
            let mut o = Output::new();
            o.stdout.push_str(HELP);
            o
        }
        _ => Output::error(HELP).with_code(1),
    }
}

/// pyenv-win's `bin` with its launchers, in this root, if it's there.
pub fn pyenv_win_bin(ctx: &Ctx) -> Option<PathBuf> {
    let bin = ctx.root.join("bin");
    ["pyenv.ps1", "pyenv.bat", "pyenv"]
        .iter()
        .any(|f| bin.join(f).is_file())
        .then_some(bin)
}

/// The setup itself (also the all-users first run).
pub fn run(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    for d in [ctx.versions_dir(), ctx.shims_dir()] {
        if let Err(e) = std::fs::create_dir_all(&d) {
            return Output::error(format!("pyenv: cannot create {}: {}", d.display(), rpyenv_core::launch::io_reason(&e)));
        }
    }
    let shims = ctx.shims_dir().display().to_string();
    let current = winenv::get(winenv::Scope::User, "Path")
        .unwrap_or(winenv::Value { text: String::new(), expand: true });
    match pathlist::put_first(&current.text, &shims, &winenv::expand) {
        None => o.out(format!("pyenv: {shims} is already first on your user PATH")),
        Some(text) => match winenv::set_user("Path", &winenv::Value { text, expand: current.expand }) {
            Ok(()) => {
                o.out(format!("pyenv: added {shims} to the front of your user PATH"));
                winenv::broadcast();
            }
            Err(e) => o.err(format!("pyenv: cannot change your user PATH: {e}")),
        },
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    o.stderr.push_str(&r.stderr);
    let p = crate::commands::pwsh_profile::install_all();
    o.stdout.push_str(&p.stdout);
    o.stderr.push_str(&p.stderr);
    if let Some(machine) = winenv::get(winenv::Scope::Machine, "Path") {
        for e in pathlist::split(&machine.text) {
            let py = PathBuf::from(winenv::expand(&e)).join("python.exe");
            if py.is_file() {
                o.err(format!("pyenv: warning: {} is on the machine PATH, which Windows searches before yours: it runs instead of the shims", py.display()));
            }
        }
    }
    if let Some(bin) = pyenv_win_bin(ctx) {
        o.err(format!("pyenv: pyenv-win is installed in {}; run `pyenv migrate` to let rpyenv take over", bin.display()));
    }
    let _ = std::fs::write(ctx.root.join(MARKER), env!("CARGO_PKG_VERSION"));
    o.out("pyenv: open a new terminal for the changes to take effect");
    if !p.stderr.is_empty() {
        o.code = 1;
    }
    o
}
```

Register `("setup", setup_win::setup)` in `WIN_ONLY` and `pub mod setup_win;` (`#[cfg(windows)]`) in `commands/mod.rs`. Add `topic("setup", None, None, commands::setup_win::HELP)` in `help.rs`, next to `uninstall`. Use the shape of the existing `topic(...)` calls exactly.

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_setup_win`.
Expected: 4 passed.

- [ ] **Step 5: Regenerate the Windows `commands` golden.**
  - Run `python parity/diff.py --rpyenv target/debug --pyenv-win <fresh clone of C:/tmp/pyenv-win-856ed5a> --only commands --update-golden`. Use the exact flags the script's `--help` shows.
  - Review the diff: `setup` is the only new line (allowlist D-16 covers it).
  - Run the full Windows diff to confirm nothing else moved.

- [ ] **Step 6: Commit:** `git commit -m "pyenv setup: PYENV_ROOT, shims first on the user PATH, rehash, PowerShell profiles, warnings for a machine-wide python and pyenv-win"`.

---

### Task 4: The all-users first run

**Files:**
- Modify: `crates/pyenv/src/commands/setup_win.rs` (`first_run`), `crates/pyenv/src/lib.rs` (`run_pyenv_win`)
- Test: `crates/pyenv/tests/cli_setup_win.rs`

**Interfaces:**
- Consumes: Task 3's `run` and `MARKER`, and Task 1's `winenv::program_files`.
- Produces: `setup_win::first_run(&Ctx, cmd: &str) -> Option<Output>`.

- [ ] **Step 1: Write the failing tests:**

```rust
/// Spec §9.4: an all-users install (pyenv.exe under Program Files) sets each user up on
/// their first `pyenv` command; a per-user install doesn't.
#[test]
fn an_all_users_install_sets_the_user_up_on_first_use() {
    let f = Fixture::new();
    let pf_bin = f.base.join("Program Files").join("rpyenv").join("bin");
    std::fs::create_dir_all(&pf_bin).unwrap();
    for exe in ["pyenv.exe", "pyenv-shim.exe", "pyenv-shimw.exe"] {
        let src = std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")).with_file_name(exe);
        std::fs::copy(&src, pf_bin.join(exe)).unwrap();
    }
    let out = f.command(&pf_bin.join("pyenv.exe"), &f.work, &[]).arg("root").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("set up for this user"), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(f.root.join(".rpyenv-setup").is_file());
    let again = f.command(&pf_bin.join("pyenv.exe"), &f.work, &[]).arg("root").output().unwrap();
    assert!(again.stderr.is_empty(), "{}", String::from_utf8_lossy(&again.stderr));
}

#[test]
fn a_per_user_install_doesnt_set_up_on_its_own() {
    let f = Fixture::new();
    assert_eq!(f.pyenv(&["root"]).code, 0);
    assert!(!f.root.join(".rpyenv-setup").exists());
}
```

- [ ] **Step 2: Run them to verify the first fails** (no "set up for this user"). The second passes today, as a pin.

- [ ] **Step 3: Implement.** In `setup_win.rs`:

```rust
/// Spec §9.4: an all-users install (this pyenv.exe under Program Files) sets the user up
/// on their first command (plan M6a, R6). `None` when nothing was needed.
pub fn first_run(ctx: &Ctx, cmd: &str) -> Option<Output> {
    if cmd == "setup" || ctx.root.join(MARKER).exists() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let pf = rpyenv_core::winenv::program_files()?;
    if !exe.starts_with(&pf) {
        return None;
    }
    let r = run(ctx);
    let mut o = Output::new();
    o.err("pyenv: set up for this user (see `pyenv setup`)");
    o.stderr.push_str(&r.stderr);
    Some(o)
}
```

In `lib.rs` `run_pyenv_win`, once the command name is known and before dispatch:

```rust
    #[cfg(windows)]
    let first = crate::commands::setup_win::first_run(ctx, cmd);
```

Then, after computing the command's `Output` `out`: `if let Some(f) = first { out.stderr.insert_str(0, &f.stderr); }`. The executor adapts this to `run_pyenv_win`'s structure: one call before dispatch, one merge after.

- [ ] **Step 4: Run them to verify they pass.**

- [ ] **Step 5: Commit:** `git commit -m "An all-users install sets each user up on their first pyenv command"`.

---

### Task 5: `pyenv migrate` and `--restore`

**Files:**
- Create: `crates/pyenv/src/commands/migrate_win.rs`, `crates/pyenv/tests/cli_migrate_win.rs`
- Modify: `crates/pyenv/src/commands/mod.rs` (`("migrate", migrate_win::migrate)`), `help.rs` (topic)

**Interfaces:**
- Consumes:
  - Task 1's `pathlist::{without, same}` and `winenv::*`;
  - Task 2's `pwsh_profile::{paths, add, remove, Added}`;
  - Task 3's `pyenv_win_bin`;
  - `rpyenv_core::junction::create`, `rpyenv_core::venv::read_cfg`, `crate::install::is_safe_win_segment`, `crate::commands::rehash::rehash`.
- Produces: `migrate_win::migrate(&Ctx, &[&str]) -> Output`, and the manifest at `PYENV_ROOT\.rpyenv-migrate\manifest.txt`. Its lines:
  - `bin\t<file name>`
  - `path\t<entry as written>`
  - `profile\t<path>`
  - `junction\t<link>\t<target>`

- [ ] **Step 1: Write the failing tests** (`cli_migrate_win.rs`). They use the same `reg_set`/`reg_get` helpers as `cli_setup_win.rs`; copy them in:

```rust
/// A pyenv-win install in the fixture's root: launchers in `bin`, `bin` and `shims` on the
/// user PATH, one version.
fn pyenv_win(f: &Fixture) {
    let bin = f.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for n in ["pyenv.ps1", "pyenv.bat", "pyenv"] {
        std::fs::write(bin.join(n), format!("{n} from pyenv-win")).unwrap();
    }
    f.version("3.12.1");
    let path = format!("{};{};C:\\Tools", bin.display(), f.root.join("shims").display());
    reg_set(f, "user", "Path", "REG_SZ", &path);
}

#[test]
fn migrate_takes_over_and_restore_gives_back() {
    let f = Fixture::new();
    pyenv_win(&f);
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(!f.root.join("bin").join("pyenv.ps1").exists());
    assert!(f.root.join(".rpyenv-migrate").join("bin").join("pyenv.ps1").is_file());
    let path = reg_get(&f, "user", "Path");
    assert!(!path.to_lowercase().contains(&f.root.join("bin").display().to_string().to_lowercase()), "{path}");
    assert!(path.contains(&f.root.join("shims").display().to_string()), "{path}");
    let p5 = f.base.join("Documents").join("WindowsPowerShell").join("Microsoft.PowerShell_profile.ps1");
    assert!(std::fs::read_to_string(&p5).unwrap().contains("pyenv init - pwsh"));

    // Review focus 2: a second run changes nothing.
    let again = f.pyenv(&["migrate"]);
    assert!(again.stdout.contains("already migrated"), "{}", again.stdout);

    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}{}", back.stdout, back.stderr);
    assert_eq!(std::fs::read_to_string(f.root.join("bin").join("pyenv.ps1")).unwrap(), "pyenv.ps1 from pyenv-win");
    assert!(reg_get(&f, "user", "Path").contains(&f.root.join("bin").display().to_string()));
    assert!(!std::fs::read_to_string(&p5).unwrap().contains("pyenv init - pwsh"));
    assert!(!f.root.join(".rpyenv-migrate").exists());
}

#[test]
fn migrate_without_pyenv_win_says_so() {
    let f = Fixture::new();
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("no pyenv-win"), "{}", r.stderr);
}

/// pyenv-win-venv envs are linked in (user decision 2026-10-08), with Review focus 4: a
/// name taken by a version, or an env whose base isn't installed, is skipped.
#[test]
fn migrate_links_pyenv_win_venv_envs() {
    let f = Fixture::new();
    pyenv_win(&f);
    let envs = f.base.join(".pyenv-win-venv").join("envs");
    let base = f.root.join("versions").join("3.12.1");
    for (name, home) in [("work", base.clone()), ("3.12.1", base.clone()), ("orphan", f.root.join("versions").join("3.9.9"))] {
        let e = envs.join(name);
        std::fs::create_dir_all(e.join("Scripts")).unwrap();
        std::fs::write(e.join("pyvenv.cfg"), format!("home = {}\r\n", home.display())).unwrap();
    }
    let r = f.pyenv(&["migrate"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(f.root.join("versions").join("work").join("pyvenv.cfg").is_file());
    assert!(base.join("envs").join("work").join("pyvenv.cfg").is_file());
    assert!(r.stderr.contains("3.12.1") && r.stderr.contains("orphan"), "{}", r.stderr);
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(back.code, 0, "{}", back.stderr);
    assert!(!f.root.join("versions").join("work").exists());
    assert!(envs.join("work").join("pyvenv.cfg").is_file(), "the env's files stay");
}

/// Review focus 3: `--restore` touches only what migrate did.
#[test]
fn restore_touches_only_what_migrate_did() {
    let f = Fixture::new();
    pyenv_win(&f);
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    std::fs::write(f.root.join("bin").join("pyenv.ps1"), "a newer pyenv.ps1").unwrap();
    let p7 = f.base.join("Documents").join("PowerShell").join("Microsoft.PowerShell_profile.ps1");
    let mut text = std::fs::read_to_string(&p7).unwrap();
    text.push_str("Set-Alias ll ls\r\n");
    std::fs::write(&p7, &text).unwrap();
    let back = f.pyenv(&["migrate", "--restore"]);
    assert_eq!(std::fs::read_to_string(f.root.join("bin").join("pyenv.ps1")).unwrap(), "a newer pyenv.ps1");
    assert!(back.stderr.contains("pyenv.ps1"), "{}", back.stderr);
    assert_eq!(std::fs::read_to_string(&p7).unwrap(), "Set-Alias ll ls\r\n");
}

/// Review focus 5: uninstalling the base removes the junction, not the env's files.
#[test]
fn uninstalling_the_base_keeps_a_linked_venv_env() {
    let f = Fixture::new();
    pyenv_win(&f);
    let env = f.base.join(".pyenv-win-venv").join("envs").join("work");
    std::fs::create_dir_all(&env).unwrap();
    std::fs::write(env.join("pyvenv.cfg"), format!("home = {}\r\n", f.root.join("versions").join("3.12.1").display())).unwrap();
    assert_eq!(f.pyenv(&["migrate"]).code, 0);
    let r = f.pyenv(&["uninstall", "-f", "3.12.1"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(env.join("pyvenv.cfg").is_file(), "the env's files stay");
}
```

- [ ] **Step 2: Run them to verify they fail:** unknown command `migrate`.

- [ ] **Step 3: Implement** `migrate_win.rs`:

```rust
//! `pyenv migrate` / `--restore` (rpyenv-only, spec §9.4): takes over a pyenv-win install,
//! links pyenv-win-venv envs in (user decision 2026-10-08), and undoes it all from a
//! manifest of what it did (plan M6a, R4, R5).

use crate::commands::pwsh_profile::{self, Added};
use crate::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{pathlist, winenv};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv migrate [--restore]

Takes over a pyenv-win install in PYENV_ROOT: moves pyenv-win's launchers out of `bin`
into a backup, takes `bin` off your user PATH, rehashes, links pyenv-win-venv envs in,
and adds the PowerShell profile line. `--restore` undoes exactly what it did.
";

fn dir(ctx: &Ctx) -> PathBuf {
    ctx.root.join(".rpyenv-migrate")
}

fn record(ctx: &Ctx, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir(ctx).join("manifest.txt")) {
        let _ = writeln!(f, "{line}");
    }
}

pub fn migrate(ctx: &Ctx, args: &[&str]) -> Output {
    match args {
        [] => forward(ctx),
        ["--restore"] => restore(ctx),
        ["--help"] => {
            let mut o = Output::new();
            o.stdout.push_str(HELP);
            o
        }
        _ => Output::error(HELP).with_code(1),
    }
}

fn forward(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    if dir(ctx).join("manifest.txt").exists() {
        o.out("pyenv: already migrated; `pyenv migrate --restore` undoes it");
        return o;
    }
    let bin = ctx.root.join("bin");
    let user = winenv::get(winenv::Scope::User, "Path");
    let on_path = user.as_ref().is_some_and(|v| {
        pathlist::split(&v.text).iter().any(|e| pathlist::same(&winenv::expand(e), &bin.display().to_string()))
    });
    let envs_dir = std::env::var_os("USERPROFILE")
        .map(|h| PathBuf::from(h).join(".pyenv-win-venv").join("envs"));
    if crate::commands::setup_win::pyenv_win_bin(ctx).is_none() && !on_path {
        return Output::error(format!("pyenv: no pyenv-win installation found in {}", ctx.root.display())).with_code(1);
    }
    let backup = dir(ctx).join("bin");
    if let Err(e) = std::fs::create_dir_all(&backup) {
        return Output::error(format!("pyenv: cannot create {}: {e}", backup.display()));
    }
    for name in ["pyenv.ps1", "pyenv.bat", "pyenv"] {
        let from = bin.join(name);
        if from.is_file() {
            match std::fs::rename(&from, backup.join(name)) {
                Ok(()) => record(ctx, &format!("bin\t{name}")),
                Err(e) => o.err(format!("pyenv: cannot move {}: {e}", from.display())),
            }
        }
    }
    if let Some(v) = user {
        let target = bin.display().to_string();
        let (text, gone) = pathlist::without(&v.text, &|e| pathlist::same(&winenv::expand(e), &target));
        if !gone.is_empty() {
            match winenv::set_user("Path", &winenv::Value { text, expand: v.expand }) {
                Ok(()) => {
                    for g in &gone {
                        record(ctx, &format!("path\t{g}"));
                    }
                    o.out(format!("pyenv: took {target} off your user PATH"));
                    winenv::broadcast();
                }
                Err(e) => o.err(format!("pyenv: cannot change your user PATH: {e}")),
            }
        }
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    o.stderr.push_str(&r.stderr);
    if let Some(docs) = winenv::documents() {
        for p in pwsh_profile::paths(&docs) {
            if let Ok(Added::Added) = pwsh_profile::add(&p) {
                record(ctx, &format!("profile\t{}", p.display()));
                o.out(format!("pyenv: added the PowerShell line to {}", p.display()));
            }
        }
    }
    if let Some(envs) = envs_dir {
        link_venvs(ctx, &envs, &mut o);
    }
    o.out("pyenv: migrated; open a new terminal. `pyenv migrate --restore` undoes it");
    o
}

/// Links each pyenv-win-venv env whose base is installed here: `versions\<base>\envs\<name>`
/// and `versions\<name>` (spec §10 layout), recording both.
fn link_venvs(ctx: &Ctx, envs: &Path, o: &mut Output) {
    let Ok(entries) = std::fs::read_dir(envs) else {
        return;
    };
    let versions = ctx.versions_dir();
    for e in entries.flatten() {
        let env = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !env.is_dir() || !crate::install::is_safe_win_segment(&name) {
            continue;
        }
        let Some(home) = rpyenv_core::venv::read_cfg(&env, ctx.flavor).and_then(|c| c.home) else {
            continue;
        };
        let base_ok = home.parent().is_some_and(|p| pathlist::same(&p.display().to_string(), &versions.display().to_string()))
            && home.is_dir();
        if !base_ok {
            o.err(format!("pyenv: {} not linked: its Python ({}) isn't installed here", env.display(), home.display()));
            continue;
        }
        let in_base = home.join("envs").join(&name);
        let top = versions.join(&name);
        if top.exists() || in_base.exists() {
            o.err(format!("pyenv: {} not linked: {} is taken", env.display(), name));
            continue;
        }
        let _ = std::fs::create_dir_all(home.join("envs"));
        if rpyenv_core::junction::create(&in_base, &env).is_ok() {
            record(ctx, &format!("junction\t{}\t{}", in_base.display(), env.display()));
            if rpyenv_core::junction::create(&top, &in_base).is_ok() {
                record(ctx, &format!("junction\t{}\t{}", top.display(), in_base.display()));
                o.out(format!("pyenv: linked the pyenv-win-venv env {name}"));
            }
        }
    }
}

fn restore(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    let Ok(text) = std::fs::read_to_string(dir(ctx).join("manifest.txt")) else {
        return Output::error("pyenv: nothing to restore: no migration recorded").with_code(1);
    };
    let lines: Vec<&str> = text.lines().collect();
    // Junctions first, newest first: `versions\<name>` before the one it points to.
    for l in lines.iter().rev() {
        let f: Vec<&str> = l.split('\t').collect();
        if let ["junction", link, target] = f[..] {
            let link = Path::new(link);
            let ours = std::fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink())
                && std::fs::read_link(link).is_ok_and(|t| pathlist::same(&t.display().to_string(), target));
            if ours {
                let _ = std::fs::remove_dir(link);
            } else if link.exists() {
                o.err(format!("pyenv: left {} alone: it isn't the link migrate made", link.display()));
            }
        }
    }
    let bin = ctx.root.join("bin");
    for l in &lines {
        let f: Vec<&str> = l.split('\t').collect();
        match f[..] {
            ["bin", name] => {
                let to = bin.join(name);
                if to.exists() {
                    o.err(format!("pyenv: kept {}, which exists again; the old one stays in {}", to.display(), dir(ctx).join("bin").display()));
                    continue;
                }
                let _ = std::fs::create_dir_all(&bin);
                if let Err(e) = std::fs::rename(dir(ctx).join("bin").join(name), &to) {
                    o.err(format!("pyenv: cannot put back {}: {e}", to.display()));
                }
            }
            ["profile", p] => {
                let _ = pwsh_profile::remove(Path::new(p));
            }
            _ => {}
        }
    }
    let gone: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("path\t")).collect();
    if !gone.is_empty() {
        let v = winenv::get(winenv::Scope::User, "Path").unwrap_or(winenv::Value { text: String::new(), expand: true });
        let mut text = v.text.clone();
        for g in gone.iter().rev() {
            text = pathlist::put_first(&text, g, &winenv::expand).unwrap_or(text);
        }
        if winenv::set_user("Path", &winenv::Value { text, expand: v.expand }).is_ok() {
            winenv::broadcast();
        }
    }
    if o.stderr.is_empty() {
        let _ = std::fs::remove_dir_all(dir(ctx));
    }
    o.out("pyenv: restored pyenv-win; run its `pyenv rehash` in a new terminal to bring back its shims");
    o
}
```

Register `("migrate", migrate_win::migrate)` and `pub mod migrate_win;` (`#[cfg(windows)]`). Add the help topic, and add `migrate` to the `commands` golden as in Task 3 Step 5.

Note for `restore_touches_only_what_migrate_did`: the backup keeps the old file and the manifest stays, because stderr isn't empty. That is intentional. The test checks only what's in the user's files.

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_migrate_win`.
Expected: 5 passed. If `uninstalling_the_base_keeps_a_linked_venv_env` fails, the M4b uninstall cascade follows a junction at `envs\<name>`. Fix it in `uninstall_win.rs`: remove the junction with `remove_dir`, never `remove_dir_all` through it. Ledger it as a finding.

- [ ] **Step 5: Commit:** `git commit -m "pyenv migrate takes over pyenv-win (launchers, PATH, profiles, pyenv-win-venv envs) and --restore undoes exactly what it did"`.

---

### Task 6: The host architecture at run time

**Files:**
- Modify: `crates/rpyenv-core/src/ctx.rs`

**Interfaces:**
- Produces: `ctx::suffix_for_machine(machine: u16) -> &'static str`.

- [ ] **Step 1: Write the failing test** (`ctx.rs` tests):

```rust
    /// Plan M6a, R7: the native machine decides, as pyenv-win sees it.
    #[test]
    fn the_suffix_follows_the_native_machine() {
        assert_eq!(suffix_for_machine(0xAA64), "-arm64"); // IMAGE_FILE_MACHINE_ARM64
        assert_eq!(suffix_for_machine(0x8664), ""); // AMD64
        assert_eq!(suffix_for_machine(0x014C), "-win32"); // I386
        assert_eq!(suffix_for_machine(0), "", "unknown: no suffix");
    }
```

- [ ] **Step 2: Run it to verify it fails:** `suffix_for_machine` not found.

- [ ] **Step 3: Implement:**

```rust
/// pyenv-win's suffix for a native machine type (`IMAGE_FILE_MACHINE_*`).
pub fn suffix_for_machine(machine: u16) -> &'static str {
    match machine {
        0xAA64 => "-arm64",
        0x014C => "-win32",
        _ => "",
    }
}

fn host_arch_suffix() -> &'static str {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
        let (mut process, mut native) = (0u16, 0u16);
        // SAFETY: two out-pointers to locals and this process's pseudo-handle.
        if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) } != 0 {
            return suffix_for_machine(native);
        }
    }
    if cfg!(target_arch = "x86") {
        "-win32"
    } else if cfg!(target_arch = "aarch64") {
        "-arm64"
    } else {
        ""
    }
}
```

`suffix_for_machine` is unused on Linux: add `#[cfg_attr(not(windows), allow(dead_code))]`, or keep it `pub` (pub items aren't flagged).

- [ ] **Step 4: Run** `cargo test -p rpyenv-core --lib ctx::` (and through WSL): PASS.

- [ ] **Step 5: Commit:** `git commit -m "The host architecture is the native machine, read at run time, so an x64 rpyenv on an ARM64 PC picks the -arm64 builds"`.

---

### Task 7: Docs as built

- [ ] **Step 1: Spec.**
  - §9.4: replace "Until M6, `pyenv init --install` refuses on Windows …" with "`pyenv init --install pwsh` adds the same line (M6a); other shells have no startup file rpyenv edits".
  - Add the as-built details: the marker `PYENV_ROOT\.rpyenv-setup`; R6's Program Files rule; the manifest and backup in `PYENV_ROOT\.rpyenv-migrate`; `--restore` removes the profile line and asks for pyenv-win's own `pyenv rehash`; pyenv-win-venv envs are linked, with their files left where they are.
  - §7: the same `init --install` sentence.
- [ ] **Step 2: README:** add `pyenv setup` and `pyenv migrate` to the Windows section in two sentences.
- [ ] **Step 3: Verify:** `cargo fmt --check` and clippy on both OSes; the full suites; the pyenv-win overlay (baseline 225 passed, 3 skipped, 56 xfailed).
- [ ] **Step 4: Commit:** `git commit -m "Docs for M6a as built: setup, migrate, init --install on Windows"`.
