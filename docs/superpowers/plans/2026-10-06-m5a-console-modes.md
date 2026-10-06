# M5a Windows Console Modes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Windows console shim started without a console (Explorer, Task Scheduler, a GUI app) shows a window only once the program prints something (LAZY), or at once if asked to (EAGER). A window the shim opened stays readable after a failure, and closing a window lets the program finish its cleanup.

**Architecture:**
- **The decision stays a pure function** in `rpyenv_core::console`:
  - `choose(Situation) -> ConsoleMode`;
  - `Setting` (`RPYENV_CONSOLE`);
  - `Hold` (`RPYENV_CONSOLE_HOLD`).
- **`pyenv-shim.exe` gets the `consoleAllocationPolicy=detached` manifest.** Windows 11 24H2+ then gives it no console when its caller has none.
- **`winproc` asks for the caller's console only when needed** (`AllocConsoleWithOptions(DEFAULT)`, resolved at runtime):
  - at once (EAGER);
  - on the child's first printable output (LAZY), through a new `conpty` module that runs the child on a pseudo-console and relays output and input.
- **Pure helpers** are in a new `vtscan` module, so their tests run on every OS:
  - the VT trigger scanner;
  - the win32-input-mode key encoder;
  - UTF-8 boundary handling.

**Tech Stack:** Rust 1.96 (edition 2021), `windows-sys` 0.61, the MSVC linker's `/MANIFESTINPUT`, ConPTY (`CreatePseudoConsole`), GitHub Actions `windows-2025` (build 26100) and `windows-2022` (pre-24H2).

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`, §5.3 (Launching on Windows), §13 (variables), §17 open questions 3–4. The design the plan argues from is `docs/windows-lazy-console.md` (the "console doc" below).

## Global Constraints

- **User decisions (2026-10-06):**
  1. M5 is split. **M5a** is the console work (this plan); **M5b** is the live rehash watcher plus removing the rehash lock on SIGINT/SIGTERM.
  2. `RPYENV_CONSOLE` defaults to **`lazy`**.
  3. A window the shim opened stays open after a failure. This is **configurable, on by default, with an optional timeout**: `RPYENV_CONSOLE_HOLD` = `0` off, `N` (a positive whole number) closes after N seconds or on a key, anything else or unset waits for a key.
- **Rule 1 of the console doc: never make it worse.** The shim must not add a window that running the target directly would not create, on any Windows version.
- **Only the console shim** (`pyenv-shim.exe`, after `winproc::set_console_shim()`) uses EAGER or LAZY. `pyenv exec`, plugins and `pyenv-shimw.exe` keep M1's tree (INHERIT, NO-WINDOW, MIRROR).
- **EAGER and LAZY only exist where `AllocConsoleWithOptions` resolves from `kernel32.dll`** (build 26100+). Elsewhere the manifest is ignored and M1's behavior stands.
- **Probe facts (build 26200, `C:\tmp\m5probe`, 2026-10-06).** The method: a child logs `GetConsoleProcessList`, `GetConsoleWindow` and its std handle types for each launch flag.
  - With the manifest, a console-less caller using no flags gives the shim **no console**; `ALLOC_CONSOLE_MODE_DEFAULT` then makes a **new window** (result 1).
  - `CREATE_NO_WINDOW` still gives a **windowless console at startup**, so the shim takes INHERIT.
  - `CREATE_NEW_CONSOLE` gives no console at startup; DEFAULT then makes a new window.
  - `DETACHED_PROCESS` gives no console at startup; DEFAULT then returns **0, no console**.
  - The PEB's `ConsoleHandle` is 0 for both DETACHED and the Explorer-like launch, so it **cannot tell them apart**.
  - Null std handles become console handles after `AllocConsoleWithOptions`.
- **ConPTY facts (same probe):**
  - The child needs `STARTF_USESTDHANDLES` with null handles; without it, the child inherited the shim's own handles and its output bypassed the pseudo-console.
  - ConPTY's output starts with `\e[?9001h\e[?1004h` (win32-input-mode and focus events), then `\e[?25l\e[2J\e[m\e[H`, the text, `\e]0;<exe path>\a`, `\e[?25h`.
- **std's `CommandExt::raw_attribute` / `spawn_with_attributes` are unstable** (issue 114854). LAZY starts the child with `CreatePseudoConsole` plus a raw `CreateProcessW`.
- **Arguments stay byte-for-byte** (spec §5.3): LAZY's command line is `"<program>"` + space + `wincmd::own_tail(1)`. Batch targets fall back to EAGER, because only std's batch escaping is BatBadBut-hardened.
- **Running tests:**
  - Windows: `cargo build --workspace`, then `cargo test --workspace`. e2e tests open console windows on the desktop; that is expected.
  - Linux (the pure modules): through WSL, with a runner script created as in M4b. Never put `$VARS` in `wsl … bash -c`.
- **Test roots:** every test uses `Fixture` (temp `PYENV_ROOT`, `HOME`, `USERPROFILE`). Never run a shim against the real `C:\Users\JM\.pyenv`.
- **Commits:** a one-line `-m`, or `-F C:\tmp\cm_<random>.txt`. No attribution lines.

## Review Focus

These are the five input classes or failure modes most likely to bite a user, none exercised by a task's main tests. Each has a pinning test in its owning task.

1. **`start python x.py` from cmd (`CREATE_NEW_CONSOLE` from a console parent).** Expected: a new window at once, exactly as with `python.exe` (EAGER, not LAZY, not MIRROR). Pinned in Task 2 (`win_a_new_console_request_gets_a_window`).
2. **A program that prints only escape sequences** (a color reset, a title) and exits. Expected: no window, ever. Pinned in Task 3 (scanner fixtures) and Task 4 (`win_lazy_a_silent_program_never_shows_a_window`).
3. **A UTF-8 character split across two pipe reads** in LAZY output. Expected: shown intact. Pinned in Task 3 (`complete_utf8_len`) and Task 5 (non-ASCII argument on screen).
4. **A program ended by Ctrl+C** (`0xC000013A`) in a window the shim opened. Expected: the window closes, no "press any key". Pinned in Task 1 (`should_hold`).
5. **The shim killed while running LAZY.** Expected: the program dies too (Job Object), and no conhost is left behind. Pinned in Task 4 (`win_lazy_killing_the_shim_kills_the_child`).

## Decisions (rulings made while planning)

- **R1, the console-parent rule.** A no-console shim whose parent *has* a console was deliberately detached (`DETACHED_PROCESS`) or asked for a new window (`CREATE_NEW_CONSOLE`). It takes **EAGER**: `ALLOC_CONSOLE_MODE_DEFAULT` returns "no console" for the first, after which the child starts with `DETACHED_PROCESS` (exactly MIRROR), and a window for the second. Its existence is checked with `AttachConsole(ATTACH_PARENT_PROCESS)` + `FreeConsole`, restoring std handles. No undocumented PEB reads.
- **R2, LAZY with a DETACHED caller whose parent has no console.** At the first output, DEFAULT says "no console": the shim keeps draining and discards. A program that reads stdin *before* printing waits; this is the same class as the console doc's deferred "input without prior output", and the workaround is the same (`RPYENV_CONSOLE=eager`).
- **R3, win32-input-mode.** ConPTY asks for it (`?9001h`), so while it is on the shim forwards each `KEY_EVENT_RECORD` as `ESC[Vk;Sc;Uc;Kd;Cs;Rc_`. This is lossless: key-ups, modifiers, and Ctrl+C as `Uc=3`. While it is off, it forwards characters with `ENABLE_VIRTUAL_TERMINAL_INPUT`. This replaces the console doc's VT-only plan.
- **R4, the window title** comes from ConPTY's own `OSC 0` (the target's path), as running `python.exe` gives. No `SetConsoleTitleW`.
- **R5, the initial pseudo-console size is 120×30** (open question 2). This is the default console size on Windows 11 for both conhost and Windows Terminal, and it lasts only until a window appears.
- **R6, no hold after a Ctrl+C exit (`0xC000013A`).** An interruption isn't a failure to read about.
- **R7, errors with nowhere to print.** When the console shim has a message (for example, a version that isn't installed) and no usable handle, it asks for the caller's console (DEFAULT), prints, and holds. With the manifest, a double-clicked script would otherwise fail silently.
- **R8, the window close.** The console handler waits for the child on `CTRL_CLOSE_EVENT`, `CTRL_LOGOFF_EVENT` and `CTRL_SHUTDOWN_EVENT`, closing the pseudo-console first in LAZY. Windows ends the shim at its own timeout, as before.
- **R9, Ctrl+Break in LAZY** reaches the shim (ignored), not the child. This is documented as a limitation.

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/rpyenv-core/src/console.rs` | rewrite | modes, `Situation`, `choose`, `Setting`, `Hold`, `should_hold` (pure) |
| `crates/rpyenv-core/src/vtscan.rs` | create | trigger `Scanner`, `win32_key`, `VtKeys`, `complete_utf8_len` (pure) |
| `crates/rpyenv-core/src/conpty.rs` | create (Windows) | LAZY: `merge_env`, `env_block`, `command_line`, `run`, output/input relay |
| `crates/rpyenv-core/src/winproc.rs` | modify | console-shim flag, `alloc_default`, parent check, EAGER, hold, message console, PTY lock, child watch, close handler |
| `crates/rpyenv-core/src/shim.rs` | modify | `set_console_shim()`, message console + hold on fatal errors |
| `crates/rpyenv-core/src/launch.rs` | modify | `is_batch` → `pub(crate)` |
| `crates/rpyenv-core/src/lib.rs`, `Cargo.toml` | modify | modules, windows-sys features |
| `crates/pyenv-shim/build.rs`, `pyenv-shim.manifest`, `Cargo.toml` | create/modify | the manifest |
| `crates/pyenv-shim/tests/manifest.rs` | create | the manifest is embedded |
| `crates/e2e/src/echo.rs`, `crates/e2e/Cargo.toml` | modify | Explorer-like launcher, screen reader, key typer, window closer, new knobs |
| `crates/e2e/tests/windows.rs` | modify | EAGER, LAZY, hold, close tests |
| `docs/windows-lazy-console.md`, spec, `README.md` | modify | as built |

---

### Task 1: The console decision, `RPYENV_CONSOLE` and `RPYENV_CONSOLE_HOLD` (pure)

**Files:**
- Modify: `crates/rpyenv-core/src/console.rs` (whole file)
- Modify: `crates/rpyenv-core/src/winproc.rs` (`spawn_and_wait`'s call to `choose`)

**Interfaces:**
- Produces:
  - `ConsoleMode::{Inherit, NoWindow, Mirror, Eager, Lazy}` and `ConsoleMode::name() -> &'static str` (`"EAGER"`, `"LAZY"`);
  - `Setting::{Lazy, Eager}`, `Setting::parse(Option<&str>) -> Setting`;
  - `Situation { attached, stdout_redirected, stderr_redirected, detached_policy, parent_has_console: bool, setting: Setting }`;
  - `choose(Situation) -> ConsoleMode`;
  - `Hold::{Off, Key, Seconds(u32)}`, `Hold::parse(Option<&str>) -> Hold`, `Hold::name(self) -> String` (`"off"`, `"key"`, `"3s"`);
  - `should_hold(code: u32) -> bool`;
  - `STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A`.

- [ ] **Step 1: Write the failing tests.** Replace the `tests` module of `console.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn s(attached: bool, out: bool, err: bool, policy: bool, parent: bool, setting: Setting) -> Situation {
        Situation {
            attached,
            stdout_redirected: out,
            stderr_redirected: err,
            detached_policy: policy,
            parent_has_console: parent,
            setting,
        }
    }

    #[test]
    fn the_decision_tree() {
        use ConsoleMode::*;
        use Setting::{Eager as E, Lazy as L};
        let cases = [
            // attached: whatever else holds
            (s(true, false, false, true, false, L), Inherit),
            (s(true, true, true, true, true, E), Inherit),
            // both outputs redirected
            (s(false, true, true, true, false, L), NoWindow),
            (s(false, true, true, false, false, L), NoWindow),
            // no policy (pre-24H2, exec, GUI shim): M1's tree
            (s(false, true, false, false, false, L), Mirror),
            (s(false, false, false, false, true, L), Mirror),
            // policy: a console parent detached us or asked for a new window
            (s(false, false, false, true, true, L), Eager),
            // policy: the setting, then partial redirection
            (s(false, false, false, true, false, E), Eager),
            (s(false, true, false, true, false, L), Eager),
            (s(false, false, true, true, false, L), Eager),
            (s(false, false, false, true, false, L), Lazy),
        ];
        for (situation, mode) in cases {
            assert_eq!(choose(situation), mode, "{situation:?}");
        }
        assert_eq!(NoWindow.name(), "NO-WINDOW");
        assert_eq!(Eager.name(), "EAGER");
        assert_eq!(Lazy.name(), "LAZY");
    }

    #[test]
    fn rpyenv_console_is_lazy_unless_eager() {
        assert_eq!(Setting::parse(None), Setting::Lazy);
        assert_eq!(Setting::parse(Some("")), Setting::Lazy);
        assert_eq!(Setting::parse(Some("lazy")), Setting::Lazy);
        assert_eq!(Setting::parse(Some("bogus")), Setting::Lazy);
        assert_eq!(Setting::parse(Some("eager")), Setting::Eager);
        assert_eq!(Setting::parse(Some(" EAGER ")), Setting::Eager);
    }

    #[test]
    fn rpyenv_console_hold_values() {
        assert_eq!(Hold::parse(None), Hold::Key);
        assert_eq!(Hold::parse(Some("")), Hold::Key);
        assert_eq!(Hold::parse(Some("yes")), Hold::Key);
        assert_eq!(Hold::parse(Some("-1")), Hold::Key);
        assert_eq!(Hold::parse(Some("0")), Hold::Off);
        assert_eq!(Hold::parse(Some(" 0 ")), Hold::Off);
        assert_eq!(Hold::parse(Some("1")), Hold::Seconds(1));
        assert_eq!(Hold::parse(Some("30")), Hold::Seconds(30));
        assert_eq!(Hold::Key.name(), "key");
        assert_eq!(Hold::Off.name(), "off");
        assert_eq!(Hold::Seconds(3).name(), "3s");
    }

    /// Review focus 4: an exit from Ctrl+C isn't a failure to hold a window for.
    #[test]
    fn only_a_real_failure_holds_the_window() {
        assert!(!should_hold(0));
        assert!(should_hold(1));
        assert!(should_hold(0xC000_0005));
        assert!(!should_hold(STATUS_CONTROL_C_EXIT));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rpyenv-core --lib console::`
Expected: compile errors (`Situation`, `Setting`, `Hold`, `should_hold` not found).

- [ ] **Step 3: Write the implementation.** Replace everything above the tests module in `console.rs`:

```rust
//! How a Windows shim starts its child, depending on the console it has
//! (docs/windows-lazy-console.md, rule 2). The decision and the settings are pure, so
//! their tests run on every OS.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleMode {
    /// Attached to a console: the child shares it, and the caller's arrangements stand.
    Inherit,
    /// No console, output redirected: `CREATE_NO_WINDOW`, so no window ever appears.
    NoWindow,
    /// No console and no way to ask for one later (before Windows 11 24H2, `pyenv exec`,
    /// the GUI shim): `DETACHED_PROCESS`, as the caller gave the shim.
    Mirror,
    /// Asks Windows at once for the console the caller wanted
    /// (`ALLOC_CONSOLE_MODE_DEFAULT`), then starts the child on it, or with
    /// `DETACHED_PROCESS` when the caller wanted none.
    Eager,
    /// Runs the child on a pseudo-console and asks for the caller's console only when the
    /// child first prints something.
    Lazy,
}

impl ConsoleMode {
    pub fn name(self) -> &'static str {
        match self {
            ConsoleMode::Inherit => "INHERIT",
            ConsoleMode::NoWindow => "NO-WINDOW",
            ConsoleMode::Mirror => "MIRROR",
            ConsoleMode::Eager => "EAGER",
            ConsoleMode::Lazy => "LAZY",
        }
    }
}

/// `RPYENV_CONSOLE`: what the console shim does when it starts without a console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Lazy,
    Eager,
}

impl Setting {
    /// `eager`, in any case, selects EAGER. Anything else, or unset, is the default, LAZY
    /// (user decision 2026-10-06).
    pub fn parse(value: Option<&str>) -> Setting {
        match value {
            Some(v) if v.trim().eq_ignore_ascii_case("eager") => Setting::Eager,
            _ => Setting::Lazy,
        }
    }
}

/// What the decision needs to know about the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    pub attached: bool,
    pub stdout_redirected: bool,
    pub stderr_redirected: bool,
    /// This is the console shim, whose manifest asks for no console at startup, on a
    /// Windows that honors it (`AllocConsoleWithOptions` exists: build 26100 or later).
    pub detached_policy: bool,
    /// The shim's parent has a console. A shim it started without one was given
    /// `DETACHED_PROCESS` or `CREATE_NEW_CONSOLE`; only EAGER's allocation tells which.
    pub parent_has_console: bool,
    pub setting: Setting,
}

/// The decision tree (docs/windows-lazy-console.md, rule 2, as built in M5a).
pub fn choose(s: Situation) -> ConsoleMode {
    if s.attached {
        ConsoleMode::Inherit
    } else if s.stdout_redirected && s.stderr_redirected {
        ConsoleMode::NoWindow
    } else if !s.detached_policy {
        ConsoleMode::Mirror
    } else if s.parent_has_console
        || s.setting == Setting::Eager
        || s.stdout_redirected
        || s.stderr_redirected
    {
        ConsoleMode::Eager
    } else {
        ConsoleMode::Lazy
    }
}

/// `RPYENV_CONSOLE_HOLD`: whether a window the shim opened stays on screen after the
/// child fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Off,
    Key,
    Seconds(u32),
}

impl Hold {
    /// `0` turns it off. A positive whole number closes the window after that many
    /// seconds, or sooner on a key. Anything else, or unset, waits for a key (user decision
    /// 2026-10-06).
    pub fn parse(value: Option<&str>) -> Hold {
        match value.map(str::trim).and_then(|v| v.parse::<u32>().ok()) {
            Some(0) => Hold::Off,
            Some(n) => Hold::Seconds(n),
            None => Hold::Key,
        }
    }

    pub fn name(self) -> String {
        match self {
            Hold::Off => "off".to_string(),
            Hold::Key => "key".to_string(),
            Hold::Seconds(n) => format!("{n}s"),
        }
    }
}

/// `STATUS_CONTROL_C_EXIT`: how a program ended by Ctrl+C exits.
pub const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// Whether an exit code is a failure worth keeping the window open for: non-zero, and
/// not Ctrl+C's.
pub fn should_hold(code: u32) -> bool {
    code != 0 && code != STATUS_CONTROL_C_EXIT
}
```

- [ ] **Step 4: Keep `winproc` compiling with M1's behavior.** In `crates/rpyenv-core/src/winproc.rs`, `spawn_and_wait`, replace

```rust
    let mode = console::choose(p.attached, p.stdout_redirected, p.stderr_redirected);
```

with

```rust
    let mode = console::choose(console::Situation {
        attached: p.attached,
        stdout_redirected: p.stdout_redirected,
        stderr_redirected: p.stderr_redirected,
        detached_policy: false,
        parent_has_console: false,
        setting: console::Setting::Lazy,
    });
```

and add `ConsoleMode::Eager | ConsoleMode::Lazy => unreachable!("no policy before Task 2"),` as a match arm after `ConsoleMode::Mirror`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p rpyenv-core --lib console::`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/rpyenv-core/src/console.rs crates/rpyenv-core/src/winproc.rs
git commit -m "Console decision for M5a: EAGER and LAZY modes, RPYENV_CONSOLE, RPYENV_CONSOLE_HOLD"
```

---

### Task 2: The manifest, EAGER, holding a window, and errors with nowhere to print

**Files:**
- Create: `crates/pyenv-shim/build.rs`, `crates/pyenv-shim/pyenv-shim.manifest`, `crates/pyenv-shim/tests/manifest.rs`
- Modify: `crates/pyenv-shim/Cargo.toml`
- Modify: `crates/rpyenv-core/Cargo.toml` (features `Win32_System_LibraryLoader`, `Win32_System_Pipes`)
- Modify: `crates/rpyenv-core/src/winproc.rs`, `crates/rpyenv-core/src/shim.rs`, `crates/rpyenv-core/src/launch.rs:431`
- Modify: `crates/e2e/src/echo.rs`, `crates/e2e/Cargo.toml` (feature `Win32_Storage_FileSystem`)
- Test: `crates/e2e/tests/windows.rs`

**Interfaces:**
- Consumes: Task 1's `console::{choose, Situation, Setting, Hold, should_hold}`.
- Produces, in `winproc`:
  - `pub fn set_console_shim()`;
  - `pub fn alloc_console_available() -> bool`;
  - `pub enum Alloc { None, New, Existing }`, `pub fn alloc_default() -> Alloc` (it logs `console=<none|new|existing> pid=<pid>`);
  - `pub fn open_console(name: &str) -> Option<HANDLE>`;
  - `pub fn hold_after(code: u32)` (it logs `hold=<name>` just before waiting);
  - `pub fn console_for_message(stderr: bool) -> bool`.
- Produces in `launch`: `pub(crate) fn is_batch(&Path) -> bool`.
- Produces the mode log line `mode=<NAME> pid=<pid> program=<path>`.
- Produces argv-echo knobs:
  - `ARGV_ECHO_SPAWN=<exe>` + `ARGV_ECHO_SPAWN_EXIT=<file>`: the Explorer-like launcher, which passes its own arguments on;
  - `ARGV_ECHO_SCREEN_PID=<pid>`: prints that console's text;
  - `ARGV_ECHO_TYPE_PID=<pid>` + `ARGV_ECHO_TYPE=<text>`: `\x03` is Ctrl+C, `\x1a` is Ctrl+Z, `\r` is Enter;
  - `ARGV_ECHO_FIRST=<file>`: touched at the very start;
  - `ARGV_ECHO_DELAY_MS`: a sleep before any output.
- Produces e2e helpers in `windows.rs`: `launch_like_explorer`, `explorer_exit`, `wait_log`, `shim_pid`, `screen`, `type_keys`, `api()`.

- [ ] **Step 1: Write the failing manifest test.** Create `crates/pyenv-shim/tests/manifest.rs`:

```rust
//! The console shim carries `consoleAllocationPolicy=detached` (spec §5.3, M5a); the GUI
//! shim doesn't need it and doesn't get it.
#![cfg(windows)]

use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
    LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
};

/// The text of `exe`'s embedded manifest (resource type 24, ID 1), or empty.
fn manifest(exe: &str) -> String {
    let wide: Vec<u16> = exe.encode_utf16().chain(Some(0)).collect();
    // SAFETY: maps the file as data only and reads one resource's bytes, whose size
    // SizeofResource gives; the module is never freed, which only leaks in a test.
    unsafe {
        let module = LoadLibraryExW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        );
        assert!(!module.is_null(), "cannot map {exe}");
        let res = FindResourceW(module, 1 as _, 24 as _);
        if res.is_null() {
            return String::new();
        }
        let size = SizeofResource(module, res) as usize;
        let data = LockResource(LoadResource(module, res)) as *const u8;
        String::from_utf8_lossy(std::slice::from_raw_parts(data, size)).into_owned()
    }
}

#[test]
fn the_console_shim_asks_for_no_console_at_startup() {
    let text = manifest(env!("CARGO_BIN_EXE_pyenv-shim"));
    assert!(text.contains("consoleAllocationPolicy"), "{text}");
    assert!(text.contains(">detached<"), "{text}");
}
```

Add to `crates/pyenv-shim/Cargo.toml`:

```toml
[target.'cfg(windows)'.dev-dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_LibraryLoader"] }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p pyenv-shim --test manifest`
Expected: FAIL, with an assertion message showing a manifest without `consoleAllocationPolicy` (or empty).

- [ ] **Step 3: Embed the manifest.** Create `crates/pyenv-shim/pyenv-shim.manifest`:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application>
    <windowsSettings>
      <consoleAllocationPolicy xmlns="http://schemas.microsoft.com/SMI/2024/WindowsSettings">detached</consoleAllocationPolicy>
    </windowsSettings>
  </application>
</assembly>
```

Create `crates/pyenv-shim/build.rs`:

```rust
//! Embeds `pyenv-shim.manifest` (spec §5.3, M5a): with `consoleAllocationPolicy=detached`,
//! Windows 11 24H2 and later give the shim no console when its caller has none, and the
//! shim asks for one itself (EAGER) or when the program prints (LAZY). Older Windows
//! ignores the setting.
fn main() {
    println!("cargo::rerun-if-changed=pyenv-shim.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo::rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo::rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\pyenv-shim.manifest");
    }
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test -p pyenv-shim --test manifest`
Expected: 1 passed.

- [ ] **Step 5: Add the argv-echo knobs.** In `crates/e2e/Cargo.toml`, add `"Win32_Storage_FileSystem"` to the Windows `windows-sys` features. In `crates/e2e/src/echo.rs`, add these functions above `pub fn main()`:

```rust
/// Starts `exe` as Explorer does: from this console-less helper (the test starts it with
/// `DETACHED_PROCESS`), with null standard handles and no creation flags, passing this
/// helper's own arguments on. Writes the exit code to `ARGV_ECHO_SPAWN_EXIT`.
#[cfg(windows)]
fn spawn_like_explorer(exe: &std::ffi::OsStr) -> i32 {
    use windows_sys::Win32::System::Console::{
        SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: clears this helper's own standard handles; std then passes null handles on.
    unsafe {
        for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetStdHandle(h, std::ptr::null_mut());
        }
    }
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .env_remove("ARGV_ECHO_SPAWN")
        .env_remove("ARGV_ECHO_SPAWN_EXIT")
        .status();
    let code = match status {
        Ok(s) => i64::from(s.code().unwrap_or(-1) as u32),
        Err(_) => -2,
    };
    if let Some(p) = std::env::var_os("ARGV_ECHO_SPAWN_EXIT") {
        let _ = std::fs::write(p, code.to_string());
    }
    0
}

/// Leaves this helper's console and attaches to process `pid`'s, keeping stdout (the
/// test's pipe) as it was. Returns that console's handle named `name` (`CONOUT$` or
/// `CONIN$`), or an exit code.
#[cfg(windows)]
fn attach_to(pid: u32, name: &str) -> Result<windows_sys::Win32::Foundation::HANDLE, i32> {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GetStdHandle, SetStdHandle, STD_OUTPUT_HANDLE,
    };
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: console attachment and standard-handle calls on this helper only; the name
    // is NUL-terminated.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return Err(2);
        }
        SetStdHandle(STD_OUTPUT_HANDLE, out);
        let h = CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(3);
        }
        Ok(h)
    }
}

/// Prints the text on process `pid`'s console, row by row up to the cursor, each row's
/// trailing blanks removed.
#[cfg(windows)]
fn read_screen(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{
        GetConsoleScreenBufferInfo, ReadConsoleOutputCharacterW, CONSOLE_SCREEN_BUFFER_INFO, COORD,
    };
    let out = match attach_to(pid, "CONOUT$") {
        Ok(h) => h,
        Err(code) => return code,
    };
    let mut text = String::new();
    // SAFETY: reads the attached console's buffer into a local of the width it reports.
    unsafe {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(out, &mut info) == 0 {
            return 4;
        }
        let width = info.dwSize.X as usize;
        let mut row = vec![0u16; width];
        for y in 0..=info.dwCursorPosition.Y {
            let mut n = 0u32;
            ReadConsoleOutputCharacterW(out, row.as_mut_ptr(), width as u32, COORD { X: 0, Y: y }, &mut n);
            text.push_str(String::from_utf16_lossy(&row[..n as usize]).trim_end());
            text.push('\n');
        }
    }
    print!("{text}");
    0
}

/// Writes `text` as key presses into process `pid`'s console: `\x03` is Ctrl+C, `\x1a`
/// Ctrl+Z, `\r` Enter, anything else that character.
#[cfg(windows)]
fn type_keys(pid: u32, text: &str) -> i32 {
    use windows_sys::Win32::System::Console::{
        WriteConsoleInputW, INPUT_RECORD, KEY_EVENT, LEFT_CTRL_PRESSED,
    };
    let conin = match attach_to(pid, "CONIN$") {
        Ok(h) => h,
        Err(code) => return code,
    };
    let mut records = Vec::new();
    for c in text.encode_utf16() {
        let (vk, sc, ctrl) = match c {
            0x03 => (0x43, 0x2E, LEFT_CTRL_PRESSED),
            0x1A => (0x5A, 0x2C, LEFT_CTRL_PRESSED),
            0x0D => (0x0D, 0x1C, 0),
            _ => (0, 0, 0),
        };
        for down in [1, 0] {
            // SAFETY: a zeroed INPUT_RECORD is valid; only the key-event member is set.
            let mut r: INPUT_RECORD = unsafe { std::mem::zeroed() };
            r.EventType = KEY_EVENT as u16;
            // SAFETY: writes the key-event member of the union just tagged KEY_EVENT.
            unsafe {
                r.Event.KeyEvent.bKeyDown = down;
                r.Event.KeyEvent.wRepeatCount = 1;
                r.Event.KeyEvent.wVirtualKeyCode = vk;
                r.Event.KeyEvent.wVirtualScanCode = sc;
                r.Event.KeyEvent.uChar.UnicodeChar = c;
                r.Event.KeyEvent.dwControlKeyState = ctrl;
            }
            records.push(r);
        }
    }
    let mut written = 0u32;
    // SAFETY: writes `records.len()` initialized records to the attached console's input.
    let ok = unsafe { WriteConsoleInputW(conin, records.as_ptr(), records.len() as u32, &mut written) };
    if ok == 0 { 4 } else { 0 }
}
```

At the top of `main()`, after the `ARGV_ECHO_CTRLC_PID` block, add:

```rust
    #[cfg(windows)]
    if let Some(exe) = std::env::var_os("ARGV_ECHO_SPAWN") {
        std::process::exit(spawn_like_explorer(&exe));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_SCREEN_PID").ok().and_then(|p| p.parse::<u32>().ok()) {
        std::process::exit(read_screen(pid));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_TYPE_PID").ok().and_then(|p| p.parse::<u32>().ok()) {
        std::process::exit(type_keys(pid, &std::env::var("ARGV_ECHO_TYPE").unwrap_or_default()));
    }
```

Right after the `ARGV_ECHO_CATCH_BREAK` block, add:

```rust
    if let Some(p) = std::env::var_os("ARGV_ECHO_FIRST") {
        let _ = std::fs::write(&p, b"");
    }
    if let Some(ms) = std::env::var("ARGV_ECHO_DELAY_MS").ok().and_then(|v| v.parse().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
```

Extend the module doc comment of `crates/e2e/src/bin/argv_echo.rs` with one sentence per new knob. Use the wording from this task's **Produces** list.

- [ ] **Step 6: Write the failing e2e tests.** In `crates/e2e/tests/windows.rs`, below `console_env`, add the helpers and tests:

```rust
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Whether this Windows honors the shim's manifest (build 26100 or later).
fn api() -> bool {
    rpyenv_core::winproc::alloc_console_available()
}

static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A path under `base` that no other call in this test process returns.
fn unique(base: &std::path::Path, stem: &str) -> std::path::PathBuf {
    base.join(format!("{stem}-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)))
}

/// Starts shim `name` as Explorer does (docs/windows-lazy-console.md, "Testing"): a helper
/// started with `DETACHED_PROCESS` has no console, clears its standard handles and starts
/// the shim with no flags and `args`. Returns the helper and the file the shim's exit code
/// goes to.
fn launch_like_explorer(
    f: &Fixture,
    name: &str,
    env: &[(&str, &OsStr)],
    args: &[&str],
) -> (std::process::Child, std::path::PathBuf) {
    let exit = unique(&f.base, "exit");
    let helper = f
        .command(&built("argv-echo"), env)
        .env("ARGV_ECHO_SPAWN", f.shim(name))
        .env("ARGV_ECHO_SPAWN_EXIT", &exit)
        .args(args)
        .creation_flags(DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    (helper, exit)
}

/// Waits for the helper; the shim's exit code, as a DWORD.
fn explorer_exit(mut helper: std::process::Child, exit: &std::path::Path) -> i64 {
    helper.wait().unwrap();
    std::fs::read_to_string(exit)
        .unwrap_or_else(|_| panic!("{} was never written", exit.display()))
        .trim()
        .parse()
        .unwrap()
}

/// Polls the debug log until it contains `needle` (20 s at most); returns the log.
fn wait_log(log: &std::path::Path, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "no {needle:?} in:\n{text}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The shim's process ID: the first `pid=` in its log.
fn shim_pid(log_text: &str) -> u32 {
    log_text
        .split_whitespace()
        .find_map(|w| w.strip_prefix("pid="))
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no pid= in:\n{log_text}"))
}

/// The text on process `pid`'s console.
fn screen(pid: u32) -> String {
    let out = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_SCREEN_PID", pid.to_string())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "cannot read the console of {pid}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Types `text` into process `pid`'s console.
fn type_keys(pid: u32, text: &str) {
    let status = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_TYPE_PID", pid.to_string())
        .env("ARGV_ECHO_TYPE", text)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0), "cannot type into the console of {pid}");
}

/// Explorer-like start with `RPYENV_CONSOLE=eager`: the window is there before the
/// program prints anything.
#[test]
fn win_eager_makes_the_window_at_startup() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let first = f.base.join("first");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
            ("ARGV_ECHO_FIRST", first.as_os_str()),
            ("ARGV_ECHO_DELAY_MS", v("1500")),
        ],
        &[],
    );
    wait_for(&first);
    if !api() {
        assert_eq!(explorer_exit(helper, &exit), 0);
        assert_eq!(mode_in(&log), "INHERIT");
        return;
    }
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(mode_in(&log), "EAGER");
    assert!(text.contains("console=new"), "{text}");
    assert_eq!(explorer_exit(helper, &exit), 0);
}

/// Review focus 1: `start python` from a terminal (`CREATE_NEW_CONSOLE`) gets its new
/// window at once, as `python.exe` does.
#[test]
fn win_a_new_console_request_gets_a_window() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let status = f
        .shim_command("python", &[("PYENV_VERSION", v("3.9.1")), ("RPYENV_DEBUG_LOG", log.as_os_str())])
        .creation_flags(CREATE_NEW_CONSOLE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    if api() {
        assert_eq!(mode_in(&log), "EAGER");
        assert!(std::fs::read_to_string(&log).unwrap().contains("console=new"));
    } else {
        assert_eq!(mode_in(&log), "INHERIT");
    }
}

/// A failed program in a window EAGER opened: the window stays for the seconds
/// `RPYENV_CONSOLE_HOLD` sets, and the exit code comes back.
#[test]
fn win_eager_holds_a_failed_programs_window_for_the_set_seconds() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let start = std::time::Instant::now();
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
            ("RPYENV_CONSOLE_HOLD", v("1")),
            ("ARGV_ECHO_EXIT", v("3")),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 3);
    if api() {
        assert!(start.elapsed() >= Duration::from_secs(1));
        assert!(std::fs::read_to_string(&log).unwrap().contains("hold=1s"));
    }
}

/// A double-clicked script whose version isn't installed: the shim asks for the window
/// the caller wanted, shows why, and waits for a key.
#[test]
fn win_an_error_with_nowhere_to_print_gets_a_window_that_waits() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[("PYENV_VERSION", v("9.9.9")), ("RPYENV_DEBUG_LOG", log.as_os_str())],
        &[],
    );
    if !api() {
        assert_ne!(explorer_exit(helper, &exit), 0);
        return;
    }
    let text = wait_log(&log, "hold=key");
    let pid = shim_pid(&text);
    let shown = screen(pid);
    assert!(shown.contains("9.9.9"), "{shown}");
    type_keys(pid, "x");
    assert_ne!(explorer_exit(helper, &exit), 0);
}
```

In `win_no_console_without_both_outputs_redirected_mirrors`, replace `assert_eq!(mode_in(&log), "MIRROR");` with:

```rust
    // R1: a console parent detached the shim; EAGER asks, gets no console, and starts the
    // child as MIRROR would.
    assert_eq!(mode_in(&log), if api() { "EAGER" } else { "MIRROR" });
```

- [ ] **Step 7: Run them to verify they fail**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows -- win_eager win_a_new_console win_an_error win_no_console`
Expected on this host (build 26200): `win_eager_makes_the_window_at_startup` fails on `mode_in == "EAGER"` (it logs `MIRROR`, or the `unreachable!`). The others fail on their EAGER/`console=new`/`hold` assertions.

- [ ] **Step 8: Implement EAGER, the hold and the message console.** In `crates/rpyenv-core/Cargo.toml`, add `"Win32_System_LibraryLoader"` and `"Win32_System_Pipes"` to the `windows-sys` features. In `launch.rs`, make `fn is_batch` `pub(crate) fn is_batch`. In `winproc.rs`:

Add imports:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use windows_sys::core::HRESULT;
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, FlushConsoleInputBuffer, FreeConsole, ReadConsoleInputW, SetConsoleMode,
    SetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS, INPUT_RECORD, KEY_EVENT,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
```

Add, above `spawn_and_wait`:

```rust
static CONSOLE_SHIM: AtomicBool = AtomicBool::new(false);

/// Marks this process as the console shim, whose manifest asks Windows for no console at
/// startup (spec §5.3). Only it starts children in EAGER or LAZY mode, and only it asks for
/// a console to show an error in.
pub fn set_console_shim() {
    CONSOLE_SHIM.store(true, Ordering::SeqCst);
}

fn console_shim() -> bool {
    CONSOLE_SHIM.load(Ordering::SeqCst)
}

/// `ALLOC_CONSOLE_OPTIONS` (ConsoleApi.h, build 26100).
#[repr(C)]
struct AllocConsoleOptions {
    mode: i32,
    use_show_window: BOOL,
    show_window: u16,
}

type AllocConsoleWithOptionsFn =
    unsafe extern "system" fn(*mut AllocConsoleOptions, *mut i32) -> HRESULT;

/// `AllocConsoleWithOptions`, looked up once: windows-sys 0.61 doesn't declare it, and
/// Windows before build 26100 doesn't have it.
fn alloc_fn() -> Option<AllocConsoleWithOptionsFn> {
    static ADDR: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *ADDR.get_or_init(|| {
        let name: Vec<u16> = "kernel32.dll".encode_utf16().chain(Some(0)).collect();
        // SAFETY: kernel32 is loaded in every process; both names are NUL-terminated.
        unsafe {
            let k = GetModuleHandleW(name.as_ptr());
            if k.is_null() {
                return None;
            }
            GetProcAddress(k, b"AllocConsoleWithOptions\0".as_ptr()).map(|f| f as usize)
        }
    })?;
    // SAFETY: the address is kernel32's AllocConsoleWithOptions, whose signature this type
    // spells out.
    Some(unsafe { std::mem::transmute::<usize, AllocConsoleWithOptionsFn>(addr) })
}

/// Whether this Windows has `AllocConsoleWithOptions` (Windows 11 24H2, Server 2025),
/// and so honors the shim's `consoleAllocationPolicy`.
pub fn alloc_console_available() -> bool {
    alloc_fn().is_some()
}

/// What asking for the caller's console gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alloc {
    /// The caller wanted none (`DETACHED_PROCESS`), or the call isn't available.
    None,
    /// A new console: a window, or a windowless one if the caller asked for that.
    New,
    /// The console this process already had.
    Existing,
}

