# M4a Plugin Dispatch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `pyenv foo` runs a `pyenv-foo` plugin from the plugin folders or `PATH`, in the environment upstream's dispatcher builds. Plugins show up in `pyenv commands`, `pyenv help` and `pyenv completions`, and plugin `sh-*` commands join `init`'s shell function. Existing bash plugins work unchanged on Linux (spec §6).

**Architecture:**
- **Discovery and environment:** a new `rpyenv_core::plugins` module computes the dispatch `PATH` (`<prefix>/libexec`, then plugin `bin` folders, then the inherited `PATH`), finds `pyenv-<cmd>`, and lists `pyenv-*` names. It also builds `PYENV_HOOK_PATH`.
- **Running a plugin:** the CLI's new `plugin.rs` runs it through the existing `launch` machinery: `exec` on Linux, spawn and wait on Windows.
- **Built-ins for bash plugins:** bash plugins call built-ins by name (`pyenv-version-name`), so on Linux rpyenv keeps a `pyenv-<cmd>` symlink per built-in in `$PYENV_ROOT/.rpyenv/libexec`. Invoked through such a link, the binary acts as `pyenv <cmd>` (multicall).

**Tech Stack:** Rust 2021; bats 1.11.1 against pyenv 2.8.8 (07171d0).

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`: §2 (decision D4: git-style plugins, no bash hooks), §6 ("Plugin dispatch"), §12, §14 item 4. Upstream facts:
- `docs/parity/pyenv-m1-reference.md` (**M1L**): "Environment setup", "Command lookup", "`pyenv help`" Modes 1–3 and "Comment-block parsing";
- `docs/parity/pyenv-m3-reference.md` (**M3L**): "`pyenv completions`" and "The `pyenv` shell function and which commands it routes".

## Decisions

1. **Built-ins always win over plugins.** Upstream puts `libexec` first on `PATH`, so a plugin can't shadow a built-in. rpyenv looks in its command table first and dispatches to a plugin only for a name it doesn't have.
2. **Built-in links for bash plugins (Linux).**
   - **What:** `$PYENV_ROOT/.rpyenv/libexec/pyenv-<cmd>` is a symlink to the running `pyenv` binary, one for each built-in, `sh-*` included. It is made or repaired on every plugin run and goes on the plugin's `PATH` after the plugin folders.
   - **Why:** plugins like pyenv-virtualenv call `pyenv-version-name`, `pyenv-prefix` and others by name. Spec §6 promises "existing bash plugins work unchanged", and rpyenv has no `libexec`.
   - **Where:** in `PYENV_ROOT` because that is writable without an installer. If it isn't writable, the plugin still runs, without the links.
   - **Who uses `<prefix>/libexec`:** it stays first on `PATH` (upstream's slot) for M6's installers to fill. The bats harness already puts wrappers there.
3. **Multicall.** When `argv[0]`'s file stem is `pyenv-<cmd>` and `<cmd>` is a built-in, `pyenv` runs `pyenv <cmd> <args>`. A stem naming no built-in is ignored, so a stray link can't loop back into plugin dispatch.
4. **The plugin environment** is exactly upstream's (M1L "Environment setup"):
   - `PYENV_ROOT` (normalized), `PYENV_DIR` (absolute) and `_PYENV_INSTALL_PREFIX`;
   - `PATH` = `<prefix>/libexec`, then `$PYENV_ROOT/plugins/*/bin` (only when the prefix isn't the root), then `<prefix>/plugins/*/bin`, each in reverse glob order; then the built-in links (Decision 2); then the inherited `PATH`;
   - `PYENV_HOOK_PATH` (Linux), built as upstream builds it.

   rpyenv runs no hooks (D-51), but it exports the variable so plugins that read it work.
5. **Windows plugins** are `PATHEXT` executables (`.exe`, `.bat`, `.cmd`) named `pyenv-<cmd>`. They are found in `%PYENV_ROOT%\plugins\*\bin` and on `PATH`, and get `PYENV_ROOT`, `PYENV_DIR`, `_PYENV_INSTALL_PREFIX` and `PATH`. There are no built-in links (pyenv-win has no bash plugins) and no `PYENV_HOOK_PATH`. pyenv-win has no plugin dispatch at all; allowlist row **D-93**.
6. **`pyenv completions <plugin>`** prints `--help`, then the output of `<plugin> --complete <args>` when the file has the marker (M3L). rpyenv spawns the plugin and appends its output, where upstream `exec`s after printing. The output is the same.
7. **D-88 is narrowed, not retired.**
   - Narrowed: the routed set now adds plugin `pyenv-sh-*` files from the dispatch `PATH`, as upstream's does.
   - Still different: upstream calls `pyenv-commands --sh` from `PATH`, so a stub `pyenv-commands` changes the set. rpyenv's `commands` is built into the binary.
   - Effect: the bats test "outputs sh-compatible case syntax" stays an expected failure under the reworded D-88. The diff case "init - ksh with a plugin sh- command" becomes `same`, so it loses its `allow`.
8. **Help for a plugin** reads the plugin file's leading comment block exactly as `pyenv-help` does (M1L "Comment-block parsing"; `libexec/pyenv-help:29-90`, quoted in Task 3). That applies to both flavors; a Windows `.exe` simply has no `#` block and is "not documented yet".

New allowlist row:

| Row | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-93 | Windows | `pyenv <cmd>` for a command that isn't built in | pyenv-win: `pyenv: no such command '<cmd>'` | Runs `pyenv-<cmd>` (a `PATHEXT` executable) from `%PYENV_ROOT%\plugins\*\bin` or `PATH` | Plugin dispatch (spec §2 D4, §6) |

D-88's new text:

| Row | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-88 | Linux | `init -` (the shell function) | The routed commands come from running `pyenv-commands --sh`, so a stub `pyenv-commands` on `PATH` changes them | Its own listing: the built-in `sh-*` commands plus `pyenv-sh-*` plugins on the dispatch `PATH` | Built-in commands are part of the binary (spec §3); a stub can't replace one |

## Global Constraints

