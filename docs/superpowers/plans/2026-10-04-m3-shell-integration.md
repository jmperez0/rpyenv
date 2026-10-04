# M3 Shell Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `pyenv init`, `pyenv shell` / `sh-shell`, `sh-rehash`, `pyenv completions` and every `--complete`, on Linux exactly as pyenv 2.8.8 does them and on Windows as spec §7 defines them, with the Windows print-and-exit-1 fallback where no integration is loaded.

**Architecture:**
- **Linux** ports upstream's bash scripts line for line into Rust command functions: `commands/init.rs`, `commands/shell.rs`, `commands/completions.rs`.
- **Windows** gets integration for PowerShell, Git Bash and fish, which pyenv-win lacks (`commands/init_win.rs`, `commands/shell_win.rs`). cmd, and any shell where integration isn't loaded, gets the detect-and-print fallback.
- **Shell detection** lives in `rpyenv-core`: `shellname.rs` (pure functions) and `winproc::parent_image_name` (Win32).

**Tech Stack:** Rust 2021; `windows-sys` 0.61, with `Win32_System_Diagnostics_ToolHelp` added; bats 1.11.1; pyenv-win's pytest suite through `parity/pyenv_win_overlay.py`.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`: §4 (parity policy), §6 (the Shell row), §7 (shell integration), §14 item 3. The upstream facts this plan reproduces:
- `docs/parity/pyenv-m3-reference.md` (**M3L**): pyenv 2.8.8, Linux;
- `docs/parity/pyenv-win-m3-reference.md` (**M3W**): pyenv-win 856ed5a;
- `docs/parity/pyenv-m1-reference.md` (**M1L**): the `--complete` output of M1 commands.

Read the M3L and M3W sections a task names before writing its code.

## Decisions

Each decision gives what was decided, why, and its source.

1. **Windows fallback detection order** (user, 2026-10-04): the parent process first, trusted only if it was created before `pyenv`; then `PYENV_SHELL`; then print every shell's command.
   - Why: `PYENV_SHELL` is set only by `pyenv init`, so by the time the fallback runs it was usually inherited from an integrated ancestor. Example: pwsh with integration starts `cmd`.
   - This amends spec §7 step 1. Task 6 edits the spec.
2. **Windows `init --install` is deferred to M6's `pyenv setup`** (user, 2026-10-04). In M3 it prints `pyenv: cannot automatically configure startup files for <shell>` and exits 1, upstream's own unsupported-shell path.
3. **Completion scripts.**
   - Upstream's four `completions/pyenv.{bash,zsh,fish,pwsh}` are vendored byte for byte (MIT) at the repository's `completions/`.
   - `init -` prints a line loading `<prefix>/completions/pyenv.<shell>` when that file is readable.
   - The prefix is the parent of the directory holding the running `pyenv` binary. Upstream's dispatcher derives `_PYENV_INSTALL_PREFIX` from its own location the same way, and overwrites any inherited value (`libexec/pyenv:79-82`), so rpyenv ignores the variable.
4. **The routed set of the shell function** is rpyenv's built-in `sh-*` commands (`rehash`, `shell`), taken from the command table.
   - Upstream lists `pyenv-sh-*` files on PATH (`pyenv-commands --sh`), so a plugin's `sh-` command joins the set. Plugin dispatch is M4.
   - Allowlist row **D-88**. The bats test "outputs sh-compatible case syntax" stubs `pyenv-commands` and stays an expected failure under D-88.
5. **`--complete` is answered from one table**, `commands::completions::TABLE_PYENV` / `TABLE_WIN`, instead of a block per command.
   - On Linux, the dispatcher answers `<cmd> --complete` from it, as each upstream script does.
   - The table's `marker` flag says whether `pyenv completions <cmd>` forwards to `<cmd> --complete`. Upstream decides this with a comment line, not by whether the command handles the flag (M3L "Commands with a marker").
   - On Windows only `pyenv completions` reads the table. `pyenv <cmd> --complete` keeps pyenv-win's behavior, since pyenv-win commands have no `--complete`.
6. **Windows `shell` follows pyenv-win's semantics** (M3W "Behavior by form"):
   - each name gets `-win32` on X86 unless it already ends in it, case-insensitively, and must then match a folder name exactly: no prefix resolution and no `system`;
   - the stored value joins the names with one space;
   - `--unset` counts only as the first argument and matches case-insensitively, and `--help` only as the first argument;
   - messages go to stdout;
   - there is no `-` and no `PYENV_VERSION_OLD`.
   rpyenv additionally refuses a name that isn't a safe Windows path segment (`install::is_safe_win_segment`), treating it as not installed.
7. **Windows PowerShell integration** is the profile line ``iex ((pyenv init - pwsh) -join "`n")``.
   - Spec §7's table says `pyenv init - pwsh | Invoke-Expression`, which runs each output line on its own and breaks the multi-line function. Task 7 corrects the table.
   - The Windows `pyenv` function routes `shell` only when it has arguments and the first isn't `--help`. That is pyenv-win's own `pyenv.ps1` rule (M3W "Code").
   - It doesn't route `rehash`, because PowerShell keeps no command hash.
8. **Windows POSIX and fish code uses MSYS paths** (`C:\x\y` → `/c/x/y`) whatever `MSYSTEM` says.
   - Why: the bash, zsh and fish that run on Windows with a `:`-separated `PATH` are MSYS2-based (Git Bash, MSYS2), and a `C:\…` entry can't sit in such a `PATH`.
   - Spec §7 says "when `MSYSTEM` is set"; Task 7 edits it to "always".
   - Cygwin (`/cygdrive/c`) is out of scope.
9. **The pyenv-win overlay** (`parity/pyenv_win_overlay.py`) changes three ways:
   - (a) `pyenv_file` escapes spaces with backtick-space for powershell and pwsh, as the suite's own fixture does. Today the path is split at its first space and `pyenv.exe` never starts (M3W "Failure output").
   - (b) Each test root loses pyenv-win's `bin\pyenv.ps1`, `bin\pyenv.bat` and `bin\pyenv`, so no pyenv-win entry point runs. Today the "passing" PowerShell shell tests run pyenv-win's `pyenv.ps1` (M3W "Why the other six already pass").
   - (c) The PowerShell `run_args` load rpyenv's integration first, as the user's profile would. Allowlist row **D-90**.
10. **cmd gets no integration.** An executable can't change cmd's environment, and `pyenv.exe` shadows any `pyenv.cmd` or `pyenv.bat` beside it because `.EXE` leads `PATHEXT` (M3W "How each host shell reaches pyenv-win").
    - The cmd shell tests that need the variable to change stay expected failures under **D-89**.
11. **`sh-rehash --complete` performs a rehash, as upstream's does** (M3L `sh-rehash`). Replicated, because it is cheap and only `pyenv completions sh-rehash` or a direct call reaches it.
12. **Errors after `--install`'s checks pass** (a failed `mkdir` or write) are printed as `pyenv: <path>: <reason>`, exit 1. Upstream's bash prints its own redirection error there. No test or case reaches that path, so it gets no allowlist row; the code comment says so.

New allowlist rows. Task 8 adds D-88, and Task 9 adds the rest:

| Row | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-88 | Linux | `init -` (the shell function) | The routed commands are every `pyenv-sh-*` file on PATH, including plugins' (`pyenv-commands --sh`) | `rehash` and `shell` only | Plugin dispatch is M4 (spec §14); the built-in set is rpyenv's command table |
| D-89 | Windows | `shell <version>`, `shell --unset` in cmd | `pyenv.bat` sets or removes `PYENV_VERSION` in the caller | Prints `set "PYENV_VERSION=<value>"` on stdout and a note on stderr, exits 1 | An executable can't change cmd's environment, and `pyenv.exe` precedes any batch wrapper in `PATHEXT` (spec §7) |
| D-90 | Windows | `shell` in PowerShell | `pyenv.ps1` works with no setup | Needs the profile line ``iex ((pyenv init - pwsh) -join "`n")``; without it, prints the command and exits 1. The pyenv-win suite runs with that line loaded | A `.ps1` beside `pyenv.exe` wins over it and fails when scripts are blocked (spec §7) |
| D-91 | Windows | `init`, `completions`, `sh-shell`, `sh-rehash` | `pyenv: no such command '<cmd>'`, exit 1 | Shell integration for PowerShell, Git Bash and fish | Spec §7 |

## Global Constraints

