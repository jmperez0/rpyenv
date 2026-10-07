# M5b Live Rehash Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With `RPYENV_LIVE_REHASH=1`, a long-running program started through a shim (Jupyter's `%pip install`) gets new scripts' shims while it runs. An interrupted rehash never leaves its lock behind.

**Architecture:**
- **The watcher runs `rehash::check` whenever the watched folders change**, after a quiet period. `check` already compares the stored state with the disk and rehashes under the lock.
- **Windows:** a thread in the shim watches the `versions` tree through one `ReadDirectoryChangesW` handle, opened with share-delete.
- **Linux:** the shim double-forks a detached watcher before `execv`. The watcher holds a pidfd on the program and watches the stored-state folders with inotify.
- **The lock cleans up after itself:** a signal handler (Linux) or a console handler (Windows, for `pyenv rehash` and the commands that run it) removes it.

**Tech Stack:** Rust 1.96, `windows-sys` 0.61, `libc` 0.2 (`fork`, `setsid`, `pidfd_open` through `syscall`, inotify, `poll`, `sigaction`).

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`, §8 "When it runs", point 3 (the live watcher) and "Safety" (the lock). Memory item: "remove the rehash lock on SIGINT/SIGTERM".

## Global Constraints

- **Spec §8 point 3, verbatim intent:**
  - opt-in with `RPYENV_LIVE_REHASH=1`;
  - Windows: a thread in the shim, started only if the child is still running after about 1 s, with directory handles opened with share-delete so they never block `pyenv uninstall`;
  - Linux: the shim forks, the child forks the watcher and exits at once, the shim reaps that child and then runs `execv`; the watcher moves to a new session (`setsid`), closes every inherited fd, reopens stdin/stdout/stderr on `/dev/null`, waits about 1 s via `pidfd_open`, and if the program is still running watches with inotify until it exits, then runs the final stored-state check;
  - skipped when the shim is PID 1;
  - both: once changes stop for about 0.5 s, one rehash runs; if the watcher can't start, it's skipped silently, and the exit check still runs.
- **Spec §8 Safety:** one rehash at a time through `shims/.rehash.lock`. A shim that can't get the lock skips.
- **The watcher runs in shims only** (spec: "a thread in the shim"), both the console and the GUI shim. `pyenv exec` doesn't start one.
- **Never change a signal the caller ignored** (`nohup`). The lock's handler is installed only for signals that aren't `SIG_IGN` on entry, as `sig::spawn_and_wait` does.
- **Running tests:**
  - Windows: `cargo build --workspace`, then `cargo test --workspace`.
  - Linux: `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m5a_wsl.sh <cmd>` (rename it `m5b_wsl.sh` with branch `m5b-live-rehash`). Never put `$VARS` in `wsl … bash -c`.
  - Every test uses `Fixture` (temp roots).
- **Commits:** a one-line `-m`, or `-F C:\tmp\cm_<random>.txt`. No attribution lines.

## Review Focus

These are the five input classes or failure modes most likely to bite a user, none exercised by a task's main tests. Each has a pinning test in its owning task.

1. **The watcher holding the caller's pipe open (Linux).** `out=$(python -c 1)` must return when the program does. Expected: the watcher's fds 0–2 are `/dev/null`, and it is in its own session. Pinned in Task 4 (`live_watcher_is_detached`).
2. **A rehash interrupted under `nohup`** (SIGHUP ignored). Expected: the rehash survives SIGHUP and finishes; the lock's handler didn't override the ignore. Pinned in Task 1 (`rehash_keeps_an_ignored_sighup`).
3. **Renaming or uninstalling a version while a Windows watcher runs.** Expected: it works (share-delete). Pinned in Task 3 (`win_live_watcher_lets_a_version_be_renamed`).
4. **The watcher appearing as the program's child (Linux).** `os.wait()` or psutil would see it. Expected: the program has no children. Pinned in Task 4 (`live_watcher_is_not_a_child_of_the_program`).
5. **A program that ends within the start delay** (almost every command). Expected: no watching, no delay. Pinned in Task 3 (Windows) and Task 4 (Linux): the log has no `live=watching`.

## Decisions (rulings made while planning)

- **R1, Windows watches the whole `versions` tree** (`bWatchSubtree=TRUE`) through one handle on `versions/`, not one handle per scripts folder. Subfolders hold no handle, so deleting or renaming them always works. pip's churn in `site-packages` only re-arms the 0.5 s debounce, and `check` is cheap (folder metadata).
- **R2, the watcher reuses `rehash::check`**, the exit check, rather than its own diffing. On Linux, the inotify set is re-derived from `rehash::state_dirs` after every check, so new versions and envs get watched.
- **R3, the lock's interruption cleanup.**
  - **Linux:** every lock holder gets it, for SIGINT, SIGTERM and SIGHUP. The handler unlinks the lock (async-signal-safe), restores the default, and re-raises.
  - **Windows:** only `rehash::rehash` callers (`pyenv rehash`, install, uninstall, virtualenv) get it, because they die on Ctrl+C. A shim's exit check or watcher ignores Ctrl+C (`on_console_event`), so removing its lock there would let a second rehash start alongside a live one. The handler deletes the lock and returns FALSE.
- **R4, a test hook for holding the lock:** `RPYENV_TEST_HOLD_REHASH_MS`, read only in debug builds (`cfg(debug_assertions)`), sleeps inside `rehash_locked`. Release builds don't have it.
- **R5, the pidfd is opened by the first child, while the shim is alive and waiting for it,** so the PID can't be reused before the watcher holds it. Without `pidfd_open` (kernel < 5.3), the watcher is skipped silently.

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/rpyenv-core/src/rehash.rs` | modify | `state_dirs` (split out of `snapshot`), lock cleanup on interruption, the debug hold hook |
| `crates/rpyenv-core/src/livewatch.rs` | create | `should_start`, `Debounce`, `start` (dispatch), constants (pure parts tested on every OS) |
| `crates/rpyenv-core/src/livewatch_win.rs` | create (Windows) | the watcher thread |
| `crates/rpyenv-core/src/livewatch_linux.rs` | create (Linux) | double fork, detach, pidfd + inotify loop |
| `crates/rpyenv-core/src/shim.rs` | modify | start the watcher before `launch::run` |
| `crates/rpyenv-core/src/lib.rs` | modify | modules |
| `crates/e2e/src/echo.rs`, `bin/argv_echo.rs` | modify | `ARGV_ECHO_CHILDREN` |
| `crates/e2e/Cargo.toml` | modify | `libc` dev-dependency on unix |
| `crates/e2e/tests/shims.rs`, `windows.rs` | modify | tests |
| spec §8, `README.md` | modify | as built |

