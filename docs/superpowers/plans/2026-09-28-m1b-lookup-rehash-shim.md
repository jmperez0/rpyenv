# M1b: Command Lookup, Rehash, `exec` and the Shim — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Python versions installed by pyenv or pyenv-win become runnable through rpyenv: `pyenv which`, `whence`, `exec`, `rehash`, `shims` and `versions --executables` work as upstream does, and `pyenv rehash` creates native shims that run the selected version's file. Linux shims are complete. Windows shims are basic (spawn, wait, pass the exit code on); M1b-win makes them exact.

**Architecture:** Every rule stays in `rpyenv-core`, so the `pyenv` CLI and the new `pyenv-shim` binary share it:
- `lookup` finds a command's file (`which`) and every version that has it (`whence`), for both flavors.
- `shimset` decides which names get a shim.
- `rehash` creates and removes shims under a lock, and stores a small state file. A shim compares that file with the disk after the child exits, and rehashes only when something changed.
- `launch` turns a command into a `LaunchPlan` (file, arguments, environment) and runs it. On Linux it replaces the process (`execv`), except for pip commands, which it spawns and waits for so the rehash check finishes before the caller continues.
- `shim` is the whole shim program. The `pyenv-shim` crate is a one-line `main`.

**Tech Stack:** Rust 1.96 (edition 2021). `rpyenv-core` uses `std`, plus `libc` on Unix (signals). `tempfile` for tests.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`: §3 (units), §5 (shims), §8 (rehash), §11 (errors), §12 (testing). The upstream behavior this plan reproduces is in `docs/parity/pyenv-m1-reference.md` (pyenv 2.8.6) and `docs/parity/pyenv-win-m1-reference.md` (pyenv-win `856ed5a`): the sections `which`, `whence`, `exec`, `rehash`, `shims` and `versions`. Read spec §4, "Parity policy", before starting.

**Milestone split:** M1 is delivered in four plans:
- **M1a (done):** core resolution and the read-only CLI.
- **M1b (this plan):** `which`, `whence`, `exec`, `rehash`, `shims`, `versions --executables`, the Linux shim, a basic Windows shim, and the M1a review items carried forward.
- **M1b-win (next):** Windows launch fidelity: the raw command-line tail, the Job Object, Ctrl+C, NO-WINDOW and MIRROR, the GUI shim, batch forwarders. It is written after this plan lands, so its code uses the interfaces below.
- **M1c:** parity CI (adapted upstream suites, differential tests, the shim dependency guard, the benchmark).

## Decisions this plan makes

These settle questions the spec leaves open. Each is recorded here so reviewers don't re-litigate it.

1. **Shims do what `pyenv exec` does.** Both upstreams' shims run `pyenv exec <name>`, so rpyenv's shim sets the same environment as `exec` on each OS: `PYENV_VERSION`, `PYENV_ROOT`, `PYENV_DIR` and the `bin` prepend on Linux, and pyenv-win's `PATH` on Windows. Spec §5.1's "environment changes are always empty in M1" refers to distribution extras such as conda's, which arrive in M7.
2. **Windows lookup searches the version folder, `Scripts`, then `bin`.** Spec §5.1 names the first two; pyenv-win also searches `bin`, and the parity policy says resolution rules follow the upstream.
3. **A Windows shim that finds nothing exits 127** with pyenv-win's `which` message, as spec §5.1 requires. pyenv-win's own shims print cmd's "not recognized" and exit 1 (allowlist D-42).
4. **Template freshness is checked by comparing bytes**, not size and hash (spec §8): it needs no hash code in the core, and is stricter.
5. **`rpyenv-core` may use `libc` on Unix.** Spec §3 allows "platform crates" in the core. M1a's plan said `std` only; signal handling for the pip path needs `sigaction`/`kill`/`raise`.
6. **Where the tests that need several binaries live.** Cross-binary tests go in a new test-only package, `crates/e2e`, with the `argv-echo` test program from spec §12. They find `pyenv` and `pyenv-shim` next to `argv-echo` in the target directory. `cargo test --workspace` builds all three, and so does CI's `cargo build --workspace` step.

## Global Constraints

- **Toolchain:** Rust `1.96`, edition 2021, workspace `resolver = "2"`. CI runs `cargo fmt --all --check`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo build --workspace`, then `cargo test --workspace`.
- **CI matrix:** `windows-2025`, `windows-2022`, `ubuntu-latest`, `ubuntu-24.04-arm`.
- **Dependencies:** `rpyenv-core` uses `std`, and `libc = "0.2"` under `[target.'cfg(unix)'.dependencies]`. Nothing else. `pyenv-shim` depends on `rpyenv-core` only (spec §3).
- **Drop-in compatibility:** same `PYENV_ROOT` layout and files; the CLI is `pyenv` (spec D2).
- **Parity policy** (spec §4):
  - Match each OS's upstream in output formats, messages, streams, exit codes, version-file formats and resolution rules.
  - Do **not** reproduce crashes, lost information, file corruption, mis-parsing or help-text typos.
  - Commands pyenv-win lacks follow upstream pyenv on Windows.
  - Every intentional difference gets a row in `docs/parity/allowlist.md`. This plan adds D-29 to D-43.
- **Line endings:** everything rpyenv prints uses `\n` on Linux and `\r\n` on Windows. Command code writes `\n`; `Output::emit` and `Report::emit` convert.
- **Windows streams:** pyenv-win prints its own messages on **stdout**; upstream pyenv prints errors on **stderr**.
- **Naming:** rpyenv-only environment variables use the `RPYENV_` prefix (spec §13). This plan adds none.
- **Arguments are bytes.** Arguments that `exec` and the shims pass on are `OsString`s end to end, never converted to `String` (review M-5).
- **Commits:** one-line `git commit -m "..."` messages, with no Co-Authored-By or other attribution lines. Run `cargo fmt --all` before every commit.
- **Tests that run `pyenv-shim`** (Tasks 9 and 11) need it built: run `cargo build --workspace` before running them one file at a time.

## Review Focus

The five inputs most likely to break this for a real user that the spec doesn't spell out. Each has a test in the task named in brackets.

1. **A shim reachable through `PATH` more than once.** Examples: the shims folder listed twice, or a symlink such as `/usr/local/bin/python -> ~/.pyenv/shims/python`. With `system` selected, the shim must find the real Python and never itself, or it recurses forever. [Task 2: `skip.exe`; Task 11: `system_command_never_finds_the_shim`]
2. **Arguments that are not valid UTF-8, or empty, on Linux.** They must reach the program byte for byte. [Task 10; Task 11: `shim_passes_arguments_byte_for_byte`]
3. **A working directory that was deleted** while a terminal still sits in it. Every command and every shim must keep working, as upstream does from `$PWD` (review M-4). [Task 1; Task 11: `works_in_a_deleted_directory`]
4. **Two rehashes at once**, for example two `pip install` shims exiting together, or a lock left behind by a killed rehash. One must win, the other must skip or wait, and a lock older than two minutes must be broken. [Task 6: lock tests]
5. **Rehash while a Windows shim is running.** A running `.exe` can't be deleted. Rehash must rename it aside and delete it on a later run instead of failing. [Task 11: `win_running_shim_is_renamed_aside`]

---

## File Structure

```
Cargo.toml                                workspace: adds crates/pyenv-shim, crates/e2e
docs/parity/allowlist.md                  rows D-29..D-43
crates/rpyenv-core/
  Cargo.toml                              adds libc on Unix
  src/ctx.rs                              (modify) home, PWD fallback, byte-exact root
  src/prefix.rs                           (modify) PrefixError
  src/select.rs                           (modify) lazy listing; WIN_NO_VERSION moves here
  src/paths.rs                            (modify) win_path_key moves here
  src/pathsearch.rs                       (modify) is_runnable public; find_cmd
  src/lookup.rs                           NEW which, whence, Report (both flavors)
  src/shimset.rs                          NEW which names get shims
  src/rehash.rs                           NEW lock, state, apply
  src/launch.rs                           NEW LaunchPlan, plan(), run()
  src/shim.rs                             NEW the shim program
crates/pyenv-shim/
  Cargo.toml, src/main.rs                 NEW one-line main
  tests/direct.rs                         NEW running the shim binary directly
crates/pyenv/
  src/main.rs, src/lib.rs                 (modify) arguments as OsString; exec dispatch
  src/output.rs                           (modify) From<Report>
  src/help.rs                             (modify) topics for the five new commands
  src/commands/mod.rs                     (modify) rows for exec, rehash, shims, whence, which
  src/commands/which.rs                   NEW which, whence
  src/commands/rehash.rs                  NEW rehash, shims
  src/commands/exec.rs                    NEW exec
  src/commands/versions.rs                (modify) --executables
  src/commands/version.rs, prefix.rs      (modify) PrefixError, moved helpers
  tests/common/mod.rs                     (modify) exe(), shim_exe()
  tests/cli_which.rs                      NEW Task 4
  tests/cli_rehash.rs                     NEW Task 9
  tests/cli_exec.rs                       NEW Task 10
crates/e2e/
  Cargo.toml                              NEW package rpyenv-e2e (test only)
  src/bin/argv_echo.rs                    NEW the argv-echo test program
  tests/common/mod.rs                     NEW fixture with real shims
  tests/shims.rs                          NEW Task 11
```

---

### Task 1: Carry-forward fixes in the core: deleted directory, byte-exact root, typed prefix errors

The M1a final review deferred three items to this plan, because the shims inherit them:
- **M-4:** a deleted working directory makes every command fail.
- **M-5:** `PYENV_ROOT` and `HOME` are converted lossily on Linux.
- **M-6:** `prefix_of` returns preformatted text, and version selection lists `versions/` on every call.

**Files:**
- Modify: `crates/rpyenv-core/src/ctx.rs`
- Modify: `crates/rpyenv-core/src/prefix.rs`
- Modify: `crates/rpyenv-core/src/select.rs`
- Modify: `crates/pyenv/src/commands/prefix.rs:39-45`, `crates/pyenv/src/commands/version.rs:216-230`
- Test: `crates/pyenv/tests/cli_dispatch.rs`

**Interfaces:**
- Produces:
  - `Ctx.home: Option<PathBuf>`: `HOME` when set and non-empty. `Ctx::for_test` sets `None`.
  - `prefix::PrefixError { SystemNotFound, NotInstalled(String) }` with `fn message(&self) -> String`.
  - `prefix::prefix_of(ctx: &Ctx, version: &str) -> Result<PathBuf, PrefixError>`

- [ ] **Step 1: Write the failing core tests**

Add to the `tests` module in `crates/rpyenv-core/src/ctx.rs`:

```rust
    #[test]
    fn deleted_current_directory_falls_back_to_an_absolute_pwd() {
        let gone = || Err(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        let abs = std::env::temp_dir().join("rpyenv-gone");
        assert_eq!(
            current_or_pwd(gone(), Some(abs.clone().into_os_string())),
            Ok(abs.clone())
        );
        assert_eq!(
            current_or_pwd(gone(), Some(OsString::from("relative/dir"))),
            Err(CtxError::NoCurrentDir("gone".to_string()))
        );
        assert_eq!(
            current_or_pwd(gone(), None),
            Err(CtxError::NoCurrentDir("gone".to_string()))
        );
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            current_or_pwd(Ok(here.clone()), Some(abs.into_os_string())),
            Ok(here)
        );
    }

    #[test]
    fn home_is_recorded() {
        let ctx = build(Pyenv, &[("HOME", "/home/u")]).unwrap();
        assert_eq!(ctx.home, Some(PathBuf::from("/home/u")));
        assert_eq!(build(Pyenv, &[("HOME", "")]).unwrap().home, None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_root_and_home_are_kept_byte_for_byte() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let cwd = std::env::current_dir().unwrap();
        let root = OsString::from_vec(b"/tmp/r\xff/".to_vec());
        let get = |k: &str| (k == "PYENV_ROOT").then(|| root.clone());
        let ctx = Ctx::build(Pyenv, &get, cwd.clone(), "").unwrap();
        assert_eq!(ctx.root.as_os_str().as_bytes(), b"/tmp/r\xff");
        let home = OsString::from_vec(b"/home/\xfe".to_vec());
        let get = |k: &str| (k == "HOME").then(|| home.clone());
        let ctx = Ctx::build(Pyenv, &get, cwd, "").unwrap();
        assert_eq!(ctx.root.as_os_str().as_bytes(), b"/home/\xfe/.pyenv");
        assert_eq!(
            ctx.home.as_deref().map(|h| h.as_os_str().as_bytes()),
            Some(&b"/home/\xfe"[..])
        );
    }
```

In `crates/rpyenv-core/src/prefix.rs`, replace the three `Err("...".to_string())` assertions in the existing tests:

```rust
        assert_eq!(
            prefix_of(&ctx, "9.9"),
            Err(PrefixError::NotInstalled("9.9".to_string()))
        );
```

```rust
        assert_eq!(prefix_of(&ctx, "system"), Err(PrefixError::SystemNotFound));
```

```rust
        assert_eq!(
            prefix_of(&ctx, "system"),
            Err(PrefixError::NotInstalled("system".to_string()))
        );
```

Add this test to the same module:

```rust
    #[test]
    fn prefix_error_messages() {
        assert_eq!(
            PrefixError::SystemNotFound.message(),
            "pyenv: system version not found in PATH"
        );
        assert_eq!(
            PrefixError::NotInstalled("3.7".to_string()).message(),
            "pyenv: version `3.7' not installed"
        );
    }
```

- [ ] **Step 2: Write the failing CLI test**

Add to `crates/pyenv/tests/cli_dispatch.rs`:

```rust
/// Review M-4: a terminal can sit in a directory that was deleted. Upstream keeps
/// working from bash's `$PWD`.
#[cfg(unix)]
#[test]
fn works_in_a_deleted_directory() {
    let f = Fixture::new();
    let doomed = f.base.join("doomed");
    std::fs::create_dir_all(&doomed).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            r#"cd "$1" && export PWD && /bin/rmdir "$1" && exec "$2" root"#,
            "sh",
        ])
        .arg(&doomed)
        .arg(env!("CARGO_BIN_EXE_pyenv"))
        .env_clear()
        .env("PYENV_ROOT", &f.root)
        .output()
        .unwrap();
    assert_eq!(
        (String::from_utf8(out.stdout).unwrap(), out.status.code()),
        (format!("{}\n", f.root.display()), Some(0))
    );
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core -- ctx:: prefix::`
Expected: compile errors: `current_or_pwd` not found, no field `home`, `PrefixError` not found.

- [ ] **Step 4: Implement the `Ctx` changes**

In `crates/rpyenv-core/src/ctx.rs`, add the field after `arch_suffix`:

```rust
    /// `HOME` when set and non-empty; upstream's `system` search expands `~` in `PATH` with it.
    pub home: Option<PathBuf>,
```

Replace `from_process`:

```rust
    pub fn from_process() -> Result<Ctx, CtxError> {
        let get = |k: &str| std::env::var_os(k);
        let cwd = current_or_pwd(std::env::current_dir(), get("PWD"))?;
        Ctx::build(Flavor::current(), &get, cwd, host_arch_suffix())
    }
```

In `build`, add `home` to the struct literal (after `arch_suffix,`):

```rust
            home: non_empty("HOME").map(PathBuf::from),
```

In `for_test`, add `home: None,` after `arch_suffix: "",`.

Replace the `Flavor::Pyenv` arm of `discover_root`:

```rust
        // Upstream removes exactly one trailing slash (libexec/pyenv:58-63). The value
        // stays an `OsString`, so a root that isn't UTF-8 still works (review M-5).
        Flavor::Pyenv => match get("PYENV_ROOT") {
            Some(r) => PathBuf::from(strip_one_slash(r)),
            None => {
                let mut s = get("HOME").unwrap_or_default();
                s.push("/.pyenv");
                PathBuf::from(s)
            }
        },
```

Add these functions after `discover_root`:

```rust
#[cfg(unix)]
fn strip_one_slash(s: OsString) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    let mut bytes = s.into_vec();
    if bytes.last() == Some(&b'/') {
        bytes.pop();
    }
    OsString::from_vec(bytes)
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn strip_one_slash(s: OsString) -> OsString {
    let t = s.to_string_lossy();
    OsString::from(t.strip_suffix('/').unwrap_or(&t))
}

/// The physical current directory. When it has been deleted, an absolute `PWD`, which is
/// what bash-based upstream keeps using (review M-4).
fn current_or_pwd(
    cwd: std::io::Result<PathBuf>,
    pwd: Option<OsString>,
) -> Result<PathBuf, CtxError> {
    match cwd {
        Ok(c) => Ok(c),
        Err(e) => match pwd.map(PathBuf::from) {
            Some(p) if p.is_absolute() => Ok(p),
            _ => Err(CtxError::NoCurrentDir(e.to_string())),
        },
    }
}
```

- [ ] **Step 5: Implement `PrefixError`**

In `crates/rpyenv-core/src/prefix.rs`, add after the `use` lines:

```rust
/// Why `prefix_of` failed. `message` gives upstream's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixError {
    /// `system` with no `python`, `python3` or `python2` on `PATH`.
    SystemNotFound,
    /// Not installed. Holds the name after prefix resolution, or `system` when the
    /// system Python is not in a `bin` or `sbin` folder.
    NotInstalled(String),
}

impl PrefixError {
    pub fn message(&self) -> String {
        match self {
            PrefixError::SystemNotFound => "pyenv: system version not found in PATH".to_string(),
            PrefixError::NotInstalled(v) => format!("pyenv: version `{v}' not installed"),
        }
    }
}
```

Replace `prefix_of`:

```rust
/// The installation directory of one version name.
pub fn prefix_of(ctx: &Ctx, version: &str) -> Result<PathBuf, PrefixError> {
    let vdir = ctx.versions_dir();
    if ctx.flavor == Flavor::Pyenv {
        if version == "system" {
            let python = system_python(ctx).ok_or(PrefixError::SystemNotFound)?;
            return match pathsearch::strip_bin(&python) {
                Some(p) if p.is_dir() => Ok(p),
                _ => Err(PrefixError::NotInstalled("system".to_string())),
            };
        }
        // `latest` returns an installed exact name unchanged, so the listing is read
        // only for prefixes. A shim calls this on every launch (review M-6).
        if vdir.join(version).is_dir() {
            return Ok(vdir.join(version));
        }
    }
    let names = installed::names(&vdir, ctx.flavor);
    let resolved = match ctx.flavor {
        Flavor::Pyenv => {
            latest::latest(version, &names, &vdir).unwrap_or_else(|| version.to_string())
        }
        Flavor::PyenvWin => winresolve::resolve(version, &names, ctx.arch_suffix),
    };
    let dir = vdir.join(&resolved);
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(PrefixError::NotInstalled(resolved))
    }
}
```

- [ ] **Step 6: Make `version_name` read the listing only when needed**

In `crates/rpyenv-core/src/select.rs`, in `version_name`, replace:

```rust
    let candidates = installed::names(&vdir, Flavor::Pyenv);
```

with:

```rust
    // Read only when a name needs prefix resolution: shims call this on every launch (review M-6).
    let mut candidates: Option<Vec<String>> = None;
    let mut listing = || -> Vec<String> {
        candidates
            .get_or_insert_with(|| installed::names(&vdir, Flavor::Pyenv))
            .clone()
    };