- **Linux output is byte-identical to pyenv 2.8.8** unless an allowlist row says otherwise (spec §4).
- Text output uses `\n`; `Output::emit` turns it into CRLF on Windows.
- **Never run rpyenv or upstream against a real root.** Tests use the `Fixture`, whose `PYENV_ROOT` and `HOME` are a tempdir. WSL work runs from script files.
- **No link to a real install from any test.** Plugins in tests are scripts written into the fixture.
- Commits have a one-line message and no attribution lines.
- Each task ends green on Windows and in WSL: fmt, clippy `-D warnings`, `cargo test --workspace`.

## Review Focus

1. **A plugin file named like a built-in** (`plugins/x/bin/pyenv-version`): the built-in runs. Task 2 test `a_plugin_cannot_shadow_a_built_in`.
2. **A bash plugin that calls `pyenv-prefix` by name** gets rpyenv's answer. Task 2 test `a_bash_plugin_reaches_built_ins_by_name`.
3. **A read-only `PYENV_ROOT`**, where the built-in links can't be made: the plugin still runs. Task 2 test `a_plugin_runs_when_the_root_is_read_only`.
4. **A `pyenv-foo` file that isn't executable:** `pyenv commands` lists it, as upstream's glob does, but `pyenv foo` says "no such command", as `command -v` does. Task 3 test `a_non_executable_plugin_is_listed_but_not_run`.
5. **A plugin folder or `PATH` entry with a space:** upstream's "commands in path with spaces" covers it (Task 5), and the fixture's root has a space in every test.

## File map

| File | Task | Responsibility |
|---|---|---|
| `crates/rpyenv-core/src/plugins.rs` (new) | 1 | Dispatch folders, `PATH`, `PYENV_HOOK_PATH`, find, list |
| `crates/pyenv/src/plugin.rs` (new) | 2 | Running a plugin; built-in links |
| `crates/pyenv/src/main.rs` | 2 | Multicall by `argv[0]` |
| `crates/pyenv/src/lib.rs` | 2, 4 | Dispatcher fallback to plugins on both flavors; `is_builtin` |
| `crates/pyenv/src/commands/mod.rs` | 2, 3 | `builtin_names`, `command_names` |
| `crates/pyenv/src/help.rs` | 3 | `Doc`, `parse_doc`, plugin help and listing |
| `crates/pyenv/src/commands/{misc,completions,init,init_win}.rs` | 3 | Listing, completion forwarding and routed set include plugins |
| `crates/pyenv/tests/cli_plugins.rs` (new) | 2, 3 | Linux tests |
| `crates/pyenv/tests/cli_plugins_win.rs` (new) | 4 | Windows tests |
| `parity/bats_run.sh`, `parity/expected/bats.txt`, `parity/diff.py`, `parity/diff_cases.py`, goldens, `docs/parity/allowlist.md`, spec §6 | 5 | Parity |

---

### Task 1: Plugin discovery and the dispatch environment (core)

**Files:**
- Create: `crates/rpyenv-core/src/plugins.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod plugins;`)

**Interfaces:**
- Produces:
  - `rpyenv_core::plugins::{front_dirs(&Path, Option<&Path>) -> Vec<PathBuf>, dispatch_path(&[PathBuf], Option<&OsStr>, Flavor) -> OsString, hook_path(Option<&str>, &Path, Option<&Path>) -> String, find(&str, &OsStr, Flavor, Option<&OsStr>) -> Option<PathBuf>, listed_names(&OsStr, Flavor, Option<&OsStr>) -> Vec<String>}`.

- [ ] **Step 1: Write the failing tests.** Create `plugins.rs` with only this test module, and add `pub mod plugins;` to `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dirs(base: &Path, plugins: &[&str]) {
        for p in plugins {
            fs::create_dir_all(base.join("plugins").join(p).join("bin")).unwrap();
        }
    }

    /// test/pyenv.bats:49-58: libexec, the root's plugins (reverse glob order), then the
    /// prefix's; a root equal to the prefix is read once.
    #[test]
    fn front_dirs_follow_upstream_order() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, prefix) = (tmp.path().join("root"), tmp.path().join("prefix"));
        dirs(&root, &["python-build", "pyenv-each", ".hidden"]);
        dirs(&prefix, &["python-build"]);
        assert_eq!(
            front_dirs(&root, Some(&prefix)),
            vec![
                prefix.join("libexec"),
                root.join("plugins/python-build/bin"),
                root.join("plugins/pyenv-each/bin"),
                prefix.join("plugins/python-build/bin"),
            ]
        );
        assert_eq!(
            front_dirs(&root, Some(&root)),
            vec![
                root.join("libexec"),
                root.join("plugins/python-build/bin"),
                root.join("plugins/pyenv-each/bin"),
            ]
        );
    }

    /// test/pyenv.bats:60-72: the inherited value first, one leading `:` dropped.
    #[test]
    fn hook_path_as_upstream() {
        let root = Path::new("/r");
        let prefix = Path::new("/p");
        let tail = ":/usr/etc/pyenv.d:/usr/local/etc/pyenv.d:/etc/pyenv.d:/usr/lib/pyenv/hooks";
        assert_eq!(
            hook_path(None, root, Some(prefix)),
            format!("/r/pyenv.d:/p/pyenv.d{tail}")
        );
        assert_eq!(
            hook_path(Some("/my/hook/path:/other/hooks"), root, Some(root)),
            format!("/my/hook/path:/other/hooks:/r/pyenv.d{tail}")
        );
    }

    #[cfg(unix)]
    #[test]
    fn find_wants_an_executable_and_list_wants_any_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("my commands");
        fs::create_dir_all(&bin).unwrap();
        for (name, mode) in [("pyenv-hello", 0o755), ("pyenv-sh-hi", 0o755), ("pyenv-plain", 0o644)] {
            fs::write(bin.join(name), "#!/bin/sh\n").unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(mode)).unwrap();
        }
        let path = dispatch_path(&[bin.clone()], Some(OsStr::new("/nonexistent")), Flavor::Pyenv);
        assert_eq!(find("hello", &path, Flavor::Pyenv, None), Some(bin.join("pyenv-hello")));
        assert_eq!(find("plain", &path, Flavor::Pyenv, None), None);
        let mut names = listed_names(&path, Flavor::Pyenv, None);
        names.sort();
        assert_eq!(names, ["hello", "plain", "sh-hi"]);
    }
}
```

- [ ] **Step 2: Run the tests and see them fail.**

Run: `cargo test -p rpyenv-core --lib plugins::` (Windows), and the same in WSL.
Expected: compile errors, because the functions don't exist yet.

