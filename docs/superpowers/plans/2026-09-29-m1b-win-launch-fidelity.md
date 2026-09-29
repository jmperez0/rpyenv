# M1b-win: Windows Launch Fidelity — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Windows shims behave as if Python were run directly:
- Arguments arrive exactly as typed.
- Ctrl+C reaches Python and the shim waits for it.
- Killing the shim kills its child.
- Exit codes such as `0xC000013A` pass through.
- A shim started without a console doesn't open a window.
- GUI programs get a GUI shim.
- Batch tools listed in `RPYENV_BATCH_FORWARD` can change the caller's environment.

The plan also closes the review items that M1a and M1b carried forward.

**Architecture:**
- The launch code stays in `rpyenv-core`, and the Windows-only process plumbing goes into a new `winproc` module (`#[cfg(windows)]`). Two new portable modules hold pure logic that is unit-tested on both OSes:
  - `wincmd`: the command-line tail.
  - `console`: the console-mode decision.
- The shim still starts its child with `std::process::Command`, handing it the raw command-line tail through `raw_arg`. `winproc` wraps the start with a Job Object, a Ctrl+C handler and console creation flags.
- `pe` reads an executable's subsystem, so rehash can link GUI targets to a new `pyenv-shimw` binary.
- Rehash writes `.cmd` forwarders for the names in `RPYENV_BATCH_FORWARD`.

**Tech Stack:** Rust 1.96 (edition 2021). `rpyenv-core` uses `std`, `libc` on Unix, and `windows-sys` 0.61 on Windows (already in `Cargo.lock` through `tempfile`). `tempfile` for tests.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`: §5.3 (launching on Windows), §8 (rehash: GUI shims, forwarders), §11 (errors), §12 (testing), §13 (`RPYENV_DEBUG_LOG`, `RPYENV_BATCH_FORWARD`). The console rules are in `docs/windows-lazy-console.md`. M1 implements INHERIT, NO-WINDOW, MIRROR and the GUI shim; EAGER and LAZY are M5. Read spec §4, "Parity policy", first.

**Where this fits:** M1a (core and read-only CLI) and M1b (lookup, rehash, `exec`, shims) are on `main`. M1c (parity CI) comes next.

## Decisions this plan makes

1. **The child is still started with `std::process::Command`, and the raw tail goes in through `CommandExt::raw_arg`.** std writes `"<program>" <tail>`: the tail after the shim's own name is passed on unchanged, and nothing is split or re-quoted (spec §5.3). The only change is that the whitespace between the program and the tail becomes one space. That doesn't change any argument the child parses. std also handles the environment block, the standard handles and the exit code.
2. **The child joins the Job Object right after `spawn`**, not while suspended: std doesn't expose the thread handle that a suspended start would need. The child runs for microseconds before it joins. Anything it starts in that window breaks away, and so would anything it starts later (`SILENT_BREAKAWAY_OK`), so nothing is lost.
3. **Batch targets (`.bat`, `.cmd`) get CRT-split arguments, not the raw tail**, so std's batch-file escaping (hardened after CVE-2024-24576) applies (spec §5.3).
4. **The GUI shim shows a message box only when it has nowhere to write.** If the stream the message belongs on (stdout for pyenv-win's messages) is a usable handle, the message is written there. This keeps pipelines and tests from blocking on a dialog (spec §11: "The GUI shim shows a message box").
5. **`RPYENV_DEBUG_LOG`** gets one line per shim decision (`rpyenv-shim: mode=NO-WINDOW program=<path>`), plus every error line the shim prints. The tests read it; users read it when there is no console (spec §11, §13).
6. **Forwarders need the absolute path of `pyenv.exe`** (spec §5.3). `pyenv rehash` records it in `shims\.template\pyenv-path.txt`, so a rehash run by a shim's exit check writes the same forwarders.
7. **Carried forward from M1b and included here:**
   - rehash state paths relative to `versions` (M-3);
   - the exit check re-checks after waiting for the lock;
   - rehash reports what it changed, which fixes a test that couldn't fail (M-5);
   - Linux signal-window fixes, and no core file from the shim;
   - the error reason without Rust's `(os error N)` suffix;
   - `argv[0]` checked against the shim binary;
   - Windows lookup that launches only runnable files (M-4);
   - `exec` names containing `\` or `/`.

   **Not included:** removing the rehash lock on Ctrl+C (the stale-lock rule covers it within 2 minutes), and the conda name filter (M7, allowlist D-44).

## Global Constraints

- **Toolchain:** Rust `1.96`, edition 2021, workspace `resolver = "2"`. CI runs `cargo fmt --all --check`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo build --workspace`, then `cargo test --workspace`.
- **CI matrix:** `windows-2025`, `windows-2022`, `ubuntu-latest`, `ubuntu-24.04-arm`.
- **Dependencies:**
  - `rpyenv-core`: `std`; `libc = "0.2"` on Unix; `windows-sys = "0.61"` on Windows, with only the features each task lists. Nothing else.
  - `pyenv-shim` and `pyenv-shimw`: `rpyenv-core` only (spec §3).
  - The test-only `rpyenv-e2e` may use `windows-sys` on Windows.
- **Parity policy** (spec §4):
  - Match each OS's upstream in messages, streams and exit codes.
  - Don't reproduce defects.
  - Every intentional difference gets a row in `docs/parity/allowlist.md`. This plan adds D-45 to D-47.
- **Line endings:** rpyenv prints `\n` on Linux and `\r\n` on Windows.
- **Windows streams:** pyenv-win prints its own messages on stdout.
- **Naming:** rpyenv-only variables use the `RPYENV_` prefix. This plan uses `RPYENV_DEBUG_LOG` and `RPYENV_BATCH_FORWARD`, both already named in spec §13.
- **Arguments are bytes:** arguments passed on are never converted to `String` and never re-quoted.
- **`unsafe`:** every `unsafe` block gets a `// SAFETY:` comment saying why it holds.
- **Commits:** one-line `git commit -m "..."` messages, with no attribution lines. Run `cargo fmt --all` before every commit.
- **Test binaries:**
  - The e2e tests need `pyenv`, `pyenv-shim`, `pyenv-shimw`, `argv-echo` and `argv-echow` built side by side. Run `cargo build --workspace` before running one test file at a time.
  - Windows-only code and tests don't compile on Linux, and Unix-only ones don't compile on Windows. The controller runs both.

## Review Focus

The five inputs most likely to break this for a real user that the spec doesn't spell out. Each has a test in the task named in brackets.

1. **Arguments that cmd or a naive re-quote would change:** `^ % ! & "`, unbalanced quotes, a trailing backslash before a quote, empty arguments, UNC paths, non-ASCII. Through a shim and through `pyenv exec`, the child must see exactly what it would see if run directly. [Task 4: `win_shim_is_transparent_to_raw_command_lines`]
2. **Ctrl+C or Ctrl+Break while Python runs.** The shim must not exit before Python does, and must not return early to the prompt. [Task 5: `win_ctrl_break_reaches_the_child_and_the_shim_waits`]
3. **The shim killed from outside** (Task Manager, a CI timeout, `taskkill`). Its child must not keep running. [Task 5: `win_killing_the_shim_kills_the_child`]
4. **Started without a console but with redirected output** (VS Code, `subprocess` with `DETACHED_PROCESS`). No window may appear, and the output must still arrive. [Task 6: console-mode e2e tests]
5. **A GUI script started from Explorer, or a GUI shim that fails.** No console window, and an error that is visible rather than silent. [Task 8: GUI shim e2e tests]

---

## File Structure

```
Cargo.toml                                  workspace: adds crates/pyenv-shimw
docs/parity/allowlist.md                    rows D-45..D-47
crates/rpyenv-core/
  Cargo.toml                                windows-sys on Windows
  src/rehash.rs                             (modify) relative state, RehashStats, re-check, GUI template, forwarders
  src/launch.rs                             (modify) raw_tail, Windows spawn through winproc, io_reason, sig fixes
  src/shim.rs                               (modify) raw tail, GUI reporting, main_gui, argv0 check
  src/lookup.rs                             (modify) which_win_runnable
  src/pathsearch.rs                         (modify) find_cmd with a cwd
  src/shimset.rs                            (modify) Wanted, ShimKind, forwarders
  src/wincmd.rs                             NEW command-line tail (pure, portable)
  src/console.rs                            NEW console-mode choice (pure, portable)
  src/pe.rs                                 NEW PE subsystem reader (pure, portable)
  src/debuglog.rs                           NEW RPYENV_DEBUG_LOG
  src/winproc.rs                            NEW Windows: job, Ctrl handler, console probe, message box
crates/pyenv-shimw/
  Cargo.toml, src/main.rs, tests/direct.rs  NEW the GUI shim
crates/pyenv/src/commands/exec.rs           (modify) raw tail for exec
crates/pyenv/src/commands/rehash.rs         (modify) RehashStats
crates/e2e/
  Cargo.toml                                argv-echow bin; windows-sys on Windows
  src/echo.rs                               NEW argv-echo's body, shared by both bins
  src/bin/argv_echo.rs                      (modify) console bin
  src/bin/argv_echow.rs                     NEW GUI-subsystem bin
  tests/common/mod.rs                       (modify) helpers
  tests/windows.rs                          NEW Windows launch-fidelity tests
```

---

### Task 1: Rehash: relative state paths, re-check after the lock, and what changed

Three items carried forward from M1b:
- **M-3:** the stored state holds absolute paths. The shim spells the root from its own location while the CLI spells it from the environment, so two spellings of one root make every exit check rehash.
- **Exit check:** a shim that waited for the lock rehashes again even when the rehash it waited for already did the work.
- **M-5:** the "links are kept" assertion compares modification times, which hardlinks share, so it can't fail.

**Files:**
- Modify: `crates/rpyenv-core/src/rehash.rs`
- Modify: `crates/pyenv/src/commands/rehash.rs` (the `Ok` arm)

**Interfaces:**
- Produces:
  - `rehash::RehashStats { linked: usize, removed: usize }` (derives `Debug, Clone, Copy, Default, PartialEq, Eq`)
  - `rehash::rehash(ctx, shim_exe, wait) -> Result<RehashStats, RehashError>` (was `Result<(), _>`)
  - `rehash::check(ctx, shim_exe) -> bool`: true when it rehashed
  - `rehash::rehash_locked(ctx, shim_exe) -> Result<RehashStats, RehashError>`: the rehash itself, for a caller that already holds the lock
  - the state header becomes `rpyenv rehash state 2`

- [ ] **Step 1: Write the failing tests**

Add to the tests in `crates/rpyenv-core/src/rehash.rs`:

