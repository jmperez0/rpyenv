# M1c-a: Fixes Before Parity CI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the behaviour the parity suites would otherwise record as differences, and close the review items M1b-win carried forward:
- Linux shims become hardlinks to a per-user template. Writing into a shim can then no longer overwrite the installed `pyenv-shim`, and `pyenv rehash` repairs a damaged shim.
- On Windows, text rpyenv writes to a pipe or a file is in the console's code page, as pyenv-win's is.
- Start-failure messages show the program's name instead of `%1`, and the debug log is consistent.
- CI pins its actions and checks the shims' dependency tree.

**Architecture:**
- **Rehash:** one code path (`apply_hardlinks`) now serves both OSes. It uses a per-OS template name and a per-OS shim-identity test: the inode on Unix, size plus modification time elsewhere.
- **Output:** a new `rpyenv-core` module, `textout`, is the single place where rpyenv's own text reaches stdout and stderr. On Windows it encodes text for a non-console handle through `wincp`.
- **Debug log:** it gets a process-wide source name and writes each line once.

**Tech Stack:** Rust 1.96 (edition 2021). `rpyenv-core` uses `std`, `libc` on Unix, and `windows-sys` 0.61 on Windows. `tempfile` for tests. Python 3 (standard library only) for one CI script.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`:
- §3 / §3.1 (architecture, shim dependency rule, layout);
- §4 (parity policy: read it first);
- §8 (rehash);
- §11 (errors);
- §12 (testing);
- §13 (variables).

Allowlist: `docs/parity/allowlist.md`.

**Where this fits:** M1a, M1b and M1b-win are on `main` (6790172). M1c is split in two:
- **M1c-a**, this plan, changes behaviour.
- **M1c-b** adds the parity CI: the adapted upstream suites, differential tests, the allowlist coverage check and the benchmark. It is written after M1c-a lands, because its expected-failure lists depend on M1c-a's behaviour. The planning probes for M1c-b are recorded under "After this plan".

## Decisions this plan makes

1. **Linux shims are hardlinks to `shims/.template/pyenv-shim`**, a per-user copy of the installed `pyenv-shim`, exactly as Windows shims are hardlinks to `shims\.template\pyenv-shim.exe`. Decided by the user on 2026-10-02 ("templates on Linux: yes").
   - **Evidence:**
     - Upstream's bats test "repairs an overwritten shim" writes `2\n` into `shims/python`. With symlink shims that wrote through the link and replaced the installed `pyenv-shim` with 2 bytes (seen in the planning probe).
     - On Windows, where shims were already template hardlinks, the same write left the installed binary intact, and one `pyenv rehash` repaired every shim (`C:\tmp\shim_clobber_probe.py`).
     - Copies were rejected: 306 shim names × 471 KB release shim ≈ 144 MB on the user's machine.
2. **A shim is "current" when it is the template:**
   - On Unix this means the same device and inode, or a copy with the template's size and modification time (the copy fallback sets that time).
   - Elsewhere it means size and modification time, as before.
   - A symlink is never current, so rpyenv 0.1's symlink shims are replaced on the first rehash.
3. **Windows output encoding follows pyenv-win and cmd.exe** (spec §4: "match … output formats"):
   - Text written to a pipe, a file or NUL is encoded in `GetConsoleOutputCP()`. With no console at all, `CP_ACP` is used.
   - When a character doesn't fit the code page, the whole text is written as UTF-8 instead. pyenv-win most likely writes `?` there, which loses the character. A reader decoding with the code page gets a broken path either way, but a reader decoding UTF-8 then gets the right one, and nothing is lost. This difference is allowlist row D-48. The user decided it on 2026-10-02 ("apply"); the trade-off was:

     | Reader | All code page | All UTF-8 | Chosen |
     |---|---|---|---|
     | cmd `for /f` | correct | garbles `é` | correct |
     | PowerShell | correct | garbled | correct |
     | `encoding="utf-8"` readers | wrong | correct | correct except for representable non-ASCII |

   - A user who wants UTF-8 everywhere sets the console to code page 65001 (`chcp 65001`, or Windows' "Use Unicode UTF-8" option), and rpyenv follows it.
   - Text written to a console is passed to std unchanged, and std writes it as Unicode.
   - **Evidence:** the planning probe `C:\tmp\pw_encoding_probe.py`. pyenv-win's `which` and `version` wrote `Jos\x82 \xa4` for `José ñ` under code page 850; rpyenv wrote UTF-8.
   - Linux output stays UTF-8 bytes.
4. **Tests that run `chcp` give their `cmd.exe` a console of its own** (`CREATE_NO_WINDOW`). A console's code page is shared by every process on it. A `chcp` on the test runner's console would change how parallel tests' output is encoded between encode and decode.
5. **The debug log names its source.** It is `rpyenv-shim` by default, and `pyenv exec` when that command runs, set once per process. Each line is written with one `write_all`. Lines end in `\r\n` on Windows.
6. **Out of scope, carried forward:**
   - the stdin→NUL branch test (it needs a launch with no stdin handle at all, which only raw `CreateProcessW` with pipes can make);
   - the `window_station_visible` false branch (it needs a non-interactive window station);
   - a test for `job=none` (Windows 8+ allows nested jobs, so no test can make the assignment fail);
   - `host_arch_suffix` (M6);
   - blocking in the Ctrl handler for close/logoff/shutdown (optional; the final M1b-win review saw cleanup finish).
7. **Already resolved, nothing to do:** `.gitattributes` sets `* text=auto eol=lf`, and `git ls-files --eol` lists no tracked file without `i/lf` (checked 2026-10-02).

## Global Constraints

- **Toolchain:** Rust `1.96`, edition 2021, workspace `resolver = "2"`. CI runs these in order:
  1. `cargo fmt --all --check`
  2. `cargo clippy --workspace --all-targets -- -D warnings`
  3. `cargo build --workspace`
  4. `cargo test --workspace`
- **CI matrix:** `windows-2025`, `windows-2022`, `ubuntu-latest`, `ubuntu-24.04-arm` (spec §12).
- **Dependencies** (spec §3):
  - `rpyenv-core`: `std`; `libc = "0.2"` on Unix; `windows-sys = "0.61"` on Windows, with only the features each task lists. Nothing else.
  - `pyenv-shim` and `pyenv-shimw`: `rpyenv-core` only.
  - The test-only `rpyenv-e2e` may use `windows-sys` on Windows.
- **Parity policy** (spec §4):
  - Match each OS's upstream in messages, streams, exit codes and output formats.
  - Don't reproduce defects.
  - Every intentional difference gets a row in `docs/parity/allowlist.md`. This plan changes the text of D-30 and D-35 and adds D-48.
- **Line endings:** rpyenv prints `\n` on Linux and `\r\n` on Windows.
- **`unsafe`:** every `unsafe` block gets a `// SAFETY:` comment saying why it holds.
- **Commits:** one-line `git commit -m "..."` messages, with no attribution or trailer lines. Run `cargo fmt --all` before every commit.
- **Test binaries:**
  - The e2e tests need `pyenv`, `pyenv-shim`, `pyenv-shimw`, `argv-echo` and `argv-echow` built side by side.
  - `cargo test -p rpyenv-e2e` alone does **not** rebuild `pyenv-shim` / `pyenv-shimw`. Run `cargo build --workspace` before any e2e run, and before any mutation check, or the mutation silently passes.