- [ ] **Step 3: Implement.** Above the test module:

```rust
//! Plugin dispatch (spec §6, decision D4): `pyenv foo` runs `pyenv-foo` from the plugin
//! folders or `PATH`, in the environment upstream's dispatcher builds (libexec/pyenv:79-106;
//! M1 reference "Environment setup").

use crate::flavor::Flavor;
use crate::pathsearch;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// `<base>/plugins/*/<sub>` that exist, in glob order (byte order, no dot names).
fn plugin_dirs(base: &Path, sub: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(base.join("plugins")) else {
        return Vec::new();
    };
    let mut names: Vec<OsString> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| !n.to_string_lossy().starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| base.join("plugins").join(n).join(sub))
        .filter(|d| d.is_dir())
        .collect()
}

/// The folders the dispatcher puts in front of `PATH`, first to last: `<prefix>/libexec`,
/// `$PYENV_ROOT/plugins/*/bin` (only when the prefix isn't the root), then
/// `<prefix>/plugins/*/bin`. Each glob is reversed, because upstream prepends every match
/// in turn (libexec/pyenv:84-94).
pub fn front_dirs(root: &Path, prefix: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = prefix {
        v.push(p.join("libexec"));
    }
    if prefix != Some(root) {
        v.extend(plugin_dirs(root, "bin").into_iter().rev());
    }
    if let Some(p) = prefix {
        v.extend(plugin_dirs(p, "bin").into_iter().rev());
    }
    v
}

/// `front`, then the inherited `PATH`, joined as text with the flavor's separator, as
/// upstream's `export PATH="…:${PATH}"` does (no validation of the entries).
pub fn dispatch_path(front: &[PathBuf], inherited: Option<&OsStr>, flavor: Flavor) -> OsString {
    let sep = match flavor {
        Flavor::Pyenv => ":",
        Flavor::PyenvWin => ";",
    };
    let mut s = OsString::new();
    for d in front {
        s.push(d.as_os_str());
        s.push(sep);
    }
    if let Some(p) = inherited {
        s.push(p);
    }
    s
}

/// `PYENV_HOOK_PATH` as upstream exports it (libexec/pyenv:96-106).
pub fn hook_path(inherited: Option<&str>, root: &Path, prefix: Option<&Path>) -> String {
    let mut s = inherited.unwrap_or("").to_string();
    s.push_str(&format!(":{}/pyenv.d", root.display()));
    if let Some(p) = prefix.filter(|p| *p != root) {
        s.push_str(&format!(":{}/pyenv.d", p.display()));
    }
    s.push_str(":/usr/etc/pyenv.d:/usr/local/etc/pyenv.d:/etc/pyenv.d:/usr/lib/pyenv/hooks");
    for d in plugin_dirs(root, "etc/pyenv.d") {
        s.push_str(&format!(":{}", d.display()));
    }
    s.strip_prefix(':').map(str::to_string).unwrap_or(s)
}

/// `command -v pyenv-<name>` on the dispatch `PATH`: the first executable file, or on
/// Windows the first `PATHEXT` match.
pub fn find(name: &str, path: &OsStr, flavor: Flavor, pathext: Option<&OsStr>) -> Option<PathBuf> {
    pathsearch::find_first(&format!("pyenv-{name}"), Some(path), None, flavor, pathext)
}

/// The names `pyenv-*` files on the dispatch `PATH` give, `pyenv-` removed, each once
/// (libexec/pyenv-commands:23-47). Linux takes any file, as upstream's glob does; Windows
/// takes `PATHEXT` files and drops the extension.
pub fn listed_names(path: &OsStr, flavor: Flavor, pathext: Option<&OsStr>) -> Vec<String> {
    let exts: Vec<String> = pathext
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".com;.exe;.bat;.cmd".to_string())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect();
    let mut names = Vec::new();
    for dir in std::env::split_paths(path) {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = file.strip_prefix("pyenv-") else {
                continue;
            };
            let name = match flavor {
                Flavor::Pyenv => Some(rest.to_string()),
                Flavor::PyenvWin => {
                    let lower = rest.to_ascii_lowercase();
                    exts.iter()
                        .find(|x| lower.ends_with(x.as_str()))
                        .map(|x| rest[..rest.len() - x.len()].to_string())
                }
            };
            if let Some(n) = name.filter(|n| !n.is_empty()) {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
    }
    names
}
```

On Linux `std::env::split_paths` splits on `:`, and on Windows on `;`, which is the separator `dispatch_path` used for each flavor's own OS.

- [ ] **Step 4: Run the tests and see them pass.**

Run: `cargo test -p rpyenv-core --lib plugins::`. Expected: 2 passed on Windows (the Unix test is cfg'd out) and 3 in WSL.

- [ ] **Step 5: Commit.** `git commit -m "Add plugin discovery and the dispatch environment"`

---

### Task 2: Running a plugin, built-in links and multicall (Linux)

**Files:**
- Create: `crates/pyenv/src/plugin.rs`, `crates/pyenv/tests/cli_plugins.rs`
- Modify:
  - `crates/pyenv/src/lib.rs`: `mod plugin;`, `pub fn is_builtin`, and the `run_pyenv` fallback and `shell` arm;
  - `crates/pyenv/src/main.rs`;
  - `crates/pyenv/src/commands/mod.rs`: `builtin_names`.

**Interfaces:**
- Consumes: Task 1; `launch::{LaunchPlan, run}`; `crate::install_prefix`.
- Produces:
  - `plugin::dispatch_path(&Ctx) -> OsString`;
  - `plugin::run(&Ctx, &str, &[OsString]) -> Option<Output>`;
  - `pub fn is_builtin(Flavor, &str) -> bool`;
  - `commands::builtin_names(Flavor) -> Vec<&'static str>`.

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/cli_plugins.rs`:

```rust
//! Plugin dispatch on Linux (spec §6; M1 reference "Environment setup").
#![cfg(unix)]

mod common;
use common::Fixture;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// An executable shell script at `root/plugins/<plugin>/bin/pyenv-<cmd>`.
fn plugin(f: &Fixture, plugin: &str, cmd: &str, body: &str) -> std::path::PathBuf {
    let p = f.root.join("plugins").join(plugin).join("bin").join(format!("pyenv-{cmd}"));
    f.file(&p, &format!("#!/bin/sh\n{body}"));
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

#[test]
fn a_plugin_runs_with_the_dispatcher_s_environment() {
    let f = Fixture::new();
    plugin(&f, "hello", "hello", "printf '%s|' \"$@\"; echo \"$PYENV_ROOT|$PYENV_DIR|${PYENV_HOOK_PATH%%:*}\"\n");
    let r = f.root.display();
    let w = f.work.display();
    assert_eq!(
        run(&f, &["hello", "a b", "c"]),
        (format!("a b|c|{r}|{w}|{r}/pyenv.d\n"), String::new(), 0)
    );
    // The plugin folders and the built-in links lead PATH, after <prefix>/libexec.
    plugin(&f, "path", "path", "echo \"$PATH\" | tr ':' '\\n' | head -n 3\n");
    let out = run(&f, &["path"]).0;
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].ends_with("/libexec"), "{out}");
    assert_eq!(lines[1], format!("{r}/plugins/path/bin"));
    assert_eq!(lines[2], format!("{r}/plugins/hello/bin"));
}

#[test]
fn unknown_commands_and_help() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["nosuch"]),
        (String::new(), "pyenv: no such command `nosuch'\n".to_string(), 1)
    );
    plugin(&f, "x", "sh-hi", "echo hi\n");
    assert_eq!(run(&f, &["sh-hi", "--help"]), ("pyenv help \"sh-hi\"\n".to_string(), String::new(), 0));
}

/// Review focus 1.
#[test]
fn a_plugin_cannot_shadow_a_built_in() {
    let f = Fixture::new();
    plugin(&f, "x", "root", "echo plugin\n");
    assert_eq!(run(&f, &["root"]).0, format!("{}\n", f.root.display()));
}

/// Review focus 2 and Decision 2: `pyenv-prefix` by name reaches rpyenv through the links.
#[test]
fn a_bash_plugin_reaches_built_ins_by_name() {
    let f = Fixture::new();
    f.version("3.12.1");
    plugin(&f, "x", "where", "pyenv-prefix 3.12.1; pyenv-version-name\n");
    let r = f.pyenv_env(&["where"], &[("PYENV_VERSION", "3.12.1")]);
    assert_eq!(
        (r.stdout, r.code),
        (format!("{}/versions/3.12.1\n3.12.1\n", f.root.display()), 0),
        "{}",
        r.stderr
    );
    let link = f.root.join(".rpyenv/libexec/pyenv-version-name");
    assert_eq!(std::fs::read_link(&link).unwrap(), Path::new(env!("CARGO_BIN_EXE_pyenv")).canonicalize().unwrap());
}

/// Review focus 3.
#[test]
fn a_plugin_runs_when_the_root_is_read_only() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let f = Fixture::new();
    plugin(&f, "x", "hello", "echo hi\n");
    std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let got = run(&f, &["hello"]);
    std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(got, ("hi\n".to_string(), String::new(), 0));
}

/// Decision 3: a link named `pyenv-<built-in>` acts as `pyenv <built-in>`.
#[test]
fn multicall_by_argv0() {
    let f = Fixture::new();
    let link = f.base.join("pyenv-root");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_pyenv"), &link).unwrap();
    let o = f.command(&link, &f.work, &[]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("{}\n", f.root.display()));
}
```

- [ ] **Step 2: Run the tests and see them fail.** Run in WSL: `cargo test -p pyenv --test cli_plugins`. Expected: every test fails, either with "no such command" or because the multicall link prints usage.

- [ ] **Step 3: Implement.**
  - **`commands/mod.rs`:** add

```rust
/// The command table's names as they are (`sh-*` included), for the built-in links.
pub fn builtin_names(flavor: Flavor) -> Vec<&'static str> {
    table(flavor).map(|&(n, _)| n).collect()
}
```

  - **`lib.rs`:** add `mod plugin;` and

```rust
/// True when `name` is a built-in command of `flavor` (multicall, Decision 3).
pub fn is_builtin(flavor: Flavor, name: &str) -> bool {
    commands::lookup(flavor, name).is_some()
}
```

  - In `run_pyenv`, replace the `"shell"` arm with:

```rust
        // Only when no `pyenv-shell` is on the dispatch PATH (libexec/pyenv:127-128).
        "shell" if plugin::find(ctx, "shell").is_none() => {
            return Output::error(
                "pyenv: shell integration not enabled. Run `pyenv init' for instructions.",
            )
        }
```

  and the `None =>` arm of the final `match commands::lookup(Flavor::Pyenv, cmd)` with:

```rust
        None => plugin::dispatch(ctx, cmd, rest, &raw[1..])
            .unwrap_or_else(|| Output::error(format!("pyenv: no such command `{cmd}'"))),
```

  - **`main.rs`:** after collecting `args`:

```rust
    // Run as `pyenv-<cmd>` (a built-in link, Decision 2): act as `pyenv <cmd>` (Decision 3).
    let mut args = args;
    if let Some(cmd) = std::env::args_os()
        .next()
        .and_then(|a| {
            std::path::Path::new(&a)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .and_then(|s| s.strip_prefix("pyenv-").map(str::to_string))
        .filter(|c| pyenv::is_builtin(Flavor::current(), c))
    {
        args.insert(0, cmd.into());
    }
```

  (Make the earlier `let args` binding `let args: Vec<…>` and shadow it as shown.)

  - **`plugin.rs`:**

```rust
//! Running a plugin command (spec §6, decision D4; plan M4a Decisions 2-5).

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{launch, plugins};
use std::ffi::OsString;
use std::path::PathBuf;

/// The `PATH` a plugin runs with, and the one plugins are looked up on.
pub(crate) fn dispatch_path(ctx: &Ctx) -> OsString {
    let prefix = crate::install_prefix();
    let mut front = plugins::front_dirs(&ctx.root, prefix.as_deref());
    if let Some(links) = builtin_links(ctx) {
        front.push(links);
    }
    plugins::dispatch_path(&front, ctx.path.as_deref(), ctx.flavor)
}

/// `pyenv-<name>` on the dispatch `PATH`.
pub(crate) fn find(ctx: &Ctx, name: &str) -> Option<PathBuf> {
    plugins::find(name, &dispatch_path(ctx), ctx.flavor, ctx.pathext.as_deref())
}

/// `$PYENV_ROOT/.rpyenv/libexec`, with a `pyenv-<cmd>` symlink to this binary per built-in
/// (Decision 2): made or repaired here, `None` when that fails (a read-only root).
#[cfg(unix)]
fn builtin_links(ctx: &Ctx) -> Option<PathBuf> {
    if ctx.flavor != Flavor::Pyenv {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = ctx.root.join(".rpyenv").join("libexec");
    std::fs::create_dir_all(&dir).ok()?;
    for name in crate::commands::builtin_names(Flavor::Pyenv) {
        let link = dir.join(format!("pyenv-{name}"));
        if std::fs::read_link(&link).ok().as_deref() != Some(exe.as_path()) {
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&exe, &link).ok()?;
        }
    }
    Some(dir)
}

#[cfg(not(unix))]
fn builtin_links(_ctx: &Ctx) -> Option<PathBuf> {
    None
}

/// The environment upstream's dispatcher exports (Decision 4).
fn env(ctx: &Ctx, path: OsString) -> Vec<(OsString, Option<OsString>)> {
    let prefix = crate::install_prefix();
    let mut v = vec![
        ("PYENV_ROOT".into(), Some(ctx.root.clone().into_os_string())),
        ("PYENV_DIR".into(), Some(ctx.dir.clone().into_os_string())),
        ("PATH".into(), Some(path)),
    ];
    if let Some(p) = &prefix {
        v.push(("_PYENV_INSTALL_PREFIX".into(), Some(p.clone().into_os_string())));
    }
    if ctx.flavor == Flavor::Pyenv {
        let inherited = std::env::var("PYENV_HOOK_PATH").ok().filter(|s| !s.is_empty());
        let hooks = plugins::hook_path(inherited.as_deref(), &ctx.root, prefix.as_deref());
        v.push(("PYENV_HOOK_PATH".into(), Some(hooks.into())));
    }
    v
}

/// `pyenv <cmd> <args>` for a command rpyenv doesn't have. `None` when no plugin has that
/// name. `sh-*` plugins answer `--help` with the help command, as the dispatcher does.
pub(crate) fn dispatch(ctx: &Ctx, cmd: &str, args: &[&str], raw: &[OsString]) -> Option<Output> {
    let path = dispatch_path(ctx);
    let program = plugins::find(cmd, &path, ctx.flavor, ctx.pathext.as_deref())?;
    if args.first() == Some(&"--help") {
        if cmd.starts_with("sh-") {
            let mut o = Output::new();
            o.out(format!("pyenv help \"{cmd}\""));
            return Some(o);
        }
        return Some(crate::help::help_command(ctx.flavor, &[cmd]));
    }
    let plan = launch::LaunchPlan {
        program,
        args: raw.to_vec(),
        raw_tail: None,
        env: env(ctx, path),
        warnings: Vec::new(),
        wait: false,
    };
    // Windows: the caller's command line after `pyenv <cmd>`, unchanged (spec §5.3).
    #[cfg(windows)]
    let plan = launch::LaunchPlan {
        raw_tail: rpyenv_core::wincmd::own_tail(2),
        ..plan
    };
    Some(match launch::run(&plan, ctx, None) {
        Ok(code) => Output::new().with_code(code),
        Err(r) => r.into(),
    })
}
```

  On Linux, `launch::run` with `wait: false` replaces this process with the plugin, as upstream's `exec` does. Check that `launch::run`'s signature is `(&LaunchPlan, &Ctx, Option<&Path>)`; `exec.rs` calls it the same way.

- [ ] **Step 4: Run the tests and see them pass.** Run in WSL: `cargo test -p pyenv --test cli_plugins`. Expected: `6 passed`. Then run `cargo test --workspace` on both OSes.

- [ ] **Step 5: Commit.** `git commit -m "Dispatch unknown commands to pyenv-<cmd> plugins, with built-in links and multicall"`

---

### Task 3: Plugins in help, the command listing, completions and init's routed set

**Files:**
- Modify:
  - `crates/pyenv/src/help.rs`: `Doc`, `parse_doc`, plugin help, listing;
  - `crates/pyenv/src/commands/mod.rs`: `command_names`;
  - `crates/pyenv/src/commands/misc.rs`: `commands`;
  - `crates/pyenv/src/commands/completions.rs`: forwarding;
  - `crates/pyenv/src/commands/init.rs` and `init_win.rs`: the routed set.
- Test: `crates/pyenv/tests/cli_plugins.rs` (append)

**Interfaces:**
- Consumes: Task 2's `plugin::{dispatch_path, find}`.
- Produces:
  - `commands::command_names(&Ctx, Listing) -> Vec<String>`: built-ins plus plugins, each once, in the flavor's order;
  - `help::{Doc, parse_doc(&str) -> Option<Doc>}`.

`pyenv-help`'s parser, which `parse_doc` reproduces (libexec/pyenv-help:29-90):

```text
sed: stop at the first line not starting with `#`; `#` alone becomes an empty line;
     `# <text>` becomes `<text>`; any other `#` line (a shebang) is dropped.
awk: /^Summary:/                   -> summary = text from column 10; next (usage state kept)
     /^Usage:/                     -> reading_usage = 1; usage = usage "\n" line
     reading_usage && /^( *$|       )/ -> usage = usage "\n" line   (empty, all spaces, or 7 spaces)
     anything else                 -> reading_usage = 0; help = help "\n" line
     END: only when usage or summary is non-empty; usage and help lose leading/trailing "\n"s.
print_help: help empty -> help = summary. With usage or summary: usage (or "Usage: pyenv <cmd>"),
     then, when help is non-empty, "", help, "". Otherwise stderr
     "Sorry, this command isn't documented yet.", exit 1.
--usage: the usage when non-empty, else nothing; exit 0.
listing: every name `pyenv-commands` gives whose Summary is non-empty.
```

- [ ] **Step 1: Write the failing tests.** Append to `cli_plugins.rs`. The plugin bodies are upstream's `test/help.bats` cases.

```rust
fn hello(f: &Fixture, block: &str) {
    plugin(f, "hello", "hello", &format!("{block}echo hello\n"));
}

#[test]
fn help_for_a_plugin_reads_its_comment_block() {
    let f = Fixture::new();
    hello(&f, "# Usage: pyenv hello <world>\n# Summary: Says \"hello\" to you, from pyenv\n# This command is useful for saying hello.\n");
    assert_eq!(
        run(&f, &["help", "hello"]).0,
        "Usage: pyenv hello <world>\n\nThis command is useful for saying hello.\n\n"
    );
    assert_eq!(run(&f, &["hello", "--help"]).0, run(&f, &["help", "hello"]).0);
    assert_eq!(run(&f, &["help", "--usage", "hello"]).0, "Usage: pyenv hello <world>\n");
    assert!(run(&f, &["help"]).0.contains("   hello       Says \"hello\" to you, from pyenv\n"));

    let g = Fixture::new();
    hello(&g, "# Usage: pyenv hello <world>\n#        pyenv hi [everybody]\n# Summary: Says \"hello\" to you, from pyenv\n");
    assert_eq!(
        run(&g, &["help", "hello"]).0,
        "Usage: pyenv hello <world>\n       pyenv hi [everybody]\n\nSays \"hello\" to you, from pyenv\n\n"
    );
    let h = Fixture::new();
    hello(&h, "# Usage: pyenv hello <world>\n# Summary: S\n# Line one.\n#\n# Line two.\n");
    assert_eq!(run(&h, &["help", "hello"]).0, "Usage: pyenv hello <world>\n\nLine one.\n\nLine two.\n\n");
    let u = Fixture::new();
    hello(&u, "# nothing documented\n");
    assert_eq!(
        run(&u, &["help", "hello"]),
        (String::new(), "Sorry, this command isn't documented yet.\n".to_string(), 1)
    );
}

#[test]
fn commands_completions_and_init_include_plugins() {
    let f = Fixture::new();
    plugin(&f, "x", "hello", "# provide pyenv completions\nif [ \"$1\" = --complete ]; then shift; for a; do echo \"$a\"; done; fi\n");
    plugin(&f, "x", "sh-activate", "echo :\n");
    let commands = run(&f, &["commands"]).0;
    assert!(commands.lines().any(|l| l == "hello") && commands.lines().any(|l| l == "activate"));
    assert_eq!(run(&f, &["commands", "--sh"]).0, "activate\nrehash\nshell\n");
    assert_eq!(run(&f, &["completions", "hello", "happy", "world"]).0, "--help\nhappy\nworld\n");
    // Decision 7: plugin sh-* commands join the routed set.
    assert!(run(&f, &["init", "-", "bash"]).0.contains("\n  activate|rehash|shell)\n"));
}

/// Review focus 4.
#[test]
fn a_non_executable_plugin_is_listed_but_not_run() {
    let f = Fixture::new();
    let p = plugin(&f, "x", "plain", "echo no\n");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(run(&f, &["commands"]).0.lines().any(|l| l == "plain"));
    assert_eq!(run(&f, &["plain"]).1, "pyenv: no such command `plain'\n");
}
```

- [ ] **Step 2: Run the tests and see them fail.** Run in WSL: `cargo test -p pyenv --test cli_plugins`. Expected: the 3 new tests fail, and the 6 from Task 2 pass.

- [ ] **Step 3: Implement.**
  - **`commands/mod.rs`:**

```rust
/// The names `pyenv commands` prints: the built-ins (`names`) plus `pyenv-*` plugins on
/// the dispatch `PATH`, with the same `sh-` handling, each once, in the flavor's order
/// (libexec/pyenv-commands:23-47).
pub fn command_names(ctx: &Ctx, listing: Listing) -> Vec<String> {
    let path = crate::plugin::dispatch_path(ctx);
    let plugins = rpyenv_core::plugins::listed_names(&path, ctx.flavor, ctx.pathext.as_deref());
    let mut all: Vec<String> = names(ctx.flavor, listing).into_iter().map(String::from).collect();
    for n in plugins {
        let short = n.strip_prefix("sh-");
        let shown = match listing {
            Listing::All => Some(short.unwrap_or(&n).to_string()),
            Listing::ShOnly => short.map(str::to_string),
            Listing::NoSh => short.is_none().then(|| n.clone()),
        };
        if let Some(s) = shown {
            all.push(s);
        }
    }
    match ctx.flavor {
        Flavor::Pyenv => all.sort_unstable(),
        Flavor::PyenvWin => all.sort_by_key(|n| format!("{}.", n.to_ascii_uppercase())),
    }
    all.dedup();
    all
}
```

  - **`misc::commands`:** print `super::command_names(ctx, listing)` instead of `names`.
  - **`init.rs` `shell_function`:** take `ctx` and use `commands::command_names(ctx, Listing::ShOnly)` (a `Vec<String>`; pass `&routed.iter().map(String::as_str).collect::<Vec<_>>()` to the helpers). Do the same for `init_win.rs`'s `function`.
  - **`help.rs`:** add `Doc` and `parse_doc` exactly per the parser block above:

```rust
/// What a command file's leading comment block documents (libexec/pyenv-help:29-90).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Doc {
    pub summary: String,
    pub usage: String,
    pub help: String,
}

/// `None` when the block has neither a Usage nor a Summary.
pub(crate) fn parse_doc(text: &str) -> Option<Doc> {
    let mut lines: Vec<&str> = Vec::new();
    for l in text.lines() {
        if !l.starts_with('#') {
            break;
        }
        if l == "#" {
            lines.push("");
        } else if let Some(rest) = l.strip_prefix("# ") {
            lines.push(rest);
        }
    }
    let (mut summary, mut usage, mut help) = (String::new(), String::new(), String::new());
    let mut reading_usage = false;
    for l in lines {
        if l.starts_with("Summary:") {
            summary = l.get(9..).unwrap_or("").to_string();
        } else if l.starts_with("Usage:") {
            reading_usage = true;
            usage.push('\n');
            usage.push_str(l);
        } else if reading_usage && (l.chars().all(|c| c == ' ') || l.starts_with("       ")) {
            usage.push('\n');
            usage.push_str(l);
        } else {
            reading_usage = false;
            help.push('\n');
            help.push_str(l);
        }
    }
    let trim = |s: &str| s.trim_matches('\n').to_string();
    (!usage.is_empty() || !summary.is_empty()).then(|| Doc {
        summary,
        usage: trim(&usage),
        help: trim(&help),
    })
}

/// A plugin's help (`pyenv-<cmd>` or `pyenv-sh-<cmd>`), `None` when no such file exists.
fn plugin_help(ctx_flavor_ctx: &rpyenv_core::ctx::Ctx, cmd: &str, usage_only: bool) -> Option<Output> {
    let file = crate::plugin::find(ctx_flavor_ctx, cmd)
        .or_else(|| crate::plugin::find(ctx_flavor_ctx, &format!("sh-{cmd}")))?;
    let text = std::fs::read(&file).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let doc = parse_doc(&text);
    let mut o = Output::new();
    if usage_only {
        if let Some(d) = doc.filter(|d| !d.usage.is_empty()) {
            o.out(d.usage);
        }
        return Some(o);
    }
    let Some(d) = doc else {
        return Some(Output::error("Sorry, this command isn't documented yet."));
    };
    let help = if d.help.is_empty() { &d.summary } else { &d.help };
    o.out(if d.usage.is_empty() { format!("Usage: pyenv {cmd}") } else { d.usage.clone() });
    if !help.is_empty() {
        o.out("");
        o.out(help);
        o.out("");
    }
    Some(o)
}
```

  `help_command` and `help_pyenv` need the `Ctx` to search plugins. Change `help_command(flavor, args)` to `help_command(ctx: &Ctx, args)`, using `ctx.flavor`, and update its callers: `lib.rs` (twice), `misc::help`, and `plugin::dispatch`. In `help_pyenv`'s final match, make the `None` arm try `plugin_help(ctx, cmd, usage_only)` before the "no such command" error. Make `help_win`'s unknown-command arm do the same.

  The listing (`listing(ctx)`): after the built-in topics, add each name from `rpyenv_core::plugins::listed_names` whose `parse_doc` Summary is non-empty, unless a built-in topic has that name. Sort by name as now.

  - **`completions.rs`:** when neither the table nor `lookup` knows `cmd` (or `sh-cmd`), find the plugin file. If none, keep "no output, exit 1". Otherwise print `--help`. Then, if any line, lowercased, starts with `# `, `% `, `-- ` or `// ` followed by `provide pyenv completions`, run the plugin with `--complete` and the remaining arguments, capturing its output (Decision 6):

```rust
        let o = std::process::Command::new(&file)
            .arg("--complete")
            .args(&args[1..])
            .envs(/* plugin::env(ctx, path) as (key, value) pairs */)
            .output();