```rust
    #[test]
    fn state_does_not_depend_on_how_the_root_is_spelled() {
        let (_t, ctx, _) = setup(Flavor::PyenvWin);
        exe(&ctx.versions_dir().join("3.9.1/python.exe"));
        let mut other = ctx.clone();
        other.root = ctx.root.join(".");
        assert_ne!(
            ctx.versions_dir().display().to_string(),
            other.versions_dir().display().to_string()
        );
        assert_eq!(snapshot(&ctx), snapshot(&other));
    }

    #[test]
    fn a_second_rehash_changes_nothing() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/pip.exe"));
        fs::create_dir_all(ctx.shims_dir()).unwrap();
        fs::write(ctx.shims_dir().join("python.bat"), "old").unwrap();
        let first = rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(first, RehashStats { linked: 2, removed: 1 });
        assert_eq!(rehash(&ctx, &shim, Wait::No).unwrap(), RehashStats::default());
    }

    #[cfg(unix)]
    #[test]
    fn a_second_linux_rehash_changes_nothing() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        exe(&ctx.versions_dir().join("3.12.1/bin/python"));
        assert_eq!(rehash(&ctx, &shim, Wait::No).unwrap().linked, 1);
        assert_eq!(rehash(&ctx, &shim, Wait::No).unwrap(), RehashStats::default());
    }

    #[test]
    fn the_check_skips_when_another_rehash_already_did_it() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        exe(&ctx.versions_dir().join("3.9.1/python.exe"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        exe(&ctx.versions_dir().join("3.9.1/Scripts/black.exe"));
        let held = lock(&ctx.shims_dir(), Wait::No).unwrap();
        let (other_ctx, other_shim) = (ctx.clone(), shim.clone());
        let other = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let stats = rehash_locked(&other_ctx, &other_shim).unwrap();
            drop(held);
            stats
        });
        assert!(!check(&ctx, &shim), "the check rehashed again");
        assert_eq!(other.join().unwrap().linked, 1);
        assert!(ctx.shims_dir().join("black.exe").exists());
    }
```

In the existing `windows_rehash_hardlinks_and_removes_pyenv_win_shims`, delete the four lines under `// Links to the current template are kept.` (the `before`/`after` modification times; `a_second_rehash_changes_nothing` replaces them).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core rehash::`
Expected: compile errors: `RehashStats` and `rehash_locked` not found, and `check` returns `()`.

- [ ] **Step 3: Implement**

In `rehash.rs`:

1. Add after `RehashError`:

```rust
/// What one rehash changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RehashStats {
    /// Shims created or re-created.
    pub linked: usize,
    /// Stale shims removed or renamed aside.
    pub removed: usize,
}
```

2. In `snapshot`, change the header to `"rpyenv rehash state 2\n"` and replace `d.display()` in the `format!` with `state_path(&d, &vdir)`. Add:

```rust
/// `d` relative to `versions`, with `/` separators (`.` for `versions` itself), so two
/// spellings of one root give the same state (M1b review M-3).
fn state_path(d: &Path, vdir: &Path) -> String {
    match d.strip_prefix(vdir) {
        Ok(r) if r.as_os_str().is_empty() => ".".to_string(),
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => d.display().to_string(),
    }
}
```

3. Replace `check` and `rehash`, and add `rehash_locked`:

```rust
/// A shim's exit check (spec §8). When the state changed, it waits up to `CHECK_WAIT` for
/// the lock, then checks again (the rehash it waited for may have done the work) before
/// rehashing. Failures are ignored; the next check catches up. True when it rehashed.
pub fn check(ctx: &Ctx, shim_exe: &Path) -> bool {
    if !needed(ctx) {
        return false;
    }
    let Ok(_lock) = lock(&ctx.shims_dir(), Wait::Upto(CHECK_WAIT)) else {
        return false;
    };
    needed(ctx) && rehash_locked(ctx, shim_exe).is_ok()
}

/// Brings `shims` in line with the installed versions.
pub fn rehash(ctx: &Ctx, shim_exe: &Path, wait: Wait) -> Result<RehashStats, RehashError> {
    let _lock = lock(&ctx.shims_dir(), wait)?;
    rehash_locked(ctx, shim_exe)
}

/// The rehash itself, for a caller that holds the lock.
pub fn rehash_locked(ctx: &Ctx, shim_exe: &Path) -> Result<RehashStats, RehashError> {
    let shims = ctx.shims_dir();
    // Taken before scanning: a change during the scan makes the next check rehash again.
    let state = snapshot(ctx);
    let wanted = shimset::wanted(ctx);
    let stats = match ctx.flavor {
        Flavor::Pyenv => apply_links(&shims, shim_exe, &wanted),
        Flavor::PyenvWin => apply_hardlinks(&shims, shim_exe, &wanted),
    }
    .map_err(RehashError::Io)?;
    write_state(&shims, &state).map_err(RehashError::Io)?;
    Ok(stats)
}
```

4. Count what changes:
   - `apply_links` returns `io::Result<RehashStats>`: `linked += 1` after each `symlink(...)?`. It ends with `Ok(RehashStats { linked, removed: remove_stale(shims, wanted, false)? })`.
   - `apply_hardlinks` returns `io::Result<RehashStats>`: `linked += 1` when the hardlink or copy succeeds. It ends with `let removed = remove_stale(shims, wanted, true)?;` and then `first_err.map_or(Ok(RehashStats { linked, removed }), Err)`.
   - `remove_stale` returns `io::Result<usize>`: `removed += 1` next to each `remove_or_rename(&e.path())` of a non-dot entry. Deleting `.old` leftovers doesn't count.

5. In `crates/pyenv/src/commands/rehash.rs`, change `Ok(()) => Output::new(),` to `Ok(_) => Output::new(),`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS. On Windows `rehash::` gains 3 tests (the Linux one is Unix-only).

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core crates/pyenv
git commit -m "Store rehash state relative to versions, re-check after the lock, count changes"
```

---

### Task 2: Carry-forward fixes in the Linux launch path

Four items carried forward from M1b's reviews:
- In `sig::spawn_and_wait`, a failed `spawn` returns without restoring the signal dispositions. `CHILD` is also cleared before they are restored.
- `die_by` can leave a core file that overwrites the child's.
- The D-43 message ends with Rust's `(os error N)`.
- An `argv[0]` containing `/` is trusted without checking that it is the shim.

**Files:**
- Modify: `crates/rpyenv-core/src/launch.rs`
- Modify: `crates/rpyenv-core/src/shim.rs`

**Interfaces:**
- Produces: `launch::io_reason(err: &std::io::Error) -> String`.

- [ ] **Step 1: Write the failing tests**

Add to the tests in `launch.rs`:

```rust
    #[test]
    fn io_reasons_drop_the_os_error_suffix() {
        let reason = io_reason(&std::io::Error::from_raw_os_error(2));
        assert!(!reason.is_empty());
        assert!(!reason.contains("os error"), "{reason}");
        assert_eq!(io_reason(&std::io::Error::other("plain")), "plain");
    }
```

In `shim.rs`, replace `own_root_linux_uses_an_argv0_with_a_slash` with:

```rust
    #[cfg(unix)]
    #[test]
    fn own_root_linux_trusts_an_argv0_with_a_slash_only_when_it_is_the_shim() {
        let tmp = tempfile::tempdir().unwrap();
        let shims = tmp.path().join("root").join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        let own_bin = tmp.path().join("pyenv-shim");
        std::fs::write(&own_bin, b"bin").unwrap();
        std::os::unix::fs::symlink(&own_bin, shims.join("python")).unwrap();
        let argv0 = shims.join("python");
        assert_eq!(
            own_root(Flavor::Pyenv, argv0.as_os_str(), Some(&own_bin), None, tmp.path()),
            Some(tmp.path().join("root"))
        );
        // Any program can set argv[0]; a path that isn't this shim is not trusted.
        let fake = tmp.path().join("fake").join("shims").join("python");
        std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
        std::fs::write(&fake, b"other").unwrap();
        assert_eq!(
            own_root(Flavor::Pyenv, fake.as_os_str(), Some(&own_bin), None, tmp.path()),
            None
        );
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core -- io_reason own_root`
Expected: compile error, `io_reason` not found. On Linux, the `own_root` test's second assertion also fails.

- [ ] **Step 3: Implement**

In `launch.rs`, change `cannot_run` to print `io_reason(err)` instead of `err`, and add:

```rust
/// An I/O error's text without Rust's ` (os error N)` suffix, as a shell prints it (D-43).
pub fn io_reason(err: &std::io::Error) -> String {
    let text = err.to_string();
    match text.rfind(" (os error ") {
        Some(i) if text.ends_with(')') => text[..i].to_string(),
        _ => text,
    }
}
```

In `sig::spawn_and_wait`, replace everything from `let mut child = cmd.spawn()?;` to the end of the function with:

```rust
        let result = match cmd.spawn() {
            Ok(mut child) => {
                CHILD.store(child.id() as i32, Ordering::SeqCst);
                child.wait()
            }
            Err(e) => Err(e),
        };
        // Restore first, then clear CHILD, on both paths: a TERM or HUP that arrives in
        // between meets the caller's own disposition, not a forwarder with no child.
        // SAFETY: restoring this process's own dispositions to exactly what `current` read
        // on entry, before anything was changed.
        unsafe {
            for (signal, original) in SIGNALS.iter().zip(on_entry) {
                install(*signal, original);
            }
        }
        CHILD.store(0, Ordering::SeqCst);
        result
```

Replace `die_by` with:

```rust
    /// Dies from `signal`, as the child did, so the caller sees the same status. No core
    /// file: the child made one if it was going to, and the shim's would overwrite it.
    pub fn die_by(signal: i32) -> ! {
        // SAFETY: plain libc calls on this process; lowering our own core limit is allowed.
        unsafe {
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal)
    }
```

In `shim.rs`, replace the body of `linux_invoked_path` with:

```rust
    let own_canon = std::fs::canonicalize(own?).ok()?;
    if argv0.as_encoded_bytes().contains(&b'/') {
        let p = Path::new(argv0);
        let joined = if p.is_relative() {
            cwd.join(p)
        } else {
            p.to_path_buf()
        };
        // Any program can set argv[0]: trust it only when it really is this shim.
        let canon = std::fs::canonicalize(&joined).ok()?;
        return (canon == own_canon).then(|| crate::paths::lexical_normalize(&joined));
    }
    std::env::split_paths(path?).find_map(|dir| {
        let candidate = dir.join(argv0);
        let canon = std::fs::canonicalize(&candidate).ok()?;
        (canon == own_canon).then_some(candidate)
    })
```