```

and replace the two `latest::latest(...)` calls in the `accepted` chain:

```rust
        } else if let Some(r) = latest::latest(&normalised, &listing(), &vdir) {
            Some(r)
        } else if normalization_done {
            latest::latest(&v, &listing(), &vdir)
```

- [ ] **Step 7: Update the two CLI call sites**

In `crates/pyenv/src/commands/prefix.rs`, change the match arm:

```rust
            Err(e) => {
                o.err(e.message());
                return o.with_code(1);
            }
```

In `crates/pyenv/src/commands/version.rs`, in `write_checked`:

```rust
            if let Err(e) = prefix::prefix_of(ctx, v) {
                return Output::error(e.message());
            }
```

- [ ] **Step 8: Run all tests**

Run: `cargo test --workspace`
Expected: PASS. On Linux the new count includes `non_utf8_root_and_home_are_kept_byte_for_byte` and `works_in_a_deleted_directory`; on Windows those two are compiled out.

- [ ] **Step 9: Commit**

```bash
git add crates/rpyenv-core crates/pyenv
git commit -m "Keep working in a deleted directory, keep root bytes, type prefix errors"
```

---

### Task 2: Upstream `which` and `whence` in the core

**Files:**
- Create: `crates/rpyenv-core/src/lookup.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (add `pub mod lookup;`)
- Modify: `crates/rpyenv-core/src/pathsearch.rs` (make `is_runnable` public)
- Modify: `docs/parity/allowlist.md` (D-29, D-30)

**Interfaces:**
- Consumes: `select::version_name(ctx, force) -> PyenvNames`, `select::split_colon`, `select::version_origin`, `prefix::prefix_of` (Task 1), `installed::{top_level, envs_of}`, `pathsearch::find_all`.
- Produces (all in `rpyenv_core::lookup`):
  - `struct Report { lines: Vec<String>, stderr: bool, code: i32 }` with `fn emit(&self, flavor: Flavor)`
  - `struct Skip { dirs: Vec<PathBuf>, exe: Option<PathBuf> }` (derives `Default`), with `fn from_env(program: &str, exe: Option<PathBuf>) -> Skip`
  - `struct Found { path: PathBuf, warnings: Vec<String> }`
  - `enum NotFound { Pyenv { missing: Vec<String>, origin: String, warnings: Vec<String> } }` (Task 3 adds variants)
  - `fn shim_paths_var(program: &str) -> String`
  - `fn which_pyenv(ctx: &Ctx, command: &str, nosystem: bool, skip: &Skip) -> Result<Found, NotFound>`
  - `fn whence_pyenv(ctx: &Ctx, command: &str) -> Vec<(String, PathBuf)>`
  - `fn not_found_report(ctx: &Ctx, command: &str, nf: &NotFound, advice: bool) -> Report`
  - `pathsearch::is_runnable(p: &Path) -> bool` (now `pub`)

Upstream's algorithm (reference, "`pyenv which`"):
1. The version list is `PYENV_VERSION` split on `:`, used raw, when set. Otherwise it is `version-name -f`.
2. Each entry is tried in order. `system` searches `PATH` without the shims. Any other entry uses `<prefix>/bin/<command>`, and a version `pyenv-prefix` rejects is recorded as missing.
3. `system` is tried again at the end unless `--nosystem` is given.
4. "Not installed" lines appear only when the command is not found anywhere.

- [ ] **Step 1: Write the failing tests**

Create `crates/rpyenv-core/src/lookup.rs` with only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A file the platform can run: mode 755 on Unix.
    fn exe(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    struct Root {
        tmp: tempfile::TempDir,
    }

    impl Root {
        fn new(versions: &[&str]) -> Root {
            let tmp = tempfile::tempdir().unwrap();
            fs::create_dir_all(tmp.path().join("root").join("versions")).unwrap();
            fs::create_dir_all(tmp.path().join("work")).unwrap();
            for v in versions {
                fs::create_dir_all(tmp.path().join("root").join("versions").join(v)).unwrap();
            }
            Root { tmp }
        }
        fn root(&self) -> PathBuf {
            self.tmp.path().join("root")
        }
        fn bin(&self, version: &str, name: &str) -> PathBuf {
            let p = self.root().join("versions").join(version).join("bin").join(name);
            exe(&p);
            p
        }
        fn ctx(&self, pyenv_version: Option<&str>) -> Ctx {
            let mut ctx = Ctx::for_test(Flavor::Pyenv, &self.root(), &self.tmp.path().join("work"));
            ctx.pyenv_version = pyenv_version.map(String::from);
            ctx
        }
    }

    #[test]
    fn first_version_with_the_command_wins() {
        let r = Root::new(&["3.11.9", "3.12.1"]);
        let tool = r.bin("3.12.1", "tool");
        r.bin("3.11.9", "python");
        let found = which_pyenv(&r.ctx(Some("3.11.9:3.12.1")), "tool", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(tool));
    }

    #[test]
    fn missing_versions_are_silent_when_a_later_one_has_it() {
        let r = Root::new(&["3.12.1"]);
        let tool = r.bin("3.12.1", "tool");
        let found = which_pyenv(&r.ctx(Some("9.9:3.12.1")), "tool", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(tool));
    }

    #[test]
    fn prefixes_resolve_through_prefix_of() {
        let r = Root::new(&["3.12.10"]);
        let py = r.bin("3.12.10", "python");
        let found = which_pyenv(&r.ctx(Some("3.12")), "python", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(py));
    }

    #[test]
    fn not_found_report_matches_upstream() {
        // Reference probe: PYENV_VERSION=9.9:8.8, `tool` in 3.11.9 and in the env 3.12.1/envs/venv1.
        let r = Root::new(&["3.11.9", "3.12.1/envs/venv1"]);
        r.bin("3.11.9", "tool");
        r.bin("3.12.1/envs/venv1", "tool");
        let ctx = r.ctx(Some("9.9:8.8"));
        let nf = which_pyenv(&ctx, "tool", false, &Skip::default()).unwrap_err();
        assert_eq!(
            nf,
            NotFound::Pyenv {
                missing: vec!["9.9".to_string(), "8.8".to_string()],
                origin: "PYENV_VERSION environment variable".to_string(),
                warnings: vec![],
            }
        );
        let report = not_found_report(&ctx, "tool", &nf, true);
        assert_eq!(
            report.lines,
            [
                "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)",
                "pyenv: version `8.8' is not installed (set by PYENV_VERSION environment variable)",
                "pyenv: tool: command not found",
                "",
                "The `tool' command exists in these Python versions:",
                "  3.11.9",
                "  3.12.1/envs/venv1",
                "",
                "Note: See 'pyenv help global' for tips on allowing multiple",
                "      Python versions to be found at the same time.",
            ]
        );
        assert!(report.stderr);
        assert_eq!(report.code, 127);
        let short = not_found_report(&ctx, "tool", &nf, false);
        assert_eq!(short.lines.len(), 3);
    }

    #[test]
    fn no_advice_block_when_no_version_has_it() {
        let r = Root::new(&["3.12.1"]);
        let ctx = r.ctx(Some("3.12.1"));
        let nf = which_pyenv(&ctx, "nothing", true, &Skip::default()).unwrap_err();
        assert_eq!(
            not_found_report(&ctx, "nothing", &nf, true).lines,
            ["pyenv: nothing: command not found"]
        );
    }

    #[test]
    fn a_directory_does_not_count_as_found() {
        // Upstream's `-x` accepts a directory (allowlist D-29).
        let r = Root::new(&["3.12.1"]);
        fs::create_dir_all(r.root().join("versions/3.12.1/bin/tool")).unwrap();
        assert!(which_pyenv(&r.ctx(Some("3.12.1")), "tool", true, &Skip::default()).is_err());
    }

    #[test]
    fn version_file_names_are_normalized() {
        let r = Root::new(&["3.12.10"]);
        let py = r.bin("3.12.10", "python");
        fs::write(r.tmp.path().join("work/.python-version"), "python-3.12\n").unwrap();
        let found = which_pyenv(&r.ctx(None), "python", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(py));
    }

    #[test]
    fn shim_paths_variable_name() {
        assert_eq!(shim_paths_var("python3.12"), "_PYENV_SHIM_PATHS_PYTHON3_12");
        assert_eq!(shim_paths_var("pip-compile"), "_PYENV_SHIM_PATHS_PIP_COMPILE");
    }

    #[test]
    fn whence_lists_versions_and_envs_in_versions_order() {
        let r = Root::new(&["3.11.9", "3.12.1/envs/venv1", "3.12.2"]);
        let a = r.bin("3.11.9", "tool");
        let b = r.bin("3.12.1/envs/venv1", "tool");
        assert_eq!(
            whence_pyenv(&r.ctx(None), "tool"),
            [
                ("3.11.9".to_string(), a),
                ("3.12.1/envs/venv1".to_string(), b)
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn system_search_skips_shims_listed_dirs_home_and_the_shim_binary() {
        use std::os::unix::fs::symlink;
        let r = Root::new(&[]);
        let base = r.tmp.path();
        let shims = r.root().join("shims");
        exe(&shims.join("tool"));
        exe(&base.join("listed/tool"));
        let shim_exe = base.join("pyenv-shim");
        exe(&shim_exe);
        fs::create_dir_all(base.join("links")).unwrap();
        symlink(&shim_exe, base.join("links/tool")).unwrap();
        let real = base.join("home/sys/tool");
        exe(&real);
        let mut ctx = r.ctx(Some("system"));
        ctx.home = Some(base.join("home"));
        let path = format!(
            "{}:{}:{}:~/sys",
            shims.display(),
            base.join("listed").display(),
            base.join("links").display()
        );
        ctx.path = Some(path.into());
        let skip = Skip {
            dirs: vec![base.join("listed")],
            exe: Some(shim_exe),
        };
        let found = which_pyenv(&ctx, "tool", false, &skip).map(|f| f.path);
        assert_eq!(found, Ok(base.join("home").join("sys").join("tool")));
    }

    #[test]
    fn nosystem_leaves_out_the_final_path_search() {
        // With nothing selected, `version-name -f` gives `system`, which is searched at its
        // own position even with `--nosystem`; so select a real version here.
        let r = Root::new(&["3.12.1"]);
        let sys = r.tmp.path().join("sys");
        exe(&sys.join("tool"));
        let mut ctx = r.ctx(Some("3.12.1"));
        ctx.path = Some(sys.clone().into_os_string());
        assert!(which_pyenv(&ctx, "tool", false, &Skip::default()).is_ok());
        assert!(which_pyenv(&ctx, "tool", true, &Skip::default()).is_err());
    }
}
```

Add `pub mod lookup;` to `crates/rpyenv-core/src/lib.rs` (keep the list alphabetical).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core lookup::`
Expected: compile errors: `which_pyenv`, `Skip`, `NotFound` and the others not found.

- [ ] **Step 3: Make `is_runnable` public**

In `crates/rpyenv-core/src/pathsearch.rs`, change both `fn is_runnable` definitions to `pub fn is_runnable` and add this doc comment above the Unix one:

```rust
/// A regular file (following symlinks) that can be run: any execute bit on Unix.
```

- [ ] **Step 4: Implement `lookup.rs`**

Put this above the test module in `crates/rpyenv-core/src/lookup.rs`:

```rust
//! Finding the file a command name runs (`which`), and every version that has it (`whence`).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, pathsearch, prefix, select};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What a failed command prints, on which stream, and its exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub lines: Vec<String>,
    pub stderr: bool,
    pub code: i32,
}

impl Report {
    /// Writes the lines with the flavor's line ending.
    pub fn emit(&self, flavor: Flavor) {
        let text: String = self
            .lines
            .iter()
            .map(|l| format!("{l}{}", flavor.eol()))
            .collect();
        if self.stderr {
            let _ = std::io::stderr().write_all(text.as_bytes());
        } else {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
    }
}

/// What the `system` search leaves out besides `<root>/shims`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skip {
    /// Upstream's `_PYENV_SHIM_PATHS_<PROGRAM>` directories.
    pub dirs: Vec<PathBuf>,
    /// rpyenv's shim binary. A `PATH` hit that resolves to it is a shim, not a system
    /// command, so taking it would recurse (allowlist D-30).
    pub exe: Option<PathBuf>,
}

impl Skip {
    /// Reads `_PYENV_SHIM_PATHS_<PROGRAM>` from the process environment.
    pub fn from_env(program: &str, exe: Option<PathBuf>) -> Skip {
        let dirs = std::env::var_os(shim_paths_var(program))
            .filter(|v| !v.is_empty())
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        Skip { dirs, exe }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// Upstream's `invalid version` lines from reading the version file, for stderr.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotFound {
    /// Upstream: the selected versions that are not installed, then "command not found".
    Pyenv {
        missing: Vec<String>,
        origin: String,
        warnings: Vec<String>,
    },
}

/// The name of upstream's `_PYENV_SHIM_PATHS_<PROGRAM>` variable: the program name
/// uppercased, with every character outside `[A-Z0-9_]` turned into `_`.
pub fn shim_paths_var(program: &str) -> String {
    let mut s = String::from("_PYENV_SHIM_PATHS_");
    s.extend(program.chars().map(|c| {
        let u = c.to_ascii_uppercase();
        if u.is_ascii_alphanumeric() || u == '_' {
            u
        } else {
            '_'
        }
    }));
    s
}

/// Upstream `pyenv which <command> [--nosystem]` (libexec/pyenv-which:65-126).
pub fn which_pyenv(
    ctx: &Ctx,
    command: &str,
    nosystem: bool,
    skip: &Skip,
) -> Result<Found, NotFound> {
    let (versions, warnings) = match &ctx.pyenv_version {
        // Raw entries: prefixes are resolved by `prefix_of`, and `python-` is kept.
        Some(v) => (select::split_colon(v), Vec::new()),
        None => {
            let r = select::version_name(ctx, true);
            (r.names, r.stderr)
        }
    };
    let mut missing = Vec::new();
    for v in &versions {
        if v == "system" {
            if let Some(path) = system_command(ctx, command, skip) {
                return Ok(Found { path, warnings });
            }
            continue;
        }
        match prefix::prefix_of(ctx, v) {
            Ok(dir) => {
                let candidate = dir.join("bin").join(command);
                // Upstream's `-x` also accepts a directory (allowlist D-29).
                if pathsearch::is_runnable(&candidate) {
                    return Ok(Found {
                        path: candidate,
                        warnings,
                    });
                }
            }
            Err(_) => missing.push(v.clone()),
        }
    }
    if !nosystem {
        if let Some(path) = system_command(ctx, command, skip) {
            return Ok(Found { path, warnings });
        }
    }
    Err(NotFound::Pyenv {
        missing,
        origin: select::version_origin(ctx),
        warnings,
    })
}

/// Upstream's `system` search: `PATH` with every `~` replaced by `$HOME`, minus
/// `<root>/shims` and `skip.dirs`, then the first runnable `command`. A hit that resolves
/// to rpyenv's shim binary is passed over (allowlist D-30).
fn system_command(ctx: &Ctx, command: &str, skip: &Skip) -> Option<PathBuf> {
    let path = ctx.path.as_ref()?;
    let shims = ctx.shims_dir();
    let home = ctx.home.clone().unwrap_or_default().into_os_string();
    let dirs: Vec<PathBuf> = std::env::split_paths(path)
        .map(|d| replace_tilde(d.as_os_str(), &home))
        .filter(|d| *d != shims && !skip.dirs.contains(d))
        .collect();
    let path = std::env::join_paths(dirs).ok()?;
    let own = skip.exe.as_ref().and_then(|e| std::fs::canonicalize(e).ok());
    pathsearch::find_all(command, Some(&path), None, ctx.flavor, ctx.pathext.as_deref())
        .into_iter()
        .find(|p| own.is_none() || std::fs::canonicalize(p).ok() != own)
}

#[cfg(unix)]
fn replace_tilde(s: &OsStr, home: &OsStr) -> PathBuf {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut out = Vec::new();
    for &b in s.as_bytes() {
        if b == b'~' {
            out.extend_from_slice(home.as_bytes());
        } else {
            out.push(b);
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn replace_tilde(s: &OsStr, home: &OsStr) -> PathBuf {
    PathBuf::from(s.to_string_lossy().replace('~', &home.to_string_lossy()))
}

/// Upstream `pyenv whence`: each entry of `versions --bare` (envs and aliases included)
/// whose `bin` has a runnable `command`, with that file's path.
pub fn whence_pyenv(ctx: &Ctx, command: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for entry in installed::top_level(&ctx.versions_dir(), Flavor::Pyenv) {
        let envs = installed::envs_of(&entry);
        for e in std::iter::once(entry).chain(envs) {
            let candidate = e.path.join("bin").join(command);
            if pathsearch::is_runnable(&candidate) {
                out.push((e.name, candidate));
            }
        }
    }
    out
}

/// What `which` prints when `command` was not found. `advice` is false for `--skip-advice`.
pub fn not_found_report(ctx: &Ctx, command: &str, nf: &NotFound, advice: bool) -> Report {
    match nf {
        NotFound::Pyenv {
            missing,
            origin,
            warnings,
        } => {
            let mut lines = warnings.clone();
            for m in missing {
                lines.push(format!(
                    "pyenv: version `{m}' is not installed (set by {origin})"
                ));
            }
            lines.push(format!("pyenv: {command}: command not found"));
            let versions = whence_pyenv(ctx, command);
            if advice && !versions.is_empty() {
                lines.push(String::new());
                lines.push(format!(
                    "The `{command}' command exists in these Python versions:"
                ));
                lines.extend(versions.into_iter().map(|(n, _)| format!("  {n}")));
                lines.push(String::new());
                lines.push("Note: See 'pyenv help global' for tips on allowing multiple".into());
                lines.push("      Python versions to be found at the same time.".into());
            }
            Report {
                lines,
                stderr: true,
                code: 127,
            }
        }
    }
}
```

- [ ] **Step 5: Add the allowlist rows**

Append to the table in `docs/parity/allowlist.md`:

```markdown
| D-29 | Linux | `which`, `whence`, `exec`, shims | A directory named like the command in a version's `bin` counts as found (`-x`) | Only runnable regular files count | Running a directory fails; mis-parsing. |
| D-30 | Linux | `which`, `exec`, shims (`system`) | Removes exact `${PYENV_ROOT}/shims` entries and `_PYENV_SHIM_PATHS_<PROGRAM>` dirs from `PATH` | Also removes the shims dir spelled with a trailing `/`, and passes over any hit that resolves to rpyenv's shim binary | A symlink to a shim elsewhere on `PATH` would otherwise make the shim run itself forever. |
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -p rpyenv-core lookup::`
Expected: PASS. 10 tests on Windows; 11 on Linux (`system_search_skips_…` is Unix-only).

- [ ] **Step 7: Commit**

```bash
git add crates/rpyenv-core docs/parity/allowlist.md
git commit -m "Add upstream which and whence to the core"
```

---

### Task 3: pyenv-win `which` and `whence` in the core

**Files:**
- Modify: `crates/rpyenv-core/src/lookup.rs`
- Modify: `crates/rpyenv-core/src/select.rs` (receives `WIN_NO_VERSION`)
- Modify: `crates/pyenv/src/commands/version.rs` (uses the moved constant)
- Modify: `docs/parity/allowlist.md` (D-31, D-32)

**Interfaces:**
- Consumes: `select::win_select(ctx) -> Vec<WinSelected>`, `installed::names`, Task 2's `Report`, `Found`, `NotFound`.
- Produces:
  - `select::WIN_NO_VERSION: [&str; 5]` (moved from `crates/pyenv/src/commands/version.rs`)
  - `NotFound::WinNoVersion`, `NotFound::WinNotInstalled(String)`, `NotFound::WinNotFound`
  - `lookup::win_extensions(pathext: Option<&OsStr>) -> Vec<String>`
  - `lookup::which_win(ctx: &Ctx, command: &str) -> Result<Found, NotFound>`
  - `lookup::whence_win(ctx: &Ctx, program: &str, with_path: bool) -> Vec<String>`
  - `not_found_report` handles the three new variants

pyenv-win's rules (reference, "which" and "whence"):
- One trailing `.` is dropped from the program name.
- The extensions are `PATHEXT` as written, with `.PY` and `.PYW` added.
- `which` walks the selected versions. A version that isn't installed stops the search with `(set by <V>)`, which repeats the name. In each version it checks the folder, `Scripts`, then `bin`: the bare name first, then each extension.
- `whence` ignores the selection and walks every installed version.

- [ ] **Step 1: Write the failing tests**

Add to the test module in `lookup.rs`:

```rust
    fn win_root(versions: &[&str]) -> (tempfile::TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        for v in versions {
            fs::create_dir_all(root.join("versions").join(v)).unwrap();
        }
        fs::create_dir_all(root.join("versions")).unwrap();
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let mut ctx = Ctx::for_test(Flavor::PyenvWin, &root, &tmp.path().join("work"));
        // Lowercase, so the tests behave the same on case-sensitive file systems.
        ctx.pathext = Some(".exe;.bat".into());
        (tmp, ctx)
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
    }

    #[test]
    fn win_extension_list() {
        assert_eq!(
            win_extensions(Some(OsStr::new(".COM;.EXE"))),
            [".COM", ".EXE", ".PY", ".PYW"]
        );
        assert_eq!(
            win_extensions(Some(OsStr::new(".py;;.EXE"))),
            [".py", ".EXE", ".PYW"]
        );
        assert_eq!(win_extensions(None), ["", ".PY", ".PYW"]);
    }

    #[test]
    fn win_which_walks_selected_versions_folder_scripts_bin() {
        let (_t, mut ctx) = win_root(&["3.8.2", "3.9.1"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.8.2/python38.exe"));
        touch(&v.join("3.9.1/Scripts/pip.exe"));
        touch(&v.join("3.9.1/bin/pip.exe"));
        touch(&v.join("3.9.1/bin/tool.bat"));
        ctx.pyenv_version = Some("3.9.1 3.8.2".to_string());
        let path = |c: &str| which_win(&ctx, c).map(|f| f.path);
        assert_eq!(path("python38"), Ok(v.join("3.8.2").join("python38.exe")));
        assert_eq!(path("pip"), Ok(v.join("3.9.1").join("Scripts").join("pip.exe")));
        assert_eq!(path("tool"), Ok(v.join("3.9.1").join("bin").join("tool.bat")));
        // One trailing dot is dropped.
        assert_eq!(path("pip."), Ok(v.join("3.9.1").join("Scripts").join("pip.exe")));
    }

    #[test]
    fn win_which_bare_name_before_extensions() {
        let (_t, mut ctx) = win_root(&["3.9.1"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.9.1/tool"));
        touch(&v.join("3.9.1/tool.exe"));
        ctx.pyenv_version = Some("3.9.1".to_string());
        assert_eq!(
            which_win(&ctx, "tool").map(|f| f.path),
            Ok(v.join("3.9.1").join("tool"))
        );
    }

    #[test]
    fn win_which_failures_and_reports() {
        let (_t, mut ctx) = win_root(&["3.8.2", "3.8.6"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.8.2/python38.exe"));
        touch(&v.join("3.8.6/python38.exe"));
        assert_eq!(which_win(&ctx, "python"), Err(NotFound::WinNoVersion));
        let r = not_found_report(&ctx, "python", &NotFound::WinNoVersion, true);
        assert_eq!((r.lines.len(), r.stderr, r.code), (5, false, 1));

        ctx.pyenv_version = Some("3.7.7".to_string());
        let nf = which_win(&ctx, "python").unwrap_err();
        assert_eq!(nf, NotFound::WinNotInstalled("3.7.7".to_string()));
        assert_eq!(
            not_found_report(&ctx, "python", &nf, true).lines,
            ["pyenv: version '3.7.7' is not installed (set by 3.7.7)"]
        );

        ctx.pyenv_version = Some("3.8.2".to_string());
        fs::remove_file(v.join("3.8.2/python38.exe")).unwrap();
        let nf = which_win(&ctx, "python38").unwrap_err();
        assert_eq!(nf, NotFound::WinNotFound);
        let r = not_found_report(&ctx, "python38", &nf, true);
        assert_eq!(
            r.lines,
            [
                "pyenv: python38: command not found",
                "",
                "The 'python38' command exists in these Python versions:",
                "  3.8.6",
                "  ",
            ]
        );
        assert_eq!((r.stderr, r.code), (false, 127));
        assert_eq!(
            not_found_report(&ctx, "unknown3.8", &NotFound::WinNotFound, true).lines,
            ["pyenv: unknown3.8: command not found"]
        );
    }

    #[test]
    fn win_whence_names_once_and_paths_in_search_order() {
        let (_t, ctx) = win_root(&["3.8.2"]);
        let v = ctx.versions_dir().join("3.8.2");
        touch(&v.join("foo.exe"));
        touch(&v.join("Scripts/foo.exe"));
        touch(&v.join("bin/foo.exe"));
        // pyenv-win prints `3.8.2` twice here (allowlist D-32).
        assert_eq!(whence_win(&ctx, "foo", false), ["3.8.2"]);
        assert_eq!(
            whence_win(&ctx, "foo", true),
            [
                v.join("foo.exe").display().to_string(),
                v.join("Scripts").join("foo.exe").display().to_string(),
                v.join("bin").join("foo.exe").display().to_string(),
            ]
        );
        assert!(whence_win(&ctx, "bar", false).is_empty());
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core lookup::`
Expected: compile errors: `win_extensions`, `which_win`, `whence_win` and the new variants not found.

- [ ] **Step 3: Move `WIN_NO_VERSION` into the core**

Cut the `WIN_NO_VERSION` constant, with its doc comment, from `crates/pyenv/src/commands/version.rs`, and paste it into `crates/rpyenv-core/src/select.rs` after the `use` lines. In `version.rs`, add this import in its place, so `win_no_version()` and the existing uses keep compiling:

```rust
pub use rpyenv_core::select::WIN_NO_VERSION;
```

- [ ] **Step 4: Implement**

In `lookup.rs`, add the variants to `NotFound`:

```rust
    /// pyenv-win: nothing is selected.
    WinNoVersion,
    /// pyenv-win: a selected version that is not installed stops the search.
    WinNotInstalled(String),
    /// pyenv-win: no selected version has it.
    WinNotFound,
```

Add the functions (after `whence_pyenv`):

```rust
/// pyenv-win `GetExtensions(True)`: `PATHEXT` entries as written, in order, then `.PY`
/// and `.PYW` unless present. An empty `PATHEXT` gives one empty extension first.
pub fn win_extensions(pathext: Option<&OsStr>) -> Vec<String> {
    let raw = pathext
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut exts: Vec<String> = raw
        .split(';')
        .filter(|e| !e.is_empty())
        .map(String::from)
        .collect();
    if exts.is_empty() {
        exts.push(String::new());
    }
    for add in [".PY", ".PYW"] {
        if !exts.iter().any(|e| e.eq_ignore_ascii_case(add)) {
            exts.push(add.to_string());
        }
    }
    exts
}

/// The files pyenv-win checks in one version: in the folder, then `Scripts`, then `bin`,
/// the bare name and then the first extension that exists. Every hit, in that order.
/// Names match without case, as on NTFS, and each hit carries the file's on-disk name.
fn win_hits(version_dir: &Path, program: &str, exts: &[String]) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    for dir in [
        version_dir.to_path_buf(),
        version_dir.join("Scripts"),
        version_dir.join("bin"),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let files: Vec<std::ffi::OsString> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name())
            .collect();
        let find = |want: &str| {
            files
                .iter()
                .find(|f| f.to_string_lossy().eq_ignore_ascii_case(want))
                .map(|f| dir.join(f))
        };
        if let Some(p) = find(program) {
            hits.push(p);
        }
        if let Some(p) = exts.iter().find_map(|e| find(&format!("{program}{e}"))) {
            hits.push(p);
        }
    }
    hits
}

/// pyenv-win `CommandWhich` (pyenv.vbs:101-169). The folders in the printed path are
/// as built from the root; only the file name is in its on-disk case (allowlist D-31).
pub fn which_win(ctx: &Ctx, command: &str) -> Result<Found, NotFound> {
    let program = command.strip_suffix('.').unwrap_or(command);
    let selected = select::win_select(ctx);
    if selected.is_empty() {
        return Err(NotFound::WinNoVersion);
    }
    let exts = win_extensions(ctx.pathext.as_deref());
    for s in &selected {
        let dir = ctx.versions_dir().join(&s.name);
        if !dir.is_dir() {
            return Err(NotFound::WinNotInstalled(s.name.clone()));
        }
        if let Some(path) = win_hits(&dir, program, &exts).into_iter().next() {
            return Ok(Found {
                path,
                warnings: Vec::new(),
            });
        }
    }
    Err(NotFound::WinNotFound)
}

/// pyenv-win `CommandWhence` (pyenv.vbs:171-261): every installed version, selection
/// ignored. Without `--path`, each version that has the program is listed once
/// (allowlist D-32); with `--path`, every hit.
pub fn whence_win(ctx: &Ctx, program: &str, with_path: bool) -> Vec<String> {
    let exts = win_extensions(ctx.pathext.as_deref());
    let mut out = Vec::new();
    for name in installed::names(&ctx.versions_dir(), Flavor::PyenvWin) {
        let hits = win_hits(&ctx.versions_dir().join(&name), program, &exts);
        if with_path {
            out.extend(hits.iter().map(|h| h.display().to_string()));
        } else if !hits.is_empty() {
            out.push(name);
        }
    }
    out
}
```

Add these arms to the `match nf` in `not_found_report`:

```rust
        NotFound::WinNoVersion => Report {
            lines: select::WIN_NO_VERSION.iter().map(|l| l.to_string()).collect(),
            stderr: false,
            code: 1,
        },
        // pyenv-win repeats the name where the origin would go (pyenv.vbs:125).
        NotFound::WinNotInstalled(v) => Report {
            lines: vec![format!("pyenv: version '{v}' is not installed (set by {v})")],
            stderr: false,
            code: 1,
        },
        NotFound::WinNotFound => {
            let program = command.strip_suffix('.').unwrap_or(command);
            let mut lines = vec![format!("pyenv: {command}: command not found")];
            let versions = whence_win(ctx, program, false);
            if !versions.is_empty() {
                lines.push(String::new());
                lines.push(format!(
                    "The '{command}' command exists in these Python versions:"
                ));
                lines.extend(versions.iter().map(|v| format!("  {v}")));
                // pyenv-win indents the CRLF that ends whence's output, too.
                lines.push("  ".to_string());
            }
            Report {
                lines,
                stderr: false,
                code: 127,
            }
        }
```

- [ ] **Step 5: Add the allowlist rows**

```markdown
| D-31 | Windows | `which`, `whence --path` | Every path component is printed in on-disk letter case (`GetFile().Path`) | The file name is in on-disk case; the folders are printed as built from the root | Cosmetic; the same file. |
| D-32 | Windows | `whence` | A version with the command in both `Scripts` and `bin` is listed twice | Each version is listed once | Duplicate output. |
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS, including the 5 new `lookup::` tests on both OSes.

- [ ] **Step 7: Commit**

```bash
git add crates/rpyenv-core crates/pyenv docs/parity/allowlist.md
git commit -m "Add pyenv-win which and whence to the core"
```

---
### Task 4: `pyenv which` and `pyenv whence`

**Files:**
- Create: `crates/pyenv/src/commands/which.rs`
- Modify: `crates/pyenv/src/commands/mod.rs` (module and two rows)
- Modify: `crates/pyenv/src/lib.rs` (`shim_exe`)
- Modify: `crates/pyenv/src/output.rs` (`From<Report>`)
- Modify: `crates/pyenv/src/help.rs` (topics)
- Modify: `crates/pyenv/tests/common/mod.rs` (`exe`)
- Modify: `crates/pyenv/tests/cli_prefix_versions.rs` (`win_commands_order`)
- Test: `crates/pyenv/tests/cli_which.rs`

**Interfaces:**
- Consumes: Task 2 and 3's `lookup::{which_pyenv, which_win, whence_pyenv, whence_win, not_found_report, Skip, Found, NotFound, Report}`.
- Produces:
  - `commands::which::{which, whence}` (both `Command`)
  - `commands::which::print_help(name: &str) -> Output`: pyenv-win's `PrintHelp` (help text, one more line ending, exit 1)
  - `crate::shim_exe() -> Option<PathBuf>`: `pyenv-shim[.exe]` next to the running `pyenv`
  - `impl From<lookup::Report> for Output`
  - Test fixture: `Fixture::exe(&self, rel: &str) -> PathBuf` makes a runnable file at `root/versions/<rel>` (`rel` is `/`-separated).

Behavior (reference sections "`pyenv which`", "`pyenv whence`", pyenv-win "which", "whence"):
- **Linux `which`:**
  - The command is `$1`, even when it looks like a flag.
  - `--nosystem` and `--skip-advice` are recognized anywhere.
  - With no command, it prints the usage on stderr and exits 1.
- **Windows `which`:** with no program, or an empty one, it prints the help text plus one more CRLF on stdout and exits 1.
- **`whence`:**
  - `--path` is recognized only as the first argument.
  - With no command, Linux prints the usage on stderr and exits 1; Windows prints the help plus one more CRLF on stdout and exits 1.
  - When nothing is printed, both exit 1.

- [ ] **Step 1: Add the fixture helper**

Add to `impl Fixture` in `crates/pyenv/tests/common/mod.rs`:

```rust
    /// A runnable file at `root/versions/<rel>` (`rel` is `/`-separated; mode 755 on Unix).
    pub fn exe(&self, rel: &str) -> PathBuf {
        let p = rel
            .split('/')
            .fold(self.root.join("versions"), |p, c| p.join(c));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }
```

- [ ] **Step 2: Write the failing tests**

Create `crates/pyenv/tests/cli_which.rs`:

```rust
mod common;
use common::Fixture;

#[cfg(unix)]
#[test]
fn which_prints_the_path() {
    let f = Fixture::new();
    let tool = f.exe("3.12.1/bin/tool");
    let r = f.pyenv_env(&["which", "tool"], &[("PYENV_VERSION", "3.12.1")]);
    assert_eq!(
        (r.stdout, r.stderr, r.code),
        (format!("{}\n", tool.display()), String::new(), 0)
    );
}

#[cfg(unix)]
#[test]
fn which_not_found_matches_upstream() {
    let f = Fixture::new();
    f.exe("3.11.9/bin/tool");
    f.exe("3.12.1/envs/venv1/bin/tool");
    let r = f.pyenv_env(&["which", "tool"], &[("PYENV_VERSION", "9.9:8.8")]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n\
         pyenv: version `8.8' is not installed (set by PYENV_VERSION environment variable)\n\
         pyenv: tool: command not found\n\
         \n\
         The `tool' command exists in these Python versions:\n  \
         3.11.9\n  \
         3.12.1/envs/venv1\n\
         \n\
         Note: See 'pyenv help global' for tips on allowing multiple\n      \
         Python versions to be found at the same time.\n"
    );
    assert_eq!(r.code, 127);
    let r = f.pyenv_env(
        &["which", "tool", "--skip-advice"],
        &[("PYENV_VERSION", "9.9")],
    );
    assert_eq!(
        r.stderr,
        "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n\
         pyenv: tool: command not found\n"
    );
}

#[cfg(unix)]
#[test]
fn which_usage_and_a_flag_in_command_position() {
    let f = Fixture::new();
    let r = f.pyenv(&["which"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Usage: pyenv which <command> [--nosystem] [--skip-advice]\n", 1)
    );
    // Reference probe: the command is always the first argument.
    let r = f.pyenv(&["which", "--nosystem", "ls"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: --nosystem: command not found\n", 127)
    );
}

#[cfg(unix)]
#[test]
fn whence_names_paths_and_no_match() {
    let f = Fixture::new();
    let a = f.exe("3.11.9/bin/python");
    let b = f.exe("3.12.1/bin/python");
    assert_eq!(f.pyenv(&["whence", "python"]).stdout, "3.11.9\n3.12.1\n");
    assert_eq!(
        f.pyenv(&["whence", "--path", "python"]).stdout,
        format!("{}\n{}\n", a.display(), b.display())
    );
    let r = f.pyenv(&["whence", "nothing"]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 1));
    let r = f.pyenv(&["whence", "--path"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Usage: pyenv whence [--path] <command>\n", 1)
    );
}

#[cfg(unix)]
#[test]
fn help_for_which_and_whence() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["help", "which"]).stdout,
        "Usage: pyenv which <command> [--nosystem] [--skip-advice]\n\n\
         Displays the full path to the executable that pyenv will invoke when\n\
         you run the given command.\n\
         Use --nosystem argument in case when you don't need to search command in the \n\
         system environment.\n\
         Internal switch --skip-advice used to skip printing an error message on a\n\
         failed search.\n\n"
    );
    assert_eq!(
        f.pyenv(&["help", "--usage", "whence"]).stdout,
        "Usage: pyenv whence [--path] <command>\n"
    );
    let listing = f.pyenv(&["help"]).stdout;
    assert!(listing.contains("   which       Display the full path to an executable\n"));
    assert!(listing.contains("   whence      List all Python versions that contain the given executable\n"));
}

#[cfg(windows)]
const WIN_ENV: [(&str, &str); 2] = [
    ("PYENV_VERSION", "3.9.1"),
    ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
];

#[cfg(windows)]
#[test]
fn win_which_and_not_found() {
    let f = Fixture::new();
    let py = f.exe("3.9.1/python.exe");
    f.exe("3.8.2/python38.exe");
    let r = f.pyenv_env(&["which", "python"], &WIN_ENV);
    assert_eq!((r.stdout, r.code), (format!("{}\r\n", py.display()), 0));
    let r = f.pyenv_env(&["which", "python38"], &WIN_ENV);
    assert_eq!(
        r.stdout,
        "pyenv: python38: command not found\r\n\r\n\
         The 'python38' command exists in these Python versions:\r\n  \
         3.8.2\r\n  \r\n"
    );
    assert_eq!(r.code, 127);
}

#[cfg(windows)]
#[test]
fn win_which_and_whence_without_a_program_print_help() {
    let f = Fixture::new();
    let r = f.pyenv(&["which"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "Usage: pyenv which <command>\r\n\r\nShows the full path of the executable\r\n\
             selected. To obtain the full path, use `pyenv which pip'.\r\n\r\n",
            1
        )
    );
    let r = f.pyenv(&["whence", "--path"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "Usage: pyenv whence [--path] <command>\r\n\r\nShows the currently given executable contains path\r\n\
             selected. To obtain python version of executable, use `pyenv whence pip'.\r\n\r\n",
            1
        )
    );
}

#[cfg(windows)]
#[test]
fn win_whence() {
    let f = Fixture::new();
    for v in ["3.8.2", "3.8.7", "3.9.1"] {
        f.exe(&format!("{v}/python.exe"));
    }
    f.exe("3.8.2/python38.exe");
    f.exe("3.8.7/python38.exe");
    assert_eq!(
        f.pyenv_env(&["whence", "python38"], &WIN_ENV).stdout,
        "3.8.2\r\n3.8.7\r\n"
    );
    let r = f.pyenv_env(&["whence", "unknown3.8"], &WIN_ENV);
    assert_eq!((r.stdout.as_str(), r.code), ("", 1));
}
```

In `crates/pyenv/tests/cli_prefix_versions.rs`, update `win_commands_order`'s expected string to:

```rust
        "--version\r\ncommands\r\nglobal\r\nhelp\r\nlocal\r\nprefix\r\nroot\r\nversion-file-read\r\nversion-file-write\r\nversion-file\r\nversion-name\r\nversion-origin\r\nversion\r\nversions\r\nvname\r\nwhence\r\nwhich\r\n"
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p pyenv --test cli_which`
Expected: FAIL. `pyenv which` prints ``pyenv: no such command `which'`` (Linux) or `pyenv: no such command 'which'` (Windows).

- [ ] **Step 4: Add `shim_exe` and `From<Report>`**

In `crates/pyenv/src/lib.rs`, add `use std::path::PathBuf;` and:

```rust
/// The shim binary installed next to this `pyenv` binary. It may not exist.
pub(crate) fn shim_exe() -> Option<PathBuf> {
    let name = format!("pyenv-shim{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe().ok().map(|e| e.with_file_name(name))
}
```

Add to `crates/pyenv/src/output.rs`:

```rust
impl From<rpyenv_core::lookup::Report> for Output {
    fn from(r: rpyenv_core::lookup::Report) -> Output {
        let mut o = Output::new();
        for line in &r.lines {
            if r.stderr {
                o.err(line);
            } else {
                o.out(line);
            }
        }
        o.with_code(r.code)
    }
}
```

- [ ] **Step 5: Implement the commands**

Create `crates/pyenv/src/commands/which.rs`:

```rust
//! `which` and `whence`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::lookup::{self, Found, NotFound, Skip};

pub fn which(ctx: &Ctx, args: &[&str]) -> Output {
    let command = args.first().copied().filter(|c| !c.is_empty());
    match (ctx.flavor, command) {
        (Flavor::Pyenv, None) => {
            Output::error("Usage: pyenv which <command> [--nosystem] [--skip-advice]")
        }
        (Flavor::PyenvWin, None) => print_help("which"),
        // Upstream takes the command from the first argument even when it is a flag, and
        // recognizes the flags anywhere (libexec/pyenv-which:24-41).
        (Flavor::Pyenv, Some(c)) => {
            let nosystem = args.contains(&"--nosystem");
            let advice = !args.contains(&"--skip-advice");
            let skip = Skip::from_env(c, crate::shim_exe());
            found_or_report(ctx, c, lookup::which_pyenv(ctx, c, nosystem, &skip), advice)
        }
        (Flavor::PyenvWin, Some(c)) => found_or_report(ctx, c, lookup::which_win(ctx, c), true),
    }
}

fn found_or_report(
    ctx: &Ctx,
    command: &str,
    result: Result<Found, NotFound>,
    advice: bool,
) -> Output {
    match result {
        Ok(found) => {
            let mut o = Output::new();
            for w in &found.warnings {
                o.err(w);
            }
            o.out(found.path.display().to_string());
            o
        }
        Err(nf) => lookup::not_found_report(ctx, command, &nf, advice).into(),
    }
}

pub fn whence(ctx: &Ctx, args: &[&str]) -> Output {
    let (with_path, rest) = match args.split_first() {
        Some((&"--path", rest)) => (true, rest),
        _ => (false, args),
    };
    let Some(command) = rest.first().copied().filter(|c| !c.is_empty()) else {
        return match ctx.flavor {
            Flavor::Pyenv => Output::error("Usage: pyenv whence [--path] <command>"),
            Flavor::PyenvWin => print_help("whence"),
        };
    };
    let lines: Vec<String> = match ctx.flavor {
        Flavor::Pyenv => lookup::whence_pyenv(ctx, command)
            .into_iter()
            .map(|(name, path)| {
                if with_path {
                    path.display().to_string()
                } else {
                    name
                }
            })
            .collect(),
        Flavor::PyenvWin => {
            let program = command.strip_suffix('.').unwrap_or(command);
            lookup::whence_win(ctx, program, with_path)
        }
    };
    let mut o = Output::new();
    for line in &lines {
        o.out(line);
    }
    // Both upstreams exit 1 when nothing was printed.
    if lines.is_empty() {
        o.with_code(1)
    } else {
        o
    }
}

/// pyenv-win `PrintHelp`: the command's help text, one more line ending, exit 1.
pub fn print_help(name: &str) -> Output {
    let mut o = crate::help::help_command(Flavor::PyenvWin, &[name]);
    o.stdout.push('\n');
    o.with_code(1)
}
```

In `crates/pyenv/src/commands/mod.rs`, add `pub mod which;` to the module list and these rows at the end of `COMMANDS`:

```rust
    ("whence", which::whence),
    ("which", which::which),
```

- [ ] **Step 6: Add the help topics**

In `crates/pyenv/src/help.rs`, add to the end of `PYENV`:

```rust
    topic("whence", Some("List all Python versions that contain the given executable"), Some("Usage: pyenv whence [--path] <command>"),
        "Usage: pyenv whence [--path] <command>\n\nList all Python versions that contain the given executable\n\n"),
    topic("which", Some("Display the full path to an executable"), Some("Usage: pyenv which <command> [--nosystem] [--skip-advice]"),
        "Usage: pyenv which <command> [--nosystem] [--skip-advice]\n\nDisplays the full path to the executable that pyenv will invoke when\nyou run the given command.\nUse --nosystem argument in case when you don't need to search command in the \nsystem environment.\nInternal switch --skip-advice used to skip printing an error message on a\nfailed search.\n\n"),
```

and to the end of `PYENV_WIN`:

```rust
    topic("whence", None, None,
        "Usage: pyenv whence [--path] <command>\n\nShows the currently given executable contains path\nselected. To obtain python version of executable, use `pyenv whence pip'.\n"),
    topic("which", None, None,
        "Usage: pyenv which <command>\n\nShows the full path of the executable\nselected. To obtain the full path, use `pyenv which pip'.\n"),
```

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo test -p pyenv`
Expected: PASS. `cli_which` runs 5 tests on Linux and 3 on Windows.

- [ ] **Step 8: Commit**

```bash
git add crates/pyenv
git commit -m "Add which and whence commands"
```

---

### Task 5: Which names get shims, and `versions --executables`

**Files:**
- Create: `crates/rpyenv-core/src/shimset.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod shimset;`)
- Modify: `crates/pyenv/src/commands/versions.rs`
- Modify: `docs/parity/allowlist.md` (D-33, D-34)
- Test: `crates/pyenv/tests/cli_prefix_versions.rs`

**Interfaces:**
- Consumes: `installed::{top_level, envs_of}`, `pathsearch::is_runnable`.
- Produces:
  - `shimset::is_activation(file_name: &str) -> bool`
  - `shimset::executables_pyenv(versions_dir: &Path) -> Vec<OsString>`: sorted, no duplicates
  - `shimset::shims_win(versions_dir: &Path) -> Vec<String>`: shim file names such as `python.exe`, sorted without case
  - `shimset::wanted(ctx: &Ctx) -> Vec<OsString>`: the shim file names for the context's flavor

Rules (spec §8):
- **Never shimmed, on either OS:** activation scripts, meaning any file whose name without its last extension is `activate` or `deactivate`.
- **Linux:** every runnable regular file in `versions/*/bin` and `versions/*/envs/*/bin`, except names starting with `.`.
- **Windows:** every `.exe`, `.bat` and `.cmd` in a version's folder, `Scripts` and `bin`. Each gets a shim named `<stem>.exe`.

- [ ] **Step 1: Write the failing tests**

Create `crates/rpyenv-core/src/shimset.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file(p: &Path, runnable: bool) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if runnable { 0o755 } else { 0o644 };
            fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = runnable;
    }

    #[test]
    fn activation_scripts() {
        for n in [
            "activate",
            "activate.csh",
            "activate.fish",
            "activate.nu",
            "activate.bat",
            "deactivate.bat",
            "Activate.ps1",
        ] {
            assert!(is_activation(n), "{n}");
        }
        for n in ["activate_this.py", "python", "pip3", "deactivated"] {
            assert!(!is_activation(n), "{n}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn linux_names_are_runnable_files_in_bin_and_env_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        file(&v.join("3.12.1/bin/python"), true);
        file(&v.join("3.12.1/bin/has space"), true);
        file(&v.join("3.12.1/bin/readme"), false);
        file(&v.join("3.12.1/bin/.hidden"), true);
        file(&v.join("3.12.1/bin/activate"), true);
        fs::create_dir_all(v.join("3.12.1/bin/subdir")).unwrap();
        file(&v.join("3.12.1/envs/e1/bin/black"), true);
        file(&v.join("3.11.9/bin/python"), true);
        file(&v.join("3.11.9/lib/x/bin/deep"), true);
        assert_eq!(
            executables_pyenv(&v),
            ["black", "has space", "python"].map(OsString::from)
        );
    }

    #[test]
    fn windows_shims_for_exe_bat_cmd_in_folder_scripts_and_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for f in [
            "3.9.1/python.exe",
            "3.9.1/python.bat",
            "3.9.1/pythonw.exe",
            "3.9.1/Scripts/pip.exe",
            "3.9.1/Scripts/hello.cmd",
            "3.9.1/Scripts/activate.bat",
            "3.9.1/Scripts/tool.py",
            "3.9.1/Scripts/.hook.bat",
            "3.9.1/bin/extra.EXE",
            "3.9.1/lib/deep.exe",
        ] {
            file(&v.join(f), true);
        }
        assert_eq!(
            shims_win(&v),
            ["extra.exe", "hello.exe", "pip.exe", "python.exe", "pythonw.exe"]
        );
    }
}
```

Add `pub mod shimset;` to `crates/rpyenv-core/src/lib.rs`.

Add to `crates/pyenv/tests/cli_prefix_versions.rs`:

```rust
#[cfg(unix)]
#[test]
fn versions_executables() {
    let f = Fixture::new();
    f.exe("3.12.1/bin/python");
    f.exe("3.12.1/bin/pip");
    f.exe("3.11.9/bin/python");
    let r = f.pyenv(&["versions", "--executables"]);
    assert_eq!((r.stdout.as_str(), r.code), ("pip\npython\n", 0));
    // Arguments after --executables are not parsed.
    assert_eq!(f.pyenv(&["versions", "--executables", "--bogus"]).code, 0);
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core shimset::`
Expected: compile errors: `is_activation`, `executables_pyenv`, `shims_win` not found.

- [ ] **Step 3: Implement**

Put this above the tests in `shimset.rs`:

```rust
//! Which command names get a shim (spec §8).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, pathsearch};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

/// Activation scripts must run inside the current shell, so they never get a shim: any
/// file whose name without its last extension is `activate` or `deactivate` (allowlist D-34).
pub fn is_activation(file_name: &str) -> bool {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    stem.eq_ignore_ascii_case("activate") || stem.eq_ignore_ascii_case("deactivate")
}

/// Upstream `pyenv versions --executables`, filtered: the runnable files in every
/// `versions/*/bin` and `versions/*/envs/*/bin`, without dotfiles or activation scripts.
/// Byte order, no duplicates (allowlist D-33).
pub fn executables_pyenv(versions_dir: &Path) -> Vec<OsString> {
    let mut names = BTreeSet::new();
    for entry in installed::top_level(versions_dir, Flavor::Pyenv) {
        let envs = installed::envs_of(&entry);
        for e in std::iter::once(entry).chain(envs) {
            let Ok(rd) = std::fs::read_dir(e.path.join("bin")) else {
                continue;
            };
            for f in rd.filter_map(Result::ok) {
                let name = f.file_name();
                let text = name.to_string_lossy();
                if !text.starts_with('.')
                    && !is_activation(&text)
                    && pathsearch::is_runnable(&f.path())
                {
                    names.insert(name);
                }
            }
        }
    }
    names.into_iter().collect()
}

/// pyenv-win's layout: `<stem>.exe` for every `.exe`, `.bat` and `.cmd` in a version's
/// folder, `Scripts` and `bin`. Names compare without case; the first spelling found wins.
pub fn shims_win(versions_dir: &Path) -> Vec<String> {
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
    for entry in installed::top_level(versions_dir, Flavor::PyenvWin) {
        for dir in [
            entry.path.clone(),
            entry.path.join("Scripts"),
            entry.path.join("bin"),
        ] {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for f in rd.filter_map(Result::ok) {
                let path = f.path();
                let (Some(stem), Some(ext)) = (
                    path.file_stem().and_then(|s| s.to_str()),
                    path.extension().and_then(|s| s.to_str()),
                ) else {
                    continue;
                };
                let ext = ext.to_ascii_lowercase();
                if !matches!(ext.as_str(), "exe" | "bat" | "cmd")
                    || stem.starts_with('.')
                    || is_activation(&f.file_name().to_string_lossy())
                    || !path.is_file()
                {
                    continue;
                }
                by_key
                    .entry(stem.to_ascii_lowercase())
                    .or_insert_with(|| format!("{stem}.exe"));
            }
        }
    }
    by_key.into_values().collect()
}

/// The shim file names `rehash` keeps in `shims` for the context's flavor.
pub fn wanted(ctx: &Ctx) -> Vec<OsString> {
    match ctx.flavor {
        Flavor::Pyenv => executables_pyenv(&ctx.versions_dir()),
        Flavor::PyenvWin => shims_win(&ctx.versions_dir())
            .into_iter()
            .map(OsString::from)
            .collect(),
    }
}
```

In `crates/pyenv/src/commands/versions.rs`, add `shimset` to the `rpyenv_core` import, and add this arm to the `match *a` in `versions_pyenv`, before the `_ =>` arm:

```rust
            // Rehash mode: argument parsing stops here (libexec/pyenv-versions).
            "--executables" => {
                let mut o = Output::new();
                for name in shimset::executables_pyenv(&ctx.versions_dir()) {
                    o.out(name.to_string_lossy());
                }
                return o;
            }
```

- [ ] **Step 4: Add the allowlist rows**

```markdown
| D-33 | Linux | `versions --executables`, `rehash` | Every entry in `versions/*/bin` and `versions/*/envs/*/bin`, including non-executables, directories, dangling links and dotfiles; a name with spaces becomes several shims; locale order | Runnable regular files only; no dotfiles; a name with spaces is one shim; byte order | Mis-parsing (spec §4); shims that can't run anything. |
| D-34 | both | `rehash` | Linux: `activate`, `activate.csh`, `activate.fish` and `activate.nu` get "source shims"; Windows: `activate.bat` gets a `.lnk` shim | No shim for a file named `activate` or `deactivate`, with any extension | They must run inside the current shell (spec §8); `pyenv activate` covers them (M4). |
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS. `shimset::` runs 2 tests on Windows and 3 on Linux; `versions_executables` runs on Linux.

- [ ] **Step 6: Commit**

```bash
git add crates/rpyenv-core crates/pyenv docs/parity/allowlist.md
git commit -m "Decide which names get shims; add versions --executables"
```

---

### Task 6: Rehash in the core: lock, stored state, applying the difference

**Files:**
- Create: `crates/rpyenv-core/src/rehash.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod rehash;`)

**Interfaces:**
- Consumes: `shimset::wanted` (Task 5), `installed::{top_level, envs_of}`.
- Produces (in `rpyenv_core::rehash`):
  - `const LOCK_NAME: &str = ".rehash.lock"`, `STATE_NAME = ".rehash-state"`, `TEMPLATE_DIR = ".template"`, `TEMPLATE_EXE = "pyenv-shim.exe"`
  - `enum Wait { Upto(Duration), No }`
  - `enum RehashError { NotWritable(PathBuf), Timeout(PathBuf), Busy, Io(io::Error) }`
  - `fn lock(shims: &Path, wait: Wait) -> Result<Lock, RehashError>`; the `Lock` guard removes the file on drop
  - `fn snapshot(ctx: &Ctx) -> String`, `fn needed(ctx: &Ctx) -> bool`
  - `fn rehash(ctx: &Ctx, shim_exe: &Path, wait: Wait) -> Result<(), RehashError>`
  - `fn check(ctx: &Ctx, shim_exe: &Path)`: a shim's exit check; rehashes only when `needed`, ignores failures

`shim_exe` is the shim binary:
- **Linux:** every shim is a symlink to it.
- **Windows:** it is copied into `shims\.template\pyenv-shim.exe` whenever its bytes differ, and every shim is a hardlink to that copy (spec §8). A shim's own exit check passes the template itself, so nothing is copied then.

The stored state is one line per watched folder: its modification time, its entry count, and its path.
- The watched folders are `versions`, plus each version's `bin`, `envs` and each env's `bin` (Linux), or each version's folder, `Scripts` and `bin` (Windows).
- Adding or removing a file changes a folder's time. The count also catches file systems with coarse timestamps.
- The snapshot is taken **before** scanning, so a change made during the scan makes the next check rehash again.

- [ ] **Step 1: Write the failing tests**

Create `crates/rpyenv-core/src/rehash.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn exe(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// A root, its context, and a fake shim binary.
    fn setup(flavor: Flavor) -> (tempfile::TempDir, Ctx, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("versions")).unwrap();
        let ctx = Ctx::for_test(flavor, &root, tmp.path());
        let shim = tmp.path().join("pyenv-shim");
        fs::write(&shim, b"shim binary").unwrap();
        (tmp, ctx, shim)
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let (_t, ctx, _) = setup(Flavor::current());
        let shims = ctx.shims_dir();
        let held = lock(&shims, Wait::No).unwrap();
        assert!(matches!(lock(&shims, Wait::No), Err(RehashError::Busy)));
        let waited = lock(&shims, Wait::Upto(Duration::from_millis(200)));
        assert!(matches!(waited, Err(RehashError::Timeout(p)) if p == shims.join(LOCK_NAME)));
        drop(held);
        assert!(!shims.join(LOCK_NAME).exists());
        assert!(lock(&shims, Wait::No).is_ok());
    }

    #[test]
    fn a_lock_older_than_two_minutes_is_broken() {
        let (_t, ctx, _) = setup(Flavor::current());
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        let f = fs::File::create(shims.join(LOCK_NAME)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(180))
            .unwrap();
        drop(f);
        assert!(lock(&shims, Wait::No).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn linux_rehash_links_replaces_and_removes() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/bin/python"));
        exe(&v.join("3.12.1/bin/pip"));
        let shims = ctx.shims_dir();
        fs::create_dir_all(shims.join("mdir")).unwrap();
        // An upstream bash shim is replaced by a link.
        fs::write(shims.join("python"), "#!/usr/bin/env bash\n").unwrap();
        fs::write(shims.join("gone"), "").unwrap();
        fs::write(shims.join(".keep"), "").unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        for n in ["python", "pip"] {
            assert_eq!(fs::read_link(shims.join(n)).unwrap(), shim);
        }
        assert!(!shims.join("gone").exists());
        assert!(shims.join(".keep").exists());
        assert!(shims.join("mdir").is_dir());
        assert!(!shims.join(LOCK_NAME).exists());
        assert!(!needed(&ctx));
    }

    #[cfg(unix)]
    #[test]
    fn the_check_notices_a_new_script() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/bin/python"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert!(!needed(&ctx));
        exe(&v.join("3.12.1/bin/black"));
        assert!(needed(&ctx));
        check(&ctx, &shim);
        assert_eq!(fs::read_link(ctx.shims_dir().join("black")).unwrap(), shim);
        assert!(!needed(&ctx));
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_shims_folder() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        fs::set_permissions(&shims, fs::Permissions::from_mode(0o555)).unwrap();
        let root_user = fs::write(shims.join("probe"), "").is_ok();
        if !root_user {
            let r = rehash(&ctx, &shim, Wait::No);
            assert!(matches!(r, Err(RehashError::NotWritable(d)) if d == shims));
        }
        fs::set_permissions(&shims, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn windows_rehash_hardlinks_and_removes_pyenv_win_shims() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/pip.exe"));
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        for old in ["python.bat", "python", "aws.lnk"] {
            fs::write(shims.join(old), "old").unwrap();
        }
        rehash(&ctx, &shim, Wait::No).unwrap();
        let template = shims.join(TEMPLATE_DIR).join(TEMPLATE_EXE);
        assert_eq!(fs::read(&template).unwrap(), b"shim binary");
        for n in ["python.exe", "pip.exe"] {
            assert_eq!(fs::read(shims.join(n)).unwrap(), b"shim binary");
        }
        for old in ["python.bat", "python", "aws.lnk"] {
            assert!(!shims.join(old).exists(), "{old}");
        }
        // Links to the current template are kept.
        let before = fs::metadata(shims.join("pip.exe")).unwrap().modified().unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        let after = fs::metadata(shims.join("pip.exe")).unwrap().modified().unwrap();
        assert_eq!(before, after);
        // A new shim binary refreshes the template and every link.
        fs::write(&shim, b"new shim binary").unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(fs::read(shims.join("pip.exe")).unwrap(), b"new shim binary");
    }
}
```

Add `pub mod rehash;` to `crates/rpyenv-core/src/lib.rs`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core rehash::`
Expected: compile errors: `lock`, `rehash`, `Wait` and the rest not found.

- [ ] **Step 3: Implement**

Put this above the tests in `rehash.rs`:

```rust
//! `pyenv rehash`: create the missing shims, remove stale ones, and store what the versions
//! looked like, so a shim can cheaply tell when another rehash is needed (spec §8).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, shimset};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Only one rehash runs at a time.
pub const LOCK_NAME: &str = ".rehash.lock";
/// What the versions looked like at the last rehash.
pub const STATE_NAME: &str = ".rehash-state";
/// Windows: the per-user copy of the shim binary that every shim is a hardlink to.
pub const TEMPLATE_DIR: &str = ".template";
pub const TEMPLATE_EXE: &str = "pyenv-shim.exe";
/// Upstream deletes a lock older than two minutes (libexec/pyenv-rehash:15-43).
const STALE_LOCK: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// `pyenv rehash`: retry every 0.1 s for up to this long, as upstream does.
    Upto(Duration),
    /// A shim's exit check: give up at once; the next check catches up.
    No,
}

#[derive(Debug)]
pub enum RehashError {
    NotWritable(PathBuf),
    /// `Wait::Upto` ran out. Holds the lock's path.
    Timeout(PathBuf),
    /// `Wait::No` found the lock held.
    Busy,
    Io(io::Error),
}

/// Held while a rehash runs. Dropping it removes the lock file, on failure too.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Takes `shims/.rehash.lock`, creating `shims` if needed.
pub fn lock(shims: &Path, wait: Wait) -> Result<Lock, RehashError> {
    let not_writable = || RehashError::NotWritable(shims.to_path_buf());
    fs::create_dir_all(shims).map_err(|_| not_writable())?;
    let path = shims.join(LOCK_NAME);
    let start = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(Lock { path }),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if is_stale(&path) {
                    let _ = fs::remove_file(&path);
                    continue;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(not_writable()),
            Err(e) => return Err(RehashError::Io(e)),
        }
        match wait {
            Wait::No => return Err(RehashError::Busy),
            Wait::Upto(limit) if start.elapsed() >= limit => {
                return Err(RehashError::Timeout(path))
            }
            Wait::Upto(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn is_stale(lock: &Path) -> bool {
    fs::metadata(lock)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or_default() > STALE_LOCK)
        .unwrap_or(false)
}

/// The watched folders' modification times and entry counts, one line each.
pub fn snapshot(ctx: &Ctx) -> String {
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
    let mut s = String::from("rpyenv rehash state 1\n");
    for d in dirs {
        let Ok(meta) = fs::metadata(&d) else {
            continue;
        };
        let t = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .unwrap_or_default();
        let count = fs::read_dir(&d).map(|r| r.count()).unwrap_or(0);
        s.push_str(&format!(
            "{}.{:09} {count} {}\n",
            t.as_secs(),
            t.subsec_nanos(),
            d.display()
        ));
    }
    s
}

/// True when the stored state is missing or differs from the disk.
pub fn needed(ctx: &Ctx) -> bool {
    fs::read_to_string(ctx.shims_dir().join(STATE_NAME))
        .map(|stored| stored != snapshot(ctx))
        .unwrap_or(true)
}

/// A shim's exit check (spec §8): rehash when the state changed. Failures are ignored,
/// including a lock held by another rehash; the next check catches up.
pub fn check(ctx: &Ctx, shim_exe: &Path) {
    if needed(ctx) {
        let _ = rehash(ctx, shim_exe, Wait::No);
    }
}

/// Brings `shims` in line with the installed versions.
pub fn rehash(ctx: &Ctx, shim_exe: &Path, wait: Wait) -> Result<(), RehashError> {
    let shims = ctx.shims_dir();
    let _lock = lock(&shims, wait)?;
    // Taken before scanning: a change during the scan makes the next check rehash again.
    let state = snapshot(ctx);
    let wanted = shimset::wanted(ctx);
    match ctx.flavor {
        Flavor::Pyenv => apply_links(&shims, shim_exe, &wanted),
        Flavor::PyenvWin => apply_hardlinks(&shims, shim_exe, &wanted),
    }
    .map_err(RehashError::Io)?;
    write_state(&shims, &state).map_err(RehashError::Io)
}

fn write_state(shims: &Path, state: &str) -> io::Result<()> {
    let tmp = shims.join(".rehash-state.tmp");
    fs::write(&tmp, state)?;
    fs::rename(&tmp, shims.join(STATE_NAME))
}

/// Linux: each shim is a symlink to the shim binary. An existing file that isn't that
/// link, such as upstream's bash shim, is replaced; directories are left alone.
fn apply_links(shims: &Path, target: &Path, wanted: &[OsString]) -> io::Result<()> {
    for name in wanted {
        let p = shims.join(name);
        match fs::symlink_metadata(&p) {
            Ok(m)
                if m.file_type().is_symlink()
                    && fs::read_link(&p).ok().as_deref() == Some(target) =>
            {
                continue
            }
            Ok(m) if m.is_dir() => continue,
            Ok(_) => fs::remove_file(&p)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        symlink(target, &p)?;
    }
    remove_stale(shims, wanted, false)
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    fs::copy(target, link).map(|_| ())
}

/// Windows: each shim is a hardlink to `shims\.template\pyenv-shim.exe`, or a copy when a
/// hardlink isn't possible. A failed shim doesn't stop the others; the first error is
/// returned, so the state isn't stored and the next check tries again.
fn apply_hardlinks(shims: &Path, source: &Path, wanted: &[OsString]) -> io::Result<()> {
    let template = refresh_template(shims, source)?;
    let tmeta = fs::metadata(&template)?;
    let mut first_err = None;
    for name in wanted {
        let p = shims.join(name);
        match fs::metadata(&p) {
            Ok(m) if same_file_data(&m, &tmeta) => continue,
            Ok(m) if m.is_dir() => continue,
            Ok(_) => remove_or_rename(&p),
            Err(_) => {}
        }
        if let Err(e) = fs::hard_link(&template, &p).or_else(|_| fs::copy(&template, &p).map(|_| ())) {
            first_err.get_or_insert(e);
        }
    }
    remove_stale(shims, wanted, true)?;
    first_err.map_or(Ok(()), Err)
}

/// A hardlink shares its size and modification time with the template. A refreshed
/// template, copied from a different shim binary, differs in at least one of them.
fn same_file_data(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.is_file() && a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

/// `shims\.template\pyenv-shim.exe`, copied again when `source` has different bytes
/// (spec §8). Old copies renamed aside by an earlier refresh are deleted first.
fn refresh_template(shims: &Path, source: &Path) -> io::Result<PathBuf> {
    let dir = shims.join(TEMPLATE_DIR);
    fs::create_dir_all(&dir)?;
    for e in fs::read_dir(&dir)?.filter_map(Result::ok) {
        if e.file_name().to_string_lossy().ends_with(".old") {
            let _ = fs::remove_file(e.path());
        }
    }
    let template = dir.join(TEMPLATE_EXE);
    if source == template {
        return Ok(template);
    }
    let fresh = match (fs::read(source), fs::read(&template)) {
        (Ok(a), Ok(b)) => a == b,
        (Ok(_), Err(_)) => false,
        (Err(e), _) if !template.is_file() => return Err(e),
        (Err(_), _) => true,
    };
    if !fresh {
        remove_or_rename(&template);
        fs::copy(source, &template)?;
    }
    Ok(template)
}

/// Removes shim files that are no longer wanted. Names starting with `.` are rpyenv's
/// own files and are kept, except `.old` leftovers, which are deleted. Directories are
/// left alone (allowlist D-35). `fold_case` compares names without case (Windows).
fn remove_stale(shims: &Path, wanted: &[OsString], fold_case: bool) -> io::Result<()> {
    let key = |n: &OsString| -> OsString {
        if fold_case {
            OsString::from(n.to_string_lossy().to_lowercase())
        } else {
            n.clone()
        }
    };
    let keep: HashSet<OsString> = wanted.iter().map(key).collect();
    for e in fs::read_dir(shims)?.filter_map(Result::ok) {
        let name = e.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') {
            if text.ends_with(".old") {
                let _ = fs::remove_file(e.path());
            }
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir && !keep.contains(&key(&name)) {
            remove_or_rename(&e.path());
        }
    }
    Ok(())
}

/// Deletes a shim. A running `.exe` can't be deleted on Windows, so it is renamed to
/// `.<name>.old` instead (hidden, and never a command name); a later rehash deletes it.
fn remove_or_rename(p: &Path) {
    if fs::remove_file(p).is_ok() || !p.exists() {
        return;
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 0..100u32 {
        let aside = if n == 0 {
            format!(".{name}.old")
        } else {
            format!(".{name}.{n}.old")
        };
        let aside = p.with_file_name(aside);
        if !aside.exists() {
            let _ = fs::rename(p, aside);
            return;
        }
    }
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p rpyenv-core rehash::`
Expected: PASS. 3 tests on Windows (the two lock tests and `windows_rehash_…`); 6 on Linux.

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core
git commit -m "Add rehash: lock, stored state, links and hardlinks"
```

---
### Task 7: Launch plans and starting the process

**Files:**
- Create: `crates/rpyenv-core/src/launch.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod launch;`)
- Modify: `crates/rpyenv-core/Cargo.toml` (`libc` on Unix)
- Modify: `crates/rpyenv-core/src/paths.rs` (receives `win_path_key`)
- Modify: `crates/rpyenv-core/src/pathsearch.rs` (`pathext_list`, `find_cmd`)
- Modify: `crates/pyenv/src/commands/version.rs` (uses the moved `win_path_key`)
- Modify: `docs/parity/allowlist.md` (D-38, D-39, D-40, D-42, D-43)

**Interfaces:**
- Consumes: `lookup::{which_pyenv, which_win, not_found_report, shim_paths_var, Skip, Report}` (Tasks 2-3), `select::{version_name, win_select}`, `rehash::check` (Task 6).
- Produces (in `rpyenv_core::launch`):
  - `struct LaunchPlan { program: PathBuf, args: Vec<OsString>, env: Vec<(OsString, Option<OsString>)>, warnings: Vec<String>, wait: bool }`
  - `struct ExecEnv { shim_path, shim_paths, appdata: Option<OsString>, shim_exe: Option<PathBuf> }` (derives `Default`), with `fn from_process(program: &str, shim_exe: Option<PathBuf>) -> ExecEnv`
  - `enum Mode { Exec, Shim }`
  - `const WIN_EXEC_NO_VERSION: [&str; 3]`
  - `fn plan(ctx: &Ctx, mode: Mode, command: &str, args: Vec<OsString>, env: &ExecEnv) -> Result<LaunchPlan, Report>`
  - `fn win_exec_version_check(ctx: &Ctx, names: &[String]) -> Result<(), Report>`
  - `fn win_child_path(ctx: &Ctx, names: &[String], appdata: Option<&OsStr>) -> OsString`
  - `fn is_pip_like(command: &str, args: &[OsString]) -> bool`
  - `fn run(plan: &LaunchPlan, ctx: &Ctx, rehash_with: Option<&Path>) -> i32`
- Also: `paths::win_path_key(p: &Path) -> String` (moved from `version.rs`); `pathsearch::find_cmd(name: &str, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf>`.

Upstream `pyenv-exec` (reference "`pyenv exec`", steps 1-9):
1. `PYENV_VERSION` becomes `version-name -f`, exported unconditionally.
2. `_PYENV_SHIM_PATH` is joined into `_PYENV_SHIM_PATHS_<PROGRAM>`.
3. `pyenv-which` finds the command. It sees the resolved `PYENV_VERSION` only if the caller had exported one; on failure its output stands and exit is 127.
4. The command's folder is prepended to `PATH` when it starts, as a string, with `PYENV_ROOT`.
5. `exec` runs with `argv[0]` set to the full path.

The dispatcher also exports `PYENV_ROOT` and `PYENV_DIR`.

pyenv-win `exec` (reference "exec"):
- **Child `PATH`:** every selected version's folder, `Scripts` and `bin`, then the last version's user-site `Scripts`, then the caller's `PATH` without the shims folder.
- **Version check:** only the last selected version must exist; otherwise the 3-line message.
- **Lookup:** cmd.exe finds the command on that `PATH`.

A Windows **shim** finds its file with pyenv-win's `which` instead (Decision 3).

`run`:
- **Linux, plain commands:** `exec` replaces the process (spec §5.2).
- **Linux, pip commands:** spawn and wait. While waiting, ignore `SIGINT` and `SIGQUIT`, pass `SIGTERM` and `SIGHUP` on to the child, and die from the child's signal if it died from one.
- **Windows:** always spawn and wait. M1b-win replaces this with the raw command line and a Job Object.
- **After a wait:** the rehash check runs before returning.

- [ ] **Step 1: Add the dependency and move `win_path_key`**

In `crates/rpyenv-core/Cargo.toml`:

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

Move `path_key` from `crates/pyenv/src/commands/version.rs` into `crates/rpyenv-core/src/paths.rs` as a public function:

```rust
/// A comparison key for a Windows path: `.` and `..` resolved by text, `/` unified to `\`,
/// a trailing `\` trimmed, and ASCII-lowercased.
pub fn win_path_key(p: &Path) -> String {
    let normalized = lexical_normalize(p).to_string_lossy().replace('/', "\\");
    normalized.trim_end_matches('\\').to_ascii_lowercase()
}
```

In `version.rs`, delete `path_key`, import `rpyenv_core::paths::win_path_key`, and replace the two `path_key(` calls in `win_path_check` with `win_path_key(`. Drop the `lexical_normalize` import there if nothing else in the file uses it (clippy reports it).

- [ ] **Step 2: Write the failing tests**

Add to the tests in `crates/rpyenv-core/src/pathsearch.rs`:

```rust
    #[test]
    fn cmd_search_tries_the_typed_name_then_pathext() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        make_exe(&a, "tool.cmd");
        make_exe(&b, "tool.exe");
        make_exe(&b, "x.py");
        let path = OsString::from(format!("{};\"{}\";", a.display(), b.display()));
        let exts = Some(OsStr::new(".EXE;.CMD"));
        assert_eq!(find_cmd("tool", &path, exts), Some(a.join("tool.cmd")));
        assert_eq!(find_cmd("tool.exe", &path, exts), Some(b.join("tool.exe")));
        assert_eq!(find_cmd("x.py", &path, None), Some(b.join("x.py")));
        assert_eq!(find_cmd("x", &path, None), None);
    }
```

(add `use std::ffi::OsString;` to that test module).

Create `crates/rpyenv-core/src/launch.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn exe(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn var<'a>(plan: &'a LaunchPlan, name: &str) -> Option<&'a Option<OsString>> {
        plan.env
            .iter()
            .find(|(k, _)| k.as_os_str() == name)
            .map(|(_, v)| v)
    }

    fn pyenv_ctx() -> (tempfile::TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("versions")).unwrap();
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let ctx = Ctx::for_test(Flavor::Pyenv, &root, &tmp.path().join("work"));
        (tmp, ctx)
    }

    #[test]
    fn pyenv_plan_sets_the_upstream_environment() {
        let (tmp, mut ctx) = pyenv_ctx();
        let py = ctx.versions_dir().join("3.12.10").join("bin").join("python");
        exe(&py);
        let sys = tmp.path().join("sys");
        ctx.path = Some(sys.clone().into_os_string());
        ctx.pyenv_version = Some("3.12".to_string());
        let p = plan(&ctx, Mode::Exec, "python", vec!["-V".into()], &ExecEnv::default()).unwrap();
        assert_eq!(p.program, py);
        assert_eq!(p.args, [OsString::from("-V")]);
        assert_eq!(var(&p, "PYENV_VERSION"), Some(&Some(OsString::from("3.12.10"))));
        assert_eq!(var(&p, "PYENV_ROOT"), Some(&Some(ctx.root.clone().into_os_string())));
        assert_eq!(var(&p, "PYENV_DIR"), Some(&Some(ctx.dir.clone().into_os_string())));
        let mut path = py.parent().unwrap().as_os_str().to_os_string();
        path.push(":");
        path.push(&sys);
        assert_eq!(var(&p, "PATH"), Some(&Some(path)));
        assert!(!p.wait);
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn pyenv_plan_for_system_leaves_path_alone() {
        let (tmp, mut ctx) = pyenv_ctx();
        let tool = tmp.path().join("sys").join("tool");
        exe(&tool);
        ctx.path = Some(tmp.path().join("sys").into_os_string());
        let p = plan(&ctx, Mode::Shim, "tool", vec![], &ExecEnv::default()).unwrap();
        assert_eq!(p.program, tool);
        assert_eq!(var(&p, "PYENV_VERSION"), Some(&Some(OsString::from("system"))));
        assert_eq!(var(&p, "PATH"), None);
    }

    #[test]
    fn pyenv_plan_not_found_is_the_which_report() {
        let (_t, mut ctx) = pyenv_ctx();
        ctx.pyenv_version = Some("9.9".to_string());
        let r = plan(&ctx, Mode::Exec, "tool", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!(
            r.lines,
            [
                "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)",
                "pyenv: tool: command not found"
            ]
        );
        assert_eq!((r.stderr, r.code), (true, 127));
    }

    #[test]
    fn upstream_shim_path_variables() {
        let (_t, mut ctx) = pyenv_ctx();
        exe(&ctx.versions_dir().join("3.12.10").join("bin").join("python"));
        ctx.pyenv_version = Some("3.12.10".to_string());
        let env = ExecEnv {
            shim_path: Some("/opt/shims".into()),
            shim_paths: Some("/a".into()),
            ..ExecEnv::default()
        };
        let p = plan(&ctx, Mode::Exec, "python", vec![], &env).unwrap();
        assert_eq!(
            var(&p, "_PYENV_SHIM_PATHS_PYTHON"),
            Some(&Some(OsString::from("/opt/shims:/a")))
        );
        assert_eq!(var(&p, "_PYENV_SHIM_PATH"), Some(&None));
    }

    #[test]
    fn pip_like_commands() {
        let args = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        for c in ["pip", "pip3", "pip3.12", "easy_install", "easy_install3", "conda"] {
            assert!(is_pip_like(c, &[]), "{c}");
        }
        assert!(is_pip_like("python3", &args(&["-m", "pip", "install", "x"])));
        for c in ["pipx", "pipenv", "black", "python"] {
            assert!(!is_pip_like(c, &args(&["-m", "venv"])), "{c}");
        }
    }

    fn win_ctx(versions: &[&str]) -> (tempfile::TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("versions")).unwrap();
        for v in versions {
            fs::create_dir_all(root.join("versions").join(v)).unwrap();
        }
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let mut ctx = Ctx::for_test(Flavor::PyenvWin, &root, &tmp.path().join("work"));
        ctx.pathext = Some(".exe;.bat".into());
        (tmp, ctx)
    }

    #[test]
    fn win_child_path_matches_pyenv_win() {
        let (_t, mut ctx) = win_ctx(&[]);
        let shims = ctx.shims_dir().display().to_string();
        ctx.path = Some(format!("C:\\Windows;{shims};;\"C:\\q d\";{shims}\\").into());
        let names = ["3.7.7".to_string(), "3.8.9-win32".to_string()];
        let v = |n: &str, sub: &str| {
            let p = ctx.versions_dir().join(n);
            let p = if sub.is_empty() { p } else { p.join(sub) };
            p.display().to_string()
        };
        let expected = format!(
            "{};{};{};{};{};{};C:\\AppData\\Python\\Python38-32\\Scripts;C:\\Windows;C:\\q d;",
            v("3.7.7", ""),
            v("3.7.7", "Scripts"),
            v("3.7.7", "bin"),
            v("3.8.9-win32", ""),
            v("3.8.9-win32", "Scripts"),
            v("3.8.9-win32", "bin"),
        );
        assert_eq!(
            win_child_path(&ctx, &names, Some(OsStr::new("C:\\AppData"))),
            OsString::from(expected)
        );
    }

    #[test]
    fn win_exec_plan() {
        let (_t, mut ctx) = win_ctx(&["3.9.1"]);
        let py = ctx.versions_dir().join("3.9.1").join("python.exe");
        exe(&py);
        ctx.pyenv_version = Some("3.9.1".to_string());
        let p = plan(&ctx, Mode::Exec, "python", vec![], &ExecEnv::default()).unwrap();
        assert_eq!(p.program, py);
        assert!(p.wait);
        assert_eq!(var(&p, "PYENV_VERSION"), None);
        let r = plan(&ctx, Mode::Exec, "nothing", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!(
            r.lines,
            [
                "'nothing' is not recognized as an internal or external command,",
                "operable program or batch file."
            ]
        );
        assert_eq!((r.stderr, r.code), (true, 1));
        ctx.pyenv_version = Some("3.9.1 3.7.7".to_string());
        let r = plan(&ctx, Mode::Exec, "python", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!(r.lines, WIN_EXEC_NO_VERSION);
    }

    #[test]
    fn win_shim_plan_uses_which() {
        let (_t, mut ctx) = win_ctx(&["3.8.2", "3.9.1"]);
        exe(&ctx.versions_dir().join("3.8.2").join("python38.exe"));
        ctx.pyenv_version = Some("3.9.1".to_string());
        let r = plan(&ctx, Mode::Shim, "python38", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!((r.stderr, r.code), (false, 127));
        assert_eq!(r.lines[0], "pyenv: python38: command not found");
    }
}
```

Add `pub mod launch;` to `crates/rpyenv-core/src/lib.rs`.

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core -- launch:: pathsearch::`
Expected: compile errors: `plan`, `ExecEnv`, `find_cmd` and the rest not found.

- [ ] **Step 4: Implement `find_cmd`**

In `crates/rpyenv-core/src/pathsearch.rs`, pull the extension list out of `find_all` into a helper, and have `find_all`'s `Flavor::PyenvWin` arm call `pathext_list(pathext)`:

```rust
/// `PATHEXT`'s extensions, lowercased; `.COM;.EXE;.BAT;.CMD` when unset or empty.
fn pathext_list(pathext: Option<&OsStr>) -> Vec<String> {
    pathext
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// cmd.exe's search for a command word on a `;`-separated `PATH`: in each folder, the name
/// as typed when it has an extension, then the name plus each `PATHEXT` extension. Quotes
/// and empty entries are dropped.
pub fn find_cmd(name: &str, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    let exts = pathext_list(pathext);
    let typed_ext = Path::new(name).extension().is_some();
    let path = path.to_string_lossy();
    for dir in path
        .split(';')
        .map(|d| d.replace('"', ""))
        .filter(|d| !d.is_empty())
    {
        let dir = Path::new(&dir);
        if typed_ext && dir.join(name).is_file() {
            return Some(dir.join(name));
        }
        for e in &exts {
            let candidate = dir.join(format!("{name}{e}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
```

- [ ] **Step 5: Implement `launch.rs`**

Put this above the tests:

```rust
//! Running a command the way `pyenv exec` and the shims do: which file, which environment,
//! and how the process is started (spec §5).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::lookup::{self, Report, Skip};
use crate::paths::win_path_key;
use crate::{pathsearch, rehash, select};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// What to run and how. Built by [`plan`], started by [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The resolved file. On Linux it is also the child's `argv[0]`, as upstream's
    /// `exec "$PYENV_COMMAND_PATH"` makes it.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Variables to set (`Some`) or remove (`None`) in the child.
    pub env: Vec<(OsString, Option<OsString>)>,
    /// Upstream's `invalid version` lines, for stderr before starting.
    pub warnings: Vec<String>,
    /// Spawn, wait, then run the rehash check, instead of replacing this process.
    pub wait: bool,
}

/// Inputs from the environment that `Ctx` doesn't carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecEnv {
    /// Upstream's `_PYENV_SHIM_PATH`, set by upstream's own shims.
    pub shim_path: Option<OsString>,
    /// Upstream's `_PYENV_SHIM_PATHS_<PROGRAM>`.
    pub shim_paths: Option<OsString>,
    /// `APPDATA`: pyenv-win puts the user-site `Scripts` folder on `PATH`.
    pub appdata: Option<OsString>,
    /// rpyenv's shim binary, never taken as a `system` command (allowlist D-30).
    pub shim_exe: Option<PathBuf>,
}

impl ExecEnv {
    pub fn from_process(program: &str, shim_exe: Option<PathBuf>) -> ExecEnv {
        let get = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        ExecEnv {
            shim_path: get("_PYENV_SHIM_PATH"),
            shim_paths: get(&lookup::shim_paths_var(program)),
            appdata: get("APPDATA"),
            shim_exe,
        }
    }
}

/// `pyenv exec` or a shim. They differ only on Windows, in how the command is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Exec,
    Shim,
}

/// pyenv-win `exec`'s own message when the last selected version is missing.
pub const WIN_EXEC_NO_VERSION: [&str; 3] = [
    "No global/local python version has been set yet. Please set the global/local version by typing:",
    "pyenv global 3.7.4",
    "pyenv local 3.7.4",
];

/// Resolves `command`. `Err` carries what to print and the exit code.
pub fn plan(
    ctx: &Ctx,
    mode: Mode,
    command: &str,
    args: Vec<OsString>,
    env: &ExecEnv,
) -> Result<LaunchPlan, Report> {
    match ctx.flavor {
        Flavor::Pyenv => plan_pyenv(ctx, command, args, env),
        Flavor::PyenvWin => plan_win(ctx, mode, command, args, env),
    }
}

/// Upstream `pyenv-exec` (libexec/pyenv-exec:24-57).
fn plan_pyenv(
    ctx: &Ctx,
    command: &str,
    args: Vec<OsString>,
    env: &ExecEnv,
) -> Result<LaunchPlan, Report> {
    let names = select::version_name(ctx, true);
    let version = names.names.join(":");
    // `_PYENV_SHIM_PATH` is prepended to `_PYENV_SHIM_PATHS_<PROGRAM>` with `:`.
    let shim_paths = match (&env.shim_path, &env.shim_paths) {
        (Some(p), Some(ps)) => {
            let mut s = p.clone();
            s.push(":");
            s.push(ps);
            Some(s)
        }
        (Some(p), None) => Some(p.clone()),
        (None, ps) => ps.clone(),
    };
    let skip = Skip {
        dirs: shim_paths
            .as_ref()
            .map(|v| std::env::split_paths(v).collect())
            .unwrap_or_default(),
        exe: env.shim_exe.clone(),
    };
    // `pyenv-which` sees the resolved PYENV_VERSION only when the caller had exported one.
    let mut which_ctx = ctx.clone();
    if which_ctx.pyenv_version.is_some() {
        which_ctx.pyenv_version = Some(version.clone());
    }
    let found = match lookup::which_pyenv(&which_ctx, command, false, &skip) {
        Ok(f) => f,
        Err(nf) => {
            let mut report = lookup::not_found_report(&which_ctx, command, &nf, true);
            let mut lines = names.stderr.clone();
            lines.append(&mut report.lines);
            report.lines = lines;
            return Err(report);
        }
    };
    let mut vars = vec![
        (OsString::from("PYENV_VERSION"), Some(OsString::from(&version))),
        (OsString::from("PYENV_ROOT"), Some(ctx.root.clone().into_os_string())),
        (OsString::from("PYENV_DIR"), Some(ctx.dir.clone().into_os_string())),
    ];
    if env.shim_path.is_some() {
        vars.push((OsString::from(lookup::shim_paths_var(command)), shim_paths));
        vars.push((OsString::from("_PYENV_SHIM_PATH"), None));
    }
    // The command's folder goes first on PATH only when it lies under PYENV_ROOT, by
    // plain string prefix (libexec/pyenv-exec:53-56). No libexec or plugin folders (D-38).
    let bin = found.path.parent().unwrap_or(Path::new("")).as_os_str();
    if starts_with(bin, ctx.root.as_os_str()) {
        let mut p = bin.to_os_string();
        p.push(":");
        if let Some(old) = &ctx.path {
            p.push(old);
        }
        vars.push((OsString::from("PATH"), Some(p)));
    }
    let mut warnings = names.stderr;
    warnings.extend(found.warnings);
    Ok(LaunchPlan {
        wait: is_pip_like(command, &args),
        program: found.path,
        args,
        env: vars,
        warnings,
    })
}

#[cfg(unix)]
fn starts_with(s: &OsStr, prefix: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().starts_with(prefix.as_bytes())
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn starts_with(s: &OsStr, prefix: &OsStr) -> bool {
    s.to_string_lossy().starts_with(&*prefix.to_string_lossy())
}

/// pyenv-win `exec` (pyenv.bat:21-129) and rpyenv's Windows shims.
fn plan_win(
    ctx: &Ctx,
    mode: Mode,
    command: &str,
    args: Vec<OsString>,
    env: &ExecEnv,
) -> Result<LaunchPlan, Report> {
    let names: Vec<String> = select::win_select(ctx)
        .into_iter()
        .map(|s| s.name)
        .collect();
    let path = win_child_path(ctx, &names, env.appdata.as_deref());
    let program = match mode {
        // A shim finds its file like `pyenv which`; not found is exit 127 (D-42).
        Mode::Shim => {
            lookup::which_win(ctx, command)
                .map_err(|nf| lookup::not_found_report(ctx, command, &nf, true))?
                .path
        }
        // `exec` lets the command line find it on the new PATH (D-40).
        Mode::Exec => {
            win_exec_version_check(ctx, &names)?;
            pathsearch::find_cmd(command, &path, ctx.pathext.as_deref()).ok_or_else(|| {
                Report {
                    lines: vec![
                        format!("'{command}' is not recognized as an internal or external command,"),
                        "operable program or batch file.".to_string(),
                    ],
                    stderr: true,
                    code: 1,
                }
            })?
        }
    };
    Ok(LaunchPlan {
        program,
        args,
        env: vec![(OsString::from("PATH"), Some(path))],
        warnings: Vec::new(),
        wait: true,
    })
}

/// pyenv-win `exec` checks only the last selected version (pyenv.bat:67-72).
pub fn win_exec_version_check(ctx: &Ctx, names: &[String]) -> Result<(), Report> {
    if names
        .last()
        .is_some_and(|n| ctx.versions_dir().join(n).is_dir())
    {
        Ok(())
    } else {
        Err(Report {
            lines: WIN_EXEC_NO_VERSION.iter().map(|l| l.to_string()).collect(),
            stderr: false,
            code: 1,
        })
    }
}

/// pyenv-win's child `PATH`: each selected version's folder, `Scripts` and `bin`; the
/// user-site `Scripts` of the last one; then the caller's `PATH` entries, unquoted, without
/// empty ones and without the shims folder in any spelling. Every entry ends with `;`
/// (allowlist D-40).
pub fn win_child_path(ctx: &Ctx, names: &[String], appdata: Option<&OsStr>) -> OsString {
    let mut out = OsString::new();
    let mut add = |p: &OsStr| {
        out.push(p);
        out.push(";");
    };
    for n in names {
        let v = ctx.versions_dir().join(n);
        add(v.as_os_str());
        add(v.join("Scripts").as_os_str());
        add(v.join("bin").as_os_str());
    }
    if let (Some(last), Some(appdata)) = (names.last(), appdata) {
        add(OsStr::new(&user_site_scripts(appdata, last)));
    }
    let shims = win_path_key(&ctx.shims_dir());
    if let Some(path) = &ctx.path {
        for entry in path.to_string_lossy().split(';') {
            let entry = entry.replace('"', "");
            if !entry.is_empty() && win_path_key(Path::new(&entry)) != shims {
                add(OsStr::new(&entry));
            }
        }
    }
    out
}

/// `%APPDATA%\Python\Python<x><y>[-32]\Scripts` for a name like `3.8.9` or `3.8.9-win32`
/// (pyenv.bat:26-36).
fn user_site_scripts(appdata: &OsStr, name: &str) -> String {
    let (version, arch) = name.split_once('-').unwrap_or((name, ""));
    let mut fields = version.split('.');
    let (x, y) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
    let suffix = if arch.eq_ignore_ascii_case("win32") {
        "-32"
    } else {
        ""
    };
    format!(
        "{}\\Python\\Python{x}{y}{suffix}\\Scripts",
        appdata.to_string_lossy()
    )
}

/// Commands after which a Linux shim waits and runs the rehash check (spec §5.2): `pip`,
/// `easy_install` and `conda`, with any trailing version digits (`pip3.12`), and anything
/// run with `-m pip` (allowlist D-39).
pub fn is_pip_like(command: &str, args: &[OsString]) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    matches!(base, "pip" | "easy_install" | "conda")
        || args.windows(2).any(|w| w[0] == "-m" && w[1] == "pip")
}

/// Starts the plan. Without `wait` (Linux) this process becomes the command, and `run`
/// returns only if that fails. With `wait`, the child runs to the end, the rehash check
/// runs (`rehash_with` is the shim binary rehash uses), and the child's exit code is
/// returned. On Linux, a child that died from a signal makes this process die from it too.
pub fn run(plan: &LaunchPlan, ctx: &Ctx, rehash_with: Option<&Path>) -> i32 {
    let mut cmd = Command::new(&plan.program);
    cmd.args(&plan.args);
    for (k, v) in &plan.env {
        match v {
            Some(v) => {
                cmd.env(k, v);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }
    #[cfg(unix)]
    if !plan.wait {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        return cannot_run(ctx.flavor, &plan.program, &err);
    }
    #[cfg(unix)]
    let status = sig::spawn_and_wait(&mut cmd);
    #[cfg(not(unix))]
    let status = cmd.status();
    if let Some(exe) = rehash_with {
        rehash::check(ctx, exe);
    }
    match status {
        Ok(s) => exit_code(s),
        Err(e) => cannot_run(ctx.flavor, &plan.program, &e),
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            sig::die_by(signal);
        }
    }
    status.code().unwrap_or(1)
}

/// A file that can't be started: exit 127 when it is missing, else 126, as a shell
/// would (allowlist D-43).
fn cannot_run(flavor: Flavor, program: &Path, err: &std::io::Error) -> i32 {
    eprint!("pyenv: {}: {err}{}", program.display(), flavor.eol());
    if err.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

#[cfg(unix)]
mod sig {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, ExitStatus};
    use std::sync::atomic::{AtomicI32, Ordering};

    static CHILD: AtomicI32 = AtomicI32::new(0);

    extern "C" fn forward(signal: libc::c_int) {
        let pid = CHILD.load(Ordering::SeqCst);
        if pid > 0 {
            // SAFETY: kill is async-signal-safe.
            unsafe {
                libc::kill(pid, signal);
            }
        }
    }

    /// Spawns and waits like a shell does (spec §5.2): SIGINT and SIGQUIT are ignored here,
    /// because the terminal sends them to the child too; SIGTERM and SIGHUP are passed on.
    /// A signal that arrives before the child's PID is stored is lost; the window is the
    /// few instructions between `spawn` and `store`.
    pub fn spawn_and_wait(cmd: &mut Command) -> io::Result<ExitStatus> {
        let handler = forward as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: changing this process's signal dispositions before spawning. The child
        // restores the defaults before exec, using only async-signal-safe calls.
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_IGN);
            libc::signal(libc::SIGQUIT, libc::SIG_IGN);
            libc::signal(libc::SIGTERM, handler);
            libc::signal(libc::SIGHUP, handler);
            cmd.pre_exec(|| {
                for s in [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP] {
                    libc::signal(s, libc::SIG_DFL);
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn()?;
        CHILD.store(child.id() as i32, Ordering::SeqCst);
        child.wait()
    }

    /// Dies from `signal`, as the child did, so the caller sees the same status.
    pub fn die_by(signal: i32) -> ! {
        // SAFETY: plain libc calls on this process.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal)
    }
}
```

- [ ] **Step 6: Add the allowlist rows**

```markdown
| D-38 | Linux | `exec`, shims | The child's `PATH` is the dispatcher's, which starts with `<prefix>/libexec` and the plugin `bin` folders; `PYENV_HOOK_PATH` and `_PYENV_INSTALL_PREFIX` are exported; a shim outside `<root>/shims` exports `_PYENV_SHIM_PATH` | The version's `bin` is prepended to the caller's `PATH`; no hook variables; rpyenv's shims don't set `_PYENV_SHIM_PATH` (they never find themselves, D-30) | No bash hooks or libexec (spec D4). |
| D-39 | both | `exec`, shims (pip) | Linux: a wrapper runs `pyenv rehash` after a successful `pip install`/`uninstall` and exports `PYENV_REHASH_REAL_COMMAND`; Windows: a `pip*` shim returns `pyenv rehash`'s exit code | Linux: `pip*`, `easy_install*`, `conda*` and `… -m pip` are spawned and waited for, then the stored-state check runs; Windows: every shim waits and runs the check; the exit code is always the child's | Spec §5.2 and §8; pyenv-win loses pip's exit code. |
| D-40 | Windows | `exec` | cmd.exe finds the command: builtins, then the current directory, then `PATH`; `PATH` entries are rewritten with `%~dpf`; a shims entry spelled with a trailing `\` stays | Only the child's `PATH` is searched (the name as typed if it has an extension, then each `PATHEXT` extension); entries are kept as written apart from quotes; the shims folder is removed in any spelling | No shell is involved (spec §5.3); the current-directory lookup is a cmd artifact. |
| D-42 | Windows | shims | A shim runs `call pyenv exec <name>`; a name no selected version has prints cmd's "is not recognized" and exits 1 | pyenv-win `which`'s not-found message, listing the versions that have the command, exit 127 | Spec §5.1. |
| D-43 | both | `exec`, shims | A file that can't be started prints the shell's error (bash: exit 126 or 127) | `pyenv: <path>: <reason>`, exit 127 when the file is missing, else 126 | No shell is involved. |
```

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS, including the 8 `launch::` tests and the new `pathsearch::` test on both OSes.

- [ ] **Step 8: Commit**

```bash
git add crates/rpyenv-core crates/pyenv docs/parity/allowlist.md Cargo.lock
git commit -m "Add launch plans for exec and the shims"
```

---

### Task 8: The shim program and the `pyenv-shim` crate

**Files:**
- Create: `crates/rpyenv-core/src/shim.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod shim;`)
- Create: `crates/pyenv-shim/Cargo.toml`, `crates/pyenv-shim/src/main.rs`
- Create: `crates/pyenv-shim/tests/direct.rs`
- Modify: `Cargo.toml` (workspace member)

**Interfaces:**
- Consumes: `launch::{plan, run, ExecEnv, Mode}` (Task 7), `rehash::{TEMPLATE_DIR, TEMPLATE_EXE}` (Task 6), `Ctx::from_process`.
- Produces:
  - `shim::SHIM_NAME: &str = "pyenv-shim"`
  - `shim::command_name(flavor: Flavor, argv0: &OsStr, own: Option<&Path>) -> Option<String>`
  - `shim::main() -> i32`
  - the `pyenv-shim` binary

How a shim knows its command (spec §5.1 step 1):
- **Linux:** every shim is a symlink to the same binary, so the command is `argv[0]`'s last component.
- **Windows:** a shim is a hardlink named `python.exe`, and `current_exe()` gives that name.
- **Run directly:** under its own name, `pyenv-shim` is not a command. It says so and exits 1.

- [ ] **Step 1: Write the failing tests**

Create `crates/rpyenv-core/src/shim.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names() {
        let linux = |argv0: &str| command_name(Flavor::Pyenv, OsStr::new(argv0), None);
        assert_eq!(linux("/r/shims/python3.12"), Some("python3.12".to_string()));
        assert_eq!(linux("pip"), Some("pip".to_string()));
        assert_eq!(linux("/usr/lib/pyenv-shim"), None);
        let win = |own: &str| command_name(Flavor::PyenvWin, OsStr::new("x"), Some(Path::new(own)));
        assert_eq!(win("python.exe"), Some("python".to_string()));
        assert_eq!(win("PYENV-SHIM.EXE"), None);
        assert_eq!(command_name(Flavor::PyenvWin, OsStr::new("x"), None), None);
    }
}
```

Add `pub mod shim;` to `crates/rpyenv-core/src/lib.rs`.

Create `crates/pyenv-shim/tests/direct.rs`:

```rust
/// Run under its own name, the shim binary is not a command.
#[test]
fn running_the_shim_binary_directly_is_an_error() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pyenv-shim"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("run this through a shim"));
}
```

- [ ] **Step 2: Create the crate**

`crates/pyenv-shim/Cargo.toml`:

```toml
[package]
name = "pyenv-shim"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true

[[bin]]
name = "pyenv-shim"
path = "src/main.rs"

[dependencies]
rpyenv-core.workspace = true
```

`crates/pyenv-shim/src/main.rs`:

```rust
//! The shim binary: `python`, `pip` and every other shim run this (spec §3, §5).

fn main() {
    std::process::exit(rpyenv_core::shim::main());
}
```

In the root `Cargo.toml`, change `members` to:

```toml
members = ["crates/rpyenv-core", "crates/pyenv", "crates/pyenv-shim"]
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -p rpyenv-core shim::`
Expected: compile errors: `command_name` and `shim::main` not found.

- [ ] **Step 4: Implement**

Put this above the tests in `shim.rs`:

```rust
//! The shim program: a shim named `python` resolves and runs the selected version's
//! `python`, as `pyenv exec python` would (spec §5).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::launch::{self, ExecEnv, Mode};
use crate::rehash;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The shim binary's own name, which is never a command.
pub const SHIM_NAME: &str = "pyenv-shim";

/// Runs the shim and returns the exit code, unless the command replaced this process.
pub fn main() -> i32 {
    let flavor = Flavor::current();
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_default();
    let args: Vec<OsString> = argv.collect();
    let own = std::env::current_exe().ok();
    let Some(program) = command_name(flavor, &argv0, own.as_deref()) else {
        eprint!(
            "pyenv-shim: run this through a shim (such as `python`), not directly{}",
            flavor.eol()
        );
        return 1;
    };
    let ctx = match Ctx::from_process() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprint!("{}{}", e.message(), flavor.eol());
            return 1;
        }
    };
    // Linux shims link to this binary; Windows shims are hardlinks to the template, so
    // the exit check passes the template itself and nothing is copied.
    let rehash_with = match flavor {
        Flavor::Pyenv => own.clone(),
        Flavor::PyenvWin => Some(
            ctx.shims_dir()
                .join(rehash::TEMPLATE_DIR)
                .join(rehash::TEMPLATE_EXE),
        ),
    };
    let env = ExecEnv::from_process(&program, own);
    match launch::plan(&ctx, Mode::Shim, &program, args, &env) {
        Err(report) => {
            report.emit(flavor);
            report.code
        }
        Ok(plan) => {
            for w in &plan.warnings {
                eprint!("{w}{}", flavor.eol());
            }
            launch::run(&plan, &ctx, rehash_with.as_deref())
        }
    }
}

/// The command a shim stands for: `argv[0]`'s last component on Linux, where every shim
/// is a symlink to one binary; the file's own name without `.exe` on Windows, where each
/// shim is a hardlink named after its command. None for the shim binary's own name.
pub fn command_name(flavor: Flavor, argv0: &OsStr, own: Option<&Path>) -> Option<String> {
    let name = match flavor {
        Flavor::Pyenv => Path::new(argv0).file_name()?.to_string_lossy().into_owned(),
        Flavor::PyenvWin => own?.file_stem()?.to_string_lossy().into_owned(),
    };
    (!name.is_empty() && !name.eq_ignore_ascii_case(SHIM_NAME)).then_some(name)
}
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test --workspace`
Expected: PASS, including `shim::command_names` and `direct::running_the_shim_binary_directly_is_an_error`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/rpyenv-core crates/pyenv-shim
git commit -m "Add the pyenv-shim binary"
```

---

### Task 9: `pyenv rehash` and `pyenv shims`

**Files:**
- Create: `crates/pyenv/src/commands/rehash.rs`
- Modify: `crates/pyenv/src/commands/mod.rs`, `crates/pyenv/src/help.rs`
- Modify: `crates/pyenv/tests/common/mod.rs` (`shim_exe`)
- Modify: `crates/pyenv/tests/cli_prefix_versions.rs` (`win_commands_order`)
- Modify: `docs/parity/allowlist.md` (D-35, D-36, D-37)
- Test: `crates/pyenv/tests/cli_rehash.rs`

**Interfaces:**
- Consumes: `rehash::{rehash, RehashError, Wait, LOCK_NAME}` (Task 6), `crate::shim_exe()` (Task 4), `commands::which::print_help` (Task 4), `installed::top_level`.
- Produces: `commands::rehash::{rehash, shims}` (both `Command`); fixture function `common::shim_exe() -> PathBuf`.

Behavior:
- **Linux `rehash`:**
  - Success prints nothing and exits 0.
  - Errors go to stderr with exit 1: ``pyenv: cannot rehash: <shims> isn't writable``, or the lock-timeout pair.
  - `PYENV_REHASH_TIMEOUT` defaults to 60 s.
- **Windows `rehash`:**
  - With no installed version it prints ``No version installed. Please install one with 'pyenv install <version>'.`` on stdout and exits 0.
  - Errors go to stdout.
- **`rehash` needs `pyenv-shim`** next to `pyenv`.
- **Linux `shims`:**
  - Lists the non-hidden entries of `shims`, full paths or with `--short` (first argument only), in byte order.
- **Windows `shims`:**
  - No argument: `dir /s /b` (full paths, recursive) plus one more CRLF.
  - `--short`: `dir /b` (names) plus one more CRLF.
  - Any other argument: the help text plus one more CRLF, exit 0.

- [ ] **Step 1: Add the fixture helper**

Add to `crates/pyenv/tests/common/mod.rs`:

```rust
/// `pyenv-shim` next to the `pyenv` under test. `cargo test --workspace` builds it; when
/// running one test file, run `cargo build --workspace` first.
pub fn shim_exe() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_pyenv"))
        .with_file_name(format!("pyenv-shim{}", std::env::consts::EXE_SUFFIX));
    assert!(
        p.is_file(),
        "{} is missing: run `cargo build --workspace` first",
        p.display()
    );
    p
}
```

- [ ] **Step 2: Write the failing tests**

Create `crates/pyenv/tests/cli_rehash.rs`:

```rust
mod common;
use common::{shim_exe, Fixture};
use std::fs;

#[cfg(unix)]
#[test]
fn rehash_links_each_executable_and_shims_lists_them() {
    let f = Fixture::new();
    f.exe("3.12.1/bin/python");
    f.exe("3.12.1/bin/pip");
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0));
    let shims = f.root.join("shims");
    let target = fs::read_link(shims.join("python")).unwrap();
    assert_eq!(
        fs::canonicalize(target).unwrap(),
        fs::canonicalize(shim_exe()).unwrap()
    );
    assert_eq!(
        f.pyenv(&["shims"]).stdout,
        format!(
            "{}\n{}\n",
            shims.join("pip").display(),
            shims.join("python").display()
        )
    );
    assert_eq!(f.pyenv(&["shims", "--short"]).stdout, "pip\npython\n");
}