```

  Expose `plugin::env` as `pub(crate) fn env(ctx, path) -> Vec<(OsString, Option<OsString>)>`, and set only its `Some` values. Append the child's stdout and stderr, and use its exit code.

- [ ] **Step 4: Run the tests and see them pass.** Run in WSL: `cargo test -p pyenv --test cli_plugins`, expected `9 passed`. Then run `cargo test --workspace` on both OSes; existing help, completions and listing tests must stay green.

- [ ] **Step 5: Commit.** `git commit -m "Show plugins in help, commands and completions, and route plugin sh- commands"`

---

### Task 4: Windows plugin dispatch

**Files:**
- Modify: `crates/pyenv/src/lib.rs` (`run_pyenv_win`'s `None` arm)
- Create: `crates/pyenv/tests/cli_plugins_win.rs`

**Interfaces:** Consumes `plugin::dispatch` (Task 2) and `command_names` and the plugin help (Task 3).

- [ ] **Step 1: Write the failing tests.**

```rust
//! Windows plugin dispatch (plan M4a Decision 5; allowlist D-93).
#![cfg(windows)]

mod common;
use common::Fixture;

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv_env(args, &[("PATHEXT", ".COM;.EXE;.BAT;.CMD")]);
    (r.stdout, r.stderr, r.code)
}