- **Two OSes:** Windows-only code and tests don't compile on Linux, and Unix-only ones don't compile on Windows. The controller runs both. Linux runs in WSL Debian at `~/rpyenv-linux`.
- **WSL safety:** never pass `$VARS` inside `wsl … -- bash -c "…"`, because the distro's default shell expands them first. Write a script file and run it with `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/<script>.sh`, from PowerShell.

## Review Focus

These are the five inputs most likely to break this for a real user that the spec doesn't spell out. Each has a test in the task named in brackets.

1. **A Linux user upgrading from rpyenv 0.1, whose shims are symlinks to the installed `pyenv-shim`.** The first rehash must replace every symlink with a hardlink and never write through one. [Task 2: `linux_rehash_hardlinks_replaces_and_removes`]
2. **A program that writes into a shim** (an installer, a script, `> ~/.pyenv/shims/python`). The installed binary must stay intact, and `pyenv rehash` must restore every shim. [Task 2: `linux_rehash_repairs_a_shim_written_through`, `win_flavor_rehash_repairs_a_shim_written_through`, e2e `linux_writing_into_a_shim_leaves_the_installed_binary_alone`]
3. **A Windows user whose root path has letters outside ASCII** (`C:\Users\José`), capturing `pyenv` output with `for /f`, a pipe or `> file`. The bytes must decode correctly in the console's code page. [Task 3: `win_redirected_output_uses_the_console_code_page`]
4. **A character the console code page lacks** (`漢` under 850). rpyenv must write the whole text as UTF-8, not `?`, and must not crash or print nothing. [Task 3: `encode_for_output_keeps_what_the_code_page_lacks`]
5. **A Windows program that can't start** (a corrupt `.exe`, the wrong architecture). The message must name the program, and must not show a literal `%1`, on stderr and in the debug log. [Task 4: `start_failure_names_the_program`; Task 5: `win_a_start_failure_reaches_the_debug_log`]

---

### Task 1: CI — pin the actions and guard the shims' dependencies

Spec §3: "The shim binaries must not link networking, TLS, or archive code. … CI checks this by reading each shim's dependency tree." The M1a review also left `dtolnay/rust-toolchain@master` unpinned, and `actions/checkout@v4` runs on the deprecated Node 20.

**Files:**
- Create: `ci/shim_deps.py`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `python3 ci/shim_deps.py` (exit 0 when the shims depend only on the allowed crates) and `python3 ci/shim_deps.py --self-test`.

- [ ] **Step 1: Write the guard with its self-test**

Create `ci/shim_deps.py`:

```python
"""CI guard (spec §3): the shim binaries depend on rpyenv-core and platform crates only.

Reads `cargo tree` for each shim, for every target, and fails on any other crate.
A new platform crate needs a deliberate edit to ALLOWED, which a reviewer sees.
"""
import subprocess
import sys

SHIMS = ("pyenv-shim", "pyenv-shimw")
# rpyenv-core's own dependencies are std and platform crates only (spec §3).
ALLOWED = {"rpyenv-core", "libc", "windows-sys", "windows-link"}


def crates(tree: str) -> set:
    """Crate names in `cargo tree --prefix none` output (`name vX.Y.Z ...` per line)."""
    return {line.split()[0] for line in tree.splitlines() if line.strip()}


def unexpected(shim: str, tree: str) -> list:
    return sorted(crates(tree) - ALLOWED - {shim})


def self_test() -> None:
    ok = "pyenv-shim v0.1.0 (/x)\nrpyenv-core v0.1.0 (/y)\nlibc v0.2.189\nlibc v0.2.189 (*)\n"
    assert unexpected("pyenv-shim", ok) == [], unexpected("pyenv-shim", ok)
    bad = ok + "reqwest v0.12.0\nrustls v0.23.0\n"
    assert unexpected("pyenv-shim", bad) == ["reqwest", "rustls"], unexpected("pyenv-shim", bad)


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        print("self-test ok")
        return 0
    failed = False
    for shim in SHIMS:
        tree = subprocess.run(
            ["cargo", "tree", "-p", shim, "-e", "normal", "--prefix", "none", "--target", "all"],
            check=True,
            capture_output=True,
            encoding="utf-8",
        ).stdout
        extra = unexpected(shim, tree)
        if extra:
            print(f"{shim} depends on crates outside spec §3's allowance: {', '.join(extra)}")
            failed = True
        else:
            print(f"{shim}: ok ({', '.join(sorted(crates(tree)))})")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 2: Run the self-test and the guard**

Run: `python ci/shim_deps.py --self-test`, then `python ci/shim_deps.py`

Expected:
- `self-test ok`
- `pyenv-shim: ok (libc, pyenv-shim, rpyenv-core, windows-link, windows-sys)` and the same line for `pyenv-shimw`.

- [ ] **Step 3: Check the guard can fail**

Mutation:
1. Add `tempfile = "3"` under `[dependencies]` in `crates/pyenv-shim/Cargo.toml`.
2. Run `python ci/shim_deps.py`. Expected: exit 1 and `pyenv-shim depends on crates outside spec §3's allowance:`, listing `tempfile` and its dependencies.
3. Revert the edit, then run `git diff --stat crates/pyenv-shim` and confirm it prints nothing.

- [ ] **Step 4: Pin the actions and add the guard step**

In `.github/workflows/ci.yml`:
- Replace `- uses: actions/checkout@v4` with `- uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5`.
- Replace `- uses: dtolnay/rust-toolchain@master` with `- uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02`.

These SHAs were resolved on 2026-10-02 with `gh api repos/actions/checkout/commits/v5 --jq .sha` and `gh api repos/dtolnay/rust-toolchain/commits/master --jq .sha`. Re-run both commands. If either now prints a different SHA, use the new one and say so in the report.

After the `cargo build --workspace` step, add:

```yaml
      - name: Shim dependency guard (spec §3)
        if: matrix.os == 'ubuntu-latest'
        run: |
          python3 ci/shim_deps.py --self-test
          python3 ci/shim_deps.py
```

`cargo tree --target all` gives the same answer on every OS, so one runner is enough.

- [ ] **Step 5: Commit**

```bash
git add ci/shim_deps.py .github/workflows/ci.yml
git commit -m "Pin CI actions by commit and check the shims' dependency tree"
```

---

### Task 2: Linux shims are hardlinks to a per-user template

**Files:**
- Modify: `crates/rpyenv-core/src/rehash.rs` (constants; `rehash_locked`; delete `apply_links` and both `symlink` functions; `apply_hardlinks`; a new `is_link_to` and `copy_template`; tests)
- Modify: `crates/rpyenv-core/src/shim.rs` (`rehash_with`; the `command_name` doc comment)
- Modify: `crates/rpyenv-core/src/lookup.rs` (`system_command`; a test)
- Modify: `crates/pyenv/tests/cli_rehash.rs` (`rehash_links_each_executable_and_shims_lists_them`)
- Modify: `crates/e2e/tests/shims.rs` (a new Linux test)
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` (§3.1 table, §8), `docs/parity/allowlist.md` (D-30, D-35)

**Interfaces:**
- Produces:
  - `rehash::TEMPLATE_BIN: &str = "pyenv-shim"`
  - `rehash::template_name(flavor: Flavor) -> &'static str` (`"pyenv-shim"` for `Pyenv`, `"pyenv-shim.exe"` for `PyenvWin`)
  - `TEMPLATE_EXE` keeps its name and value.