impl Alloc {
    pub fn name(self) -> &'static str {
        match self {
            Alloc::None => "none",
            Alloc::New => "new",
            Alloc::Existing => "existing",
        }
    }
}

/// `AllocConsoleWithOptions(ALLOC_CONSOLE_MODE_DEFAULT)`: the console the caller asked
/// for when it started this process, created now. Logs `console=<what> pid=<id>`.
pub fn alloc_default() -> Alloc {
    let alloc = match alloc_fn() {
        None => Alloc::None,
        Some(f) => {
            let mut options = AllocConsoleOptions { mode: 0, use_show_window: 0, show_window: 0 };
            let mut result = 0i32;
            // SAFETY: both pointers are to locals of the documented layouts.
            let hr = unsafe { f(&mut options, &mut result) };
            match (hr, result) {
                (0, 1) => Alloc::New,
                (0, 2) => Alloc::Existing,
                _ => Alloc::None,
            }
        }
    };
    debuglog::append(&format!("console={} pid={}", alloc.name(), std::process::id()));
    alloc
}

/// Whether this process is attached to a console.
fn attached() -> bool {
    let mut one = 0u32;
    // SAFETY: asks for at most one process id into a one-element buffer.
    unsafe { GetConsoleProcessList(&mut one, 1) != 0 }
}