/// allowlist D-93
#[test]
fn a_batch_plugin_runs_with_the_dispatcher_s_environment() {
    let f = Fixture::new();
    f.file(
        &f.root.join("plugins/x/bin/pyenv-hello.bat"),
        "@echo off\r\necho [%1][%2][%PYENV_ROOT%]\r\nexit /b 3\r\n",
    );
    let (out, err, code) = run(&f, &["hello", "a", "b"]);
    assert_eq!((out, code), (format!("[a][b][{}]\r\n", f.root.display()), 3), "{err}");
    assert!(run(&f, &["commands"]).0.lines().any(|l| l == "hello"));
    assert_eq!(
        run(&f, &["help", "hello"]),
        (String::new(), "Sorry, this command isn't documented yet.\r\n".to_string(), 1)
    );
    assert_eq!(run(&f, &["nosuch"]).0, "pyenv: no such command 'nosuch'\r\n");
}
```

`{}` of the fixture root contains `ñ`. If the batch file's output doesn't decode, compare a root-independent marker instead: echo `[%PYENV_ROOT:~-4%]`, giving `root`. Don't loosen the arguments check.

- [ ] **Step 2: Run it and see it fail.** Run: `cargo test -p pyenv --test cli_plugins_win`. Expected: `no such command 'hello'`.

- [ ] **Step 3: Implement.** In `run_pyenv_win`, change the `None` arm to try `plugin::dispatch(ctx, &cmd, rest, &raw[1..])` first, keeping pyenv-win's message as the fallback. `plugin::dispatch` already sets `raw_tail` from `own_tail(2)`, so batch plugins get the caller's arguments unchanged.

- [ ] **Step 4: Run it and see it pass.** Then run `cargo test --workspace` on Windows and in WSL.

- [ ] **Step 5: Commit.** `git commit -m "Dispatch to pyenv-<cmd> plugins on Windows"`

---

### Task 5: Parity: bats, diff cases, allowlist, spec

**Files:**
- Modify:
  - `parity/bats_run.sh`, `parity/expected/bats.txt`;
  - `parity/diff.py` and `parity/diff_cases.py` (an `executable` field), plus goldens;
  - `docs/parity/allowlist.md` (D-88 reworded, D-93 added);
  - `docs/specs/2026-09-27-rpyenv-design.md` §6.

- [ ] **Step 1: Give the harness's run tree the suite's plugin folder.** In `bats_run.sh`, after `install -d … "$work/run/libexec"`, add:

```bash
# The suite's tree has plugins/python-build/bin; "adds its own libexec and plugin bin dirs to
# PATH" expects it on PATH. Empty here: rpyenv's install is built in.
install -d -m 755 "$work/run/plugins/python-build/bin"
```

- [ ] **Step 2: Run bats in WSL and update `bats.txt`.**
  - Build as `jm`, run `bats_run.sh` as root, then `bats_check.py` as `jm`, as in M3's Task 8.
  - Remove every row reported "listed, but it passed". Expected: the 13 M4 rows, plus `commands.bats | commands` and `commands.bats | commands in path with spaces`.
  - Reword `init.bats | outputs sh-compatible case syntax` to cite the new D-88.
  - Any other change in the failing set is a bug against M1L: fix it here.
  - Expected end state: `0 problems`, with no `| M4` reason left.

- [ ] **Step 3: Add the `executable` field to diff cases.** In `diff.py` `build()`, after writing `case.files`:

```python
    for rel in case.executable:
        os.chmod(resolve(places, rel), 0o755)