and update its doc comment to say that the `argv0` path must canonicalize to the shim binary too.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS. `io_reasons_drop_the_os_error_suffix` runs on both OSes; the `own_root` test runs on Linux.

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core
git commit -m "Tidy the Linux launch path: restore signals on every path, no core file, plain reasons"
```

---

### Task 3: Windows lookup: launch only runnable files, and `exec` names with a folder

Two items carried forward from M1b's review:
- **M-4:** `which_win` prefers a bare extensionless file over `name.exe`. `pyenv which` must keep that for parity, but a shim can't launch such a file. It works today only because std appends `.exe`.
- **Deferred D-40 item:** `find_cmd` searches `PATH` even for a name like `.\tool.exe`, where cmd.exe runs the file relative to the current folder.

**Files:**
- Modify: `crates/rpyenv-core/src/lookup.rs`
- Modify: `crates/rpyenv-core/src/pathsearch.rs`
- Modify: `crates/rpyenv-core/src/launch.rs` (`plan_win`)
- Modify: `docs/parity/allowlist.md` (D-40 wording)

**Interfaces:**
- Produces:
  - `lookup::which_win_runnable(ctx: &Ctx, command: &str) -> Result<Found, NotFound>`: like `which_win`, but only files ending in `.exe`, `.com`, `.bat` or `.cmd` count
  - `pathsearch::find_cmd(name: &str, path: &OsStr, pathext: Option<&OsStr>, cwd: &Path) -> Option<PathBuf>` (gains `cwd`)

- [ ] **Step 1: Write the failing tests**

Add to the tests in `lookup.rs`:

```rust
    #[test]
    fn win_runnable_lookup_skips_files_windows_cannot_start() {
        let (_t, mut ctx) = win_root(&["3.9.1"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.9.1/Scripts/tool"));
        touch(&v.join("3.9.1/Scripts/tool.exe"));
        ctx.pyenv_version = Some("3.9.1".to_string());
        assert_eq!(
            which_win(&ctx, "tool").map(|f| f.path),
            Ok(v.join("3.9.1").join("Scripts").join("tool"))
        );
        assert_eq!(
            which_win_runnable(&ctx, "tool").map(|f| f.path),
            Ok(v.join("3.9.1").join("Scripts").join("tool.exe"))
        );
    }
```

In `pathsearch.rs`, change the four `find_cmd(...)` calls in `cmd_search_tries_the_typed_name_then_pathext` to pass `tmp.path()` as a fourth argument, and add:

```rust
    #[test]
    fn cmd_search_runs_a_name_with_a_folder_from_the_current_folder() {
        let tmp = tempfile::tempdir().unwrap();
        make_exe(&tmp.path().join("sub"), "tool.exe");
        let elsewhere = tmp.path().join("elsewhere");
        make_exe(&elsewhere, "tool.exe");
        let path = OsString::from(elsewhere.display().to_string());
        assert_eq!(
            find_cmd("sub\\tool", &path, Some(OsStr::new(".exe")), tmp.path()),
            Some(tmp.path().join("sub\\tool.exe"))
        );
        assert_eq!(
            find_cmd("sub/tool.exe", &path, None, tmp.path()),
            Some(tmp.path().join("sub/tool.exe"))
        );
    }
```

On Linux, `sub\tool` is one file name rather than a folder and a file, so that assertion only holds on Windows. Put `#[cfg(windows)]` on this test.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core -- lookup:: pathsearch::`
Expected: compile errors: `which_win_runnable` not found, and `find_cmd` takes 3 arguments.

- [ ] **Step 3: Implement**

In `lookup.rs`, rename the body of `which_win` into a private `which_win_with(ctx, command, runnable_only: bool)`. It filters the hits of each version with `hits.into_iter().find(|h| !runnable_only || is_runnable_win(h))` instead of taking the first. Then:

```rust
/// pyenv-win `CommandWhich` (pyenv.vbs:101-169). The folders in the printed path are
/// as built from the root; only the file name is in its on-disk case (allowlist D-31).
pub fn which_win(ctx: &Ctx, command: &str) -> Result<Found, NotFound> {
    which_win_with(ctx, command, false)
}

/// `which_win` for launching: only files Windows can start count, so a bare `tool` next
/// to `tool.exe` is passed over (M1b review M-4).
pub fn which_win_runnable(ctx: &Ctx, command: &str) -> Result<Found, NotFound> {
    which_win_with(ctx, command, true)
}

fn is_runnable_win(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["exe", "com", "bat", "cmd"].iter().any(|r| e.eq_ignore_ascii_case(r)))
}
```

In `pathsearch.rs`, change `find_cmd`:

```rust
/// cmd.exe's search for a command word. A name with `\` or `/` in it is run from `cwd`
/// (or as given, when absolute), without searching `PATH`. Otherwise each folder of the
/// `;`-separated `path` is searched: the name as typed when it has an extension, then the
/// name plus each `PATHEXT` extension. Quotes and empty entries are dropped.
pub fn find_cmd(name: &str, path: &OsStr, pathext: Option<&OsStr>, cwd: &Path) -> Option<PathBuf> {
    let exts = pathext_list(pathext);
    let typed_ext = Path::new(name).extension().is_some();
    let try_dir = |dir: &Path| -> Option<PathBuf> {
        if typed_ext && dir.join(name).is_file() {
            return Some(dir.join(name));
        }
        exts.iter()
            .map(|e| dir.join(format!("{name}{e}")))
            .find(|c| c.is_file())
    };
    if name.contains(['\\', '/']) {
        return try_dir(cwd);
    }
    let path = path.to_string_lossy();
    path.split(';')
        .map(|d| d.replace('"', ""))
        .filter(|d| !d.is_empty())
        .find_map(|d| try_dir(Path::new(&d)))
}
```

In `launch.rs` `plan_win`:
- change `lookup::which_win(ctx, command)` to `lookup::which_win_runnable(ctx, command)`;
- pass `&ctx.pwd` as `find_cmd`'s fourth argument.

In `docs/parity/allowlist.md`, append to D-40's rpyenv cell: "; a name containing `\` or `/` is run from the current folder without searching `PATH`, as cmd does".

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core docs/parity/allowlist.md
git commit -m "Launch only runnable files on Windows; run exec names with a folder from cwd"
```

---

### Task 4: Pass the raw command-line tail on Windows

Spec §5.3: "The shim takes its raw command line (`GetCommandLineW`), removes the first token using the same rules as the C runtime, and passes the rest to `CreateProcessW` unchanged. It never splits and re-quotes arguments."

**Files:**
- Create: `crates/rpyenv-core/src/wincmd.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod wincmd;`)
- Modify: `crates/rpyenv-core/Cargo.toml` (windows-sys)
- Modify: `crates/rpyenv-core/src/launch.rs` (`LaunchPlan.raw_tail`, Windows `run`)
- Modify: `crates/rpyenv-core/src/shim.rs`, `crates/pyenv/src/commands/exec.rs` (set `raw_tail`)
- Modify: `crates/e2e/Cargo.toml`, `crates/e2e/tests/common/mod.rs`
- Create: `crates/e2e/tests/windows.rs`

**Interfaces:**
- Produces:
  - `wincmd::after_args(cmdline: &[u16], n: usize) -> &[u16]`: what follows the first `n` arguments
  - `wincmd::trim_blanks(s: &[u16]) -> &[u16]`: without leading spaces and tabs
  - `wincmd::own_tail(n: usize) -> Option<OsString>` (`#[cfg(windows)]`): this process's command line after `n` arguments, trimmed; `None` when empty
  - `LaunchPlan.raw_tail: Option<OsString>`: when set on Windows, a non-batch child gets it through `raw_arg` instead of `args`
- Consumes: `LaunchPlan`, `launch::run` (M1b).

The C runtime's rules, as `std::env::args` implements them on Windows:
- **argv[0]:** if it starts with `"`, it runs to the next `"`, with no escapes. Otherwise it runs to the first space or tab.
- **Later arguments:** spaces and tabs separate arguments outside quotes, and `"` toggles quoting. Inside quotes, `""` is a literal `"` and quoting continues.
- **Backslashes:** backslashes count only before a `"`. `2n` backslashes then `"` give `n` backslashes, and the quote acts as a quote. `2n+1` backslashes then `"` give `n` backslashes and a literal `"`.

- [ ] **Step 1: Add the dependency**

In `crates/rpyenv-core/Cargo.toml`:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Environment"] }
```

- [ ] **Step 2: Write the failing unit tests**

Create `crates/rpyenv-core/src/wincmd.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn after(s: &str, n: usize) -> String {
        String::from_utf16(after_args(&w(s), n)).unwrap()
    }

    #[test]
    fn program_name_rules() {
        assert_eq!(after(r#"python.exe a b"#, 1), " a b");
        assert_eq!(after(r#""C:\p q\python.exe" "x y""#, 1), r#" "x y""#);
        assert_eq!(after(r#""C:\p q\python.exe"arg"#, 1), "arg");
        // argv[0] has no escapes: the backslash doesn't protect the quote.
        assert_eq!(after(r#""C:\dir\"x y"#, 1), "x y");
        assert_eq!(after("python.exe\ta", 1), "\ta");
        assert_eq!(after("python.exe", 1), "");
        assert_eq!(after("", 1), "");
    }

    #[test]
    fn later_argument_rules() {
        assert_eq!(after(r#"pyenv exec python "a^b" 100%"#, 3), r#" "a^b" 100%"#);
        assert_eq!(after(r#"pyenv "ex ec" python x"#, 3), " x");
        // \" is a literal quote, so the argument doesn't end at the space inside.
        assert_eq!(after(r#"p a\"b c d"#, 2), " c d");
        // \\" is one backslash, then a quote that opens quoting.
        assert_eq!(after(r#"p a\\"b c" d"#, 2), " d");
        // "" inside quotes is a literal quote, and quoting continues.
        assert_eq!(after(r#"p "a""b c" d"#, 2), " d");
        assert_eq!(after("p  a   b", 2), "   b");
        assert_eq!(after("p a", 3), "");
    }

    #[test]
    fn blanks_are_trimmed_from_the_front_only() {
        assert_eq!(trim_blanks(&w(" \t a b ")), w("a b ").as_slice());
    }
}
```

Add `pub mod wincmd;` to `crates/rpyenv-core/src/lib.rs`.

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core wincmd::`
Expected: compile errors: `after_args` and `trim_blanks` not found.

- [ ] **Step 4: Implement `wincmd`**

Put this above the tests:

```rust
//! A Windows command line, handled as the C runtime parses it, without re-quoting
//! (spec §5.3). The pure functions run on every OS so their tests do too.

const SPACE: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;

fn is_blank(c: u16) -> bool {
    c == SPACE || c == TAB
}

/// What follows the first `n` arguments of `cmdline`, starting with the whitespace after
/// the `n`th, or empty. The program name and later arguments follow the C runtime's
/// rules (the module doc lists them).
pub fn after_args(cmdline: &[u16], n: usize) -> &[u16] {
    let len = cmdline.len();
    if n == 0 {
        return cmdline;
    }
    let mut i = 0;
    if cmdline.first() == Some(&QUOTE) {
        i = 1;
        while i < len && cmdline[i] != QUOTE {
            i += 1;
        }
        if i < len {
            i += 1;
        }
    } else {
        while i < len && !is_blank(cmdline[i]) {
            i += 1;
        }
    }
    for _ in 1..n {
        while i < len && is_blank(cmdline[i]) {
            i += 1;
        }
        if i >= len {
            break;
        }
        let mut quoted = false;
        while i < len {
            let c = cmdline[i];
            if c == BACKSLASH {
                let start = i;
                while i < len && cmdline[i] == BACKSLASH {
                    i += 1;
                }
                // An odd run escapes the quote after it; an even run leaves the quote to
                // the next iteration.
                if i < len && cmdline[i] == QUOTE && (i - start) % 2 == 1 {
                    i += 1;
                }
                continue;
            }
            if c == QUOTE {
                if quoted && i + 1 < len && cmdline[i + 1] == QUOTE {
                    i += 2;
                    continue;
                }
                quoted = !quoted;
                i += 1;
                continue;
            }
            if is_blank(c) && !quoted {
                break;
            }
            i += 1;
        }
    }
    &cmdline[i..]
}

/// `s` without leading spaces and tabs.
pub fn trim_blanks(s: &[u16]) -> &[u16] {
    let start = s.iter().position(|&c| !is_blank(c)).unwrap_or(s.len());
    &s[start..]
}

/// This process's command line after its first `n` arguments, without leading blanks.
/// `None` when nothing follows.
#[cfg(windows)]
pub fn own_tail(n: usize) -> Option<std::ffi::OsString> {
    use std::os::windows::ffi::OsStringExt;
    // SAFETY: GetCommandLineW returns this process's command line, NUL-terminated and
    // valid for the life of the process; it is only read here.
    let all = unsafe {
        let p = windows_sys::Win32::System::Environment::GetCommandLineW();
        if p.is_null() {
            return None;
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        std::slice::from_raw_parts(p, len)
    };
    let rest = trim_blanks(after_args(all, n));
    (!rest.is_empty()).then(|| std::ffi::OsString::from_wide(rest))
}
```

- [ ] **Step 5: Run the unit tests to see them pass**

Run: `cargo test -p rpyenv-core wincmd::`
Expected: PASS, 3 tests, on both OSes.

- [ ] **Step 6: Carry the tail through the launch**

In `launch.rs`:

1. Add to `LaunchPlan`, after `args`:

```rust
    /// Windows: the caller's command line after the command, unchanged. When set, a
    /// non-batch child gets it through `raw_arg` instead of `args` (spec §5.3).
    pub raw_tail: Option<OsString>,
```

   and `raw_tail: None,` in the two `LaunchPlan { … }` literals in `plan_pyenv` and `plan_win`.

2. In `run`, replace `cmd.args(&plan.args);` with:

```rust
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        match &plan.raw_tail {
            Some(tail) if !is_batch(&plan.program) => {
                cmd.raw_arg(tail);
            }
            _ => {
                cmd.args(&plan.args);
            }
        }
    }
    #[cfg(not(windows))]
    cmd.args(&plan.args);
```

3. Add:

```rust
/// A `.bat` or `.cmd` file: std starts it through cmd.exe with its batch-file escaping,
/// so it gets CRT-split arguments rather than the raw tail (spec §5.3).
#[cfg(windows)]
fn is_batch(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
}
```

In `shim.rs` `main`, add this as the first statement of the `Ok(plan) => {` arm (struct-update syntax rather than `mut plan`, so Linux has no unused `mut`):

```rust
            #[cfg(windows)]
            let plan = launch::LaunchPlan {
                raw_tail: crate::wincmd::own_tail(1),
                ..plan
            };
```

In `crates/pyenv/src/commands/exec.rs`, add this as the first statement of the `Ok(plan) => {` arm (`pyenv exec <command>` is three arguments):

```rust
            #[cfg(windows)]
            let plan = launch::LaunchPlan {
                raw_tail: rpyenv_core::wincmd::own_tail(3),
                ..plan
            };
```

- [ ] **Step 7: Write the end-to-end tests**

In `crates/e2e/tests/common/mod.rs`, add:

```rust
/// The `arg=` lines `argv-echo` printed.
pub fn arg_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("arg="))
        .map(String::from)
        .collect()
}
```

Create `crates/e2e/tests/windows.rs`:

```rust
//! Windows launch fidelity (spec §5.3), with real shims.
#![cfg(windows)]

mod common;
use common::*;
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;

fn v(s: &str) -> &OsStr {
    OsStr::new(s)
}

/// Raw command-line tails that cmd.exe or a split-and-requote round trip would change.
const RAW: [&str; 13] = [
    r#"a b"#,
    r#""a b""#,
    r#""""#,
    r#"x^y 100% %USERNAME% a&b !x!"#,
    r#""say \"hi\"""#,
    r#""C:\path\\""#,
    r#""abc"#,
    r#"a""b"#,
    "ñ ü 漢",
    r#"\\server\share"#,
    "  lead",
    r#"a\\\"b"#,
    r#"a|b <c >d"#,
];

/// Review focus 1: through the shim, the child parses exactly what it parses run directly.
#[test]
fn win_shim_is_transparent_to_raw_command_lines() {
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    f.rehash();
    let env = [("PYENV_VERSION", v("3.9.1"))];
    for raw in RAW {
        let direct = f.command(&python, &env).raw_arg(raw).output().unwrap();
        let shim = f.shim_command("python", &env).raw_arg(raw).output().unwrap();
        assert_eq!(arg_lines(&shim), arg_lines(&direct), "raw tail {raw:?}");
    }
}

#[test]
fn win_exec_is_transparent_to_raw_command_lines() {
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    let env = [("PYENV_VERSION", v("3.9.1"))];
    for raw in RAW {
        let direct = f.command(&python, &env).raw_arg(raw).output().unwrap();
        let exec = f
            .command(&built("pyenv"), &env)
            .args(["exec", "python"])
            .raw_arg(raw)
            .output()
            .unwrap();
        assert_eq!(arg_lines(&exec), arg_lines(&direct), "raw tail {raw:?}");
    }
}
```

- [ ] **Step 8: Run the end-to-end tests**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows`
Expected: PASS, 2 tests, on Windows. The file compiles to nothing on Linux.

Check that the test can fail: temporarily replace `cmd.raw_arg(tail)` with `cmd.args(tail.to_string_lossy().split(' '))`, rebuild, and rerun. It must fail. Restore the code, and note the result in your report.

- [ ] **Step 9: Commit**

```bash
git add crates/rpyenv-core crates/pyenv crates/e2e Cargo.lock
git commit -m "Pass the raw command-line tail to Windows children"
```

---
### Task 5: The Job Object, Ctrl+C, and exit codes

Spec §5.3:
- **Job Object:** the child runs in a Job Object with `KILL_ON_JOB_CLOSE`, so killing the shim kills the child. The job also has `SILENT_BREAKAWAY_OK`, so processes the child starts aren't in it.
- **Ctrl+C:** the shim registers a handler that returns `TRUE`. It never calls `SetConsoleCtrlHandler(NULL, TRUE)`, which children would inherit.
- **Exit code:** the child's `DWORD` exit code is returned unchanged, including `0xC000013A`.

**Files:**
- Create: `crates/rpyenv-core/src/winproc.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`#[cfg(windows)] pub mod winproc;`)
- Modify: `crates/rpyenv-core/Cargo.toml` (windows-sys features)
- Modify: `crates/rpyenv-core/src/launch.rs` (Windows `run`)
- Modify: `crates/e2e/Cargo.toml`, `crates/e2e/src/bin/argv_echo.rs`, `crates/e2e/tests/windows.rs`
- Modify: `docs/parity/allowlist.md` (D-45)

**Interfaces:**
- Produces (`#[cfg(windows)]`, in `rpyenv_core::winproc`):
  - `Job::new() -> Option<Job>` and `Job::assign(&self, child: &Child) -> bool`; dropping a `Job` closes it
  - `ignore_console_events()`
  - `spawn_and_wait(cmd: &mut Command, program: &Path) -> io::Result<ExitStatus>` (Task 6 adds the console mode; `program` is for its log line)
- Produces (argv-echo, Windows):
  - `ARGV_ECHO_CATCH_BREAK=1`: catch console events, and exit 5 after the sleep if one arrived
  - `ARGV_ECHO_BREAK_PID=<pid>`: send Ctrl+Break to that process group and exit, before doing anything else

- [ ] **Step 1: Add the dependencies**

`crates/rpyenv-core/Cargo.toml`, replacing the Windows dependency line from Task 4:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_System_Console",
    "Win32_System_Environment",
    "Win32_System_JobObjects",
    "Win32_System_Threading",
] }
```

`crates/e2e/Cargo.toml`, added:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_System_Console"] }
```

- [ ] **Step 2: Teach argv-echo to catch and send Ctrl+Break**

In `crates/e2e/src/bin/argv_echo.rs`:

1. Add to the module doc comment:

   ```
   //! On Windows, `ARGV_ECHO_CATCH_BREAK=1` catches console events and makes the exit code 5
   //! if one arrived by the end of the sleep, and `ARGV_ECHO_BREAK_PID=<pid>` only sends
   //! Ctrl+Break to that process group (attaching to its console) and exits 0, or 2 or 3 on
   //! failure.
   ```

2. Add after the `use` line:

```rust
#[cfg(windows)]
static CAUGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
unsafe extern "system" fn catch(_event: u32) -> windows_sys::core::BOOL {
    CAUGHT.store(true, std::sync::atomic::Ordering::SeqCst);
    1
}

/// Sends Ctrl+Break to process group `pid`, which must own a console: this helper leaves
/// its own console and attaches to that one first.
#[cfg(windows)]
fn send_break(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT,
    };
    // SAFETY: console calls that affect only this helper process.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        if GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) == 0 {
            return 3;
        }
    }
    0
}
```

3. At the very start of `main`, before `let mut out`:

```rust
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_BREAK_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(send_break(pid));
    }
    #[cfg(windows)]
    if std::env::var("ARGV_ECHO_CATCH_BREAK").as_deref() == Ok("1") {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(catch), 1);
        }
    }
```

4. Right after the `ARGV_ECHO_AFTER` block, before `std::process::exit(...)`:

```rust
    #[cfg(windows)]
    if CAUGHT.load(std::sync::atomic::Ordering::SeqCst) {
        std::process::exit(5);
    }
```

- [ ] **Step 3: Write the failing end-to-end tests**

Add to `crates/e2e/tests/windows.rs` (add `use std::process::{Command, Stdio};` and `use std::time::Duration;`):

```rust
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Exit codes are DWORDs; STATUS_CONTROL_C_EXIT must come back unchanged.
#[test]
fn win_exit_codes_pass_through_unchanged() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let code = 0xC000_013Au32 as i32;
    let text = code.to_string();
    let out = f.run_shim(
        "python",
        &[],
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v(&text))],
    );
    assert_eq!(out.status.code(), Some(code));
}

/// Review focus 3.
#[test]
fn win_killing_the_shim_kills_the_child() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let after = f.base.join("after");
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
                ("ARGV_ECHO_AFTER", after.as_os_str()),
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    shim.kill().unwrap();
    let _ = shim.wait();
    std::thread::sleep(Duration::from_millis(3500));
    assert!(!after.exists(), "the child kept running after the shim was killed");
}

/// Review focus 2. The shim gets a windowless console of its own and a process group, the
/// child shares both, and a helper sends Ctrl+Break to the group. The child catches it and
/// exits 5 when its sleep ends. A shim that didn't ignore the event would die at once with
/// 0xC000013A.
#[test]
fn win_ctrl_break_reaches_the_child_and_the_shim_waits() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_CATCH_BREAK", v("1")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
            ],
        )
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_BREAK_PID", shim.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+Break");
    assert_eq!(shim.wait().unwrap().code(), Some(5));
}
```

- [ ] **Step 4: Run them to see the job and Ctrl+Break tests fail**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows`
Expected:
- `win_killing_the_shim_kills_the_child` FAILS: the child keeps running.
- `win_ctrl_break_reaches_the_child_and_the_shim_waits` FAILS: the shim exits with `-1073741510`.
- `win_exit_codes_pass_through_unchanged` may already pass: std returns the full `DWORD`. It stays as the contract's guard.

- [ ] **Step 5: Implement `winproc`**

Create `crates/rpyenv-core/src/winproc.rs`:

```rust
//! Windows process plumbing for the shims and `pyenv exec` (spec §5.3): a Job Object that
//! ends the child with the shim, and a console handler that keeps the shim alive through
//! the child's Ctrl+C.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, TRUE};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
};

/// A Job Object whose processes end when its last handle closes, which happens when this
/// process ends, however it ends. Processes they start break away silently and live on,
/// as if Python were run directly.
pub struct Job(HANDLE);

impl Job {
    pub fn new() -> Option<Job> {
        // SAFETY: creates an unnamed job and sets its limits from a zero-initialized
        // plain-data struct, as the API documents; the handle is closed on failure and on
        // drop.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Job(job))
        }
    }

    /// Puts `child` in the job. False when Windows refuses, for example when the shim runs
    /// in a job that forbids it; the child then runs without one.
    pub fn assign(&self, child: &Child) -> bool {
        // SAFETY: both handles are valid for the duration of the call.
        unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) != 0 }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

unsafe extern "system" fn keep_running(_event: u32) -> BOOL {
    TRUE
}

/// Ctrl+C, Ctrl+Break and closing the console reach the child, which shares the console.
/// The shim ignores them and waits for the child. It uses a handler, never
/// `SetConsoleCtrlHandler(NULL, TRUE)`, which children would inherit (spec §5.3).
pub fn ignore_console_events() {
    // SAFETY: registers a handler that only returns TRUE and touches no state.
    unsafe {
        SetConsoleCtrlHandler(Some(keep_running), TRUE);
    }
}

/// Starts `cmd` inside a job, with console events ignored, and waits for it. The child
/// joins the job right after it starts (plan decision 2).
pub fn spawn_and_wait(cmd: &mut Command, program: &Path) -> io::Result<ExitStatus> {
    let _ = program;
    ignore_console_events();
    let job = Job::new();
    let mut child = cmd.spawn()?;
    if let Some(job) = &job {
        job.assign(&child);
    }
    let status = child.wait();
    drop(job);
    status
}
```

(`program` is used from Task 6 on; the `let _ = program;` line goes away then.)

Add `#[cfg(windows)] pub mod winproc;` to `crates/rpyenv-core/src/lib.rs`.

In `launch.rs` `run`, replace:

```rust
    #[cfg(not(unix))]
    let status = cmd.status();
```

with:

```rust
    #[cfg(windows)]
    let status = crate::winproc::spawn_and_wait(&mut cmd, &plan.program);
    #[cfg(not(any(unix, windows)))]
    let status = cmd.status();
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows`
Expected: PASS, 5 tests, on Windows.

- [ ] **Step 7: Add the allowlist row**

```markdown
| D-45 | Windows | shims | A `.bat` shim: Ctrl+C asks "Terminate batch job (Y/N)?"; killing the shim leaves Python running | No prompt: Ctrl+C reaches Python and the shim waits for it; killing the shim ends Python (Job Object); the exit code is Python's, unchanged | Spec §1, §5.3. |
```

- [ ] **Step 8: Commit**

```bash
git add crates/rpyenv-core crates/e2e docs/parity/allowlist.md Cargo.lock
git commit -m "Run Windows children in a job and keep the shim through Ctrl+C"
```

---

### Task 6: Console modes: INHERIT, NO-WINDOW, MIRROR

`docs/windows-lazy-console.md`, rule 2, without the 24H2 branches (EAGER and LAZY are M5):
- attached to a console → **INHERIT** (no flags);
- else stdout and stderr both redirected → **NO-WINDOW** (`CREATE_NO_WINDOW`; stdin becomes `NUL` if the caller gave none);
- else → **MIRROR** (`DETACHED_PROCESS`).

"Redirected" means `GetFileType` says disk or pipe. `NUL` is a character device, so it doesn't count. The shim logs its choice to `RPYENV_DEBUG_LOG` (spec §11, §13; plan decision 5).

**Files:**
- Create: `crates/rpyenv-core/src/console.rs`, `crates/rpyenv-core/src/debuglog.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod console; pub mod debuglog;`)
- Modify: `crates/rpyenv-core/Cargo.toml` (feature `Win32_Storage_FileSystem`)
- Modify: `crates/rpyenv-core/src/winproc.rs`, `crates/rpyenv-core/src/shim.rs`
- Modify: `crates/e2e/src/bin/argv_echo.rs`, `crates/e2e/tests/windows.rs`

**Interfaces:**
- Produces:
  - `console::ConsoleMode { Inherit, NoWindow, Mirror }` with `fn name(self) -> &'static str` (`"INHERIT"`, `"NO-WINDOW"`, `"MIRROR"`)
  - `console::choose(attached: bool, stdout_redirected: bool, stderr_redirected: bool) -> ConsoleMode`
  - `debuglog::append(line: &str)`
  - `winproc::Probe { attached, stdout_redirected, stderr_redirected, stdin_provided: bool }`, `winproc::probe() -> Probe`
  - argv-echo: `ARGV_ECHO_CONSOLE=1` prints `console=<n>` (`GetConsoleProcessList`'s count, 0 without a console)

- [ ] **Step 1: Write the failing unit tests**

Create `crates/rpyenv-core/src/console.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decision_tree() {
        use ConsoleMode::*;
        let cases = [
            ((true, false, false), Inherit),
            ((true, true, true), Inherit),
            ((false, true, true), NoWindow),
            ((false, true, false), Mirror),
            ((false, false, true), Mirror),
            ((false, false, false), Mirror),
        ];
        for ((attached, out, err), mode) in cases {
            assert_eq!(choose(attached, out, err), mode, "{attached} {out} {err}");
        }
        assert_eq!(NoWindow.name(), "NO-WINDOW");
    }
}
```

Create `crates/rpyenv-core/src/debuglog.rs` with its test first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_lines_when_set() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("debug.log");
        std::env::set_var("RPYENV_DEBUG_LOG", &log);
        append("a");
        append("b");
        std::env::remove_var("RPYENV_DEBUG_LOG");
        append("not logged");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "rpyenv-shim: a\nrpyenv-shim: b\n"
        );
    }
}
```

Add `pub mod console;` and `pub mod debuglog;` to `crates/rpyenv-core/src/lib.rs`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p rpyenv-core -- console:: debuglog::`
Expected: compile errors: `choose` and `append` not found.

- [ ] **Step 3: Implement the two modules**

Above the tests in `console.rs`:

```rust
//! How a Windows shim starts its child, depending on the console it has
//! (docs/windows-lazy-console.md, rule 2). M1 has three modes; EAGER and LAZY are M5.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleMode {
    /// Attached to a console: the child shares it, and the caller's arrangements stand.
    Inherit,
    /// No console, output redirected: `CREATE_NO_WINDOW`, so no window ever appears.
    NoWindow,
    /// No console otherwise: `DETACHED_PROCESS`, as the caller gave the shim.
    Mirror,
}

impl ConsoleMode {
    pub fn name(self) -> &'static str {
        match self {
            ConsoleMode::Inherit => "INHERIT",
            ConsoleMode::NoWindow => "NO-WINDOW",
            ConsoleMode::Mirror => "MIRROR",
        }
    }
}

/// The decision tree, without the 24H2 branches.
pub fn choose(attached: bool, stdout_redirected: bool, stderr_redirected: bool) -> ConsoleMode {
    if attached {
        ConsoleMode::Inherit
    } else if stdout_redirected && stderr_redirected {
        ConsoleMode::NoWindow
    } else {
        ConsoleMode::Mirror
    }
}
```

Above the test in `debuglog.rs`:

```rust
//! `RPYENV_DEBUG_LOG` (spec §11, §13): a file the shim appends its decisions and errors
//! to, for when there is no console to print on.

use std::io::Write;

/// Appends `rpyenv-shim: <line>` to the file `RPYENV_DEBUG_LOG` names. Nothing happens
/// when it is unset, and failures are ignored: a diagnostic must never stop the shim.
pub fn append(line: &str) {
    let Some(path) = std::env::var_os("RPYENV_DEBUG_LOG").filter(|p| !p.is_empty()) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "rpyenv-shim: {line}");
    }
}
```

- [ ] **Step 4: Run the unit tests to see them pass**

Run: `cargo test -p rpyenv-core -- console:: debuglog::`
Expected: PASS, 2 tests, on both OSes.

- [ ] **Step 5: Write the failing end-to-end tests**

In `crates/e2e/src/bin/argv_echo.rs`:
- add to the doc comment: "On Windows, `ARGV_ECHO_CONSOLE=1` prints `console=<n>`, the number of processes on its console (0 without one)";
- add `"Win32_System_Console"` usage (the feature is already there);
- add this block after the `ARGV_ECHO_STDIN` block:

```rust
    #[cfg(windows)]
    if var("ARGV_ECHO_CONSOLE").as_deref() == Some("1") {
        let mut one = 0u32;
        // SAFETY: asks for at most one process id into a one-element buffer.
        let n = unsafe {
            windows_sys::Win32::System::Console::GetConsoleProcessList(&mut one, 1)
        };
        out.push_str(&format!("console={n}\n"));
    }
```

Add to `crates/e2e/tests/windows.rs`:

```rust
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// The console mode the shim logged.
fn mode_in(log: &std::path::Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .find_map(|l| l.split(' ').find_map(|w| w.strip_prefix("mode=")))
        .unwrap_or("")
        .to_string()
}

/// The `console=` count argv-echo printed.
fn console_count(out: &std::process::Output) -> u32 {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("console="))
        .and_then(|n| n.parse().ok())
        .expect("argv-echo printed no console= line")
}

fn console_env(log: &std::path::Path) -> [(&'static str, &OsStr); 3] {
    [
        ("PYENV_VERSION", v("3.9.1")),
        ("RPYENV_DEBUG_LOG", log.as_os_str()),
        ("ARGV_ECHO_CONSOLE", v("1")),
    ]
}

/// Review focus 4: no console, output redirected → a windowless console for the child.
#[test]
fn win_no_console_with_redirected_output_gets_no_window() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command("python", &console_env(&log))
        .creation_flags(DETACHED_PROCESS)
        .output()
        .unwrap();
    assert_eq!(mode_in(&log), "NO-WINDOW");
    assert!(console_count(&out) >= 1, "the child should have a windowless console");
}

/// No console and stderr on NUL (not redirected) → the child gets no console either.
#[test]
fn win_no_console_without_both_outputs_redirected_mirrors() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command("python", &console_env(&log))
        .creation_flags(DETACHED_PROCESS)
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(mode_in(&log), "MIRROR");
    assert_eq!(console_count(&out), 0);
}

/// A shim with a console shares it with the child: the console then holds at least the
/// shim and the child.
#[test]
fn win_a_shim_with_a_console_shares_it() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command("python", &console_env(&log))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert_eq!(mode_in(&log), "INHERIT");
    assert!(console_count(&out) >= 2);
}
```

- [ ] **Step 6: Run them to see them fail**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e --test windows`
Expected: the three new tests FAIL, because nothing writes `mode=` to the log yet.

- [ ] **Step 7: Implement the probe and the modes**

Add `"Win32_Storage_FileSystem"` to the windows-sys features in `crates/rpyenv-core/Cargo.toml`.

In `winproc.rs`, add these imports:

```rust
use crate::console::{self, ConsoleMode};
use crate::debuglog;
use std::os::windows::process::CommandExt;
use std::process::Stdio;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_DISK, FILE_TYPE_PIPE};
use windows_sys::Win32::System::Console::{
    GetConsoleProcessList, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};
```

Add:

```rust
/// What the console decision needs to know about this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub attached: bool,
    pub stdout_redirected: bool,
    pub stderr_redirected: bool,
    pub stdin_provided: bool,
}

fn usable(h: HANDLE) -> bool {
    !h.is_null() && h != INVALID_HANDLE_VALUE
}

/// A disk file or a pipe (the doc's "redirected"). These are this process's own handles,
/// with no I/O pending, so `GetFileType` can't block.
fn redirected(which: STD_HANDLE) -> bool {
    // SAFETY: reads this process's own standard handle and asks for its type.
    unsafe {
        let h = GetStdHandle(which);
        usable(h) && matches!(GetFileType(h), FILE_TYPE_DISK | FILE_TYPE_PIPE)
    }
}

pub fn probe() -> Probe {
    let mut one = 0u32;
    // SAFETY: asks for at most one process id into a one-element buffer; the count it
    // returns is 0 exactly when this process has no console.
    let attached = unsafe { GetConsoleProcessList(&mut one, 1) } != 0;
    // SAFETY: reads this process's own standard input handle.
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    Probe {
        attached,
        stdout_redirected: redirected(STD_OUTPUT_HANDLE),
        stderr_redirected: redirected(STD_ERROR_HANDLE),
        stdin_provided: usable(stdin),
    }
}
```

In `spawn_and_wait`, replace `let _ = program;` with:

```rust
    let p = probe();
    let mode = console::choose(p.attached, p.stdout_redirected, p.stderr_redirected);
    debuglog::append(&format!("mode={} program={}", mode.name(), program.display()));
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
    }
```

In `shim.rs` `main`, in the `Err(report)` arm before `report.emit(flavor);`, add:

```rust
            for line in &report.lines {
                crate::debuglog::append(line);
            }
```

- [ ] **Step 8: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test --workspace`
Expected: PASS. The `windows` e2e file has 8 tests on Windows.

- [ ] **Step 9: Commit**

```bash
git add crates/rpyenv-core crates/e2e Cargo.lock
git commit -m "Choose INHERIT, NO-WINDOW or MIRROR for a Windows shim's child"
```

---

### Task 7: GUI programs link to a GUI shim

Spec §8: a Windows `.exe` shim is "a hardlink to the copy of `pyenv-shim.exe` or `pyenv-shimw.exe` kept in `shims\.template\`, whichever matches the target's PE `Subsystem`. If the type differs between versions, the console shim wins."

**Files:**
- Create: `crates/rpyenv-core/src/pe.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod pe;`)
- Modify: `crates/rpyenv-core/src/shimset.rs`, `crates/rpyenv-core/src/rehash.rs`

**Interfaces:**
- Produces:
  - `pe::subsystem(bytes: &[u8]) -> Option<u16>`, `pe::is_gui(path: &Path) -> bool`, `pe::SUBSYSTEM_WINDOWS_GUI = 2`, `pe::SUBSYSTEM_WINDOWS_CUI = 3`; under `#[cfg(test)]`, `pe::image(subsystem: u16) -> Vec<u8>` (a minimal PE header for tests)
  - `shimset::ShimKind { Console, Gui }` (Task 9 adds `Forward`) and `shimset::Wanted { name: OsString, kind: ShimKind }`
  - `shimset::shims_win(versions_dir: &Path) -> Vec<Wanted>` and `shimset::wanted(ctx) -> Vec<Wanted>` (were lists of names)
  - `rehash::TEMPLATE_GUI_EXE = "pyenv-shimw.exe"`: also the GUI shim's installed name, next to `pyenv-shim.exe`

- [ ] **Step 1: Write the failing tests**

Create `crates/rpyenv-core/src/pe.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_subsystem() {
        assert_eq!(subsystem(&image(SUBSYSTEM_WINDOWS_GUI)), Some(2));
        assert_eq!(subsystem(&image(SUBSYSTEM_WINDOWS_CUI)), Some(3));
    }

    #[test]
    fn not_a_pe_image() {
        assert_eq!(subsystem(b"#!/bin/sh\n"), None);
        assert_eq!(subsystem(&image(2)[..0x90]), None);
        let mut bad = image(2);
        bad[0x80] = b'X';
        assert_eq!(subsystem(&bad), None);
    }

    #[test]
    fn is_gui_reads_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (gui, cui) = (tmp.path().join("w.exe"), tmp.path().join("c.exe"));
        std::fs::write(&gui, image(2)).unwrap();
        std::fs::write(&cui, image(3)).unwrap();
        assert!(is_gui(&gui));
        assert!(!is_gui(&cui));
        assert!(!is_gui(&tmp.path().join("missing.exe")));
    }
}
```

Add `pub mod pe;` to `crates/rpyenv-core/src/lib.rs`.

In `shimset.rs` tests:
- in `windows_shims_for_exe_bat_cmd_in_folder_scripts_and_bin`, compare names: replace `shims_win(&v)` in the assertion with `shims_win(&v).iter().map(|w| w.name.to_string_lossy().into_owned()).collect::<Vec<_>>()`;
- add:

```rust
    #[test]
    fn gui_only_when_every_target_is_gui() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        let write = |rel: &str, sub: u16| {
            let p = v.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, crate::pe::image(sub)).unwrap();
        };
        write("3.9.1/pythonw.exe", 2);
        write("3.9.1/tool.exe", 2);
        write("3.8.2/tool.exe", 3);
        let kinds: Vec<(String, ShimKind)> = shims_win(&v)
            .into_iter()
            .map(|w| (w.name.to_string_lossy().into_owned(), w.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("pythonw.exe".to_string(), ShimKind::Gui),
                ("tool.exe".to_string(), ShimKind::Console)
            ]
        );
    }
```

Add to the tests in `rehash.rs`:

```rust
    #[test]
    fn gui_programs_link_to_the_gui_shim() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        fs::write(tmp.path().join(TEMPLATE_GUI_EXE), b"gui shim").unwrap();
        let v = ctx.versions_dir().join("3.9.1");
        fs::create_dir_all(&v).unwrap();
        fs::write(v.join("python.exe"), crate::pe::image(3)).unwrap();
        fs::write(v.join("pythonw.exe"), crate::pe::image(2)).unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        let shims = ctx.shims_dir();
        assert_eq!(fs::read(shims.join("python.exe")).unwrap(), b"shim binary");
        assert_eq!(fs::read(shims.join("pythonw.exe")).unwrap(), b"gui shim");
        assert_eq!(rehash(&ctx, &shim, Wait::No).unwrap(), RehashStats::default());
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core -- pe:: shimset:: rehash::`
Expected: compile errors: `subsystem`, `ShimKind`, `TEMPLATE_GUI_EXE` not found.

- [ ] **Step 3: Implement `pe`**

Above the tests in `pe.rs`:

```rust
//! The PE `Subsystem` field: whether an `.exe` is a GUI or a console program (spec §8;
//! docs/windows-lazy-console.md, rule 1).

use std::io::Read;
use std::path::Path;

pub const SUBSYSTEM_WINDOWS_GUI: u16 = 2;
pub const SUBSYSTEM_WINDOWS_CUI: u16 = 3;

/// The `Subsystem` of a PE image, from its first bytes. None when they aren't a PE image.
pub fn subsystem(bytes: &[u8]) -> Option<u16> {
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let at = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if bytes.get(at..at.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    // The optional header follows the 4-byte signature and the 20-byte file header;
    // Subsystem is at offset 68 in it, in both PE32 and PE32+.
    let field = at.checked_add(4 + 20 + 68)?;
    Some(u16::from_le_bytes(
        bytes.get(field..field.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// True for a GUI-subsystem executable. Reads at most the first 64 KiB.
pub fn is_gui(path: &Path) -> bool {
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(65536).read_to_end(&mut head));
    read.is_ok() && subsystem(&head) == Some(SUBSYSTEM_WINDOWS_GUI)
}

/// A minimal PE header with the given subsystem, for tests.
#[cfg(test)]
pub(crate) fn image(subsystem: u16) -> Vec<u8> {
    let mut b = vec![0u8; 0x200];
    b[..2].copy_from_slice(b"MZ");
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    let field = 0x80 + 4 + 20 + 68;
    b[field..field + 2].copy_from_slice(&subsystem.to_le_bytes());
    b
}
```

- [ ] **Step 4: Give each wanted shim a kind**

In `shimset.rs`:

1. Add:

```rust
/// Which shim binary a shim is. Linux shims are always `Console`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimKind {
    /// `pyenv-shim`.
    Console,
    /// `pyenv-shimw`: every version's file with this name is a GUI program.
    Gui,
}

/// One file rehash keeps in `shims`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub name: OsString,
    pub kind: ShimKind,
}
```

2. Change `shims_win` to return `Vec<Wanted>`. The map becomes `BTreeMap<String, Wanted>`, and inside the loop replace the `by_key.entry(...).or_insert_with(...)` statement with:

```rust
                let kind = if ext == "exe" && crate::pe::is_gui(&path) {
                    ShimKind::Gui
                } else {
                    ShimKind::Console
                };
                // The console shim wins when versions disagree (spec §8).
                let slot = by_key.entry(stem.to_ascii_lowercase()).or_insert(Wanted {
                    name: OsString::from(format!("{stem}.exe")),
                    kind,
                });
                if kind == ShimKind::Console {
                    slot.kind = ShimKind::Console;
                }
```

   Update its doc comment: "…each with the shim binary it needs: GUI only when every version's file is a GUI program."

3. Change `wanted` to return `Vec<Wanted>`:

```rust
/// The shims rehash keeps in `shims` for the context's flavor.
pub fn wanted(ctx: &Ctx) -> Vec<Wanted> {
    match ctx.flavor {
        Flavor::Pyenv => executables_pyenv(&ctx.versions_dir())
            .into_iter()
            .map(|name| Wanted {
                name,
                kind: ShimKind::Console,
            })
            .collect(),
        Flavor::PyenvWin => shims_win(&ctx.versions_dir()),
    }
}
```

- [ ] **Step 5: Link each kind to its template**

In `rehash.rs`:

1. Add the constant:

```rust
/// Windows: the GUI shim's template name, and its installed name next to `pyenv-shim.exe`.
pub const TEMPLATE_GUI_EXE: &str = "pyenv-shimw.exe";
```

2. Import `use crate::shimset::{ShimKind, Wanted};`, and change `apply_links`, `apply_hardlinks` and `remove_stale` to take `wanted: &[Wanted]`. `remove_stale` builds its `keep` set from `wanted.iter().map(|w| key(&w.name))`, and `apply_links` uses `&w.name`.

3. `refresh_template` gains a `name: &str` parameter and uses it for both the template (`dir.join(name)`) and the temporary file (`dir.join(format!("{name}.tmp"))`), instead of `TEMPLATE_EXE`. Its `.old` cleanup loop also deletes leftover `.tmp` files.

4. The start of `apply_hardlinks` becomes:

```rust
fn apply_hardlinks(shims: &Path, source: &Path, wanted: &[Wanted]) -> io::Result<RehashStats> {
    let console = refresh_template(shims, source, TEMPLATE_EXE)?;
    // The GUI shim is installed next to the console one, and its template next to the
    // console template. Without it, GUI programs get the console shim.
    let gui_source = source.with_file_name(TEMPLATE_GUI_EXE);
    let gui = if gui_source.is_file() {
        refresh_template(shims, &gui_source, TEMPLATE_GUI_EXE)?
    } else {
        console.clone()
    };
    let (console_meta, gui_meta) = (fs::metadata(&console)?, fs::metadata(&gui)?);
```

   In the loop, pick the pair per shim, and use `template`/`tmeta` where the loop used the single template before:

```rust
    for w in wanted {
        let (template, tmeta) = match w.kind {
            ShimKind::Console => (&console, &console_meta),
            ShimKind::Gui => (&gui, &gui_meta),
        };
        let p = shims.join(&w.name);
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS: 3 new `pe::` tests, the new `shimset::` and `rehash::` tests, and every existing test.

- [ ] **Step 7: Commit**

```bash
git add crates/rpyenv-core
git commit -m "Read the PE subsystem and link GUI programs to a GUI shim template"
```

---

### Task 8: The `pyenv-shimw` binary and GUI error reporting

Spec §3 lists `pyenv-shimw` as the GUI-subsystem shim, and §11 says: "The GUI shim shows a message box." Plan decision 4 adds that it does so only when it has nowhere to write.

**Files:**
- Create: `crates/pyenv-shimw/Cargo.toml`, `crates/pyenv-shimw/src/main.rs`, `crates/pyenv-shimw/tests/direct.rs`
- Modify: `Cargo.toml` (workspace member)
- Modify: `crates/rpyenv-core/src/shim.rs`, `crates/rpyenv-core/src/winproc.rs`, `crates/rpyenv-core/Cargo.toml` (feature `Win32_UI_WindowsAndMessaging`)
- Create: `crates/e2e/src/echo.rs`, `crates/e2e/src/bin/argv_echow.rs`
- Modify: `crates/e2e/Cargo.toml`, `crates/e2e/src/bin/argv_echo.rs`, `crates/e2e/tests/windows.rs`
- Modify: `docs/parity/allowlist.md` (D-46)

**Interfaces:**
- Produces:
  - `shim::SHIMW_NAME = "pyenv-shimw"`; `shim::main_gui() -> i32`
  - `winproc::std_handle_usable(stderr: bool) -> bool`, `winproc::message_box(text: &str)`
  - the `argv-echow` test program: argv-echo built for the GUI subsystem

- [ ] **Step 1: Split argv-echo so a GUI build can share it**

1. Move the whole body of `crates/e2e/src/bin/argv_echo.rs` (everything except the module doc comment) into a new `crates/e2e/src/echo.rs`, renaming `fn main()` to `pub fn main()`.
2. `crates/e2e/src/bin/argv_echo.rs` keeps its doc comment and becomes:

```rust
#[path = "../echo.rs"]
mod echo;

fn main() {
    echo::main()
}
```

3. Create `crates/e2e/src/bin/argv_echow.rs`:

```rust
//! argv-echo built for the Windows GUI subsystem, as `pythonw` is, for the GUI-shim tests.
#![windows_subsystem = "windows"]

#[path = "../echo.rs"]
mod echo;

fn main() {
    echo::main()
}
```

4. Add to `crates/e2e/Cargo.toml`:

```toml
[[bin]]
name = "argv-echow"
path = "src/bin/argv_echow.rs"
test = false
```

- [ ] **Step 2: Create the crate and its test**

`crates/pyenv-shimw/Cargo.toml`:

```toml
[package]
name = "pyenv-shimw"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "pyenv-shimw"
path = "src/main.rs"

[dependencies]
rpyenv-core.workspace = true
```

`crates/pyenv-shimw/src/main.rs`:

```rust
//! The GUI-subsystem shim (spec §3): rehash links GUI programs such as `pythonw` to this,
//! so starting one opens no console window.
#![windows_subsystem = "windows"]

fn main() {
    std::process::exit(rpyenv_core::shim::main_gui());
}
```

`crates/pyenv-shimw/tests/direct.rs`:

```rust
/// Run under its own name, the GUI shim is not a command. It has a stderr to write to
/// here, so it says so there rather than in a message box.
#[test]
fn running_the_gui_shim_directly_is_an_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pyenv-shimw"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("run this through a shim"));
}
```

In the root `Cargo.toml`, `members` becomes:

```toml
members = ["crates/rpyenv-core", "crates/pyenv", "crates/pyenv-shim", "crates/pyenv-shimw", "crates/e2e"]
```

- [ ] **Step 3: Write the failing end-to-end tests**

Add to `crates/e2e/tests/windows.rs`:

```rust
/// Review focus 5: a GUI program's shim is the GUI shim, and it passes arguments and the
/// exit code on like the console shim.
#[test]
fn win_gui_programs_get_the_gui_shim() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let pythonw = f.root.join("versions").join("3.9.1").join("pythonw.exe");
    std::fs::copy(built("argv-echow"), &pythonw).unwrap();
    f.rehash();
    let read = |p: std::path::PathBuf| std::fs::read(p).unwrap();
    assert_eq!(read(f.shim("pythonw")), read(built("pyenv-shimw")));
    assert_eq!(read(f.shim("python")), read(built("pyenv-shim")));
    let out = f.run_shim(
        "pythonw",
        &["a b".into()],
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v("7"))],
    );
    assert_eq!(arg_lines(&out), [line("arg", "a b")]);
    assert_eq!(out.status.code(), Some(7));
}

/// With an output to write to, the GUI shim reports there, not in a message box (which
/// would block this test).
#[test]
fn win_gui_shim_reports_on_a_given_output() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let tool = f.root.join("versions").join("3.8.2").join("tool.exe");
    std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
    std::fs::copy(built("argv-echow"), &tool).unwrap();
    f.rehash();
    let out = f.run_shim("tool", &[], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(out.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("pyenv: tool: command not found\r\n"));
}
```

- [ ] **Step 4: Run them to see them fail**

Run: `cargo build --workspace`
Expected: build errors: `shim::main_gui` not found.

- [ ] **Step 5: Implement**

Add `"Win32_UI_WindowsAndMessaging"` to the windows-sys features in `crates/rpyenv-core/Cargo.toml`.

In `winproc.rs`, add:

```rust
use windows_sys::Win32::Storage::FileSystem::FILE_TYPE_UNKNOWN;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

/// Whether this process has a usable stdout (or, with `stderr`, stderr) to write to. A
/// GUI program started from Explorer has neither.
pub fn std_handle_usable(stderr: bool) -> bool {
    let which = if stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    // SAFETY: reads this process's own standard handle and asks for its type.
    unsafe {
        let h = GetStdHandle(which);
        usable(h) && GetFileType(h) != FILE_TYPE_UNKNOWN
    }
}

/// A modal error box titled "rpyenv", for the GUI shim when there is nowhere to print.
pub fn message_box(text: &str) {
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("rpyenv"));
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
```

In `shim.rs`:

1. Add `pub const SHIMW_NAME: &str = "pyenv-shimw";`. In `command_name`, reject both names:

   `(!name.is_empty() && !name.eq_ignore_ascii_case(SHIM_NAME) && !name.eq_ignore_ascii_case(SHIMW_NAME)).then_some(name)`.

   Add `assert_eq!(win("pyenv-shimw.exe"), None);` to the `command_names` test.

2. Rename the current `pub fn main() -> i32` to `fn run(gui: bool) -> i32`, and add:

```rust
/// The console shim's main. Returns the exit code, unless the command replaced this process.
pub fn main() -> i32 {
    run(false)
}

/// The GUI shim's main: the same, except that a message with nowhere to go appears in a
/// message box.
pub fn main_gui() -> i32 {
    run(true)
}

/// Prints what the shim has to say on its stream, and logs it to `RPYENV_DEBUG_LOG`. The
/// GUI shim, when that stream isn't a usable handle, shows a message box instead
/// (plan decision 4).
fn say(gui: bool, flavor: Flavor, lines: &[String], to_stderr: bool) {
    if lines.is_empty() {
        return;
    }
    for line in lines {
        crate::debuglog::append(line);
    }
    #[cfg(windows)]
    if gui && !crate::winproc::std_handle_usable(to_stderr) {
        crate::winproc::message_box(&lines.join("\r\n"));
        return;
    }
    let _ = gui;
    crate::lookup::Report {
        lines: lines.to_vec(),
        stderr: to_stderr,
        code: 0,
    }
    .emit(flavor);
}
```

3. In `run`, route every message through `say`:
   - the direct-run message: `say(gui, flavor, &[format!("{SHIM_NAME}: run this through a shim (such as `python`), not directly")], true);`
   - the `Ctx` error: `say(gui, flavor, &[e.message()], true);`
   - the `Err(report)` arm: `say(gui, flavor, &report.lines, report.stderr); report.code`. This replaces `report.emit` and the Task 6 `debuglog` loop, which `say` now does.
   - the warnings: `say(gui, flavor, &plan.warnings, true);` in place of the `eprint!` loop.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test --workspace`
Expected: PASS, including `pyenv-shimw`'s `direct` test on both OSes and the two new `windows` e2e tests.

- [ ] **Step 7: Add the allowlist row**

```markdown
| D-46 | Windows | shims (GUI programs) | `pythonw` runs through a `.bat` shim, so a console window opens; a GUI launcher in `Scripts` gets only a `.lnk` shim | A GUI program's shim is the GUI-subsystem `pyenv-shimw`: no console window; its errors go to its output when it has one, else to a message box | Spec §3, §8, §11. |
```

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/pyenv-shimw crates/rpyenv-core crates/e2e docs/parity/allowlist.md
git commit -m "Add the pyenv-shimw GUI shim"
```

---

### Task 9: `.cmd` forwarders for `RPYENV_BATCH_FORWARD`

Spec §5.3 and §8: a batch tool that must change the caller's environment is named in `RPYENV_BATCH_FORWARD` (`;`-separated). Instead of an exe shim, it gets a `.cmd` forwarder that:
- resolves the target with `pyenv which`, using the absolute path of `pyenv.exe` written in at rehash time;
- runs the target in the caller's cmd, without `call` and without `setlocal`;
- if resolution fails, prints pyenv's error and sets errorlevel 127.

Known limitation: arguments typed without quotes lose one level of `^` escaping.

**Files:**
- Modify: `crates/rpyenv-core/src/shimset.rs`, `crates/rpyenv-core/src/rehash.rs`
- Modify: `crates/e2e/tests/windows.rs`
- Modify: `docs/parity/allowlist.md` (D-47)

**Interfaces:**
- Produces:
  - `ShimKind::Forward`
  - `shimset::forward_names(value: Option<&OsStr>) -> Vec<String>`: lowercased, trimmed, non-empty entries
  - `shimset::shims_win(versions_dir: &Path, forward: &[String]) -> Vec<Wanted>` (gains `forward`)
  - `shimset::forwarder(pyenv: &Path, name: &str) -> String`
  - `rehash::PYENV_PATH_FILE = "pyenv-path.txt"`, kept in `shims\.template`

How the forwarder works:
- Its first line runs `pyenv which` quietly. If that fails, it runs it again to print the error, then `exit /b 127`.
- Its second line captures the path.
- Its third line clears the helper variable and runs the target on the same line. cmd expands `%RPYENV_FORWARD_TARGET%` when it reads the line, before the `set` runs, and without `call` control passes to the target.

- [ ] **Step 1: Write the failing tests**

Add to the tests in `shimset.rs`:

```rust
    #[test]
    fn forward_names_parse() {
        assert_eq!(
            forward_names(Some(OsStr::new(" Setvar ;;activate-env;"))),
            ["setvar", "activate-env"]
        );
        assert!(forward_names(None).is_empty());
    }

    #[test]
    fn listed_batch_tools_get_forwarders() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for f in ["3.9.1/python.exe", "3.9.1/Scripts/setvar.bat", "3.9.1/Scripts/other.bat"] {
            file(&v.join(f), true);
        }
        let kinds: Vec<(String, ShimKind)> = shims_win(&v, &["setvar".to_string()])
            .into_iter()
            .map(|w| (w.name.to_string_lossy().into_owned(), w.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("other.exe".to_string(), ShimKind::Console),
                ("python.exe".to_string(), ShimKind::Console),
                ("setvar.cmd".to_string(), ShimKind::Forward),
            ]
        );
    }

    #[test]
    fn forwarder_text() {
        assert_eq!(
            forwarder(Path::new(r"C:\bin\pyenv.exe"), "setvar"),
            "@\"C:\\bin\\pyenv.exe\" which setvar >nul 2>&1 || (\"C:\\bin\\pyenv.exe\" which setvar & exit /b 127)\r\n\
             @for /f \"delims=\" %%i in ('\"\"C:\\bin\\pyenv.exe\" which setvar\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
             @(set \"RPYENV_FORWARD_TARGET=\") & \"%RPYENV_FORWARD_TARGET%\" %*\r\n"
        );
    }
```

Add `use std::ffi::OsStr;` to that test module if it isn't there. In the two existing `shims_win(&v)` test calls, pass `&[]` as the second argument.

Add to the tests in `rehash.rs`:

```rust
    #[test]
    fn forwarders_use_the_recorded_pyenv_path() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        let pyenv = tmp.path().join("pyenv.exe");
        fs::write(&pyenv, b"cli").unwrap();
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/setvar.bat"));
        let wanted = shimset::shims_win(&v, &["setvar".to_string()]);
        let shims = ctx.shims_dir();
        let stats = apply_hardlinks(&shims, &shim, &wanted).unwrap();
        assert_eq!(stats.linked, 2);
        assert_eq!(
            fs::read_to_string(shims.join("setvar.cmd")).unwrap(),
            shimset::forwarder(&pyenv, "setvar")
        );
        assert!(!shims.join("setvar.exe").exists());
        // A shim's exit check passes the template; the recorded path keeps the forwarder.
        let template = shims.join(TEMPLATE_DIR).join(TEMPLATE_EXE);
        assert_eq!(
            apply_hardlinks(&shims, &template, &wanted).unwrap(),
            RehashStats::default()
        );
    }
```

Add to `crates/e2e/tests/windows.rs`:

```rust
fn cmd_exe() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe")
}

fn install_setvar(f: &Fixture) {
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("setvar.bat"), "@set FROM_BAT=%1\r\n").unwrap();
}