#[cfg(unix)]
#[test]
fn rehash_lock_timeout_and_unwritable_messages() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let shims = f.root.join("shims");
    let lock = shims.join(".rehash.lock");
    fs::create_dir_all(&shims).unwrap();
    fs::write(&lock, "").unwrap();
    let r = f.pyenv_env(&["rehash"], &[("PYENV_REHASH_TIMEOUT", "0")]);
    assert_eq!(
        r.stderr,
        format!(
            "pyenv: cannot rehash: couldn't acquire lock {l} for 0 seconds. Last error message:\n\
             {l}: cannot overwrite existing file\n",
            l = lock.display()
        )
    );
    assert_eq!(r.code, 1);
    fs::remove_file(&lock).unwrap();
    fs::set_permissions(&shims, fs::Permissions::from_mode(0o555)).unwrap();
    let root_user = fs::write(shims.join("probe"), "").is_ok();
    if !root_user {
        let r = f.pyenv(&["rehash"]);
        assert_eq!(
            (r.stderr, r.code),
            (
                format!("pyenv: cannot rehash: {} isn't writable\n", shims.display()),
                1
            )
        );
    }
    fs::set_permissions(&shims, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn help_for_rehash_and_shims() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["help", "rehash"]).stdout,
        "Usage: pyenv rehash\n\nRehash pyenv shims (run this after installing executables)\n\n"
    );
    assert_eq!(f.pyenv(&["help", "--usage", "rehash"]).stdout, "");
    assert_eq!(
        f.pyenv(&["help", "--usage", "shims"]).stdout,
        "Usage: pyenv shims [--short]\n"
    );
    let r = f.pyenv(&["shims"]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 0));
}

