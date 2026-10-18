# Python distributions

rpyenv installs three kinds of Python: CPython, PyPy, and the conda distributions
(Anaconda, Miniconda and Miniforge). This page says where each one comes from, how
rpyenv checks it, and what it does not support.

| Distribution | Linux | Windows |
|---|---|---|
| CPython | Compiled from source, as pyenv does | python.org's official packages, extracted |
| PyPy | x64, aarch64 | x64 |
| Anaconda, Miniconda, Miniforge | x64, aarch64 | x64; ARM64 runs the x64 installers under emulation. After v0.1.0 |

## CPython

- **Windows:** rpyenv extracts python.org's official packages directly. It runs no
  installer and writes nothing to the registry.
- **Linux:** rpyenv compiles CPython from source, as pyenv does.

### Missing libraries on Linux

CPython 3 needs some system libraries to build. If one is missing, the matching module
(`ssl`, `sqlite3`, `tkinter` and so on) is left out of Python. rpyenv checks for them
before it builds, so you find out first.

**What is checked.** For each library, rpyenv compiles and links a small program. It
also checks the versions below, and that CA certificates exist. `tkinter` is checked
without needing a display.

| Library | Version |
|---|---|
| OpenSSL | 1.1.1 or later for Python 3.10 and later; 1.0.2 or later for 3.7 to 3.9 |
| SQLite | 3.15.2 or later for Python 3.13 and later |
| zstd | 1.4.5 or later, for 3.14's `compression.zstd` |
| Tk | 8.5 or 8.6 for Python 3.12 and older, which can't use Tk 9 |
| CA certificates | A file or directory must exist. `SSL_CERT_FILE` and `SSL_CERT_DIR`, when set, are where rpyenv and the built Python look |

Fedora 42 and later ship Tk 9 as `tk-devel`. To build Python 3.12 or older there, rpyenv
asks for `tk8-devel` (Tk 8.6) instead.

These checks are for CPython built from source. Python 2.x, PyPy and conda keep their
old behavior: no check before the install, and only upstream pyenv's module check after
it.

**The prompt.** At a terminal, rpyenv lists the missing modules and the packages that
provide them. Then you choose:

| Key | What happens |
|---|---|
| `p` (default) | Prints the install command and stops |
| `i` | Runs the command through `sudo` (no `sudo` when you are root; no `-y`), then checks again |
| `b` | Builds the missing libraries from source, without root, and links them into Python. Offered for CPython 3.7 and later when the compiler, `make`, `patch` and `awk` are there; it says what it can't build here and why |
| `c` | Builds without the missing libraries and lists them at the end. Offered only when the compiler, `make`, `patch`, `awk`, OpenSSL and zlib are all there |

`i` is left out, with the reason, when `sudo` can't be used or rpyenv doesn't know your
distribution. rpyenv knows apt, dnf, zypper and pacman. Alpine comes later.

On RHEL, Rocky, Alma and CentOS Stream, some `-devel` packages are in the CRB repository
(PowerTools on version 8), which is off by default. The prompt, the printed list and the
summary at the end of a script's run show the command that turns it on:

```
Some packages live in the CRB repository; enable it first: sudo dnf install dnf-plugins-core && sudo dnf config-manager --set-enabled crb
```

**`--deps` and `RPYENV_BUILD_DEPS`.** Either one picks the route and skips the prompt,
even at a terminal. The flag wins over the variable.

| Value | What happens |
|---|---|
| `system` | Stops with the list of what is missing |
| `install` | Installs the packages, then builds. Without a terminal it adds `-y` and uses `sudo -n` |
| `build` | Builds the missing libraries from source, then Python. Stops before building anything when one of them can't be built here |
| `prebuilt` | Says it comes in a later release |

**Scripts that set neither.** Without a terminal, `--deps` or `RPYENV_BUILD_DEPS`,
rpyenv is lenient, as upstream is. Only the compiler, `make`, `patch` (when the
definition needs it), `awk`, OpenSSL and zlib stop the build. Any other missing library is a
warning, and the run ends with a three-line summary. A later rpyenv release will make
that summary stop the build. To choose now, use `--deps=install`, `--deps=build` or
`--deps=system`.