---

### Task 1: The lock cleans up after an interruption

**Files:**
- Modify: `crates/rpyenv-core/src/rehash.rs`
- Modify: `crates/e2e/Cargo.toml` (`[target.'cfg(unix)'.dev-dependencies] libc = "0.2"`)
- Test: `crates/e2e/tests/shims.rs` (Linux), `crates/e2e/tests/windows.rs`

**Interfaces:**
- Produces:
  - `Lock` with a private `armed_ctrl: bool`;
  - the debug-only `RPYENV_TEST_HOLD_REHASH_MS`.
- No public signature changes: `lock()` arms the Unix handler itself, and `rehash()` arms the Windows one.

- [ ] **Step 1: Write the failing Linux tests.** In `crates/e2e/tests/shims.rs`:

```rust
/// Polls until `path` exists (20 s at most).
#[cfg(unix)]
fn wait_until_exists(path: &std::path::Path) {
    let start = std::time::Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(20), "{} never appeared", path.display());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Spec §8 Safety (M5b): a rehash ended by SIGINT, SIGTERM or SIGHUP removes its lock
/// instead of leaving it for two minutes, and dies by that signal.
#[cfg(target_os = "linux")]
#[test]
fn an_interrupted_rehash_removes_its_lock() {
    use std::os::unix::process::ExitStatusExt;
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let f = Fixture::new();
        f.install("3.12.10/bin/python");
        let lock = f.root.join("shims").join(".rehash.lock");
        let mut child = f
            .command(&built("pyenv"), &[("RPYENV_TEST_HOLD_REHASH_MS", v("20000"))])
            .arg("rehash")
            .spawn()
            .unwrap();
        wait_until_exists(&lock);
        // SAFETY: sends a signal to the child this test started.
        unsafe { libc::kill(child.id() as libc::pid_t, signal) };
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(signal), "signal {signal}");
        assert!(!lock.exists(), "signal {signal} left the lock behind");
    }
}

/// Review focus 2: under `nohup` (SIGHUP ignored) the rehash survives SIGHUP and finishes.
#[cfg(target_os = "linux")]
#[test]
fn rehash_keeps_an_ignored_sighup() {
    use std::os::unix::process::CommandExt;
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    let lock = f.root.join("shims").join(".rehash.lock");
    let mut cmd = f.command(&built("pyenv"), &[("RPYENV_TEST_HOLD_REHASH_MS", v("1500"))]);
    cmd.arg("rehash");
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    wait_until_exists(&lock);
    // SAFETY: sends a signal to the child this test started.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGHUP) };
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert!(f.shim("python").exists());
    assert!(!lock.exists());
}
```

Add to `crates/e2e/Cargo.toml`:

```toml
[target.'cfg(unix)'.dev-dependencies]
libc = "0.2"
```

- [ ] **Step 2: Write the failing Windows test.** In `crates/e2e/tests/windows.rs`, next to the Ctrl+Break test:

```rust
/// Spec §8 Safety (M5b): `pyenv rehash` ended by Ctrl+Break (or Ctrl+C, or closing its
/// window) removes its lock instead of leaving it for two minutes.
#[test]
fn win_an_interrupted_rehash_removes_its_lock() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let lock = f.root.join("shims").join(".rehash.lock");
    let mut child = KillOnDrop(
        f.command(&built("pyenv"), &[("RPYENV_TEST_HOLD_REHASH_MS", v("20000"))])
            .arg("rehash")
            .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(&lock);
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_BREAK_PID", child.0.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+Break");
    let _ = child.0.wait();
    assert!(!lock.exists(), "Ctrl+Break left the lock behind");
}
```

- [ ] **Step 3: Run them to verify they fail**

Run on Windows: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows -- win_an_interrupted_rehash`.
Expected: FAIL. The lock never appears (the hook doesn't exist yet), so `wait_for` times out after 20 s.

Run on Linux (WSL): `cargo test -p rpyenv-e2e --test shims -- an_interrupted rehash_keeps`.
Expected: both FAIL with "never appeared".

- [ ] **Step 4: Add the hold hook only, and run again.** In `rehash_locked`, first line:

```rust
    // Test hook (debug builds only): hold the lock this long, so tests can interrupt a
    // rehash that is mid-way (plan M5b, R4).
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("RPYENV_TEST_HOLD_REHASH_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        std::thread::sleep(Duration::from_millis(ms));
    }