/// Whether the parent process has a console (R1): attaches to it and leaves at once,
/// keeping this process's standard handles as they were.
fn parent_has_console() -> bool {
    // SAFETY: reads and restores this process's own standard handles; the console
    // attachment it changes is none before and after.
    unsafe {
        let saved = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].map(|w| (w, GetStdHandle(w)));
        let found = AttachConsole(ATTACH_PARENT_PROCESS) != 0;
        if found {
            FreeConsole();
        }
        for (w, h) in saved {
            SetStdHandle(w, h);
        }
        found
    }
}

/// Opens this process's console input (`CONIN$`) or screen (`CONOUT$`).
pub fn open_console(name: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated name; the handle is the caller's to close.
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    (h != INVALID_HANDLE_VALUE).then_some(h)
}

/// After a child ended with `code` in a window this shim opened: keeps the window on
/// screen as `RPYENV_CONSOLE_HOLD` says, when `code` is a failure (console doc, LAZY step 6).
pub fn hold_after(code: u32) {
    if !console::should_hold(code) {
        return;
    }
    let hold = console::Hold::parse(std::env::var("RPYENV_CONSOLE_HOLD").ok().as_deref());
    let wait_ms = match hold {
        console::Hold::Off => {
            debuglog::append("hold=off");
            return;
        }
        console::Hold::Key => INFINITE,
        console::Hold::Seconds(n) => n.saturating_mul(1000),
    };
    let shown = if code > 0xFFFF { format!("0x{code:08X}") } else { code.to_string() };
    let text = match hold {
        console::Hold::Seconds(n) => {
            format!("\r\n[exited with code {shown}] This window closes in {n} s, or press any key.")
        }
        _ => format!("\r\n[exited with code {shown}] Press any key to close this window."),
    };
    let (Some(conin), Some(conout)) = (open_console("CONIN$"), open_console("CONOUT$")) else {
        return;
    };
    let wide: Vec<u16> = text.encode_utf16().collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(u64::from(wait_ms));
    // SAFETY: console calls on handles opened above, with local buffers of the sizes given.
    unsafe {
        let mut written = 0u32;
        WriteConsoleW(conout, wide.as_ptr().cast(), wide.len() as u32, &mut written, std::ptr::null());
        // Ctrl+C is a key here, not a signal.
        SetConsoleMode(conin, 0);
        FlushConsoleInputBuffer(conin);
        debuglog::append(&format!("hold={}", hold.name()));
        loop {
            let left = if wait_ms == INFINITE {
                INFINITE
            } else {
                deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as u32
            };
            if WaitForSingleObject(conin, left) != WAIT_OBJECT_0 {
                break;
            }
            let mut records: [INPUT_RECORD; 16] = std::mem::zeroed();
            let mut n = 0u32;
            if ReadConsoleInputW(conin, records.as_mut_ptr(), 16, &mut n) == 0 {
                break;
            }
            if records[..n as usize]
                .iter()
                .any(|r| u32::from(r.EventType) == KEY_EVENT && r.Event.KeyEvent.bKeyDown != 0)
            {
                break;
            }
        }
        CloseHandle(conin);
        CloseHandle(conout);
    }
}