**After the build.** rpyenv imports every standard-library module, checks that CA
certificates exist, and starts Tcl. If one fails, it removes the build and tells you
which module and which package. For missing CA certificates, it names the file and the
directory the built Python looked in. In lenient mode this is the three-line summary,
and the build stays. With `RPYENV_SKIP_PREFLIGHT=1` it is a one-line warning, and the
build stays. `RPYENV_SKIP_PREFLIGHT=1` also skips the check before the build.

### Building libraries without root

`pyenv install --deps=build`, or `b` at the prompt, compiles each missing library from source
and links it into Python statically. Nothing is installed on the system.

- **Where:** `$PYENV_ROOT/cache/deps/<library>-<version>-<hash>/`. Another Python version that
  needs the same library, version and options reuses it.
- **Only what is missing** is built. Libraries the system has keep getting its updates.
- **It is a cache.** Deleting `cache/deps` breaks no installed Python; the next build compiles
  again. (The OpenSSL folder below lives elsewhere, in `$PYENV_ROOT/.rpyenv/ssl/`; keep that one.)
- **Tcl/Tk scripts:** a Python whose Tcl/Tk was built this way, and every virtual environment
  made from it, reads Tcl's and Tk's scripts from `$PYENV_ROOT/.rpyenv/tcltk/`. Keep that
  folder: if it is deleted, `pyenv install` and `pyenv rehash` re-create it from the installed
  versions, and `pyenv uninstall` removes what no installed version uses.
- Creating `cache/deps` also creates `$PYENV_ROOT/cache`, which turns on the download cache for later installs (as creating it by hand does).
- **Versions:** rpyenv pins each library's version and SHA-256 and checks every download. A
  Python version whose definition pins OpenSSL or readline gets that version.
