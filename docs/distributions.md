# Python distributions

rpyenv installs three kinds of Python: CPython, PyPy, and the conda distributions
(Anaconda, Miniconda and Miniforge). This page says where each one comes from, how
rpyenv checks it, and what it does not support.

| Distribution | Linux | Windows |
|---|---|---|
| CPython | Compiled from source, as pyenv does | python.org's official packages, extracted |
| PyPy | x64, aarch64 | x64 |
| Anaconda, Miniconda, Miniforge | x64, aarch64 | x64. ARM64 runs the x64 installers under emulation. From v0.2.0 |

## CPython

- **Windows:** rpyenv extracts python.org's official packages directly. It runs no
  installer and writes nothing to the registry.
- **Linux:** rpyenv compiles CPython from source, as pyenv does.

### Missing libraries on Linux

CPython 3 needs some system libraries to build. If one is missing, the build leaves the
matching module (`ssl`, `sqlite3`, `tkinter` and so on) out of Python. rpyenv checks for
them before it builds, so you find out first.

**What rpyenv checks.** For each library, rpyenv compiles and links a small program. It
also checks the versions below, and that CA certificates exist. The `tkinter` check needs
no display.

| Library | Version |
|---|---|
| OpenSSL | 1.1.1 or later for Python 3.10 and later. 1.0.2 or later for 3.7 to 3.9 |
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
| `i` | Runs the command through `sudo` (no `sudo` when you are root, and no `-y`), then checks again |
| `b` | Builds the missing libraries from source, without root, and links them into Python. Offered for CPython 3.7 and later when the compiler, `make`, `patch` and `awk` are there. It says what it can't build here and why |
| `c` | Builds without the missing libraries and lists them at the end. Offered only when the compiler, `make`, `patch` (when the definition needs it), `awk`, OpenSSL and zlib are all there |

When rpyenv can't use `sudo`, or doesn't know your distribution, the prompt omits `i` and
gives the reason. rpyenv knows apt, dnf, zypper and pacman. Alpine comes later.

On RHEL, Rocky, Alma and CentOS Stream, some `-devel` packages are in the CRB repository
(PowerTools on version 8), which is off by default. The prompt, the printed list and the
summary at the end of a script's run give the command that enables it:

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
| `prebuilt` | Installs a python-build-standalone build instead of compiling (see Prebuilt builds). Never the default |

**Scripts that set neither.** When there is no terminal, no `--deps` and no
`RPYENV_BUILD_DEPS`, rpyenv treats the run as `--deps=system`. Anything missing stops the
build. The report names the packages. Where `--deps=build` can run, the report names it too.
(Before M8c rpyenv only warned, as upstream does.) CPython 2.x keeps the earlier check. For
it, a missing optional library is still only a warning.

**After the build.** rpyenv imports every standard-library module, checks that CA
certificates exist, and starts Tcl. If one fails, it deletes the build and tells you
which module and which package. For missing CA certificates, it names the file and the
directory the built Python looked in. After `[c]` this is a short summary, and the build
stays. With `RPYENV_SKIP_PREFLIGHT=1` it is a one-line warning, and the build stays.
`RPYENV_SKIP_PREFLIGHT=1` also skips the check before the build.

### Building libraries without root

`pyenv install --deps=build`, or `b` at the prompt, compiles each missing library from source
and links it into Python statically. rpyenv installs nothing on the system.

- **Where:** `$PYENV_ROOT/cache/deps/<library>-<version>-<hash>/`. Another Python version that
  needs the same library, version and options reuses it.
- **Only what is missing:** rpyenv builds only the libraries the system lacks. The libraries
  the system has keep getting its updates.
- **It is a cache.** Deleting `cache/deps` breaks no installed Python. The next build compiles
  again. (The OpenSSL folder below is elsewhere, in `$PYENV_ROOT/.rpyenv/ssl/`. Keep that one.)