#[cfg(windows)]
#[test]
fn win_rehash_without_versions() {
    let r = Fixture::new().pyenv(&["rehash"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "No version installed. Please install one with 'pyenv install <version>'.\r\n",
            0
        )
    );
}

#[cfg(windows)]
#[test]
fn win_rehash_and_shims() {
    let f = Fixture::new();
    f.exe("3.9.1/python.exe");
    f.exe("3.9.1/Scripts/pip.exe");
    let shims = f.root.join("shims");
    fs::create_dir_all(&shims).unwrap();
    fs::write(shims.join("python.bat"), "@echo off\r\n").unwrap();
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 0));
    assert_eq!(
        fs::read(shims.join("python.exe")).unwrap(),
        fs::read(shim_exe()).unwrap()
    );
    assert!(!shims.join("python.bat").exists());
    assert_eq!(
        f.pyenv(&["shims"]).stdout,
        format!(
            "{}\r\n{}\r\n\r\n",
            shims.join("pip.exe").display(),
            shims.join("python.exe").display()
        )
    );
    assert_eq!(
        f.pyenv(&["shims", "--short"]).stdout,
        "pip.exe\r\npython.exe\r\n\r\n"
    );
    let help = "Usage: pyenv shims\r\n       pyenv shims --short\r\n\r\nList the existing pyenv shims\r\n\r\n";
    assert_eq!(f.pyenv(&["shims", "--help"]).stdout, help);
    let r = f.pyenv(&["shims", "--other"]);
    assert_eq!((r.stdout, r.code), (format!("{help}\r\n"), 0));
}
```

In `crates/pyenv/tests/cli_prefix_versions.rs`, update `win_commands_order`'s expected string to:

```rust
        "--version\r\ncommands\r\nglobal\r\nhelp\r\nlocal\r\nprefix\r\nrehash\r\nroot\r\nshims\r\nversion-file-read\r\nversion-file-write\r\nversion-file\r\nversion-name\r\nversion-origin\r\nversion\r\nversions\r\nvname\r\nwhence\r\nwhich\r\n"
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_rehash`
Expected: FAIL with ``pyenv: no such command `rehash'`` (Linux) or `pyenv: no such command 'rehash'` (Windows).