/// For the console shim with a message and no usable handle to print it on (R7): asks
/// for the console the caller wanted, so a double-clicked script shows why it failed.
/// True when that made a new console, which the caller then holds open.
pub fn console_for_message(stderr: bool) -> bool {
    if !console_shim() || std_handle_usable(stderr) || attached() || !alloc_console_available() {
        return false;
    }
    alloc_default() == Alloc::New
}
```

`HANDLE` is already imported. Remove the now-duplicate `GetConsoleProcessList` body from `probe()` by calling `attached()`.

Replace the body of `spawn_and_wait` with:

```rust
pub fn spawn_and_wait(cmd: &mut Command, program: &Path) -> io::Result<ExitStatus> {
    let p = probe();
    let policy = console_shim() && alloc_console_available();
    let parent = policy
        && !p.attached
        && !(p.stdout_redirected && p.stderr_redirected)
        && parent_has_console();
    let mut mode = console::choose(console::Situation {
        attached: p.attached,
        stdout_redirected: p.stdout_redirected,
        stderr_redirected: p.stderr_redirected,
        detached_policy: policy,
        parent_has_console: parent,
        setting: console::Setting::parse(std::env::var("RPYENV_CONSOLE").ok().as_deref()),
    });
    // LAZY starts the child itself, with the raw command line; std's batch escaping is the
    // only safe way to start a batch file, so a batch target takes EAGER.
    if mode == ConsoleMode::Lazy && crate::launch::is_batch(program) {
        debuglog::append("lazy=batch");
        mode = ConsoleMode::Eager;
    }
    debuglog::append(&format!(
        "mode={} pid={} program={}",
        mode.name(),
        std::process::id(),
        program.display()
    ));
    let mut new_window = false;
    match mode {
        ConsoleMode::Inherit => {}
        ConsoleMode::NoWindow => {
            cmd.creation_flags(CREATE_NO_WINDOW);
            if !p.stdin_provided {
                cmd.stdin(Stdio::null());
            }
        }
        ConsoleMode::Mirror => {
            cmd.creation_flags(DETACHED_PROCESS);
        }
        // LAZY is EAGER until Task 4 adds the pseudo-console.
        ConsoleMode::Eager | ConsoleMode::Lazy => match alloc_default() {
            Alloc::New => new_window = true,
            Alloc::Existing => {}
            Alloc::None => {
                cmd.creation_flags(DETACHED_PROCESS);
            }
        },
    }
    ignore_console_events();
    let job = Job::new();
    let mut child = cmd.spawn()?;
    // Without the job, killing the shim leaves the child running (D-45); say so in the log.
    if !job.as_ref().is_some_and(|j| j.assign(&child)) {
        debuglog::append("job=none");
    }
    let status = child.wait();
    drop(job);
    if let (true, Ok(s)) = (new_window, &status) {
        hold_after(s.code().unwrap_or(1) as u32);
    }
    status
}
```

- [ ] **Step 9: Wire the shim.** In `crates/rpyenv-core/src/shim.rs`, start `pub fn main()` with:

```rust
    #[cfg(windows)]
    crate::winproc::set_console_shim();
```

Make `say` return whether it made a console:

```rust
/// Prints what the shim has to say on its stream, and logs it to `RPYENV_DEBUG_LOG`. The
/// GUI shim, when that stream isn't a usable handle, shows a message box instead
/// (plan decision 4). The console shim asks for the caller's console (R7); true when that
/// made a new one.
fn say(gui: bool, flavor: Flavor, lines: &[String], to_stderr: bool) -> bool {
    if lines.is_empty() {
        return false;
    }
    for line in lines {
        crate::debuglog::append(line);
    }
    #[cfg(windows)]
    if gui && !crate::winproc::std_handle_usable(to_stderr) {
        crate::winproc::message_box(&lines.join("\r\n"));
        return false;
    }
    #[cfg(windows)]
    let made = !gui && crate::winproc::console_for_message(to_stderr);
    #[cfg(not(windows))]
    let made = false;
    crate::lookup::Report {
        lines: lines.to_vec(),
        stderr: to_stderr,
        code: 0,
    }
    .emit(flavor);
    made
}

/// A fatal message: printed as `say` does, then the window it opened held (R7).
fn fail(gui: bool, flavor: Flavor, lines: &[String], to_stderr: bool, code: i32) -> i32 {
    let made = say(gui, flavor, lines, to_stderr);
    #[cfg(windows)]
    if made {
        crate::winproc::hold_after(code as u32);
    }
    let _ = made;
    code
}
```

In `run`, turn each fatal `say(...); return 1;` / `say(...); report.code` / `say(...); r.code` into `return fail(gui, flavor, &lines, true, 1);`, `fail(gui, flavor, &report.lines, report.stderr, report.code)` and `fail(gui, flavor, &r.lines, r.stderr, r.code)`. The warning call becomes `let _ = say(false, flavor, &plan.warnings, true);`.

- [ ] **Step 10: Run the tests to verify they pass**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows` then `cargo test -p rpyenv-core`
Expected: all pass. The four new tests pass on this host, and `win_no_console_without_both_outputs_redirected_mirrors` logs `EAGER` with `console=0`.

- [ ] **Step 11: Commit**

```bash
git add crates/pyenv-shim crates/rpyenv-core crates/e2e
git commit -m "EAGER console mode: the shim's consoleAllocationPolicy manifest, the caller's console on request, a held window after a failure, errors shown in a window"
```

---

### Task 3: The VT trigger scanner and the input encoders (pure)

**Files:**
- Create: `crates/rpyenv-core/src/vtscan.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod vtscan;`)

**Interfaces:**
- Produces:
  - `Scanner::new()`, `Scanner::feed(&mut self, &[u8]) -> Option<usize>` (the index of the first printable byte in this chunk), `Scanner::win32_input(&self) -> bool`;
  - `win32_key(vk: u16, sc: u16, uc: u16, down: bool, state: u32, repeat: u16) -> String`;
  - `VtKeys::default()`, `VtKeys::push(&mut self, unit: u16, repeat: u16, out: &mut Vec<u8>)`;
  - `complete_utf8_len(&[u8]) -> usize`.

- [ ] **Step 1: Write the failing tests.** Create `crates/rpyenv-core/src/vtscan.rs` with only the tests module and `#[allow(unused)]` stubs:

