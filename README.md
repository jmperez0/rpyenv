# rpyenv

A drop-in replacement for [pyenv](https://github.com/pyenv/pyenv) and
[pyenv-win](https://github.com/pyenv-win/pyenv-win), written in Rust.
It works the same on Linux and Windows and uses native executable shims
instead of shell scripts and batch wrappers.

> **Status: v0.2.0** (Windows MSI, Linux tarball and install script). It covers roadmap
> milestones M1 to M8 below. The full design is in
> [docs/specs/2026-09-27-rpyenv-design.md](docs/specs/2026-09-27-rpyenv-design.md).

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
- **Official builds by default.** rpyenv installs three kinds of Python:
  - **CPython:** python.org's packages on Windows, extracted with no installer and no
    registry writes; compiled from source on Linux, as pyenv does. On Linux, missing
    libraries are found before the build: rpyenv can install them through the package
    manager or compile them without root. If you ask for it (never by default), it installs
    a third-party python-build-standalone build instead of compiling.
  - **PyPy:** PyPy's official binaries, on Linux (x64, aarch64) and Windows (x64),
    each checked against the SHA-256 PyPy publishes.
  - **Anaconda, Miniconda and Miniforge:** the publishers' installers, on Linux (x64,
    aarch64) and, from v0.2.0, on Windows (x64), each checked against its
    publisher's SHA-256.

  Names, hashes, limits and what isn't supported:
  [docs/distributions.md](docs/distributions.md).
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

## Architecture

A Cargo workspace with one shared library and three binaries:

| Component | Role |
|---|---|
| `rpyenv-core` | Version resolution, `PYENV_ROOT` layout, version files, rehash |
| `pyenv` | The CLI: every command, installer, virtualenv support |
| `pyenv-shim` | Console shim. `pyenv rehash` links it as `python`, `pip`, …: symlinks on Linux, hardlinks to a per-user copy on Windows |
| `pyenv-shimw` | Windows GUI shim for `pythonw` and GUI scripts |

## Commands

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

These are the variables you can set to change what rpyenv does. "Both" means
Linux and Windows. rpyenv-only settings use the `RPYENV_` prefix, so they can't
clash with a future pyenv variable.

### Where things are and which version

| Variable | OS | What it does |
|---|---|---|
| `PYENV_ROOT` | Both | The root folder, which holds the versions and the shims. Default `~/.pyenv` on Linux, `%USERPROFILE%\.pyenv\pyenv-win` on Windows. |
| `PYENV`, `PYENV_HOME` | Windows | pyenv-win's names for the root folder. rpyenv uses the first one set of `PYENV_ROOT`, `PYENV` and `PYENV_HOME`. |
| `PYENV_VERSION` | Both | Selects the version, ahead of any version file. `pyenv shell` sets it. |
| `PYENV_DIR` | Linux | The folder where the search for a `.python-version` file starts, instead of the current folder. The folder must exist. |
| `PYENV_FORCE_ARCH` | Windows | Which builds a version name means: `X86` for 32-bit (`-win32`), `ARM64` for `-arm64`, any other value for x64. Default: the machine's own architecture. |
| `PYENV_SHELL` | Both | The shell that `pyenv shell`, `activate` and the other shell commands write code for. `pyenv init` sets it; without it, Linux uses `SHELL` and Windows looks at the parent process. |
| `PYENV_HOOK_PATH` | Linux | Folders, separated by `:`, that `pyenv hooks` searches before the default hook folders. rpyenv runs no hooks itself; plugins use the list. |
| `PYENV_REHASH_TIMEOUT` | Both | How many seconds `pyenv rehash` waits while another rehash holds the lock. Default 60. |
| `PYENV_DEBUG` | Linux | Any value makes `pyenv install` verbose, as `-v` does. rpyenv has no equivalent of upstream's bash trace. |

### Installing Python

On Linux, `pyenv install` builds CPython from source and reads python-build's
variables. The build-dependency check and its settings are described in
[docs/distributions.md](docs/distributions.md).

| Variable | OS | What it does |
|---|---|---|
| `PYTHON_BUILD_MIRROR_URL` | Both | Linux: a mirror tried before each file's own URL, with files named by their SHA-256. Windows: a site with python.org's layout that `pyenv update` lists, and that later installs download from. |
| `PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM` | Linux | Any value: the mirror has python.org's folder layout instead of files named by SHA-256. |
| `PYTHON_BUILD_SKIP_MIRROR` | Linux | Any value: download each file from its own URL only. Otherwise files not from python.org are tried first at `https://pyenv.github.io/pythons`. |
| `PYTHON_BUILD_CACHE_PATH` | Linux | A folder of downloaded archives that `pyenv install` reuses and adds to. Default `$PYENV_ROOT/cache`. It is used only when the folder exists. |
| `PYTHON_BUILD_DEFINITIONS` | Linux | Folders, separated by `:`, searched for build definitions before the plugins' and the built-in ones. |
| `PYENV_BUILD_ROOT` | Linux | Builds in `<folder>/<version>` and keeps the source tree there, as `-k` does. `-k` alone uses `$PYENV_ROOT/sources`. |
| `PYTHON_BUILD_BUILD_PATH` | Linux | The folder to build in. Default: a new folder under `TMPDIR`. It is deleted after a successful build, unless `-k` or `PYENV_BUILD_ROOT` is used. |
| `TMPDIR` | Linux | Where the build log and the default build folder go. Default `/tmp`. Programs must be able to run from it. |
| `CONFIGURE_OPTS` | Linux | Extra options for `./configure`. `--enable-framework` and `--enable-universalsdk` are refused (they are for macOS). |
| `PYTHON_CONFIGURE_OPTS` | Linux | More options for `./configure`, after `CONFIGURE_OPTS`. |
| `PYTHON_CONFIGURE` | Linux | The command to run instead of `./configure`. |
| `PYTHON_BUILD_FREE_THREADING` | Linux | Any value builds a free-threaded Python (`--disable-gil`). |
| `CC` | Linux | The C compiler. The build gets it as it is, and the dependency check uses it. Default for the check: `cc`, else `gcc`. |
| `CFLAGS` | Linux | Compiler flags. The build gets them as they are. |
| `PYTHON_CFLAGS` | Linux | Added after `CFLAGS` for `./configure` only. |
| `CPPFLAGS` | Linux | Preprocessor flags. rpyenv puts `-I<prefix>/include` before them. The dependency check uses them to find headers. |
| `PYTHON_CPPFLAGS` | Linux | Added after `CPPFLAGS` for `./configure` and the dependency check. |
| `LDFLAGS` | Linux | Linker flags. rpyenv puts `-L<prefix>/lib` and, for a shared build, an rpath before them. |
| `PYTHON_LDFLAGS` | Linux | Added after `LDFLAGS` for `./configure` and the dependency check. |
| `LIBS` | Linux | Libraries to link. rpyenv puts `-L<prefix>/lib` and the rpath before them. |
| `MAKE` | Linux | The make program. Default `make`. |
| `MAKEOPTS`, `MAKE_OPTS` | Linux | Options for the build's `make`. `MAKEOPTS` wins. Default `-j <number of CPUs>`. |
| `PYTHON_MAKE_OPTS` | Linux | More options for the build's `make`, after `MAKEOPTS`. |
| `PYTHON_MAKE_INSTALL_TARGET` | Linux | The make target that installs. Default `install`. With a target that contains `altinstall`, rpyenv adds no unversioned names such as `python`. |
| `MAKE_INSTALL_OPTS` | Linux | Options for the installing `make`. |
| `PYTHON_MAKE_INSTALL_OPTS` | Linux | More options for the installing `make`, after `MAKE_INSTALL_OPTS`. |
| `SSL_CERT_FILE` | Linux | A file of CA certificates. When it or `SSL_CERT_DIR` is set, rpyenv's downloads trust only those certificates. The dependency check accepts it as the CA certificates. |
| `SSL_CERT_DIR` | Linux | Folders of CA certificates, separated by `:`. Used as `SSL_CERT_FILE` is. |
| `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` | Both | The proxy for rpyenv's downloads. rpyenv uses the first one set of `ALL_PROXY`, `HTTPS_PROXY` and `HTTP_PROXY`; lower-case names work too. |
| `NO_PROXY` | Both | Hosts, separated by commas, that rpyenv's downloads reach without the proxy. Lower case works too. |
| `RPYENV_BUILD_DEPS` | Linux | How `pyenv install` gets missing build libraries: `system`, `install`, `build` or `prebuilt`, as `--deps=` (the flag wins). `prebuilt` installs a third-party python-build-standalone build instead of compiling; it is never the default. With it set, rpyenv doesn't ask. When neither it nor `--deps` is set and there is no terminal, a missing library stops the build (CPython 3). |
| `RPYENV_SKIP_PREFLIGHT` | Linux | `1` skips the dependency check before a build, and turns a failed check after it into a one-line warning: the build stays. |

### Virtual environments

| Variable | OS | What it does |
|---|---|---|
| `PYENV_VIRTUALENV_CACHE_PATH` | Both | The working folder of `pyenv virtualenv`, made if missing. Default `PYTHON_BUILD_CACHE_PATH`, else `$PYENV_ROOT/cache`. |
| `GET_PIP` | Both | A local `get-pip.py` that `pyenv virtualenv` runs when `ensurepip` can't install pip. |
| `GET_PIP_OPTS` | Both | Options for that `get-pip.py`. |
| `GET_PIP_URL` | Both | Not used: rpyenv never downloads `get-pip.py`. When pip can't be installed, the error says this variable was ignored. |
| `PYENV_VIRTUALENV_ROOT` | Linux | `pyenv virtualenv-init` puts `$PYENV_VIRTUALENV_ROOT/shims` first on `PATH`, instead of the folder rpyenv makes under `$PYENV_ROOT/.rpyenv`. rpyenv writes nothing there. |
| `PYENV_VIRTUALENV_PROMPT` | Both | The prompt tag `activate` adds. `{venv}` stands for the environment's name. Default `(<name>)`; fish ignores it. |
| `PYENV_VIRTUALENV_DISABLE_PROMPT`, `PYENV_VIRTUAL_ENV_DISABLE_PROMPT`, `VIRTUAL_ENV_DISABLE_PROMPT` | Both | Any value: `activate` leaves the prompt as it is. |
| `PYENV_VIRTUALENV_VERBOSE_ACTIVATE` | Both | Any value makes `activate` and `deactivate` verbose, as `-v` does. |

### rpyenv only

| Variable | OS | What it does |
|---|---|---|
| `RPYENV_LIVE_REHASH` | Both | `1`: while a program started through a shim runs, rpyenv rehashes as soon as new commands appear, for example packages installed from Jupyter. |
| `RPYENV_CONSOLE` | Windows | For a program started without a console (from Explorer, say): `lazy` (default) opens a window only once it prints; `eager` opens one at once. Needs Windows 11 24H2 or later. |
| `RPYENV_CONSOLE_HOLD` | Windows | After a failure, how long a window the shim opened stays: until a key (default), `0` closes it at once, a whole number N closes it after N seconds. |
| `RPYENV_DEBUG_LOG` | Both | A file that shims and `pyenv exec` add their decisions and errors to, in UTF-8. Useful when there is no console. |
| `RPYENV_BATCH_FORWARD` | Windows | Names of batch files, separated by `;`, that `pyenv rehash` gives a `.cmd` forwarder instead of an `.exe` shim. This is for batch tools that must change the caller's environment. |

"Any value" means any value that is not empty. rpyenv also reads variables
that the system, your shell or an activation tool set, but they are not rpyenv
settings: `HOME`, `USER`, `SHELL`, `PATH`, `PWD`, `DISPLAY`, `LD_LIBRARY_PATH`
(which `pyenv exec -N` extends), `USERPROFILE`, `APPDATA`, `PATHEXT`,
`ProgramFiles`, `VIRTUAL_ENV`, `CONDA_DEFAULT_ENV`, `PYTHONHOME` and `PROMPT`.
Debug builds also read `RPYENV_TEST_*` variables for tests; release builds
ignore them.

## Installing

Releases: https://github.com/jmperez0/rpyenv/releases. From v0.2.0 the MSIs and the programs
in them are signed with a Certum code-signing certificate. SmartScreen may still warn while
the certificate is new ("More info", then "Run anyway"); v0.1.0 is unsigned and always warns.

### Windows

1. Run `rpyenv-<version>-x64.msi` (or `-arm64.msi`).
2. Choose **Just for me** (no administrator rights) or **For all users**.
3. Open a new terminal. For all users, each user's setup runs at their next sign-in or
   on their first `pyenv` command. `python`, `pip` and the other shims then work in
   every shell, cmd included.

Silent install, just for me: `msiexec /i rpyenv-<version>-x64.msi /qn`.

Uninstall from Settings, Apps. Your Python versions stay.

The MSI's other options, PowerShell's execution policy (which `pyenv shell` needs),
all-users and silent installs, Smart App Control, and uninstall details:
[docs/windows-install.md](docs/windows-install.md).

### Linux

```sh
curl -fsSL https://github.com/jmperez0/rpyenv/releases/latest/download/install.sh | sh
```

It installs `pyenv` into `$PYENV_ROOT/bin` (default `~/.pyenv/bin`), checks the
download against `SHA256SUMS`, and offers to add pyenv to your shell's startup file.
Options go after `sh -s --`: `--version vX.Y.Z`, `--init` or `--no-init`, `--uninstall`.

If an upstream pyenv is installed in `~/.pyenv`, the script offers to move it aside to
`~/.pyenv/.upstream-pyenv`; `--take-over` does it without asking, and
`--restore-upstream` puts it back. Your Python versions, `version` file,
`default-packages` and plugins stay.

### Verifying a download

```sh
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify rpyenv-<version>-linux-x64.tar.gz --repo jmperez0/rpyenv \
  --source-ref refs/tags/v<version>
```

### Migrating from pyenv-win

The MSI offers it, or run `pyenv migrate` later. It:

- takes pyenv-win's `bin` off your `PATH`,
- moves pyenv-win's `pyenv.bat`, `pyenv.ps1`, and `pyenv` into a backup folder (a
  `pyenv.ps1` would win over `pyenv.exe` in PowerShell),