- [ ] **Step 4: Implement**

Create `crates/pyenv/src/commands/rehash.rs`:

```rust
//! `rehash` and `shims`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::installed;
use rpyenv_core::rehash::{self, RehashError, Wait};
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

pub fn rehash(ctx: &Ctx, _args: &[&str]) -> Output {
    if ctx.flavor == Flavor::PyenvWin
        && installed::top_level(&ctx.versions_dir(), Flavor::PyenvWin).is_empty()
    {
        let mut o = Output::new();
        o.out("No version installed. Please install one with 'pyenv install <version>'.");
        return o;
    }
    let Some(exe) = crate::shim_exe().filter(|e| e.is_file()) else {
        return fail(ctx, &["pyenv: cannot rehash: pyenv-shim is missing next to pyenv".to_string()]);
    };
    let timeout = std::env::var("PYENV_REHASH_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(60)
        .max(0);
    let lock = ctx.shims_dir().join(rehash::LOCK_NAME);
    match rehash::rehash(ctx, &exe, Wait::Upto(Duration::from_secs(timeout as u64))) {
        Ok(()) => Output::new(),
        Err(RehashError::NotWritable(dir)) => fail(
            ctx,
            &[format!("pyenv: cannot rehash: {} isn't writable", dir.display())],
        ),
        // The second line stands in for bash's noclobber error (allowlist D-35).
        Err(RehashError::Timeout(_) | RehashError::Busy) => fail(
            ctx,
            &[
                format!(
                    "pyenv: cannot rehash: couldn't acquire lock {} for {timeout} seconds. Last error message:",
                    lock.display()
                ),
                format!("{}: cannot overwrite existing file", lock.display()),
            ],
        ),
        Err(RehashError::Io(e)) => fail(ctx, &[format!("pyenv: cannot rehash: {e}")]),
    }
}

/// Upstream prints rehash errors on stderr; on Windows they follow pyenv-win's stdout
/// convention (allowlist D-36).
fn fail(ctx: &Ctx, lines: &[String]) -> Output {
    let mut o = Output::new();
    for line in lines {
        match ctx.flavor {
            Flavor::Pyenv => o.err(line),
            Flavor::PyenvWin => o.out(line),
        }
    }
    o.with_code(1)
}

pub fn shims(ctx: &Ctx, args: &[&str]) -> Output {
    let dir = ctx.shims_dir();
    let mut o = Output::new();
    match ctx.flavor {
        Flavor::Pyenv => {
            let short = args.first() == Some(&"--short");
            let mut names = visible_entries(&dir);
            // Byte order rather than the locale's collation (allowlist D-37).
            names.sort();
            for n in names {
                if short {
                    o.out(n.to_string_lossy());
                } else {
                    o.out(dir.join(&n).display().to_string());
                }
            }
        }
        // pyenv-win relays `dir` output and adds one more line ending (pyenv.vbs:69-82).
        Flavor::PyenvWin => match args.first() {
            None => {
                list_recursive(&dir, &mut o);
                o.out("");
            }
            Some(&"--short") => {
                for n in visible_entries(&dir) {
                    o.out(n.to_string_lossy());
                }
                o.out("");
            }
            Some(_) => return super::which::print_help("shims").with_code(0),
        },
    }
    o
}

/// `dir /s /b`: a folder's entries, then each subfolder's, depth first.
fn list_recursive(dir: &Path, o: &mut Output) {
    let names = visible_entries(dir);
    for n in &names {
        o.out(dir.join(n).display().to_string());
    }
    for n in &names {
        let p = dir.join(n);
        if p.is_dir() {
            list_recursive(&p, o);
        }
    }
}

/// Entries in directory order, without names starting with `.` (rpyenv's own files) and,
/// on Windows, without hidden or system files, which `dir` leaves out.
fn visible_entries(dir: &Path) -> Vec<OsString> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| !hidden(e))
        .map(|e| e.file_name())
        .collect()
}

#[cfg(windows)]
fn hidden(e: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
    e.metadata()
        .map(|m| m.file_attributes() & HIDDEN_OR_SYSTEM != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn hidden(_: &std::fs::DirEntry) -> bool {
    false
}
```