```

Run the same commands.
Expected:
- Linux `an_interrupted_rehash_removes_its_lock` FAILS: "left the lock behind";
- `rehash_keeps_an_ignored_sighup` PASSES, because SIGHUP is ignored and nothing overrides that yet. Note it in the ledger: it pins that the next step doesn't break `nohup`;
- Windows FAILS: "Ctrl+Break left the lock behind".

- [ ] **Step 5: Implement the cleanup.** In `rehash.rs`, replace `Lock` and its `Drop`:

```rust
/// Held while a rehash runs. Dropping it removes the lock file, on failure too. An
/// interruption removes it as well (plan M5b, R3): on Linux, SIGINT, SIGTERM or SIGHUP
/// (unless ignored on entry); on Windows, Ctrl+C, Ctrl+Break or a closed window, for the
/// callers of `rehash` (which `arm_ctrl` arms).
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    armed_ctrl: bool,
}

impl Lock {
    fn new(path: PathBuf) -> Lock {
        #[cfg(unix)]
        cleanup::arm(&path);
        Lock {
            path,
            armed_ctrl: false,
        }
    }

    /// Windows: removes the lock if a console event ends this process (`pyenv rehash` and
    /// the commands that run it, which Ctrl+C ends).
    #[cfg(windows)]
    fn arm_ctrl(&mut self) {
        cleanup::arm(&self.path);
        self.armed_ctrl = true;
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        #[cfg(unix)]
        cleanup::disarm();
        #[cfg(windows)]
        if self.armed_ctrl {
            cleanup::disarm();
        }
        let _ = self.armed_ctrl;
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
mod cleanup {
    //! The lock's path for a signal handler: SIGINT, SIGTERM and SIGHUP unlink it, then end
    //! the process by the same signal, as if no handler were there.
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::Mutex;

    const SIGNALS: [libc::c_int; 3] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP];
    static PATH: AtomicPtr<libc::c_char> = AtomicPtr::new(std::ptr::null_mut());
    static OLD: Mutex<Option<[libc::sigaction; 3]>> = Mutex::new(None);

    extern "C" fn on_signal(signal: libc::c_int) {
        let p = PATH.swap(std::ptr::null_mut(), Ordering::SeqCst);
        // SAFETY: unlink, sigaction and raise are async-signal-safe; `p` is a CString leaked
        // by `arm` and never freed while stored.
        unsafe {
            if !p.is_null() {
                libc::unlink(p);
            }
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut act.sa_mask);
            libc::sigaction(signal, &act, std::ptr::null_mut());
            libc::raise(signal);
        }
    }

    pub fn arm(path: &Path) {
        let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        let old_path = PATH.swap(c.into_raw(), Ordering::SeqCst);
        if !old_path.is_null() {
            // SAFETY: a CString leaked by an earlier `arm`, now unreachable from the handler.
            drop(unsafe { CString::from_raw(old_path) });
        }
        // SAFETY: reads and sets dispositions with zeroed, then filled, sigaction values.
        unsafe {
            let mut old: [libc::sigaction; 3] = std::mem::zeroed();
            for (i, &s) in SIGNALS.iter().enumerate() {
                libc::sigaction(s, std::ptr::null(), &mut old[i]);
                if old[i].sa_sigaction == libc::SIG_IGN {
                    continue;
                }
                let mut act: libc::sigaction = std::mem::zeroed();
                act.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
                libc::sigemptyset(&mut act.sa_mask);
                libc::sigaction(s, &act, std::ptr::null_mut());
            }
            *OLD.lock().unwrap_or_else(|e| e.into_inner()) = Some(old);
        }
    }

    pub fn disarm() {
        if let Some(old) = OLD.lock().unwrap_or_else(|e| e.into_inner()).take() {
            for (i, &s) in SIGNALS.iter().enumerate() {
                if old[i].sa_sigaction != libc::SIG_IGN {
                    // SAFETY: restores the disposition read in `arm`.
                    unsafe { libc::sigaction(s, &old[i], std::ptr::null_mut()) };
                }
            }
        }
        let p = PATH.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !p.is_null() {
            // SAFETY: a CString leaked by `arm`, no longer reachable from the handler.
            drop(unsafe { CString::from_raw(p) });
        }
    }
}

#[cfg(windows)]
mod cleanup {
    //! The lock's path for a console handler: Ctrl+C, Ctrl+Break and a closed window delete
    //! it, then let the next handler (the default one ends the process) run.
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

    unsafe extern "system" fn on_event(_event: u32) -> BOOL {
        if let Ok(mut p) = PATH.try_lock() {
            if let Some(p) = p.take() {
                let _ = std::fs::remove_file(p);
            }
        }
        0
    }