- rebuilds the shims as `.exe` files,
- adds the PowerShell profile line (Windows PowerShell's only where it can run scripts),
- links your pyenv-win-venv envs in as virtualenvs, leaving their files where they are.

Installed versions, the global `version` file, and `.python-version` files stay as they
are. `pyenv migrate --restore` undoes exactly what the migration did.

Without pyenv-win, `pyenv setup` prepares your user, and running it again changes
nothing. `pyenv init --install pwsh` adds just the profile line.

## Roadmap

1. **M1 Core** (done). Version resolution, shims, rehash, and the inspect and
   version-selection commands. rpyenv becomes usable with Pythons that pyenv
   or pyenv-win already installed.
2. **M2 Installer** (done). `install`, `uninstall`, `update`, `latest`: official
   packages on Windows, source builds on Linux.
3. **M3 Shell integration** (done). `init`, `shell`, and completions.
4. **M4 Virtualenvs and plugins** (done).
5. **M5 Windows console and live rehash** (done). Console windows that appear only
   when needed; live rehash on both OSes.
6. **M6 Release** (done, v0.1.0). The Windows MSI, the Linux tarball, and the install
   script.
7. **M7 More distributions** (done). PyPy where official binaries exist (M7a, v0.1.0);
   Anaconda, Miniconda and Miniforge on Linux (M7b, v0.1.0) and on Windows (M7c, v0.2.0).
8. **M8 Missing Linux build dependencies** (done, v0.2.0). Install them through the
   package manager (M8a), compile them without root access (M8b), including X11 for Tk
   (M8c), or install a python-build-standalone build instead (M8d).
9. **M9 Shared versions** that an administrator manages for all users, if
   the need appears.

## Building

A standard Cargo workspace; the toolchain is pinned in `rust-toolchain.toml` (Rust 1.96).

```sh
cargo build --release   # pyenv, pyenv-shim, pyenv-shimw (the Windows GUI shim)
cargo test --workspace
```

Tests never touch your own pyenv root: they set `PYENV_ROOT` (and on Windows `PYENV` and
`PYENV_HOME`) to temporary folders. The Windows MSI is built with
`installer/build.ps1 -Arch x64 -BinDir target/release -OutDir dist`, which uses the WiX
version it pins.

### Accepting a changed conda hash

The weekly sync pins every Windows conda installer hash in `crates/pyenv/conda/windows.tsv`.
When it fails with `changed hash, kept the pinned one: <name>`, a publisher's file changed
under a name rpyenv already trusts. Find out why first: the release notes, the publisher's
announcement, `Get-AuthenticodeSignature` on the new installer. Then run
`cargo run -p pyenv --example conda_windows_table -- crates/pyenv/conda/linux.tsv crates/pyenv/conda/windows.tsv --accept <name>`
and open a pull request that says why the new hash is right.

## License

[MIT](LICENSE)