```rust
//! The pure parts of LAZY mode (docs/windows-lazy-console.md, LAZY steps 3 and 5): finding
//! the first printable character in a pseudo-console's VT output, and encoding key
//! presses for its input. Pure, so the tests run on every OS.

#[cfg(test)]
mod tests {
    use super::*;

    /// ConPTY's startup output, captured on build 26200 (plan M5a, probe 4).
    const STARTUP: &[u8] = b"\x1b[?9001h\x1b[?1004h";
    const HELLO: &[u8] =
        b"\x1b[?25l\x1b[2J\x1b[m\x1b[Hhello\r\n\x1b]0;C:\\tmp\\m5probe\\target\\release\\plain.exe\x07\x1b[?25h";
    const SILENT_END: &[u8] =
        b"\x1b[?25l\x1b[?9001l\x1b[?1004l\x1b[2J\x1b[m\x1b[H\x1b]0;C:\\Windows\\System32\\cmd.exe\x07\x1b[?25h";

    #[test]
    fn conpty_startup_is_not_a_trigger_and_turns_win32_input_on() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(STARTUP), None);
        assert!(s.win32_input());
    }

    #[test]
    fn the_first_printable_character_is_the_trigger() {
        let mut s = Scanner::new();
        s.feed(STARTUP);
        assert_eq!(s.feed(HELLO), Some(16));
    }

    /// Review focus 2: escapes, a title and a reset only: never a trigger.
    #[test]
    fn a_silent_program_never_triggers_and_turns_win32_input_off() {
        let mut s = Scanner::new();
        s.feed(STARTUP);
        assert_eq!(s.feed(SILENT_END), None);
        assert!(!s.win32_input());
    }

    #[test]
    fn sequences_split_across_reads() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b]0;tit"), None);
        assert_eq!(s.feed(b"le\x07 x"), Some(4));
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b["), None);
        assert_eq!(s.feed(b"?9001"), None);
        assert_eq!(s.feed(b"h"), None);
        assert!(s.win32_input());
    }

    #[test]
    fn string_sequences_and_charset_escapes_are_skipped() {
        assert_eq!(Scanner::new().feed(b"\x1bP1$r0m\x1b\\Z"), Some(9));
        assert_eq!(Scanner::new().feed(b"\x1b]2;t\x1b\\Q"), Some(7));
        assert_eq!(Scanner::new().feed(b"\x1b(BA"), Some(3));
        assert_eq!(Scanner::new().feed(b"\x1b_apc\x1b\\"), None);
    }

    #[test]
    fn whitespace_and_controls_are_not_triggers_but_utf8_is() {
        assert_eq!(Scanner::new().feed(b" \t\r\n\x08\x07\x7f"), None);
        assert_eq!(Scanner::new().feed("ñ".as_bytes()), Some(0));
        assert_eq!(Scanner::new().feed(b"\x1b[31m!"), Some(5));
    }

    #[test]
    fn win32_input_mode_key_sequences() {
        // Ctrl+C down: VK_C, scan 46, U+0003, left Ctrl.
        assert_eq!(win32_key(0x43, 46, 3, true, 0x0008, 1), "\x1b[67;46;3;1;8;1_");
        assert_eq!(win32_key(0x41, 30, 97, false, 0, 1), "\x1b[65;30;97;0;0;1_");
    }

    #[test]
    fn vt_keys_join_surrogates_and_repeat() {
        let mut k = VtKeys::default();
        let mut out = Vec::new();
        k.push(u16::from(b'a'), 2, &mut out);
        k.push(0, 1, &mut out);
        let units: Vec<u16> = "😀".encode_utf16().collect();
        k.push(units[0], 1, &mut out);
        k.push(units[1], 1, &mut out);
        assert_eq!(out, "aa😀".as_bytes());
    }

    /// Review focus 3: a character split across reads is held until it's whole.
    #[test]
    fn complete_utf8_prefix() {
        assert_eq!(complete_utf8_len(b"ab"), 2);
        assert_eq!(complete_utf8_len(b""), 0);
        assert_eq!(complete_utf8_len(&[0xC3]), 0);
        assert_eq!(complete_utf8_len(&[b'a', 0xE2, 0x82]), 1);
        assert_eq!(complete_utf8_len(&[0xE2, 0x82, 0xAC]), 3);
        assert_eq!(complete_utf8_len(&[b'x', 0xF0, 0x9F, 0x98]), 1);
        assert_eq!(complete_utf8_len(&[0xF0, 0x9F, 0x98, 0x80]), 4);
        // Stray continuation bytes are passed on, not held forever.
        assert_eq!(complete_utf8_len(&[0x80, 0x80, 0x80, 0x80, 0x80]), 5);
    }
}
```

Add `pub mod vtscan;` to `lib.rs`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p rpyenv-core --lib vtscan::`
Expected: compile errors (`Scanner`, `win32_key`, `VtKeys`, `complete_utf8_len` not found).

- [ ] **Step 3: Write the implementation** above the tests module:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Str,
    StrEsc,
}

/// Scans VT output for the first printable character, across reads. It also tracks
/// whether the pseudo-console asked for win32-input-mode (`CSI ? 9001 h` / `l`, R3).
#[derive(Debug, Clone)]
pub struct Scanner {
    state: State,
    params: Vec<u8>,
    win32_input: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner::new()
    }
}

impl Scanner {
    pub fn new() -> Scanner {
        Scanner { state: State::Ground, params: Vec::new(), win32_input: false }
    }

    /// Feeds one read's bytes. Returns the index of the first printable character outside
    /// any escape sequence. Space, C0 controls and DEL don't count; any byte from 0x80 up
    /// does (UTF-8). The whole chunk is always scanned, so the mode stays current.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<usize> {
        let mut first = None;
        for (i, &b) in bytes.iter().enumerate() {
            if self.step(b) && first.is_none() {
                first = Some(i);
            }
        }
        first
    }

    /// Whether the last `?9001` mode change turned win32-input-mode on.
    pub fn win32_input(&self) -> bool {
        self.win32_input
    }

    fn step(&mut self, b: u8) -> bool {
        match self.state {
            State::Ground => match b {
                0x1b => {
                    self.state = State::Esc;
                    false
                }
                0x00..=0x20 | 0x7f => false,
                _ => true,
            },
            State::Esc => {
                self.state = match b {
                    b'[' => {
                        self.params.clear();
                        State::Csi
                    }
                    b']' => State::Osc,
                    b'P' | b'X' | b'^' | b'_' => State::Str,
                    // ESC itself, or an intermediate (as in `ESC ( B`): still in the escape.
                    0x1b | 0x20..=0x2f => State::Esc,
                    _ => State::Ground,
                };
                false
            }
            State::Csi => {
                match b {
                    0x30..=0x3f => self.params.push(b),
                    0x40..=0x7e => {
                        self.csi_final(b);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Esc,
                    _ => {}
                }
                false
            }
            State::Osc => {
                match b {
                    0x07 => self.state = State::Ground,
                    0x1b => self.state = State::OscEsc,
                    _ => {}
                }
                false
            }
            State::Str => {
                if b == 0x1b {
                    self.state = State::StrEsc;
                }
                false
            }
            State::OscEsc | State::StrEsc => {
                if b == b'\\' {
                    self.state = State::Ground;
                    false
                } else {
                    // An ESC that isn't the terminator starts a new sequence.
                    self.state = State::Esc;
                    self.step(b)
                }
            }
        }
    }

    fn csi_final(&mut self, last: u8) {
        if (last == b'h' || last == b'l') && self.params.first() == Some(&b'?') {
            if self.params[1..].split(|&c| c == b';').any(|p| p == b"9001") {
                self.win32_input = last == b'h';
            }
        }
    }
}

/// One key event in win32-input-mode: `ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc _`.
pub fn win32_key(vk: u16, sc: u16, uc: u16, down: bool, state: u32, repeat: u16) -> String {
    format!("\x1b[{vk};{sc};{uc};{};{state};{repeat}_", u8::from(down))
}

/// Turns key-down characters (UTF-16 units, as `KEY_EVENT_RECORD` carries them) into
/// UTF-8, joining surrogate pairs that arrive as two records.
#[derive(Debug, Default)]
pub struct VtKeys {
    high: Option<u16>,
}

impl VtKeys {
    pub fn push(&mut self, unit: u16, repeat: u16, out: &mut Vec<u8>) {
        if unit == 0 {
            return;
        }
        if (0xD800..0xDC00).contains(&unit) {
            self.high = Some(unit);
            return;
        }
        let s = match self.high.take() {
            Some(h) if (0xDC00..0xE000).contains(&unit) => String::from_utf16_lossy(&[h, unit]),
            _ => String::from_utf16_lossy(&[unit]),
        };
        for _ in 0..repeat.max(1) {
            out.extend_from_slice(s.as_bytes());
        }
    }
}

/// The length of `b`'s longest prefix that doesn't end inside a UTF-8 sequence: what can
/// be written now, the rest waiting for the next read.
pub fn complete_utf8_len(b: &[u8]) -> usize {
    let len = b.len();
    for back in 1..=len.min(4) {
        let i = len - back;
        let c = b[i];
        if c & 0xC0 == 0x80 {
            continue;
        }
        let need = match c {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        return if i + need > len { i } else { len };
    }
    len
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rpyenv-core --lib vtscan::`
Expected: 8 passed. Then run the same on Linux through WSL (`cargo test -p rpyenv-core --lib vtscan::`): 8 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core/src/vtscan.rs crates/rpyenv-core/src/lib.rs
git commit -m "VT trigger scanner, win32-input-mode encoder and UTF-8 boundary for LAZY mode"
```

---

### Task 4: LAZY: run the child on a pseudo-console (silent programs, exit codes, kill)

**Files:**
- Create: `crates/rpyenv-core/src/conpty.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`#[cfg(windows)] pub mod conpty;`)
- Modify: `crates/rpyenv-core/src/winproc.rs` (`Job::assign_handle`, PTY lock, LAZY branch)
- Modify: `crates/e2e/src/echo.rs` (`ARGV_ECHO_QUIET`, `ARGV_ECHO_OUT`)
- Test: `crates/e2e/tests/windows.rs`

**Interfaces:**
- Consumes:
  - Task 2's `winproc::{alloc_default, Alloc, open_console, hold_after}` and the mode log line;
  - Task 3's `vtscan::{Scanner, complete_utf8_len}`.
- Produces:
  - `conpty::merge_env(Vec<(OsString, OsString)>, &[(OsString, Option<OsString>)]) -> Vec<(OsString, OsString)>`;
  - `conpty::env_block(&[(OsString, OsString)]) -> Vec<u16>`;
  - `conpty::command_line(&Path, Option<&OsStr>) -> Vec<u16>`;
  - `conpty::run(&Command, &Path, Option<&Job>) -> Result<Outcome, LazyError>`, with `Outcome { code: u32, new_window: bool }` and `LazyError::{Setup(io::Error), Start(io::Error)}`;
  - `winproc::{set_pty(HPCON), close_pty(), resize_pty(COORD), Job::assign_handle(HANDLE) -> bool}`.
- Produces argv-echo knobs: `ARGV_ECHO_QUIET=1` (prints nothing to stdout) and `ARGV_ECHO_OUT=<file>` (writes what it would print there too).

- [ ] **Step 1: Write the failing unit tests.** Create `crates/rpyenv-core/src/conpty.rs` with only:

```rust
//! LAZY console mode (docs/windows-lazy-console.md, "LAZY mode: pseudo-console relay"):
//! the child runs on a pseudo-console, and the shim asks Windows for the console the
//! caller wanted only when the child first prints something.

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn changes_apply_without_regard_to_case_and_the_result_is_sorted() {
        let base = vec![(os("Path"), os("a")), (os("ZED"), os("z")), (os("keep"), os("k"))];
        let changes = vec![(os("PATH"), Some(os("b"))), (os("zed"), None), (os("NEW"), Some(os("n")))];
        assert_eq!(
            merge_env(base, &changes),
            vec![(os("keep"), os("k")), (os("NEW"), os("n")), (os("PATH"), os("b"))]
        );
    }

    #[test]
    fn the_block_is_nul_separated_and_double_terminated() {
        let block = env_block(&[(os("A"), os("1")), (os("B"), os("ñ"))]);
        let want: Vec<u16> = "A=1\0B=ñ\0\0".encode_utf16().collect();
        assert_eq!(block, want);
        assert_eq!(env_block(&[]), vec![0, 0]);
    }

    #[test]
    fn the_command_line_is_the_quoted_program_and_the_raw_tail() {
        let line = command_line(Path::new(r"C:\a b\python.exe"), Some(OsStr::new(r#" "x y" z"#)));
        assert_eq!(String::from_utf16(&line).unwrap(), "\"C:\\a b\\python.exe\"  \"x y\" z\0");
        let line = command_line(Path::new(r"C:\p.exe"), None);
        assert_eq!(String::from_utf16(&line).unwrap(), "\"C:\\p.exe\"\0");
    }
}
```

Add `#[cfg(windows)] pub mod conpty;` to `lib.rs`.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p rpyenv-core --lib conpty::`
Expected: compile errors (`merge_env`, `env_block`, `command_line` not found).

- [ ] **Step 3: Write the helpers and `run`.** Above the tests module in `conpty.rs`:

```rust
use crate::debuglog;
use crate::vtscan::{self, Scanner};
use crate::winproc::{self, Alloc, Job};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Console::{CreatePseudoConsole, COORD, HPCON};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};