```

  In `diff_cases.py`, add `executable: tuple = ()  # paths made executable after the fixture is built` to `Case`.

  Then add the Linux cases below. Remove `allow=("D-88",)` from "init - ksh with a plugin sh- command", since plugin `sh-*` commands now route (Decision 7):

```python
    # M4a: plugin dispatch (Linux).
    Case("a plugin command", ("hello", "a b"), os="Linux",
         files=(("root/plugins/hello/bin/pyenv-hello",
                 "#!/bin/sh\n# Usage: pyenv hello <who>\n# Summary: Says hello\n# Greets.\nprintf '%s|' \"$@\"; echo \"$PYENV_ROOT|$PYENV_DIR\"\n"),),
         executable=("root/plugins/hello/bin/pyenv-hello",)),
    Case("help for a plugin command", ("help", "hello"), os="Linux",
         files=(("root/plugins/hello/bin/pyenv-hello", "#!/bin/sh\n# Usage: pyenv hello <who>\n# Summary: Says hello\n# Greets.\n"),),
         executable=("root/plugins/hello/bin/pyenv-hello",)),
    Case("help --usage for a plugin command", ("help", "--usage", "hello"), os="Linux",
         files=(("root/plugins/hello/bin/pyenv-hello", "#!/bin/sh\n# Usage: pyenv hello <who>\n# Summary: Says hello\n"),),
         executable=("root/plugins/hello/bin/pyenv-hello",)),
    Case("completions for a plugin with the marker", ("completions", "hello", "x"), os="Linux",
         files=(("root/plugins/hello/bin/pyenv-hello", "#!/bin/sh\n# Provide pyenv completions\n[ \"$1\" = --complete ] && shift && echo \"$@\"\n"),),
         executable=("root/plugins/hello/bin/pyenv-hello",)),
    Case("a plugin command on Windows", ("hello",), os="Windows",
         files=(("root/plugins/hello/bin/pyenv-hello.bat", "@echo hello\r\n"),), allow=("D-93",)),
```