/// A forwarded batch tool changes the caller's environment, and the helper variable
/// doesn't stay behind.
#[test]
fn win_forwarder_changes_the_callers_environment() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    install_setvar(&f);
    let env = [("PYENV_VERSION", v("3.9.1")), ("RPYENV_BATCH_FORWARD", v("setvar"))];
    assert!(f.pyenv(&["rehash"], &env).status.success());
    assert!(f.root.join("shims").join("setvar.cmd").is_file());
    assert!(!f.shim("setvar").exists());
    let out = f
        .command(&cmd_exe(), &env)
        .args(["/d", "/c"])
        .raw_arg(r#""setvar hello & set FROM_BAT & set RPYENV_FORWARD_TARGET""#)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
}

/// A forwarded tool the selected version lacks: pyenv's message, errorlevel 127.
#[test]
fn win_forwarder_reports_a_missing_command_with_127() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.install("3.8.2/python.exe");
    install_setvar(&f);
    let env = [("RPYENV_BATCH_FORWARD", v("setvar"))];
    assert!(f.pyenv(&["rehash"], &env).status.success());
    let out = f
        .command(&cmd_exe(), &[("PYENV_VERSION", v("3.8.2"))])
        .args(["/d", "/c", "setvar"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&out.stdout).contains("pyenv: setvar: command not found"));
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core -- shimset:: rehash::`
Expected: compile errors: `forward_names`, `forwarder`, `ShimKind::Forward` not found, and `shims_win` takes one argument.

- [ ] **Step 3: Implement the wanted set**

In `shimset.rs`:

1. Add the variant to `ShimKind`:

```rust
    /// A `.cmd` forwarder, for a batch tool named in `RPYENV_BATCH_FORWARD`.
    Forward,