- [ ] **Step 1: Write the failing unit tests (rehash.rs)**

In `crates/rpyenv-core/src/rehash.rs`'s test module, **replace** `linux_rehash_links_replaces_and_removes` with:

```rust
    /// Review focus 1: an rpyenv 0.1 symlink shim and an upstream bash shim are both
    /// replaced by hardlinks to the template, and the template is a copy of the installed
    /// shim binary.
    #[cfg(unix)]
    #[test]
    fn linux_rehash_hardlinks_replaces_and_removes() {
        use std::os::unix::fs::MetadataExt;
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/bin/python"));
        exe(&v.join("3.12.1/bin/pip"));
        let shims = ctx.shims_dir();
        fs::create_dir_all(shims.join("mdir")).unwrap();
        fs::write(shims.join("python"), "#!/usr/bin/env bash\n").unwrap();
        std::os::unix::fs::symlink(&shim, shims.join("pip")).unwrap();
        fs::write(shims.join("gone"), "").unwrap();
        fs::write(shims.join(".keep"), "").unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        let template = shims.join(TEMPLATE_DIR).join(TEMPLATE_BIN);
        assert_eq!(fs::read(&template).unwrap(), fs::read(&shim).unwrap());
        let t = fs::metadata(&template).unwrap();
        for n in ["python", "pip"] {
            let m = fs::symlink_metadata(shims.join(n)).unwrap();
            assert!(m.is_file(), "{n} should be a hardlink, not a symlink");
            assert_eq!((m.dev(), m.ino()), (t.dev(), t.ino()), "{n}");
        }
        assert_eq!(fs::read(&shim).unwrap(), b"shim binary");
        assert!(!shims.join("gone").exists());
        assert!(shims.join(".keep").exists());
        assert!(shims.join("mdir").is_dir());
        assert!(!shims.join(LOCK_NAME).exists());
        assert!(!needed(&ctx));
    }

    /// Review focus 2: writing into one shim writes into the template every shim shares,
    /// never into the installed binary; `pyenv rehash` then restores all of them.
    #[cfg(unix)]
    #[test]
    fn linux_rehash_repairs_a_shim_written_through() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/bin/aaa"));
        exe(&v.join("3.12.1/bin/python"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        let shims = ctx.shims_dir();
        fs::write(shims.join("python"), b"2\n").unwrap();
        assert_eq!(fs::read(&shim).unwrap(), b"shim binary");
        let stats = rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(stats.linked, 2);
        for n in ["aaa", "python"] {
            assert_eq!(fs::read(shims.join(n)).unwrap(), b"shim binary", "{n}");
        }
    }

    /// The same contract for the Windows flavor, which runs on both hosts.
    #[test]
    fn win_flavor_rehash_repairs_a_shim_written_through() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/aaa.exe"));
        exe(&v.join("3.12.1/python.exe"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        let shims = ctx.shims_dir();
        fs::write(shims.join("python.exe"), b"2\n").unwrap();
        assert_eq!(fs::read(&shim).unwrap(), b"shim binary");
        let stats = rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(stats.linked, 2);
        for n in ["aaa.exe", "python.exe"] {
            assert_eq!(fs::read(shims.join(n)).unwrap(), b"shim binary", "{n}");
        }
    }

    /// When a hardlink isn't possible the shim is a copy; the copy takes the template's
    /// modification time, so a second rehash keeps it instead of copying again.
    #[test]
    fn a_copied_shim_is_current_on_the_next_rehash() {
        let (_t, ctx, shim) = setup(Flavor::current());
        let shims = ctx.shims_dir();
        let template = shims.join(TEMPLATE_DIR).join(template_name(ctx.flavor));
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(&template, b"shim binary").unwrap();
        let t = fs::metadata(&template).unwrap();
        let p = shims.join("copied");
        copy_template(&template, &p, &t).unwrap();
        assert!(is_link_to(&fs::symlink_metadata(&p).unwrap(), &t));
    }
```

In `the_check_notices_a_new_script`, replace `assert_eq!(fs::read_link(ctx.shims_dir().join("black")).unwrap(), shim);` with:

```rust
        let template = ctx.shims_dir().join(TEMPLATE_DIR).join(TEMPLATE_BIN);
        assert!(is_link_to(
            &fs::symlink_metadata(ctx.shims_dir().join("black")).unwrap(),
            &fs::metadata(&template).unwrap()
        ));
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p rpyenv-core rehash::` (Windows host).
Expected: compile errors: `TEMPLATE_BIN`, `template_name`, `copy_template` and `is_link_to` not found. On Linux, after Step 3 compiles, the old symlink code would fail `linux_rehash_hardlinks_replaces_and_removes` at "should be a hardlink, not a symlink".

- [ ] **Step 3: Implement (rehash.rs)**

1. Replace the two template constants and their doc comment (`TEMPLATE_DIR`, `TEMPLATE_EXE`) with:

```rust
/// Where the per-user copy of the shim binary lives; every shim is a hardlink to it
/// (spec §8). `PATH` never searches subfolders, so it is never a command itself.
pub const TEMPLATE_DIR: &str = ".template";
/// The console shim template's name on Windows.
pub const TEMPLATE_EXE: &str = "pyenv-shim.exe";
/// The shim template's name on Linux.
pub const TEMPLATE_BIN: &str = "pyenv-shim";

/// The console shim template's file name for `flavor`.
pub fn template_name(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Pyenv => TEMPLATE_BIN,
        Flavor::PyenvWin => TEMPLATE_EXE,
    }
}
```

2. In `rehash_locked`, replace the `match ctx.flavor { … apply_links … apply_hardlinks … }` with:

```rust
    let stats =
        apply_hardlinks(&shims, shim_exe, &wanted, ctx.flavor).map_err(RehashError::Io)?;
```

3. Delete `apply_links` and both `fn symlink` (the `#[cfg(unix)]` one and the `#[cfg(not(unix))]` one).

4. In `apply_hardlinks`:
   - Change its signature to `fn apply_hardlinks(shims: &Path, source: &Path, wanted: &[Wanted], flavor: Flavor) -> io::Result<RehashStats>`.
   - Change its first line to `let console = refresh_template(shims, source, template_name(flavor))?;`.
   - Replace the doc comment's first sentence with: `Each shim is a hardlink to the template in \`shims/.template/\` (Windows: \`pyenv-shim.exe\` or \`pyenv-shimw.exe\`, matching its kind; Linux: \`pyenv-shim\`), or a copy when a hardlink isn't possible.` Keep the rest of the comment.
   - In the shim loop, replace the block from `let p = shims.join(&w.name);` through the `match fs::hard_link(...)` statement with:

```rust
        let p = shims.join(&w.name);
        match fs::symlink_metadata(&p) {
            Ok(m) if is_link_to(&m, tmeta) => continue,
            Ok(m) if m.is_dir() => continue,
            Ok(_) => remove_or_rename(&p),
            Err(_) => {}
        }
        match fs::hard_link(template, &p).or_else(|_| copy_template(template, &p, tmeta)) {
```

   - Replace `let removed = remove_stale(shims, &effective, true)?;` with `let removed = remove_stale(shims, &effective, flavor == Flavor::PyenvWin)?;`.

5. Add, next to `same_file_data`:

```rust
/// Whether a shim, with metadata `m` read without following symlinks, already is the
/// template whose metadata is `t`. On Unix that's the same inode. Everywhere, a file with
/// the template's size and modification time also counts: a hardlink shares both on
/// Windows, and a copy made by `copy_template` gets both. A symlink is never current, so
/// rpyenv 0.1's symlink shims are replaced (Review focus 1).
fn is_link_to(m: &fs::Metadata, t: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.is_file() && m.dev() == t.dev() && m.ino() == t.ino() {
            return true;
        }
    }
    same_file_data(m, t)
}