Regenerate the Linux goldens in WSL and the Windows goldens on Windows (`--update-golden`), then review `git diff parity/golden`. Expected changes:
- new files only for cases with an `allow`;
- `commands`, `help` and `no arguments` unchanged, since the fixtures have no plugins.

On Windows the fixture root is named `pyenv-win`, so write that case's path relative to `root`, as above.

- [ ] **Step 4: Update the allowlist and the spec.**
  - Replace D-88's row with the new text from Decisions, and add D-93 after D-92.
  - In spec §6, replace the "Plugin dispatch" paragraph with:

```markdown
Plugin dispatch: for a command rpyenv doesn't have, run `pyenv-<cmd>` from
`<prefix>/libexec`, `$PYENV_ROOT/plugins/*/bin`, `<prefix>/plugins/*/bin` or
`PATH`, in that order, with the environment upstream's dispatcher exports
(`PYENV_ROOT`, `PYENV_DIR`, `_PYENV_INSTALL_PREFIX`, that `PATH`, and on Linux
`PYENV_HOOK_PATH`). A plugin can't shadow a built-in. On Linux, existing bash
plugins work unchanged: rpyenv keeps a `pyenv-<cmd>` symlink to itself for
each built-in in `$PYENV_ROOT/.rpyenv/libexec` (on the plugin's `PATH`), and
run under such a name it acts as `pyenv <cmd>`. On Windows, a plugin must be
an executable that `PATHEXT` finds (`.exe`, `.bat`, `.cmd`). Plugins appear
in `pyenv commands`, `pyenv help` (from their leading `#` comment block) and
`pyenv completions`, and plugin `sh-*` commands join the shell function's
routed set (§7).
```

- [ ] **Step 5: Run every check on both OSes.**
  - fmt, clippy, `cargo test --workspace`;
  - the parity unit tests;
  - `coverage.py`: 93 rows, all covered (D-93 by its Windows case and the `cli_plugins_win` citation);
  - the Linux and Windows diffs, with no `differs`;
  - bats, with 0 problems;
  - the pyenv-win overlay, unchanged.

- [ ] **Step 6: Commit.** `git commit -m "Run the plugin bats tests against rpyenv, add plugin diff cases, narrow D-88 and add D-93"`

---

## Self-review

**Spec coverage:**

| Spec requirement | Task |
|---|---|
| §2 D4: `pyenv foo` runs `pyenv-foo`, git style | 2 (Linux), 4 (Windows) |
| §2 D4: no bash hooks | `PYENV_HOOK_PATH` is exported but nothing is run (Decision 4); D-51 stands |
| §6: `pyenv-foo` from `$PYENV_ROOT/plugins/*/bin` or `PATH`, with `PYENV_ROOT` exported | 1, 2 |
| §6: on Linux, existing bash plugins work unchanged | 2 (built-in links and multicall; test `a_bash_plugin_reaches_built_ins_by_name`) |
| §6: on Windows, a plugin must be an executable | 4 |
| §14 item 4 (plugin half) | 1–5; virtualenvs are M4b |

**Placeholders:**
- Task 3 Step 3 describes the `help_command` signature change and the completion `envs` wiring in prose. Both name every caller to update and the exact pieces (`plugin::env`), and the tests pin the outcome.
- Task 4 gives an explicit fallback if the fixture's `ñ` root doesn't survive `echo`.

**Type consistency:**
- `plugin::find(ctx, name)` (Task 2) is used by help (Task 3).
- `command_names` returns `Vec<String>`; Task 3 converts it for `init`'s `&[&str]` helpers.
- `help_command` changes from `(Flavor, …)` to `(&Ctx, …)` in Task 3, and every caller is listed there.

**Review Focus:** each of the five has its test, named at the top.