/// The pseudo-console's size until a window shows (R5).
const START_SIZE: COORD = COORD { X: 120, Y: 30 };

/// Why LAZY didn't run the child.
#[derive(Debug)]
pub enum LazyError {
    /// The pseudo-console couldn't be set up: the caller falls back to EAGER.
    Setup(io::Error),
    /// The program itself couldn't start, as `Command::spawn` would report.
    Start(io::Error),
}

/// How the child ended, and whether the shim opened a window for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub code: u32,
    pub new_window: bool,
}

/// The child's variables: `base` with `changes` applied (`None` removes). Names compare
/// without regard to case, as Windows does, and the result is sorted the same way, as
/// `CreateProcessW` expects.
pub fn merge_env(
    base: Vec<(OsString, OsString)>,
    changes: &[(OsString, Option<OsString>)],
) -> Vec<(OsString, OsString)> {
    let key = |k: &OsStr| k.to_string_lossy().to_uppercase();
    let mut out: Vec<(OsString, OsString)> = base
        .into_iter()
        .filter(|(k, _)| !changes.iter().any(|(c, _)| key(c) == key(k)))
        .collect();
    out.extend(changes.iter().filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v))));
    out.sort_by_key(|(k, _)| key(k));
    out
}

/// A `CREATE_UNICODE_ENVIRONMENT` block: `NAME=value` entries, each NUL-terminated, then
/// one more NUL.
pub fn env_block(vars: &[(OsString, OsString)]) -> Vec<u16> {
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(k.encode_wide());
        block.push(u16::from(b'='));
        block.extend(v.encode_wide());
        block.push(0);
    }
    if vars.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// The child's command line, NUL-terminated: the program, quoted, then the shim's own
/// arguments exactly as it got them (spec §5.3).
pub fn command_line(program: &Path, tail: Option<&OsStr>) -> Vec<u16> {
    let mut line = vec![u16::from(b'"')];
    line.extend(program.as_os_str().encode_wide());
    line.push(u16::from(b'"'));
    if let Some(t) = tail {
        line.push(u16::from(b' '));
        line.extend(t.encode_wide());
    }
    line.push(0);
    line
}

/// A handle closed on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is owned here and closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn pipe() -> io::Result<(Owned, Owned)> {
    let (mut r, mut w) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: two out-pointers to locals; no security attributes, default size.
    if unsafe { CreatePipe(&mut r, &mut w, std::ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((Owned(r), Owned(w)))
}

/// Runs `cmd`'s program on a pseudo-console and waits for it. `cmd` supplies the variable
/// changes; the arguments are the shim's own raw tail.
pub fn run(cmd: &Command, program: &Path, job: Option<&Job>) -> Result<Outcome, LazyError> {
    let (in_read, in_write) = pipe().map_err(LazyError::Setup)?;
    let (out_read, out_write) = pipe().map_err(LazyError::Setup)?;
    let mut hpc: HPCON = 0;
    // SAFETY: two valid pipe ends and an out-pointer; the pseudo-console keeps its own
    // copies of the handles, so ours close below.
    let hr = unsafe { CreatePseudoConsole(START_SIZE, in_read.0, out_write.0, 0, &mut hpc) };
    if hr != 0 {
        return Err(LazyError::Setup(io::Error::from_raw_os_error(hr)));
    }
    drop(in_read);
    drop(out_write);
    winproc::set_pty(hpc);
    let changes: Vec<(OsString, Option<OsString>)> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_owned(), v.map(OsStr::to_owned)))
        .collect();
    let env = env_block(&merge_env(std::env::vars_os().collect(), &changes));
    let tail = crate::wincmd::own_tail(1);
    let mut line = command_line(program, tail.as_deref());
    let process = match start(&mut line, &env, hpc, job) {
        Ok(p) => p,
        Err(e) => {
            winproc::close_pty();
            return Err(LazyError::Start(e));
        }
    };
    let win32 = Arc::new(AtomicBool::new(false));
    let reader = {
        let (out, input, win32) = (out_read.0 as usize, in_write.0 as usize, win32.clone());
        std::thread::spawn(move || relay_output(out as HANDLE, input as HANDLE, win32))
    };
    let mut code = 1u32;
    // SAFETY: waits on and reads the child's process handle, which `process` owns.
    unsafe {
        WaitForSingleObject(process.0, INFINITE);
        GetExitCodeProcess(process.0, &mut code);
    }
    // Closing the pseudo-console ends its output once drained; the reader then returns.
    winproc::close_pty();
    let shown = reader.join().unwrap_or_default();
    let new_window = shown.alloc == Some(Alloc::New);
    shown.finish();
    drop(in_write);
    drop(out_read);
    Ok(Outcome { code, new_window })
}

/// Starts the child suspended on the pseudo-console, puts it in the job, then lets it run.
fn start(line: &mut [u16], env: &[u16], hpc: HPCON, job: Option<&Job>) -> io::Result<Owned> {
    // SAFETY: the attribute list lives in `attrs` for the whole call and is deleted on
    // every path; all other pointers are to locals or to the caller's NUL-terminated
    // buffers; the thread handle is closed here and the process handle returned owned.
    unsafe {
        let mut size = 0usize;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
        let mut attrs = vec![0u8; size];
        let list = attrs.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0 {
            return Err(io::Error::last_os_error());
        }
        let ok = UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            hpc as *const core::ffi::c_void,
            std::mem::size_of::<HPCON>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if ok == 0 {
            let e = io::Error::last_os_error();
            DeleteProcThreadAttributeList(list);
            return Err(e);
        }
        let mut si: STARTUPINFOEXW = std::mem::zeroed();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Null standard handles: the child takes the pseudo-console's. Without the flag it
        // inherits the shim's own, and its output bypasses the pseudo-console (probe 4).
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.lpAttributeList = list;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            env.as_ptr().cast(),
            std::ptr::null(),
            &si.StartupInfo,
            &mut pi,
        );
        let err = io::Error::last_os_error();
        DeleteProcThreadAttributeList(list);
        if ok == 0 {
            return Err(err);
        }
        if !job.is_some_and(|j| j.assign_handle(pi.hProcess)) {
            debuglog::append("job=none");
        }
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);
        Ok(Owned(pi.hProcess))
    }
}

/// What the output relay did: whether it asked for a console, and what to stop afterwards.
#[derive(Default)]
struct Shown {
    alloc: Option<Alloc>,
}

impl Shown {
    /// Stops what the relay started.
    fn finish(self) {}
}

/// Reads the pseudo-console's output until it closes. Until the first printable
/// character the bytes are held; then the console the caller wanted is asked for.
fn relay_output(out: HANDLE, _input: HANDLE, win32: Arc<AtomicBool>) -> Shown {
    let mut scanner = Scanner::new();
    let mut held: Vec<u8> = Vec::new();
    let mut shown = Shown::default();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut n = 0u32;
        // SAFETY: reads into a local buffer of the length given.
        let ok = unsafe { ReadFile(out, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) };
        if ok == 0 || n == 0 {
            break;
        }
        let chunk = &buf[..n as usize];
        let trigger = scanner.feed(chunk);
        win32.store(scanner.win32_input(), Ordering::SeqCst);
        if shown.alloc.is_none() {
            held.extend_from_slice(chunk);
            if trigger.is_some() {
                // Task 5 shows the window here; until then the output is dropped.
                shown.alloc = Some(Alloc::None);
                held.clear();
            }
        }
    }
    let _ = (vtscan::complete_utf8_len(&held), WriteFile as usize);
    shown
}
```

`relay_output` and `Shown` are deliberately minimal here. Task 5 replaces them with the version that shows the window. The `let _ = …` line only keeps Task 5's imports compiling, and Task 5 removes it.

- [ ] **Step 4: Add the PTY lock and `assign_handle` to `winproc.rs`:**

```rust
use std::sync::Mutex;
use windows_sys::Win32::System::Console::{ClosePseudoConsole, ResizePseudoConsole, COORD, HPCON};

/// The running pseudo-console, if any: LAZY's main thread, its input relay and the
/// console handler all reach it through this lock, so it's never used after closing.
static PTY: Mutex<HPCON> = Mutex::new(0);

pub fn set_pty(hpc: HPCON) {
    *PTY.lock().unwrap_or_else(|e| e.into_inner()) = hpc;
}

/// Closes the pseudo-console once, whichever thread gets here first.
pub fn close_pty() {
    let mut pty = PTY.lock().unwrap_or_else(|e| e.into_inner());
    if *pty != 0 {
        // SAFETY: a pseudo-console from CreatePseudoConsole, closed once under the lock.
        unsafe { ClosePseudoConsole(*pty) };
        *pty = 0;
    }
}

/// Resizes the pseudo-console, if it's still open.
pub fn resize_pty(size: COORD) {
    let pty = PTY.lock().unwrap_or_else(|e| e.into_inner());
    if *pty != 0 {
        // SAFETY: an open pseudo-console, held open by the lock.
        unsafe { ResizePseudoConsole(*pty, size) };
    }
}
```

In `impl Job`, replace `assign` with:

```rust
    /// Puts `child` in the job. False when Windows refuses, for example when the shim runs
    /// in a job that forbids it; the child then runs without one.
    pub fn assign(&self, child: &Child) -> bool {
        self.assign_handle(child.as_raw_handle())
    }

    /// `assign`, for a process this crate started itself.
    pub fn assign_handle(&self, process: HANDLE) -> bool {
        // SAFETY: both handles are valid for the duration of the call.
        unsafe { AssignProcessToJobObject(self.0, process) != 0 }
    }