/// The fallback when a hardlink isn't possible: a copy of the template that gets the
/// template's modification time, so the next rehash sees it as current.
fn copy_template(template: &Path, p: &Path, t: &fs::Metadata) -> io::Result<()> {
    fs::copy(template, p)?;
    set_mtime(p, t.modified()?)
}
```

- [ ] **Step 4: Implement (shim.rs)**

Replace:

```rust
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
```

with:

```rust
    // Shims are hardlinks to the template, so the exit check passes the template itself
    // and nothing is copied (spec §8).
    let rehash_with = Some(
        ctx.shims_dir()
            .join(rehash::TEMPLATE_DIR)
            .join(rehash::template_name(flavor)),
    );
```

In `command_name`'s doc comment, replace `` `argv[0]`'s last component on Linux, where every shim is a symlink to one binary; `` with `` `argv[0]`'s last component on Linux, where every shim is a hardlink named after its command (a symlink to a shim elsewhere keeps the name it was run by); ``.

- [ ] **Step 5: Harden the `system` search (lookup.rs)**

Before this change, a symlink to a shim canonicalized to the shim binary. Now it canonicalizes to `<root>/shims/<name>`. In `system_command`:
1. Replace the line `.find(|p| own.is_none() || std::fs::canonicalize(p).ok() != own)` with:

```rust
    .find(|p| match std::fs::canonicalize(p) {
        Ok(c) => Some(&c) != own.as_ref() && c.parent() != shims_canon.as_deref(),
        Err(_) => true,
    })
```

2. Add `let shims_canon = std::fs::canonicalize(&shims).ok();` after the `let own = …;` statement.
3. In its doc comment, replace `A hit that resolves to rpyenv's shim binary is passed over (allowlist D-30).` with `A hit that resolves to rpyenv's shim binary, or into the \`shims\` folder, is passed over (allowlist D-30).`

Add a test next to `system_search_skips_shims_listed_dirs_home_and_the_shim_binary`:

```rust
    /// A symlink elsewhere on PATH to a shim is passed over even when the caller doesn't
    /// know the shim binary (`pyenv exec`, whose shim binary isn't the shim's template).
    #[cfg(unix)]
    #[test]
    fn system_search_skips_a_link_to_any_shim() {
        use std::os::unix::fs::symlink;
        let r = Root::new(&[]);
        let base = r.tmp.path();
        let shims = r.root().join("shims");
        exe(&shims.join("tool"));
        fs::create_dir_all(base.join("links")).unwrap();
        symlink(shims.join("tool"), base.join("links/tool")).unwrap();
        let real = base.join("sys/tool");
        exe(&real);
        let mut ctx = r.ctx(Some("system"));
        let path = format!("{}:{}", base.join("links").display(), base.join("sys").display());
        ctx.path = Some(path.into());
        let found = which_pyenv(&ctx, "tool", false, &Skip::default()).map(|f| f.path);
        assert_eq!(found, Ok(real));
    }
```

`Skip` already derives `Default` (`lookup.rs:37`).

- [ ] **Step 6: Update the CLI and e2e tests**

In `crates/pyenv/tests/cli_rehash.rs`, `rehash_links_each_executable_and_shims_lists_them`, replace:

```rust
    let target = fs::read_link(shims.join("python")).unwrap();
    assert_eq!(
        fs::canonicalize(target).unwrap(),
        fs::canonicalize(shim_exe()).unwrap()
    );
```

with:

```rust
    use std::os::unix::fs::MetadataExt;
    let template = shims.join(".template").join("pyenv-shim");
    assert_eq!(fs::read(&template).unwrap(), fs::read(shim_exe()).unwrap());
    let (s, t) = (
        fs::symlink_metadata(shims.join("python")).unwrap(),
        fs::metadata(&template).unwrap(),
    );
    assert!(s.is_file(), "python should be a hardlink, not a symlink");
    assert_eq!((s.dev(), s.ino()), (t.dev(), t.ino()));
```

In `crates/e2e/tests/shims.rs`, add:

```rust
/// Review focus 2, with the real binaries: a write into one shim breaks only the per-user
/// template, never the installed `pyenv-shim`, and `pyenv rehash` makes the shims run again.
#[cfg(unix)]
#[test]
fn linux_writing_into_a_shim_leaves_the_installed_binary_alone() {
    let f = Fixture::new();
    f.install("3.12.10/bin/python");
    f.rehash();
    let installed = std::fs::read(built("pyenv-shim")).unwrap();
    std::fs::write(f.shim("python"), b"2\n").unwrap();
    assert_eq!(std::fs::read(built("pyenv-shim")).unwrap(), installed);
    let r = f.pyenv(&["rehash"], &[]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let out = f.run_shim("python", &["x".into()], &[("PYENV_VERSION", v("3.12.10"))]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(arg_lines(&out), [line("arg", "x")]);
}
```

