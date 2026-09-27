# rpyenv

A drop-in replacement for [pyenv](https://github.com/pyenv/pyenv) and
[pyenv-win](https://github.com/pyenv-win/pyenv-win), written in Rust.
It works the same on Linux and Windows and uses native executable shims
instead of shell scripts and batch wrappers.

> **Status: design stage.** No code has been written yet. This README
> describes what rpyenv is meant to do; nothing below works today.

## Why

pyenv-win shims are batch files. On Windows that causes problems for
everyday use:

- **Mangled arguments.** `cmd.exe` re-parses `%*`, so arguments with
  `^`, `%`, `!`, or `&` can be changed before they reach Python.
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
  `init`.
- **Official CPython builds.** On Windows: python.org's official
  packages, extracted directly, with no installer run and no registry
  writes. On Linux: compiled from source, as pyenv does.
- **Built-in virtualenv support.** Replaces the `pyenv-virtualenv`
  plugin and works natively on Windows too.
- **Git-style plugin dispatch.** `pyenv foo` runs `pyenv-foo` if it is
  on `PATH`.

### Non-goals (for now)

- Python distributions other than CPython (PyPy, Anaconda/Miniconda,
  GraalPy).
- pyenv's bash hook scripts (`pyenv hooks`). These can't run natively
  on Windows.
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
| Hand-off | `execv` replaces the shim; no process stays behind | Starts the child, waits, returns its exit code |
| Ctrl+C | Handled by the interpreter | Shim ignores it; the child handles it |
| Shim killed | n/a | Job Object kills the child too |
| GUI commands (`pythonw`) | n/a | Separate GUI-subsystem shim, so no console window flashes |

Shims resolve the version themselves, in-process, without starting
`pyenv` as a second process. The shim binaries don't include the
networking, TLS, or archive code the CLI needs, so they stay small and
start fast.

## Planned architecture

A Cargo workspace with one shared library and three binaries:

| Component | Role |
|---|---|
| `rpyenv-core` | Version resolution, `PYENV_ROOT` layout, version files |
| `pyenv` | The CLI: every command, installer, virtualenv support |
| `pyenv-shim` | Console shim; `pyenv rehash` hardlinks it as `python`, `pip`, ... |
| `pyenv-shimw` | Windows GUI-subsystem shim for `pythonw` and GUI scripts |

## Planned commands

pyenv's command set:

| Group | Commands |
|---|---|
| Select a version | `global`, `local`, `shell`, `version`, `version-name`, `version-origin`, `version-file` |
| Inspect | `versions`, `which`, `whence`, `prefix`, `root`, `latest`, `shims` |
| Run | `exec`, `rehash` |
| Install | `install`, `uninstall`, `install --list` |
| Shell setup | `init`, `completions`, `commands`, `help`, `--version` |
| Virtualenvs | `virtualenv`, `virtualenvs`, `virtualenv-delete`, `virtualenv-prefix`, `activate`, `deactivate` |

## Environment variables

| Variable | Meaning |
|---|---|
| `PYENV_ROOT` | Root directory. Default `~/.pyenv` on Linux, `%USERPROFILE%\.pyenv\pyenv-win` on Windows. pyenv-win's `PYENV` and `PYENV_HOME` are honored too. |
| `PYENV_VERSION` | Overrides the selected version for the current shell |
| `PYTHON_BUILD_MIRROR_URL` | Alternate download mirror for CPython sources and packages |

## Migrating from pyenv-win (planned)

1. Put the rpyenv binaries in `%PYENV_ROOT%\bin`.
2. Run `pyenv rehash`. It replaces the `.bat` shims with `.exe` shims.
   It also removes the extensionless `sh` shims: in Git Bash, the
   `python` script would otherwise be found before `python.exe`.

Installed versions, the global `version` file, and `.python-version`
files stay as they are.

## Roadmap

1. **Core.** Version resolution, shims, `exec`/`which`/`rehash`, and the
   version-selection commands. rpyenv becomes usable with Pythons that
   pyenv or pyenv-win already installed.
2. **Installer.** `install`/`uninstall`: official packages on Windows,
   source builds on Linux.
3. **Shell integration.** `init` and `shell` for bash, zsh, fish,
   PowerShell, and Git Bash.
4. **Virtualenvs and plugins.** Built-in virtualenv commands and
   `pyenv-foo` dispatch.
5. **Release.** Prebuilt binaries for Linux and Windows, and an install
   script.

## Building

Not available yet. When code lands it will be a standard Cargo
workspace: `cargo build --release`.

## License

[MIT](LICENSE)