```

2. Add:

```rust
/// The names in `RPYENV_BATCH_FORWARD` (`;`-separated), lowercased.
pub fn forward_names(value: Option<&OsStr>) -> Vec<String> {
    value
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default()
        .split(';')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect()
}

/// A `.cmd` forwarder (spec §5.3). It resolves `name` with `pyenv which`, then runs it in
/// the caller's cmd, without `call` or `setlocal`, so whatever the batch file sets stays
/// set. The helper variable is cleared on the line that uses it: cmd expands `%…%` when it
/// reads the line. When `name` can't be resolved, `pyenv which` says why and errorlevel is
/// 127.
pub fn forwarder(pyenv: &Path, name: &str) -> String {
    let p = pyenv.display();
    format!(
        "@\"{p}\" which {name} >nul 2>&1 || (\"{p}\" which {name} & exit /b 127)\r\n\
         @for /f \"delims=\" %%i in ('\"\"{p}\" which {name}\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
         @(set \"RPYENV_FORWARD_TARGET=\") & \"%RPYENV_FORWARD_TARGET%\" %*\r\n"
    )
}
```

3. `shims_win` gains `forward: &[String]`. For a `.bat` or `.cmd` whose lowercase stem is in `forward`, the candidate is `Wanted { name: "<stem>.cmd", kind: ShimKind::Forward }`. Merge with a rank so that a forwarder beats an exe shim, and a console shim beats a GUI shim. Replace the Task 7 slot logic with:

```rust
                let lower = stem.to_ascii_lowercase();
                let is_batch = ext == "bat" || ext == "cmd";
                let candidate = if is_batch && forward.contains(&lower) {
                    Wanted {
                        name: OsString::from(format!("{stem}.cmd")),
                        kind: ShimKind::Forward,
                    }
                } else if ext == "exe" && crate::pe::is_gui(&path) {
                    Wanted {
                        name: OsString::from(format!("{stem}.exe")),
                        kind: ShimKind::Gui,
                    }
                } else {
                    Wanted {
                        name: OsString::from(format!("{stem}.exe")),
                        kind: ShimKind::Console,
                    }
                };
                let rank = |k: ShimKind| match k {
                    ShimKind::Gui => 0,
                    ShimKind::Console => 1,
                    ShimKind::Forward => 2,
                };
                let slot = by_key.entry(lower).or_insert(candidate.clone());
                if rank(candidate.kind) > rank(slot.kind) {
                    *slot = candidate;
                }
```

   Update its doc comment to mention forwarders.

4. In `wanted`, the Windows arm becomes:

```rust
        Flavor::PyenvWin => shims_win(
            &ctx.versions_dir(),
            &forward_names(std::env::var_os("RPYENV_BATCH_FORWARD").as_deref()),
        ),
```

- [ ] **Step 4: Write forwarders during rehash**

In `rehash.rs`:

1. Add:

```rust
/// Where `pyenv rehash` records the `pyenv.exe` that forwarders call (plan decision 6).
pub const PYENV_PATH_FILE: &str = "pyenv-path.txt";

/// The `pyenv.exe` forwarders call. `pyenv rehash` passes the shim binary installed next
/// to `pyenv.exe`, and the path is recorded; a shim's exit check passes the template and
/// reads the record back.
fn pyenv_exe(source: &Path, template_dir: &Path) -> Option<PathBuf> {
    let record = template_dir.join(PYENV_PATH_FILE);
    if source.parent() == Some(template_dir) {
        return fs::read_to_string(&record)
            .ok()
            .map(|s| PathBuf::from(s.trim()));
    }
    let pyenv = source.with_file_name("pyenv.exe");
    if !pyenv.is_file() {
        return None;
    }
    let _ = fs::write(&record, pyenv.display().to_string());
    Some(pyenv)
}
```

2. In `apply_hardlinks`, after the templates, add:

```rust
    let pyenv = pyenv_exe(source, &shims.join(TEMPLATE_DIR));
```

   At the top of the loop body, handle forwarders before the template choice. Without a known `pyenv.exe`, a forwarder falls back to the console shim:

```rust
        if let (ShimKind::Forward, Some(pyenv)) = (w.kind, &pyenv) {
            let p = shims.join(&w.name);
            let file = w.name.to_string_lossy();
            let text = shimset::forwarder(pyenv, file.strip_suffix(".cmd").unwrap_or(&file));
            if fs::read_to_string(&p).ok().as_deref() != Some(text.as_str()) {
                remove_or_rename(&p);
                match fs::write(&p, &text) {
                    Ok(()) => linked += 1,
                    Err(e) => {
                        first_err.get_or_insert(e);
                    }
                }
            }
            continue;
        }
        let (template, tmeta) = match w.kind {
            ShimKind::Gui => (&gui, &gui_meta),
            ShimKind::Console | ShimKind::Forward => (&console, &console_meta),
        };
```

   `strip_suffix(".cmd")` drops the extension the wanted name carries. The forwarder resolves the bare command name, as a user would type it.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test --workspace`
Expected: PASS: the new unit tests on both OSes, and the two forwarder e2e tests on Windows.

- [ ] **Step 6: Add the allowlist row**

```markdown
| D-47 | Windows | `rehash` (batch tools) | A batch file in `Scripts` gets only a `.lnk` shim, which cmd can't run by name | A batch tool gets the console exe shim (it runs in a child cmd); a name listed in `RPYENV_BATCH_FORWARD` gets a `.cmd` forwarder that runs it in the caller's cmd, so it can change the caller's environment (unquoted arguments lose one level of `^`) | Spec §5.3, §8. |
```

- [ ] **Step 7: Commit**

```bash
git add crates/rpyenv-core crates/e2e docs/parity/allowlist.md
git commit -m "Write .cmd forwarders for RPYENV_BATCH_FORWARD"
```

---

## After this plan

- **M1c (next):** parity CI:
  - the adapted pyenv-win pytest suite and upstream bats subset;
  - differential tests against real pyenv and pyenv-win, reading `docs/parity/allowlist.md`;
  - the shim dependency guard (the shims link `rpyenv-core` only);
  - the shim overhead benchmark.

  It also closes the M1a test gaps: Windows CLI tests for `version-file-read` and `version-file-write`, and allowlist rows D-04, D-17, D-19, D-21 and D-22.
- **M5:**
  - the EAGER and LAZY console modes, and the `consoleAllocationPolicy=detached` manifest entry;
  - the live rehash watcher;
  - removing the rehash lock when `pyenv rehash` is interrupted.
- **M7:** conda support, with upstream's `conda.d/default.list` filter (allowlist D-44).
- **Still open, low priority:**
  - `RPYENV_BATCH_FORWARD` is read at each rehash, so a shim's exit check running without it turns forwarders back into exe shims. Put the variable in the user environment, as the MSI will (M6).
  - The rename-aside branch runs only on hosts that refuse to delete a running shim; it is unit-tested.