- **Tcl/Tk scripts:** when rpyenv builds a Python's Tcl/Tk this way, that Python reads Tcl's and
  Tk's scripts from `$PYENV_ROOT/.rpyenv/tcltk/`. So does every virtual environment made from
  it. Keep that folder. If you delete it, `pyenv install` and `pyenv rehash` re-create it from
  the installed versions. `pyenv uninstall` deletes what no installed version uses.
- When rpyenv creates `cache/deps`, it also creates `$PYENV_ROOT/cache`. That enables the
  download cache for later installs, as creating it by hand does.
- **Versions:** rpyenv pins each library's version and SHA-256 and checks every download. A
  Python version whose definition pins OpenSSL or readline gets that version.
- **What the system still needs:** a C compiler, `make`, `patch` and `awk` (`gawk`).
  Tumbleweed's base image has no `awk`. OpenSSL and libxcrypt also need Perl with its core
  modules (`perl-core` on Fedora). Tk uses the system's X11 headers when it has them. Without
  them, rpyenv builds the X11 client libraries too. This takes about 35 s, plus 35 to 60 s for
  a minimal Python from the same tarball that runs xcb's code generator. Both times were
  measured on one machine. A version whose definition gets CPython with git (the `-dev`
  definitions, such as `3.14-dev`) can't build X11 that way. It needs the headers.
- **X11 locale data:** a self-built X11 reads its locale data from `/usr/share/X11/locale` when
  the system has it. Without that folder (package `libx11-data` or `libX11-common`), Tk windows
  open and ASCII typing works. But characters such as `ñ` and `é` don't reach Tk. Install that
  package to type them.
- **`PYENV_ROOT`** must have no space, colon or shell character in its path.
- **CA certificates:** a self-built OpenSSL gets a private folder, `$PYENV_ROOT/.rpyenv/ssl/`,
  with no `openssl.cnf`. (A distribution writes its config for its own OpenSSL. Tumbleweed's
  config broke `ssl` in a vanilla OpenSSL.) The folder's `cert.pem` and `certs` link to the
  system's certificates, so the distribution's updates still apply. When the system has none,
  `cert.pem` is a bundle rpyenv pins (curl's copy of Mozilla's list). `pyenv install` keeps
  the links and the bundle current. It replaces the bundle when an rpyenv upgrade pins a newer
  one. rpyenv itself also downloads without system certificates. It then trusts Mozilla's list
  compiled into it, and still checks every download's SHA-256. The folder is rpyenv's: it
  replaces `cert.pem` when it needs to, so don't edit it. To trust your own certificates, set
  `SSL_CERT_FILE` (or `SSL_CERT_DIR`) for the Python you run.
- **Updates:** each version built this way lists its libraries in `versions/<v>/.rpyenv-deps`.
  When an rpyenv upgrade pins a newer version of one of them, `pyenv install` prints one line
  per affected Python version, with the command that rebuilds it.

### Prebuilt builds

`pyenv install --deps=prebuilt <version>`, `RPYENV_BUILD_DEPS=prebuilt`, or `s` at the prompt
downloads a CPython build made by the python-build-standalone project instead of compiling.
It needs no compiler and no root. rpyenv never chooses it for you. The prompt offers `s` only
when rpyenv has a build of that version for your machine (x86_64 or aarch64, glibc). The
prompt then says that it is a third-party build, and gives its date and its bundled OpenSSL
and Tk versions.

**Trust.** python-build-standalone is not part of CPython. Before a build enters rpyenv's table,
a weekly job checks two things: the build's SHA-256 against the one the release publishes, and
the release's GitHub attestation. `pyenv install` then checks the download against the SHA-256
in the table. rpyenv uses only releases from 2025-03-11 on. That release is the first with
attestations.

**Which versions.** The option works for CPython 3 versions that rpyenv knows as built-in
definitions and for which the table has a build. This includes free-threaded versions
(`3.13.12t`). `PyPy`, conda and CPython 2 ignore the option. The option refuses:

- `-g`
- `-p`
- a definition file of your own
- a version folder whose path has a space, colon or shell character
- a system without glibc. The builds need `/lib64/ld-linux-x86-64.so.2` or
  `/lib/ld-linux-aarch64.so.1`. Alpine and other musl systems, and NixOS without nix-ld, don't
  have it. On such a system the prompt has no `s`.
- a version with no build in the table

**What the build lacks.** It has no `dbm.gnu` and no `nis`. The check after the install skips
these two. It requires every other standard-library module, CA certificates, and Tcl in a
virtual environment. Builds from 2026 ship Tk 9.0, and some older Tk programs behave
differently under it. rpyenv prints a line when the build has Tk 9. The check after the install
runs `import tkinter` and `tkinter.Tcl()` on every install. So a build that needs X11 libraries
the system lacks fails there, and rpyenv does not install it. With `RPYENV_SKIP_PREFLIGHT=1`,
rpyenv keeps it, with a warning. What was measured: on one build (3.13.16 for x86_64, release
20261013), `readelf -d` reports that Tk links no X11 library of the system. Also, 3.13.16 for
x86_64 (release 20261020) was installed in an `ubuntu:24.04` container, which has no X11
libraries. It failed the check only for the missing CA certificates, so `tkinter` and Tcl
worked there. Other builds were not examined. Opening a window still needs a display.

**Compiler settings.** python-build-standalone made the builds with `clang` in `/install`.
After unpacking, rpyenv rewrites `_sysconfigdata_*.py`, `python3.*-config` and the `Makefile`:

- `clang` becomes `cc`
- `clang++` becomes `c++`
- `/install` becomes the version's folder

A C extension built with `pip` then uses the system's compiler.

**Newer builds.** rpyenv records the build in `versions/<name>/.rpyenv-deps`. When a later
rpyenv has a newer build of that version in its table, any `pyenv install` prints a line
naming both dates. To reinstall:

```
pyenv install -f --deps=prebuilt 3.13.5
```

python-build-standalone rebuilds only the newest patch of each minor version, so an older
patch (for example 3.13.5 once 3.13.6 is out) never gets a newer build. When the table has a
later patch of the same minor (and the same kind: free-threaded or not), the line names that
patch instead. The line gives its date and OpenSSL version, and the command that installs it
next to the old one:

```
pyenv install --deps=prebuilt 3.13.6
```

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

**rpyenv leaves your active conda env alone.** The installer and its `conda install pip`
step run without the variables that `conda activate` sets (`CONDA_PREFIX` and the rest).
They also run without Python's path variables. So an env you have active is never the
target. Upstream pyenv passes these variables to them. conda's settings, such as `CONDA_CHANNEL_ALIAS` for a
company mirror or `CONDA_SSL_VERIFY`, still apply.

## Conda on Windows

Conda on Windows is new in v0.2.0.

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
- **rpyenv leaves your active conda env alone**, as on Linux. The installer and its steps
  run without the variables that `conda activate` sets. conda's settings, such as
  `CONDA_CHANNEL_ALIAS`, still apply.
- **Reinstall.** `pyenv install -f` keeps the install's `envs\` folder.

### The version table

rpyenv keeps a table of the conda versions it can install.

- **After you install rpyenv**, the table that the MSI ships is already there. So
  `pyenv install <conda name>`, `pyenv install --list` and `pyenv latest -k` work
  at once, without `pyenv update`.
- **`pyenv update`** rebuilds a newer table from the publishers' pages and prints
  `conda: <n> installable versions (<k> new)`. It adds new releases only.
- **If a publisher changes an installer** that rpyenv already pinned, `update` keeps
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

If your pyenv folder has a space, do one of these:

- Use a `miniconda3` version.
- Set `PYENV_ROOT`, `PYENV` and `PYENV_HOME` to a folder without spaces.

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
