# M4b research: pyenv-virtualenv's bats suite as a third upstream suite, and Windows virtualenvs

Research for plan M4b (virtualenvs). Part 1 covers running pyenv-virtualenv v1.4.0's bats suite
against rpyenv. Part 2 covers Windows, where pyenv-win has no virtualenv support. The behavior
of each pyenv-virtualenv command is in the companion behavior reference, not here.

Written 2026-10-05 against `main` at 6fc3604. Where a number was measured, the method is stated
next to it.

## Part 1: pyenv-virtualenv's bats suite

### 1.1 The pinned upstream

| | |
|---|---|
| Tag | `v1.4.0` |
| Commit | `eda64556af9b2992386deeb75dad2130899fc4c9` (read from the `comment` pax header that `git archive` writes into GitHub's tag tarball) |
| Tarball | `https://github.com/pyenv/pyenv-virtualenv/archive/refs/tags/v1.4.0.tar.gz`, sha256 `9aaf9f01660f10f538251fdaaf552d429e7fd41efb7b651b69a3a9768f4a181f` (fetched 2026-10-05) |
| Local copy | WSL `/home/jm/m4b-upstream/pyenv-virtualenv-1.4.0` |
| Upstream's own CI | bats-core `v1.10.0` (cloned from git), on Ubuntu 22.04/24.04 and macOS 14, 15, 15-intel and 26 |

Like `PYENV_SHA`, CI should fetch by commit (`archive/<sha>.tar.gz`), not by tag. A tag can be
moved; a commit can't.

### 1.2 What the suite is

The suite has 18 files and 111 `@test`s (counted with `grep -c '^@test'` on each file):

| File | Tests | Command under test |
|---|---|---|
| activate.bats | 21 | `pyenv-sh-activate`, `pyenv-activate` |
| conda-activate.bats | 6 | `pyenv-sh-activate` (conda) |
| conda-deactivate.bats | 3 | `pyenv-sh-deactivate` (conda) |
| conda-prefix.bats | 2 | `pyenv-virtualenv-prefix` (conda) |
| conda.bats | 3 | `pyenv-virtualenv` (conda create) |
| deactivate.bats | 12 | `pyenv-sh-deactivate`, `pyenv-deactivate` |
| delete.bats | 5 | `pyenv-virtualenv-delete` |
| envs.bats | 1 | `pyenv-virtualenv` (a root path that contains `envs`) |
| hooks.bats | 3 | `pyenv-virtualenv`, `pyenv-sh-activate`, `pyenv-sh-deactivate` with hook scripts |
| init.bats | 9 | `pyenv-virtualenv-init` |
| installer.bats | 3 | the plugin's own `install.sh` |
| pip.bats | 2 | `pyenv-virtualenv` (ensurepip / get-pip) |
| prefix.bats | 11 | `pyenv-virtualenv-prefix` |
| python.bats | 4 | `pyenv-virtualenv --python=` |
| pyvenv.bats | 6 | `pyenv-virtualenv` (venv vs virtualenv) |
| version.bats | 2 | `pyenv-virtualenv --version` |
| virtualenv.bats | 10 | `pyenv-virtualenv` |
| virtualenvs.bats | 8 | `pyenv-virtualenvs` |

**Q1. What does each test invoke?** Only `pyenv-<cmd>` names, found on `PATH`. The tests never
call `pyenv <cmd>`, and they never call a script by path, except `installer.bats`, which runs
`${BATS_TEST_DIRNAME}/../install.sh`. `test_helper.bash` sets the `PATH` itself and discards the
caller's:

```bash
PATH=/usr/bin:/usr/sbin:/bin:/sbin
PATH="$BATS_TEST_DIRNAME/../bin:$PATH"   # the plugin's own bin/
PATH="$TMP/bin:$PATH"                    # the stubs, first
```

`TMP` is `$BATS_TEST_DIRNAME/tmp`, so the test folder must be writable. `teardown` runs
`rm -fr "$TMP"/*`. No pyenv core is on `PATH`. The suite is a unit suite of the plugin's
scripts: every core command they call is stubbed. The plugin's scripts also call `pyenv` by
name in one place, `pyenv-virtualenv-init`'s `pyenv hooks version-name`.

**Q2. Stubbing.** The suite doesn't use bats-mock. It has its own stub helper:
`test/stubs/stub` plus `stub`/`unstub` in `test_helper.bash`. That helper is a bats-mock
derivative with `-N` (no order) and `-M` (multiple) flags and a lock file in `$TMPDIR`.

- `stub <prog> "<arg patterns> : <command>" …` symlinks `$TMP/bin/<prog>` to the stub and
  writes a plan with one line per expected call.
- `unstub <prog>` runs the stub in verify mode. It **fails the test if any regular (`-`) plan
  line was never consumed**, so the test also checks that the script called the stub. It prints
  the plan and the log on failure.

The stubbed programs are `pyenv-version-name`, `pyenv-version-origin`, `pyenv-prefix`,
`pyenv-hooks`, `pyenv-rehash`, `pyenv-exec`, `pyenv-which`, `pyenv-whence`, `pyenv-latest`,
`python-build`, `curl`, and the plugin's own `pyenv-virtualenv-prefix` and `pyenv-sh-deactivate`
(the last two because the plugin's scripts call each other).

How this meets rpyenv's single binary: rpyenv answers every one of these in-process. A stub on
`PATH` is never consulted, so:

1. its output is missing. For example, the `deactivated` line that `pyenv-sh-deactivate`'s stub
   prints, or the `PYENV_VERSION=… virtualenv …` echo of a stubbed `pyenv-exec`;
2. the "facts" it asserts don't hold on disk. Stubs claim `venv27` is a virtualenv, or that
   `PYENV_VERSION` resolves to `venv314`, while the fixture tree says otherwise;
3. `unstub` fails because the stub was never called.

This is the same mechanism as core pyenv's suite (allowlist D-52, `latest.bats`), but here it
covers most of the suite rather than seven tests. Point 3 alone can be fixed in the harness.
Points 1 and 2 can't, without rewriting the tests.

**Q3. Can it run against rpyenv?** Yes. The plumbing was prototyped and works (§1.4). The
harness puts rpyenv where the plugin's `bin/` would be:

```
run/bin/pyenv, run/bin/pyenv-shim                      rpyenv (root-owned copies)
run/bin/pyenv-{activate,deactivate,sh-activate,sh-deactivate,virtualenv,
              virtualenv-delete,virtualenv-init,virtualenv-prefix,virtualenvs}
                                                       -> symlinks to pyenv (multicall, D-94)
run/test/                                              upstream's test/ only (writable by tester)
```

Upstream's `bin/`, `libexec/`, `shims/` and `etc/pyenv.d/` are **not** copied: their scripts
would answer instead of rpyenv. The links can be plain symlinks. `main.rs` already acts as
`pyenv <cmd>` when it runs as `pyenv-<cmd>` for a built-in (D-94). Core's `bats_run.sh` uses
`#!/bin/sh` wrappers with an absolute path, because core tests stub `pyenv` itself on `PATH`.
This suite never stubs `pyenv`, but the wrappers do no harm either; keep them if you prefer one
style across both runners. `run/bin/pyenv` must be rpyenv in either case, because
`pyenv-virtualenv-init` calls `pyenv hooks` through `PATH`.

**Q4. Expected pass rate.** This was measured by emulation, because rpyenv has no virtualenv
commands yet. A "faithful built-in" was modelled as upstream's own plugin scripts plus real
upstream pyenv core (`07171d0`, the `PYENV_SHA` CI pins). An overlay on `test_helper.bash` kept
every stub of the 13 programs above off `PATH`, so the real command answered, as an in-process
implementation would. `curl` was replaced by a failing stand-in, so no test reached the network.
Two variants were run with bats 1.11.1 in WSL Debian:

| Harness variant | Pass | Fail | Of which `installer.bats` |
|---|---|---|---|
| Upstream, unmodified (sanity) | 111 | 0 | 3 pass |
| **strict**: `unstub` verifies as upstream does | 35 | 76 | 3 pass |
| **lenient**: `unstub` of an in-process command only removes it | 60 | 51 | 3 pass |

The emulation is a model, not rpyenv. It predicts how a correct implementation fares, but it
can't see rpyenv's own bugs or intended differences. For example, rpyenv runs no hooks (D-51);
here that held only because `pyenv-hooks` found no hook path. The 3 `installer.bats` tests pass
in every variant, but they test upstream's `install.sh`, not rpyenv (see the recommendation).
Without that file the suite has 108 tests: **32 pass under strict, 57 under lenient**.

The 25 tests the lenient overlay gains have assertions that hold for a correct implementation;
only the "was the stub called" check fails. They are:

- `prefix.bats` 1–10
- `delete.bats` 2–5 (delete through or beside the link)
- `virtualenvs.bats` 2 and 6
- `conda-prefix.bats` 1–2
- `conda-deactivate.bats` 1–3 (those fail strict only through `teardown`'s `unstub pyenv-hooks`)
- `activate.bats` 13, 14, 16, 17 (not-a-virtualenv and `--quiet` failures)

The 51 that fail even under lenient are failures by design:

| Reason | Tests | Allowlist |
|---|---|---|
| Creation through a stubbed `pyenv-exec`: the stub echoes the command and fakes the result (the fixture `python` is an empty file), and rpyenv runs the base Python itself | virtualenv.bats 1–5, 10; pyvenv.bats 1–6; pip.bats 1–2; envs.bats 1; conda.bats 1–3; version.bats 1–2; python.bats 1–3 (24) | new row, or widen D-52 |
| Output a stub prints (`deactivated` from `pyenv-sh-deactivate`), with `PYENV_ROOT/versions/venv` not on disk | activate.bats 1–12; conda-activate.bats 1–6 (18) | D-52 (widened) |
| A stub states what the disk contradicts: a version selection, a plain folder claimed as a virtualenv, `python-build --definitions` contents | activate.bats 15, 18; delete.bats 1; prefix.bats 11; virtualenvs.bats 1, 3, 7 (7) | D-52 (widened) |
| Hook scripts (`before_activate`, `after_virtualenv`, …) | hooks.bats 1–3 (3) | D-51 |

The second and third groups total 25 of these. The numbers come from the TAP logs of the
emulation runs. The reasons come from reading each failing test and from the failure output of
two of them (`python.bats` 3: `python-build --definitions` is stubbed to list `python2.7`;
`activate.bats` 15: the stub says `venv` is a virtualenv, the empty fixture says it isn't).

macOS-only cases: none. The only platform switch is `stat -c %Y` vs `stat -f %m` in `init.bats`,
which reads the platform's own format, so GNU `stat` in Debian is right.

`init.bats` (9 tests) passes only if rpyenv implements `virtualenv-init` with byte-identical
output. That output includes `export PATH="$PYENV_VIRTUALENV_ROOT/shims:…"`, the plugin's
folder of error-printing `activate`/`deactivate` shims, and the `PROMPT_COMMAND` /
`precmd_functions` / fish auto-activation hook. Spec §6 and §10 don't list `virtualenv-init`. If
M4b leaves it out, these 9 move to the expected list (under strict they are 9 of the 32 passes),
which gives **23 strict / 48 lenient**.

**Q5. Bats and helpers.** The suite needs no bats-support, bats-assert or bats-mock. Its helpers
are self-contained (`test_helper.bash`, `test/stubs/stub`). bats-core **1.11.1** (the
`BATS_VERSION` CI already uses) ran all 111 against upstream: 111 ok, 0 not ok, no skips, in
WSL Debian with bash 5.3.3 (run 2026-10-05, `bats --tap test` in a scratch copy). Upstream's CI
uses 1.10.0; that version was not tried. Runtime tools: bash, coreutils (`stat`, `ln`, `tr`,
`install`, `mktemp`), sed, grep. These are all in Debian's essential set, so
`debian:trixie-slim` should have them; that wasn't checked inside the container. Upstream's
`pyenv-virtualenv-init` calls `ps` (procps, not in slim images), but rpyenv reads
`/proc/<ppid>/cmdline` first (`rpyenv-core/src/shellname.rs`), so no extra package is expected
to be needed.

### 1.3 Recommendation: harness design

1. **A sibling runner, `parity/bats_venv_run.sh`**, with the same signature and safety model as
   `bats_run.sh`: `<rpyenv-bin-dir> <pyenv-virtualenv-src> <bats-dir> <tap-out>`. It runs as
   root in the container, each file runs as `tester`, the binaries are root-owned copies, and
   each TAP line is prefixed with `<file>\t`. It lays out `run/` as in Q3, appends the overlay
   below to `run/test/test_helper.bash`, `chown -R tester run/test`, and runs every `*.bats`
   except `installer.bats`. A separate script is simpler than adding modes to `bats_run.sh`:
   the two layouts share almost nothing (no `pyenv.d`, no `libexec`, no completions, different
   `PATH` model).

2. **The lenient overlay.** It changes how the harness works, not what the tests assert. It
   follows the precedent of `pyenv_win_overlay.py`, which patches pyenv-win's `conftest.py`:

   ```bash
   # --- rpyenv overlay: appended to test/test_helper.bash by parity/bats_venv_run.sh ---
   # rpyenv answers these in-process (allowlist D-52), so a test's stub of one is never run;
   # unstub only removes it. Every other unstub verifies as upstream's does.
   _RPY_INPROC=" pyenv-version-name pyenv-version-origin pyenv-prefix pyenv-hooks pyenv-rehash pyenv-exec pyenv-which pyenv-whence pyenv-latest python-build curl pyenv-virtualenv-prefix pyenv-sh-deactivate "
   eval "_rpy_up_unstub() $(declare -f unstub | tail -n +2)"
   unstub() {
     if [[ $_RPY_INPROC == *" $1 "* ]]; then
       rm -f "$TMP/bin/$1" "$TMP/$1-stub-plan" "$TMP/$1-stub-run" "$TMP/$1-stub-log"
       return 0
     fi
     _rpy_up_unstub "$@"
   }
   ```

   It roughly doubles the useful coverage, from 32 to 57 of the 108 tests (emulated).
   Everything it skips is a "did you shell out to X" check that rpyenv fails by design anyway.
   The alternative is to keep strict and list 76 expected failures, mostly as D-52; that tests
   little beyond `deactivate` output, `init` output and name validation. If the overlay is
   adopted, each `parity/expected/bats-virtualenv.txt` reason should say "stub output" or
   "stub facts", never "stub not called", because the overlay removes that cause.

3. **Leave out `installer.bats`.** It tests the plugin's `install.sh`. Run against rpyenv's
   layout, it would either fail (no `install.sh`) or pass while testing nothing. Leave it out of
   the loop and of the files list, and say why in a comment.

4. **The checker.** Add a suite argument to `parity/bats_check.py`, for example
   `bats_check.py --suite virtualenv <tap>`. It selects
   `parity/expected/bats-virtualenv.txt` (`<file> | <test> | <reason>`, the same format) and
   `parity/expected/bats-virtualenv-files.txt` (17 files), and a summary heading "Upstream
   pyenv-virtualenv bats suite". The structural and strict checks stay as they are.

5. **`parity/allowlist.py`.** `MILESTONES` has `M4` but not `M4a` or `M4b`. A reason tag is the
   reason's first whole word, so `M4b …` is currently rejected as "neither an allowlist row nor
   a milestone". Add `M4a` and `M4b`. Separately, `DELIVERED` doesn't list `M4a`, although M4a
   is merged; adding it is M4a's step (1) of spec §12.3, which looks missed.

6. **Allowlist rows.** D-52 is worded for `prefix`, `which`, `latest` and shims. Either widen it
   to "pyenv-virtualenv's suite stubs core commands on PATH", or add one Linux row for
   "`virtualenv` creates the env by running the base version's Python itself, not through
   `pyenv-exec`" (24 tests) and widen D-52 for the 25 stub-output and stub-fact tests. D-51
   covers `hooks.bats`.

7. **Ordering.** The suite can land before the commands (TDD style). Every test then fails
   except 3 that pass by accident. Prototype result: `activate.bats` 21 and `deactivate.bats`
   12 only assert failure, and `init.bats` 2 asserts absence; today `pyenv-activate` falls
   through to `pyenv` with no command and exits 1. The expected list would then cite `M4b` for
   the rest (after point 5). The strict checker tells you which ones to drop as each command
   lands.

### 1.4 Plumbing prototype against today's rpyenv

The prototype used the WSL build `/home/jm/rpyenv-linux/target/debug`. That clone's head
commit, `b6bd585`, has the same subject as `main`'s `6fc3604`. Upstream's `test/` and the overlay went into a scratch copy, and rpyenv
plus the 9 multicall symlinks into `bin/`. It ran 17 files (installer left out) as user `jm`,
not as `tester` in the container. Result: 108 TAP results with correct `1..N` plans, 3 ok
(listed in §1.3 point 7), 105 not ok. The failures show the `pyenv` help text, since no
virtualenv command is built in yet. Strict and lenient gave the same result, as expected with
nothing implemented. The prototype does **not** confirm the `tester`/`runuser` container path;
that path is `bats_run.sh`'s and unchanged.

### 1.5 CI wiring

In `.github/workflows/parity.yml`, job `linux`:

```yaml
env:
  PYENV_VIRTUALENV_SHA: eda64556af9b2992386deeb75dad2130899fc4c9   # v1.4.0
# in "Fetch upstream pyenv and bats":
curl -fsSL "https://github.com/pyenv/pyenv-virtualenv/archive/${PYENV_VIRTUALENV_SHA}.tar.gz" | tar -xz -C upstream
mv "upstream/pyenv-virtualenv-${PYENV_VIRTUALENV_SHA}" upstream/pyenv-virtualenv
# a new step after the pyenv bats step:
- name: Upstream pyenv-virtualenv bats suite in debian:trixie-slim (spec §12.3)
  if: ${{ !cancelled() }}
  run: |
    docker run --rm -v "$PWD:/w" -w /w debian:trixie-slim \
      bash parity/bats_venv_run.sh /w/target/debug /w/upstream/pyenv-virtualenv /w/upstream/bats /w/bats-venv-tap.txt
    python3 parity/bats_check.py --suite virtualenv bats-venv-tap.txt
```

The step should need no `apt-get`: every tool the runner and suite use is in Debian's essential
set (`runuser` is util-linux). Confirm this on the first CI run. It has its own step, so a failure in one suite doesn't hide the other's result. Keep its
`if: !cancelled()`, so it runs even when the core suite fails.

Spec §12 item 3 then gains a third bullet: "pyenv-virtualenv v1.4.0's bats suite on Linux,
against rpyenv's built-in commands through `pyenv-<cmd>` links, with an overlay that skips the
stub-was-called check for commands rpyenv answers in-process, and its own expected-failure
list". Step (2) of the milestone checklist doesn't apply to it: all 9 links exist from the
start.

## Part 2: Windows

### 2.1 Measured facts

Probe 1 checked how `std::fs` sees a junction. A Rust program, `rustc 1.96.1`, was run in
`C:\tmp\m4b_b`. It created the junctions with `mklink /J` and pointed `versions\foo` at
`versions\3.12.9\envs\foo`:

| Call on the junction | Result |
|---|---|
| `DirEntry::file_type().is_dir()` | **false** |
| `DirEntry::file_type().is_symlink()` | **true** (the comment at `install/txn.rs:192` already says so) |
| `entry.path().is_dir()` (follows) | true |
| `read_link` | `C:\…\versions\3.12.9\envs\foo` (plain absolute path, no `\??\`) |
| `canonicalize` | `\\?\C:\…\versions\3.12.9\envs\foo` |
| `remove_dir_all(junction)` | removes the link only; the env survives |
| `remove_dir(junction)` | removes the link only |
| `remove_file(junction)` | **fails: Access is denied (os error 5)** |
| `remove_dir_all(base)` where an env holds a junction to outside the root | base deleted, the outside target survives |
| a junction whose base was renamed away | `symlink_metadata` ok, `is_dir()`/`exists()` false; `remove_dir` removes it |

Probe 2 created a real Windows venv and ran today's rpyenv on it. It used the repo's
`target\debug` build of 08:37; `main`'s last commit is from 08:50. The build was copied into
`C:\tmp\m4b_b\bin` and run with `PYENV`, `PYENV_ROOT`, `PYENV_HOME` and `HOME` set to
scratch and pyenv-win dropped from `PATH`. The base `versions\3.13.12` was a scratch copy of
uv's CPython 3.13.12. The env was made with `python -m venv`, and `versions\foo` was a junction
to `versions\3.13.12\envs\foo`.

- **Layout.** The env holds `Include\`, `Lib\`, `Scripts\`, `.gitignore` and `pyvenv.cfg`.
  `Scripts\` holds `activate` (bash), `activate.bat`, `activate.fish`, `Activate.ps1`,
  `deactivate.bat`, `pip.exe`, `pip3.exe`, `pip3.13.exe`, `python.exe` and `pythonw.exe`.
  There is no `bin\` and no `deactivate.ps1`: `Activate.ps1` defines a `deactivate` function.
- **The launcher.** `Scripts\python.exe` is the venv launcher (241,152 bytes, against 91,648
  for the base `python.exe`). It is not a link.
- **`pyvenv.cfg`.**

  ```
  home = C:\tmp\m4b_b\root\versions\3.13.12
  include-system-site-packages = false
  version = 3.13.12
  executable = C:\tmp\m4b_b\root\versions\3.13.12\python.exe
  command = C:\tmp\m4b_b\root\versions\3.13.12\python.exe -m venv C:\tmp\m4b_b\root\versions\3.13.12\envs\foo
  ```

  On Windows, `home` is the base folder itself. On Linux it is `<base>/bin`. Spec §10's
  system-site-packages fallback must therefore search `home` the Windows way: the folder, then
  `Scripts`, as `lookup::win_hits` does.
- **`pyenv versions`** lists `3.13.12` and `foo`.
- **`pyenv rehash`** made shims `pip`, `pip3`, `pip3.13`, `python` and `pythonw`. The activation
  scripts were skipped (D-34, `shimset::is_activation`).
- **With `PYENV_VERSION=foo`:**
  - `pyenv prefix` gives `…\versions\foo`.
  - `which python` gives `…\versions\foo\Scripts\python.exe`.
  - `pyenv exec python` reports `sys.prefix` = `…\versions\foo` (the junction path, unresolved)
    and `sys.base_prefix` = `…\versions\3.13.12`.
  - The `python` shim gives the same prefix. `VIRTUAL_ENV` is unset, as expected.
  - `pip --version` through the shim reports `…\3.13.12\envs\foo\Lib\site-packages`, the
    resolved path.
- **`PYENV_VERSION=3.13.12/envs/foo`** is accepted today. `pyenv prefix` then prints mixed
  separators: `C:\…\versions\3.13.12/envs/foo`.
- **`pyenv uninstall -f foo`** (today) removed the junction only, and the env folder stayed.
  This contradicts spec §10, which says uninstalling by link name deletes both.
- **`pyenv uninstall -f 3.13.12`** (today) deleted the base and its env, but left the dangling
  junction `versions\foo` on disk. It is invisible to `pyenv versions`, because `subdirs` uses
  `path().is_dir()`, which follows the link. It would still block a later env or version named
  `foo`.

### 2.2 How the existing code sees a junctioned env

- **Listing.** `installed::subdirs` filters with `e.path().is_dir()`, which follows the link, so
  junctions are listed (probe 2). Code that used `file_type().is_dir()` would drop them (probe
  1). A grep of `crates/*/src` for `file_type()` without `is_symlink` finds three uses, none in
  the version listing:
  - `rehash.rs:509`, over `shims`;
  - `install/archive.rs:138` and `install/txn.rs:233`, installer internals.

  `txn.rs`'s `carry_over` moves `envs/` into a reinstalled base by rename. The junction's
  absolute target path is the same afterwards, so it keeps working; it dangles only during the
  swap.
- **Shims and rehash.** `shimset::shims_win` and `lookup::win_hits` read
  `<version>\{.,Scripts,bin}` through the junction. `rehash::snapshot` (PyenvWin) watches the
  same three folders for every `top_level` entry, junctions included, so new `Scripts\*.exe` in
  an env trigger the exit-check rehash. Windows never reads `<base>\envs\*` (`envs_of` runs only
  for the Pyenv flavor), so an env with no junction gets no shims and isn't listed.
- **Uninstall.** `uninstall_win::remove` treats `is_symlink()` (junctions included) as a link
  and calls `remove_dir`, which is correct for the link itself. It needs M4b logic for env and
  base (§2.1). The rename-to-`.del-` path is fine for a base whose envs hold junctions: deleting
  doesn't follow them (probe 1). Name checks will need care:
  - `is_version` (`^[a-zA-Z_0-9-.]+$`) rejects some env names a user might pick.
  - On X86, `with_arch` / `Check32Bit` appends `-win32` to every name, which would turn `foo`
    into `foo-win32` in `uninstall` and `shell`.
- **Creating junctions.** std has no API for it. Spawning `cmd /c mklink /J` brings back cmd
  quoting, which D-47 and D-89 work hard to avoid. Use `FSCTL_SET_REPARSE_POINT` through
  `windows-sys`, or a small crate (`junction`). Remove with `remove_dir`, never `remove_file`.
  Junctions need absolute targets. Moving `PYENV_ROOT` breaks them, as it breaks upstream's
  absolute symlinks (`ln -fs "${VIRTUALENV_PATH}" …`). They work only on local NTFS/ReFS
  volumes: FAT/exFAT has no reparse points, which needs an error message.

### 2.3 Activate and deactivate on Windows

Today's integration is set up as follows:

- **PowerShell.** `init_win.rs`'s `PWSH_FUNCTION` routes only `shell`, through `sh-shell`, and
  runs everything else directly.
- **Git Bash and fish.** They get `init::posix_function` / `fish_function`, which already route
  every command `command_names(ctx, Listing::ShOnly)` returns.
- **cmd.** It has no integration. `pyenv shell` prints the `set` line and exits 1 (D-89).

Proposal:

| Shell | `pyenv activate <env>` | `pyenv deactivate` |
|---|---|---|
| PowerShell | Add `activate` and `deactivate` to `PWSH_FUNCTION`'s routing, to `sh-activate` / `sh-deactivate` (same exit-code handling as `shell`). The code sets `$Env:PYENV_VERSION` (unless auto-activation, as upstream), `$Env:PYENV_ACTIVATE_SHELL`, `$Env:PYENV_VIRTUAL_ENV` and `$Env:VIRTUAL_ENV`, and wraps `prompt` the way `Activate.ps1` does unless `PYENV_VIRTUALENV_DISABLE_PROMPT` / `VIRTUAL_ENV_DISABLE_PROMPT` is set. All values go through `ps_literal`. | Removes those variables, restores the saved prompt, and restores `_OLD_VIRTUAL_PATH` / `_OLD_VIRTUAL_PYTHONHOME` when set. If a `deactivate` function exists (from the user's own `Activate.ps1`), it is removed, mirroring upstream's `unset -f deactivate`. |
| cmd | No integration: print `set "PYENV_VERSION=…"`, `set "PYENV_VIRTUAL_ENV=…"`, `set "VIRTUAL_ENV=…"` (and optionally `PROMPT`), then exit 1 with the D-89-style message. | Same, with `set "X="` lines. |
| Git Bash | Free once `sh-activate` / `sh-deactivate` are built-ins listed under `ShOnly`. | Same. |
| fish | Same as Git Bash. | Same. |

Two more points:

- **Don't prepend `Scripts` to `PATH`.** Upstream doesn't prepend `bin`, and the shims already
  resolve the env's `Scripts` (probe 2). This also keeps `deactivate.bat` and `activate.bat`
  off `PATH`.
- **Git Bash path form.** Python's own bash `activate` exports `VIRTUAL_ENV` through `cygpath`,
  in MSYS form (`/c/…`). That is the template `Lib\venv\scripts\common\activate`, lines 40–50.
  `init_win.rs` already writes the shims path in MSYS form (`msys()`).

### 2.4 What Windows users have today (conventions, not to copy blindly)

- **[pyenv-win-venv](https://github.com/pyenv-win/pyenv-win-venv)** (the pyenv-win org's own
  CLI, `pyenv-venv`):
  - Envs live outside `PYENV_ROOT`, in `~\.pyenv-win-venv\envs\<name>`, created with
    `python -m venv` after `pyenv shell <ver>`.
  - Commands: `install <ver> <name>`, `uninstall <name>`, `activate`, `deactivate`,
    `list envs|python`, `local <name>` (writes the env name to `.python-version`), `which`,
    `init [root]` (auto-activation from `.python-version`, from the PowerShell profile).
  - Activation runs the venv's own `Scripts\Activate.ps1`, or `cmd /k activate.bat` (a nested
    cmd). It doesn't set `PYENV_VERSION` and doesn't touch the shims.
- **[pyenv-virtualenv-windows](https://github.com/michaelpaulkorthals/pyenv-virtualenv-windows)**
  (third party):
  - Same layout as spec §10: a junction `versions\<name>-<ver>` to
    `versions\<ver>\envs\<name>-<ver>`.
  - Commands are `venv-new`, `venv-list`, `venv-del` and `venv-props`, plus `virtualenv`,
    `activate` and `deactivate` through a patched `pyenv.bat`.
  - Activation spawns a subshell. Project state is kept in `.python-version` plus a
    `.python-env` file.

These sources were read through their READMEs, and pyenv-win-venv's main script, on 2026-10-05.
Takeaways:

- Windows users expect activation to change the prompt.
- They expect `.python-version` holding an env name to select it. rpyenv gets that for free
  from the junction name.
- pyenv-win-venv users have envs outside `PYENV_ROOT` that rpyenv won't see.
- Nobody else uses nested shells for M4b's purposes. rpyenv shouldn't either; it has a real
  PowerShell function.

### 2.5 Decisions for the user

1. **`virtualenv-init` (auto-activation).** Implement it? It decides `init.bats` (9 tests) on
   Linux. On Windows it would be a PowerShell `prompt` hook. Or defer it to a later milestone.
2. **Windows `pyenv versions`.** pyenv-win prints names only. Options: keep that (the env shows
   as `foo`), add `<base>/envs/<name>` entries as Linux does, or mark junctions with
   `--> target`. This is a new allowlist row either way.
3. **`<base>/envs/<name>` as a version name on Windows.** It is accepted today, with
   mixed-separator output (§2.1). Accept it and normalize to `\`, accept only the junction name,
   or reject `/`?
4. **Uninstall semantics on Windows.** Spec §10 asks for confirmation before deleting a base's
   envs unless `-f`. pyenv-win's `uninstall` never prompts except with `-a`. Prompt only when
   envs exist? A base uninstall must also delete the junctions that point into it (today they
   dangle).
5. **Env names on Windows.** Which characters are allowed (`is_version`'s regex, reserved names
   such as `CON`, trailing dots)? And must the `-win32` suffix logic skip env names?
6. **PowerShell activate.** Use pyenv-virtualenv semantics (variables only, shims do the rest,
   §2.3), or dot-source the env's `Activate.ps1` as pyenv-win-venv does (adds `Scripts` to
   `PATH` and defines `deactivate`)? This research recommends the former.
7. **cmd activate.** Print `set` lines and exit 1, consistent with D-89? Or print
   `call <env>\Scripts\activate.bat`, which sets `VIRTUAL_ENV` and `PATH` but not
   `PYENV_VERSION`?
8. **Git Bash `VIRTUAL_ENV` form.** MSYS (`/c/…`, as venv's own bash `activate` does) or
   Windows (`C:\…`)?
9. **Junction creation.** Use a dependency (`junction` crate) or own `windows-sys` code? And
   what message on a volume without reparse-point support?
10. **pyenv-win-venv envs.** Ignore them, or teach `pyenv migrate` (M6) to junction them in?
11. **Windows test coverage.** pyenv-win has no virtualenv suite and pyenv-virtualenv's bats is
    bash-only. Windows coverage would be rpyenv's own e2e tests:
    - junction create and remove;
    - listing, rehash and uninstall of env and base;
    - `pwsh -NoProfile` activate/deactivate round trips;
    - cmd `set` output.

    Is that enough, or should `parity/diff.py` grow Windows cases against a fixture built by
    real `python -m venv` (needs a Python on the runner)?