- **Linux output is byte-identical to pyenv 2.8.8** unless an allowlist row says otherwise (spec §4). Every string in Tasks 2–5 is copied from M3L or from upstream's source quoted there. Don't reword one.
- Text output uses `\n`; `Output::emit` turns it into CRLF on Windows (`crates/pyenv/src/output.rs`).
- **Never run rpyenv, pyenv-win or upstream pyenv against a real root.** Every test and probe sets `PYENV_ROOT` (and on Windows `PYENV` and `PYENV_HOME`) to a scratch directory. The user's real install is `C:\Users\JM\.pyenv\pyenv-win`.
- **No test lets `init --install` write a real startup file.** The `Fixture` sets `HOME` to its tempdir. Any new test that runs a shell sets `HOME` and `USERPROFILE` to the fixture's `base` and starts PowerShell with `-NoProfile`.
- The shims (`pyenv-shim`, `pyenv-shimw`) gain no crate: `python ci/shim_deps.py` still prints `ok`. A `windows-sys` feature adds no crate.
- WSL work runs from script files: `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/<name>.sh`. Never put `$VARS` inside `wsl … -- bash -c "…"`.
- Commits have a one-line message and no attribution lines. Never wildcard-delete in `C:\tmp`.
- Each task ends green:
  - Windows: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`;
  - WSL: the same three, run in `~/rpyenv-linux` after fetching the branch.

## Review Focus

The five inputs most likely to hurt a user that no upstream test covers, each pinned by the test named:

1. **A version value with a quote character on Windows** (a folder named `a'b`): the printed PowerShell, bash or fish code must set exactly `a'b` when evaluated. Task 6 test `quotes_survive_each_shell`.
2. **A root path with a space and a non-ASCII letter on Windows**: the `Fixture`'s `py env ñ` root must make it through `init - pwsh` into a real PowerShell's `PATH` unchanged. Task 7 test `powershell_runs_init_and_shell`.
3. **`pyenv` started by a program that isn't a shell** (a Python subprocess, an editor task): the fallback prints every shell's line, labeled, and exits 1. Task 6 test `unknown_parent_prints_every_shell`.
4. **`PYENV_SHELL=pwsh` inherited into cmd**: cmd's own `set` line wins (Decision 1). Task 6 test `cmd_parent_wins_over_inherited_pyenv_shell`.
5. **A startup file that only mentions "pyenv" in a comment**: `init --install` refuses it and writes no file, the second one included. Task 4 test `install_refuses_any_mention_and_writes_nothing`.

## File map

| File | Task | Responsibility |
|---|---|---|
| `crates/rpyenv-core/src/shellname.rs` (new) | 1 | Shell names → families; Linux parent command line; the Windows detection order |
| `crates/rpyenv-core/src/winproc.rs` | 1 | `parent_image_name()`: the parent's executable name, when trusted |
| `crates/rpyenv-core/Cargo.toml` | 1 | The `Win32_System_Diagnostics_ToolHelp` feature |
| `crates/pyenv/src/commands/shell.rs` (new) | 2 | Linux `sh-shell`, `sh-rehash` |
| `crates/pyenv/src/commands/mod.rs` | 2, 3, 5, 6, 7 | Command table rows; `names(flavor, Listing)` |
| `crates/pyenv/src/commands/misc.rs` | 2 | `commands [--sh\|--no-sh]` |
| `crates/pyenv/src/help.rs` | 2, 3, 5, 6 | Topics for `shell`, `sh-shell`, `sh-rehash`, `init`, `completions` |
| `crates/pyenv/src/lib.rs` | 2, 5, 6 | Dispatcher: `sh-* --help`, central `--complete`; `install_prefix()` |
| `crates/pyenv/src/commands/init.rs` (new) | 3, 4 | Linux `init`: help, `--detect-shell`, `-`, `--path`, `--install` |
| `completions/pyenv.{bash,zsh,fish,pwsh}`, `completions/LICENSE` (new) | 5 | Upstream's scripts, vendored |
| `crates/pyenv/src/commands/completions.rs` (new) | 5 | `completions` command and both tables |
| `crates/pyenv/src/commands/shell_win.rs` (new) | 6 | Windows `shell` (fallback), `sh-shell`, `sh-rehash` |
| `crates/pyenv/src/commands/init_win.rs` (new) | 7 | Windows `init` |
| `crates/pyenv/tests/cli_shell.rs`, `cli_init.rs`, `cli_completions.rs` (new) | 2–5 | Linux CLI tests |
| `crates/pyenv/tests/cli_shell_win.rs`, `cli_init_win.rs` (new), `common/winshell.rs` (new) | 6, 7 | Windows CLI tests that run real cmd, PowerShell and Git Bash |
| `parity/bats_run.sh`, `parity/expected/bats.txt`, `parity/diff_cases.py`, `parity/golden/linux/*` | 8 | Linux parity |
| `parity/pyenv_win_overlay.py`, `parity/expected/pyenv-win.txt`, `parity/golden/windows/*`, `parity/allowlist.py` | 9 | Windows parity; `DELIVERED` gains `M3` |
| `docs/parity/allowlist.md` | 8, 9 | D-88 to D-91 |
| `docs/specs/2026-09-27-rpyenv-design.md` | 6, 7 | §7 amendments (Decisions 1, 2, 7, 8) |

---

### Task 1: Shell names and parent-process detection

**Files:**
- Create: `crates/rpyenv-core/src/shellname.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (add `pub mod shellname;` beside the other `pub mod` lines)
- Modify: `crates/rpyenv-core/src/winproc.rs` (add `parent_image_name`)
- Modify: `crates/rpyenv-core/Cargo.toml` (feature list)

**Interfaces:**
- Produces:
  - `rpyenv_core::shellname::{Family, family(&str, Flavor) -> Family, from_env(Option<&str>, Option<&str>) -> String, from_parent_cmdline(&str, Option<&str>) -> String, windows_name(&str) -> Option<String>, windows_shell(Option<&str>, Option<&str>) -> Option<String>}`;
  - `#[cfg(unix)] rpyenv_core::shellname::parent_cmdline() -> String`;
  - `#[cfg(windows)] rpyenv_core::winproc::parent_image_name() -> Option<String>`.

- [ ] **Step 1: Write the failing tests.** Create `crates/rpyenv-core/src/shellname.rs` with only the test module below, and add `pub mod shellname;` to `lib.rs`.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// M3L "Shell detection": parent `argv[0]` values observed with `exec -a`, plus the
    /// `$SHELL` fallback. NULs are already spaces, so every command line ends in one.
    #[test]
    fn parent_cmdline_detection_matches_upstream() {
        for (cmdline, shell, want) in [
            ("bash ", None, "bash"),
            ("-bash ", None, "bash"),
            ("/usr/bin/bash ", None, "bash"),
            ("-zsh ", None, "zsh"),
            ("/usr/local/bin/zsh-5.9 ", None, "zsh"),
            ("bash-5.3 ", None, "bash"),
            ("fish ", None, "fish"),
            ("-pwsh ", None, "pwsh"),
            ("ksh93 ", None, "ksh93"),
            ("sh /tmp/script.sh ", None, "sh"),
            ("xonsh.py ", None, "xonsh.py"),
            ("/usr/bin/zsh -l ", None, "zsh"),
            ("- ", Some("/opt/x/fish"), "fish"),
            ("", Some("/opt/x/zsh"), "zsh"),
            ("", None, ""),
        ] {
            assert_eq!(from_parent_cmdline(cmdline, shell), want, "{cmdline:?}");
        }
    }

    /// `basename "${PYENV_SHELL:-$SHELL}"`: an empty PYENV_SHELL counts as unset.
    #[test]
    fn env_shell_is_the_basename_of_pyenv_shell_or_shell() {
        assert_eq!(from_env(Some("/usr/bin/pwsh"), Some("/bin/bash")), "pwsh");
        assert_eq!(from_env(Some(""), Some("/usr/bin/fish")), "fish");
        assert_eq!(from_env(None, Some("/bin/zsh/")), "zsh");
        assert_eq!(from_env(None, None), "");
    }

    /// Upstream matches exact names: on Linux `powershell` and `cmd` are POSIX names.
    #[test]
    fn families_by_flavor() {
        use Flavor::{Pyenv, PyenvWin};
        assert_eq!(family("fish", Pyenv), Family::Fish);
        assert_eq!(family("pwsh", Pyenv), Family::Pwsh);
        assert_eq!(family("powershell", Pyenv), Family::Posix);
        assert_eq!(family("powershell", PyenvWin), Family::Pwsh);
        assert_eq!(family("mksh", Pyenv), Family::Ksh);
        assert_eq!(family("cmd", Pyenv), Family::Posix);
        assert_eq!(family("cmd", PyenvWin), Family::Cmd);
        assert_eq!(family("nu", Pyenv), Family::Posix);
    }

    /// Decision 1: the parent wins over PYENV_SHELL; a parent that isn't a shell falls back.
    #[test]
    fn windows_order_is_parent_then_pyenv_shell() {
        assert_eq!(windows_name("PWSH.EXE").as_deref(), Some("pwsh"));
        assert_eq!(windows_name("Code.exe"), None);
        assert_eq!(windows_shell(Some("cmd.exe"), Some("pwsh")).as_deref(), Some("cmd"));
        assert_eq!(windows_shell(Some("python.exe"), Some("pwsh")).as_deref(), Some("pwsh"));
        assert_eq!(windows_shell(Some("python.exe"), Some("nu")), None);
        assert_eq!(windows_shell(None, None), None);
    }
}
```

- [ ] **Step 2: Run the tests and see them fail.**

Run: `cargo test -p rpyenv-core --lib shellname::`
Expected: compile errors: `from_parent_cmdline`, `Family` and the rest are not defined.

- [ ] **Step 3: Implement.** Put this above the test module in `shellname.rs`:

```rust
//! Which shell `pyenv init` and `pyenv shell` talk to (spec §7).

use crate::flavor::Flavor;

/// The code a shell name selects. Upstream's `pyenv-sh-shell`, `pyenv-sh-rehash` and most of
/// `pyenv-init` know fish, pwsh, and POSIX for every other name; `pyenv-init`'s function
/// header adds a Korn-shell variant (libexec/pyenv-init:554-563). Windows adds cmd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Posix,
    Ksh,
    Fish,
    Pwsh,
    Cmd,
}

/// The family of a normalized shell name. Upstream matches names exactly, so on Linux
/// `powershell` and `cmd` are POSIX names. On Windows `powershell` is Windows PowerShell
/// 5.1, which runs the same code as `pwsh`, and `cmd` is cmd.exe.
pub fn family(name: &str, flavor: Flavor) -> Family {
    match (name, flavor) {
        ("fish", _) => Family::Fish,
        ("pwsh", _) | ("powershell", Flavor::PyenvWin) => Family::Pwsh,
        ("ksh" | "ksh93" | "mksh", _) => Family::Ksh,
        ("cmd", Flavor::PyenvWin) => Family::Cmd,
        _ => Family::Posix,
    }
}

/// `basename "${PYENV_SHELL:-$SHELL}"` (libexec/pyenv-sh-shell:33): an empty
/// `PYENV_SHELL` counts as unset.
pub fn from_env(pyenv_shell: Option<&str>, shell: Option<&str>) -> String {
    let v = pyenv_shell.filter(|s| !s.is_empty()).or(shell).unwrap_or("");
    basename(v).to_string()
}

/// POSIX `basename`: trailing slashes go first, and `/` alone stays `/`.
fn basename(s: &str) -> &str {
    let t = s.trim_end_matches('/');
    if t.is_empty() {
        return if s.is_empty() { "" } else { "/" };
    }
    t.rsplit('/').next().unwrap_or(t)
}

/// `pyenv init`'s detection from the parent's command line, NULs already turned into spaces
/// (libexec/pyenv-init:62-66): up to the first space, one leading `-` removed, `$SHELL` when
/// that leaves nothing, then after the last `/` and before the first `-`.
pub fn from_parent_cmdline(cmdline: &str, shell: Option<&str>) -> String {
    let first = cmdline.split(' ').next().unwrap_or("");
    let first = first.strip_prefix('-').unwrap_or(first);
    let s = if first.is_empty() {
        shell.unwrap_or("")
    } else {
        first
    };
    let s = s.rsplit('/').next().unwrap_or("");
    s.split('-').next().unwrap_or("").to_string()
}

/// The parent's command line with NULs as spaces: `/proc/<ppid>/cmdline`, else
/// `ps p <ppid> -o args=` (libexec/pyenv-init:57-61). Empty when both fail.
#[cfg(unix)]
pub fn parent_cmdline() -> String {
    let ppid = std::os::unix::process::parent_id();
    if let Ok(bytes) = std::fs::read(format!("/proc/{ppid}/cmdline")) {
        return String::from_utf8_lossy(&bytes).replace('\0', " ");
    }
    std::process::Command::new("ps")
        .args(["p", &ppid.to_string(), "-o", "args="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim_end_matches('\n').to_string())
        .unwrap_or_default()
}

/// The shells rpyenv supports on Windows, by executable name.
const WINDOWS_SHELLS: [&str; 7] = ["cmd", "powershell", "pwsh", "bash", "sh", "zsh", "fish"];

/// A Windows executable or shell name as a supported shell name: `PWSH.EXE` → `pwsh`.
pub fn windows_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    WINDOWS_SHELLS.contains(&stem).then(|| stem.to_string())
}

/// The shell on Windows when integration isn't loaded (spec §7, amended 2026-10-04): the
/// parent process's image when it names a supported shell, else `PYENV_SHELL`, else none.
/// Only `pyenv init` sets `PYENV_SHELL`, so a child shell may inherit a value that names
/// its ancestor; that is why the parent comes first.
pub fn windows_shell(parent_image: Option<&str>, pyenv_shell: Option<&str>) -> Option<String> {
    parent_image
        .and_then(windows_name)
        .or_else(|| pyenv_shell.and_then(|s| windows_name(basename(s))))
}
```

- [ ] **Step 4: Run the tests and see them pass.**

Run: `cargo test -p rpyenv-core --lib shellname::`
Expected: `4 passed`.

- [ ] **Step 5: Add the Windows parent lookup.** In `crates/rpyenv-core/Cargo.toml`, add `"Win32_System_Diagnostics_ToolHelp",` to the `windows-sys` feature list, in alphabetical order (after `"Win32_Storage_FileSystem",`). Then append to `crates/rpyenv-core/src/winproc.rs`:

```rust
/// The parent process's executable file name (`cmd.exe`, `pwsh.exe`, …), when the parent
/// was created before this process. A parent that exited may have had its process ID
/// reused by a later process; the creation-time check rejects that (spec §7).
pub fn parent_image_name() -> Option<String> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn created(process: HANDLE) -> Option<u64> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
        // SAFETY: four valid FILETIME out-pointers and a process handle.
        let ok = unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) };
        (ok != 0).then(|| (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
    }

    // SAFETY: plain Win32 calls; the snapshot handle is closed on every path below.
    unsafe {
        let me = GetCurrentProcessId();
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ppid = None;
        let mut names: Vec<(u32, String)> = Vec::new();
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push((
                entry.th32ProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..len]),
            ));
            if entry.th32ProcessID == me {
                ppid = Some(entry.th32ParentProcessID);
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        let ppid = ppid?;
        let name = names.into_iter().find(|(p, _)| *p == ppid)?.1;
        let mine = created(GetCurrentProcess())?;
        let parent = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, ppid);
        if parent.is_null() {
            return None;
        }
        let theirs = created(parent);
        CloseHandle(parent);
        (theirs? < mine).then_some(name)
    }
}
```

Add a smoke test to `winproc.rs`'s test module, or create one with `#[cfg(test)] mod tests { use super::*; … }` if the file has none:

```rust
    /// Under `cargo test` the parent (cargo, or the shell that ran the test binary) is
    /// alive and older than this process. Tasks 6 and 7 check the name under cmd and
    /// PowerShell.
    #[test]
    fn a_test_process_has_a_trusted_parent() {
        let name = parent_image_name().expect("a trusted parent");
        assert!(name.to_ascii_lowercase().ends_with(".exe"), "{name}");
    }
```

- [ ] **Step 6: Run the checks.**

Run on Windows:
- `cargo test -p rpyenv-core --lib`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python ci/shim_deps.py`

Expected: everything passes, and `shim_deps.py` prints its `ok` line unchanged.

Run in WSL: `cargo test -p rpyenv-core --lib shellname::` passes.

- [ ] **Step 7: Commit.**

```bash
git add crates/rpyenv-core
git commit -m "Add shell names and the trusted parent-process lookup for shell integration"
```

### Task 2: Linux `sh-shell`, `sh-rehash`, the command listing and help

Read M3L "`pyenv shell` and `pyenv sh-shell`" and "`pyenv sh-rehash`" first.

**Files:**
- Create: `crates/pyenv/src/commands/shell.rs`
- Modify:
  - `crates/pyenv/src/commands/mod.rs`: `pub mod shell;`, the `LINUX_ONLY` rows, `Listing`, `names`;
  - `crates/pyenv/src/commands/misc.rs`: `commands`;
  - `crates/pyenv/src/help.rs`: the topics, `find`, `listing`, `help_pyenv`;
  - `crates/pyenv/src/lib.rs`: `run_pyenv`.
- Test: `crates/pyenv/tests/cli_shell.rs`

**Interfaces:**
- Consumes: `shellname::{from_env, family, Family}` (Task 1); `prefix::prefix_of`, `select::split_colon` (existing).
- Produces:
  - `commands::shell::{sh_shell, sh_rehash}: Command`;
  - `commands::Listing { All, ShOnly, NoSh }`;
  - `commands::names(Flavor, Listing) -> Vec<&'static str>`.

`names` replaces today's `names(Flavor)`, whose only caller is `misc::commands`. Task 3 uses `names(Flavor::Pyenv, Listing::ShOnly)` for the routed set.

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/cli_shell.rs`:

```rust
//! `pyenv sh-shell`, `sh-rehash`, `shell`, and the command listing on Linux (M3L).
#![cfg(unix)]

mod common;
use common::Fixture;
use std::path::Path;

fn run(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let r = f.pyenv_env(args, env);
    (r.stdout, r.stderr, r.code)
}

fn out(s: &str) -> (String, String, i32) {
    (s.to_string(), String::new(), 0)
}

#[test]
fn no_arguments_prints_code_or_fails() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["sh-shell"], &[]),
        (
            String::new(),
            "pyenv: no shell-specific version configured\n".to_string(),
            1
        )
    );
    for shell in ["bash", "fish", "pwsh"] {
        assert_eq!(
            run(&f, &["sh-shell"], &[("PYENV_SHELL", shell), ("PYENV_VERSION", "1.2.3")]),
            out("echo \"$PYENV_VERSION\"\n"),
            "{shell}"
        );
    }
    // An empty first argument counts as none.
    assert_eq!(run(&f, &["sh-shell", ""], &[]).2, 1);
}

#[test]
fn unset_per_shell() {
    let f = Fixture::new();
    let unset = |shell| run(&f, &["sh-shell", "--unset", "extra"], &[("PYENV_SHELL", shell)]);
    assert_eq!(
        unset("bash"),
        out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"\nunset PYENV_VERSION\n")
    );
    assert_eq!(
        unset("fish"),
        out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"\nset -e PYENV_VERSION\n")
    );
    assert_eq!(
        unset("pwsh"),
        out("$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $null, $Env:PYENV_VERSION\n")
    );
    // PYENV_SHELL empty: $SHELL decides.
    assert_eq!(
        run(&f, &["sh-shell", "--unset"], &[("PYENV_SHELL", ""), ("SHELL", "/usr/bin/fish")]).0,
        "set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"\nset -e PYENV_VERSION\n"
    );
}

#[test]
fn revert_per_shell() {
    let f = Fixture::new();
    let revert = |shell| run(&f, &["sh-shell", "-"], &[("PYENV_SHELL", shell)]).0;
    assert_eq!(
        revert("bash"),
        r#"if [ -n "${PYENV_VERSION_OLD+x}" ]; then
  if [ -n "$PYENV_VERSION_OLD" ]; then
    PYENV_VERSION_OLD_="$PYENV_VERSION"
    export PYENV_VERSION="$PYENV_VERSION_OLD"
    PYENV_VERSION_OLD="$PYENV_VERSION_OLD_"
    unset PYENV_VERSION_OLD_
  else
    PYENV_VERSION_OLD="$PYENV_VERSION"
    unset PYENV_VERSION
  fi
else
  echo "pyenv: PYENV_VERSION_OLD is not set" >&2
  false
fi
"#
    );
    assert!(revert("fish").starts_with("if set -q PYENV_VERSION_OLD\n"));
    // The fifth line ends with a space (libexec/pyenv-sh-shell:89).
    assert_eq!(
        revert("pwsh"),
        "if ( Get-Item -Path Env:\\PYENV_VERSION* ) {\n  $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $Env:PYENV_VERSION_OLD, $Env:PYENV_VERSION\n} else {\n  Write-Error \"pyenv: Env:PYENV_VERSION_OLD is not set\"\n  return $false\n} \n"
    );
}

#[test]
fn set_per_shell_stores_the_literal_arguments() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    let set = |args: &[&str], shell| {
        let mut a = vec!["sh-shell"];
        a.extend_from_slice(args);
        run(&f, &a, &[("PYENV_SHELL", shell)])
    };
    // `3.12` resolves for validation, but the literal argument is stored (M3L).
    assert_eq!(
        set(&["3.12"], "bash"),
        out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"\nexport PYENV_VERSION=\"3.12\"\n")
    );
    assert_eq!(
        set(&["3.12.1", "3.11.9"], "fish"),
        out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"\nset -gx PYENV_VERSION \"3.12.1:3.11.9\"\n")
    );
    assert_eq!(
        set(&["3.12.1"], "pwsh"),
        out("$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = \"3.12.1\", $Env:PYENV_VERSION\n")
    );
    // The same value as the current one prints nothing.
    assert_eq!(
        run(&f, &["sh-shell", "3.12.1"], &[("PYENV_VERSION", "3.12.1")]),
        out("")
    );
}

#[test]
fn a_version_not_installed_fails_with_false() {
    let f = Fixture::new();
    f.version("3.12.1");
    assert_eq!(
        run(&f, &["sh-shell", "1.2.3"], &[]),
        (
            "false\n".to_string(),
            "pyenv: version `1.2.3' not installed\n".to_string(),
            1
        )
    );
    // Every argument is a version name, options included.
    assert_eq!(
        run(&f, &["sh-shell", "3.12.1", "--unset"], &[]).1,
        "pyenv: version `--unset' not installed\n"
    );
}

#[test]
fn sh_rehash_prints_code_and_does_not_rehash() {
    let f = Fixture::new();
    let rehash = |shell| run(&f, &["sh-rehash", "x"], &[("PYENV_SHELL", shell)]);
    assert_eq!(
        rehash("bash"),
        out("command pyenv rehash\nhash -r 2>/dev/null || true\n")
    );
    assert_eq!(rehash("fish"), out("command pyenv rehash\n"));
    assert_eq!(
        rehash("pwsh"),
        out("& (get-command pyenv -commandtype application) rehash\n")
    );
    assert!(!f.root.join("shims").exists());
}

const SHELL_HELP: &str = "Usage: pyenv shell <version>...\n       pyenv shell -\n       pyenv shell --unset\n\nSets a shell-specific Python version by setting the `PYENV_VERSION'\nenvironment variable in your shell. This version overrides local\napplication-specific versions and the global version.\n\n<version> should be a string matching a Python version known to pyenv.\nThe special version string `system' will use your default system Python.\nRun `pyenv versions' for a list of available Python versions.\n\nWhen `-` is passed instead of the version string, the previously set\nversion will be restored. With `--unset`, the `PYENV_VERSION`\nenvironment variable gets unset, restoring the environment to the\nstate before the first `pyenv shell` call.\n\n";

#[test]
fn shell_without_integration_and_help() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["shell", "3.12"], &[]),
        (
            String::new(),
            "pyenv: shell integration not enabled. Run `pyenv init' for instructions.\n"
                .to_string(),
            1
        )
    );
    // The dispatcher prints the help command for the shell function to evaluate.
    assert_eq!(
        run(&f, &["sh-shell", "--help"], &[]),
        out("pyenv help \"sh-shell\"\n")
    );
    assert_eq!(
        run(&f, &["sh-rehash", "--help"], &[]),
        out("pyenv help \"sh-rehash\"\n")
    );
    assert_eq!(run(&f, &["help", "shell"], &[]), out(SHELL_HELP));
    assert_eq!(run(&f, &["help", "sh-shell"], &[]), out(SHELL_HELP));
    assert_eq!(
        run(&f, &["help", "--usage", "shell"], &[]),
        out("Usage: pyenv shell <version>...\n       pyenv shell -\n       pyenv shell --unset\n")
    );
    assert_eq!(
        run(&f, &["help", "sh-rehash"], &[]),
        (
            String::new(),
            "Sorry, this command isn't documented yet.\n".to_string(),
            1
        )
    );
    assert_eq!(run(&f, &["help", "--usage", "sh-rehash"], &[]), out(""));
}

#[test]
fn commands_lists_sh_commands_by_their_short_name() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["commands", "--sh"], &[]), out("rehash\nshell\n"));
    let all = run(&f, &["commands"], &[]).0;
    assert_eq!(all.lines().filter(|l| *l == "shell").count(), 1);
    assert_eq!(all.lines().filter(|l| *l == "rehash").count(), 1);
    assert!(!all.contains("sh-"));
    let no_sh = run(&f, &["commands", "--no-sh"], &[]).0;
    assert!(no_sh.lines().any(|l| l == "rehash"));
    assert!(!no_sh.lines().any(|l| l == "shell"));
    assert!(run(&f, &["help"], &[])
        .0
        .contains("   shell       Set or show the shell-specific Python version\n"));
}

/// The printed code, evaluated by a real bash.
#[test]
fn bash_evaluates_the_code() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_pyenv"), f.syspath.join("pyenv")).unwrap();
    let script = r#"set -e
eval "$(pyenv sh-shell 3.12.1)"; echo "a[$PYENV_VERSION][${PYENV_VERSION_OLD-unset}]"
eval "$(pyenv sh-shell 3.11.9)"; echo "b[$PYENV_VERSION][$PYENV_VERSION_OLD]"
eval "$(pyenv sh-shell -)"; echo "c[$PYENV_VERSION][$PYENV_VERSION_OLD]"
eval "$(pyenv sh-shell --unset)"; echo "d[${PYENV_VERSION-unset}][$PYENV_VERSION_OLD]"
"#;
    let o = f
        .command(Path::new("/bin/bash"), &f.work, &[("PYENV_SHELL", "bash")])
        .arg("-c")
        .arg(script)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "a[3.12.1][]\nb[3.11.9][3.12.1]\nc[3.12.1][3.11.9]\nd[unset][3.12.1]\n",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
```

- [ ] **Step 2: Run the tests and see them fail.**

Run in WSL: `cargo test -p pyenv --test cli_shell`
Expected: most tests fail with ``pyenv: no such command `sh-shell'``. `shell_without_integration_and_help` fails on the `sh-shell --help` assertion.

- [ ] **Step 3: Implement `shell.rs`.**

```rust
//! `pyenv sh-shell` and `pyenv sh-rehash` on Linux (libexec/pyenv-sh-shell and
//! pyenv-sh-rehash at 2.8.8; M3L). They print shell code for the `pyenv` function that
//! `pyenv init -` defines to evaluate. Windows' are in `shell_win.rs`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::shellname::{self, Family};
use rpyenv_core::{prefix, select};

/// `basename "${PYENV_SHELL:-$SHELL}"`, as the family the scripts tell apart: fish, pwsh,
/// or POSIX for every other name.
fn env_family(ctx: &Ctx) -> Family {
    let get = |k: &str| std::env::var(k).ok();
    let name = shellname::from_env(get("PYENV_SHELL").as_deref(), get("SHELL").as_deref());
    match shellname::family(&name, ctx.flavor) {
        Family::Fish => Family::Fish,
        Family::Pwsh => Family::Pwsh,
        _ => Family::Posix,
    }
}

const REVERT_POSIX: &str = r#"if [ -n "${PYENV_VERSION_OLD+x}" ]; then
  if [ -n "$PYENV_VERSION_OLD" ]; then
    PYENV_VERSION_OLD_="$PYENV_VERSION"
    export PYENV_VERSION="$PYENV_VERSION_OLD"
    PYENV_VERSION_OLD="$PYENV_VERSION_OLD_"
    unset PYENV_VERSION_OLD_
  else
    PYENV_VERSION_OLD="$PYENV_VERSION"
    unset PYENV_VERSION
  fi
else
  echo "pyenv: PYENV_VERSION_OLD is not set" >&2
  false
fi
"#;

const REVERT_FISH: &str = r#"if set -q PYENV_VERSION_OLD
  if [ -n "$PYENV_VERSION_OLD" ]
    set PYENV_VERSION_OLD_ "$PYENV_VERSION"
    set -gx PYENV_VERSION "$PYENV_VERSION_OLD"
    set -gu PYENV_VERSION_OLD "$PYENV_VERSION_OLD_"
    set -e PYENV_VERSION_OLD_
  else
    set -gu PYENV_VERSION_OLD "$PYENV_VERSION"
    set -e PYENV_VERSION
  end
else
  echo "pyenv: PYENV_VERSION_OLD is not set" >&2
  false
end
"#;

/// The line `} ` ends with a space, as upstream's heredoc does (libexec/pyenv-sh-shell:89).
const REVERT_PWSH: &str = "if ( Get-Item -Path Env:\\PYENV_VERSION* ) {\n  $Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $Env:PYENV_VERSION_OLD, $Env:PYENV_VERSION\n} else {\n  Write-Error \"pyenv: Env:PYENV_VERSION_OLD is not set\"\n  return $false\n} \n";

/// `pyenv sh-shell [<version>...|-|--unset]`. Only the first argument is checked for
/// `--unset` and `-`; any other argument list is a list of versions.
pub fn sh_shell(ctx: &Ctx, args: &[&str]) -> Output {
    let family = env_family(ctx);
    let mut o = Output::new();
    match args.first().copied() {
        None | Some("") => match &ctx.pyenv_version {
            None => return Output::error("pyenv: no shell-specific version configured"),
            Some(_) => o.out("echo \"$PYENV_VERSION\""),
        },
        Some("--unset") => match family {
            Family::Fish => {
                o.out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"");
                o.out("set -e PYENV_VERSION");
            }
            Family::Pwsh => {
                o.out("$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = $null, $Env:PYENV_VERSION")
            }
            _ => {
                o.out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"");
                o.out("unset PYENV_VERSION");
            }
        },
        Some("-") => o.stdout.push_str(match family {
            Family::Fish => REVERT_FISH,
            Family::Pwsh => REVERT_PWSH,
            _ => REVERT_POSIX,
        }),
        Some(_) => {
            // `pyenv-prefix "${versions[@]}"` joins the arguments with `:`, splits them
            // again and resolves each: the first failure prints its message.
            let joined = args.join(":");
            for name in select::split_colon(&joined) {
                if let Err(e) = prefix::prefix_of(ctx, &name) {
                    o.err(e.message());
                    o.out("false");
                    return o.with_code(1);
                }
            }
            if ctx.pyenv_version.as_deref() != Some(joined.as_str()) {
                match family {
                    Family::Fish => {
                        o.out("set -gu PYENV_VERSION_OLD \"$PYENV_VERSION\"");
                        o.out(format!("set -gx PYENV_VERSION \"{joined}\""));
                    }
                    Family::Pwsh => o.out(format!(
                        "$Env:PYENV_VERSION, $Env:PYENV_VERSION_OLD = \"{joined}\", $Env:PYENV_VERSION"
                    )),
                    _ => {
                        o.out("PYENV_VERSION_OLD=\"${PYENV_VERSION-}\"");
                        o.out(format!("export PYENV_VERSION=\"{joined}\""));
                    }
                }
            }
        }
    }
    o
}

/// `pyenv sh-rehash`: the code that rehashes and, outside fish and pwsh, empties the
/// shell's command hash. It doesn't rehash itself; arguments are ignored.
pub fn sh_rehash(ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    match env_family(ctx) {
        Family::Pwsh => o.out("& (get-command pyenv -commandtype application) rehash"),
        Family::Fish => o.out("command pyenv rehash"),
        _ => {
            o.out("command pyenv rehash");
            o.out("hash -r 2>/dev/null || true");
        }
    }
    o
}
```

- [ ] **Step 4: Register the commands and the listing.** In `crates/pyenv/src/commands/mod.rs`:
  - add `pub mod shell;` in alphabetical order;
  - add these two rows to `LINUX_ONLY` (they need no `#[cfg]`):

```rust
    ("sh-rehash", shell::sh_rehash),
    ("sh-shell", shell::sh_shell),
```

Replace `names` with:

```rust
/// Which commands `pyenv commands` lists (libexec/pyenv-commands:15-47).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// Every command, `sh-` stripped, so `sh-shell` lists as `shell`.
    All,
    /// `--sh`: only the `sh-` commands, stripped.
    ShOnly,
    /// `--no-sh`: every command but the `sh-` ones.
    NoSh,
}

/// The names `pyenv commands` prints, each once, in the flavor's order.
pub fn names(flavor: Flavor, listing: Listing) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = table(flavor)
        .filter_map(|&(n, _)| {
            let short = n.strip_prefix("sh-");
            match listing {
                Listing::All => Some(short.unwrap_or(n)),
                Listing::ShOnly => short,
                Listing::NoSh => short.is_none().then_some(n),
            }
        })
        .collect();
    match flavor {
        // `sort -u` in the C locale.
        Flavor::Pyenv => names.sort_unstable(),
        // pyenv-win lists `libexec\pyenv-<name>.<ext>` in NTFS order (allowlist D-16).
        Flavor::PyenvWin => names.sort_by_key(|n| format!("{}.", n.to_ascii_uppercase())),
    }
    names.dedup();
    names
}
```

In `crates/pyenv/src/commands/misc.rs`, replace `commands`:

```rust
/// `pyenv commands [--sh|--no-sh]`. pyenv-win's has no options.
pub fn commands(ctx: &Ctx, args: &[&str]) -> Output {
    let listing = match (ctx.flavor, args.first()) {
        (Flavor::Pyenv, Some(&"--sh")) => super::Listing::ShOnly,
        (Flavor::Pyenv, Some(&"--no-sh")) => super::Listing::NoSh,
        _ => super::Listing::All,
    };
    let mut o = Output::new();
    for name in super::names(ctx.flavor, listing) {
        o.out(name);
    }
    o
}
```

- [ ] **Step 5: Add the dispatcher rule.** In `crates/pyenv/src/lib.rs` `run_pyenv`, put a new first arm in the `match commands::lookup(Flavor::Pyenv, cmd)`:

```rust
        // `pyenv sh-<cmd> --help` prints the help command for the shell function to
        // evaluate (libexec/pyenv:133-136).
        Some(_) if rest.first() == Some(&"--help") && cmd.starts_with("sh-") => {
            let mut o = Output::new();
            o.out(format!("pyenv help \"{cmd}\""));
            o
        }
```

- [ ] **Step 6: Add the help topics.** In `crates/pyenv/src/help.rs`, add the three topics below to `PYENV`, keeping its alphabetical order. Copy `SHELL_HELP` from the test file in Step 1.

```rust
const SHELL_USAGE: &str =
    "Usage: pyenv shell <version>...\n       pyenv shell -\n       pyenv shell --unset";
```

```rust
    topic("sh-rehash", None, None, ""),
    topic("sh-shell", None, Some(SHELL_USAGE), SHELL_HELP),
    topic("shell", Some("Set or show the shell-specific Python version"), Some(SHELL_USAGE), SHELL_HELP),
```

Make `find` accept a name whose command exists only as `sh-<name>`, as `pyenv-help` falls back to `pyenv-sh-<cmd>` (libexec/pyenv-help:26). Change its first line from `commands::lookup(flavor, name)?;` to:

```rust
    commands::lookup(flavor, name)
        .or_else(|| commands::lookup(flavor, &format!("sh-{name}")))?;
```

In `listing`, change the filter to `t.summary.is_some() && find(Flavor::Pyenv, t.name).is_some()`.

In `help_pyenv`, an empty topic text means the script has no comment block. Replace the last arm (`Some(t) => { o.stdout.push_str(t.text); o }`) with:

```rust
        // `pyenv-sh-rehash` has no Summary or Usage block (M3L).
        Some(t) if t.text.is_empty() => {
            Output::error("Sorry, this command isn't documented yet.")
        }
        Some(t) => {
            o.stdout.push_str(t.text);
            o
        }
```

- [ ] **Step 7: Run the tests and see them pass.**

Run in WSL: `cargo test -p pyenv --test cli_shell`
Expected: `9 passed`.

Then, on both OSes: `cargo test --workspace`. The existing `cli_dispatch` and `commands` tests must still pass. If a golden-based parity test changes, check whether the listing now has `shell` and is otherwise unchanged; Task 8 regenerates goldens.

- [ ] **Step 8: Commit.**

```bash
git add crates/pyenv
git commit -m "Add pyenv sh-shell and sh-rehash, and list sh- commands by their short name"
```

---

### Task 3: Linux `pyenv init`: help, `--detect-shell`, `-` and `--path`

Read M3L "`pyenv init`" up to, but not including, "`--install`". The code below follows `libexec/pyenv-init` at 2.8.8 line for line. `--install` is Task 4. Until then, `--install` is read as a shell name, which is not yet correct.

**Files:**
- Create: `crates/pyenv/src/commands/init.rs`
- Modify:
  - `crates/pyenv/src/commands/mod.rs`: `pub mod init;` and the `LINUX_ONLY` row `("init", init::init)`;
  - `crates/pyenv/src/lib.rs`: `install_prefix()`;
  - `crates/pyenv/src/help.rs`: the `init` topic.
- Test: `crates/pyenv/tests/cli_init.rs`

**Interfaces:**
- Consumes: `commands::names(Flavor, Listing::ShOnly)` (Task 2); `shellname::{from_parent_cmdline, parent_cmdline}` (Task 1); `rpyenv_core::launch::io_reason` (existing).
- Produces, for Tasks 4 and 7:
  - `commands::init::{parse, Args, Mode}`;
  - `init_dirs(&Ctx) -> Result<(), Output>`;
  - `posix_path_lines(shims: &str, no_push_path: bool) -> Vec<String>`;
  - `fish_path_lines(shims: &str, no_push_path: bool) -> Vec<String>`;
  - `posix_function(header: &str, routed: &[&str]) -> String`;
  - `fish_function(routed: &[&str]) -> String`;
  - `crate::install_prefix() -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/cli_init.rs`:

```rust
//! `pyenv init` on Linux: help, --detect-shell, print and path modes (M3L "pyenv init").
#![cfg(unix)]

mod common;
use common::Fixture;
use std::path::Path;

fn init(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut a = vec!["init"];
    a.extend_from_slice(args);
    let r = f.pyenv_env(&a, env);
    (r.stdout, r.stderr, r.code)
}

fn root(f: &Fixture) -> String {
    f.root.display().to_string()
}

#[test]
fn detect_shell_reports_profile_and_rc() {
    let f = Fixture::new();
    let detect = |shell| init(&f, &["--detect-shell", shell], &[]).0;
    assert_eq!(
        detect("bash"),
        "PYENV_SHELL_DETECT=bash\nPYENV_PROFILE_DETECT=~/.profile\nPYENV_RC_DETECT=~/.bashrc\n"
    );
    assert_eq!(
        detect("zsh"),
        "PYENV_SHELL_DETECT=zsh\nPYENV_PROFILE_DETECT=~/.zprofile\nPYENV_RC_DETECT=~/.zshrc\n"
    );
    assert_eq!(
        detect("fish"),
        "PYENV_SHELL_DETECT=fish\nPYENV_PROFILE_DETECT=~/.config/fish/config.fish\nPYENV_RC_DETECT=~/.config/fish/config.fish\n"
    );
    assert_eq!(
        detect("pwsh"),
        "PYENV_SHELL_DETECT=pwsh\nPYENV_PROFILE_DETECT=~/.config/powershell/profile.ps1\nPYENV_RC_DETECT=~/.config/powershell/profile.ps1\n"
    );
    assert_eq!(
        detect("mksh"),
        "PYENV_SHELL_DETECT=mksh\nPYENV_PROFILE_DETECT=~/.profile\nPYENV_RC_DETECT=~/.profile\n"
    );
    assert_eq!(
        detect("nu"),
        "PYENV_SHELL_DETECT=nu\nPYENV_PROFILE_DETECT=\nPYENV_RC_DETECT=\n"
    );
    f.file(&f.base.join(".bash_profile"), "");
    assert!(detect("bash").contains("PYENV_PROFILE_DETECT=~/.bash_profile\n"));
    // The last mode flag and the last other argument win.
    assert_eq!(
        init(&f, &["--detect-shell", "zsh", "fish"], &[]).0.lines().next(),
        Some("PYENV_SHELL_DETECT=fish")
    );
}

const TAIL: &str = "\n# Restart your shell for the changes to take effect.\n\n";

#[test]
fn help_for_each_family() {
    let f = Fixture::new();
    let r = root(&f);
    let bash = init(&f, &["bash"], &[]);
    assert_eq!(
        bash,
        (
            String::new(),
            format!("# Load pyenv automatically by appending\n# the following to \n# ~/.bash_profile if it exists, otherwise ~/.profile (for login shells)\n# and ~/.bashrc (for interactive shells) :\n\nexport PYENV_ROOT=\"{r}\"\n[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"\neval \"$(pyenv init - bash)\"\n{TAIL}"),
            1
        )
    );
    assert!(init(&f, &["zsh"], &[]).1.contains("# ~/.zprofile (for login shells)\n# and ~/.zshrc (for interactive shells) :\n"));
    assert_eq!(
        init(&f, &["ksh"], &[]).1,
        format!("# Load pyenv automatically by appending\n# the following to ~/.profile :\n\nexport PYENV_ROOT=\"{r}\"\n[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"\neval \"$(pyenv init - ksh)\"\n{TAIL}")
    );
    assert!(init(&f, &["sh"], &[]).1.contains("# your shell's login startup file (for login shells)\n# and your shell's interactive startup file (for interactive shells) :\n"));
    // The default root keeps the portable `$HOME` form.
    let default = [("PYENV_ROOT", f.base.join(".pyenv").display().to_string())];
    let default: Vec<(&str, &str)> = default.iter().map(|(k, v)| (*k, v.as_str())).collect();
    assert!(init(&f, &["bash"], &default).1.contains("\nexport PYENV_ROOT=\"$HOME/.pyenv\"\n"));
    assert_eq!(
        init(&f, &["fish"], &default).1,
        format!("# Add pyenv executable to PATH by running\n# the following interactively:\n\nset -Ux PYENV_ROOT $HOME/.pyenv\nif functions -q fish_add_path\n  test -d $PYENV_ROOT/bin; and fish_add_path $PYENV_ROOT/bin\nelse\n  test -d $PYENV_ROOT/bin; and set -U fish_user_paths $PYENV_ROOT/bin $fish_user_paths\nend\n\n# Load pyenv automatically by appending\n# the following to ~/.config/fish/config.fish:\n\npyenv init - fish | source\n\n{TAIL}")
    );
    assert_eq!(
        init(&f, &["pwsh"], &default).1,
        format!("# Load pyenv automatically by appending\n# the following to ~/.config/powershell/profile.ps1 :\n\n$Env:PYENV_ROOT=\"$Env:HOME/.pyenv\"\nif (Test-Path -LP \"$Env:PYENV_ROOT/bin\" -PathType Container) {{\n  $Env:PATH=\"$Env:PYENV_ROOT/bin:$Env:PATH\" }}\niex ((pyenv init -) -join \"`n\")\n{TAIL}")
    );
}

/// test/init.bats:161: a custom root is quoted for each shell.
#[test]
fn custom_root_is_quoted_per_shell() {
    let f = Fixture::new();
    let odd = format!("{}/a$b\"c`d\\e", f.base.display());
    let env = [("PYENV_ROOT", odd.as_str())];
    let b = f.base.display();
    assert!(init(&f, &["bash"], &env).1.contains(&format!("\nexport PYENV_ROOT=\"{b}/a\\$b\\\"c\\`d\\\\e\"\n")));
    assert!(init(&f, &["fish"], &env).1.contains(&format!("\nset -Ux PYENV_ROOT \"{b}/a\\$b\\\"c`d\\\\e\"\n")));
    let quote = format!("{}/it's", f.base.display());
    assert!(init(&f, &["pwsh"], &[("PYENV_ROOT", quote.as_str())]).1.contains(&format!("\n$Env:PYENV_ROOT='{b}/it''s'\n")));
}

#[test]
fn print_mode_for_bash() {
    let f = Fixture::new();
    let r = root(&f);
    let (out, err, code) = init(&f, &["-", "bash"], &[]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert_eq!(
        out,
        format!(r#"PATH="$(bash --norc -ec 'IFS=:; paths=($PATH); 
for i in ${{!paths[@]}}; do 
if [[ ${{paths[i]}} == "''{r}/shims''" ]]; then unset '\''paths[i]'\''; 
fi; done; 
echo "${{paths[*]}}"')"
export PATH="{r}/shims:${{PATH}}"
export PYENV_SHELL=bash
command pyenv rehash
pyenv() {{
  local command=${{1:-}}
  [ "$#" -gt 0 ] && shift
  case "$command" in
  rehash|shell)
    eval "$(pyenv "sh-$command" "$@")"
    ;;
  *)
    command pyenv "$command" "$@"
    ;;
  esac
}}
"#)
    );
    assert!(f.root.join("shims").is_dir() && f.root.join("versions").is_dir());
}

#[test]
fn print_mode_for_fish_and_pwsh() {
    let f = Fixture::new();
    let r = root(&f);
    assert_eq!(
        init(&f, &["-", "fish", "--no-rehash"], &[]).0,
        format!("while set pyenv_index (contains -i -- \"{r}/shims\" $PATH)\nset -eg PATH[$pyenv_index]; end; set -e pyenv_index\nset -gx PATH '{r}/shims' $PATH\nset -gx PYENV_SHELL fish\nfunction pyenv\n  set command $argv[1]\n  set -e argv[1]\n\n  switch \"$command\"\n  case rehash shell\n    source (pyenv \"sh-$command\" $argv|psub)\n  case \"*\"\n    command pyenv \"$command\" $argv\n  end\nend\n")
    );
    assert_eq!(
        init(&f, &["-", "pwsh"], &[]).0,
        format!("$Env:PATH=\"$(($Env:PATH -split ':' | where {{ -not ($_ -match '{r}/shims') }}) -join ':')\"\n$Env:PATH=\"{r}/shims:$Env:PATH\"\n$Env:PYENV_SHELL=\"pwsh\"\n& pyenv rehash\nfunction pyenv {{\n  $command=\"\"\n  if ( $args.Count -gt 0 ) {{\n    $command, $args = $args\n  }}\n\n  if ( (\"rehash shell\" -split ' ') -contains $command ) {{\n    $shell_cmds = (& (get-command -commandtype application pyenv -totalcount 1) sh-$command $args)\n    if ( $shell_cmds.Count -gt 0 ) {{\n      iex ($shell_cmds -join \"`n\")\n    }}\n  }} else {{\n    & (get-command -commandtype application pyenv -totalcount 1) $command $args\n  }}\n}}\n")
    );
}

#[test]
fn ksh_header_no_push_path_and_path_mode() {
    let f = Fixture::new();
    let r = root(&f);
    let ksh = init(&f, &["-", "ksh", "--no-push-path", "--no-rehash"], &[]).0;
    assert!(ksh.starts_with(&format!("if [[ \":$PATH:\" != *':{r}/shims:'* ]]; then\nexport PATH=\"{r}/shims:${{PATH}}\"\nfi\nexport PYENV_SHELL=ksh\nfunction pyenv {{\n  typeset command=${{1:-}}\n")));
    assert!(init(&f, &["-", "fish", "--no-push-path"], &[]).0.starts_with(&format!("if not contains -- \"{r}/shims\" $PATH\nset -gx PATH '{r}/shims' $PATH\nend\n")));
    assert!(init(&f, &["-", "pwsh", "--no-push-path"], &[]).0.starts_with(&format!("if ( $Env:PATH -notmatch \"{r}/shims\" ) {{\n$Env:PATH=\"{r}/shims:$Env:PATH\"\n}}\n")));
    // --path: PATH lines and the rehash only, and no directories are created.
    let fresh = f.base.join("fresh");
    let fresh_s = fresh.display().to_string();
    let (out, _, code) = init(&f, &["--path", "sh"], &[("PYENV_ROOT", fresh_s.as_str())]);
    assert_eq!(code, 0);
    assert!(out.ends_with(&format!("export PATH=\"{fresh_s}/shims:${{PATH}}\"\ncommand pyenv rehash\n")));
    assert!(!fresh.exists());
}

/// The completion line names `<prefix>/completions/pyenv.<shell>` when that file is
/// readable; the prefix is the parent of the binary's folder (Decision 3).
#[test]
fn completion_line_next_to_an_installed_binary() {
    let f = Fixture::new();
    let inst = f.base.join("inst");
    std::fs::create_dir_all(inst.join("bin")).unwrap();
    let exe = inst.join("bin").join("pyenv");
    std::fs::copy(env!("CARGO_BIN_EXE_pyenv"), &exe).unwrap();
    for s in ["bash", "pwsh"] {
        f.file(&inst.join("completions").join(format!("pyenv.{s}")), "");
    }
    let run = |shell: &str| {
        let o = f
            .command(&exe, &f.work, &[])
            .args(["init", "-", shell])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let i = inst.display();
    assert!(run("bash").contains(&format!("\nsource '{i}/completions/pyenv.bash'\n")));
    assert!(run("pwsh").contains(&format!("\niex (gc {i}/completions/pyenv.pwsh -Raw)\n")));
    assert!(!run("zsh").contains("completions"));
}

#[test]
fn mkdir_failure_prints_mkdirs_messages() {
    use std::os::unix::fs::PermissionsExt;
    if unsafe { libc::geteuid() } == 0 {
        return; // root can create the directory anyway
    }
    let f = Fixture::new();
    let ro = f.base.join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = ro.join("root").display().to_string();
    let got = init(&f, &["-", "bash"], &[("PYENV_ROOT", r.as_str())]);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    let msg = format!("mkdir: cannot create directory \u{2018}{r}\u{2019}: Permission denied\n");
    assert_eq!(got, (String::new(), format!("{msg}{msg}"), 1));
}

/// Help mode detects the parent shell; `"$0" init; true` keeps bash from exec'ing pyenv.
#[test]
fn help_mode_detects_the_parent_shell() {
    let f = Fixture::new();
    let o = f
        .command(Path::new("/bin/bash"), &f.work, &[])
        .args(["-c", "\"$0\" init; echo \"rc=$?\"; \"$0\" init --detect-shell; true", env!("CARGO_BIN_EXE_pyenv")])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(String::from_utf8_lossy(&o.stderr).contains("eval \"$(pyenv init - bash)\""));
    assert!(out.starts_with("rc=1\nPYENV_SHELL_DETECT=bash\n"), "{out}");
}

/// The printed code, evaluated by a real bash: shims end up first, once, and the function
/// routes `shell` through `sh-shell`.
#[test]
fn bash_evaluates_init_and_shell() {
    let f = Fixture::new();
    f.version("3.12.1");
    for (name, target) in [("pyenv", env!("CARGO_BIN_EXE_pyenv")), ("bash", "/bin/bash")] {
        std::os::unix::fs::symlink(target, f.syspath.join(name)).unwrap();
    }
    let shims = f.root.join("shims").display().to_string();
    let sys = f.syspath.display().to_string();
    let path = format!("{shims}:{sys}:{shims}");
    let script = r#"eval "$(pyenv init - bash --no-rehash)"
pyenv shell 3.12.1
echo "[$PYENV_VERSION]"
pyenv shell
echo "$PATH"
type -t pyenv
"#;
    let o = f
        .command(Path::new("/bin/bash"), &f.work, &[("PATH", path.as_str())])
        .args(["-c", script])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        format!("[3.12.1]\n3.12.1\n{shims}:{sys}\nfunction\n"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
```

The `libc` dev-dependency already exists for Unix. If `cli_init.rs` can't see it, use `#[cfg(unix)] extern crate libc;` the way other test files do (`grep -rn libc crates/pyenv/tests`).

- [ ] **Step 2: Run the tests and see them fail.**

Run in WSL: `cargo test -p pyenv --test cli_init`
Expected: every test fails with ``pyenv: no such command `init'``.

- [ ] **Step 3: Add `install_prefix`.** In `crates/pyenv/src/lib.rs`, next to `shim_exe`:

```rust
/// The folder above the one holding this `pyenv` binary: upstream's
/// `_PYENV_INSTALL_PREFIX`, which its dispatcher derives the same way and overwrites any
/// inherited value with (libexec/pyenv:79-82; Decision 3).
pub(crate) fn install_prefix() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.to_path_buf())
}
```

- [ ] **Step 4: Implement `init.rs`.**

```rust
//! `pyenv init` on Linux (libexec/pyenv-init at 2.8.8; M3L "pyenv init"). Windows' is
//! `init_win.rs`, which reuses the argument loop and the POSIX and fish code below.

use crate::commands::{self, Listing};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::shellname;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Help,
    Print,
    Path,
    DetectShell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Args {
    pub mode: Mode,
    pub no_push_path: bool,
    pub no_rehash: bool,
    pub shell: Option<String>,
}

/// The argument loop (libexec/pyenv-init:28-53). Every argument is read: the last mode
/// flag wins, and any other argument, unknown options included, is the shell name, the
/// last one winning.
pub(crate) fn parse(args: &[&str]) -> Args {
    let mut a = Args {
        mode: Mode::Help,
        no_push_path: false,
        no_rehash: false,
        shell: None,
    };
    for &arg in args {
        match arg {
            "-" => a.mode = Mode::Print,
            "--path" => a.mode = Mode::Path,
            "--detect-shell" => a.mode = Mode::DetectShell,
            "--no-push-path" => a.no_push_path = true,
            "--no-rehash" => a.no_rehash = true,
            other => a.shell = Some(other.to_string()),
        }
    }
    a
}

/// The shell argument, or the one detected from the parent's command line
/// (libexec/pyenv-init:56-67).
fn shell_name(given: Option<String>) -> String {
    given.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        #[cfg(unix)]
        let cmdline = shellname::parent_cmdline();
        #[cfg(not(unix))]
        let cmdline = String::new();
        let shell = std::env::var("SHELL").ok();
        shellname::from_parent_cmdline(&cmdline, shell.as_deref())
    })
}

pub fn init(ctx: &Ctx, args: &[&str]) -> Output {
    let a = parse(args);
    let shell = shell_name(a.shell.clone());
    match a.mode {
        Mode::Help => help(ctx, &shell),
        Mode::DetectShell => {
            let p = detect_profile(&shell);
            let mut o = Output::new();
            o.out(format!("PYENV_SHELL_DETECT={shell}"));
            o.out(format!("PYENV_PROFILE_DETECT={}", p.profile));
            o.out(format!("PYENV_RC_DETECT={}", p.rc));
            o
        }
        Mode::Path => {
            let mut o = Output::new();
            print_path(&mut o, ctx, &shell, a.no_push_path);
            print_rehash(&mut o, &shell, a.no_rehash);
            o
        }
        Mode::Print => {
            if let Err(o) = init_dirs(ctx) {
                return o;
            }
            let mut o = Output::new();
            print_path(&mut o, ctx, &shell, a.no_push_path);
            print_env(&mut o, &shell);
            print_completion(&mut o, &shell);
            print_rehash(&mut o, &shell, a.no_rehash);
            o.stdout.push_str(&shell_function(&shell));
            o
        }
    }
}

/// `HOME`, as the script sees it (empty when unset).
pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// What `detect_profile` sets (libexec/pyenv-init:103-142). The `~` is literal.
pub(crate) struct Profile {
    pub profile: &'static str,
    pub rc: &'static str,
    pub profile_explain: Option<&'static str>,
    pub rc_explain: Option<&'static str>,
}

pub(crate) fn detect_profile(shell: &str) -> Profile {
    let p = |profile, rc| Profile {
        profile,
        rc,
        profile_explain: None,
        rc_explain: None,
    };
    match shell {
        "bash" => Profile {
            profile: if Path::new(&format!("{}/.bash_profile", home())).exists() {
                "~/.bash_profile"
            } else {
                "~/.profile"
            },
            rc: "~/.bashrc",
            profile_explain: Some("~/.bash_profile if it exists, otherwise ~/.profile"),
            rc_explain: None,
        },
        "fish" => p("~/.config/fish/config.fish", "~/.config/fish/config.fish"),
        "pwsh" => p("~/.config/powershell/profile.ps1", "~/.config/powershell/profile.ps1"),
        "zsh" => p("~/.zprofile", "~/.zshrc"),
        "ksh" | "ksh93" | "mksh" => p("~/.profile", "~/.profile"),
        _ => Profile {
            profile: "",
            rc: "",
            profile_explain: Some("your shell's login startup file"),
            rc_explain: Some("your shell's interactive startup file"),
        },
    }
}

fn root(ctx: &Ctx) -> String {
    ctx.root.to_string_lossy().into_owned()
}

/// `[[ ${PYENV_ROOT} == "${HOME}/.pyenv" ]]`, a plain string comparison.
fn root_is_default(ctx: &Ctx) -> bool {
    root(ctx) == format!("{}/.pyenv", home())
}

/// The `PYENV_ROOT` line of the help text and of `--install` (libexec/pyenv-init:197-258).
fn root_line(ctx: &Ctx, family: &str) -> String {
    let r = root(ctx);
    let default = root_is_default(ctx);
    match family {
        "fish" if default => "set -Ux PYENV_ROOT $HOME/.pyenv".to_string(),
        "fish" => format!(
            "set -Ux PYENV_ROOT \"{}\"",
            r.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$")
        ),
        "pwsh" if default => "$Env:PYENV_ROOT=\"$Env:HOME/.pyenv\"".to_string(),
        "pwsh" => format!("$Env:PYENV_ROOT='{}'", r.replace('\'', "''")),
        _ if default => "export PYENV_ROOT=\"$HOME/.pyenv\"".to_string(),
        _ => format!(
            "export PYENV_ROOT=\"{}\"",
            r.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`")
        ),
    }
}

pub(crate) fn posix_shell_setup(ctx: &Ctx, shell: &str) -> Vec<String> {
    vec![
        root_line(ctx, "posix"),
        "[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"".to_string(),
        format!("eval \"$(pyenv init - {shell})\""),
    ]
}

pub(crate) const FISH_SHELL_SETUP: &str = "pyenv init - fish | source";

pub(crate) fn fish_user_path_setup(ctx: &Ctx) -> Vec<String> {
    vec![
        root_line(ctx, "fish"),
        "if functions -q fish_add_path".to_string(),
        "  test -d $PYENV_ROOT/bin; and fish_add_path $PYENV_ROOT/bin".to_string(),
        "else".to_string(),
        "  test -d $PYENV_ROOT/bin; and set -U fish_user_paths $PYENV_ROOT/bin $fish_user_paths"
            .to_string(),
        "end".to_string(),
    ]
}

pub(crate) fn pwsh_shell_setup(ctx: &Ctx) -> Vec<String> {
    vec![
        root_line(ctx, "pwsh"),
        "if (Test-Path -LP \"$Env:PYENV_ROOT/bin\" -PathType Container) {".to_string(),
        "  $Env:PATH=\"$Env:PYENV_ROOT/bin:$Env:PATH\" }".to_string(),
        "iex ((pyenv init -) -join \"`n\")".to_string(),
    ]
}

/// Help mode: everything on stderr, exit 1 (libexec/pyenv-init:150-190).
fn help(ctx: &Ctx, shell: &str) -> Output {
    let p = detect_profile(shell);
    let mut lines: Vec<String> = Vec::new();
    let mut add = |l: &str| lines.push(l.to_string());
    match shell {
        "fish" => {
            add("# Add pyenv executable to PATH by running");
            add("# the following interactively:");
            add("");
            for l in fish_user_path_setup(ctx) {
                add(&l);
            }
            add("");
            add("# Load pyenv automatically by appending");
            add("# the following to ~/.config/fish/config.fish:");
            add("");
            add(FISH_SHELL_SETUP);
            add("");
        }
        "pwsh" => {
            add("# Load pyenv automatically by appending");
            add(&format!("# the following to {} :", p.profile));
            add("");
            for l in pwsh_shell_setup(ctx) {
                add(&l);
            }
        }
        _ => {
            add("# Load pyenv automatically by appending");
            if p.profile == p.rc && p.rc_explain.is_none() {
                add(&format!(
                    "# the following to {} :",
                    p.profile_explain.unwrap_or(p.profile)
                ));
            } else {
                // `echo -n "# the following to "` then a bare `echo`: a trailing space.
                add("# the following to ");
                add(&format!(
                    "# {} (for login shells)",
                    p.profile_explain.unwrap_or(p.profile)
                ));
                add(&format!(
                    "# and {} (for interactive shells) :",
                    p.rc_explain.unwrap_or(p.rc)
                ));
            }
            add("");
            for l in posix_shell_setup(ctx, shell) {
                add(&l);
            }
        }
    }
    add("");
    add("# Restart your shell for the changes to take effect.");
    add("");
    let mut o = Output::new();
    for l in &lines {
        o.err(l);
    }
    o.with_code(1)
}

/// `mkdir -p "${PYENV_ROOT}/"{shims,versions}`: one message per directory that can't be
/// made, naming the first missing ancestor, as `mkdir -p` does; exit 1 with no stdout.
pub(crate) fn init_dirs(ctx: &Ctx) -> Result<(), Output> {
    let mut o = Output::new();
    for d in [ctx.shims_dir(), ctx.versions_dir()] {
        if let Err(e) = std::fs::create_dir_all(&d) {
            o.err(format!(
                "mkdir: cannot create directory \u{2018}{}\u{2019}: {}",
                first_missing(&d).display(),
                rpyenv_core::launch::io_reason(&e)
            ));
        }
    }
    if o.stderr.is_empty() {
        Ok(())
    } else {
        Err(o.with_code(1))
    }
}

/// The first ancestor of `d` (or `d`) that doesn't exist: the one `mkdir -p` fails on.
fn first_missing(d: &Path) -> PathBuf {
    let mut p = d;
    while let Some(parent) = p.parent() {
        if parent.exists() {
            return p.to_path_buf();
        }
        p = parent;
    }
    d.to_path_buf()
}

/// The POSIX PATH code: remove every exact shims entry with a nested `bash --norc`, then
/// prepend one; or, with `--no-push-path`, prepend only when absent
/// (libexec/pyenv-init:412-475). `shims` is pasted in without escaping, as upstream does.
pub(crate) fn posix_path_lines(shims: &str, no_push_path: bool) -> Vec<String> {
    let prepend = format!("export PATH=\"{shims}:${{PATH}}\"");
    if no_push_path {
        return vec![
            format!("if [[ \":$PATH:\" != *':{shims}:'* ]]; then"),
            prepend,
            "fi".to_string(),
        ];
    }
    vec![
        "PATH=\"$(bash --norc -ec 'IFS=:; paths=($PATH); ".to_string(),
        "for i in ${!paths[@]}; do ".to_string(),
        format!("if [[ ${{paths[i]}} == \"''{shims}''\" ]]; then unset '\\''paths[i]'\\''; "),
        "fi; done; ".to_string(),
        "echo \"${paths[*]}\"')\"".to_string(),
        prepend,
    ]
}

pub(crate) fn fish_path_lines(shims: &str, no_push_path: bool) -> Vec<String> {
    let prepend = format!("set -gx PATH '{shims}' $PATH");
    if no_push_path {
        return vec![
            format!("if not contains -- \"{shims}\" $PATH"),
            prepend,
            "end".to_string(),
        ];
    }
    vec![
        format!("while set pyenv_index (contains -i -- \"{shims}\" $PATH)"),
        "set -eg PATH[$pyenv_index]; end; set -e pyenv_index".to_string(),
        prepend,
    ]
}

fn print_path(o: &mut Output, ctx: &Ctx, shell: &str, no_push_path: bool) {
    let shims = format!("{}/shims", root(ctx));
    let lines = match shell {
        "fish" => fish_path_lines(&shims, no_push_path),
        "pwsh" if no_push_path => vec![
            format!("if ( $Env:PATH -notmatch \"{shims}\" ) {{"),
            format!("$Env:PATH=\"{shims}:$Env:PATH\""),
            "}".to_string(),
        ],
        "pwsh" => vec![
            format!("$Env:PATH=\"$(($Env:PATH -split ':' | where {{ -not ($_ -match '{shims}') }}) -join ':')\""),
            format!("$Env:PATH=\"{shims}:$Env:PATH\""),
        ],
        _ => posix_path_lines(&shims, no_push_path),
    };
    for l in lines {
        o.out(l);
    }
}

fn print_env(o: &mut Output, shell: &str) {
    o.out(match shell {
        "fish" => format!("set -gx PYENV_SHELL {shell}"),
        "pwsh" => format!("$Env:PYENV_SHELL=\"{shell}\""),
        _ => format!("export PYENV_SHELL={shell}"),
    });
}

/// `<prefix>/completions/pyenv.<shell>`, when readable (libexec/pyenv-init:491-503).
fn print_completion(o: &mut Output, shell: &str) {
    let Some(prefix) = crate::install_prefix() else {
        return;
    };
    let path = prefix.join("completions").join(format!("pyenv.{shell}"));
    if std::fs::File::open(&path).is_ok() {
        let p = path.display();
        o.out(if shell == "pwsh" {
            format!("iex (gc {p} -Raw)")
        } else {
            format!("source '{p}'")
        });
    }
}

fn print_rehash(o: &mut Output, shell: &str, no_rehash: bool) {
    if !no_rehash {
        o.out(if shell == "pwsh" {
            "& pyenv rehash"
        } else {
            "command pyenv rehash"
        });
    }
}

const FISH_FUNCTION: &str = r#"function pyenv
  set command $argv[1]
  set -e argv[1]

  switch "$command"
  case @ROUTED@
    source (pyenv "sh-$command" $argv|psub)
  case "*"
    command pyenv "$command" $argv
  end
end
"#;

const PWSH_FUNCTION: &str = r#"function pyenv {
  $command=""
  if ( $args.Count -gt 0 ) {
    $command, $args = $args
  }

  if ( ("@ROUTED@" -split ' ') -contains $command ) {
    $shell_cmds = (& (get-command -commandtype application pyenv -totalcount 1) sh-$command $args)
    if ( $shell_cmds.Count -gt 0 ) {
      iex ($shell_cmds -join "`n")
    }
  } else {
    & (get-command -commandtype application pyenv -totalcount 1) $command $args
  }
}
"#;

const POSIX_BODY: &str = r#"  [ "$#" -gt 0 ] && shift
  case "$command" in
  @PATTERN@)
    eval "$(pyenv "sh-$command" "$@")"
    ;;
  *)
    command pyenv "$command" "$@"
    ;;
  esac
}
"#;

pub(crate) fn fish_function(routed: &[&str]) -> String {
    FISH_FUNCTION.replace("@ROUTED@", &routed.join(" "))
}

/// `header` is the function's first two lines, ending in a newline. An empty routed list
/// becomes the pattern `/`, which matches no command (libexec/pyenv-init:571).
pub(crate) fn posix_function(header: &str, routed: &[&str]) -> String {
    let pattern = if routed.is_empty() {
        "/".to_string()
    } else {
        routed.join("|")
    };
    format!("{header}{}", POSIX_BODY.replace("@PATTERN@", &pattern))
}

/// The `pyenv` function; it routes the `sh-` commands (Decision 4, allowlist D-88).
fn shell_function(shell: &str) -> String {
    let routed = commands::names(Flavor::Pyenv, Listing::ShOnly);
    match shell {
        "fish" => fish_function(&routed),
        "pwsh" => PWSH_FUNCTION.replace("@ROUTED@", &routed.join(" ")),
        "ksh" | "ksh93" | "mksh" => {
            posix_function("function pyenv {\n  typeset command=${1:-}\n", &routed)
        }
        _ => posix_function("pyenv() {\n  local command=${1:-}\n", &routed),
    }
}
```

- [ ] **Step 5: Register `init` and its help topic.**
  - In `commands/mod.rs`, add `pub mod init;` and the `LINUX_ONLY` row `("init", init::init)`.
  - In `help.rs`, add this topic to `PYENV`, in alphabetical order:

```rust
    topic("init", Some("Configure the shell environment for pyenv"), Some(INIT_USAGE),
        "Usage: eval \"$(pyenv init [-|--path] [--no-push-path] [--no-rehash] [<shell>])\"\n       pyenv init --install [<shell>]\n       pyenv init --detect-shell [<shell>]\n\nConfigure the shell environment for pyenv\n\n"),
```

with

```rust
const INIT_USAGE: &str = "Usage: eval \"$(pyenv init [-|--path] [--no-push-path] [--no-rehash] [<shell>])\"\n       pyenv init --install [<shell>]\n       pyenv init --detect-shell [<shell>]";
```

- [ ] **Step 6: Run the tests and see them pass.**

Run in WSL: `cargo test -p pyenv --test cli_init`
Expected: `10 passed`. If `print_mode_for_bash` fails, diff the text line by line against M3L "Print mode"; lines 1, 2, 3 and 4 of the PATH code end with a space.

Then run `cargo test --workspace` on both OSes. On Windows, `init.rs` compiles but no Windows test reaches it.

- [ ] **Step 7: Commit.**

```bash
git add crates/pyenv
git commit -m "Add pyenv init for Linux: help, --detect-shell, print and path modes"
```

### Task 4: Linux `pyenv init --install`

Read M3L "`--install`" first. The code follows `libexec/pyenv-init:291-406`.

**Files:**
- Modify: `crates/pyenv/src/commands/init.rs`
- Test: `crates/pyenv/tests/cli_init.rs` (append)

**Interfaces:**
- Consumes: `detect_profile`, `posix_shell_setup`, `pwsh_shell_setup`, `fish_user_path_setup` and `FISH_SHELL_SETUP` (Task 3); `rpyenv_core::pathsearch::find_first` (existing).
- Produces: `Mode::Install`, which Task 7 maps to Decision 2's refusal on Windows.

- [ ] **Step 1: Write the failing tests.** Append to `crates/pyenv/tests/cli_init.rs`:

```rust
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

const BASH_SETUP: &str = "export PYENV_ROOT=\"$HOME/.pyenv\"\n[[ -d $PYENV_ROOT/bin ]] && export PATH=\"$PYENV_ROOT/bin:$PATH\"\neval \"$(pyenv init - bash)\"\n";

/// The default root, so the setup text is the portable `$HOME` form.
fn default_root(f: &Fixture) -> String {
    f.base.join(".pyenv").display().to_string()
}

#[test]
fn install_writes_rc_then_profile() {
    let f = Fixture::new();
    let r = default_root(&f);
    let env = [("PYENV_ROOT", r.as_str())];
    assert_eq!(init(&f, &["--install", "bash"], &env), (String::new(), String::new(), 0));
    assert_eq!(read(&f.base.join(".bashrc")), BASH_SETUP);
    assert_eq!(read(&f.base.join(".profile")), BASH_SETUP);
    // A second run refuses: idempotence by refusal.
    let again = init(&f, &["--install", "bash"], &env);
    assert_eq!(again.2, 1);
    assert_eq!(read(&f.base.join(".bashrc")), BASH_SETUP);
}

#[test]
fn install_uses_an_existing_bash_profile_and_other_shells_files() {
    let f = Fixture::new();
    let r = default_root(&f);
    let env = [("PYENV_ROOT", r.as_str())];
    f.file(&f.base.join(".bash_profile"), "alias ll=ls");
    assert_eq!(init(&f, &["--install", "bash"], &env).2, 0);
    // No trailing newline: one is added before the setup.
    assert_eq!(read(&f.base.join(".bash_profile")), format!("alias ll=ls\n{BASH_SETUP}"));
    assert!(!f.base.join(".profile").exists());

    let g = Fixture::new();
    let r = default_root(&g);
    let env = [("PYENV_ROOT", r.as_str())];
    assert_eq!(init(&g, &["--install", "zsh"], &env).2, 0);
    let zsh = BASH_SETUP.replace("init - bash", "init - zsh");
    assert_eq!(read(&g.base.join(".zshrc")), zsh);
    assert_eq!(read(&g.base.join(".zprofile")), zsh);
    assert_eq!(init(&g, &["--install", "mksh"], &env).2, 0);
    assert_eq!(read(&g.base.join(".profile")), BASH_SETUP.replace("init - bash", "init - mksh"));
    assert_eq!(init(&g, &["--install", "pwsh"], &env).2, 0);
    assert_eq!(
        read(&g.base.join(".config/powershell/profile.ps1")),
        "$Env:PYENV_ROOT=\"$Env:HOME/.pyenv\"\nif (Test-Path -LP \"$Env:PYENV_ROOT/bin\" -PathType Container) {\n  $Env:PATH=\"$Env:PYENV_ROOT/bin:$Env:PATH\" }\niex ((pyenv init -) -join \"`n\")\n"
    );
}

/// Review focus 5: any mention of "pyenv", in any case, refuses, and the check of every
/// file comes before any write.
#[test]
fn install_refuses_any_mention_and_writes_nothing() {
    let f = Fixture::new();
    f.file(&f.base.join(".profile"), "# managed by PYENV-tools\n");
    let (out, err, code) = init(&f, &["--install", "bash"], &[]);
    let p = f.base.join(".profile").display().to_string();
    assert_eq!(
        (out.as_str(), code),
        ("", 1)
    );
    assert_eq!(
        err,
        format!("pyenv: cannot automatically apply changes to {p}: it appears to already contain Pyenv-related code.\npyenv: review the file's contents and apply changes manually if necessary.\npyenv: run `pyenv init bash` to see the suggested setup.\n")
    );
    assert!(!f.base.join(".bashrc").exists());
}

#[test]
fn install_refusals() {
    let f = Fixture::new();
    std::fs::create_dir(f.base.join(".bashrc")).unwrap();
    let rc = f.base.join(".bashrc").display().to_string();
    assert_eq!(
        init(&f, &["--install", "bash"], &[]),
        (String::new(), format!("pyenv: failed to inspect {rc}\n"), 1)
    );
    assert!(!f.base.join(".profile").exists());
    for shell in ["sh", "nu"] {
        assert_eq!(
            init(&f, &["--install", shell], &[]),
            (
                String::new(),
                format!("pyenv: cannot automatically configure startup files for {shell}\n"),
                1
            )
        );
    }
    assert_eq!(
        init(&f, &["--install", "bash"], &[("HOME", "")]),
        (
            String::new(),
            "pyenv: HOME must be set to configure shell startup files\n".to_string(),
            1
        )
    );
    // No fish on PATH: refused before anything is written.
    assert_eq!(
        init(&f, &["--install", "fish"], &[]).1,
        "pyenv: fish is not available to configure fish universal variables\n"
    );
    assert!(!f.base.join(".config/fish").exists());
}

/// fish: the universal-variable block goes to `fish -c`, and only the source line to
/// config.fish (test/init.bats:134-148).
#[test]
fn install_for_fish_runs_fish_and_appends_one_line() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let log = f.base.join("fish.log");
    let stub = f.syspath.join("fish");
    std::fs::write(
        &stub,
        format!("#!/bin/sh\nprintf '%s\\n---\\n' \"$@\" > '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    f.file(&f.base.join(".config/fish/config.fish"), "end");
    let r = default_root(&f);
    assert_eq!(init(&f, &["--install", "fish"], &[("PYENV_ROOT", r.as_str())]).2, 0);
    assert_eq!(
        read(&log),
        "-c\n---\nset -Ux PYENV_ROOT $HOME/.pyenv\nif functions -q fish_add_path\n  test -d $PYENV_ROOT/bin; and fish_add_path $PYENV_ROOT/bin\nelse\n  test -d $PYENV_ROOT/bin; and set -U fish_user_paths $PYENV_ROOT/bin $fish_user_paths\nend\n---\n"
    );
    assert_eq!(
        read(&f.base.join(".config/fish/config.fish")),
        "end\npyenv init - fish | source\n"
    );
}

/// Writes go through symlinks; a dangling one has its target created (M3L).
#[test]
fn install_writes_through_symlinks() {
    let f = Fixture::new();
    std::fs::create_dir(f.base.join("dotfiles")).unwrap();
    f.file(&f.base.join("dotfiles/bashrc"), "");
    std::os::unix::fs::symlink(f.base.join("dotfiles/bashrc"), f.base.join(".bashrc")).unwrap();
    std::os::unix::fs::symlink(f.base.join("dotfiles/profile"), f.base.join(".profile")).unwrap();
    let r = default_root(&f);
    assert_eq!(init(&f, &["--install", "bash"], &[("PYENV_ROOT", r.as_str())]).2, 0);
    assert_eq!(read(&f.base.join("dotfiles/bashrc")), BASH_SETUP);
    assert_eq!(read(&f.base.join("dotfiles/profile")), BASH_SETUP);
}
```

- [ ] **Step 2: Run the tests and see them fail.**

Run in WSL: `cargo test -p pyenv --test cli_init install`
Expected: all six fail. `--install` is still read as a shell name, so help mode runs and exits 1 with the wrong text.

- [ ] **Step 3: Implement.** In `init.rs`, make these changes:
  - add `Install` to `Mode`;
  - add the arm `"--install" => a.mode = Mode::Install,` to `parse`;
  - add the arm `Mode::Install => install(ctx, &shell),` to `init`;
  - append the code below.

```rust
/// `pyenv init --install` (libexec/pyenv-init:291-406): checks every startup file before
/// writing any, then appends the setup text to each.
fn install(ctx: &Ctx, shell: &str) -> Output {
    let home = home();
    if home.is_empty() {
        return Output::error("pyenv: HOME must be set to configure shell startup files");
    }
    let p = detect_profile(shell);
    // `${path/#\~/$HOME}`
    let expand = |s: &str| match s.strip_prefix('~') {
        Some(rest) => format!("{home}{rest}"),
        None => s.to_string(),
    };
    let files: Vec<(String, String)> = match shell {
        "bash" | "zsh" | "ksh" | "ksh93" | "mksh" => {
            let setup = posix_shell_setup(ctx, shell).join("\n");
            let (rc, profile) = (expand(p.rc), expand(p.profile));
            let mut f = vec![(rc.clone(), setup.clone())];
            if profile != rc {
                f.push((profile, setup));
            }
            f
        }
        "fish" => vec![(expand(p.rc), FISH_SHELL_SETUP.to_string())],
        "pwsh" => vec![(expand(p.rc), pwsh_shell_setup(ctx).join("\n"))],
        _ => {
            return Output::error(format!(
                "pyenv: cannot automatically configure startup files for {shell}"
            ))
        }
    };
    for (file, _) in &files {
        if let Err(o) = check_startup_file(file, shell) {
            return o;
        }
    }
    if shell == "fish" {
        if let Err(o) = install_fish_user_paths(ctx) {
            return o;
        }
    }
    for (file, text) in &files {
        // Decision 12: upstream's bash prints its own error here; no test reaches it.
        if let Err(e) = append_lines(file, text) {
            return Output::error(format!(
                "pyenv: {file}: {}",
                rpyenv_core::launch::io_reason(&e)
            ));
        }
    }
    Output::new()
}

/// `check_startup_file`: a missing file is fine (a dangling symlink counts as missing); one
/// that isn't a readable regular file, or that mentions "pyenv" in any case, refuses.
fn check_startup_file(file: &str, shell: &str) -> Result<(), Output> {
    let path = Path::new(file);
    if !path.exists() {
        return Ok(());
    }
    let inspect = || Output::error(format!("pyenv: failed to inspect {file}"));
    if !path.is_file() {
        return Err(inspect());
    }
    let bytes = std::fs::read(path).map_err(|_| inspect())?;
    // `grep -Fi pyenv`
    if bytes.to_ascii_lowercase().windows(5).any(|w| w == b"pyenv") {
        let mut o = Output::new();
        o.err(format!("pyenv: cannot automatically apply changes to {file}: it appears to already contain Pyenv-related code."));
        o.err("pyenv: review the file's contents and apply changes manually if necessary.");
        o.err(format!("pyenv: run `pyenv init {shell}` to see the suggested setup."));
        return Err(o.with_code(1));
    }
    Ok(())
}

/// `fish -c "<the fish PATH block>"`, with `fish` from PATH (libexec/pyenv-init:373-386).
fn install_fish_user_paths(ctx: &Ctx) -> Result<(), Output> {
    let fish = rpyenv_core::pathsearch::find_first(
        "fish",
        ctx.path.as_deref(),
        None,
        ctx.flavor,
        ctx.pathext.as_deref(),
    )
    .ok_or_else(|| {
        Output::error("pyenv: fish is not available to configure fish universal variables")
    })?;
    let ok = std::process::Command::new(fish)
        .arg("-c")
        .arg(fish_user_path_setup(ctx).join("\n"))
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        Err(Output::error(
            "pyenv: failed to configure fish universal variables",
        ))
    }
}

/// `append_lines`: make the parent folder, add a newline when the file doesn't end in one,
/// then append the text and a newline. Opening follows symlinks.
fn append_lines(file: &str, text: &str) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    if let Some(dir) = Path::new(file).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(file)?;
    if f.metadata()?.len() > 0 {
        let mut last = [0u8; 1];
        f.seek(SeekFrom::End(-1))?;
        f.read_exact(&mut last)?;
        if last[0] != b'\n' {
            f.write_all(b"\n")?;
        }
    }
    f.write_all(text.as_bytes())?;
    f.write_all(b"\n")
}
```

If `find_first`'s signature differs from `(name, Option<&OsStr>, Option<&Path>, Flavor, Option<&OsStr>)`, adapt the call. Its use in `rpyenv-core/src/prefix.rs` (`system_python`) shows the current form.

- [ ] **Step 4: Run the tests and see them pass.**

Run in WSL: `cargo test -p pyenv --test cli_init`
Expected: `16 passed`.

- [ ] **Step 5: Commit.**

```bash
git add crates/pyenv
git commit -m "Add pyenv init --install for Linux startup files"
```

---

### Task 5: Completion scripts, `pyenv completions` and `--complete`

Read M3L "`pyenv completions`" and "`--complete` outputs not recorded in M1 or M2" first, plus M1L's `--complete` lines. Decision 5 says why one table replaces a block per command.

**Files:**
- Create:
  - `completions/pyenv.bash`, `completions/pyenv.zsh`, `completions/pyenv.fish`, `completions/pyenv.pwsh` (copies);
  - `completions/LICENSE` (copy of upstream's `LICENSE`);
  - `crates/pyenv/src/commands/completions.rs`.
- Modify:
  - `.gitattributes`: add `completions/* -text`, so Windows checkouts keep LF;
  - `crates/pyenv/src/commands/mod.rs`: `pub mod completions;` and a `COMMANDS` row;
  - `crates/pyenv/src/lib.rs`: `run_pyenv`;
  - `crates/pyenv/src/help.rs`: the topic.
- Test: `crates/pyenv/tests/cli_completions.rs`

**Interfaces:**
- Consumes: `commands::versions::versions`, `commands::rehash::{rehash, shims}`, `commands::misc::commands`, `installed::names`, and `crate::run` (existing); `commands::lookup`.
- Produces:
  - `commands::completions::completions: Command`;
  - `commands::completions::answer(&Ctx, &str) -> Option<Output>`, the Linux dispatcher's `--complete`;
  - `pub(crate) const TABLE_PYENV` and `TABLE_WIN`. Task 6 adds a test that every entry names a command.

- [ ] **Step 1: Vendor the scripts.** Write `C:\tmp\m3_vendor_completions.sh`:

```bash
#!/usr/bin/env bash
# Copy upstream's completion scripts and license, byte for byte, into the rpyenv checkout.
set -eu
UP=/home/jm/m1cb-upstream/pyenv-07171d013cac53d1cc9248b4c160217f288a2965
DST=/mnt/c/Users/JM/repos/OWN/rpyenv/completions
mkdir -p "$DST"
for s in bash zsh fish pwsh; do
  cp "$UP/completions/pyenv.$s" "$DST/pyenv.$s"
  cmp "$UP/completions/pyenv.$s" "$DST/pyenv.$s"
done
cp "$UP/LICENSE" "$DST/LICENSE"
sha256sum "$DST"/*
```

Run it with `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m3_vendor_completions.sh`. Then:
- add the line `completions/* -text` to `.gitattributes`;
- put the four SHA-256 values and the upstream commit `07171d0` in the commit message body (use a `-F` message file).

- [ ] **Step 2: Write the failing tests.** Create `crates/pyenv/tests/cli_completions.rs`:

```rust
//! `pyenv completions` and `--complete` (M3L "pyenv completions"; Decision 5).

mod common;
use common::Fixture;

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

/// One version, 3.12.1, with `pip` and `python`, rehashed so `shims --short` lists both.
fn fixture() -> Fixture {
    let f = Fixture::new();
    for exe in ["pip", "python"] {
        let name = if cfg!(windows) { format!("{exe}.exe") } else { exe.to_string() };
        let rel = if cfg!(windows) {
            format!("3.12.1/{name}")
        } else {
            format!("3.12.1/bin/{name}")
        };
        f.exe(&rel);
    }
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    f
}

/// M3L "pyenv completions <cmd> for every command, at 2.8.8", for the commands rpyenv has.
#[cfg(unix)]
#[test]
fn every_linux_command_completes_as_upstream() {
    let f = fixture();
    for (cmd, want) in [
        ("--version", "--help"),
        ("latest", "--help"),
        ("rehash", "--help"),
        ("root", "--help"),
        ("version", "--help"),
        ("version-name", "--help"),
        ("commands", "--help --sh --no-sh"),
        ("exec", "--help --environment pip python"),
        ("global", "--help system 3.12.1"),
        ("prefix", "--help system 3.12.1"),
        ("local", "--help --unset system 3.12.1"),
        ("shell", "--help --unset system 3.12.1"),
        ("sh-shell", "--help --unset system 3.12.1"),
        ("init", "--help - --path --install --no-push-path --no-rehash --detect-shell bash fish ksh pwsh zsh"),
        ("shims", "--help --short"),
        ("versions", "--help --bare --skip-aliases --skip-envs"),
        ("whence", "--help --path pip python"),
        ("which", "--help pip python"),
        ("uninstall", "--help --force 3.12.1"),
    ] {
        let (out, err, code) = run(&f, &["completions", cmd]);
        assert_eq!(
            (out.split_whitespace().collect::<Vec<_>>().join(" ").as_str(), err.as_str(), code),
            (want, "", 0),
            "{cmd}"
        );
    }
    // `completions` and `help` list every command after their own words.
    let commands = run(&f, &["commands"]).0;
    assert_eq!(run(&f, &["completions", "completions"]).0, format!("--help\n{commands}"));
    assert_eq!(run(&f, &["completions", "help"]).0, format!("--help\n--usage\n{commands}"));
    // `install`: --help, nine options, then every definition.
    let install = run(&f, &["completions", "install"]).0;
    let list = run(&f, &["install", "--list", "--bare"]).0;
    assert!(install.starts_with("--help\n--bare\n--list\n--force\n--skip-existing\n--keep\n--patch\n--verbose\n--version\n--debug\n"));
    assert!(install.ends_with(&list));
}

#[cfg(unix)]
#[test]
fn completions_edge_cases() {
    let f = fixture();
    assert_eq!(
        run(&f, &["completions"]),
        (
            String::new(),
            "Usage: pyenv completions <command> [arg1 arg2...]\n".to_string(),
            1
        )
    );
    // A command that exists nowhere: no output at all, exit 1.
    assert_eq!(run(&f, &["completions", "nosuchcmd"]), (String::new(), String::new(), 1));
    assert_eq!(run(&f, &["completions", "--complete"]), run(&f, &["commands"]));
    // Extra arguments are ignored by the built-in commands.
    assert_eq!(run(&f, &["completions", "shell", "foo", "bar"]), run(&f, &["completions", "shell"]));
    // `<cmd> --complete` directly: version has no marker but answers.
    assert_eq!(run(&f, &["version", "--complete"]).0, "--bare\n");
    assert_eq!(run(&f, &["exec", "--complete"]).0, "--environment\npip\npython\n");
    assert_eq!(run(&f, &["init", "--complete"]).0.lines().count(), 11);
    // `latest` has no `--complete`: the flag is a prefix to match.
    assert_eq!(run(&f, &["latest", "--complete"]).2, 1);
    assert_eq!(run(&f, &["help", "completions"]).0, "Usage: pyenv completions <command> [arg1 arg2...]\n");
}

/// Decision 11: `sh-rehash --complete` rehashes, as upstream's does.
#[cfg(unix)]
#[test]
fn sh_rehash_complete_rehashes() {
    let f = Fixture::new();
    f.exe("3.12.1/bin/python");
    assert_eq!(run(&f, &["completions", "sh-rehash"]), ("--help\n".to_string(), String::new(), 0));
    assert!(f.root.join("shims").join("python").exists());
}

/// The vendored bash script, sourced into a real bash (M3L "The completion scripts").
#[cfg(unix)]
#[test]
fn the_bash_script_completes_through_pyenv() {
    let f = fixture();
    for (name, target) in [("pyenv", env!("CARGO_BIN_EXE_pyenv")), ("bash", "/bin/bash")] {
        std::os::unix::fs::symlink(target, f.syspath.join(name)).unwrap();
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../completions/pyenv.bash");
    let body = format!(
        "source '{}'\nCOMP_WORDS=(pyenv sh); COMP_CWORD=1; _pyenv; echo \"${{COMPREPLY[*]}}\"\nCOMP_WORDS=(pyenv shell ''); COMP_CWORD=2; _pyenv; echo \"${{COMPREPLY[*]}}\"\n",
        script.display()
    );
    let o = f
        .command(std::path::Path::new("/bin/bash"), &f.work, &[])
        .args(["-c", &body])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "shell shims\n--help --unset system 3.12.1\n",
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Windows: `pyenv completions` reads the table, and `<cmd> --complete` keeps pyenv-win's
/// behavior (Decision 5).
#[cfg(windows)]
#[test]
fn windows_completions_read_the_table() {
    let f = fixture();
    assert_eq!(run(&f, &["completions", "versions"]).0, "--help\r\n--bare\r\n--skip-aliases\r\n");
    assert_eq!(run(&f, &["completions", "update"]).0, "--help\r\n--ignore\r\n");
    assert_eq!(run(&f, &["completions", "uninstall"]).0, "--help\r\n--force\r\n--all\r\n3.12.1\r\n");
    assert_eq!(run(&f, &["completions", "latest"]).0, "--help\r\n");
    assert!(!run(&f, &["versions", "--complete"]).0.contains("--bare"));
}
```

- [ ] **Step 3: Run the tests and see them fail.**

Run in WSL: `cargo test -p pyenv --test cli_completions`. On Windows: `cargo test -p pyenv --test cli_completions`.
Expected: every test fails, with `no such command` for `completions`.

- [ ] **Step 4: Implement `completions.rs`.**

```rust
//! `pyenv completions` and the `--complete` answers (libexec/pyenv-completions, and each
//! upstream command's "Provide pyenv completions" block; M3L, M1L). Decision 5: one table
//! per flavor instead of a block per command.

use crate::commands;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use std::ffi::OsString;

/// What follows a command's fixed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tail {
    Nothing,
    /// `pyenv-versions --bare`
    VersionsBare,
    /// `pyenv-shims --short`
    ShimsShort,
    /// `pyenv-commands`
    Commands,
    /// The installed version names.
    InstalledNames,
    /// `pyenv-rehash --complete`, which rehashes (Decision 11).
    Rehash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Words(&'static [&'static str], Tail),
    /// The command handles `--complete` itself (Linux `install` and `uninstall`).
    Itself,
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub name: &'static str,
    /// Upstream's `# Provide pyenv completions` marker: whether `pyenv completions <cmd>`
    /// forwards to `<cmd> --complete`.
    pub marker: bool,
    pub answer: Answer,
}

const fn e(name: &'static str, marker: bool, answer: Answer) -> Entry {
    Entry {
        name,
        marker,
        answer,
    }
}

use Answer::{Itself, Words};
use Tail::{Commands, InstalledNames, Nothing, Rehash, ShimsShort, VersionsBare};

/// Linux, from upstream's scripts at 2.8.8. A command not listed has no `--complete`.
pub(crate) const TABLE_PYENV: &[Entry] = &[
    e("commands", true, Words(&["--sh", "--no-sh"], Nothing)),
    e("completions", true, Words(&[], Commands)),
    e("exec", true, Words(&["--environment"], ShimsShort)),
    e("global", true, Words(&["system"], VersionsBare)),
    e("help", true, Words(&["--usage"], Commands)),
    e(
        "init",
        true,
        Words(
            &["-", "--path", "--install", "--no-push-path", "--no-rehash", "--detect-shell", "bash", "fish", "ksh", "pwsh", "zsh"],
            Nothing,
        ),
    ),
    e("install", true, Itself),
    e("local", true, Words(&["--unset", "system"], VersionsBare)),
    e("prefix", true, Words(&["system"], VersionsBare)),
    e("sh-rehash", true, Words(&[], Rehash)),
    e("sh-shell", true, Words(&["--unset", "system"], VersionsBare)),
    e("shims", true, Words(&["--short"], Nothing)),
    e("uninstall", true, Itself),
    // Handles `--complete` but has no marker, so `pyenv completions version` is `--help`.
    e("version", false, Words(&["--bare"], Nothing)),
    e("versions", true, Words(&["--bare", "--skip-aliases", "--skip-envs"], Nothing)),
    e("whence", true, Words(&["--path"], ShimsShort)),
    e("which", true, Words(&[], ShimsShort)),
];

/// Windows: rpyenv's own (pyenv-win has none). The options are each pyenv-win command's.
pub(crate) const TABLE_WIN: &[Entry] = &[
    e("commands", true, Words(&[], Nothing)),
    e("completions", true, Words(&[], Commands)),
    e("exec", true, Words(&[], ShimsShort)),
    e("global", true, Words(&["--unset"], VersionsBare)),
    e("help", true, Words(&[], Commands)),
    e(
        "init",
        true,
        Words(
            &["-", "--path", "--no-push-path", "--no-rehash", "--detect-shell", "bash", "cmd", "fish", "powershell", "pwsh", "zsh"],
            Nothing,
        ),
    ),
    e(
        "install",
        true,
        Words(
            &["--list", "--force", "--skip-existing", "--register", "--all", "--clear", "--32only", "--64only", "--quiet", "--dev"],
            Nothing,
        ),
    ),
    e("local", true, Words(&["--unset"], VersionsBare)),
    e("prefix", true, Words(&[], VersionsBare)),
    e("sh-rehash", true, Words(&[], Nothing)),
    e("sh-shell", true, Words(&["--unset"], VersionsBare)),
    e("shims", true, Words(&["--short"], Nothing)),
    e("uninstall", true, Words(&["--force", "--all"], InstalledNames)),
    e("update", true, Words(&["--ignore"], Nothing)),
    e("versions", true, Words(&["--bare", "--skip-aliases"], Nothing)),
    e("whence", true, Words(&["--path"], ShimsShort)),
    e("which", true, Words(&[], ShimsShort)),
];

pub(crate) fn table(flavor: Flavor) -> &'static [Entry] {
    match flavor {
        Flavor::Pyenv => TABLE_PYENV,
        Flavor::PyenvWin => TABLE_WIN,
    }
}

fn tail(ctx: &Ctx, t: Tail) -> Output {
    match t {
        Tail::Nothing => Output::new(),
        Tail::VersionsBare => commands::versions::versions(ctx, &["--bare"]),
        Tail::ShimsShort => commands::rehash::shims(ctx, &["--short"]),
        Tail::Commands => commands::misc::commands(ctx, &[]),
        Tail::InstalledNames => {
            let mut o = Output::new();
            for n in rpyenv_core::installed::names(&ctx.versions_dir(), ctx.flavor) {
                o.out(n);
            }
            o
        }
        Tail::Rehash => commands::rehash::rehash(ctx, &[]),
    }
}

/// `<cmd> --complete` from the table: the words, then the tail and its exit code. `None`
/// when the command answers it itself, or has no answer.
pub fn answer(ctx: &Ctx, cmd: &str) -> Option<Output> {
    let entry = table(ctx.flavor).iter().find(|e| e.name == cmd)?;
    let Answer::Words(words, t) = entry.answer else {
        return None;
    };
    let mut o = Output::new();
    for w in words {
        o.out(w);
    }
    let rest = tail(ctx, t);
    o.stdout.push_str(&rest.stdout);
    o.stderr.push_str(&rest.stderr);
    Some(o.with_code(rest.code))
}

/// `pyenv completions <command> [arg1 arg2...]` (libexec/pyenv-completions).
pub fn completions(ctx: &Ctx, args: &[&str]) -> Output {
    let Some(&cmd) = args.first().filter(|c| !c.is_empty()) else {
        return Output::error("Usage: pyenv completions <command> [arg1 arg2...]");
    };
    if cmd == "--complete" {
        return commands::misc::commands(ctx, &[]);
    }
    // `command -v pyenv-<cmd> || command -v pyenv-sh-<cmd>`: the plain command wins, so
    // `rehash` finds `pyenv-rehash`, which has no marker.
    let sh = format!("sh-{cmd}");
    let target = if commands::lookup(ctx.flavor, cmd).is_some() {
        cmd
    } else if commands::lookup(ctx.flavor, &sh).is_some() {
        sh.as_str()
    } else {
        // `set -e` ends the script when neither exists: no output at all.
        return Output::new().with_code(1);
    };
    let mut o = Output::new();
    o.out("--help");
    let Some(entry) = table(ctx.flavor)
        .iter()
        .find(|e| e.name == target && e.marker)
    else {
        return o;
    };
    let rest = match entry.answer {
        Answer::Words(..) => answer(ctx, target).unwrap_or_default(),
        Answer::Itself => {
            let mut a: Vec<OsString> = vec![target.into(), "--complete".into()];
            a.extend(args[1..].iter().map(OsString::from));
            crate::run(&a, ctx)
        }
    };
    o.stdout.push_str(&rest.stdout);
    o.stderr.push_str(&rest.stderr);
    o.with_code(rest.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Linux entry names a Linux command (Task 6 extends this to Windows).
    #[test]
    fn every_linux_entry_names_a_command() {
        for e in TABLE_PYENV {
            assert!(
                commands::lookup(Flavor::Pyenv, e.name).is_some(),
                "{}",
                e.name
            );
        }
    }
}
```

- [ ] **Step 5: Wire it in.**
  - `commands/mod.rs`: add `pub mod completions;` and the `COMMANDS` row `("completions", completions::completions),`, which both flavors get.
  - `lib.rs` `run_pyenv`: right after `let Some((&cmd, rest)) = args.split_first() else { … };`, add:

```rust
    // Each upstream command answers `--complete` as its first argument (Decision 5).
    if rest.first() == Some(&"--complete") {
        if let Some(o) = commands::completions::answer(ctx, cmd) {
            return o;
        }
    }
```

`run_pyenv_win` gets no such block.

  - `help.rs`: add to `PYENV`, in alphabetical order:

```rust
    topic("completions", None, Some("Usage: pyenv completions <command> [arg1 arg2...]"),
        "Usage: pyenv completions <command> [arg1 arg2...]\n"),
```

`install.rs` and `uninstall.rs` keep their own `--complete` blocks; the `Itself` entries reach them. Check that `version.rs` has no `--complete` block of its own that would now be unreachable: `grep -n complete crates/pyenv/src/commands/version.rs`. Remove one if found, since the table answers.

- [ ] **Step 6: Run the tests and see them pass.**

Run in WSL: `cargo test -p pyenv --test cli_completions`. Expected: `4 passed`.
Run on Windows: the same command. Expected: `1 passed`.

Then run `cargo test --workspace` on both OSes. `pyenv commands` now lists `completions`. Fix any golden-based unit test that pins the listing by adding the name; Task 8 regenerates the parity goldens.

- [ ] **Step 7: Commit.**

```bash
git add .gitattributes completions crates/pyenv
git commit -F C:/tmp/cm_m3t5.txt
```

with `C:\tmp\cm_m3t5.txt` holding:

```
Add pyenv completions, --complete for every command, and upstream's completion scripts

completions/pyenv.{bash,zsh,fish,pwsh} and LICENSE are pyenv 07171d0's, byte for byte
(cmp). SHA-256: <the four values from Step 1>.
```

### Task 6: Windows `shell`, `sh-shell` and `sh-rehash`

Read M3W "`pyenv shell` in cmd" and "`pyenv shell` in PowerShell", and Decisions 1, 6 and 10.

**Line endings.** `Output::emit` turns `\n` into `\r\n` on Windows and writes through the console code page. That is right for cmd and PowerShell, which strip the CR when they split native output into lines. It is wrong for code that bash, zsh or fish evaluate: `eval "$(…)"` keeps the CR, so `export PYENV_VERSION='3.7.7'\r` stores `3.7.7\r`.

So POSIX and fish code goes out through `Output::raw_stdout` as `\n`-terminated UTF-8 (MSYS shells read UTF-8). The helper `lf` below does that, and Task 7 reuses it.

**Files:**
- Create:
  - `crates/pyenv/src/commands/shell_win.rs`;
  - `crates/pyenv/tests/common/winshell.rs`;
  - `crates/pyenv/tests/cli_shell_win.rs`.
- Modify:
  - `crates/pyenv/src/commands/mod.rs`: `pub mod shell_win;` and three `WIN_ONLY` rows;
  - `crates/pyenv/src/commands/completions.rs`: the `TABLE_WIN` row `shell`, and the test;
  - `crates/pyenv/src/help.rs`: the `PYENV_WIN` topics `shell` and `sh-shell`;
  - `crates/pyenv/src/lib.rs`: `run_pyenv_win`, the `sh-* --help` rule;
  - `crates/pyenv/tests/common/mod.rs`: add `#[cfg(windows)] pub mod winshell;`;
  - `docs/specs/2026-09-27-rpyenv-design.md`: §7, Decision 1.

**Interfaces:**
- Consumes:
  - `shellname::{windows_shell, windows_name, family, Family}` and `winproc::parent_image_name` (Task 1);
  - `install::is_safe_win_segment` (existing);
  - `commands::names(PyenvWin, ShOnly)`, which now gives `rehash` and `shell` on Windows too.
- Produces:
  - `commands::shell_win::{shell, sh_shell, sh_rehash, HELP}`;
  - `pub(crate) fn lf(Output) -> Output`;
  - the test helpers `winshell::{install_pyenv, host, output, cmd_exe, powershell, pwsh, git_bash, system32}`.

- [ ] **Step 1: Write the test helpers.** Create `crates/pyenv/tests/common/winshell.rs`:

```rust
//! Real Windows shells for the shell-integration tests: cmd, Windows PowerShell 5.1,
//! PowerShell 7 and Git Bash. Every shell gets the fixture's root and HOME, and PowerShell
//! runs with `-NoProfile`, so nothing reads or writes a real profile.
use super::Fixture;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A copy of rpyenv's `pyenv.exe` in the fixture's `syspath`, so a shell finds it by name.
pub fn install_pyenv(f: &Fixture) -> PathBuf {
    let dst = f.syspath.join("pyenv.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_pyenv"), &dst).unwrap();
    dst
}

pub fn system32() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join("System32")
}

pub fn cmd_exe() -> PathBuf {
    system32().join("cmd.exe")
}

pub fn powershell() -> PathBuf {
    system32().join(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// `pwsh.exe` from this process's PATH, when PowerShell 7 is installed.
pub fn pwsh() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("pwsh.exe"))
        .find(|p| p.is_file())
}

/// Git for Windows' bash, when installed.
pub fn git_bash() -> Option<PathBuf> {
    let p = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    p.is_file().then_some(p)
}

/// `program` with the fixture's environment; `PATH` is `syspath`, System32, then `extra`.
pub fn host(f: &Fixture, program: &Path, env: &[(&str, &str)], extra: &[&Path]) -> Command {
    let mut dirs = vec![f.syspath.clone(), system32()];
    dirs.extend(extra.iter().map(|p| p.to_path_buf()));
    let mut cmd = f.command(program, &f.work, env);
    cmd.env("PATH", std::env::join_paths(dirs).unwrap());
    for k in ["TEMP", "TMP", "LOCALAPPDATA", "APPDATA", "windir", "ComSpec"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    cmd
}

/// stdout, stderr and the exit code, lossily decoded: compare only ASCII text.
pub fn output(mut cmd: Command) -> (String, String, i32) {
    let o = cmd.output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}
```

- [ ] **Step 2: Write the failing tests.** Create `crates/pyenv/tests/cli_shell_win.rs`:

```rust
//! Windows `pyenv shell`, `sh-shell` and `sh-rehash` (M3W; Decisions 1, 6, 10).
#![cfg(windows)]

mod common;
use common::winshell::{self, host, output};
use common::Fixture;

fn run(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let r = f.pyenv_env(args, env);
    (r.stdout, r.stderr, r.code)
}

fn not_installed(name: &str) -> String {
    format!("pyenv specific python requisite didn't meet. Project is using different version of python.\r\nInstall python '{name}' by typing: 'pyenv install {name}'\r\n")
}

const HELP: &str = "Usage: pyenv shell <version>\r\n       pyenv shell --unset\r\n\r\nSets a shell-specific Python version by setting the `PYENV_VERSION'\r\nenvironment variable in your shell. This version overrides local\r\napplication-specific versions and the global version.\r\n\r\n";

#[test]
fn pyenv_win_output_forms() {
    let f = Fixture::new();
    f.version("3.7.7").version("3.8.9");
    assert_eq!(
        run(&f, &["shell"], &[]),
        ("no shell-specific version configured\r\n".to_string(), String::new(), 0)
    );
    assert_eq!(run(&f, &["shell"], &[("PYENV_VERSION", "3.7.7 3.8.9")]).0, "3.7.7 3.8.9\r\n");
    assert_eq!(run(&f, &["shell", "9.9.9"], &[]), (not_installed("9.9.9"), String::new(), 1));
    assert_eq!(run(&f, &["shell", "3.7.7", "9.9.9"], &[]).0, not_installed("9.9.9"));
    assert_eq!(run(&f, &["shell", "3.7.7", "--unset"], &[]).0, not_installed("--unset"));
    // Decision 6: no prefix resolution and no `system`.
    assert_eq!(run(&f, &["shell", "3.7"], &[]).0, not_installed("3.7"));
    assert_eq!(run(&f, &["shell", "system"], &[]).0, not_installed("system"));
    assert_eq!(run(&f, &["shell", ""], &[]).0, not_installed(""));
    for args in [&["shell", "--help"][..], &["--help", "shell"], &["help", "shell"]] {
        assert_eq!(run(&f, args, &[]).0, HELP, "{args:?}");
    }
}

/// allowlist D-89
/// Review focus 3: run by a program that isn't a shell (this test), `shell` prints every
/// shell's line, labeled, and exits 1.
#[test]
fn unknown_parent_prints_every_shell() {
    let f = Fixture::new();
    f.version("3.7.7");
    assert_eq!(
        run(&f, &["shell", "3.7.7"], &[]),
        (
            "cmd: set \"PYENV_VERSION=3.7.7\"\r\nPowerShell: $Env:PYENV_VERSION = '3.7.7'\r\nbash: export PYENV_VERSION='3.7.7'\r\nfish: set -gx PYENV_VERSION '3.7.7'\r\n".to_string(),
            "pyenv: shell integration is not enabled, so nothing was changed: run the line for your shell above.\r\n".to_string(),
            1
        )
    );
    // With only PYENV_SHELL to go by, that shell's line. `--unset` matches in any case.
    assert_eq!(
        run(&f, &["shell", "--UNSET", "x"], &[("PYENV_SHELL", "pwsh")]),
        (
            "Remove-Item Env:PYENV_VERSION -ErrorAction SilentlyContinue\r\n".to_string(),
            "pyenv: shell integration is not enabled in this shell, so nothing was changed: run the command above.\r\npyenv: to enable it, see `pyenv init pwsh`.\r\n".to_string(),
            1
        )
    );
}

/// Review focus 4, Decision 1: cmd's own line wins over an inherited PYENV_SHELL.
#[test]
fn cmd_parent_wins_over_inherited_pyenv_shell() {
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let mut c = host(&f, &winshell::cmd_exe(), &[("PYENV_SHELL", "pwsh")], &[]);
    c.args(["/d", "/c", "call", "pyenv", "shell", "3.7.7"]);
    assert_eq!(
        output(c),
        (
            "set \"PYENV_VERSION=3.7.7\"\r\n".to_string(),
            "pyenv: cmd has no shell integration, so nothing was changed: run the command above.\r\n".to_string(),
            1
        )
    );
}

#[test]
fn x86_appends_win32_unless_present() {
    let f = Fixture::new();
    f.version("3.7.7-win32");
    let x86 = [("PYENV_FORCE_ARCH", "X86"), ("PYENV_SHELL", "cmd")];
    assert_eq!(run(&f, &["shell", "3.7.7"], &x86).0, "set \"PYENV_VERSION=3.7.7-win32\"\r\n");
    assert_eq!(run(&f, &["shell", "3.7.7-WIN32"], &x86).0, "set \"PYENV_VERSION=3.7.7-WIN32\"\r\n");
}

/// `sh-shell`'s code, evaluated by Windows PowerShell 5.1 and, when installed, pwsh 7.
#[test]
fn powershell_evaluates_sh_shell() {
    let f = Fixture::new();
    f.version("3.7.7").version("3.8.9");
    winshell::install_pyenv(&f);
    let script = "iex ((pyenv sh-shell 3.7.7 3.8.9) -join \"`n\"); $env:PYENV_VERSION; iex ((pyenv sh-shell --unset) -join \"`n\"); [string]::IsNullOrEmpty($env:PYENV_VERSION)";
    let mut shells = vec![winshell::powershell()];
    shells.extend(winshell::pwsh());
    for sh in shells {
        let mut c = host(&f, &sh, &[("PYENV_SHELL", "pwsh")], &[]);
        c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        let (out, err, code) = output(c);
        assert_eq!((out.as_str(), code), ("3.7.7 3.8.9\r\nTrue\r\n", 0), "{}: {err}", sh.display());
    }
}

/// Review focus 1: a value with a quote character survives each shell's code. POSIX and
/// fish code is `\n`-terminated even on Windows.
#[test]
fn quotes_survive_each_shell() {
    let f = Fixture::new();
    f.version("a'b");
    winshell::install_pyenv(&f);
    assert_eq!(
        run(&f, &["sh-shell", "a'b"], &[("PYENV_SHELL", "fish")]).0,
        "set -gx PYENV_VERSION 'a\\'b'\n"
    );
    assert_eq!(
        run(&f, &["sh-shell", "a'b"], &[("PYENV_SHELL", "bash")]).0,
        "export PYENV_VERSION='a'\\''b'\n"
    );
    let mut c = host(&f, &winshell::powershell(), &[("PYENV_SHELL", "pwsh")], &[]);
    c.args(["-NoProfile", "-NonInteractive", "-Command", "iex ((pyenv sh-shell \"a'b\") -join \"`n\"); $env:PYENV_VERSION"]);
    assert_eq!(output(c).0, "a'b\r\n");
    if let Some(bash) = winshell::git_bash() {
        let mut c = host(&f, &bash, &[("PYENV_SHELL", "bash")], &[]);
        c.args(["-c", "eval \"$(pyenv sh-shell \"a'b\")\"; echo \"$PYENV_VERSION\""]);
        assert_eq!(output(c).0, "a'b\n");
    }
}

#[test]
fn sh_shell_for_the_posix_function_and_failures() {
    let f = Fixture::new();
    f.version("3.7.7");
    let bash = [("PYENV_SHELL", "bash"), ("PYENV_VERSION", "3.7.7")];
    assert_eq!(run(&f, &["sh-shell"], &bash).0, "echo \"$PYENV_VERSION\"\n");
    // Evaluated by bash or fish: the message on stderr and `false` to run.
    let (out, err, code) = run(&f, &["sh-shell", "9.9.9"], &[("PYENV_SHELL", "bash")]);
    assert_eq!((out.as_str(), err, code), ("false\n", not_installed("9.9.9"), 1));
    // Evaluated by the PowerShell function: the message on stdout, which it prints.
    assert_eq!(run(&f, &["sh-shell", "9.9.9"], &[("PYENV_SHELL", "pwsh")]).0, not_installed("9.9.9"));
}

#[test]
fn sh_rehash_listing_help_and_completions() {
    let f = Fixture::new();
    let rehash = |shell| run(&f, &["sh-rehash"], &[("PYENV_SHELL", shell)]).0;
    assert_eq!(rehash("pwsh"), "& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash\r\n");
    assert_eq!(rehash("bash"), "command pyenv rehash\nhash -r 2>/dev/null || true\n");
    assert_eq!(rehash("fish"), "command pyenv rehash\n");
    assert_eq!(rehash("cmd"), "pyenv rehash\r\n");
    let commands = run(&f, &["commands"], &[]).0;
    assert_eq!(commands.lines().filter(|l| *l == "shell").count(), 1);
    assert!(!commands.contains("sh-"));
    assert_eq!(run(&f, &["sh-shell", "--help"], &[]).0, "pyenv help \"sh-shell\"\r\n");
    assert_eq!(run(&f, &["help", "sh-shell"], &[]).0, HELP);
    assert_eq!(run(&f, &["completions", "shell"], &[]).0, "--help\r\n--unset\r\n");
}
```

- [ ] **Step 3: Run the tests and see them fail.**

Run on Windows: `cargo test -p pyenv --test cli_shell_win`
Expected: failures with `pyenv: no such command 'shell'` and `'sh-shell'`.

- [ ] **Step 4: Implement `shell_win.rs`.**

```rust
//! `pyenv shell`, `sh-shell` and `sh-rehash` on Windows (spec §7; pyenv-win's semantics,
//! M3W; Decisions 1, 6, 10). `shell` is what runs without integration: it validates, then
//! prints the command for the detected shell and exits 1 (allowlist D-89, D-90).
//! `sh-shell` and `sh-rehash` print code for the `pyenv` function of `pyenv init -`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::shellname::{self, Family};

/// pyenv-win's `pyenv shell` help (libexec\pyenv-shell.bat:10-17).
pub const HELP: &str = "Usage: pyenv shell <version>\n       pyenv shell --unset\n\nSets a shell-specific Python version by setting the `PYENV_VERSION'\nenvironment variable in your shell. This version overrides local\napplication-specific versions and the global version.\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Set(String),
    Unset,
}

/// Code for bash, zsh or fish: `\n` line ends and UTF-8, written as they are. `emit`
/// would add `\r`, which `eval "$(…)"` keeps.
pub(crate) fn lf(mut o: Output) -> Output {
    o.raw_stdout = Some(std::mem::take(&mut o.stdout).into_bytes());
    o
}

/// pyenv-win's not-installed message, on stdout (libexec\libs\pyenv-lib.vbs:265-268).
fn not_installed(name: &str) -> Output {
    let mut o = Output::new();
    o.out("pyenv specific python requisite didn't meet. Project is using different version of python.");
    o.out(format!("Install python '{name}' by typing: 'pyenv install {name}'"));
    o.with_code(1)
}

/// `-win32` on X86 unless the name already ends in it, in any case (pyenv-lib.vbs:450-455).
fn with_arch(name: &str, ctx: &Ctx) -> String {
    if ctx.arch_suffix == "-win32" && !name.to_ascii_lowercase().ends_with("-win32") {
        format!("{name}-win32")
    } else {
        name.to_string()
    }
}

/// Decision 6: each name must be a folder of exactly that name, and a safe path segment;
/// the value joins them with a space. `Err` is the first name that fails.
fn validate(ctx: &Ctx, args: &[&str]) -> Result<String, String> {
    let mut names = Vec::new();
    for a in args {
        let name = with_arch(a, ctx);
        if !(crate::install::is_safe_win_segment(&name)
            && ctx.versions_dir().join(&name).is_dir())
        {
            return Err(name);
        }
        names.push(name);
    }
    Ok(names.join(" "))
}

/// No arguments: the raw value or the "no version" line, on stdout, exit 0.
fn show(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    o.out(
        ctx.pyenv_version
            .as_deref()
            .unwrap_or("no shell-specific version configured"),
    );
    o
}

/// The command that applies `action` in a shell of `family`.
fn code(family: Family, action: &Action) -> Vec<String> {
    let line = match (family, action) {
        (Family::Pwsh, Action::Set(v)) => {
            format!("$Env:PYENV_VERSION = '{}'", v.replace('\'', "''"))
        }
        (Family::Pwsh, Action::Unset) => {
            "Remove-Item Env:PYENV_VERSION -ErrorAction SilentlyContinue".to_string()
        }
        (Family::Cmd, Action::Set(v)) => format!("set \"PYENV_VERSION={v}\""),
        (Family::Cmd, Action::Unset) => "set \"PYENV_VERSION=\"".to_string(),
        (Family::Fish, Action::Set(v)) => format!(
            "set -gx PYENV_VERSION '{}'",
            v.replace('\\', "\\\\").replace('\'', "\\'")
        ),
        (Family::Fish, Action::Unset) => "set -e PYENV_VERSION".to_string(),
        (_, Action::Set(v)) => format!("export PYENV_VERSION='{}'", v.replace('\'', "'\\''")),
        (_, Action::Unset) => "unset PYENV_VERSION".to_string(),
    };
    vec![line]
}

fn parent_image() -> Option<String> {
    #[cfg(windows)]
    {
        rpyenv_core::winproc::parent_image_name()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn pyenv_shell_var() -> Option<String> {
    std::env::var("PYENV_SHELL").ok().filter(|s| !s.is_empty())
}

/// `pyenv shell` without integration (spec §7 "Without integration", Decision 1).
pub fn shell(ctx: &Ctx, args: &[&str]) -> Output {
    let action = match args.first() {
        None => return show(ctx),
        Some(a) if a.eq_ignore_ascii_case("--unset") => Action::Unset,
        Some(_) => match validate(ctx, args) {
            Ok(v) => Action::Set(v),
            Err(name) => return not_installed(&name),
        },
    };
    let name = shellname::windows_shell(parent_image().as_deref(), pyenv_shell_var().as_deref());
    let mut o = Output::new();
    match name {
        Some(name) => {
            let family = shellname::family(&name, Flavor::PyenvWin);
            for l in code(family, &action) {
                o.out(l);
            }
            if family == Family::Cmd {
                o.err("pyenv: cmd has no shell integration, so nothing was changed: run the command above.");
            } else {
                o.err("pyenv: shell integration is not enabled in this shell, so nothing was changed: run the command above.");
                o.err(format!("pyenv: to enable it, see `pyenv init {name}`."));
            }
        }
        None => {
            for (label, family) in [
                ("cmd", Family::Cmd),
                ("PowerShell", Family::Pwsh),
                ("bash", Family::Posix),
                ("fish", Family::Fish),
            ] {
                for l in code(family, &action) {
                    o.out(format!("{label}: {l}"));
                }
            }
            o.err("pyenv: shell integration is not enabled, so nothing was changed: run the line for your shell above.");
        }
    }
    o.with_code(1)
}

/// The shell `sh-shell` and `sh-rehash` print code for: `PYENV_SHELL`, which `pyenv init`
/// set in the shell that runs the function, else the parent process.
fn code_family() -> Option<Family> {
    pyenv_shell_var()
        .and_then(|s| shellname::windows_name(&s))
        .or_else(|| shellname::windows_shell(parent_image().as_deref(), None))
        .map(|n| shellname::family(&n, Flavor::PyenvWin))
}

pub fn sh_shell(ctx: &Ctx, args: &[&str]) -> Output {
    let Some(family) = code_family() else {
        return Output::error("pyenv: can't tell which shell to print code for: set PYENV_SHELL, or load the integration (`pyenv init`)");
    };
    let evaluated = matches!(family, Family::Posix | Family::Ksh | Family::Fish);
    let action = match args.first() {
        // The POSIX and fish functions route `shell` without arguments here (Task 7):
        // print code that echoes the variable, as upstream's sh-shell does.
        None if evaluated => {
            return match ctx.pyenv_version {
                None => Output::error("pyenv: no shell-specific version configured"),
                Some(_) => {
                    let mut o = Output::new();
                    o.out("echo \"$PYENV_VERSION\"");
                    lf(o)
                }
            }
        }
        None => return show(ctx),
        Some(a) if a.eq_ignore_ascii_case("--unset") => Action::Unset,
        Some(_) => match validate(ctx, args) {
            Ok(v) => Action::Set(v),
            // bash and fish evaluate stdout: the message goes to stderr, then `false`.
            Err(name) if evaluated => {
                let m = not_installed(&name);
                let mut o = Output::new();
                o.stderr = m.stdout;
                o.out("false");
                return lf(o.with_code(1));
            }
            // The PowerShell function prints stdout when the exit code isn't 0 (Task 7).
            Err(name) => return not_installed(&name),
        },
    };
    let mut o = Output::new();
    for l in code(family, &action) {
        o.out(l);
    }
    if evaluated {
        lf(o)
    } else {
        o
    }
}

pub fn sh_rehash(_ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    match code_family() {
        Some(Family::Pwsh) => {
            o.out("& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash");
            return o;
        }
        Some(Family::Cmd) => {
            o.out("pyenv rehash");
            return o;
        }
        Some(Family::Fish) => o.out("command pyenv rehash"),
        _ => {
            o.out("command pyenv rehash");
            o.out("hash -r 2>/dev/null || true");
        }
    }
    lf(o)
}
```

- [ ] **Step 5: Wire it in.**
  - `commands/mod.rs`: add `pub mod shell_win;` and these `WIN_ONLY` rows:

```rust
    ("sh-rehash", shell_win::sh_rehash),
    ("sh-shell", shell_win::sh_shell),
    ("shell", shell_win::shell),
```

  - `completions.rs`: add to `TABLE_WIN`, in alphabetical order, `e("shell", true, Words(&["--unset"], VersionsBare)),`. On Windows the plain `shell` command exists, so `pyenv completions shell` finds it first. Then extend the test module:

```rust
    #[test]
    fn every_windows_entry_names_a_command() {
        for e in TABLE_WIN {
            assert!(
                commands::lookup(Flavor::PyenvWin, e.name).is_some(),
                "{}",
                e.name
            );
        }
    }
```

  - `help.rs` `PYENV_WIN`: add `topic("sh-shell", None, None, commands::shell_win::HELP),` and `topic("shell", None, None, commands::shell_win::HELP),`, in alphabetical order.
  - `lib.rs` `run_pyenv_win`: immediately before `match commands::lookup(Flavor::PyenvWin, &cmd)`:

```rust
    // As upstream's dispatcher does for the POSIX function to evaluate (libexec/pyenv:133-136).
    if cmd.starts_with("sh-")
        && rest.first() == Some(&"--help")
        && commands::lookup(Flavor::PyenvWin, &cmd).is_some()
    {
        let mut o = Output::new();
        o.out(format!("pyenv help \"{cmd}\""));
        return o;
    }
```

- [ ] **Step 6: Amend spec §7, Decision 1.** In `docs/specs/2026-09-27-rpyenv-design.md` §7 "Without integration", replace step 1 with:

```markdown
1. pick the shell from the parent process's image name (`cmd.exe`,
   `powershell.exe`/`pwsh.exe`, `bash`/`zsh`/`fish`), trusted only if the parent
   was created before `pyenv` itself, which guards against a reused process ID;
   else from `PYENV_SHELL`. (Amended 2026-10-04: the parent comes first because
   only `pyenv init` sets `PYENV_SHELL`, so a shell started from an integrated
   one, such as `cmd` run from PowerShell, inherits a value that names the
   wrong shell.)
```

- [ ] **Step 7: Run the tests and see them pass.**

Run on Windows:
- `cargo test -p pyenv --test cli_shell_win`. Expected: `8 passed`.
- `cargo test --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings`.

Run in WSL: `cargo test --workspace`. `shell_win.rs` compiles there, and its tests are Windows-only.

- [ ] **Step 8: Commit.**

```bash
git add crates/pyenv docs/specs/2026-09-27-rpyenv-design.md
git commit -m "Add Windows pyenv shell with the detect-and-print fallback, sh-shell and sh-rehash"
```

---

### Task 7: Windows `pyenv init`

Read spec §7 and Decisions 2, 7, 8 and 10. pyenv-win has no `init` (M3W "Inventory"), so the PowerShell code here is rpyenv's own. The bash, zsh and fish code is upstream's, built by Task 3's helpers with MSYS paths.

**Files:**
- Create: `crates/pyenv/src/commands/init_win.rs`, `crates/pyenv/tests/cli_init_win.rs`
- Modify:
  - `crates/pyenv/src/commands/mod.rs`: `pub mod init_win;` and the `WIN_ONLY` row `("init", init_win::init)`;
  - `crates/pyenv/src/commands/init.rs`: make `Mode` public within the crate, which it already is as `pub(crate)`;
  - `docs/specs/2026-09-27-rpyenv-design.md`: the §7 table, and a note for Decision 2.

**Interfaces:**
- Consumes:
  - `init::{parse, Mode, init_dirs, posix_path_lines, fish_path_lines, posix_function, fish_function}` (Tasks 3 and 4);
  - `shell_win::lf` (Task 6);
  - `crate::install_prefix` (Task 3);
  - `winproc::parent_image_name` (Task 1).
- Produces: `commands::init_win::init: Command`.

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/cli_init_win.rs`:

```rust
//! Windows `pyenv init` (spec §7; Decisions 2, 7, 8, 10; allowlist D-91).
#![cfg(windows)]

mod common;
use common::winshell::{self, host, output};
use common::Fixture;

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

/// `C:\x\y` → `/c/x/y` (Decision 8).
fn msys(p: &std::path::Path) -> String {
    let s = p.display().to_string().replace('\\', "/");
    format!("/{}{}", s[..1].to_ascii_lowercase(), &s[2..])
}

const FUNCTION: &str = "function pyenv {\n  $command = ''\n  if ($args.Count -gt 0) { $command, $args = $args }\n  $pyenv = Get-Command -CommandType Application pyenv -TotalCount 1\n  if ($command -eq 'shell' -and $args.Count -gt 0 -and $args[0] -ne '--help') {\n    $shell_cmds = & $pyenv sh-shell @args\n    if ($LASTEXITCODE -ne 0) { $shell_cmds; return }\n    if ($shell_cmds) { Invoke-Expression ($shell_cmds -join \"`n\") }\n  } elseif ($command -eq '') {\n    & $pyenv\n  } else {\n    & $pyenv $command @args\n  }\n}\n";

#[test]
fn print_mode_for_powershell() {
    let f = Fixture::new();
    let s = f.root.join("shims").display().to_string();
    let want = format!("$Env:PATH = (($Env:PATH -split ';') | Where-Object {{ $_ -ne '{s}' -and $_ -ne '{s}\\' }}) -join ';'\n$Env:PATH = '{s};' + $Env:PATH\n$Env:PYENV_SHELL = 'pwsh'\n{FUNCTION}");
    assert_eq!(run(&f, &["init", "-", "pwsh", "--no-rehash"]), (want.replace('\n', "\r\n"), String::new(), 0));
    assert!(f.root.join("shims").is_dir() && f.root.join("versions").is_dir());
    let push = run(&f, &["init", "-", "powershell", "--no-push-path"]).0;
    assert!(push.starts_with(&format!("if (-not (($Env:PATH -split ';') -contains '{s}')) {{ $Env:PATH = '{s};' + $Env:PATH }}\r\n$Env:PYENV_SHELL = 'powershell'\r\n& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash\r\n")));
}

#[test]
fn print_mode_for_bash_uses_msys_paths_and_lf() {
    let f = Fixture::new();
    let s = msys(&f.root.join("shims"));
    let out = run(&f, &["init", "-", "bash", "--no-push-path", "--no-rehash"]).0;
    assert_eq!(
        out,
        format!("if [[ \":$PATH:\" != *':{s}:'* ]]; then\nexport PATH=\"{s}:${{PATH}}\"\nfi\nexport PYENV_SHELL=bash\npyenv() {{\n  local command=${{1:-}}\n  [ \"$#\" -gt 0 ] && shift\n  case \"$command\" in\n  rehash|shell)\n    eval \"$(pyenv \"sh-$command\" \"$@\")\"\n    ;;\n  *)\n    command pyenv \"$command\" \"$@\"\n    ;;\n  esac\n}}\n")
    );
    assert!(run(&f, &["init", "-", "fish"]).0.contains(&format!("set -gx PATH '{s}' $PATH\n")));
}

#[test]
fn help_and_refusals() {
    let f = Fixture::new();
    let tail = "\r\n# Restart your shell for the changes to take effect.\r\n\r\n";
    assert_eq!(
        run(&f, &["init", "pwsh"]),
        (String::new(), format!("# Load pyenv automatically by appending\r\n# the following to your PowerShell profile ($PROFILE) :\r\n\r\niex ((pyenv init - pwsh) -join \"`n\")\r\n{tail}"), 1)
    );
    assert_eq!(
        run(&f, &["init", "bash"]).1,
        format!("# Load pyenv automatically by appending\r\n# the following to ~/.bashrc :\r\n\r\neval \"$(pyenv init - bash)\"\r\n{tail}")
    );
    assert_eq!(
        run(&f, &["init", "fish"]).1,
        format!("# Load pyenv automatically by appending\r\n# the following to ~/.config/fish/config.fish:\r\n\r\npyenv init - fish | source\r\n{tail}")
    );
    assert_eq!(
        run(&f, &["init", "cmd"]),
        (String::new(), "# cmd has no shell integration. `pyenv shell` prints the `set`\r\n# command to run, and every other command needs no setup.\r\n\r\n".to_string(), 1)
    );
    // Decision 2: no startup-file editing on Windows before M6.
    assert_eq!(
        run(&f, &["init", "--install", "pwsh"]),
        (String::new(), "pyenv: cannot automatically configure startup files for pwsh\r\n".to_string(), 1)
    );
    assert_eq!(run(&f, &["init", "-", "cmd"]).2, 1);
    // Run by this test, no shell is detected.
    assert_eq!(
        run(&f, &["init"]).1,
        "# pyenv can't tell which shell runs it. Name one: `pyenv init <shell>`,\r\n# where <shell> is pwsh, powershell, bash, zsh, fish or cmd.\r\n\r\n"
    );
}

#[test]
fn detect_shell_from_cmd_and_powershell() {
    let f = Fixture::new();
    winshell::install_pyenv(&f);
    let mut c = host(&f, &winshell::cmd_exe(), &[], &[]);
    c.args(["/d", "/c", "call", "pyenv", "init", "--detect-shell"]);
    assert_eq!(output(c).0, "PYENV_SHELL_DETECT=cmd\r\nPYENV_PROFILE_DETECT=\r\nPYENV_RC_DETECT=\r\n");
    let mut c = host(&f, &winshell::powershell(), &[], &[]);
    c.args(["-NoProfile", "-NonInteractive", "-Command", "pyenv init --detect-shell"]);
    assert_eq!(output(c).0, "PYENV_SHELL_DETECT=powershell\r\nPYENV_PROFILE_DETECT=$PROFILE\r\nPYENV_RC_DETECT=$PROFILE\r\n");
}

/// allowlist D-90
/// Review focus 2: the fixture's root (a space and `ñ`) reaches PowerShell's PATH
/// unchanged, and the function sets, shows and unsets the version, and reports a failure.
#[test]
fn powershell_runs_init_and_shell() {
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let shims = f.root.join("shims").display().to_string().replace('\'', "''");
    let script = format!("iex ((pyenv init - powershell --no-rehash) -join \"`n\"); pyenv shell 3.7.7; $env:PYENV_VERSION; (($env:PATH -split ';')[0] -eq '{shims}'); pyenv shell; pyenv shell --unset; [string]::IsNullOrEmpty($env:PYENV_VERSION); pyenv shell 9.9.9; $LASTEXITCODE");
    let mut shells = vec![winshell::powershell()];
    shells.extend(winshell::pwsh());
    for sh in shells {
        let mut c = host(&f, &sh, &[], &[]);
        c.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        let (out, err, _) = output(c);
        assert_eq!(
            out,
            "3.7.7\r\nTrue\r\n3.7.7\r\nTrue\r\npyenv specific python requisite didn't meet. Project is using different version of python.\r\nInstall python '9.9.9' by typing: 'pyenv install 9.9.9'\r\n1\r\n",
            "{}: {err}",
            sh.display()
        );
    }
}

/// Git Bash, when installed: the bash code with MSYS paths runs and routes `shell`.
#[test]
fn git_bash_runs_init_and_shell() {
    let Some(bash) = winshell::git_bash() else {
        eprintln!("skipped: no Git Bash");
        return;
    };
    let f = Fixture::new();
    f.version("3.7.7");
    winshell::install_pyenv(&f);
    let usr_bin = std::path::PathBuf::from(r"C:\Program Files\Git\usr\bin");
    let mut c = host(&f, &bash, &[], &[&usr_bin]);
    c.args(["-c", "eval \"$(pyenv init - bash --no-rehash)\"; pyenv shell 3.7.7; echo \"$PYENV_VERSION\"; type -t pyenv; pyenv shell 9.9.9; echo \"rc=$?\""]);
    let (out, err, _) = output(c);
    assert_eq!(out, "3.7.7\nfunction\nrc=1\n", "{err}");
}
```

- [ ] **Step 2: Run the tests and see them fail.**

Run on Windows: `cargo test -p pyenv --test cli_init_win`
Expected: `pyenv: no such command 'init'`.

- [ ] **Step 3: Implement `init_win.rs`.**

```rust
//! `pyenv init` on Windows (spec §7; allowlist D-91, since pyenv-win has none). PowerShell
//! gets rpyenv's own code (Decision 7); bash, zsh and fish get upstream's code with MSYS
//! paths (Decision 8); cmd gets no integration (Decision 10).

use crate::commands::init::{self, Mode};
use crate::commands::shell_win::lf;
use crate::commands::{self, Listing};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::shellname::{self, Family};
use std::path::Path;

/// The function the profile line defines. It routes `shell` only with arguments, and not
/// for `--help`, as pyenv-win's `pyenv.ps1` does; a failure's message, printed by
/// `sh-shell` on stdout, is shown as is (Decision 7).
const PWSH_FUNCTION: &str = r#"function pyenv {
  $command = ''
  if ($args.Count -gt 0) { $command, $args = $args }
  $pyenv = Get-Command -CommandType Application pyenv -TotalCount 1
  if ($command -eq 'shell' -and $args.Count -gt 0 -and $args[0] -ne '--help') {
    $shell_cmds = & $pyenv sh-shell @args
    if ($LASTEXITCODE -ne 0) { $shell_cmds; return }
    if ($shell_cmds) { Invoke-Expression ($shell_cmds -join "`n") }
  } elseif ($command -eq '') {
    & $pyenv
  } else {
    & $pyenv $command @args
  }
}
"#;

const RESTART: &str = "# Restart your shell for the changes to take effect.";

pub fn init(ctx: &Ctx, args: &[&str]) -> Output {
    let a = init::parse(args);
    let shell = a
        .shell
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(detected);
    let family = shellname::family(&shell, Flavor::PyenvWin);
    match a.mode {
        Mode::Help => help(&shell, family),
        Mode::DetectShell => detect(&shell, family),
        // Decision 2: startup files are M6's `pyenv setup`.
        Mode::Install => Output::error(format!(
            "pyenv: cannot automatically configure startup files for {shell}"
        )),
        Mode::Path | Mode::Print if family == Family::Cmd => Output::error(
            "pyenv: cmd has no shell integration; `pyenv init cmd` explains what works without it",
        ),
        Mode::Path => {
            let mut o = Output::new();
            path_lines(&mut o, ctx, family, a.no_push_path);
            rehash_line(&mut o, family, a.no_rehash);
            finish(o, family)
        }
        Mode::Print => {
            if let Err(o) = init::init_dirs(ctx) {
                return o;
            }
            let mut o = Output::new();
            path_lines(&mut o, ctx, family, a.no_push_path);
            env_line(&mut o, &shell, family);
            completion_line(&mut o, &shell, family);
            rehash_line(&mut o, family, a.no_rehash);
            o.stdout.push_str(&function(family));
            finish(o, family)
        }
    }
}

/// The parent process, when it is a supported shell; empty otherwise.
fn detected() -> String {
    #[cfg(windows)]
    let parent = rpyenv_core::winproc::parent_image_name();
    #[cfg(not(windows))]
    let parent: Option<String> = None;
    parent
        .as_deref()
        .and_then(shellname::windows_name)
        .unwrap_or_default()
}

/// PowerShell code keeps the console's line ends and code page; bash, zsh and fish code
/// goes out as `\n` and UTF-8 (Task 6, `lf`).
fn finish(o: Output, family: Family) -> Output {
    if family == Family::Pwsh {
        o
    } else {
        lf(o)
    }
}

/// `C:\x\y` → `/c/x/y`, `\\server\share` → `//server/share` (Decision 8).
pub(crate) fn msys(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        format!("/{}{}", (b[0] as char).to_ascii_lowercase(), &s[2..])
    } else {
        s
    }
}

/// A PowerShell single-quoted literal.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn path_lines(o: &mut Output, ctx: &Ctx, family: Family, no_push_path: bool) {
    let shims = ctx.shims_dir();
    let lines = match family {
        // `-ne` and `-contains` compare without case, as Windows paths do.
        Family::Pwsh => {
            let s = shims.display().to_string();
            if no_push_path {
                vec![format!(
                    "if (-not (($Env:PATH -split ';') -contains {})) {{ $Env:PATH = {} + $Env:PATH }}",
                    sq(&s),
                    sq(&format!("{s};"))
                )]
            } else {
                vec![
                    format!(
                        "$Env:PATH = (($Env:PATH -split ';') | Where-Object {{ $_ -ne {} -and $_ -ne {} }}) -join ';'",
                        sq(&s),
                        sq(&format!("{s}\\"))
                    ),
                    format!("$Env:PATH = {} + $Env:PATH", sq(&format!("{s};"))),
                ]
            }
        }
        Family::Fish => init::fish_path_lines(&msys(&shims), no_push_path),
        _ => init::posix_path_lines(&msys(&shims), no_push_path),
    };
    for l in lines {
        o.out(l);
    }
}

fn env_line(o: &mut Output, shell: &str, family: Family) {
    o.out(match family {
        Family::Pwsh => format!("$Env:PYENV_SHELL = {}", sq(shell)),
        Family::Fish => format!("set -gx PYENV_SHELL {shell}"),
        _ => format!("export PYENV_SHELL={shell}"),
    });
}

/// `<prefix>\completions\pyenv.<shell>` when readable; Windows PowerShell 5.1 uses the
/// pwsh script. `iex (Get-Content …)` runs it whatever the execution policy says.
fn completion_line(o: &mut Output, shell: &str, family: Family) {
    let Some(prefix) = crate::install_prefix() else {
        return;
    };
    let script = if family == Family::Pwsh { "pwsh" } else { shell };
    let path = prefix.join("completions").join(format!("pyenv.{script}"));
    if std::fs::File::open(&path).is_err() {
        return;
    }
    o.out(match family {
        Family::Pwsh => format!("iex (Get-Content {} -Raw)", sq(&path.display().to_string())),
        _ => format!("source '{}'", msys(&path)),
    });
}

fn rehash_line(o: &mut Output, family: Family, no_rehash: bool) {
    if !no_rehash {
        o.out(if family == Family::Pwsh {
            "& (Get-Command -CommandType Application pyenv -TotalCount 1) rehash"
        } else {
            "command pyenv rehash"
        });
    }
}

fn function(family: Family) -> String {
    let routed = commands::names(Flavor::PyenvWin, Listing::ShOnly);
    match family {
        Family::Pwsh => PWSH_FUNCTION.to_string(),
        Family::Fish => init::fish_function(&routed),
        Family::Ksh => init::posix_function("function pyenv {\n  typeset command=${1:-}\n", &routed),
        _ => init::posix_function("pyenv() {\n  local command=${1:-}\n", &routed),
    }
}

fn help(shell: &str, family: Family) -> Output {
    let lines: Vec<String> = if shell.is_empty() {
        vec![
            "# pyenv can't tell which shell runs it. Name one: `pyenv init <shell>`,".into(),
            "# where <shell> is pwsh, powershell, bash, zsh, fish or cmd.".into(),
            String::new(),
        ]
    } else {
        match family {
            Family::Cmd => vec![
                "# cmd has no shell integration. `pyenv shell` prints the `set`".into(),
                "# command to run, and every other command needs no setup.".into(),
                String::new(),
            ],
            Family::Pwsh => vec![
                "# Load pyenv automatically by appending".into(),
                "# the following to your PowerShell profile ($PROFILE) :".into(),
                String::new(),
                format!("iex ((pyenv init - {shell}) -join \"`n\")"),
                String::new(),
                RESTART.into(),
                String::new(),
            ],
            Family::Fish => vec![
                "# Load pyenv automatically by appending".into(),
                "# the following to ~/.config/fish/config.fish:".into(),
                String::new(),
                "pyenv init - fish | source".into(),
                String::new(),
                RESTART.into(),
                String::new(),
            ],
            _ => {
                let rc = match shell {
                    "bash" => "~/.bashrc",
                    "zsh" => "~/.zshrc",
                    _ => "your shell's interactive startup file",
                };
                vec![
                    "# Load pyenv automatically by appending".into(),
                    format!("# the following to {rc} :"),
                    String::new(),
                    format!("eval \"$(pyenv init - {shell})\""),
                    String::new(),
                    RESTART.into(),
                    String::new(),
                ]
            }
        }
    };
    let mut o = Output::new();
    for l in lines {
        o.err(l);
    }
    o.with_code(1)
}

fn detect(shell: &str, family: Family) -> Output {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let (profile, rc) = match (family, shell) {
        (Family::Pwsh, _) => ("$PROFILE", "$PROFILE"),
        (Family::Fish, _) => ("~/.config/fish/config.fish", "~/.config/fish/config.fish"),
        (Family::Ksh, _) => ("~/.profile", "~/.profile"),
        (_, "bash") if Path::new(&home).join(".bash_profile").exists() => {
            ("~/.bash_profile", "~/.bashrc")
        }
        (_, "bash") => ("~/.profile", "~/.bashrc"),
        (_, "zsh") => ("~/.zprofile", "~/.zshrc"),
        _ => ("", ""),
    };
    let mut o = Output::new();
    o.out(format!("PYENV_SHELL_DETECT={shell}"));
    o.out(format!("PYENV_PROFILE_DETECT={profile}"));
    o.out(format!("PYENV_RC_DETECT={rc}"));
    o
}
```

- [ ] **Step 4: Register `init` on Windows.** In `commands/mod.rs`, add `pub mod init_win;` and the `WIN_ONLY` row `("init", init_win::init)`. `help init` on Windows uses upstream's topic, as D-12 provides for commands pyenv-win lacks.

- [ ] **Step 5: Amend spec §7, Decisions 2, 7 and 8.**
  - In the profile-line table, the PowerShell row's line becomes ``iex ((pyenv init - pwsh) -join "`n")``.
  - The bash row's note becomes "Git Bash gets `/c/...` paths (always: the bash, zsh and fish that run on Windows are MSYS2-based)".
  - Below the table, add: "On Windows, `pyenv init --install` refuses until M6's `pyenv setup` edits profiles (decided 2026-10-04). `pyenv init - pwsh | Invoke-Expression` would run each output line on its own and break the multi-line function, hence `-join`."

- [ ] **Step 6: Run the tests and see them pass.**

Run on Windows:
- `cargo test -p pyenv --test cli_init_win`. Expected: `6 passed`. `git_bash_runs_init_and_shell` prints `skipped` where Git Bash is absent; GitHub's Windows runners have it.
- `cargo test --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings`.

Run in WSL: `cargo test --workspace`.

- [ ] **Step 7: Commit.**

```bash
git add crates/pyenv docs/specs/2026-09-27-rpyenv-design.md
git commit -m "Add Windows pyenv init for PowerShell, Git Bash and fish"
```

### Task 8: Linux parity: the bats harness, expected failures, diff cases, D-88

Read M3L "Upstream bats tests in rpyenv's M3 target". The 58 rows tagged "M3" in `parity/expected/bats.txt` should now pass, except the four this task re-tags.

**Files:**
- Modify:
  - `parity/bats_run.sh`;
  - `parity/expected/bats.txt`;
  - `parity/diff_cases.py`;
  - `parity/golden/linux/*` (regenerated);
  - `docs/parity/allowlist.md` (D-88).

**Interfaces:**
- Consumes: every command from Tasks 2–5, and `completions/` (Task 5).
- Produces: a green Linux parity run with no "M3" reason left in `bats.txt`. Task 9 relies on that when it adds `M3` to `DELIVERED`.

- [ ] **Step 1: Change the harness layout and wrappers.** In `parity/bats_run.sh`, replace the block from `work=$(mktemp …)` through the wrapper loop with the code below. It puts rpyenv's binaries in `<run>/bin`, so rpyenv's install prefix is the run tree, as `_PYENV_INSTALL_PREFIX` is in the suite (Decision 3). It also copies `completions/` there and adds the M3 wrappers.

```bash
repo=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d /tmp/rpyenv-bats.XXXXXX)
chmod 755 "$work"
# rpyenv's install prefix is the parent of its binary's folder (plan M3 Decision 3), so the
# binaries go in <run>/bin: `pyenv init -` then sources <run>/completions/pyenv.<shell>,
# the file the suite expects under `_PYENV_INSTALL_PREFIX`.
install -d -m 755 "$work/run" "$work/run/bin" "$work/run/libexec"
install -m 755 "$bin/pyenv" "$bin/pyenv-shim" "$work/run/bin/"
cp -r "$repo/completions" "$work/run/completions"
# The suite puts `<test>/../libexec` on PATH: there, `pyenv` is rpyenv and each `pyenv-<cmd>`
# the suite calls directly is a wrapper for `pyenv <cmd>`. The wrappers use the absolute path: a
# test may stub `pyenv` on PATH, and upstream's `pyenv-<cmd>` scripts never go through it.
cp -r "$up/test" "$work/run/test"
cp -r "$up/pyenv.d" "$work/run/pyenv.d"
ln -s "$work/run/bin/pyenv" "$work/run/libexec/pyenv"
# When a milestone delivers a command, add it to this list, or its upstream tests keep
# failing through the missing wrapper. `shell` has none: upstream has no `pyenv-shell` either.
for c in root prefix version version-name version-origin version-file version-file-read \
         version-file-write versions which whence exec rehash shims commands help global local latest \
         init sh-shell sh-rehash completions; do
  printf '#!/bin/sh\nexec "%s" %s "$@"\n' "$work/run/bin/pyenv" "$c" > "$work/run/libexec/pyenv-$c"
done
printf '#!/bin/sh\nexec "%s" --version "$@"\n' "$work/run/bin/pyenv" > "$work/run/libexec/pyenv---version"
```

The lines after the loop (`chmod 755 …pyenv-*`, the `tester` user, `chown -R tester "$work/run/test" "$work/run/pyenv.d"`, the run loop) stay as they are. `bin` and `completions` stay root-owned, so no test can change them.

- [ ] **Step 2: Run the suite in WSL.** Write `C:\tmp\m3_bats.sh`:

```bash
#!/usr/bin/env bash
# Fetch the M3 branch, build, run upstream's bats suite as root, and judge the TAP log.
set -u
source /home/jm/.cargo/env
R=/home/jm/rpyenv-linux
UP=/home/jm/m1cb-upstream/pyenv-07171d013cac53d1cc9248b4c160217f288a2965
git -C "$R" fetch -q /mnt/c/Users/JM/repos/OWN/rpyenv m3-shell-integration
git -C "$R" reset -q --hard FETCH_HEAD
cd "$R" && cargo build -q --workspace
bash "$R/parity/bats_run.sh" "$R/target/debug" "$UP" /home/jm/m1cb-upstream/bats-core-1.11.1 /home/jm/m3-tap.txt
chown jm:jm /home/jm/m3-tap.txt
runuser -u jm -- python3 "$R/parity/bats_check.py" /home/jm/m3-tap.txt
```

Run it with `wsl -d Debian -u root --exec /usr/bin/bash /mnt/c/tmp/m3_bats.sh` after committing Step 1 (the script fetches the branch). Expected: `bats_check.py` reports the M3 rows that now pass as problems ("expected to fail, but passed"). These are the rows to remove.

- [ ] **Step 3: Update `parity/expected/bats.txt`.**
  - Remove every M3 row whose test now passes.
  - Re-tag these four, which stay failing for a reason outside M3:

```text
completions.bats | command with no completion support | M4 plugin dispatch (`pyenv-<cmd>` on PATH)
completions.bats | command with completion support | M4 plugin dispatch (`pyenv-<cmd>` on PATH)
completions.bats | forwards extra arguments | M4 plugin dispatch (`pyenv-<cmd>` on PATH)
init.bats | outputs sh-compatible case syntax | D-88 the routed set is rpyenv's built-in sh- commands
```

  - **Any other M3 row that still fails is a bug against M3L.** Fix the code in this task, re-run Step 2, and don't add a row for it. The tests above cover the fixed behavior. Typical causes: a missing wrapper, or a text difference visible in the TAP log (`grep -A20 'not ok' /home/jm/m3-tap.txt`).
  - Re-run Step 2 until `bats_check.py` reports `0 problems` and no line in `bats.txt` starts a reason with `M3`. Check with `grep -c '| M3' parity/expected/bats.txt`, which should give 0.

- [ ] **Step 4: Add D-88 to `docs/parity/allowlist.md`.** Insert this row after D-87, copied from the Decisions table:

```markdown
| D-88 | Linux | `init -` (the shell function) | The routed commands are every `pyenv-sh-*` file on PATH, including plugins' (`pyenv-commands --sh`) | `rehash` and `shell` only | Plugin dispatch is M4 (spec §14); the built-in set is rpyenv's command table |
```

- [ ] **Step 5: Add Linux diff cases.** Append to `CASES` in `parity/diff_cases.py`, before the closing parenthesis.
  - Every case names its shell: a missing shell argument would detect `diff.py`'s own Python process.
  - None prints a `source` line (bash, fish and pwsh print mode), because rpyenv's and upstream's install prefixes differ; the bats suite covers those lines.

```python
    # M3: shell integration (Linux). Each names its shell; print mode avoids the shells
    # whose completion line names the install prefix, which differs (bats covers those).
    Case("init --detect-shell bash", ("init", "--detect-shell", "bash"), os="Linux"),
    Case("init help for bash", ("init", "bash"), os="Linux"),
    Case("init help for fish", ("init", "fish"), os="Linux"),
    Case("init help for pwsh", ("init", "pwsh"), os="Linux"),
    Case("init - ksh", ("init", "-", "ksh"), os="Linux"),
    Case("init - sh --no-push-path --no-rehash", ("init", "-", "sh", "--no-push-path", "--no-rehash"), os="Linux"),
    Case("init --path sh", ("init", "--path", "sh"), os="Linux"),
    Case("init --install for an unsupported shell", ("init", "--install", "nu"), os="Linux"),
    Case("init - ksh with a plugin sh- command", ("init", "-", "ksh"), os="Linux",
         files=(("root/plugins/fake/bin/pyenv-sh-activate", "#!/bin/sh\n"),), allow=("D-88",)),
    Case("sh-shell --unset in fish", ("sh-shell", "--unset"), os="Linux", env=(("PYENV_SHELL", "fish"),)),
    Case("sh-shell - in bash", ("sh-shell", "-"), os="Linux", env=(("PYENV_SHELL", "bash"),)),
    Case("sh-shell sets a version in pwsh", ("sh-shell", "{v1}"), os="Linux", env=(("PYENV_SHELL", "pwsh"),)),
    Case("sh-shell with a version not installed", ("sh-shell", "9.9"), os="Linux"),
    Case("sh-rehash in pwsh", ("sh-rehash",), os="Linux", env=(("PYENV_SHELL", "pwsh"),)),
    Case("shell without integration", ("shell", "{v1}"), os="Linux"),
    Case("completions shell", ("completions", "shell"), os="Linux"),
    Case("completions with no command", ("completions",), os="Linux"),
    Case("commands --sh", ("commands", "--sh"), os="Linux"),
    Case("help shell", ("help", "shell"), os="Linux"),
    Case("help --usage init", ("help", "--usage", "init"), os="Linux"),
    Case("help sh-rehash", ("help", "sh-rehash"), os="Linux"),
```

If `files` can't create a path under `root/plugins/…`, read `build()` in `parity/diff.py` and use the form it supports. Upstream's dispatcher puts `$PYENV_ROOT/plugins/*/bin` on PATH, and `pyenv-commands --sh` lists any file there, executable or not.

- [ ] **Step 6: Regenerate and review the Linux goldens.** Write `C:\tmp\m3_golden.sh` like the 2.8.8 sync's golden script:
  - `--update-golden` in `~/rpyenv-linux` against `$UP`;
  - copy `parity/golden/linux` back to the Windows checkout;
  - then a plain run.

Run it, then review `git status parity/golden` and `git diff parity/golden`. Expected:
- one new pair of golden files per new case;
- changed files only for `commands`, `help` and `no arguments`, where the listing gains `completions`, `init` and `shell`;
- no other line changes.

The final summary line must read `Linux: <n> allowed, <m> same` with no `differs`, where `<n>` is today's 23 plus the D-88 case.

- [ ] **Step 7: Run every check.**
  - In WSL:
    - `cargo fmt --all --check`, clippy and `cargo test --workspace`;
    - `python3 -m unittest discover -s parity -p "test_*.py"`;
    - `python3 parity/coverage.py`, which should report `allowlist rows: 88; covered: 88`;
    - the diff;
    - Step 2's bats run, with `0 problems`.
  - On Windows: `cargo test --workspace` and `python parity/coverage.py`.

- [ ] **Step 8: Commit.**

```bash
git add parity docs/parity/allowlist.md crates
git commit -m "Run upstream's M3 bats tests against rpyenv and add the Linux shell-integration diff cases"
```

---

### Task 9: Windows parity: the overlay, expected failures, diff cases, D-89 to D-91, `DELIVERED`

Read M3W "Overlay results against rpyenv" and Decision 9.

**Files:**
- Modify:
  - `parity/pyenv_win_overlay.py`;
  - `parity/expected/pyenv-win.txt`;
  - `parity/diff_cases.py`;
  - `parity/golden/windows/*` (regenerated);
  - `parity/allowlist.py` (`DELIVERED`);
  - `docs/parity/allowlist.md` (D-89, D-90, D-91).

**Interfaces:**
- Consumes: Tasks 6 and 7, and Task 8's empty set of "M3" reasons in `bats.txt`.
- Produces: M3 delivered. `DELIVERED = ("M1", "M2a", "M2b", "M3")`.

- [ ] **Step 1: Change the overlay.** In `parity/pyenv_win_overlay.py`, replace the `pyenv_file` fixture with:

```python
@pytest.fixture()
def pyenv_file(bin_path, shell):
    path = str(Path(bin_path, "pyenv.exe"))
    # The suite's own fixture escapes spaces for PowerShell (tests/conftest.py:55-60);
    # unescaped, PowerShell splits the path and never starts pyenv.exe (plan M3 Decision 9a).
    return path if shell == "cmd" else path.replace(" ", "` ")


@pytest.fixture()
def run_args(shell):
    if shell == "cmd":
        return ["cmd", "/d", "/c", "call"]
    # PowerShell loads rpyenv's integration first, as the profile line would (allowlist
    # D-90, plan M3 Decision 9c). --no-rehash: a test's shims are its own business.
    return [shell, "-Command", 'iex ((pyenv init - pwsh --no-rehash) -join "`n");']
```

In `tmp_pyenv`, right after `pyenv_setup(settings)`, add:

```python
    # No pyenv-win entry point may run: PowerShell would pick pyenv.ps1 over pyenv.exe
    # (plan M3 Decision 9b).
    for f in ("pyenv.ps1", "pyenv.bat", "pyenv"):
        p = Path(bin_path, f)
        if p.exists():
            p.unlink()
```

Check `tests/conftest.py` in the pinned pyenv-win checkout for the exact name and parameters of `run_args`. M3W "How each shell is invoked" quotes the default: `['powershell', '-Command']` and `['cmd', '/d', '/c', 'call']`. Keep the overlay's signature compatible.

- [ ] **Step 2: Run the shell tests, then the whole suite.**

```
python parity\pyenv_win_run.py --pyenv-win C:/tmp/pyenv-win-856ed5a --rpyenv target\debug -k test_pyenv_feature_shell --runxfail
```

Expected:
- every powershell and pwsh id passes;
- the cmd ids pass except the two that need the variable to change: `test_shell_set_installed_version[cmd-<lambda>]` and `test_shell_set_many_versions[cmd-<lambda>]`;
- `test_shell_unset_unaffected[cmd]` only reads the variable, so it is expected to pass.

Then run the whole suite without `-k` and `--runxfail`, as CI does, after Step 3.

- [ ] **Step 3: Update `parity/expected/pyenv-win.txt`.**
  - Remove the 21 "M3" rows.
  - Add a row for each cmd id that Step 2 showed still failing:

```text
test_pyenv_feature_shell.py::test_shell_set_installed_version[cmd-<lambda>] | D-89 an executable can't change cmd's environment
test_pyenv_feature_shell.py::test_shell_set_many_versions[cmd-<lambda>] | D-89 an executable can't change cmd's environment
```

  - Any other failing shell id is a bug against M3W: fix it, re-run, and don't add a row.
  - Re-run the whole suite. Expected: exit 0, with no XPASS and no unexpected failure.

- [ ] **Step 4: Add D-89, D-90 and D-91 to `docs/parity/allowlist.md`.** Insert them after D-88, copied from the Decisions table.

- [ ] **Step 5: Add Windows diff cases.** Append to `CASES`:

```python
    # M3: shell integration (Windows). pyenv-win has no init or completions (D-91), and
    # its `shell` sets the variable through pyenv.bat, which rpyenv can't (D-89).
    Case("init help for pwsh", ("init", "pwsh"), os="Windows", allow=("D-91",)),
    Case("completions shell", ("completions", "shell"), os="Windows", allow=("D-91",)),
    Case("shell with no version set", ("shell",), os="Windows"),
    Case("shell with a version not installed", ("shell", "9.9"), os="Windows"),
    Case("shell sets a version", ("shell", "{v1}"), os="Windows", allow=("D-89",)),
```

Run with `--update-golden`, then plainly:

```
python parity\diff.py --rpyenv target\debug --upstream C:/tmp/pyenv-win-856ed5a/pyenv-win --update-golden
python parity\diff.py --rpyenv target\debug --upstream C:/tmp/pyenv-win-856ed5a/pyenv-win
```

Expected: no `differs`.
- The two `shell` cases without a row must be `same`; that is pyenv-win's own output.
- If `shell with no version set` differs, check whether `diff.py` sets `PYENV_VERSION` in its fixture. Compare against pyenv-win's actual output before changing anything.
- Review `git diff parity/golden/windows`. `commands`, `help` and `no arguments` change only where rpyenv's listing gains the new commands; pyenv-win's help texts are fixed strings.

- [ ] **Step 6: Deliver M3.** In `parity/allowlist.py`, set `DELIVERED = ("M1", "M2a", "M2b", "M3")`. Then:

```
python -m unittest discover -s parity -p "test_*.py"
python parity\coverage.py
```

Expected: every test passes, and coverage reports `allowlist rows: 91; covered: 91`:
- D-88 by the Linux diff case;
- D-89 by the Windows diff case and the test citation in `cli_shell_win.rs`;
- D-90 by the citation in `cli_init_win.rs`;
- D-91 by the diff cases.

In WSL, re-run Task 8's bats script and `bats_check.py`. `M3` being delivered must not flag any `bats.txt` row.

- [ ] **Step 7: Run every check on both OSes.**
  - Windows:
    - fmt, clippy, `cargo test --workspace`;
    - the parity unit tests, coverage and the diff;
    - the full overlay run;
    - `python ci/shim_deps.py`.
  - WSL: fmt, clippy, `cargo test --workspace`, the parity unit tests, coverage, the diff, and bats.

- [ ] **Step 8: Commit.**

```bash
git add parity docs/parity/allowlist.md
git commit -m "Run pyenv-win's shell tests against rpyenv's integration and deliver M3"
```

---

## Self-review

**Spec coverage (§6 Shell row, §7, §14 item 3):**

| Spec requirement | Task |
|---|---|
| `init - <shell>` prints profile code: shims on PATH, `PYENV_SHELL`, a `pyenv` function, completions | 3 (Linux), 7 (Windows) |
| The function routes `shell`, `activate`, `deactivate` through `sh-<cmd>` and evaluates the result | 3, 7. `activate` and `deactivate` arrive with M4's virtualenv commands, which join the routed set through the command table (Decision 4) |
| bash, zsh, Git Bash (`/c/...` paths), fish, PowerShell 5.1 and 7 profile lines | 3, 7 (Decision 8 amends "when `MSYSTEM` is set") |
| No `pyenv.ps1` or `pyenv.cmd` for the CLI | 7 (the profile function); 9 removes pyenv-win's from the test roots |
| Without integration: shell detection, print the command, note on stderr, exit 1, all shells when unknown | 6 (Decision 1 amends the order) |
| `pyenv init` with no arguments prints setup instructions using the same detection | 3 (Linux parent command line), 7 (Windows parent process) |
| `completions` and `sh-*` commands (§6) | 2, 5, 6 |
| Parity: upstream suites and differential cases grow with the milestone (§12, §14) | 8, 9 |

**Placeholder scan:** none intended. Two steps tell the implementer to confirm an existing signature before calling it: `find_first` in Task 4, and `run_args` in pyenv-win's conftest in Task 9. Both name the file to read and the expected form.

**Type consistency:**
- `names(Flavor, Listing)` replaces `names(Flavor)` in Task 2. Its one caller, `misc::commands`, is updated there; `init.rs` and `init_win.rs` call the new form.
- `Mode` gains `Install` in Task 4. `init_win` (Task 7) matches all five variants.
- `shell_win::lf` (Task 6) is the one `\n` writer `init_win` reuses.

**Review Focus:** each of the five has its test, named in the list at the top, in the task that owns the code.

**Known gaps, by design:**
- zsh, fish and ksh are not *run* by any test, since WSL and CI have neither installed. Their output is fixed by upstream's tests and by string tests here (M3L "Open points").
- The four completion scripts are upstream's, unchanged; only the bash one is executed (Task 5).
- Windows `init --install` waits for M6 (Decision 2).