The helpers are the existing ones: `f.pyenv(args, env)`, `f.run_shim(name, &[OsString], env)`, `arg_lines` and `line` from `crates/e2e/tests/common/mod.rs`, and `v` from `shims.rs`. **This test must run against the built binaries.** Before this task's code, it overwrites `target/debug/pyenv-shim` itself. If you run it as a RED check, run `cargo build --workspace` again afterwards, after touching `crates/pyenv-shim/src/main.rs`: cargo does not notice a damaged output file.

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo build --workspace`, then `cargo test --workspace`. Expected: PASS on Windows. The Unix tests (`linux_*`, `cli_rehash`, `shims.rs`) run in the controller's WSL check.

- [ ] **Step 8: Update the spec and the allowlist**

In `docs/specs/2026-09-27-rpyenv-design.md`:
- §3.1 table:
  - The Shims row's Linux cell `` `shims/` (symlinks) `` becomes `` `shims/` (hardlinks to the template) ``.
  - The Shim template row's Linux cell `n/a` becomes `` `shims/.template/` (a per-user copy of `pyenv-shim`) ``.
  - The row's Windows cell is unchanged.
- §8, replace the bullet:

  ```
  - Linux: every other executable in `versions/*/bin`, as a symlink to
    `pyenv-shim`.
  ```

  with:

  ```
  - Linux: every other executable in `versions/*/bin`, as a hardlink to
    `shims/.template/pyenv-shim`, a per-user copy of `pyenv-shim` (a copy when
    a hardlink isn't possible). With symlinks to the installed binary, a
    program that writes into one shim would overwrite `pyenv-shim` itself and
    break every shim beyond what `pyenv rehash` can repair; with the copy, only
    the copy is damaged, and rehash replaces it when its bytes differ from the
    installed binary. Linux's `fs.protected_hardlinks` also forbids a hardlink
    to a file the user doesn't own, as Windows does below.
  ```

In `docs/parity/allowlist.md`:
- D-35's rpyenv column starts with `Shims are symlinks to \`pyenv-shim\`;`. Change that to `Shims are hardlinks to \`shims/.template/pyenv-shim\`, a per-user copy of \`pyenv-shim\` that rehash refreshes when it differs;`.
- D-30's rpyenv column ends `…and passes over any hit that resolves to rpyenv's shim binary`. Append ` or into the shims folder`.

- [ ] **Step 9: Commit**

```bash
git add crates/rpyenv-core crates/pyenv/tests crates/e2e/tests docs/specs docs/parity
git commit -m "Make Linux shims hardlinks to a per-user template, as on Windows"
```

---

### Task 3: Windows output in the console's code page

**Files:**
- Create: `crates/rpyenv-core/src/textout.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod textout;`)
- Modify: `crates/rpyenv-core/src/wincp.rs` (`output_cp`, `encode_for_output`, `decode`, a shared helper, tests)
- Modify: `crates/rpyenv-core/src/winproc.rs` (`std_is_console`)
- Modify: `crates/rpyenv-core/src/lookup.rs` (`Report::emit`), `crates/pyenv/src/output.rs` (`Output::emit`)
- Modify: `crates/pyenv/tests/common/mod.rs` (`command`, output decoding), `crates/pyenv/tests/cli_version.rs` (a new test)
- Modify: `crates/e2e/tests/windows.rs` (`CREATE_NO_WINDOW` on every command that uses `pinned_850`)
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` (§11), `docs/parity/allowlist.md` (D-48)

**Interfaces:**
- Produces:
  - `textout::write(to_stderr: bool, text: &str)`
  - `wincp::output_cp() -> u32`
  - `wincp::encode_for_output(text: &str, cp: u32) -> Vec<u8>`
  - `wincp::decode(bytes: &[u8], cp: u32) -> String`
  - `winproc::std_is_console(stderr: bool) -> bool`
- `wincp::encode_for_console` (strict, used by `RPYENV_FORWARD_CP`) keeps its signature and behaviour.

- [ ] **Step 1: Write the failing unit tests (wincp.rs)**

Add to `wincp.rs`'s test module:

```rust
    /// Review focus 4. Explicit code pages, so the result doesn't depend on the host.
    #[test]
    fn encode_for_output_keeps_what_the_code_page_lacks() {
        assert_eq!(encode_for_output("José ñ", 850), b"Jos\x82 \xa4");
        // A character code page 850 lacks: the whole text in UTF-8, not just that character.
        assert_eq!(encode_for_output("a漢b", 850), "a漢b".as_bytes());
        assert_eq!(encode_for_output("José 漢", 850), "José 漢".as_bytes());
        assert_eq!(encode_for_output("José", 1252), b"Jos\xe9");
        assert_eq!(encode_for_output("José 漢", CP_UTF8), "José 漢".as_bytes());
        assert_eq!(encode_for_output("", 850), b"");
    }

    #[test]
    fn decode_reads_a_code_page() {
        assert_eq!(decode(b"Jos\x82 \xa4", 850), "José ñ");
        assert_eq!(decode("José".as_bytes(), CP_UTF8), "José");
        assert_eq!(decode(b"", 850), "");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p rpyenv-core wincp::`
Expected: compile errors, `encode_for_output` and `decode` not found.

- [ ] **Step 3: Implement (wincp.rs)**

1. Update the module doc comment's first sentence to cover both uses: `Encoding text for the console's code page (spec §4, §5.3): rpyenv's redirected output is written in it, as cmd.exe and pyenv-win write theirs, and a \`.cmd\` forwarder's \`for /f\` decodes with it.` Keep the rest.
2. Add, and use it inside `encode_for_console` in place of the duplicated two-call block (`encode_for_console` then reads `let cp = output_cp();` and calls `to_code_page(cp, &wide)`, returning `Err(cp)` on `None`):

```rust
/// The console's active output code page; 0 (`CP_ACP`, the ANSI code page) when this
/// process has no console.
pub fn output_cp() -> u32 {
    // SAFETY: `GetConsoleOutputCP` takes no arguments; it has no documented failure.
    unsafe { GetConsoleOutputCP() }
}

/// `text` for a pipe or a file. It is in code page `cp` when every character survives the
/// round trip, as cmd.exe and pyenv-win write it. Otherwise the whole text is UTF-8: a
/// reader decoding UTF-8 still gets it right, and no character becomes `?` (allowlist
/// D-48). Windows refusing the code page also gives UTF-8. Never fails.
pub fn encode_for_output(text: &str, cp: u32) -> Vec<u8> {
    if cp == CP_UTF8 {
        return text.as_bytes().to_vec();
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Vec::new();
    }
    match to_code_page(cp, &wide) {
        Some(bytes) if decode_matches(cp, &bytes, &wide) => bytes,
        _ => text.as_bytes().to_vec(),
    }
}

/// `bytes` decoded from code page `cp`; invalid sequences become U+FFFD.
pub fn decode(bytes: &[u8], cp: u32) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    // SAFETY: `bytes` is a valid buffer of `bytes.len()` bytes; a null output buffer and 0
    // length ask for the required size only.
    let needed = unsafe {
        MultiByteToWideChar(cp, 0, bytes.as_ptr(), bytes.len() as i32, std::ptr::null_mut(), 0)
    };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    // SAFETY: `wide` is a writable buffer of `needed` `u16`s, matching `cchWideChar`;
    // `bytes` is unchanged from the sizing call.
    let written = unsafe {
        MultiByteToWideChar(cp, 0, bytes.as_ptr(), bytes.len() as i32, wide.as_mut_ptr(), needed)
    };
    String::from_utf16_lossy(&wide[..written.max(0) as usize])
}

/// `wide` converted to code page `cp` with flags 0 (default-character substitution), or
/// `None` when Windows refuses. Both encoders check the result with `decode_matches`.
fn to_code_page(cp: u32, wide: &[u16]) -> Option<Vec<u8>> {
    // SAFETY: `wide` is a valid UTF-16 buffer of `wide.len()` elements; a null output
    // buffer and 0 length ask for the required size only, per `WideCharToMultiByte`'s
    // documented two-call sizing pattern.
    let needed = unsafe {
        WideCharToMultiByte(
            cp,
            0,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if needed <= 0 {
        return None;
    }
    let mut buf = vec![0u8; needed as usize];
    // SAFETY: `buf` is a valid, writable buffer of `needed` bytes, matching `cbMultiByte`;
    // `wide` and its length are unchanged from the sizing call above.
    let written = unsafe {
        WideCharToMultiByte(
            cp,
            0,
            wide.as_ptr(),
            wide.len() as i32,
            buf.as_mut_ptr(),
            buf.len() as i32,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if written <= 0 {
        return None;
    }
    buf.truncate(written as usize);
    Some(buf)
}
```

After the refactor, `encode_for_console` must still pass `ascii_round_trips_in_any_code_page`, and the forwarder e2e tests must still pass.

- [ ] **Step 4: Implement `std_is_console` (winproc.rs) and `textout`**

In `winproc.rs`, add `GetConsoleMode` to the `windows_sys::Win32::System::Console` import, then:

```rust
/// Whether this process's stdout (or, with `stderr`, stderr) is a console. std writes text
/// to a console as Unicode; anything else (a pipe, a file, NUL) gets bytes.
pub fn std_is_console(stderr: bool) -> bool {
    let which = if stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    let mut mode = 0u32;
    // SAFETY: reads this process's own standard handle; `GetConsoleMode` writes one `u32`
    // into `mode` and fails harmlessly on a handle that isn't a console.
    unsafe {
        let h = GetStdHandle(which);
        usable(h) && GetConsoleMode(h, &mut mode) != 0
    }
}
```

Create `crates/rpyenv-core/src/textout.rs`:

```rust
//! Where rpyenv's own text reaches stdout and stderr. On Windows, text going to a pipe, a
//! file or NUL is encoded in the console's output code page, as cmd.exe and pyenv-win write
//! theirs (spec §4, §11); text going to a console is handed to std, which writes it as
//! Unicode. On Linux the text's UTF-8 bytes are written as they are.

use std::borrow::Cow;
use std::io::Write;

/// Writes `text` to stderr (`to_stderr`) or stdout, flushing stdout. Errors are ignored:
/// there is nowhere left to report them.
pub fn write(to_stderr: bool, text: &str) {
    let bytes = encode(to_stderr, text);
    if to_stderr {
        let _ = std::io::stderr().write_all(&bytes);
    } else {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&bytes);
        let _ = out.flush();
    }
}

#[cfg(windows)]
fn encode(to_stderr: bool, text: &str) -> Cow<'_, [u8]> {
    if crate::winproc::std_is_console(to_stderr) {
        Cow::Borrowed(text.as_bytes())
    } else {
        Cow::Owned(crate::wincp::encode_for_output(text, crate::wincp::output_cp()))
    }
}

#[cfg(not(windows))]
fn encode(_to_stderr: bool, text: &str) -> Cow<'_, [u8]> {
    Cow::Borrowed(text.as_bytes())
}
```

Add `pub mod textout;` to `crates/rpyenv-core/src/lib.rs`, in alphabetical order with the other modules.

- [ ] **Step 5: Route both emitters through `textout`**

In `lookup.rs`, `Report::emit` becomes:

```rust
    /// Writes the lines with the flavor's line ending.
    pub fn emit(&self, flavor: Flavor) {
        let text: String = self
            .lines
            .iter()
            .map(|l| format!("{l}{}", flavor.eol()))
            .collect();
        crate::textout::write(self.stderr, &text);
    }
```

Remove `use std::io::Write;` from `lookup.rs` if nothing else uses it; clippy will say.

In `crates/pyenv/src/output.rs`, `Output::emit` becomes:

```rust
    /// Writes stderr, then stdout.
    pub fn emit(&self, flavor: Flavor) {
        let convert = |s: &str| match flavor {
            Flavor::Pyenv => s.to_string(),
            Flavor::PyenvWin => s.replace('\n', "\r\n"),
        };
        rpyenv_core::textout::write(true, &convert(&self.stderr));
        match &self.raw_stdout {
            // Already encoded for the console (`RPYENV_FORWARD_CP`): written as it is.
            Some(bytes) => {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(bytes);
                let _ = stdout.flush();
            }
            None => rpyenv_core::textout::write(false, &convert(&self.stdout)),
        }
    }
```

- [ ] **Step 6: Make the tests decode what rpyenv now writes**

In `crates/pyenv/tests/common/mod.rs`:

1. Split `run` so that tests can build the same command for another program:

```rust
    /// A command for `program` with this fixture's clean environment, run in `dir`.
    pub fn command(&self, program: &Path, dir: &Path, env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(dir)
            .env_clear()
            .env("PYENV_ROOT", &self.root)
            .env("HOME", &self.base)
            .env("USERPROFILE", &self.base)
            .env("PATH", &self.syspath)
            .env("PWD", dir);
        if let Some(v) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", v);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    fn run(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
        let out = self
            .command(Path::new(env!("CARGO_BIN_EXE_pyenv")), dir, env)
            .args(args)
            .output()
            .unwrap();
        Run {
            stdout: decode(&out.stdout),
            stderr: decode(&out.stderr),
            code: out.status.code().unwrap(),
        }
    }
```

2. Add, at module level:

```rust
/// rpyenv's redirected output as text. On Windows it is in the console's output code page
/// (spec §11); this test process shares that console with the `pyenv` it ran.
pub fn decode(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        rpyenv_core::wincp::decode(bytes, rpyenv_core::wincp::output_cp())
    }
    #[cfg(not(windows))]
    {
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}
```

3. In `crates/e2e/tests/windows.rs`, every command chain that calls `.raw_arg(pinned_850(…))` also gets `.creation_flags(CREATE_NO_WINDOW)` (Decision 4). `grep -c "pinned_850(" crates/e2e/tests/windows.rs` gave 5 on 2026-10-02: four call sites plus the definition. Add the flag to each call site, and extend the `pinned_850` doc comment with: `Callers start cmd.exe with \`CREATE_NO_WINDOW\`, so the \`chcp\` changes a console of its own, not the one this test shares with parallel tests.`

- [ ] **Step 7: Write the end-to-end encoding test**

Add to `crates/pyenv/tests/cli_version.rs`:

```rust
/// Review focus 3. The fixture's root holds `ñ`. With the console's code page pinned in a
/// console of the test's own, redirected output must be in that code page, as cmd.exe and
/// pyenv-win write it: `ñ` is 0xA4 in code page 850 and 0xC3 0xB1 in UTF-8 (65001).
#[cfg(windows)]
#[test]
fn win_redirected_output_uses_the_console_code_page() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let f = Fixture::new();
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let pyenv = env!("CARGO_BIN_EXE_pyenv");
    let run = |cp: u32| {
        f.command(&system.join("cmd.exe"), &f.work, &[])
            .raw_arg(format!(
                r#"/d /s /c ""{}" {cp} >nul & "{pyenv}" root""#,
                system.join("chcp.com").display()
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap()
            .stdout
    };
    let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    let oem = run(850);
    assert!(has(&oem, b"py env \xa4\\root\r\n"), "{oem:?}");
    let utf8 = run(65001);
    assert!(has(&utf8, "py env ñ\\root\r\n".as_bytes()), "{utf8:?}");
}
```

- [ ] **Step 8: Run the tests**

Run: `cargo build --workspace`, then `cargo test --workspace`.

Expected: PASS, with the two new `wincp` tests and `win_redirected_output_uses_the_console_code_page`.

Mutation check:
1. Make `textout::encode` on Windows always return `Cow::Borrowed(text.as_bytes())`.
2. Rebuild with `cargo build --workspace`.
3. Run `cargo test -p pyenv --test cli_version win_redirected`. Expected: FAIL on the code-page-850 assertion.
4. Restore the code.

Report all four steps.

- [ ] **Step 9: Spec note and allowlist row**

In spec §11, after the **Where errors go:** bullet list, add:

```
- **Encoding (Windows):** text rpyenv writes to a pipe, a file or NUL is in
  the console's output code page (with no console, the ANSI code page), as
  cmd.exe and pyenv-win write theirs. When a character doesn't fit that code
  page, the whole text is written as UTF-8 instead of turning the character
  into `?` (allowlist D-48); a console set to code page 65001 gets UTF-8
  always. Text written to a console is Unicode. Linux writes UTF-8.
```

In `docs/parity/allowlist.md`, add after the D-47 row:

```markdown
| D-48 | Windows | all output to a pipe or a file | Written in the console's code page; a character the code page lacks most likely becomes `?` (not measured; the M1c-b differential tests check it) | Written in the console's code page when every character fits; otherwise the whole text is written as UTF-8 | `?` turns a path into one that doesn't exist and loses the character. A reader decoding with the code page gets a broken path either way, but one decoding UTF-8 then gets it right. |
```

- [ ] **Step 10: Commit**

```bash
git add crates/rpyenv-core crates/pyenv crates/e2e/tests/windows.rs docs/specs docs/parity
git commit -m "Write redirected Windows output in the console's code page, as pyenv-win does"
```

---

### Task 4: Start-failure text and a consistent debug log

**Files:**
- Modify: `crates/rpyenv-core/src/launch.rs` (`cannot_run`, a new `start_failure_reason`, a test)
- Modify: `crates/rpyenv-core/src/debuglog.rs` (`set_source`, `append`, `append_to`, tests)
- Modify: `crates/rpyenv-core/src/winproc.rs` (`spawn_and_wait`: `job=none`)
- Modify: `crates/pyenv/src/commands/exec.rs` (source name; log plan errors and warnings)

**Interfaces:**
- Produces:
  - `debuglog::set_source(name: &'static str)`
  - `debuglog::append_to(path: &Path, source: &str, line: &str)`
- `debuglog::append(line: &str)` keeps its signature.

- [ ] **Step 1: Write the failing tests**

In `launch.rs`'s test module:

```rust
    /// Review focus 5: Windows leaves `%1` in some messages for the program's name
    /// (`ERROR_BAD_EXE_FORMAT`: "%1 is not a valid Win32 application.").
    #[cfg(windows)]
    #[test]
    fn start_failure_names_the_program() {
        let err = std::io::Error::from_raw_os_error(193);
        let reason = start_failure_reason(Path::new(r"C:\v\bad.exe"), &err);
        assert!(!reason.contains("%1"), "{reason}");
        assert!(reason.contains("bad.exe"), "{reason}");
    }
```

Replace `debuglog.rs`'s test module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_whole_lines_with_their_source() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("debug.log");
        append_to(&log, "rpyenv-shim", "a");
        append_to(&log, "pyenv exec", "b");
        let eol = if cfg!(windows) { "\r\n" } else { "\n" };
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("rpyenv-shim: a{eol}pyenv exec: b{eol}")
        );
    }

    #[test]
    fn an_unopenable_log_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        append_to(&tmp.path().join("missing-dir").join("debug.log"), "x", "y");
    }
}
```

The old test changed `RPYENV_DEBUG_LOG` for the whole test process. `append_to` is the seam that replaces that.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p rpyenv-core debuglog:: launch::`
Expected: compile errors, `append_to` and `start_failure_reason` not found.