- **What the system still needs:** a C compiler, `make`, `patch` and `awk` (`gawk`; Tumbleweed's base image has none). OpenSSL and libxcrypt also
  need Perl with its core modules (`perl-core` on Fedora); Tk needs the X11 headers
  (`libx11-dev`, `libX11-devel`) until a later release builds those too.
- **`PYENV_ROOT`** must have no spaces, shell characters or `:` in its path.
- **CA certificates:** a self-built OpenSSL gets a private folder, `$PYENV_ROOT/.rpyenv/ssl/`,
  with no `openssl.cnf` (a distribution's config is written for its own OpenSSL; Tumbleweed's
  broke `ssl` in a vanilla one). Its `cert.pem` and `certs` link to the system's certificates,
  so the distribution's updates still apply. When the system has none, `cert.pem` is a bundle
  rpyenv pins (curl's copy of Mozilla's list); `pyenv install` keeps the links and the bundle
  current, and replaces the bundle when an rpyenv upgrade pins a newer one. rpyenv itself
  downloads without system certificates too: it then trusts Mozilla's list compiled into it,
  and still checks every download's SHA-256.
- **Updates:** each version built this way lists its libraries in `versions/<v>/.rpyenv-deps`.
  When an rpyenv upgrade pins a newer version of one of them, `pyenv install` prints one line
  per affected Python version, with the command that rebuilds it.

## PyPy

rpyenv installs PyPy from PyPy's official binaries, and checks each download against
the SHA-256 that PyPy publishes.

| | Linux (x64, aarch64) | Windows (x64) |
|---|---|---|
| Names | Upstream pyenv's | pyenv-win's codes |
| Example | `pyenv install pypy3.11` | `pyenv install pypy3.11-v7.3.20-win64` |
| Before the first install | Nothing | Run `pyenv update` |

Not supported:

- PyPy source builds and nightlies.
- Distribution-specific definitions.
- Third-party builds: older definitions that download someone else's portable PyPy.

## Conda on Linux

**Names.** rpyenv uses upstream pyenv's names, for example
`pyenv install miniforge3-<release>`.

**Hashes.** rpyenv checks each installer against its published SHA-256.

**`-latest` names.** For these, rpyenv reads the hash from the same publisher at install
time. That catches a broken download, but not a compromised publisher. To get a hash
that upstream pyenv reviewed, pick a numbered version.

**A bare family name.** `pyenv install miniconda3` on its own follows upstream's `latest`
sort. That sort picks an old release that has only an MD5 hash (for example
`miniconda3-4.7.12`), and rpyenv refuses it. The refusal names the newest release that
installs (for example `miniconda3-3.14-26.7.1-1`).

**Terms.** Miniconda 25.x prints a note that its defaults channel is under Anaconda's
terms. Miniforge has no such note.

**Shims.** Conda's bundled system tools get no shims, as with upstream.

**Your active conda env is left alone.** The installer and its `conda install pip` step
run without the variables `conda activate` sets (`CONDA_PREFIX` and the rest) and
without Python's path variables, so an env you have active is never the target.
Upstream pyenv passes them on. conda's settings, such as `CONDA_CHANNEL_ALIAS` for a
company mirror or `CONDA_SSL_VERIFY`, still apply.

## Conda on Windows

Conda on Windows is new after v0.1.0.

### Install

- **Names** are the ones Linux uses: `pyenv install miniforge3-26.7.2-0`,
  `miniconda3-3.13-25.9.1-3`, `anaconda3-2026.07-1`.
- **A bare family name** (`pyenv install miniforge3`) installs that family's newest
  installable release.
- **The installer.** rpyenv runs the publisher's official installer silently, into
  `versions\<name>`. The installer makes no registry entries, no shortcuts and no
  `PATH` change.
- **Hashes.** rpyenv checks each download against its publisher's SHA-256.
- **Log.** Each install writes its output to `install_cache\<name>.log`. If the install
  fails, the error names that file.
- **Ctrl+C** ends the installer and all its child processes.
- **Your active conda env is left alone**, as on Linux: the installer and its steps run
  without the variables `conda activate` sets. conda's settings, such as
  `CONDA_CHANNEL_ALIAS`, still apply.
- **Reinstall.** `pyenv install -f` keeps the install's `envs\` folder.

### The version table

rpyenv keeps a table of the conda versions it can install.

- **After you install rpyenv**, the table that the MSI ships is already there. So
  `pyenv install <conda name>`, `pyenv install --list` and `pyenv latest -k` work
  straight away, without `pyenv update`.
- **`pyenv update`** rebuilds a newer table from the publishers' pages and prints
  `conda: <n> installable versions (<k> new)`. It adds new releases only.
- **If a publisher changes an installer** that rpyenv has already pinned, `update` keeps
  the pinned hash and warns. That version will not install until an rpyenv release
  carries the reviewed hash.
- **If a conda site can't be read**, `update` warns, keeps the previous table and
  exits 0.

### What the hashes mean

| Where the hash came from | What it means |
|---|---|
| Shipped with rpyenv | Pinned when its pull request was reviewed |
| Added by `pyenv update` | Read from the publisher's page at that moment: catches a broken download, not a compromised publisher |
| A `-latest` name | Read at install time, as for `update` |

### Characters the installers refuse

The installers refuse some characters in the install folder's path. rpyenv checks the
path first and refuses before it downloads anything.

| Character | Refused by |
|---|---|
| `^ % ! = & ( ) ; , ' # $ @ ~ [ ] { }` | All three installers |
| Characters outside Latin-1 | All three (inferred from two measured characters) |
| Space | Miniforge only |

If your pyenv folder has a space, either:

- use a `miniconda3` version, or
- set `PYENV_ROOT`, `PYENV` and `PYENV_HOME` to a folder without spaces.

A path written with 8.3 short names (for example `C:\Users\RUNNER~1`) is fine: rpyenv
passes the installer the long form.

### Shims and `PATH`

- **Which programs get shims** depends on the conda package that owns each file.
  `python`, `pip`, `conda` and other Python tools get a shim. `curl`, `cygpath` and
  conda's other system tools don't.
- **`PATH`.** A command run through a shim or `pyenv exec` sees conda's own `PATH` order
  (`Library\bin` and the rest).
- **Envs.** An env keeps its install's `condabin` on `PATH`, so `pyenv exec conda`
  works inside it. To use an env:
  1. Run `conda create -n <env>` through the shim.
  2. Run `pyenv shell <install>/envs/<env>` or `pyenv activate <install>/envs/<env>`.

### Terms

Miniconda 25 and later accepts Anaconda's terms only during the install, as upstream
does. rpyenv prints a note first. The first later use of the defaults channel asks you
to accept them.

### Not supported on Windows

- `conda activate`. It needs conda's shell hook.
- `activate.d` scripts. Some packages, for example GDAL, set variables there.
- `conda install` from a selected env. Without `-n`, it installs into the base install,
  as upstream does, because rpyenv sets no `CONDA_PREFIX`.