```

In `spawn_and_wait`, after `debuglog::append(&format!("mode=…`, insert:

```rust
    ignore_console_events();
    let job = Job::new();
    if mode == ConsoleMode::Lazy {
        match crate::conpty::run(cmd, program, job.as_ref()) {
            Ok(o) => {
                drop(job);
                if o.new_window {
                    hold_after(o.code);
                }
                return Ok(std::os::windows::process::ExitStatusExt::from_raw(o.code));
            }
            Err(crate::conpty::LazyError::Start(e)) => return Err(e),
            Err(crate::conpty::LazyError::Setup(e)) => {
                debuglog::append(&format!("lazy=failed {e}"));
                mode = ConsoleMode::Eager;
            }
        }
    }
```

Remove the later `ignore_console_events();` and `let job = Job::new();` lines, so each runs once. Change the match arm `ConsoleMode::Eager | ConsoleMode::Lazy =>` to `ConsoleMode::Eager =>` and add `ConsoleMode::Lazy => unreachable!("LAZY returned or fell back above"),`.

- [ ] **Step 5: Run the unit tests to verify they pass**

Run: `cargo test -p rpyenv-core --lib conpty::`
Expected: 3 passed.

- [ ] **Step 6: Add the argv-echo knobs.** In `echo.rs`, after the `ARGV_ECHO_TOUCH` block, replace the stdout write with:

```rust
    if let Some(p) = std::env::var_os("ARGV_ECHO_OUT") {
        let _ = std::fs::write(&p, out.as_bytes());
    }
    if var("ARGV_ECHO_QUIET").as_deref() != Some("1") {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
    }
```

- [ ] **Step 7: Write the e2e tests:**

```rust
/// LAZY, review focus 2: a program that never prints never gets a window. Its arguments
/// arrive unchanged, the variables too, and its exit code comes back.
#[test]
fn win_lazy_a_silent_program_never_shows_a_window() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f.base.join("out.txt");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_QUIET", v("1")),
            ("ARGV_ECHO_OUT", out.as_os_str()),
            ("ARGV_ECHO_ENV", v("MARKER")),
            ("MARKER", v("ñ x")),
            ("ARGV_ECHO_EXIT", v("7")),
        ],
        &["a b", "ñ"],
    );
    assert_eq!(explorer_exit(helper, &exit), 7);
    if !api() {
        assert_eq!(mode_in(&log), "INHERIT");
        return;
    }
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(mode_in(&log), "LAZY");
    assert!(!text.contains("console="), "{text}");
    let got = std::fs::read_to_string(&out).unwrap();
    assert!(got.contains(&line("arg", "a b")), "{got}");
    assert!(got.contains(&line("arg", "ñ")), "{got}");
    assert!(got.contains("env MARKER=\"ñ x\""), "{got}");
}

/// Review focus 5: killing a LAZY shim ends its program, through the Job Object.
#[test]
fn win_lazy_killing_the_shim_kills_the_child() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let after = f.base.join("after");
    let (mut helper, _exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_QUIET", v("1")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("3000")),
            ("ARGV_ECHO_AFTER", after.as_os_str()),
        ],
        &[],
    );
    wait_for(&ready);
    let pid = shim_pid(&std::fs::read_to_string(&log).unwrap());
    let killed = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(killed.success());
    let _ = helper.wait();
    std::thread::sleep(Duration::from_millis(3500));
    assert!(!after.exists(), "the child kept running after the shim was killed");
}
```

- [ ] **Step 8: Run them**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows -- win_lazy`
Expected: 2 passed. RED check: with Step 4's LAZY branch commented out, Task 2's code runs LAZY as EAGER. The log then has `mode=LAZY` and `console=new`, and the silent test fails on `!text.contains("console=")`. Record that RED run in the ledger, then restore the branch.

- [ ] **Step 9: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add crates/rpyenv-core crates/e2e
git commit -m "LAZY console mode: the child runs on a pseudo-console, raw command line and merged variables, inside the job"
```

---

### Task 5: LAZY: show the window on the first output, relay input, hold on failure

**Files:**
- Modify: `crates/rpyenv-core/src/conpty.rs` (`Shown`, `relay_output`, new `reveal`, `relay_input`, `write_console`, `window_size`)
- Modify: `crates/e2e/src/echo.rs` (`ARGV_ECHO_PROMPT`)
- Test: `crates/e2e/tests/windows.rs`

**Interfaces:**
- Consumes:
  - Task 4's `run`, `Outcome`, `Owned` and the PTY functions;
  - Task 3's `Scanner`, `win32_key`, `VtKeys`, `complete_utf8_len`;
  - Task 2's `alloc_default`, `open_console`, `hold_after`.
- Produces: the log line `console=new pid=<shim pid>` at the first output. It also produces argv-echo's `ARGV_ECHO_PROMPT=<text>`, printed and flushed right after `ARGV_ECHO_DELAY_MS`, before stdin is read.

- [ ] **Step 1: Add the argv-echo prompt.** In `echo.rs`, right after the `ARGV_ECHO_DELAY_MS` block:

```rust
    if let Some(prompt) = std::env::var_os("ARGV_ECHO_PROMPT") {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(prompt.to_string_lossy().as_bytes());
        let _ = stdout.flush();
    }
```

- [ ] **Step 2: Write the failing e2e tests:**

```rust
/// LAZY: no window while the program is quiet; it appears when the program prints, and
/// shows the output intact (review focus 3: non-ASCII).
#[test]
fn win_lazy_shows_the_window_on_first_output() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let first = f.base.join("first");
    let ready = f.base.join("ready");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_FIRST", first.as_os_str()),
            ("ARGV_ECHO_DELAY_MS", v("1500")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("3000")),
        ],
        &["ñ 漢"],
    );
    wait_for(&first);
    if !api() {
        assert_eq!(explorer_exit(helper, &exit), 0);
        return;
    }
    assert!(!std::fs::read_to_string(&log).unwrap().contains("console="), "a window before any output");
    wait_for(&ready);
    let text = wait_log(&log, "console=new");
    let shown = screen(shim_pid(&text));
    assert!(shown.contains("argv0="), "{shown}");
    assert!(shown.contains("ñ 漢"), "{shown}");
    assert_eq!(explorer_exit(helper, &exit), 0);
}

/// LAZY: what's typed in the window reaches the program's stdin.
#[test]
fn win_lazy_relays_typed_input() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_PROMPT", v("name? ")),
            ("ARGV_ECHO_STDIN", v("1")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("3000")),
        ],
        &[],
    );
    if !api() {
        let _ = helper;
        return;
    }
    let pid = shim_pid(&wait_log(&log, "console=new"));
    type_keys(pid, "abc\r\x1a\r");
    wait_for(&ready);
    let shown = screen(pid);
    assert!(shown.contains(r#"stdin="abc\r\n""#), "{shown}");
    assert_eq!(explorer_exit(helper, &exit), 0);
}

/// LAZY: Ctrl+C typed in the window reaches the program as Ctrl+C.
#[test]
fn win_lazy_ctrl_c_reaches_the_program() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE_HOLD", v("0")),
            ("ARGV_ECHO_CATCH_BREAK", v("1")),
            ("ARGV_ECHO_PROMPT", v("go")),
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    if !api() {
        let _ = (helper, exit);
        return;
    }
    let pid = shim_pid(&wait_log(&log, "console=new"));
    type_keys(pid, "\x03");
    assert_eq!(explorer_exit(helper, &exit), 5);
}

/// LAZY: a failed program's window waits for a key (the default hold).
#[test]
fn win_lazy_holds_a_failed_programs_window_until_a_key() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_PROMPT", v("x")),
            ("ARGV_ECHO_EXIT", v("3")),
        ],
        &[],
    );
    if !api() {
        assert_eq!(explorer_exit(helper, &exit), 3);
        return;
    }
    let pid = shim_pid(&wait_log(&log, "hold=key"));
    assert!(screen(pid).contains("exited with code 3"));
    type_keys(pid, "x");
    assert_eq!(explorer_exit(helper, &exit), 3);
}

/// LAZY can't start a batch file with the raw command line safely, so it takes EAGER.
#[test]
fn win_lazy_a_batch_target_takes_eager() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("tool.bat"), "@exit /b 4\r\n").unwrap();
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "tool",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE_HOLD", v("0")),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 4);
    if api() {
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("lazy=batch"), "{text}");
        assert_eq!(mode_in(&log), "EAGER");
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows -- win_lazy`
Expected: `win_lazy_shows_the_window_on_first_output`, `…relays_typed_input`, `…ctrl_c…` and `…holds…` fail, with "no \"console=new\"" (or "no \"hold=key\"") after 20 s. The batch test and Task 4's two tests pass.

- [ ] **Step 4: Implement the window, the relays and the hold.** In `conpty.rs`, extend the imports:

```rust
use crate::vtscan::VtKeys;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::Globalization::CP_UTF8;
use windows_sys::Win32::System::Console::{
    GetConsoleScreenBufferInfo, ReadConsoleInputW, SetConsoleMode, SetConsoleOutputCP,
    CONSOLE_SCREEN_BUFFER_INFO, DISABLE_NEWLINE_AUTO_RETURN, ENABLE_PROCESSED_OUTPUT,
    ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
    ENABLE_WRAP_AT_EOL_OUTPUT, INPUT_RECORD, KEY_EVENT, WINDOW_BUFFER_SIZE_EVENT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
```

Replace `Shown`, its `impl`, and `relay_output` with:

```rust
/// What the output relay did: whether it asked for a console, and the input relay it
/// started, to stop once the child is gone.
#[derive(Default)]
struct Shown {
    alloc: Option<Alloc>,
    conout: usize,
    input: Option<(std::thread::JoinHandle<()>, usize)>,
}

impl Shown {
    /// Stops the input relay and closes the screen handle.
    fn finish(self) {
        if let Some((thread, stop)) = self.input {
            // SAFETY: an event this module created; set, then closed after the thread
            // that waits on it has ended.
            unsafe { SetEvent(stop as HANDLE) };
            let _ = thread.join();
            unsafe { CloseHandle(stop as HANDLE) };
        }
        if self.conout != 0 {
            // SAFETY: the screen handle `reveal` opened, closed once.
            unsafe { CloseHandle(self.conout as HANDLE) };
        }
    }
}

/// Reads the pseudo-console's output until it closes. Until the first printable
/// character the bytes are held. Then the console the caller wanted is asked for, and
/// they and everything after go to it, or nowhere if the caller wanted none (R2).
fn relay_output(out: HANDLE, input: HANDLE, win32: Arc<AtomicBool>) -> Shown {
    let mut scanner = Scanner::new();
    let mut held: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut shown = Shown::default();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut n = 0u32;
        // SAFETY: reads into a local buffer of the length given.
        let ok = unsafe { ReadFile(out, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) };
        if ok == 0 || n == 0 {
            break;
        }
        let chunk = &buf[..n as usize];
        let trigger = scanner.feed(chunk);
        win32.store(scanner.win32_input(), Ordering::SeqCst);
        match shown.alloc {
            None => {
                held.extend_from_slice(chunk);
                if trigger.is_some() {
                    shown = reveal(input, win32.clone());
                    if shown.conout != 0 {
                        write_console(shown.conout as HANDLE, &mut pending, &held);
                    }
                    held = Vec::new();
                }
            }
            Some(_) if shown.conout != 0 => write_console(shown.conout as HANDLE, &mut pending, chunk),
            Some(_) => {}
        }
    }
    shown
}

/// Asks for the caller's console and gets it ready for the relays (console doc, LAZY
/// step 4): UTF-8 VT output sized to the pseudo-console, and an input relay.
fn reveal(input: HANDLE, win32: Arc<AtomicBool>) -> Shown {
    let alloc = winproc::alloc_default();
    let mut shown = Shown { alloc: Some(alloc), ..Shown::default() };
    if alloc == Alloc::None {
        return shown;
    }
    let (Some(conin), Some(conout)) = (winproc::open_console("CONIN$"), winproc::open_console("CONOUT$")) else {
        return shown;
    };
    // SAFETY: console calls on the handles just opened.
    unsafe {
        SetConsoleOutputCP(CP_UTF8);
        SetConsoleMode(
            conout,
            ENABLE_PROCESSED_OUTPUT
                | ENABLE_WRAP_AT_EOL_OUTPUT
                | ENABLE_VIRTUAL_TERMINAL_PROCESSING
                | DISABLE_NEWLINE_AUTO_RETURN,
        );
    }
    if let Some(size) = window_size(conout) {
        winproc::resize_pty(size);
    }
    // SAFETY: a manual-reset event, initially unset, unnamed; `Shown::finish` closes it.
    let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    let (ci, co, inp, st) = (conin as usize, conout as usize, input as usize, stop as usize);
    let thread = std::thread::spawn(move || {
        relay_input(ci as HANDLE, co as HANDLE, inp as HANDLE, st as HANDLE, win32)
    });
    shown.conout = conout as usize;
    shown.input = Some((thread, st));
    shown
}

/// The window's size in character cells.
fn window_size(conout: HANDLE) -> Option<COORD> {
    // SAFETY: reads into a zeroed local of the documented layout.
    unsafe {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        (GetConsoleScreenBufferInfo(conout, &mut info) != 0).then(|| COORD {
            X: info.srWindow.Right - info.srWindow.Left + 1,
            Y: info.srWindow.Bottom - info.srWindow.Top + 1,
        })
    }
}

/// Writes whole UTF-8 characters; a character cut by the read waits in `pending`.
fn write_console(conout: HANDLE, pending: &mut Vec<u8>, bytes: &[u8]) {
    pending.extend_from_slice(bytes);
    let n = vtscan::complete_utf8_len(pending);
    write_all(conout, &pending[..n]);
    pending.drain(..n);
}

/// Writes all of `b`; false when the handle stops taking it.
fn write_all(h: HANDLE, mut b: &[u8]) -> bool {
    while !b.is_empty() {
        let mut w = 0u32;
        // SAFETY: writes from a live slice of the length given.
        let ok = unsafe { WriteFile(h, b.as_ptr(), b.len() as u32, &mut w, std::ptr::null_mut()) };
        if ok == 0 || w == 0 {
            return false;
        }
        b = &b[w as usize..];
    }
    true
}

/// Relays the window's keys to the pseudo-console (console doc, LAZY step 5; R3), and
/// its size changes, until `stop` is set or the pseudo-console stops reading.
fn relay_input(conin: HANDLE, conout: HANDLE, input: HANDLE, stop: HANDLE, win32: Arc<AtomicBool>) {
    let mut mode = None;
    let mut keys = VtKeys::default();
    // SAFETY: a zeroed INPUT_RECORD array is valid.
    let mut records: [INPUT_RECORD; 64] = unsafe { std::mem::zeroed() };
    loop {
        let want = win32.load(Ordering::SeqCst);
        if mode != Some(want) {
            // Processed, line and echo input off: Ctrl+C arrives as a key, for the child.
            let m = if want { ENABLE_WINDOW_INPUT } else { ENABLE_WINDOW_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT };
            // SAFETY: sets the mode of the input handle this thread owns.
            unsafe { SetConsoleMode(conin, m) };
            mode = Some(want);
        }
        let handles = [stop, conin];
        // SAFETY: waits on two live handles.
        if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } != WAIT_OBJECT_0 + 1 {
            break;
        }
        let mut n = 0u32;
        // SAFETY: reads into the local array, at most its length.
        if unsafe { ReadConsoleInputW(conin, records.as_mut_ptr(), records.len() as u32, &mut n) } == 0 {
            break;
        }
        let mut bytes = Vec::new();
        for r in &records[..n as usize] {
            match u32::from(r.EventType) {
                KEY_EVENT => {
                    // SAFETY: the record is tagged KEY_EVENT.
                    let k = unsafe { r.Event.KeyEvent };
                    // SAFETY: both union members are a u16-sized character.
                    let uc = unsafe { k.uChar.UnicodeChar };
                    if want {
                        bytes.extend_from_slice(
                            vtscan::win32_key(
                                k.wVirtualKeyCode,
                                k.wVirtualScanCode,
                                uc,
                                k.bKeyDown != 0,
                                k.dwControlKeyState,
                                k.wRepeatCount,
                            )
                            .as_bytes(),
                        );
                    } else if k.bKeyDown != 0 {
                        keys.push(uc, k.wRepeatCount, &mut bytes);
                    }
                }
                WINDOW_BUFFER_SIZE_EVENT => {
                    if let Some(size) = window_size(conout) {
                        winproc::resize_pty(size);
                    }
                }
                _ => {}
            }
        }
        if !bytes.is_empty() && !write_all(input, &bytes) {
            break;
        }
    }
    // SAFETY: the input handle `reveal` opened for this thread, closed once.
    unsafe { CloseHandle(conin) };
}
```

Delete Task 4's placeholder line `let _ = (vtscan::complete_utf8_len(&held), WriteFile as usize);` if it is still present.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows -- win_lazy`
Expected: 7 passed.

- [ ] **Step 6: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/rpyenv-core/src/conpty.rs crates/e2e
git commit -m "LAZY: the window appears on the first output; keys, Ctrl+C and resizes relayed; a failed program's window held"
```

---

### Task 6: Closing the window lets the program finish (CLOSE, LOGOFF, SHUTDOWN)

**Files:**
- Modify: `crates/rpyenv-core/src/winproc.rs` (`watch_child`, the handler)
- Modify: `crates/rpyenv-core/src/conpty.rs` (`start` calls `watch_child`)
- Modify: `crates/e2e/src/echo.rs` (`ARGV_ECHO_ON_CLOSE`, `ARGV_ECHO_CLOSE_PID`)
- Test: `crates/e2e/tests/windows.rs`

**Interfaces:**
- Consumes: Task 4's `close_pty`.
- Produces:
  - `winproc::watch_child(HANDLE)`;
  - argv-echo's `ARGV_ECHO_ON_CLOSE=<file>`: on `CTRL_CLOSE_EVENT` it sleeps 1 s, writes the file, then returns;
  - argv-echo's `ARGV_ECHO_CLOSE_PID=<pid>`: posts `WM_CLOSE` to that process's console window, exiting 4 when it isn't a conhost window.

- [ ] **Step 1: Add the argv-echo knobs.** In `echo.rs`, change `catch`:

```rust
#[cfg(windows)]
unsafe extern "system" fn catch(event: u32) -> windows_sys::core::BOOL {
    if event == windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT {
        // Cleanup that takes a while: Windows ends this process when the handler returns.
        if let Some(p) = std::env::var_os("ARGV_ECHO_ON_CLOSE") {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let _ = std::fs::write(p, b"");
        }
        return 1;
    }
    CAUGHT.store(true, std::sync::atomic::Ordering::SeqCst);
    1
}

/// Posts `WM_CLOSE` to process `pid`'s console window, as clicking its close button does.
/// Exits 4 when the window isn't conhost's (Windows Terminal hosts it elsewhere).
#[cfg(windows)]
fn close_window(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, PostMessageW, WM_CLOSE};
    // SAFETY: console attachment of this helper only; reads a class name into a local.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        let hwnd = GetConsoleWindow();
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), 64);
        if String::from_utf16_lossy(&class[..n.max(0) as usize]) != "ConsoleWindowClass" {
            return 4;
        }
        if PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0 {
            return 3;
        }
    }
    0
}
```

Register the handler when either knob is set:

```rust
    #[cfg(windows)]
    if std::env::var("ARGV_ECHO_CATCH_BREAK").as_deref() == Ok("1")
        || std::env::var_os("ARGV_ECHO_ON_CLOSE").is_some()
    {
```

Next to the other `_PID` helpers, add:

```rust
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_CLOSE_PID").ok().and_then(|p| p.parse::<u32>().ok()) {
        std::process::exit(close_window(pid));
    }