- [ ] **Step 3: Implement**

`launch.rs`: in `cannot_run`, replace `io_reason(err)` with `start_failure_reason(program, err)`, and add:

```rust
/// `io_reason`, with the `%1` that some Windows messages leave for the program's name
/// filled in (`FormatMessage` is called without inserts).
fn start_failure_reason(program: &Path, err: &std::io::Error) -> String {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    io_reason(err).replace("%1", &name)
}
```

`debuglog.rs` becomes:

```rust
//! `RPYENV_DEBUG_LOG` (spec §11, §13): a file a shim, or `pyenv exec`, appends its
//! decisions and errors to, for when there is no console to print on.

use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

static SOURCE: OnceLock<&'static str> = OnceLock::new();

/// Names the program at the start of every later line: `rpyenv-shim` unless this is
/// called first. The first call wins.
pub fn set_source(name: &'static str) {
    let _ = SOURCE.set(name);
}

/// Appends `<source>: <line>` to the file `RPYENV_DEBUG_LOG` names. Nothing happens when
/// it is unset or empty.
pub fn append(line: &str) {
    let Some(path) = std::env::var_os("RPYENV_DEBUG_LOG").filter(|p| !p.is_empty()) else {
        return;
    };
    append_to(
        Path::new(&path),
        SOURCE.get().copied().unwrap_or("rpyenv-shim"),
        line,
    );
}

/// Appends `<source>: <line>` and the platform's line ending to `path` in one write, so
/// lines from nested shims sharing one log don't interleave. Failures are ignored: a
/// diagnostic must never stop the shim.
pub fn append_to(path: &Path, source: &str, line: &str) {
    let eol = if cfg!(windows) { "\r\n" } else { "\n" };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(format!("{source}: {line}{eol}").as_bytes());
    }
}
```

