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