```

- [ ] **Step 2: Write the failing e2e tests:**

```rust
/// Closes process `pid`'s console window. False (after ending the shim) when the window
/// isn't conhost's, which this test can't close.
fn close_window_of(pid: u32) -> bool {
    let code = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_CLOSE_PID", pid.to_string())
        .stdin(Stdio::null())
        .status()
        .unwrap()
        .code();
    if code == Some(4) {
        eprintln!("skipped: the console window isn't conhost's (Windows Terminal is the default terminal)");
        let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).status();
        return false;
    }
    assert_eq!(code, Some(0), "cannot close the window of {pid}");
    true
}

/// R8: closing a window the program shares with the shim lets the program's own close
/// handler finish, instead of the job killing it as soon as the shim ends.
#[test]
fn win_closing_the_window_lets_the_program_finish_its_cleanup() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let cleaned = f.base.join("cleaned");
    let (helper, _exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
            ("ARGV_ECHO_ON_CLOSE", cleaned.as_os_str()),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    wait_for(&ready);
    let pid = shim_pid(&std::fs::read_to_string(&log).unwrap());
    if !close_window_of(pid) {
        return;
    }
    let mut helper = helper;
    helper.wait().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(cleaned.exists(), "the program was ended before its cleanup finished");
}

/// R8 in LAZY: closing the shim's window closes the pseudo-console, and the program's
/// close handler runs to the end.
#[test]
fn win_lazy_closing_the_window_reaches_the_program() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    if !api() {
        return;
    }
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let cleaned = f.base.join("cleaned");
    let (mut helper, _exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_ON_CLOSE", cleaned.as_os_str()),
            ("ARGV_ECHO_PROMPT", v("x")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    let pid = shim_pid(&wait_log(&log, "console=new"));
    wait_for(&ready);
    if !close_window_of(pid) {
        return;
    }
    helper.wait().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(cleaned.exists(), "the program never got to finish its close handler");
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows -- closing_the_window`
Expected: both fail. The EAGER one says "ended before its cleanup finished". The LAZY one says "never got to finish", because the shim's handler returns at once, Windows ends the shim, and the job kills the program. If the EAGER test passes before the fix (conhost waited on its own), ledger that, and keep the test as a pin.

- [ ] **Step 4: Implement.** In `winproc.rs`:

```rust
use std::sync::atomic::AtomicUsize;
use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
use windows_sys::Win32::System::Console::{CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// A copy of the child's process handle, never closed, for the console handler to wait on.
static CHILD: AtomicUsize = AtomicUsize::new(0);

/// Remembers the child for the console handler (R8).
pub fn watch_child(process: HANDLE) {
    let mut copy: HANDLE = std::ptr::null_mut();
    // SAFETY: duplicates a live process handle within this process; the copy is kept for
    // the life of the process.
    unsafe {
        let me = GetCurrentProcess();
        if DuplicateHandle(me, process, me, &mut copy, 0, 0, DUPLICATE_SAME_ACCESS) != 0 {
            CHILD.store(copy as usize, Ordering::SeqCst);
        }
    }
}
```

Replace `keep_running` and its doc comment, and update `ignore_console_events`'s doc comment:

```rust
/// Ctrl+C and Ctrl+Break: TRUE, so the shim ignores them and waits for the child, which
/// gets them too. CLOSE, LOGOFF and SHUTDOWN (R8): Windows ends the shim when this
/// returns, and the job would then kill the child mid-cleanup. So the handler first closes
/// a LAZY pseudo-console (which sends the child its own CTRL_CLOSE_EVENT), then waits for
/// the child, until Windows' own timeout ends both.
unsafe extern "system" fn on_console_event(event: u32) -> BOOL {
    if matches!(event, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
        close_pty();
        let child = CHILD.load(Ordering::SeqCst);
        if child != 0 {
            WaitForSingleObject(child as HANDLE, INFINITE);
        }
    }
    TRUE
}
```

In `ignore_console_events`, register `on_console_event`. In `spawn_and_wait`, right after `let mut child = cmd.spawn()?;`, add `watch_child(child.as_raw_handle() as HANDLE);`. In `conpty::start`, right after the job assignment, add `winproc::watch_child(pi.hProcess);`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo build --workspace` then `cargo test -p rpyenv-e2e --test windows -- closing_the_window` then `cargo test --workspace`
Expected: both pass (or print the Windows Terminal skip note); the suite is green.

- [ ] **Step 6: Commit**

```bash
git add crates/rpyenv-core crates/e2e
git commit -m "Closing the window, logging off or shutting down waits for the program instead of killing it through the job"
```

---

### Task 7: Docs as built, the measurement, CI

**Files:**
- Modify: `docs/windows-lazy-console.md`, `docs/specs/2026-09-27-rpyenv-design.md` (§5.3, the §13 variable table, §17 questions 3–4), `README.md` (the variables table)

- [ ] **Step 1: The console doc.** Make these changes in `docs/windows-lazy-console.md`:

1. Status line: `**Status:** implemented in M5a (EAGER, LAZY, the manifest, holding a window, closing). Detecting input without prior output stays deferred.`
2. Rule 2's decision tree:

```
shim starts
├─ attached to a console? ─────────────── yes → INHERIT
└─ no
   ├─ stdout and stderr both redirected? ─ yes → NO-WINDOW
   ├─ not the console shim, or 24H2
   │  APIs unavailable? ───────────────── yes → MIRROR
   ├─ the parent has a console, or
   │  RPYENV_CONSOLE=eager, or
   │  only some handles redirected? ────── yes → EAGER
   └─ otherwise ─────────────────────────────── → LAZY
```

   Add a paragraph after it on the parent-console rule. Use R1's text, plus the probe result that the PEB's `ConsoleHandle` is 0 for both DETACHED and Explorer-like launches (build 26200).
3. In the table, the EAGER row becomes "`AllocConsoleWithOptions(DEFAULT)` at startup, then INHERIT, or `DETACHED_PROCESS` when it gives no console".
4. In "LAZY mode", fold in:
   - step 1: `STARTF_USESTDHANDLES` with null handles (the probe finding), and `CreateProcessW` (std's attribute API is unstable), with batch targets taking EAGER;
   - step 3: the captured startup bytes;
   - step 4: replace "set the window title" with R4;
   - step 5: R3 (win32-input-mode, with VT characters when it is off);
   - step 6: hold only for failures other than `0xC000013A`, controlled by `RPYENV_CONSOLE_HOLD`;
   - R2's NO_CONSOLE case and R9's Ctrl+Break limitation.
5. Configuration: `lazy` **is** the default (user decision 2026-10-06). Add `RPYENV_CONSOLE_HOLD` with its three forms.
6. Open questions:
   1. Still open; on the manual checklist.
   2. Resolved, R5.
   3. Resolved, with the measurement: "ConPTY launch to exit 715 ms, a fresh windowless conhost 649 ms, an inherited console 86 ms. Median of 30 launches of a trivial program, build 26200, `C:\tmp\m5probe` `conpty time`. LAZY's extra cost over a launch that gets a new console anyway is about 20–70 ms on that host."
   4. Resolved, configurable (`RPYENV_CONSOLE_HOLD`).

- [ ] **Step 2: The spec.**
   - §5.3 "Console": replace the two paragraphs that begin "Milestone M1 implements" and "The M1 shims ship" with: "M1 implemented INHERIT, NO-WINDOW, MIRROR and the GUI shim; M5a added EAGER and LAZY and the `consoleAllocationPolicy=detached` manifest entry on `pyenv-shim.exe`. Before build 26100 the entry is ignored and M1's behavior stands. `pyenv exec` and the GUI shim keep M1's modes."
   - §13 variables: add `| RPYENV_CONSOLE | rpyenv | Windows: lazy (default) or eager, for a shim started without a console |` and `| RPYENV_CONSOLE_HOLD | rpyenv | Windows: 0 never holds a window the shim opened after a failure; N holds it N seconds; otherwise until a key |`.
   - §17: mark 3 and 4 resolved, pointing to the console doc.

- [ ] **Step 3: README.** Add the same two rows to the variables table, next to `RPYENV_LIVE_REHASH`.

- [ ] **Step 4: Check that the Linux build is unaffected**

Run, through WSL on the branch: `cargo test -p rpyenv-core --lib` and `cargo clippy --workspace --all-targets -- -D warnings`
Expected: pass. `conpty` is Windows-only; `console` and `vtscan` tests run there.

Run on Windows: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add docs README.md
git commit -m "Docs for M5a as built: the decision tree with the parent-console rule, LAZY details, RPYENV_CONSOLE and RPYENV_CONSOLE_HOLD, the measured ConPTY cost"
```

- [ ] **Step 6: Manual checklist** (console doc, "Testing"). Record the results in the ledger; it's for the user to run:
   - an Explorer double-click of a `.py` with `print` + `input()`, and a silent one;
   - a `.lnk` shortcut;
   - a Task Scheduler job;
   - `start python` from cmd;
   - each of these with conhost and with Windows Terminal as the default terminal.