In `crates/pyenv/src/commands/mod.rs`, add `pub mod rehash;` and these rows (keep `COMMANDS` alphabetical):

```rust
    ("rehash", rehash::rehash),
    ("shims", rehash::shims),
```

In `crates/pyenv/src/help.rs`, add to `PYENV`:

```rust
    topic("rehash", Some("Rehash pyenv shims (run this after installing executables)"), None,
        "Usage: pyenv rehash\n\nRehash pyenv shims (run this after installing executables)\n\n"),
    topic("shims", Some("List existing pyenv shims"), Some("Usage: pyenv shims [--short]"),
        "Usage: pyenv shims [--short]\n\nList existing pyenv shims\n\n"),
```

and to `PYENV_WIN`:

```rust
    topic("rehash", None, None,
        "Usage: pyenv rehash\n\nRehash pyenv shims (run this after installing executables)\n\n"),
    topic("shims", None, None,
        "Usage: pyenv shims\n       pyenv shims --short\n\nList the existing pyenv shims\n\n"),
```

- [ ] **Step 5: Add the allowlist rows**

```markdown
| D-35 | Linux | `rehash` | Shims are copies of a bash script; the lock is `shims/.pyenv-shim`; an unregistered directory in `shims` makes `rm` fail with exit 1; the lock-timeout message's second line is bash's noclobber error with the script path; a negative `PYENV_REHASH_TIMEOUT` exits 1 silently | Shims are symlinks to `pyenv-shim`; the lock is `shims/.rehash.lock` and the state `shims/.rehash-state`; directories are left alone; the second line is `<lock>: cannot overwrite existing file`; a negative timeout counts as 0 | Native shims (spec §3.1, §8); bash-specific text. |
| D-36 | Windows | `rehash` | Writes a `.bat` and an extensionless sh shim for each `.exe` (and a `.lnk` for other types) after deleting every file in `shims` | One `<name>.exe` per `.exe`, `.bat` or `.cmd`, hardlinked to `shims\.template\pyenv-shim.exe`; other types get none; unwanted files, including old `.bat`, sh and `.lnk` shims, are removed; a running shim is renamed to `.<name>.old`; errors print on stdout with exit 1 | Native shims (spec §5.3, §8). |
| D-37 | both | `shims` | Linux: sorted by the locale; Windows: `dir` output, which drops only hidden and system files | Linux: byte order; both: names starting with `.` (`.template`, `.rehash-state`, renamed `.old` shims) are left out | Deterministic output; rpyenv's own files are not shims. |
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test --workspace`
Expected: PASS. `cli_rehash` runs 3 tests on Linux and 2 on Windows.

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv docs/parity/allowlist.md
git commit -m "Add rehash and shims commands"
```

---

### Task 10: `pyenv exec`, with arguments kept as bytes

**Files:**
- Create: `crates/pyenv/src/commands/exec.rs`
- Modify: `crates/pyenv/src/main.rs`, `crates/pyenv/src/lib.rs`
- Modify: `crates/pyenv/src/commands/mod.rs`, `crates/pyenv/src/help.rs`
- Modify: `crates/pyenv/tests/cli_prefix_versions.rs` (`win_commands_order`)
- Modify: `docs/parity/allowlist.md` (D-41)
- Test: `crates/pyenv/tests/cli_exec.rs`

**Interfaces:**
- Consumes: `launch::{plan, run, win_exec_version_check, ExecEnv, Mode}` (Task 7), `crate::shim_exe()` (Task 4).
- Produces:
  - `pyenv::run(args: &[OsString], ctx: &Ctx) -> Output` (was `&[String]`)
  - `commands::exec::exec(ctx: &Ctx, args: &[OsString]) -> Output`
  - `commands::exec::exec_listed` (the table's `Command` for `exec`)

`exec` is the one command whose arguments must survive as bytes (review M-5). The dispatcher therefore keeps the raw `OsString`s next to the text it matches command names against, and hands `exec` the raw ones.

- [ ] **Step 1: Write the failing tests**

Create `crates/pyenv/tests/cli_exec.rs`:

```rust
mod common;
use common::Fixture;