    pub fn arm(path: &Path) {
        *PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.to_path_buf());
        // SAFETY: registers a handler that only touches `PATH` and the file system.
        unsafe { SetConsoleCtrlHandler(Some(on_event), 1) };
    }

    pub fn disarm() {
        // SAFETY: removes the handler `arm` registered.
        unsafe { SetConsoleCtrlHandler(Some(on_event), 0) };
        PATH.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
```

In `lock()`, replace `Ok(_) => return Ok(Lock { path }),` with `Ok(_) => return Ok(Lock::new(path)),`. In `rehash()`:

```rust
pub fn rehash(ctx: &Ctx, shim_exe: &Path, wait: Wait) -> Result<RehashStats, RehashError> {
    #[allow(unused_mut)]
    let mut lock = lock(&ctx.shims_dir(), wait)?;
    #[cfg(windows)]
    lock.arm_ctrl();
    let stats = rehash_locked(ctx, shim_exe, Caller::Command);
    drop(lock);
    stats
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run the Step 3 commands, then `cargo test -p rpyenv-core --lib rehash::`.
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/rpyenv-core/src/rehash.rs crates/e2e
git commit -m "An interrupted rehash removes its lock: SIGINT, SIGTERM and SIGHUP on Linux (unless ignored), Ctrl+C, Ctrl+Break and closing the window for pyenv rehash on Windows"
```

---

### Task 2: The watcher's pure parts and `rehash::state_dirs`

**Files:**
- Create: `crates/rpyenv-core/src/livewatch.rs`
- Modify: `crates/rpyenv-core/src/rehash.rs` (`state_dirs`), `crates/rpyenv-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `rehash::state_dirs(&Ctx) -> Vec<PathBuf>`;
  - `livewatch::{START_DELAY, QUIET}`;
  - `livewatch::should_start(own_pid: u32, setting: Option<&str>) -> bool`;
  - `livewatch::Debounce { changed(&mut self, Instant), wait(&self, Instant) -> Option<Duration>, fire(&mut self, Instant) -> bool }`;
  - `livewatch::Guard` and `livewatch::start(&Ctx, Option<&Path>) -> Guard`, a no-op until Tasks 3 and 4.

- [ ] **Step 1: Write the failing tests.** Create `crates/rpyenv-core/src/livewatch.rs` with only:

```rust
//! The live rehash watcher (spec §8 point 3, `RPYENV_LIVE_REHASH=1`): while a long-running
//! program runs (Jupyter's `%pip install`), a new script gets its shim without waiting for
//! the program to exit.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn it_starts_only_when_asked_and_never_as_pid_1() {
        assert!(should_start(4321, Some("1")));
        assert!(!should_start(4321, None));
        assert!(!should_start(4321, Some("0")));
        assert!(!should_start(4321, Some("yes")));
        assert!(!should_start(1, Some("1")), "PID 1 (a container without init)");
    }

    #[test]
    fn one_rehash_once_changes_stop_for_the_quiet_period() {
        let t0 = Instant::now();
        let mut d = Debounce::default();
        assert_eq!(d.wait(t0), None);
        assert!(!d.fire(t0));
        d.changed(t0);
        assert_eq!(d.wait(t0), Some(QUIET));
        assert!(!d.fire(t0 + QUIET - Duration::from_millis(1)));
        // A later change moves the deadline.
        d.changed(t0 + Duration::from_millis(300));
        assert!(!d.fire(t0 + QUIET));
        assert!(d.fire(t0 + Duration::from_millis(300) + QUIET));
        assert!(!d.fire(t0 + Duration::from_secs(5)), "fires once");
        assert_eq!(d.wait(t0 + Duration::from_secs(5)), None);
    }
}
```

In `rehash.rs` tests, add:

```rust
    #[test]
    fn state_dirs_are_the_folders_the_snapshot_records() {
        let (_tmp, ctx, _) = setup(Flavor::Pyenv);
        fs::create_dir_all(ctx.versions_dir().join("3.12.1").join("bin")).unwrap();
        let dirs = state_dirs(&ctx);
        assert_eq!(dirs[0], ctx.versions_dir());
        assert!(dirs.contains(&ctx.versions_dir().join("3.12.1").join("bin")));
        assert!(snapshot(&ctx).contains("3.12.1/bin"));
    }
```

Add `pub mod livewatch;` to `lib.rs`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rpyenv-core --lib livewatch:: rehash::tests::state_dirs`
Expected: compile errors (`should_start`, `Debounce`, `QUIET`, `state_dirs` not found).

- [ ] **Step 3: Implement.** In `rehash.rs`, split `snapshot`:

```rust
/// The folders whose times and entry counts the stored state records (spec §8): the ones
/// a new or removed script changes.
pub fn state_dirs(ctx: &Ctx) -> Vec<PathBuf> {
    let vdir = ctx.versions_dir();
    let mut dirs = vec![vdir.clone()];
    for entry in installed::top_level(&vdir, ctx.flavor) {
        match ctx.flavor {
            Flavor::Pyenv => {
                dirs.push(entry.path.join("bin"));
                dirs.push(entry.path.join("envs"));
                dirs.extend(
                    installed::envs_of(&entry)
                        .into_iter()
                        .map(|e| e.path.join("bin")),
                );
            }
            Flavor::PyenvWin => {
                dirs.push(entry.path.join("Scripts"));
                dirs.push(entry.path.join("bin"));
                dirs.push(entry.path);
            }
        }
    }
    dirs
}

/// The watched folders' modification times and entry counts, one line each.
pub fn snapshot(ctx: &Ctx) -> String {
    let vdir = ctx.versions_dir();
    let mut s = String::from("rpyenv rehash state 2\n");
    for d in state_dirs(ctx) {
```

The rest of `snapshot`'s loop body is unchanged.

In `livewatch.rs`, above the tests:

```rust
use crate::ctx::Ctx;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long the watcher waits before it starts: most commands finish sooner and don't need
/// it (spec §8).
pub const START_DELAY: Duration = Duration::from_secs(1);
/// Changes must stop for this long before one rehash runs (spec §8).
pub const QUIET: Duration = Duration::from_millis(500);

/// Whether this shim starts a watcher: `RPYENV_LIVE_REHASH` is exactly `1`, and this
/// process isn't PID 1. Orphans go back to PID 1, which would be the program itself
/// (spec §8).
pub fn should_start(own_pid: u32, setting: Option<&str>) -> bool {
    setting == Some("1") && own_pid != 1
}

/// The one rehash that follows a burst of changes: each change moves the deadline to
/// `QUIET` after it, and the rehash is due once the deadline passes.
#[derive(Debug, Default, Clone, Copy)]
pub struct Debounce {
    due: Option<Instant>,
}

impl Debounce {
    pub fn changed(&mut self, now: Instant) {
        self.due = Some(now + QUIET);
    }

    /// How long to wait for more changes; `None` when no rehash is pending.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.due.map(|d| d.saturating_duration_since(now))
    }

    /// True, once, when the pending rehash is due.
    pub fn fire(&mut self, now: Instant) -> bool {
        match self.due {
            Some(d) if now >= d => {
                self.due = None;
                true
            }
            _ => false,
        }
    }
}

/// Keeps a Windows watcher thread running until it's dropped (after the program and the
/// exit check). Nothing on Linux, where the watcher is a detached process.
#[derive(Default)]
pub struct Guard {
    #[cfg(windows)]
    running: Option<crate::livewatch_win::Running>,
}

/// Starts the watcher for this shim when `RPYENV_LIVE_REHASH=1` (spec §8 point 3). Any
/// failure skips it silently; the exit check still runs.
pub fn start(ctx: &Ctx, shim_exe: Option<&Path>) -> Guard {
    let setting = std::env::var("RPYENV_LIVE_REHASH").ok();
    let Some(_exe) = shim_exe else {
        return Guard::default();
    };
    if !should_start(std::process::id(), setting.as_deref()) {
        if setting.as_deref() == Some("1") {
            crate::debuglog::append("live=skipped pid1");
        }
        return Guard::default();
    }
    let _ = ctx;
    Guard::default()
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rpyenv-core --lib livewatch:: rehash::`
Expected: PASS. Then run the same through WSL: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core/src
git commit -m "Live rehash: the start rule, the quiet-period debounce, and the folders the stored state records"
```

---

### Task 3: The Windows watcher thread

**Files:**
- Create: `crates/rpyenv-core/src/livewatch_win.rs`
- Modify: `crates/rpyenv-core/src/livewatch.rs` (`start` on Windows), `crates/rpyenv-core/src/shim.rs`, `crates/rpyenv-core/src/lib.rs`
- Test: `crates/e2e/tests/windows.rs`

**Interfaces:**
- Consumes: Task 2's `Debounce`, `START_DELAY`, `Guard`, `start`, and `rehash::check(&Ctx, &Path) -> bool`.
- Produces:
  - `livewatch_win::Running` (Drop: stop and join);
  - `livewatch_win::spawn(&Ctx, &Path) -> Option<Running>`;
  - log lines `live=watching pid=<pid>` and `live=check`.

- [ ] **Step 1: Write the failing e2e tests:**

```rust
/// Spec §8 point 3: with `RPYENV_LIVE_REHASH=1`, a script installed while the program runs
/// gets its shim before the program exits.
#[test]
fn win_live_watcher_makes_a_new_scripts_shim_while_the_program_runs() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let mut shim = KillOnDrop(
        f.shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("8000")),
            ],
        )
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for(&ready);
    wait_log(&log, "live=watching");
    f.install("3.9.1/Scripts/black.exe");
    let start = std::time::Instant::now();
    while !f.shim("black").exists() {
        assert!(shim.0.try_wait().unwrap().is_none(), "the program ended first");
        assert!(start.elapsed() < Duration::from_secs(5), "no shim while the program ran");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Without the variable nothing changes: no shim until the exit check.
#[test]
fn win_no_live_watcher_without_the_variable() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let ready = f.base.join("ready");
    let mut shim = KillOnDrop(
        f.shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("5000")),
            ],
        )
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for(&ready);
    std::thread::sleep(Duration::from_millis(1500));
    f.install("3.9.1/Scripts/black.exe");
    std::thread::sleep(Duration::from_millis(2000));
    assert!(shim.0.try_wait().unwrap().is_none());
    assert!(!f.shim("black").exists(), "a shim appeared without the variable");
    let _ = shim.0.wait();
    assert!(f.shim("black").exists(), "the exit check still rehashes");
}

/// Review focus 3: the watcher never blocks renaming or uninstalling a version.
#[test]
fn win_live_watcher_lets_a_version_be_renamed() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.install("3.8.0/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let mut shim = KillOnDrop(
        f.shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("6000")),
            ],
        )
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for(&ready);
    wait_log(&log, "live=watching");
    let versions = f.root.join("versions");
    std::fs::rename(versions.join("3.8.0"), versions.join("3.8.0-old")).expect("rename while watched");
    std::fs::remove_dir_all(versions.join("3.8.0-old")).expect("delete while watched");
    let _ = shim.0.wait();
}

/// Review focus 5: a program that ends within the start delay is never watched.
#[test]
fn win_live_watcher_skips_a_quick_program() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
            ],
        )
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!text.contains("live=watching"), "{text}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows -- win_live win_no_live`.
Expected:
- FAIL: `…makes_a_new_scripts_shim…` and `…renamed`, with `no "live=watching"`;
- PASS: `win_no_live_watcher_without_the_variable` and `…skips_a_quick_program`. They pin today's behavior; note that in the ledger.

- [ ] **Step 3: Implement.** Create `crates/rpyenv-core/src/livewatch_win.rs`:

```rust
//! The Windows live watcher (spec §8 point 3): a thread in the shim. After `START_DELAY`, if
//! the program still runs, it watches the whole `versions` tree through one handle
//! (`ReadDirectoryChangesW`, subtree, share-delete: plan M5b, R1). Once changes stop for
//! `QUIET`, it runs the stored-state check.

use crate::ctx::Ctx;
use crate::debuglog;
use crate::livewatch::{Debounce, START_DELAY};
use crate::rehash;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Instant;
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects, WaitForSingleObject, INFINITE,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// A running watcher: dropping it stops the thread and waits for it.
pub struct Running {
    stop: usize,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // SAFETY: the stop event this module created; set, then closed once the thread
        // that waits on it has ended.
        unsafe { SetEvent(self.stop as HANDLE) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe { CloseHandle(self.stop as HANDLE) };
    }
}

/// Starts the watcher thread, or `None` when it can't (silently, spec §8).
pub fn spawn(ctx: &Ctx, shim_exe: &Path) -> Option<Running> {
    // SAFETY: a manual-reset event, initially unset, unnamed.
    let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if stop.is_null() {
        return None;
    }
    let (ctx, exe, stop_id) = (ctx.clone(), shim_exe.to_path_buf(), stop as usize);
    let thread = std::thread::spawn(move || watch(&ctx, &exe, stop_id as HANDLE));
    Some(Running {
        stop: stop as usize,
        thread: Some(thread),
    })
}

fn watch(ctx: &Ctx, exe: &Path, stop: HANDLE) {
    // SAFETY: waits on the stop event, owned by `Running`, which outlives this thread.
    if unsafe { WaitForSingleObject(stop, START_DELAY.as_millis() as u32) } == WAIT_OBJECT_0 {
        return;
    }
    let versions: PathBuf = ctx.versions_dir();
    let wide: Vec<u16> = versions.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated path; the handle is closed below on every path.
    let dir = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if dir == INVALID_HANDLE_VALUE {
        return;
    }
    // SAFETY: a manual-reset event, initially unset, unnamed; closed below.
    let changed = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if changed.is_null() {
        unsafe { CloseHandle(dir) };
        return;
    }
    debuglog::append(&format!("live=watching pid={}", std::process::id()));
    // `u32` elements: the notification buffer must be DWORD-aligned.
    let mut buf = vec![0u32; 16 * 1024];
    let mut debounce = Debounce::default();
    'outer: loop {
        // SAFETY: a zeroed OVERLAPPED is valid; its event is the one created above.
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = changed;
        // SAFETY: the buffer and OVERLAPPED live until the operation completes or is
        // cancelled and waited for below.
        let armed = unsafe {
            ResetEvent(changed);
            ReadDirectoryChangesW(
                dir,
                buf.as_mut_ptr().cast(),
                (buf.len() * 4) as u32,
                1,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_ATTRIBUTES,
                std::ptr::null_mut(),
                &mut ov,
                None,
            ) != 0
        };
        if !armed {
            break;
        }
        loop {
            let timeout = debounce
                .wait(Instant::now())
                .map(|d| d.as_millis() as u32 + 1)
                .unwrap_or(INFINITE);
            let handles = [stop, changed];
            // SAFETY: waits on two live events.
            let w = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout) };
            if w == WAIT_OBJECT_0 + 1 {
                let mut n = 0u32;
                // SAFETY: the operation has completed; reads its result.
                unsafe { GetOverlappedResult(dir, &ov, &mut n, 0) };
                debounce.changed(Instant::now());
                continue 'outer;
            }
            if w == WAIT_TIMEOUT {
                if debounce.fire(Instant::now()) {
                    rehash::check(ctx, exe);
                    debuglog::append("live=check");
                }
                continue;
            }
            // Stopped (or the wait failed): cancel the read and wait for it to end before
            // the buffer goes away.
            let mut n = 0u32;
            // SAFETY: cancels this thread's own operation on `dir` and waits for it.
            unsafe {
                CancelIoEx(dir, &ov);
                GetOverlappedResult(dir, &ov, &mut n, 1);
            }
            break 'outer;
        }
    }
    // SAFETY: handles created above, closed once.
    unsafe {
        CloseHandle(changed);
        CloseHandle(dir);
    }
}
```

In `livewatch.rs`, replace the end of `start` (`let _ = ctx; Guard::default()`) with:

```rust
    #[cfg(windows)]
    return Guard {
        running: crate::livewatch_win::spawn(ctx, _exe),
    };
    #[cfg(not(windows))]
    {
        let _ = ctx;
        Guard::default()
    }
```

In `lib.rs`, add `#[cfg(windows)] pub mod livewatch_win;`.

In `shim.rs`'s `Ok(plan)` branch, before `match launch::run(...)`:

```rust
            // Dropped after the program and the exit check (spec §8 point 3).
            let _live = crate::livewatch::start(&ctx, rehash_with.as_deref());
```

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows -- win_live win_no_live`, then `cargo test --workspace`.
Expected: 4 PASS, and the suite is green.

- [ ] **Step 5: Commit**

```bash
git add crates
git commit -m "Live rehash on Windows: a watcher thread in the shim watches the versions tree and rehashes once changes stop"
```

---

### Task 4: The Linux watcher process

**Files:**
- Create: `crates/rpyenv-core/src/livewatch_linux.rs`
- Modify: `crates/rpyenv-core/src/livewatch.rs` (`start` on Linux), `crates/rpyenv-core/src/lib.rs`
- Modify: `crates/e2e/src/echo.rs`, `crates/e2e/src/bin/argv_echo.rs` (`ARGV_ECHO_CHILDREN=1` prints `children=<n>`)
- Test: `crates/e2e/tests/shims.rs`

**Interfaces:**
- Consumes: Task 2's `Debounce`, `START_DELAY`, `rehash::state_dirs`, `rehash::check`.
- Produces: `livewatch_linux::spawn(&Ctx, &Path)`, and log lines `live=watching pid=<watcher pid>` and `live=check`.

- [ ] **Step 1: Add the argv-echo knob.** In `echo.rs`, after the `ARGV_ECHO_SIGIGN` block:

```rust
    #[cfg(target_os = "linux")]
    if var("ARGV_ECHO_CHILDREN").as_deref() == Some("1") {
        let mut n = 0;
        if let Ok(tasks) = std::fs::read_dir("/proc/self/task") {
            for t in tasks.flatten() {
                n += std::fs::read_to_string(t.path().join("children"))
                    .map(|s| s.split_whitespace().count())
                    .unwrap_or(0);
            }
        }
        out.push_str(&format!("children={n}\n"));
    }
```

Document it in the `argv_echo.rs` header, next to `ARGV_ECHO_SIGIGN`.

- [ ] **Step 2: Write the failing e2e tests** in `shims.rs`:

```rust
/// Spec §8 point 3 on Linux: a script installed while the program runs gets its shim
/// before the program exits.
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_makes_a_new_scripts_shim_while_the_program_runs() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let mut child = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("8000")),
            ],
        )
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_until_exists(&ready);
    wait_log(&log, "live=watching");
    f.install("3.12.10/bin/black");
    let start = std::time::Instant::now();
    while !f.shim("black").exists() {
        assert!(child.try_wait().unwrap().is_none(), "the program ended first");
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "no shim while the program ran");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Review focus 4: the watcher isn't the program's child (spec §8: "Why two forks").
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_is_not_a_child_of_the_program() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.12.10")),
            ("RPYENV_LIVE_REHASH", v("1")),
            ("ARGV_ECHO_CHILDREN", v("1")),
        ],
    );
    assert!(stdout(&out).contains("children=0\n"), "{}", stdout(&out));
}