followed by the test module from Step 1.

`winproc.rs`, in `spawn_and_wait`, replace:

```rust
    if let Some(job) = &job {
        job.assign(&child);
    }
```

with:

```rust
    // Without the job, killing the shim leaves the child running (D-45); say so in the log.
    if !job.as_ref().is_some_and(|j| j.assign(&child)) {
        debuglog::append("job=none");
    }
```

`crates/pyenv/src/commands/exec.rs`:
1. Make `rpyenv_core::debuglog::set_source("pyenv exec");` the first statement of `exec`.
2. In the `Err(report) => report.into(),` arm, log each line first:

   ```rust
        Err(report) => {
            for line in &report.lines {
                rpyenv_core::debuglog::append(line);
            }
            report.into()
        }
   ```

3. In the warnings loop, log each warning before `before.err(w);` with `rpyenv_core::debuglog::append(w);`.

- [ ] **Step 4: Run the tests**

Run: `cargo build --workspace`, then `cargo test --workspace`.
Expected: PASS. The e2e helper `mode_in` reads the log with `str::lines`, which strips `\r`, so the console-mode tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/rpyenv-core crates/pyenv/src
git commit -m "Name the program in start failures and keep the debug log's lines whole"
```

---

### Task 5: Windows test gaps

**Files:**
- Modify: `crates/e2e/tests/windows.rs` (a new start-failure test; a kill guard for the two Ctrl tests; box-text assertion in the ignored box test)
- Modify: `crates/pyenv/tests/cli_version.rs` (a new Windows `version-file-read` / `version-file-write` test)

**Interfaces:**
- Consumes:
  - `debuglog`'s `rpyenv-shim: ` prefix and Windows `\r\n` (Task 4);
  - `start_failure_reason` (Task 4);
  - `decode` in the CLI test helpers (Task 3).

- [ ] **Step 1: Write the tests**

In `crates/e2e/tests/windows.rs`, add:

```rust
/// Review focus 5: a program that can't start is reported on stderr and in the debug log,
/// named, with exit code 126 (allowlist D-43), and without a literal `%1`.
#[test]
fn win_a_start_failure_reaches_the_debug_log() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let bad = f.root.join("versions").join("3.9.1").join("bad.exe");
    std::fs::copy(built("argv-echo"), &bad).unwrap();
    f.rehash();
    std::fs::write(&bad, b"not a PE file").unwrap();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command(
            "bad",
            &[("PYENV_VERSION", v("3.9.1")), ("RPYENV_DEBUG_LOG", log.as_os_str())],
        )
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(126));
    let text = std::fs::read_to_string(&log).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("rpyenv-shim: pyenv: "))
        .unwrap_or_else(|| panic!("no start-failure line in the log:\n{text}"));
    assert!(line.contains("bad.exe: "), "{line}");
    assert!(!line.contains("%1"), "{line}");
}