#[cfg(unix)]
#[test]
fn exec_runs_the_file_with_the_upstream_environment() {
    let f = Fixture::new();
    let tool = f.exe("3.12.10/bin/tool");
    std::fs::write(
        &tool,
        "#!/bin/sh\nprintf '%s|' \"$0\" \"$@\" \"$PYENV_VERSION\"\nprintf '%s\\n' \"${PATH%%:*}\"\n",
    )
    .unwrap();
    let r = f.pyenv_env(&["exec", "tool", "a b", ""], &[("PYENV_VERSION", "3.12")]);
    assert_eq!(
        (r.stdout, r.code),
        (
            format!(
                "{t}|a b||3.12.10|{b}\n",
                t = tool.display(),
                b = tool.parent().unwrap().display()
            ),
            0
        )
    );
}

#[cfg(unix)]
#[test]
fn exec_usage_and_not_found() {
    let f = Fixture::new();
    let r = f.pyenv(&["exec"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Usage: pyenv exec <command> [arg1 arg2...]\n", 1)
    );
    let r = f.pyenv_env(&["exec", "tool"], &[("PYENV_VERSION", "9.9")]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n\
             pyenv: tool: command not found\n",
            127
        )
    );
}

/// Review M-5: arguments that aren't UTF-8 reach the program unchanged.
#[cfg(unix)]
#[test]
fn exec_passes_non_utf8_arguments() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new();
    let tool = f.exe("3.12.10/bin/tool");
    std::fs::write(&tool, "#!/bin/sh\nprintf '%s' \"$1\"\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pyenv"))
        .arg("exec")
        .arg("tool")
        .arg(std::ffi::OsStr::from_bytes(b"a\xffb"))
        .current_dir(&f.work)
        .env_clear()
        .env("PYENV_ROOT", &f.root)
        .env("PYENV_VERSION", "3.12.10")
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"a\xffb");
}

#[cfg(unix)]
#[test]
fn help_for_exec() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["exec", "--help"]).stdout,
        "Usage: pyenv exec <command> [arg1 arg2...]\n\n\
         Runs an executable by first preparing PATH so that the selected Python\n\
         version's `bin' directory is at the front.\n\n\
         For example, if the currently selected Python version is 2.7.6:\n  \
         pyenv exec pip install -r requirements.txt\n\n\
         is equivalent to:\n  \
         PATH=\"$PYENV_ROOT/versions/2.7.6/bin:$PATH\" pip install -r requirements.txt\n\n"
    );
}

#[cfg(windows)]
const WIN_ENV: [(&str, &str); 2] = [
    ("PYENV_VERSION", "3.9.1"),
    ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
];

#[cfg(windows)]
#[test]
fn win_exec_messages() {
    let f = Fixture::new();
    let r = f.pyenv(&["exec", "python"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "No global/local python version has been set yet. Please set the global/local version by typing:\r\n\
             pyenv global 3.7.4\r\npyenv local 3.7.4\r\n",
            1
        )
    );
    f.version("3.9.1");
    let r = f.pyenv_env(&["exec", "nothing"], &WIN_ENV);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "'nothing' is not recognized as an internal or external command,\r\n\
             operable program or batch file.\r\n",
            1
        )
    );
    let r = f.pyenv_env(&["exec"], &WIN_ENV);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("Usage: pyenv exec <command> [arg1 arg2...]\r\n", 1)
    );
}

#[cfg(windows)]
#[test]
fn win_exec_runs_a_batch_file_with_the_version_first_on_path() {
    let f = Fixture::new();
    let bat = f.exe("3.9.1/Scripts/hello.bat");
    std::fs::write(&bat, "@echo hello %1\r\n@echo %PATH%\r\n").unwrap();
    let r = f.pyenv_env(&["exec", "hello", "world"], &WIN_ENV);
    let v = f.root.join("versions").join("3.9.1");
    let start = format!(
        "hello world\r\n{};{};{};",
        v.display(),
        v.join("Scripts").display(),
        v.join("bin").display()
    );
    assert!(r.stdout.starts_with(&start), "{}", r.stdout);
    assert_eq!(r.code, 0);
}
```

In `crates/pyenv/tests/cli_prefix_versions.rs`, update `win_commands_order`'s expected string to:

```rust
        "--version\r\ncommands\r\nexec\r\nglobal\r\nhelp\r\nlocal\r\nprefix\r\nrehash\r\nroot\r\nshims\r\nversion-file-read\r\nversion-file-write\r\nversion-file\r\nversion-name\r\nversion-origin\r\nversion\r\nversions\r\nvname\r\nwhence\r\nwhich\r\n"
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p pyenv --test cli_exec`
Expected: FAIL with ``pyenv: no such command `exec'`` (Linux) or `pyenv: no such command 'exec'` (Windows).

- [ ] **Step 3: Pass arguments as `OsString`**

`crates/pyenv/src/main.rs`:

```rust
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let output = match Ctx::from_process() {
        Ok(ctx) => pyenv::run(&args, &ctx),
        Err(e) => pyenv::Output::error(e.message()),
    };
    output.emit(Flavor::current());
    std::process::exit(output.code);
}
```

In `crates/pyenv/src/lib.rs`, add `use std::ffi::OsString;`, replace `run`, and replace `run_pyenv`'s signature and its `let args = match args.first() { … };` statement with the lines below (the rest of `run_pyenv` keeps using `args` and `rest` as before):

```rust
/// Runs one invocation. `args` excludes the program name. They stay `OsString`s so that
/// `exec` passes them on byte for byte (review M-5); other commands see them as text.
pub fn run(args: &[OsString], ctx: &Ctx) -> Output {
    let text: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let strs: Vec<&str> = text.iter().map(String::as_str).collect();
    match ctx.flavor {
        Flavor::Pyenv => run_pyenv(&strs, args, ctx),
        Flavor::PyenvWin => run_pyenv_win(&strs, args, ctx),
    }
}

/// Upstream `libexec/pyenv`.
fn run_pyenv(args: &[&str], raw: &[OsString], ctx: &Ctx) -> Output {
    // `--debug` is accepted and ignored (allowlist D-19).
    let skip = usize::from(args.first() == Some(&"--debug"));
    let (args, raw) = (&args[skip..], &raw[skip..]);
```

and in `run_pyenv`'s `match cmd`, add before the `_ => {}` arm:

```rust
        "exec" if rest.first() != Some(&"--help") => {
            return commands::exec::exec(ctx, &raw[1..])
        }
```

Change `run_pyenv_win`'s signature to `fn run_pyenv_win(args: &[&str], raw: &[OsString], ctx: &Ctx) -> Output` and add, right after the `--help`/`help` check:

```rust
    if cmd == "exec" && rest.first() != Some(&"--help") {
        return commands::exec::exec(ctx, &raw[1..]);
    }
```

- [ ] **Step 4: Implement `exec`**

Create `crates/pyenv/src/commands/exec.rs`:

```rust
//! `pyenv exec`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::launch::{self, ExecEnv, Mode};
use rpyenv_core::select;
use std::ffi::OsString;

const USAGE: &str = "Usage: pyenv exec <command> [arg1 arg2...]";

/// `pyenv exec <command> [args...]`. On Linux this process becomes the command, except
/// for pip commands; otherwise the command's exit code comes back in the `Output`.
pub fn exec(ctx: &Ctx, args: &[OsString]) -> Output {
    let command = args
        .first()
        .map(|a| a.to_string_lossy().into_owned())
        .filter(|c| !c.is_empty());
    let Some(command) = command else {
        return usage(ctx);
    };
    let env = ExecEnv::from_process(&command, crate::shim_exe());
    match launch::plan(ctx, Mode::Exec, &command, args[1..].to_vec(), &env) {
        Err(report) => report.into(),
        Ok(plan) => {
            let mut before = Output::new();
            for w in &plan.warnings {
                before.err(w);
            }
            before.emit(ctx.flavor);
            let rehash_with = crate::shim_exe().filter(|e| e.is_file());
            Output::new().with_code(launch::run(&plan, ctx, rehash_with.as_deref()))
        }
    }
}

/// No command. Upstream resolves the version first, so its warnings come before the
/// usage. pyenv-win checks the version first too, then fails on the empty command line;
/// rpyenv prints the usage instead (allowlist D-41).
fn usage(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    match ctx.flavor {
        Flavor::Pyenv => {
            for w in select::version_name(ctx, true).stderr {
                o.err(w);
            }
            o.err(USAGE);
        }
        Flavor::PyenvWin => {
            let names: Vec<String> = select::win_select(ctx)
                .into_iter()
                .map(|s| s.name)
                .collect();
            if let Err(report) = launch::win_exec_version_check(ctx, &names) {
                return report.into();
            }
            o.out(USAGE);
        }
    }
    o.with_code(1)
}

/// The command-table entry, so that `commands` and `help` know `exec`. The dispatcher
/// calls [`exec`] directly with the raw arguments.
pub fn exec_listed(ctx: &Ctx, args: &[&str]) -> Output {
    let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
    exec(ctx, &raw)
}
```

In `crates/pyenv/src/commands/mod.rs`, add `pub mod exec;` and the row (alphabetical, after `commands`):

```rust
    ("exec", exec::exec_listed),
```

In `crates/pyenv/src/help.rs`, add to `PYENV`:

```rust
    topic("exec", Some("Run an executable with the selected Python version"), Some("Usage: pyenv exec <command> [arg1 arg2...]"),
        "Usage: pyenv exec <command> [arg1 arg2...]\n\nRuns an executable by first preparing PATH so that the selected Python\nversion's `bin' directory is at the front.\n\nFor example, if the currently selected Python version is 2.7.6:\n  pyenv exec pip install -r requirements.txt\n\nis equivalent to:\n  PATH=\"$PYENV_ROOT/versions/2.7.6/bin:$PATH\" pip install -r requirements.txt\n\n"),
```

and to `PYENV_WIN` (lines 9 and 12 of pyenv-win's text are a single space):

```rust
    topic("exec", None, None,
        "Usage: pyenv exec <command> [arg1 arg2...]\n\nRuns an executable by first preparing PATH so that the selected Python\nversion's `bin' directory is at the front.\n \nFor example, if the currently selected Python version is 3.5.3:\n  pyenv exec pip install -r requirements.txt\n \nis equivalent to:\n  PATH=\"$PYENV_ROOT/versions/3.5.3/bin:$PATH\" pip install -r requirements.txt\n\n"),
```

- [ ] **Step 5: Add the allowlist row**

```markdown
| D-41 | Windows | `exec` without a command | `\|\| was unexpected at this time.` on stderr, exit 255 (an empty command line) | `Usage: pyenv exec <command> [arg1 arg2...]` on stdout, exit 1 | Batch syntax error. |
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test --workspace`
Expected: PASS. `cli_exec` runs 4 tests on Linux and 2 on Windows.

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv docs/parity/allowlist.md
git commit -m "Add exec, passing arguments on as bytes"
```

---

### Task 11: End-to-end tests with real shims and the `argv-echo` program

**Files:**
- Create: `crates/e2e/Cargo.toml`, `crates/e2e/src/bin/argv_echo.rs`
- Create: `crates/e2e/tests/common/mod.rs`, `crates/e2e/tests/shims.rs`
- Modify: `Cargo.toml` (workspace member)

**Interfaces:**
- Consumes: the `pyenv` and `pyenv-shim` binaries (Tasks 8-10), found next to `argv-echo` in the target directory.
- Produces: the `argv-echo` test program (spec §12.2), and the end-to-end suite.

This task writes no product code. If a test fails, the bug is in Tasks 1-10: fix it in the task's module, and say which in the report.

- [ ] **Step 1: Create the package and `argv-echo`**

`crates/e2e/Cargo.toml`:

```toml
[package]
name = "rpyenv-e2e"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish.workspace = true
autobins = false

[[bin]]
name = "argv-echo"
path = "src/bin/argv_echo.rs"
test = false

[dev-dependencies]
tempfile.workspace = true
```

`crates/e2e/src/bin/argv_echo.rs`:

```rust
//! A test program (spec §12.2). It prints how it was started, one item per line, each
//! value with `{:?}` so tests compare exact bytes:
//! `argv0=`, one `arg=` per argument, `cwd=`, `env NAME=` (or `env NAME unset`) for each
//! name in `ARGV_ECHO_ENV` (comma-separated), and `stdin=` when `ARGV_ECHO_STDIN=1`.
//! Then `ARGV_ECHO_TOUCH=<path>` creates that file (runnable), as `pip install` creates a
//! script; `ARGV_ECHO_SLEEP_MS` waits; `ARGV_ECHO_EXIT` is the exit code (default 0).

use std::io::{Read, Write};

fn main() {
    let mut out = String::new();
    let mut args = std::env::args_os();
    out.push_str(&format!("argv0={:?}\n", args.next().unwrap_or_default()));
    for a in args {
        out.push_str(&format!("arg={a:?}\n"));
    }
    out.push_str(&format!(
        "cwd={:?}\n",
        std::env::current_dir().unwrap_or_default()
    ));
    let var = |k: &str| std::env::var(k).ok();
    for name in var("ARGV_ECHO_ENV")
        .unwrap_or_default()
        .split(',')
        .filter(|n| !n.is_empty())
    {
        match std::env::var_os(name) {
            Some(v) => out.push_str(&format!("env {name}={v:?}\n")),
            None => out.push_str(&format!("env {name} unset\n")),
        }
    }
    if var("ARGV_ECHO_STDIN").as_deref() == Some("1") {
        let mut input = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut input);
        out.push_str(&format!("stdin={:?}\n", String::from_utf8_lossy(&input)));
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_TOUCH") {
        let _ = std::fs::write(&p, b"");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
    }
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    drop(stdout);
    if let Some(ms) = var("ARGV_ECHO_SLEEP_MS").and_then(|v| v.parse().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    std::process::exit(
        var("ARGV_ECHO_EXIT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
}
```

In the root `Cargo.toml`, change `members` to:

```toml
members = ["crates/rpyenv-core", "crates/pyenv", "crates/pyenv-shim", "crates/e2e"]
```

- [ ] **Step 2: Write the fixture**

`crates/e2e/tests/common/mod.rs`:

```rust
#![allow(dead_code)]
//! A temporary PYENV_ROOT whose versions hold copies of `argv-echo`, with real shims made
//! by the real `pyenv rehash`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const EXE: &str = std::env::consts::EXE_SUFFIX;

/// A binary built next to `argv-echo`.
pub fn built(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_argv-echo")).with_file_name(format!("{name}{EXE}"));
    assert!(
        p.is_file(),
        "{} is missing: run `cargo build --workspace` first",
        p.display()
    );
    p
}

/// The line `argv-echo` prints for one value.
pub fn line(key: &str, value: impl AsRef<OsStr>) -> String {
    format!("{key}={:?}", value.as_ref())
}

pub struct Fixture {
    _tmp: tempfile::TempDir,
    pub base: PathBuf,
    pub root: PathBuf,
    pub work: PathBuf,
    /// On PATH after the shims: `<base>/sys/bin`.
    pub syspath: PathBuf,
}

impl Fixture {
    pub fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        // A space and a non-ASCII letter in every path.
        let base = tmp.path().join("py env ñ");
        let root = base.join("root");
        let work = base.join("work");
        let syspath = base.join("sys").join("bin");
        for d in [&root.join("versions"), &work, &syspath] {
            std::fs::create_dir_all(d).unwrap();
        }
        Fixture {
            _tmp: tmp,
            base,
            root,
            work,
            syspath,
        }
    }

    /// Copies `argv-echo` to `versions/<rel>` (`/`-separated, e.g. `3.12.10/bin/python`).
    pub fn install(&self, rel: &str) -> PathBuf {
        let p = rel
            .split('/')
            .fold(self.root.join("versions"), |p, c| p.join(c));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::copy(built("argv-echo"), &p).unwrap();
        p
    }

    pub fn shim(&self, name: &str) -> PathBuf {
        self.root.join("shims").join(format!("{name}{EXE}"))
    }

    /// A command with a clean environment: the root, the shims first on PATH, then `syspath`.
    pub fn command(&self, program: &Path, env: &[(&str, &OsStr)]) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(&self.work)
            .env_clear()
            .env("PYENV_ROOT", &self.root)
            .env("HOME", &self.base)
            .env("USERPROFILE", &self.base)
            .env(
                "PATH",
                std::env::join_paths([self.root.join("shims"), self.syspath.clone()]).unwrap(),
            )
            .env("PWD", &self.work);
        for k in ["SystemRoot", "PATHEXT"] {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    pub fn pyenv(&self, args: &[&str], env: &[(&str, &OsStr)]) -> Output {
        self.command(&built("pyenv"), env)
            .args(args)
            .output()
            .unwrap()
    }

    pub fn rehash(&self) {
        let out = self.pyenv(&["rehash"], &[]);
        assert!(
            out.status.success(),
            "rehash failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn shim_command(&self, name: &str, env: &[(&str, &OsStr)]) -> Command {
        self.command(&self.shim(name), env)
    }

    pub fn run_shim(&self, name: &str, args: &[std::ffi::OsString], env: &[(&str, &OsStr)]) -> Output {
        self.shim_command(name, env).args(args).output().unwrap()
    }
}
```

- [ ] **Step 3: Write the tests**

`crates/e2e/tests/shims.rs`:

```rust
mod common;
use common::*;
use std::ffi::{OsStr, OsString};
use std::process::Stdio;
use std::time::{Duration, Instant};

fn v(s: &str) -> &OsStr {
    OsStr::new(s)
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[cfg(unix)]
#[test]
fn shim_passes_arguments_byte_for_byte() {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new();
    let python = f.install("3.12.10/bin/python");
    f.rehash();
    let mut args: Vec<OsString> = ["a b", "", "x^y", "100%", "%USERNAME%", "a&b", "say \"hi\"", "ñ"]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(OsString::from_vec(b"\xff\xfe".to_vec()));
    let out = f.run_shim(
        "python",
        &args,
        &[("PYENV_VERSION", v("3.12.10")), ("ARGV_ECHO_EXIT", v("7"))],
    );
    let mut expected = vec![line("argv0", &python)];
    expected.extend(args.iter().map(|a| line("arg", a)));
    let text = stdout(&out);
    assert_eq!(text.lines().take(expected.len()).collect::<Vec<_>>(), expected);
    assert_eq!(out.status.code(), Some(7));
}

#[cfg(unix)]
#[test]
fn shim_and_exec_export_the_upstream_environment() {
    let f = Fixture::new();
    let python = f.install("3.12.10/bin/python");
    f.rehash();
    let env = [
        ("PYENV_VERSION", v("3.12")),
        ("ARGV_ECHO_ENV", v("PYENV_VERSION,PYENV_ROOT,PATH")),
    ];
    let mut path = python.parent().unwrap().as_os_str().to_os_string();
    path.push(":");
    path.push(std::env::join_paths([f.root.join("shims"), f.syspath.clone()]).unwrap());
    let expected = [
        line("env PYENV_VERSION", "3.12.10"),
        line("env PYENV_ROOT", &f.root),
        line("env PATH", &path),
    ];
    for out in [f.run_shim("python", &[], &env), f.pyenv(&["exec", "python"], &env)] {
        let text = stdout(&out);
        for e in &expected {
            assert!(text.lines().any(|l| l == e), "missing {e} in:\n{text}");
        }
    }
}

#[cfg(unix)]
#[test]
fn not_found_exits_127_listing_the_versions_that_have_it() {
    let f = Fixture::new();
    f.install("3.11.9/bin/tool");
    f.install("3.12.10/bin/python");
    f.rehash();
    let out = f.run_shim("tool", &[], &[("PYENV_VERSION", v("3.12.10"))]);
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "pyenv: tool: command not found\n\n\
         The `tool' command exists in these Python versions:\n  \
         3.11.9\n\n\
         Note: See 'pyenv help global' for tips on allowing multiple\n      \
         Python versions to be found at the same time.\n"
    );
    assert_eq!(out.status.code(), Some(127));
}

#[cfg(unix)]
#[test]
fn pip_shim_rehashes_before_returning() {
    let f = Fixture::new();
    f.install("3.12.10/bin/pip");
    f.rehash();
    let script = f.root.join("versions/3.12.10/bin/black");
    let out = f.run_shim(
        "pip",
        &["install".into(), "black".into()],
        &[
            ("PYENV_VERSION", v("3.12.10")),
            ("ARGV_ECHO_TOUCH", script.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(f.shim("black").exists(), "the exit check did not rehash");
}

#[cfg(unix)]
#[test]
fn sigterm_reaches_the_child_and_the_shim_dies_the_same_way() {
    use std::os::unix::process::ExitStatusExt;
    let f = Fixture::new();
    f.install("3.12.10/bin/pip");
    f.rehash();
    let start = Instant::now();
    let mut shim = f
        .shim_command(
            "pip",
            &[
                ("PYENV_VERSION", v("3.12.10")),
                ("ARGV_ECHO_SLEEP_MS", v("20000")),
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let killed = std::process::Command::new("kill")
        .args(["-TERM", &shim.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let status = shim.wait().unwrap();
    assert_eq!(status.signal(), Some(15));
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "the child kept running: SIGTERM was not passed on"
    );
}

#[cfg(unix)]
#[test]
fn stdin_reaches_the_program() {
    use std::io::Write;
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let mut child = f
        .shim_command(
            "python",
            &[("PYENV_VERSION", v("3.12.10")), ("ARGV_ECHO_STDIN", v("1"))],
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(stdout(&out).lines().any(|l| l == "stdin=\"hello\\n\""));
}

/// Review M-4 and review focus 3.
#[cfg(unix)]
#[test]
fn works_in_a_deleted_directory() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let doomed = f.base.join("doomed");
    std::fs::create_dir_all(&doomed).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .args(["-c", r#"cd "$1" && export PWD && /bin/rmdir "$1" && exec "$2""#, "sh"])
        .arg(&doomed)
        .arg(f.shim("python"))
        .env_clear()
        .env("PYENV_ROOT", &f.root)
        .env("PYENV_VERSION", "3.12.10")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Review focus 1: a symlink to a shim elsewhere on PATH must not make `system` recurse.
#[cfg(unix)]
#[test]
fn system_command_never_finds_the_shim() {
    let f = Fixture::new();
    f.install("3.12.10/bin/tool");
    f.rehash();
    let links = f.base.join("links");
    std::fs::create_dir_all(&links).unwrap();
    std::os::unix::fs::symlink(f.shim("tool"), links.join("tool")).unwrap();
    let real = f.syspath.join("tool");
    std::fs::copy(built("argv-echo"), &real).unwrap();
    let path = std::env::join_paths([links, f.root.join("shims"), f.syspath.clone()]).unwrap();
    let out = f
        .shim_command(
            "tool",
            &[("PYENV_VERSION", v("system")), ("PATH", path.as_os_str())],
        )
        .output()
        .unwrap();
    assert_eq!(stdout(&out).lines().next(), Some(line("argv0", &real).as_str()));
}

#[cfg(windows)]
#[test]
fn win_shim_passes_plain_arguments_and_the_exit_code() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let args: Vec<OsString> = ["a b", "ñ", "plain"].iter().map(OsString::from).collect();
    let out = f.run_shim(
        "python",
        &args,
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v("7"))],
    );
    let text = stdout(&out);
    for a in &args {
        assert!(text.lines().any(|l| l == line("arg", a)), "{text}");
    }
    assert_eq!(out.status.code(), Some(7));
}

#[cfg(windows)]
#[test]
fn win_not_found_exits_127() {
    let f = Fixture::new();
    f.install("3.8.2/tool.exe");
    f.install("3.9.1/python.exe");
    f.rehash();
    let out = f.run_shim("tool", &[], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(
        stdout(&out),
        "pyenv: tool: command not found\r\n\r\nThe 'tool' command exists in these Python versions:\r\n  3.8.2\r\n  \r\n"
    );
    assert_eq!(out.status.code(), Some(127));
}

#[cfg(windows)]
#[test]
fn win_every_shim_runs_the_rehash_check() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let script = scripts.join("black.exe");
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("ARGV_ECHO_TOUCH", script.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(f.shim("black").is_file(), "the exit check did not rehash");
}

#[cfg(windows)]
#[test]
fn win_batch_target_runs_through_its_exe_shim() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("hello.bat"), "@echo hello %1\r\n").unwrap();
    f.rehash();
    let out = f.run_shim("hello", &["world".into()], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(stdout(&out), "hello world\r\n");
}

/// Review focus 5: a running shim can't be deleted, so rehash renames it aside.
#[cfg(windows)]
#[test]
fn win_running_shim_is_renamed_aside() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let bat = scripts.join("tool.bat");
    // The shim stays running while cmd runs this for about 3 s.
    std::fs::write(&bat, "@\"%SystemRoot%\\System32\\ping.exe\" -n 4 127.0.0.1 > NUL\r\n").unwrap();
    f.rehash();
    let mut running = f
        .shim_command("tool", &[("PYENV_VERSION", v("3.9.1"))])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    std::fs::remove_file(&bat).unwrap();
    f.rehash();
    let shims = f.root.join("shims");
    assert!(!shims.join("tool.exe").exists());
    assert!(shims.join(".tool.exe.old").exists());
    let _ = running.wait();
    f.rehash();
    assert!(!shims.join(".tool.exe.old").exists());
}
```

- [ ] **Step 4: Run the suite**

Run: `cargo build --workspace`, then `cargo test -p rpyenv-e2e`
Expected: PASS. 8 tests on Linux, 5 on Windows. If one fails, fix the product code in the module the failure points to, keep the test as written, and report the fix.

- [ ] **Step 5: Run everything once**

Run: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
Expected: clean and PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/e2e
git commit -m "Add end-to-end tests with real shims and argv-echo"
```

---

## After this plan

- **M1b-win (next plan):** Windows launch fidelity, written against this plan's `launch` and `shim` modules:
  - the raw command-line tail (`GetCommandLineW`) instead of re-quoted arguments, with the tricky-argument tests from spec §12.2 (`^ % ! & "`, empty, Unicode);
  - the Job Object (`KILL_ON_JOB_CLOSE`, `SILENT_BREAKAWAY_OK`), with a kill-the-shim test run while the child is still alive;
  - the Ctrl+C handler, and exit codes such as `0xC000013A`;
  - NO-WINDOW and MIRROR modes;
  - `pyenv-shimw` and the PE `Subsystem` choice in rehash;
  - `RPYENV_BATCH_FORWARD` forwarders.
- **Known interim gaps on Windows until M1b-win:**
  - Arguments are split and re-quoted, so cmd-special characters may not arrive intact.
  - Ctrl+C can end the shim before its child.
  - Killing the shim leaves the child running.
  - GUI programs get the console shim.
- **M1c:** parity CI (adapted upstream suites, differential tests against the allowlist, the shim dependency guard, the benchmark).
- **Not in M1:** `PYENV_DEBUG` tracing in the shim (spec §11); `--complete` arguments (M3).