/// Review focus 1: the watcher holds none of the caller's descriptors (its stdin, stdout and
/// stderr are /dev/null) and is in a session of its own.
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_is_detached() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let mut child = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("RPYENV_LIVE_REHASH", v("1")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("6000")),
            ],
        )
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_until_exists(&ready);
    let text = wait_log(&log, "live=watching");
    let pid: u32 = text
        .split_whitespace()
        .find_map(|w| w.strip_prefix("pid="))
        .and_then(|p| p.parse().ok())
        .expect("watcher pid");
    for fd in 0..3 {
        let target = std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).unwrap();
        assert_eq!(target, std::path::Path::new("/dev/null"), "fd {fd}");
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap().split_whitespace().collect();
    assert_eq!(fields[3], pid.to_string(), "session id (setsid)");
    let _ = child.kill();
    let _ = child.wait();
}

/// Review focus 5: a program that ends within the start delay is never watched.
#[cfg(target_os = "linux")]
#[test]
fn live_watcher_skips_a_quick_program() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.12.10")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_LIVE_REHASH", v("1")),
        ],
    );
    assert!(out.status.success());
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(!text.contains("live=watching"), "{text}");
}

/// Polls the debug log until it contains `needle` (20 s at most); returns the log.
#[cfg(unix)]
fn wait_log(log: &std::path::Path, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(20), "no {needle:?} in:\n{text}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run (WSL): `cargo test -p rpyenv-e2e --test shims -- live_watcher`.
Expected:
- FAIL: `…makes_a_new_scripts_shim…` and `…is_detached`, with `no "live=watching"`;
- PASS: `…is_not_a_child…` and `…skips_a_quick_program`. They pin today's behavior.

- [ ] **Step 4: Implement.** Create `crates/rpyenv-core/src/livewatch_linux.rs`:

```rust
//! The Linux live watcher (spec §8 point 3). The shim forks; the child opens a pidfd on the
//! shim (alive, waiting for it: plan M5b, R5), forks the watcher and exits; the shim reaps
//! it and then runs `execv`, so the watcher is never the program's child. The watcher moves
//! to a new session, puts stdin, stdout and stderr on /dev/null, keeps only the pidfd, and
//! after `START_DELAY` watches the stored-state folders with inotify until the program
//! exits; then it runs the final stored-state check.

use crate::ctx::Ctx;
use crate::debuglog;
use crate::livewatch::{Debounce, START_DELAY};
use crate::rehash;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

/// Forks the watcher. Every failure skips it silently (spec §8).
pub fn spawn(ctx: &Ctx, shim_exe: &Path) {
    // SAFETY: getpid has no preconditions. The shim runs no other threads here, so the
    // forked processes may run ordinary Rust code.
    let shim = unsafe { libc::getpid() };
    let first = unsafe { libc::fork() };
    if first < 0 {
        return;
    }
    if first == 0 {
        // SAFETY: the pidfd syscall on a live process (the shim waits for this one).
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, shim, 0) } as libc::c_int;
        if pidfd >= 0 && unsafe { libc::fork() } == 0 {
            watcher(ctx, shim_exe, pidfd);
        }
        // SAFETY: ends the first child at once, without running exit handlers.
        unsafe { libc::_exit(0) };
    }
    let mut status = 0;
    // SAFETY: reaps the first child, which exits at once.
    unsafe { libc::waitpid(first, &mut status, 0) };
}

fn watcher(ctx: &Ctx, shim_exe: &Path, pidfd: libc::c_int) -> ! {
    // SAFETY: plain syscalls on this process's own session and descriptors.
    unsafe {
        libc::setsid();
        if pidfd != 3 {
            libc::dup2(pidfd, 3);
            libc::close(pidfd);
        }
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null >= 0 {
            for fd in 0..3 {
                libc::dup2(null, fd);
            }
        }
        close_from(4);
    }
    run(ctx, shim_exe, 3);
    // SAFETY: ends the watcher without running the shim's exit handlers.
    unsafe { libc::_exit(0) }
}

/// Closes every descriptor from `low` up.
unsafe fn close_from(low: libc::c_int) {
    if libc::syscall(libc::SYS_close_range, low as libc::c_uint, libc::c_uint::MAX, 0) == 0 {
        return;
    }
    // Kernels before 5.9: close what /proc lists.
    let fds: Vec<libc::c_int> = std::fs::read_dir("/proc/self/fd")
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok()))
                .collect()
        })
        .unwrap_or_default();
    for fd in fds.into_iter().filter(|&fd| fd >= low) {
        libc::close(fd);
    }
}

fn run(ctx: &Ctx, shim_exe: &Path, pidfd: libc::c_int) {
    let mut pfd = libc::pollfd {
        fd: pidfd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: polls one descriptor this process owns.
    if unsafe { libc::poll(&mut pfd, 1, START_DELAY.as_millis() as libc::c_int) } != 0 {
        return; // ended (or failed) within the start delay
    }
    // SAFETY: creates an inotify instance; closed when the process exits.
    let ino = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
    if ino < 0 {
        return;
    }
    watch_all(ino, ctx);
    debuglog::append(&format!("live=watching pid={}", std::process::id()));
    let mut debounce = Debounce::default();
    let mut buf = [0u8; 4096];
    loop {
        if debounce.fire(Instant::now()) {
            rehash::check(ctx, shim_exe);
            debuglog::append("live=check");
            watch_all(ino, ctx);
        }
        let timeout = debounce
            .wait(Instant::now())
            .map(|d| d.as_millis() as libc::c_int + 1)
            .unwrap_or(-1);
        let mut fds = [
            libc::pollfd { fd: pidfd, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: ino, events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: polls two descriptors this process owns.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[0].revents != 0 {
            break; // the program ended
        }
        if fds[1].revents != 0 {
            // SAFETY: drains the non-blocking inotify descriptor into a local buffer.
            while unsafe { libc::read(ino, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
            debounce.changed(Instant::now());
        }
    }
    // The program ended: the final stored-state check (spec §8).
    rehash::check(ctx, shim_exe);
}

/// Watches every stored-state folder (re-adding a watched one only refreshes it). Folders
/// that can't be watched (gone, or the inotify limit) are skipped.
fn watch_all(ino: libc::c_int, ctx: &Ctx) {
    let mask = libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_ATTRIB
        | libc::IN_ONLYDIR;
    for d in rehash::state_dirs(ctx) {
        if let Ok(c) = CString::new(d.as_os_str().as_bytes()) {
            // SAFETY: a NUL-terminated path; failures are ignored.
            unsafe { libc::inotify_add_watch(ino, c.as_ptr(), mask) };
        }
    }
}
```

In `livewatch.rs` `start`, add the Linux branch before the Windows return:

```rust
    #[cfg(target_os = "linux")]
    crate::livewatch_linux::spawn(ctx, _exe);
```

Add `#[cfg(target_os = "linux")] pub mod livewatch_linux;` to `lib.rs`.

- [ ] **Step 5: Run them to verify they pass**

Run (WSL): `cargo test -p rpyenv-e2e --test shims -- live_watcher`, then `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
Expected: 4 PASS, the suite green, clippy clean. Run the venv bats in podman as M4b did? Not needed: no venv code changed.

- [ ] **Step 6: Commit**

```bash
git add crates
git commit -m "Live rehash on Linux: a double-forked, detached watcher with a pidfd on the program and inotify on the stored-state folders"
```

---

### Task 5: Docs as built

**Files:**
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` §8, `README.md`

- [ ] **Step 1: Spec §8.**
  - Point 3: "Windows: a thread in the shim … watches the whole `versions` tree through one handle opened with share-delete; renaming or deleting a version always works" (R1). "Both: once changes stop for about 0.5 s, the stored-state check runs (the same check as at exit)" (R2).
  - Safety: add "**An interrupted rehash removes its lock.** On Linux, this covers SIGINT, SIGTERM and SIGHUP, except a signal the caller ignored. On Windows it covers Ctrl+C, Ctrl+Break and a closed window for `pyenv rehash` and the commands that run it; a shim's own rehash ignores those events. A lock older than two minutes is still broken."
- [ ] **Step 2: README.** Keep the existing `RPYENV_LIVE_REHASH=1` sentence, and add "(Windows and Linux)".
- [ ] **Step 3: Verify:** `cargo fmt --check` and clippy, on both OSes.
- [ ] **Step 4: Commit:** `git commit -m "Docs for M5b as built: the live watcher and the lock's cleanup"`.