/// Ends the child when a test leaves early (a failed assert or a panic), so a failure
/// never leaves a shim and its 30-second child running.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
```

In `win_ctrl_break_reaches_the_child_and_the_shim_waits` and in `win_ctrl_c_reaches_the_child_through_the_shim`:
1. Rename `let mut shim = f … .spawn().unwrap();` to `let mut shim = KillOnDrop(f … .spawn().unwrap());`.
2. Use `shim.0.id()` and `shim.0.wait()` where `shim.id()` and `shim.wait()` were.
3. Delete the Ctrl+C test's `if sent.code() != Some(0) { let _ = shim.kill(); }`, which the guard replaces.

In the ignored `win_gui_shim_shows_a_message_box_when_it_has_nowhere_to_print`:
1. Add `text: String` to `Search`, initialized to `String::new()`.
2. When the `"rpyenv"` window is found, collect its child controls' text before returning 0:

```rust
            unsafe extern "system" fn each_child(child: HWND, lparam: LPARAM) -> BOOL {
                // SAFETY: `lparam` is `&mut String`, valid for the whole
                // `EnumChildWindows` call below.
                let text = unsafe { &mut *(lparam as *mut String) };
                let mut buf = [0u16; 1024];
                // SAFETY: `WM_GETTEXT` copies at most `buf.len()` UTF-16 units into `buf`;
                // the system marshals it across processes, which `GetWindowTextW` would not
                // do for another process's control.
                let len = unsafe {
                    SendMessageW(child, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as isize)
                };
                text.push_str(&String::from_utf16_lossy(&buf[..usize::try_from(len.max(0)).unwrap_or(0)]));
                text.push('\n');
                1
            }
            // SAFETY: `each_child` only dereferences the pointer during this call, and
            // `search.text` outlives it.
            unsafe {
                EnumChildWindows(hwnd, Some(each_child), std::ptr::addr_of_mut!(search.text) as isize);
            }
```

3. Add `EnumChildWindows`, `SendMessageW` and `WM_GETTEXT` to the `WindowsAndMessaging` import.
4. After the existing `assert!(search.found, …)`, add `assert!(search.text.contains("bad.exe: "), "box text: {}", search.text);`, so the test can't pass on the older "command not found" box.

`each_child` must not panic, because a panic in an `extern "system"` function aborts the process and skips the `Killer` guard. Keep it free of `unwrap` on fallible values, as written.

In `crates/pyenv/tests/cli_version.rs`, add the Windows counterpart of `version_file_read_and_write`, with values measured from rpyenv's Windows build on 2026-10-02:

```rust
#[cfg(windows)]
#[test]
fn win_version_file_read_and_write() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    f.file(&f.work.join("vf"), "3.12.1\r\n3.11.9\r\n");
    assert_eq!(
        f.pyenv(&["version-file-read", "vf"]).stdout,
        "3.12.1:3.11.9\r\n"
    );
    f.file(&f.work.join("empty"), "");
    let r = f.pyenv(&["version-file-read", "empty"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
    let r = f.pyenv(&["version-file-read", "missing"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
    assert_eq!(
        f.pyenv(&["version-file-write", "out", "3.12.1", "3.11.9"]).code,
        0
    );
    assert_eq!(
        std::fs::read(f.work.join("out")).unwrap(),
        b"3.12.1\r\n3.11.9\r\n"
    );
    let r = f.pyenv(&["version-file-write", "out", "9.9"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("", "pyenv: version `9.9' not installed\r\n", 1)
    );
    assert_eq!(
        std::fs::read(f.work.join("out")).unwrap(),
        b"3.12.1\r\n3.11.9\r\n"
    );
    let r = f.pyenv(&["version-file-write", "out"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        (
            "",
            "Usage: pyenv version-file-write [-f|--force] <file> <version> [...]\r\n",
            1
        )
    );
}
```

- [ ] **Step 2: Run them**

Run: `cargo build --workspace`, then `cargo test --workspace`, then `cargo test -p rpyenv-e2e --test windows -- --ignored`. The last one opens and closes a message box.

Expected:
- PASS, with `win_a_start_failure_reaches_the_debug_log` and `win_version_file_read_and_write` among the new tests.
- The ignored box test passes, and the guard leaves no process behind. Check with `Get-Process | Where-Object { $_.Path -like '*\Temp\*' }`, which should list no `bad`, `pyenv-shim` or `pyenv-shimw` processes.

Mutation checks. Rebuild with `cargo build --workspace` before each, and restore afterwards:
1. In `cannot_run`, use `io_reason(err)` again. Expected: `win_a_start_failure_reaches_the_debug_log` fails on `%1`.
2. In the box test, replace `"bad.exe: "` with `"command not found"`. Expected: the ignored test fails on the text assertion.

- [ ] **Step 3: Commit**

```bash
git add crates/e2e/tests/windows.rs crates/pyenv/tests/cli_version.rs
git commit -m "Test start failures in the debug log, the box text and version files on Windows"
```

---

## After this plan

**M1c-b (next plan): parity CI.**
- **What it is:**
  - the upstream pyenv bats suite (commit `ab74141`, bats 1.11.1) run in a `debian:trixie-slim` container;
  - pyenv-win's pytest suite (commit `856ed5a`) adapted by appending a conftest overlay;
  - differential tests against real pyenv and pyenv-win that read `docs/parity/allowlist.md`;
  - a check that every allowlist row is covered by some test (which closes the M1a gap: D-04, D-17, D-19, D-21, D-22);
  - the hyperfine shim-overhead benchmark in the CI summary.
- **Probes made while writing this plan (2026-10-02, before M1c-a):**
  - **bats, all upstream files, against rpyenv 6790172:**
    - Results: 151 ok, 8 skipped (fish and pwsh not installed), 115 not ok.
    - 62 of the failures are commands from later milestones: `init` 34, `shell` 15, `latest` 10, `completions` 3.
    - The rest are bash hooks (spec D4), plugin and libexec commands (M4, D-38), `commands` listing libexec, help parsed from command sources, lock and state internals (D-35), `pip-rehash` (D-39), `--version` from git (D-01), and `sort` without `-V` (a new row).
    - Several `rehash.bats` repair tests should pass after Task 2.
  - **pyenv-win pytest, both architectures, against the same build:**
    - AMD64: 97 passed, 45 failed. X86: 97 passed, 42 failed, 3 skipped. The X86 failures are a subset of the AMD64 ones.
    - The in-scope failures are all allowlisted: `python.bat` shims (D-36) and the PATH check's `python.exe` (D-15).
    - The rest are `shell` (M3), `latest` and `install` (M2).
  - **pyenv-win output encoding:** console code page, not UTF-8. This led to Task 3.
- **Constraints learned:**
  - The bats suite writes into shims, so the rpyenv binaries it uses must be root-owned copies. The container gives that.
  - GitHub's Ubuntu runners have `pwsh`, which would turn 4 skipped tests into failures. The container avoids that too.
  - The expected-failure lists must be measured after this plan, not before.

**Later:**
- M2 (installer), M3 (shell integration), M4 (virtualenvs and plugins), M5 (EAGER/LAZY consoles, live rehash, lock removal on Ctrl+C), M6 (release and MSI, ARM64 suffix), M7 (conda and PyPy).
- Carried, low priority: the stdin→NUL test and the `window_station_visible` false-branch test (Decision 6).
