# rpyenv design

**Date:** 2026-09-27
**Status:** design approved in conversation; waiting for review of this
written spec.
**Related:** [README](../../README.md),
[Avoiding unnecessary console windows on Windows](../windows-lazy-console.md)

This spec is the shared design for the whole project. Each milestone
(see [Milestones](#milestones)) gets its own implementation plan.

## 1. Purpose

rpyenv is a drop-in replacement for pyenv (Linux) and pyenv-win (Windows),
written in Rust. The CLI and native shims are the same on both operating
systems.

The main problem it fixes is pyenv-win's batch-file shims:

- `cmd.exe` re-parses arguments and changes `^ % ! &`. Measured on the
  development machine: 5 arguments sent through pyenv-win's `python.bat` shim
  compared with the real `python.exe`, and **0 of 5** arrived intact:
  - `x^y` → `xy`
  - `100%` → `100`
  - `%USERNAME%` → the user name
  - `a&b` → `a`, and the text after `&` **ran as a separate command**
  - `say "hi"` never arrived

  Passing arguments you don't control through these shims can execute
  commands.
- Every Ctrl+C asks "Terminate batch job (Y/N)?".
- Shims can run `chcp`, which changes your console's code page.
- Anything that starts a process without a shell can't find `python.exe`,
  because the shim is `python.bat`.
- Each call goes through a slow chain of batch files and VBScript.

**Success means:**

- `python`, `pip`, and every installed console script work from cmd,
  PowerShell, Git Bash, bash, zsh, fish, and from callers that don't use a
  shell.
- Arguments arrive unchanged, exit codes pass through, and Ctrl+C behaves as
  if Python were run directly.
- Existing pyenv and pyenv-win installs, including versions, `version` files,
  and `.python-version` files, keep working without migration.

## 2. Decisions

| # | Topic | Decision |
|---|---|---|
| D1 | Scope | Full pyenv parity, delivered in milestones. The core comes first. Two later releases follow (§15): PyPy, Anaconda, Miniconda, and Miniforge (M7), and automatic handling of missing Linux build dependencies (M8). |
| D2 | Compatibility | Drop-in. Same `PYENV_ROOT` layout and files; the CLI answers to `pyenv`. |
| D3 | Python builds | CPython in M1–M6; PyPy and conda in M7. CPython on Windows: official python.org packages, extracted directly (no installer run, no registry writes). Linux: compiled from source. |
| D4 | Plugins | `pyenv foo` runs `pyenv-foo` (git-style). Virtualenv support is built in. No bash hooks. |
| D5 | Shim architecture | Small shim executables plus a separate CLI, sharing one core library. Shims resolve the version themselves, in-process. |
| D6 | Console windows | See [windows-lazy-console.md](../windows-lazy-console.md). Detecting console reads that come without prior output is deferred. |
| D7 | Shell integration | A `pyenv init` profile function for bash, zsh, fish, Git Bash, and PowerShell. No `.ps1` or `.cmd` for the CLI. Everywhere else, detect the shell and print the command. |
| D8 | Auto-rehash | A stored-state check at shim exit is always on. A live watcher is opt-in via `RPYENV_LIVE_REHASH=1`; the MSI asks whether to enable it. |
| D9 | Parity testing | Own tests, adapted upstream test suites, and differential testing against real pyenv and pyenv-win. |
| D10 | Distribution | Windows: one MSI that installs either per-user (no admin rights) or for all users (UAC elevation raised automatically); see §9.4. Installed Python versions stay per-user in both modes; shared, admin-managed versions come later (§15.3). Linux: tarball and install script. |
| D11 | Hosting | Public GitHub repo `jmperez0/rpyenv`, MIT license, GitHub Actions CI (free for public repos). |

## 3. Architecture

A Cargo workspace with one library and three binaries:

| Unit | Purpose | Depends on |
|---|---|---|
| `rpyenv-core` (lib) | Locate `PYENV_ROOT`, resolve versions, read and write version files, compute and apply rehash, find executables | std, and platform crates only |
| `pyenv` (bin) | Every CLI command, installer, virtualenvs, shell-integration output, plugin dispatch | core, HTTP/TLS, archive, CLI crates |
| `pyenv-shim` (bin) | Console shim: resolve and launch the target; Windows console modes; post-exit rehash check | core only |
| `pyenv-shimw` (bin) | GUI-subsystem shim for GUI targets (`pythonw`, gui-scripts) | core only |

The shim binaries must not link networking, TLS, or archive code. They run
on every `python` call, so a smaller binary starts faster. CI checks this by
reading each shim's dependency tree.

### 3.1 Directory layout (unchanged from upstream)

| Path | Linux default | Windows default |
|---|---|---|
| `PYENV_ROOT` | `~/.pyenv` | `%USERPROFILE%\.pyenv\pyenv-win` |
| CLI | `bin/pyenv` | `bin\pyenv.exe` |
| Shims | `shims/` (hardlinks to the template) | `shims\` (hardlinks; a `.cmd` forwarder only for names in `RPYENV_BATCH_FORWARD`) |
| Versions | `versions/<name>/bin` | `versions\<name>\` and `versions\<name>\Scripts` |
| Global version | `version` | `version` |
| Download cache | `cache/` | `install_cache\` (pyenv-win's name) |
| Default packages | `default-packages` | same (the pyenv-default-packages plugin's file) |
| Rehash state | `shims/.rehash-state` and `shims/.rehash.lock` | same |
| Shim template | `shims/.template/` (a per-user copy of `pyenv-shim`) | `shims\.template\` (per-user copies of the shim binaries; `PATH` never searches subfolders) |

On Windows, `PYENV_ROOT` is read from `PYENV_ROOT`, then pyenv-win's `PYENV`
and `PYENV_HOME`, then the default.

## 4. Version resolution

Upstream order; the first match wins:

1. `PYENV_VERSION`
2. `.python-version` in the current directory or any parent
3. `$PYENV_ROOT/version`
4. `system`

The details follow upstream: several versions in one file or variable
searched in order, `system`, comments, prefix matching, and error text. The
parity suites (§12) pin the exact behavior instead of this spec restating it.
Where pyenv and pyenv-win differ, each OS follows its own upstream, and the
difference is listed in the differential-test allowlist.

**Parity policy (decided 2026-09-28): match the contract, not the bugs.**

- **Match** each OS's upstream in:
  - output formats, messages, output streams, and exit codes
  - version-file formats and resolution rules

  Scripts rely on these. Example: pyenv-win writes "no local version
  configured" to stdout and exits 0.
- **Don't reproduce defects:**
  - crashes (pyenv-win fails when two installed versions tie numerically)
  - lost information (pyenv-win's `pip*` shims return rehash's exit code
    instead of pip's)
  - file corruption (pyenv's `global -f X` writes `-f` into the file)
  - mis-parsing (pyenv's rehash splits names containing spaces into several
    shims)
  - typos in help text
- **Commands pyenv-win lacks** (`root`, `prefix`, `version-file`,
  `version-origin`) follow upstream pyenv on Windows.
- **Every intentional difference** is listed in the differential-test
  allowlist with its reason.

The verbatim references are
[docs/parity/pyenv-m1-reference.md](../parity/pyenv-m1-reference.md)
(pyenv 2.8.6, commit `ab74141`) and
[docs/parity/pyenv-win-m1-reference.md](../parity/pyenv-win-m1-reference.md)
(pyenv-win 3.1.1 plus 232 commits, commit `856ed5a`).

## 5. Shims

### 5.1 Resolving a command

1. Take the command name from the shim's own file name (`python`, `black`, …).
2. Resolve the version list (§4).
3. Search each version for the command:
   - Linux: `bin/`
   - Windows: the version folder, then `Scripts\`
4. If no version has it, exit 127 with upstream's message, listing the
   versions that do have the command (same output as `pyenv whence`).

The result is a **launch plan**, not just a path: the executable, plus any
environment changes the distribution needs (extra `PATH` entries). In M1 the
environment changes are always empty. Conda on Windows (M7) needs them, and
adding them later must not change the shim's interface.

### 5.2 Launching on Linux

- **Most commands:** `execv` the target. The PID stays the same, and signals
  and exit status are native.
- **`pip*` and `python* -m pip`:** spawn the target and wait, so the rehash
  check (§8) finishes **before** the caller gets control back. A detached
  watcher can't do this: in `pip install black && black`, the shell starts
  `black` as soon as pip exits, possibly before an asynchronous rehash has
  created its shim. While waiting, the shim:
  - ignores `SIGINT` and `SIGQUIT` (the child gets them from the terminal),
  - passes `SIGTERM` and `SIGHUP` on to the child,
  - if the child dies from a signal, raises the same signal on itself.

### 5.3 Launching on Windows

- **Arguments are passed through byte-for-byte.** The shim takes its raw
  command line (`GetCommandLineW`), removes the first token using the same
  rules as the C runtime, and passes the rest to `CreateProcessW` unchanged.
  It never splits and re-quotes arguments, because that round trip can change
  them.
- **Process lifetime:** the child runs in a Job Object with
  `KILL_ON_JOB_CLOSE`, so killing the shim kills the child. The job also has
  `SILENT_BREAKAWAY_OK`, so processes the child starts aren't in the job and
  behave as if Python were run directly.
- **Ctrl+C:** the shim registers a handler that returns `TRUE`. It does
  **not** call `SetConsoleCtrlHandler(NULL, TRUE)`: child processes inherit
  that setting, which would make Python ignore Ctrl+C.
- **Exit code:** the child's `DWORD` exit code is returned unchanged,
  including status codes like `0xC000013A`.
- **Console:** see [windows-lazy-console.md](../windows-lazy-console.md) for
  the shim type rule and the modes (INHERIT, NO-WINDOW, MIRROR, EAGER, LAZY).
  Milestone M1 implements INHERIT, NO-WINDOW, MIRROR, and the GUI shim. EAGER
  and LAZY come in M5.

  The M1 shims ship **without** the `consoleAllocationPolicy=detached`
  manifest entry, so Windows gives them a console at startup, just as it does
  `python.exe`. Without the entry, the shim can only lack a console when the
  caller passed `DETACHED_PROCESS`, and NO-WINDOW and MIRROR cover that case.
  The entry is added in M5, together with the EAGER and LAZY modes that handle
  a shim launched with no console and nothing redirected.
- **Batch targets** (`.bat`/`.cmd` in `Scripts`) get the same console exe
  shim as any other command:
  - The shim splits its command-line tail into arguments using the C
    runtime's rules, as any `.exe` would.
  - It launches the target through `cmd.exe` using the batch-file escaping in
    Rust's `std::process::Command` (hardened after CVE-2024-24576, known as
    "BatBadBut").
  - The shim adds no extra round of cmd parsing, and the target is found by
    callers that don't use a shell.
  - The target runs in a child `cmd`, so it can't change the caller's
    environment. The same is true on Linux, where scripts like `activate` must
    be sourced.
- **Opt-in forwarders** are for batch tools that must change the caller's
  environment. They're listed in `RPYENV_BATCH_FORWARD` (names separated by
  `;`), and each gets a `.cmd` forwarder instead. Requirements:
  - It resolves the target with `pyenv which`, using the absolute path of
    `pyenv.exe` written in at rehash time.
  - It runs the target in the caller's `cmd` process, without `call` and
    without `setlocal`, so environment changes persist.
  - If resolution fails, it prints pyenv's error and sets errorlevel 127.
  - **Known limitation:** arguments typed without quotes lose one level of `^`
    escaping. These are the only batch files rpyenv generates.
- **Probe results for these choices** (development machine; each argument
  checked byte-for-byte):
  - Direct launch with Rust's escaping: 11 of 11 tricky arguments arrived
    intact (`& ^ % %VAR% " ! | < >`, spaces, empty, non-ASCII).
  - A plain `%*` forwarder: also 11 of 11 when the caller quotes arguments.
    Typed at the prompt as `a^^^&b`, it delivered `a` where running the target
    directly delivers `a&b`.
  - A `call` forwarder: 8 of 11. Carets were doubled, `%` was dropped, and
    `%PATH%` was expanded.

## 6. CLI commands

Upstream's command set:

| Group | Commands |
|---|---|
| Select a version | `global`, `local`, `shell`, `version`, `version-name`, `version-origin`, `version-file`, `version-file-read`, `version-file-write` |
| Inspect | `versions`, `which`, `whence`, `prefix`, `root`, `latest`, `shims`, `commands`, `help`, `--version` |
| Run | `exec`, `rehash` |
| Install | `install` (including `--list`), `uninstall`, `update` (pyenv-win: refresh the version list) |
| Shell | `init`, `completions`, `sh-shell`, `sh-activate`, `sh-deactivate` |
| Setup (rpyenv-only, Windows) | `setup` (prepare one user's environment), `migrate` / `migrate --restore` (take over pyenv-win or undo it); see §9.4 |
| Virtualenvs | `virtualenv`, `virtualenvs`, `virtualenv-delete`, `virtualenv-prefix`, `activate`, `deactivate` |

Plugin dispatch: for an unknown command `foo`, run `pyenv-foo` from
`$PYENV_ROOT/plugins/*/bin` or from `PATH`, with `PYENV_ROOT` exported. On
Linux, existing bash plugins work unchanged. On Windows, a plugin must be an
executable.

## 7. Shell integration

`pyenv init - <shell>` prints code for the shell profile. The code:

- adds `shims` to `PATH`,
- sets `PYENV_SHELL`,
- defines a `pyenv` function,
- loads completions.

The function passes every command straight to the binary, except `shell`,
`activate`, and `deactivate`. For those, it runs `pyenv sh-<cmd> …` and
evaluates the printed code in the current shell.

| Shell | Profile line |
|---|---|
| bash, zsh, Git Bash | `eval "$(pyenv init - bash)"` (or `zsh`). Git Bash gets `/c/...` paths when `MSYSTEM` is set. |
| fish | `pyenv init - fish \| source` |
| PowerShell 5.1 and 7 | `pyenv init - pwsh \| Invoke-Expression` |

**No `pyenv.ps1` or `pyenv.cmd` for the CLI.** This was tested on the
development machine, in PowerShell 5.1 and 7.6:

- A `.ps1` wins over an `.exe` in the same folder.
- When the execution policy blocks the `.ps1` (`Restricted`, or
  `RemoteSigned` with a file marked as downloaded), `pyenv` fails with
  `UnauthorizedAccess` and does **not** fall back to the `.exe`.

A profile function fails safely instead: when scripts are blocked, the
profile doesn't load, and `pyenv` resolves to `pyenv.exe`.

**Without integration** (always in cmd; PowerShell when the profile is
blocked or not set up), `shell`, `activate`, and `deactivate`:

1. pick the shell from `PYENV_SHELL`, or else from the parent process's image
   name (`cmd.exe`, `powershell.exe`/`pwsh.exe`, `bash`/`zsh`/`fish`). The
   parent is trusted only if it was created before `pyenv` itself, which
   guards against a reused process ID.
2. print that shell's exact command on stdout, and a note on stderr saying
   nothing was applied (plus how to enable integration, except for cmd).
3. exit with 1. If the shell is unknown, print the command for all supported
   shells.

Parent-process detection was checked on the development machine with a probe
from Git Bash (`bash.exe`), PowerShell (`pwsh.exe`), and cmd (`cmd.exe`).

`pyenv init` with no arguments uses the same detection to print setup
instructions, as upstream does.

## 8. Rehash

**What gets a shim:**

- **Both OSes, never shimmed:** activation scripts, meaning any file whose
  name without extension is `activate` or `deactivate` (`activate`,
  `activate.csh`, `activate.fish`, `activate.nu`, `activate.bat`, …). They
  have to run inside the current shell, which a shim can't do; `pyenv
  activate` covers them.
  - Upstream instead writes special "source shims" for these names (the
    `rehash/source.bash` hook).
  - On Linux this matters for conda: its `bin/activate` is executable and
    owned by the `conda` package, so without this rule it would get a normal
    shim that does nothing.
- Linux: every other executable in `versions/*/bin`, as a hardlink to
  `shims/.template/pyenv-shim`, a per-user copy of `pyenv-shim` (a copy when
  a hardlink isn't possible). With symlinks to the installed binary, a
  program that writes into one shim would overwrite `pyenv-shim` itself and
  break every shim beyond what `pyenv rehash` can repair; with the copy, only
  the copy is damaged, and rehash replaces it when its bytes differ from the
  installed binary. Every shim then fails until `pyenv rehash`, because the
  shims share the copy and so no shim can run its own check; `pyenv rehash`
  restores them from the installed binary, which stays intact. Linux's `fs.protected_hardlinks` also forbids a hardlink
  to a file the user doesn't own, as Windows does below.
- Windows:
  - `.exe` → a hardlink to the copy of `pyenv-shim.exe` or `pyenv-shimw.exe`
    kept in `shims\.template\`, whichever matches the target's PE `Subsystem`.
    If the type differs between versions, the console shim wins. If a hardlink
    isn't possible, a copy is made.
  - Shims link to a per-user copy, not to the installed binary, because an
    ordinary user can't hardlink to a file they can only read. Tested on the
    development machine: linking to `System32\hostname.exe` (Users:
    ReadAndExecute) failed with "Access is denied", and linking to a file the
    user owns worked. `Program Files` grants users the same rights, so an
    all-users install would fail without the copy. The copy is used in both
    install modes, so there's only one code path.
  - `.bat`/`.cmd` → the console exe shim, or a `.cmd` forwarder for names in
    `RPYENV_BATCH_FORWARD` (§5.3). Activation scripts are excluded as
    described above.
  - Anything else gets no shim.
- Migration from pyenv-win: rehash removes the old `.bat` shims and the
  extensionless `sh` shims. In Git Bash, the extensionless scripts would be
  found before the `.exe`.

**When it runs:**

1. **Explicitly:** `pyenv rehash`, and `install`, `uninstall`, and
   `virtualenv` run it when they finish.
2. **The stored-state check** (always on):
   - `shims/.rehash-state` records the modification times of `versions/` and
     of each version's scripts folder.
   - When a shim's child exits, the shim compares them and rehashes if
     anything changed. On Windows this happens after every shim; on Linux,
     after the pip commands (§5.2).
   - This works because adding or removing a file changes the folder's
     modification time, and editing a file doesn't. That was checked on NTFS on
     the development machine.
3. **Live watcher** (opt-in, `RPYENV_LIVE_REHASH=1`):
   - Covers long-running processes that install packages, such as Jupyter's
     `%pip install`.
   - Windows: a thread in the shim, started only if the child is still running
     after about 1 s. It uses `ReadDirectoryChangesW`, with directory handles
     opened with share-delete so they never block `pyenv uninstall`.
   - Linux: the shim forks, the child forks the watcher and exits
     immediately, and the shim reaps that child and then runs `execv`. The
     watcher is handed to init (or the nearest subreaper process) and waits
     about 1 s via `pidfd_open` on the program's PID. If the program is still
     running, it watches with inotify until the program exits, then runs the
     final stored-state check.
     - **Why two forks:** with a single fork, the watcher would be a child of
       the program once `execv` runs, and the program could see it:
       `os.wait()` in a program with no children of its own would block
       instead of returning "no child processes", `psutil` would list it, and
       "kill all my children" cleanup code would kill it.
     - **Safeguards:** the watcher moves to a new session (`setsid`), so
       terminal signals don't reach it. It closes every inherited file
       descriptor and reopens stdin, stdout, and stderr on `/dev/null`, so it
       never holds a pipe open.
     - **Skipped when the shim is PID 1** (a container without an init
       process): orphaned processes are handed back to PID 1, which would be
       the program itself.
   - Both: once changes stop for about 0.5 s, one rehash runs. If the watcher
     can't start (for example, the inotify limit is reached), it is skipped
     silently; the exit check still runs.
   - The MSI shows an "Enable live rehash" checkbox, which sets the variable
     for the user. On Linux you set it in your shell profile.

**Safety:**

- A lock file makes sure only one rehash runs at a time. A shim that can't
  get the lock skips; the next check will catch up.
- Only differences are applied: missing shims are added, stale ones removed.
- A stale shim that is running can't be deleted on Windows, so it is renamed
  to `*.old` and deleted by a later rehash.
- After an rpyenv upgrade, rehash notices that the installed shim binaries no
  longer match the copies in `shims/.template/` (`shims\.template\` on
  Windows) by their bytes. It refreshes the copies and recreates the
  hardlinks. A shim's own exit check never refreshes an existing template; on
  Linux it creates a missing one from the running shim (rpyenv 0.1 roots).
  Besides `pyenv rehash`, what refreshes it is, on Linux, `pyenv exec` running
  a pip command (any other `pyenv exec` replaces its own process and runs no
  check); on Windows, any `pyenv exec` that runs a rehash. Until then, shims
  keep running the previous copy.

## 9. Installer

### 9.1 Windows

- **Source:** official python.org packages, extracted into
  `versions\.tmp-<ver>` and then renamed into place. No installer runs and
  nothing is written to the registry.
- The python.org package index used by the Python Install Manager and the
  NuGet packages are both candidates. Which source to use for which version
  range, and whether it includes Tcl/Tk (tkinter), must be checked in the
  first installer task. For versions neither source covers, the fallback is
  extracting the MSI payload from the `.exe` installer, as pyenv-win does
  today.
- Architectures: amd64, arm64, and win32, matching the host by default.
- `pyenv install --list` reads a cached version list. `pyenv update`
  refreshes it from the rpyenv-published catalog (§15.1, resolution 1). The
  scheduled workflow that publishes the catalog is built in M2, the first
  milestone that needs it.

### 9.2 Linux

- **Catalog:** CPython definitions from upstream pyenv's `python-build`
  (MIT-licensed), which provide versions, URLs, and SHA-256 hashes. This also
  gives `install --list` the same output as upstream.
- **Build:** download → check the hash → `configure && make && make install`
  into a temporary prefix → rename into place.
- Respects upstream's variables: `PYTHON_CONFIGURE_OPTS`, `PYTHON_CFLAGS`,
  `MAKE_OPTS`, `PYTHON_MAKE_OPTS`, `PYTHON_BUILD_MIRROR_URL`.
- **Pre-flight check:** a compiler, `make`, and key headers (OpenSSL, zlib,
  libffi, readline or libedit, sqlite, bz2, lzma). When something is missing,
  print the package names to install for the detected distribution. The check
  returns structured results (dependency, required or optional, what exactly
  is missing), and M8 builds on them (§15.2).
- **On failure:** keep the build directory and log, and print the last lines
  and the path, as upstream does.

### 9.3 Both

- **Atomic:** a failure or Ctrl+C never leaves a partial `versions/<ver>`.
- **Verified:** every download is checked against a published SHA-256 before
  it is used.
- **`:latest` (M2).** `pyenv install 3.12:latest` installs the newest
  matching release. Candidates exclude `-dev`, `-src`, pre-releases
  (`a`/`b`/`rc`), and free-threaded builds (`t` suffix), and are sorted by
  version number, the same filters as upstream's `install/latest.bash` hook.
  How other version prefixes are handled follows upstream's `pyenv-install`,
  pinned by the parity tests.
- **Default packages** (the pyenv-default-packages plugin's behavior, built
  in):
  - After a successful `pyenv install` (M2) or `pyenv virtualenv` (M4), if
    `$PYENV_ROOT/default-packages` exists, rpyenv runs
    `pip install -r $PYENV_ROOT/default-packages` in the new version.
  - If that fails, it prints an error, but the install itself still counts as
    successful, as with the plugin.
  - It reads the same file as the plugin, so existing plugin users need no
    changes.

### 9.4 Installing rpyenv itself on Windows (MSI)

**One MSI with both modes:** a Windows Installer package that can install
either per-user or per-machine (WiX `Scope="perUserOrMachine"`). The dialog
offers:

- **Just for me:** no admin rights needed, no UAC prompt.
- **For all users:** the button shows the UAC shield, and Windows Installer
  raises the UAC prompt itself when installation starts. You don't have to
  relaunch the installer elevated.

**Silent installs:**

- `msiexec /i rpyenv.msi /qn` installs per-user.
- For all users, add `ALLUSERS=1 MSIINSTALLPERUSER=""` and start it from an
  elevated prompt (for example `sudo msiexec …`). A fully silent install is
  expected to have no way to show a UAC prompt; verify this in M6.

**Where things go:**

| | Just for me | For all users |
|---|---|---|
| Binaries | `%LOCALAPPDATA%\Programs\rpyenv\bin` | `%ProgramFiles%\rpyenv\bin` |
| `bin` on `PATH` | User `PATH` | Machine `PATH` |
| `shims` on `PATH` | User `PATH`, at the front | Each user's `PATH`, added by `pyenv setup` |
| Python versions and shims | Your `PYENV_ROOT` | Each user's own `PYENV_ROOT` (shared versions: §15.3) |
| `RPYENV_LIVE_REHASH` checkbox | User environment | Machine environment |

**`pyenv setup`** (rpyenv-only) prepares one user's environment: it creates
`PYENV_ROOT`, puts `shims` at the front of the user `PATH`, and runs rehash.
It runs:

- for a per-user install, at the end of installation;
- for an all-users install, once per user at their next logon (through
  Windows' Active Setup mechanism), and on any `pyenv` command that finds it
  hasn't run yet for this user.

**`pyenv migrate`** (rpyenv-only) takes over an existing pyenv-win install. It
is needed because pyenv-win's `bin\pyenv.ps1` would otherwise win over
`pyenv.exe` in PowerShell (§7 test). It:

- removes pyenv-win's `bin` from the user `PATH`,
- moves `bin\pyenv.ps1`, `bin\pyenv.bat`, and `bin\pyenv` into a backup
  folder inside `PYENV_ROOT`,
- runs rehash, which replaces the `.bat` and extensionless shims.

`pyenv migrate --restore` undoes it. The per-user MSI offers to run
`migrate` when it finds pyenv-win. For an all-users install, `pyenv setup`
prints a hint when it finds pyenv-win for that user.

**Uninstall** removes the binaries and the `PATH` entries it added. It keeps
`PYENV_ROOT`: installed Python versions are user data. If a pyenv-win backup
exists, uninstall offers `migrate --restore`.

**`PATH` order:** Windows puts machine `PATH` entries before user entries.
Any `python.exe` in the machine `PATH` (for example from an all-users
python.org install) therefore wins over the per-user shims. `pyenv setup`
warns when it finds one. pyenv-win has the same limitation.

## 10. Virtualenvs

- **Layout:** as in pyenv-virtualenv: `versions/<base>/envs/<name>`, with
  `versions/<name>` linking to it. The link is a symlink on Linux and a
  directory junction on Windows, since junctions need no special privileges.
- **Creation:** `python -m venv` from the base version.
- **`activate` / `deactivate`:** through the shell function. They set
  `PYENV_VERSION`, `PYENV_VIRTUAL_ENV`, and `VIRTUAL_ENV`, the same variables
  as pyenv-virtualenv.
- A virtualenv's scripts get shims like any other version.
- **Command fallbacks,** built in from pyenv-virtualenv's `which` hooks. When
  a command isn't in the selected virtualenv:
  - **System site packages:** if `pyvenv.cfg` says
    `include-system-site-packages = true`, the command is looked up in the
    base Python named by `pyvenv.cfg`'s `home` key.
  - **`python*-config`:** always looked up in the base Python.
  - **Conda environments:** `conda` is looked up in the conda installation
    that owns the environment.
  - **Older virtualenvs** that have no `pyvenv.cfg` are recognized by their
    `orig-prefix.txt` and `no-global-site-packages.txt` files, as upstream
    does.
- **Uninstall:**
  - Uninstalling a base version also deletes its virtualenvs
    (`versions/<base>/envs/*`), as pyenv-virtualenv's `uninstall` hook does.
    Each deletion is confirmed unless `-f` is given.
  - Uninstalling a virtualenv by its link name deletes both the environment
    and the link.

## 11. Error handling

- **Messages and exit codes match upstream,** prefixed `pyenv: `. Examples:
  - a version that isn't installed exits 1, naming the file that set it
  - a command not found exits 127 and lists the versions that have it
- **Where errors go:**
  - The GUI shim shows a message box.
  - LAZY mode opens the console and keeps it open on error.
  - NO-WINDOW and MIRROR modes write to the caller's stderr and, if set, to
    `RPYENV_DEBUG_LOG`.
- **Encoding (Windows):** text rpyenv's stdout and stderr write when they go to
  a pipe, a file or NUL is in
  the console's output code page (with no console, the ANSI code page), as
  cmd.exe and pyenv-win write theirs. When a character doesn't fit that code
  page, the whole text is written as UTF-8 instead of turning the character
  into `?` (allowlist D-48); a console set to code page 65001 gets UTF-8
  always. Text written to a console is Unicode. `RPYENV_DEBUG_LOG` is always
  UTF-8. Linux writes UTF-8.
- **Debugging:** `PYENV_DEBUG=1` (upstream's variable) prints where the
  version came from and which executable was chosen.
- **Downloads:** retry with backoff, then report the URL and suggest
  `PYTHON_BUILD_MIRROR_URL`. A hash mismatch is fatal and deletes the file.

## 12. Testing

1. **Unit tests** (pure, table-driven):
   - version resolution and file parsing
   - shell detection
   - the console-mode choice and the output-trigger scanner
   - the rehash comparison of wanted vs existing shims
   - command-line tail extraction
2. **Integration tests with a fake `PYENV_ROOT`.** The "versions" contain an
   `argv-echo` test program that prints its arguments, working directory, and
   selected environment variables, then exits with a requested code. Tests
   check:
   - arguments arrive byte-for-byte (`^ % ! & "`, empty arguments, Unicode)
   - exit codes, stdin/stdout piping, Ctrl+C
   - killing the shim kills the child, checked while the child would still be
     running
3. **Upstream suites, adapted:**
   - pyenv-win's pytest suite on Windows. A grep for `def test_` found 94 test
     functions in 17 test files, one of them currently empty; parametrized
     cases add more. It runs pyenv through one fixture path in `conftest.py`.
   - a subset of upstream pyenv's bats suite, with small wrappers for its
     `pyenv-<cmd>` calls, on Linux
   - expected failures (batch internals, cmd `shell` semantics) are listed
     with reasons
4. **Differential testing.** CI installs real pyenv (Linux) and pyenv-win
   (Windows), runs the same commands against both tools on shared fixture
   roots, and compares stdout, stderr, and exit codes. Intended differences
   are listed in a checked-in allowlist, each with a reason.
5. **Installer tests, in three tiers.** Nothing is compiled on Windows: the
   installer only downloads and extracts prebuilt python.org packages (§9.1).

   | Tier | Runs | Covers | Needs the network? |
   |---|---|---|---|
   | 1. Fake download server | Every PR, both OSes | Installer logic against a local test server that serves tiny fixture archives and catalogs: success, hash mismatch, network error then retry, interruption leaving no partial version, Linux build-script generation | No |
   | 2. Real Windows install | Every PR | One pinned version, end to end. Downloads are cached in CI and keyed by SHA-256, so after the first run the network isn't used. | Only when the cache is empty |
   | 3. Real Linux source build | Nightly, plus PRs that change Linux installer code (path filter) | PRs: one pinned version, built with ccache and without PGO/LTO optimizations. Nightly: every supported version, and the full Windows version list too. | Yes |

   Tier 1 makes PR results depend only on the code. Tiers 2 and 3 confirm
   that the real download sources still work. The PR build time for tier 3 is
   estimated at a few minutes; the first M2 CI run will measure it.
6. **Performance:** the shim overhead benchmark (`python -c pass` through the
   shim vs directly, timed with hyperfine) is posted in the CI summary.
7. **Manual checks:** the desktop checklist is in the lazy-console doc.

**CI matrix:** `windows-2025`, `windows-2022`, `ubuntu-latest`,
`ubuntu-24.04-arm`.

## 13. Configuration

| Variable | Source | Meaning |
|---|---|---|
| `PYENV_ROOT` (`PYENV`, `PYENV_HOME` on Windows) | upstream | Root directory |
| `PYENV_VERSION` | upstream | Version override for the current shell |
| `PYENV_SHELL` | upstream | Set by `pyenv init` |
| `PYENV_DEBUG` | upstream | Trace output |
| `PYTHON_BUILD_MIRROR_URL`, `PYTHON_CONFIGURE_OPTS`, `PYTHON_CFLAGS`, `MAKE_OPTS`, `PYTHON_MAKE_OPTS` | upstream | Installer options |
| `RPYENV_CONSOLE` | rpyenv | `lazy` or `eager` (Windows, when the shim has no console) |
| `RPYENV_LIVE_REHASH` | rpyenv | `1` enables the live watcher |
| `RPYENV_DEBUG_LOG` | rpyenv | File for diagnostics from shims and `pyenv exec` when there is no console |
| `RPYENV_BATCH_FORWARD` | rpyenv | `;`-separated batch-target names that get a `.cmd` forwarder instead of an exe shim (Windows) |
| `RPYENV_CATALOG_URL` | rpyenv | Alternative location (a mirror) for the rpyenv-published Windows catalog |
| `RPYENV_BUILD_DEPS` | rpyenv (M8) | `system`, `install`, or `build`: how `pyenv install` resolves missing Linux build dependencies when there is no terminal to ask |

`RPYENV_FORWARD_CP`, `RPYENV_FORWARD_PYENV`, `RPYENV_FORWARD_TARGET` and `RPYENV_FORWARD_DIR` are internal helpers that `.cmd` forwarders set and clear while they run; they are not user settings (allowlist D-47).

rpyenv-only variables use the `RPYENV_` prefix so they can't collide with a
future upstream `PYENV_` variable.

## 14. Milestones

Each milestone gets its own implementation plan.

1. **M1 Core.**
   - Workspace; core resolution
   - both shims (INHERIT, NO-WINDOW, MIRROR), rehash with the stored-state
     check
   - `exec`, `which`, `whence`, `versions`, `global`, `local`, `version*`,
     `root`, `prefix`, `shims`, `commands`, `help`
   - unit and integration tests; parity suites wired into CI
   - Outcome: usable with Pythons already installed by pyenv or pyenv-win.
2. **M2 Installer.** `install`, `uninstall`, `update`, `latest`, and
   `install --list`, for Windows packages and Linux source builds.
3. **M3 Shell integration.** `init`, `sh-*`, `shell`, completions, and the
   detect-and-print fallback.
4. **M4 Virtualenvs and plugins.**
5. **M5 Windows console and live rehash.** EAGER and LAZY modes, and the live
   watcher on both OSes.
6. **M6 Release.** Windows MSI with per-user and all-users modes (§9.4),
   `pyenv setup`, and `pyenv migrate`; Linux tarball and install script;
   release workflow.

7. **M7 PyPy, Anaconda, Miniconda, Miniforge** (a later release, see §15.1).
8. **M8 Missing Linux build dependencies** (a later release, see §15.2).
9. **M9 Shared admin-managed versions** (only if needed, see §15.3).

Differential testing starts in M1 for the commands M1 delivers and grows with
each milestone.

## 15. Later releases

### 15.1 PyPy, Anaconda, Miniconda, Miniforge (M7)

**Scope:**

- **PyPy** only where PyPy publishes official prebuilt binaries for the host
  OS and architecture. No PyPy source builds.
- **Anaconda, Miniconda, and Miniforge** via their official installers.

**Choices made now so M7 doesn't force a redesign:**

1. **Version names follow upstream pyenv:** `pypy3.10-7.3.17`,
   `anaconda3-<release>`, `miniconda3-<release>`. Conda environments appear as
   `<install>/envs/<name>`, as upstream shows them. Version resolution (§4)
   already accepts any directory name.
2. **Launch plans** (§5.1). Conda Python on Windows expects its activation
   folders on `PATH`: the install root, `Library\mingw-w64\bin`,
   `Library\usr\bin`, `Library\bin`, `Scripts`, and `bin`. Without them, some
   packages fail to load their DLLs. The launch plan carries these entries.
   The shim applies them to the child's environment only, never to the
   caller's.
3. **Each distribution has its own layout,** detected per version from marker
   files: a `conda-meta/` folder means conda, a `pypy*` executable means PyPy.
   Layout decides where to look for executables and which get shims.
4. **Install flow differs:**
   - **PyPy** archives can be moved after extraction, so they use the same
     extract-to-temporary-folder-and-rename flow as CPython (§9.3).
   - **Conda** installs **can't be moved after installing**: the install path
     is written into the installed files. So conda installs straight into the
     final `versions/<name>` folder, writes a completion marker last, and
     deletes the folder on failure. A folder without the marker counts as not
     installed.
5. **Catalog:** on Linux, upstream `python-build` already has PyPy, Anaconda,
   Miniconda, and Miniforge definitions with hashes. On Windows, versions come
   from the rpyenv-published catalog (resolution 1 below).
6. **`install --list` shows only what can be installed on this machine:**
   combinations of distribution, version, OS, and architecture that have an
   official binary.

**Resolved M7 questions** (2026-09-27; sources checked that day):

1. **Windows download lists and hashes: an rpyenv-published catalog.**
   - **The upstream sources differ:**
     - PyPy's `versions.json` lists files and platforms but no hashes. SHA-256
       values appear only on the `checksums.html` web page.
     - Miniconda and Anaconda publish SHA-256 values as a column in their
       download listing (a web page).
     - Miniforge publishes a `.sha256` file next to each versioned installer
       on GitHub.
   - **How the catalog works:** a scheduled CI workflow in the rpyenv repo
     reads these sources and publishes one JSON catalog. `pyenv update`
     downloads it, and `install --list` reads the cached copy.
     `RPYENV_CATALOG_URL` points to a mirror instead.
   - **Where each hash comes from:** from the source's own published checksum.
     The catalog never contains a hash that rpyenv computed from a download,
     so a tampered download at catalog time can't slip in.
   - **Why:** when a web page changes layout, the scheduled job fails in CI,
     not on users' machines.
   - The Windows CPython list (§9.1) is delivered through the same catalog.
   - **PyPy on Windows is x64 only:** PyPy 8.0.0 (Python 3.12) ships Windows
     binaries only for x64.
2. **Conda silent install:**
   - **Windows:** `/S /InstallationType=JustMe /AddToPath=0 /RegisterPython=0
     /NoRegistry=1 /NoShortcuts=1 /D=<path>`. These options are documented by
     conda's `constructor` tool, which builds these installers.
   - **Linux:** `-b -p <prefix>`. With `-b`, the installer doesn't edit shell
     startup files.
   - **Uninstall:** nothing is registered with Windows and no shortcuts are
     created, so `pyenv uninstall` only deletes the folder.
   - **Post-install scripts stay enabled** (`/NoScripts=0`), because some
     packages need them.
3. **Which conda executables get shims:** decided by which package owns each
   file, read from `conda-meta/*.json`. No list of tool names is involved.
   - **Folders searched:** the install root, `Scripts`, and `Library\bin` on
     Windows; `bin/` on Linux.
   - **An executable gets a shim when:**
     - its owning package is Python-related (`python`, `conda`, or any
       package that installs files under `site-packages`), **or**
     - no package owns it and it's in `Scripts` (Windows) or `bin/` (Linux),
       which is where pip writes.
   - **Always skipped:** conda's hook scripts (names starting with `.`, such
     as `.spyder-post-link.bat`) and `activate`/`deactivate`.
   - **Measured on the development machine's `anaconda3`:** 444 executables,
     211 shims, 233 skipped.
     - The skipped ones include 184 general tools in `Library\bin` (e.g.
       `adig.exe`, `aomenc.exe`, Qt tools) and 32 MSYS2 tools in
       `Library\usr\bin` (e.g. `cygpath.exe`, which would otherwise hide Git
       Bash's copy).
     - An earlier draft also shimmed `pre_uninstall.bat` from the install
       root. Limiting unowned files to `Scripts` fixed that.
     - **Compared with upstream's list:** upstream pyenv hides conda's
       non-Python tools with a hand-maintained list of 177 names
       (`rehash/conda.d/default.list`). 16 of those names exist in the
       development machine's install, and the ownership rule shims **none**
       of them. The check could have failed, since any of the 16 getting a
       shim would have appeared. That's one Windows install; a Linux conda
       install should be checked the same way in M7.
   - **Second filter (decided 2026-09-27): upstream's list as well.**
     - rpyenv includes a copy of upstream's `rehash/conda.d/default.list`
       (MIT-licensed; attribution in the copied file), compiled into the
       binary.
     - After the ownership rule, any executable from a conda install or
       environment whose name is on that list gets no shim.
     - It's belt and braces: the ownership rule remains the main filter, and
       the list catches anything conda's package metadata classifies
       unexpectedly.
     - **Keeping the copy current:** the scheduled catalog workflow (§15.1,
       resolution 1) compares the copy with upstream's file and opens a pull
       request when they differ. A copy that lags between rpyenv releases does
       no harm, since the ownership rule already excludes these tools.
   - **`conda` itself:** `Scripts\conda.exe` is owned by `conda`, so
     `conda install …` works through its shim. `conda activate` needs conda's
     own shell integration and isn't supported. Conda environments are
     activated with `pyenv activate <install>/envs/<name>`, as with
     pyenv-virtualenv.
4. **Anaconda's terms of service, and Miniforge:**
   - The docs state that Anaconda's terms require a paid license for
     organizations with 200 or more employees that use Anaconda's default
     channel or the Anaconda Distribution.
   - **M7 also offers Miniforge.** It uses conda-forge, which isn't covered by
     those terms. Miniforge uses the same installer technology and flags as
     Miniconda, and upstream pyenv already lists `miniforge3-*` versions.

### 15.2 Missing Linux build dependencies (M8)

**Goal:** `pyenv install` on Linux succeeds even when library headers that
CPython needs aren't installed. It either installs them through the
distribution's package manager, or compiles them from source without root
access.

**Dependencies:**

| Kind | Libraries | If missing |
|---|---|---|
| Required | OpenSSL (`ssl`, which pip needs), zlib, libffi (`ctypes`), bzip2, xz/lzma, SQLite, readline or libedit, ncurses | Build refuses to start until resolved |
| Optional | Tcl/Tk (`tkinter`), libuuid, gdbm | Warning; the module is left out unless requested |
| Toolchain | C compiler, `make`, `pkg-config` | Can only come from the package manager; rpyenv never compiles a compiler |

**Ways to resolve** (chosen interactively, or with a flag or
`RPYENV_BUILD_DEPS` when there's no terminal):

1. **`system`:** fail and print the exact install command for the detected
   distribution (from `/etc/os-release`: apt, dnf, zypper, pacman, apk). This
   is M2's behavior and stays the default when there's no terminal.
2. **`install`:** run that package-manager command through `sudo`, after
   showing it and getting explicit confirmation. Never runs `sudo` silently.
3. **`build`:** compile the missing libraries from pinned, hash-checked source
   tarballs into the version's own prefix (`versions/<ver>/`). CPython is then
   built against them with `CPPFLAGS`, `LDFLAGS`, and `PKG_CONFIG_PATH`, and
   an `$ORIGIN`-relative rpath so it finds them at runtime. This needs no root,
   and `pyenv uninstall` removes the libraries along with the version.

**Choices made now:**

- **M2's pre-flight check returns structured results:** each missing
  dependency, whether it's required or optional, and what exactly is missing
  (a header, a library, or a `pkg-config` entry). M8 acts on that list without
  rewriting the check.
- **Pinned dependency catalog:** version, URL, and SHA-256 per library,
  checked in. Where upstream `python-build` already pins a library for a
  specific Python version (for example OpenSSL for older releases), that pin
  takes precedence.
- **Source tarballs are cached** in `cache/`, so rebuilding a library for
  another Python version doesn't download it again.

**Open questions for M8:**

- **Share compiled libraries between Python versions?** A shared prefix saves
  build time and disk space, but gives up having each version
  self-contained. The default is per-version.
- **Support musl-based distributions (Alpine)** in the `build` route.
- **Tcl/Tk:** worth building from source, or only via the package manager?

### 15.3 Shared, admin-managed Python versions (M9, only if needed)

M6 keeps Python versions per-user even for an all-users install. If a real
need appears (lab machines, CI images, shared workstations), M9 adds a
machine-wide root in `%ProgramData%\rpyenv` (Windows) or a configurable
system path (Linux):

- Admins install versions there. `pyenv install --system` elevates
  automatically (UAC on Windows, `sudo` on Linux).
- All users can use those versions and can't change them.
- Version resolution searches the user root first, then the machine root, so
  users can still install their own versions.

**Security rule, which applies now as well:** no folder that is shared across
users and searched through `PATH` or used to find executables may ever be
writable by ordinary users. Otherwise one user could plant a `python.exe`
that an administrator later runs.

## 16. Non-goals

- GraalPy and other distributions not listed in §15.
- PyPy source builds, and PyPy on OS/architecture combinations without
  official PyPy binaries.
- pyenv's bash hooks (`pyenv hooks`, `pyenv.d`) as a mechanism: they are
  sourced into pyenv's bash scripts, and a compiled binary has no bash process
  to source them into. The behaviors that the common hooks provide are built
  in instead:
  - upstream's pip-rehash, conda, source-shim, and `:latest` hooks (§8,
    §9.3, §15.1)
  - pyenv-virtualenv's hooks (§10)
  - pyenv-default-packages (§9.3)
- macOS. Not targeted, but nothing in the design rules it out.
- Detecting console reads that come without prior output (deferred; see the
  lazy-console doc).

## 17. Open questions

1. **Windows package source for each version range**, and tkinter coverage
   (§9.1). Answered by the first M2 task.
2. **Code signing** for the shim executables and the MSI, which affects
   SmartScreen warnings and machines that only allow signed code. Options and
   cost to be evaluated before M6.
3. **Default for `RPYENV_CONSOLE`,** decided after measuring the cost of
   LAZY mode (lazy-console open question 3).
4. **The lazy-console doc's other open questions** (Explorer launches with
   `ALLOC_CONSOLE_MODE_DEFAULT`, initial pseudo-console size, hold on error).
