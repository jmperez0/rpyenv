# rpyenv

A drop-in replacement for [pyenv](https://github.com/pyenv/pyenv) and
[pyenv-win](https://github.com/pyenv-win/pyenv-win), written in Rust.
It works the same on Linux and Windows and uses native executable shims
instead of shell scripts and batch wrappers.

> **Status: design stage.** No code has been written yet. This README
> describes what rpyenv is meant to do; nothing below works today. The full
> design is in [docs/specs/2026-09-27-rpyenv-design.md](docs/specs/2026-09-27-rpyenv-design.md).

## Why

pyenv-win shims are batch files. On Windows that causes problems for
everyday use:

- **Arguments are mangled, and can run commands.** Five test arguments were
  sent through pyenv-win's `python.bat` shim and compared with running
  `python.exe` directly. **None** arrived intact:
  - `x^y` became `xy`
  - `100%` became `100`
  - `%USERNAME%` was replaced by the user name
  - `a&b` became `a`, and the text after `&` **ran as a separate command**
  - `say "hi"` never arrived
- **"Terminate batch job (Y/N)?"** after every Ctrl+C.
- **Side effects on your console.** Shims can run `chcp` and switch your
  console's code page, and it stays switched after Python exits.
- **Invisible to non-shell callers.** Anything that starts a process
  without a shell (`subprocess.run(["python", ...])`, IDEs, build tools,
  Rust's `Command`) looks for `python.exe`. There isn't one on `PATH`,
  only `python.bat`.
- **Slow.** Each call goes through a chain: batch shim → `pyenv.bat` →
  VBScript host → the real interpreter.

rpyenv replaces each shim with a small native executable. It runs the
right interpreter directly, passes arguments through unchanged, returns
the child's exit code, and leaves your console as it found it.

## Goals

- **Drop-in compatible.** Same `PYENV_ROOT` layout, same `version` and
  `.python-version` files, same `PYENV_VERSION` variable. The CLI
  answers to `pyenv`. Existing installed versions and project files
  keep working.
- **Native shims on both OSes.** One code path for version resolution.
  Behavior differs by platform only where the OS forces it (see
  [How shims work](#how-shims-work)).
- **Full pyenv command parity**, including `install`, `shell`, and
  `init`. The test suites of pyenv and pyenv-win, plus side-by-side runs
  against the real tools, check this.
- **Official CPython builds.**
  - Windows: python.org's official packages, extracted directly, with no
    installer run and no registry writes.
  - Linux: compiled from source, as pyenv does.
- **Built in instead of plugins:**
  - virtualenvs (replaces `pyenv-virtualenv`, and works natively on Windows
    too)
  - auto-rehash after `pip install`, and for any other tool run through a
    shim
  - default packages (reads the same `default-packages` file as the
    `pyenv-default-packages` plugin)
- **Git-style plugin dispatch.** `pyenv foo` runs `pyenv-foo` if it is
  on `PATH`.
- **No unnecessary console windows on Windows.** Shims never add a window
  that running `python.exe` directly wouldn't create. On Windows 11 24H2
  and later, a window can wait until the program prints something. See
  [docs/windows-lazy-console.md](docs/windows-lazy-console.md).

### Non-goals

- pyenv's bash hooks (`pyenv hooks`) as a mechanism. They are sourced into
  pyenv's bash scripts, and a compiled binary has nothing to source them
  into. What the common hooks do is built in instead.
- GraalPy and other distributions not on the [roadmap](#roadmap).
- macOS. Not targeted, but nothing in the design rules it out.

## How version selection works

Same order as pyenv. The first match wins:

1. `PYENV_VERSION` environment variable (set by `pyenv shell`)
2. A `.python-version` file in the current directory or any parent
   (set by `pyenv local`)
3. The global `version` file in `PYENV_ROOT` (set by `pyenv global`)
4. The system Python, if one exists

## How shims work

A shim is a small executable named after a command (`python`, `pip`,
`ruff`, ...) in `PYENV_ROOT/shims`. When you run it, it works out which
Python version applies and runs that version's copy of the command.

| | Linux | Windows |
|---|---|---|
| Hand-off | `execv` replaces the shim, so no process stays behind. `pip` commands are the exception: the shim waits, so new commands get shims before your next command runs. | Starts the child, waits, returns its exit code |
| Arguments | Passed natively | Passed through byte-for-byte, never re-quoted |
| Ctrl+C | Handled by the interpreter | Shim ignores it; the child handles it |
| Shim killed | n/a (the shim became the program) | A Job Object kills the child too |
| GUI commands (`pythonw`) | n/a | A separate GUI shim, so no console window flashes |
| Batch targets (`.bat`/`.cmd`) | n/a | Launched with Rust's batch escaping, which delivered 11 of 11 test arguments intact |

Shims resolve the version themselves, in-process, without starting
`pyenv` as a second process. The shim binaries don't include the
networking, TLS, or archive code the CLI needs, so they stay small and
start fast.

## Planned architecture

A Cargo workspace with one shared library and three binaries:

| Component | Role |
|---|---|
| `rpyenv-core` | Version resolution, `PYENV_ROOT` layout, version files, rehash |
| `pyenv` | The CLI: every command, installer, virtualenv support |
| `pyenv-shim` | Console shim. `pyenv rehash` links it as `python`, `pip`, …: symlinks on Linux, hardlinks to a per-user copy on Windows |
| `pyenv-shimw` | Windows GUI shim for `pythonw` and GUI scripts |

## Planned commands

pyenv's command set, plus two rpyenv-only setup commands:

| Group | Commands |
|---|---|
| Select a version | `global`, `local`, `shell`, `version`, `version-name`, `version-origin`, `version-file` |
| Inspect | `versions`, `which`, `whence`, `prefix`, `root`, `latest`, `shims` |
| Run | `exec`, `rehash` |
| Install | `install` (including `--list` and `3.12:latest`), `uninstall`, `update` |
| Shell setup | `init`, `completions`, `commands`, `help`, `--version` |
| Virtualenvs | `virtualenv`, `virtualenvs`, `virtualenv-delete`, `virtualenv-prefix`, `activate`, `deactivate` |
| Windows setup (rpyenv-only) | `setup` (prepare a user's `PATH` and folders), `migrate` (take over pyenv-win; `--restore` undoes it) |

## Shell integration

Add one line to your shell profile, as with upstream pyenv:

| Shell | Profile line |
|---|---|
| bash, zsh, Git Bash | `eval "$(pyenv init - bash)"` (or `zsh`) |
| fish | `pyenv init - fish \| source` |
| PowerShell 5.1 and 7 | `pyenv init - pwsh \| Invoke-Expression` |

cmd has no shell functions, so `pyenv shell` can't change its environment.
Instead, it detects that it's running in cmd and prints the exact
`set PYENV_VERSION=…` command to run. It does the same in any shell without
integration.

## Environment variables

| Variable | Meaning |
|---|---|
| `PYENV_ROOT` | Root directory. Default `~/.pyenv` on Linux, `%USERPROFILE%\.pyenv\pyenv-win` on Windows. pyenv-win's `PYENV` and `PYENV_HOME` are honored too. |
| `PYENV_VERSION` | Overrides the selected version for the current shell |
| `PYENV_DEBUG` | Prints where the version came from and which executable was chosen |
| `PYTHON_BUILD_MIRROR_URL` | Alternate download mirror for CPython sources and packages |

rpyenv-only settings use the `RPYENV_` prefix, for example
`RPYENV_LIVE_REHASH=1` to rehash while long-running programs such as Jupyter
install packages. The full list is in the design spec (§13).

## Installing (planned)

- **Windows:** an MSI that installs either **just for you** (no admin
  rights) or **for all users**. For all users, Windows asks for elevation
  when installation starts. Your Python versions stay in your own profile
  either way.
- **Linux:** a tarball and an install script.

### Migrating from pyenv-win

1. Install rpyenv with the MSI.
2. Run `pyenv migrate`, or let the MSI run it. It:
   - takes pyenv-win's `bin` off your `PATH`,
   - moves pyenv-win's `pyenv.bat`, `pyenv.ps1`, and `pyenv` into a backup
     folder,
   - rebuilds the shims as `.exe` files.

   This matters because a `pyenv.ps1` would win over `pyenv.exe` in
   PowerShell.

Installed versions, the global `version` file, and `.python-version` files
stay as they are. `pyenv migrate --restore` undoes the migration.

## Roadmap

1. **M1 Core.** Version resolution, shims, rehash, and the inspect and
   version-selection commands. rpyenv becomes usable with Pythons that pyenv
   or pyenv-win already installed.
2. **M2 Installer.** `install`, `uninstall`, `update`, `latest`: official
   packages on Windows, source builds on Linux.
3. **M3 Shell integration.** `init`, `shell`, and completions.
4. **M4 Virtualenvs and plugins.**
5. **M5 Windows console and live rehash.** Console windows that appear only
   when needed; live rehash on both OSes.
6. **M6 Release.** The Windows MSI, the Linux tarball, and the install
   script.
7. **M7 More distributions.** PyPy (where official binaries exist), plus
   Anaconda, Miniconda, and Miniforge.
8. **M8 Missing Linux build dependencies.** Install them through the
   package manager, or compile them without root access.
9. **M9 Shared versions** that an administrator manages for all users, if
   the need appears.

## Building

Not available yet. When code lands it will be a standard Cargo
workspace: `cargo build --release`.

## License

[MIT](LICENSE)
