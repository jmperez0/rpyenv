# pyenv behavioral reference: rpyenv M2 installer (Linux, bash)

Parity target for rpyenv's installer on Linux: `pyenv install`, `pyenv install --list`, `pyenv uninstall`, the
known-mode use of `pyenv latest`, and the parts of `python-build` that a CPython source build needs. This file records
facts only. Each fact carries an evidence tag.

How `file:line` paths are written:
- By default a path is relative to `plugins/python-build/` at the pinned commit below. So `bin/…`, `share/…` and
  `test/…` mean the plugin's files, and `test/pyenv.bats` is `plugins/python-build/test/pyenv.bats`.
- Paths starting with `libexec/` or `pyenv.d/`, and paths written `<root>/…`, are relative to the pyenv repository
  root. Example: `<root>/test/latest.bats`.
- `default-packages:<path>` is a file of the pyenv-default-packages plugin.
- `parity/…` is a file in this (rpyenv) repository.

Companion to `pyenv-m1-reference.md` (abbreviated **M1** below), which already covers the dispatcher, hooks,
`pyenv rehash`, `pyenv versions` and the full `pyenv latest` algorithm. This file cross-references M1 instead of
repeating it.

## Source, version, and evidence method

- **Source.** GitHub `pyenv/pyenv`, commit `ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2` (2.8.6), the same pin as M1. A
  full unpacked copy of that commit, including `plugins/python-build`, `plugins/pyenv-binary` and `plugins/pyenv-link`,
  was read at `/home/jm/m1cb-upstream/pyenv-ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2` in WSL Debian.
- **Versions.** `bin/python-build:17` sets `PYTHON_BUILD_VERSION="2.8.6"`. `libexec/pyenv---version:15` sets `2.8.6`
  (M1).
- **pyenv-default-packages.** `github.com/pyenv/pyenv-default-packages` does not exist: `gh api
  repos/pyenv/pyenv-default-packages` returned HTTP 404 on 2026-10-03. The plugin lives at
  `github.com/jawshooah/pyenv-default-packages`. Its latest commit, and the one read here, is
  `fb6f71a0288b5aec5723b6118aa0ca02e2cc9a23` (2015-12-19, "Merge remote-tracking branch 'rbenv-origin/master'"),
  fetched with `gh api repos/jawshooah/pyenv-default-packages/contents/<path>?ref=<sha>`. Its paths are written
  `default-packages:<path>:<line>`.

Evidence tags:

| Tag | Meaning |
|---|---|
| `[src f:l]` | Derived by reading the code at that line. Not executed by itself. |
| `[test f:l]` | A value asserted by upstream's own bats suite. |
| `[probe]` | Observed by executing the pinned scripts. See the next paragraphs. |
| **UNCONFIRMED** | Not established by any of the above. |

**Probe environment.** The probes did **not** run in a Debian trixie container. They ran directly on the WSL2 Debian
host (`/etc/os-release`: "Debian GNU/Linux forky/sid"; kernel 6.18.40.1-microsoft-standard-WSL2), as the unprivileged
user `jm`, because that host already had every CPython build dependency installed. Tools: GNU bash 5.3.3, curl 8.18.0,
GNU Wget 1.25.0, GNU Make 4.4.1, GNU tar 1.35, GNU patch 2.8, GNU coreutils 9.7, gcc 15.2.0, `shasum` present, no
`aria2c`, `lsb_release` present (it prints `Debian n/a`). The machine has 32 CPUs. WSLg sets `DISPLAY=:0`, and the
probes did not unset it, which matters for the tkinter check in `verify_py*`.

- The pinned tree was copied to `/home/jm/m2ref-linux/pyenv`, where `bin/pyenv -> ../libexec/pyenv` and there is no
  `.git` directory.
- Each probe exported `HOME=/home/jm/m2ref-linux/home`, `PYENV_ROOT=/home/jm/m2ref-linux/root`,
  `TMPDIR=/home/jm/m2ref-linux/tmp`, `LANG=C.UTF-8` and `PATH=/home/jm/m2ref-linux/pyenv/bin:/usr/local/bin:/usr/bin:/bin`.
  It unset `PYENV_VERSION`, `PYENV_DIR`, `PYENV_DEBUG` and every build-flag variable this file names.
- Stdout and stderr were captured separately and shown with `cat -A`, so `$` marks each end of line. `[probe, tty]`
  marks a run under `script -qec` on a pseudo-terminal. There, the two streams are merged and every line ends in
  `^M$` because the tty translates newlines.
- In quoted output, `<root>` is `PYENV_ROOT`, `<tmp>` is `TMPDIR`, and `<seed>` is the `YYYYmmddHHMMSS.<pid>` stamp.

**Real builds** [probe]:
- `pyenv install 3.12` ran into a `PYENV_ROOT` that did not yet exist. It resolved to 3.12.14, downloaded it from
  python.org and built it. Exit 0. Every module in `ssl sqlite3 bz2 lzma readline ctypes curses zlib tkinter` then
  imported.
- Two timed `pyenv install -f 3.12.14` runs used a `$PYENV_ROOT/cache` directory. Run 1 downloaded and cached the
  tarball, built and installed in **64.8 s**. Run 2 hit the cache and took **59.2 s**. Both used `make -j 32`.
- A mistake in a probe script caused three more real installs. Their output is used below: a debug build
  (`pyenv install -g 3.12:dbg`) and two PyPy binary installs (`pypy3.10:latest` and `pypy3.10`).

**Fake builds** [probe]: cheap local definitions with `file://` URLs exercised every python-build path except a real
compile:
- a trivial `pkg-1.0` tarball installed with the `copy` step;
- `Python-3.12.99`, whose `configure` logs its argv and environment, writes a Makefile, and installs a stand-in
  `python3.12` script that can be told to fail chosen `import` checks;
- `Python-3.12.98`, whose `configure` prints `configure: error: no acceptable C compiler found in $PATH` and fails;
- `Python-3.12.97`, whose `make install` sleeps 20 s, used for the Ctrl+C probe;
- a `MAKE` wrapper that logs make's argv.

The definition files are named after the version (`3.12.99`) so that python-build's version-derived logic applies.

**Harness check** [probe]: the upstream python-build bats suite (`plugins/python-build/test/*.bats`, 14 files) ran
with Bats 1.11.1 on the same host: **165 tests, 165 ok, 0 not ok, 0 skipped**. The per-file `@test` counts sum to the
same 165. Upstream's Makefile pins Bats v1.10.0 [src <root>/Makefile:1]. The M1 run already covered
`<root>/test/latest.bats`.

**DSL counts** were computed mechanically by `C:\tmp\m2ref_linux\count_dsl.py` over every file in
`share/python-build/` whose name starts with a digit: the 292 "CPython definitions". Forks with a name prefix
(`nogil-`, `cinder-`, `stackless-`) are excluded. The script does the following:
1. Splits each file into simple commands, on newlines, `;`, `&&` and `||`.
2. Strips the leading `if`, `then`, `else`, `elif`, `!` and `fi`.
3. Takes the first word as the function called.
4. For `install_*` calls, parses the arguments with `shlex`, skips the fetch arguments, and counts the remaining words
   as build steps, or as `--if` conditions after `--if`.

A count is the number of **files** that use the function at least once.

**Cross-check.** A second method, `grep -l`/`grep -lw <word> [0-9]*`, was run for `install_package`, `install_git`,
`has_tar_xz_support`, the three `prefer_openssl*`, `source`, `require_gcc`, `standard`, `mac_openssl`, `mac_readline`,
`ensurepip`, `ensurepip_lt21`, `copy_python_gdb` and the `python` step. It gave the same numbers. The per-version
`verify_pyXY` counts were checked only through their sum, which equals the 263 `install_package` files.

---

## `pyenv install` — `plugins/python-build/bin/pyenv-install`

### Help block

`pyenv install -h`, `pyenv install --help` and `pyenv help install` print the following to stdout and exit 0
[probe]. The text is rendered from the comment block [src bin/pyenv-install:1-33] by `pyenv-help` (M1). The output
ends with one empty line.

```text
Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...
       pyenv install [-f] [-kvp] <definition-file>[:<alias>]
       pyenv install -l|--list [--bare]
       pyenv install --version

  -l/--list          List all available versions
  -f/--force         Install even if the version appears to be installed already
  -s/--skip-existing Skip if the version appears to be installed already

  python-build options:

  -k/--keep          Keep source tree in $PYENV_BUILD_ROOT after installation
                     (defaults to $PYENV_ROOT/sources)
  -p/--patch         Apply a patch from stdin before building
  -v/--verbose       Verbose mode: print compilation status to stdout
  --version          Show version of python-build
  -g/--debug         Build a debug version

  Append `:<alias>' to a version to install it under a custom name, so that
  several builds of the same version can coexist:

      pyenv install 3.12.0:my-3.12

  This installs into $PYENV_ROOT/versions/my-3.12.

For detailed information on installing Python versions with
python-build, including a list of environment variables for adjusting
compilation, see: https://github.com/pyenv/pyenv#readme

```

`pyenv help --usage install` prints only the four `Usage:` lines and exits 0 [probe].

### Option parsing

- Options are split by `parse_options`, which `pyenv-install` gets from `eval "$(python-build --lib)"`
  [src bin/pyenv-install:61] [src bin/python-build:30-59]:
  - Every argument that starts with `-` is an option, wherever it appears. Any other argument is a positional
    argument.
  - `--name` gives the option `name`.
  - `-abc` gives the three options `a`, `b` and `c`. [probe]: `pyenv install -sfv 3.12.14` was accepted.
  - A version string can never start with `-`.
  - `--` gives an empty option name, which falls to the usage error. This follows from
    [src bin/python-build:38-39] [src bin/pyenv-install:121-123] and was not probed.
- All options are processed in order before anything else happens [src bin/pyenv-install:89-125]:

| Option | Effect | Evidence |
|---|---|---|
| `-h`, `--help` | Prints the help to stdout and exits 0 at once. | [src bin/pyenv-install:91-93] [test test/pyenv.bats:311-317] |
| `--bare` | Sets `BARE=1`. Without `-l` it does nothing, so `pyenv install --bare` behaves like `pyenv install` with no arguments. [probe]: usage on stderr, exit 1. | [src bin/pyenv-install:94-96] |
| `-l`, `--list` | Sets `LIST=1`. See `pyenv install --list`. | [src bin/pyenv-install:97-99] |
| `-f`, `--force` | Sets `FORCE=true`. | [src bin/pyenv-install:100-102] |
| `-s`, `--skip-existing` | Sets `SKIP_EXISTING=true`. When `-s` and `-f` are both given, `-s` wins for an existing version. [probe]: `-sfv 3.12.14` with 3.12.14 present exited 0 silently. | [src bin/pyenv-install:103-105,213-226] |
| `-k`, `--keep` | Sets `PYENV_BUILD_ROOT=$PYENV_ROOT/sources`, unless `PYENV_BUILD_ROOT` is already non-empty. | [src bin/pyenv-install:106-108] |
| `-v`, `--verbose` | Passes `-v` to python-build. A non-empty `PYENV_DEBUG` also sets it. | [src bin/pyenv-install:86,109-111] |
| `-p`, `--patch` | Passes `-p` to python-build. | [src bin/pyenv-install:112-114] |
| `-g`, `--debug` | Passes `-g` to python-build and appends `-debug` to `VERSION_NAME`. | [src bin/pyenv-install:115-117,205] |
| `--version` | Runs `exec python-build --version`. [probe]: prints `python-build 2.8.6` to stdout and exits 0. In a git checkout whose remote matches `/pyenv`, it prints `python-build <git describe --tags HEAD, leading v removed>` [src bin/python-build:2460-2468] [test test/version.bats:8-38]. | [src bin/pyenv-install:118-120] |
| any other | Prints the help to **stderr** and exits **1**. [probe]: `-x` and `--bogus`. Because options are handled in order, `-x --help` fails and `--help -x` succeeds. | [src bin/pyenv-install:121-123] |

- `--complete` must be the first argument. It prints `--bare --list --force --skip-existing --keep --patch --verbose
  --version --debug`, one per line, and then execs `python-build --definitions` [src bin/pyenv-install:47-58]
  [test test/pyenv.bats:276-299] [probe]. Only the long spellings are listed. `--help` is not in the list.

### Plugin definition directories

Before doing anything else, including `--complete`, `pyenv-install` appends `:<dir>` to `PYTHON_BUILD_DEFINITIONS`
for every `$PYENV_ROOT/plugins/*/share/python-build` directory, in glob order, and exports the variable
[src bin/pyenv-install:39-44]. Without any plugin the value is empty [test test/pyenv.bats:230-239]. With the plugins
`foo` and `bar` it is `:<root>/plugins/bar/share/python-build:<root>/plugins/foo/share/python-build`, with a leading
`:` [test test/pyenv.bats:241-256].

- [probe]: a definition `3.12.99` placed in `$PYENV_ROOT/plugins/fake/share/python-build` appeared in
  `pyenv install --list` and won the prefix resolution `pyenv install 3.12 → 3.12.99`. A standalone
  `pyenv latest -k 3.12` still printed `3.12.14`, because `pyenv-latest` does not add plugin directories.

### Which versions get installed

1. **Arguments.** The positional arguments, in order [src bin/pyenv-install:143].
2. **No arguments.** `DEFINITIONS=($(pyenv-local 2>/dev/null || true))`, word-split [src bin/pyenv-install:144].
   - Only the **local** `.python-version` counts, found by `pyenv local` (M1). `PYENV_VERSION` and the global
     `$PYENV_ROOT/version` file are ignored. [probe]: with `PYENV_VERSION=3.12.14`, or with a global file containing
     `3.12.14`, `pyenv install` printed the usage to stderr and exited 1.
   - [probe]: a local file containing `3.11.15` and `3.12.14`, both already installed, gave two "already exists"
     lines in file order. [test test/pyenv.bats:130-144] asserts only exit 0.
3. **Still nothing.** The help goes to stderr and the exit code is 1 [src bin/pyenv-install:145] [probe]. When
   `pyenv-help` is stubbed to print nothing, the output is empty and the exit is non-zero
   [test test/pyenv.bats:301-309].
4. **Alias split** [src bin/pyenv-install:151-159]. In an argument that contains `:`, the text after the **last** `:` is
   the alias, unless it is exactly `latest`. The definition becomes everything before that `:`.
   - `3.4.2:3.4.2-custom` installs into `<root>/versions/3.4.2-custom` [test test/pyenv.bats:34-42].
   - Several aliases work in one run [test test/pyenv.bats:44-58].
   - `X:latest` is left whole for the hook.
   - An alias also applies to a definition **path**: `pyenv install /path/3.12.99:fresh` [probe].

### The `install` hooks and `:latest`

- `pyenv-install` defines `before_install <code>` and `after_install <code>`, which append a string of code to the
  arrays `before_hooks` and `after_hooks` [src bin/pyenv-install:164-174].
- It then `source`s every script printed by `pyenv-hooks install`, after splitting on newlines
  [src bin/pyenv-install:184-187]. M1 covers hook lookup and `PYENV_HOOK_PATH`. Plugins add
  `$PYENV_ROOT/plugins/<p>/etc/pyenv.d`.
- The scripts run **once**, before the version loop, and can rewrite the `DEFINITIONS` array.
- The `before_hooks` code is `eval`ed for every version just before python-build runs. The `after_hooks` code is
  `eval`ed just after python-build returns, whether it succeeded or failed, and before the rehash or cleanup
  [src bin/pyenv-install:290,322].
- A hook can read `VERSION_NAME`, `PREFIX`, `DEFINITION`, `VERSION_ALIAS` and `STATUS`.

[probe] A hook that echoes from both sides printed this on **stdout**:

```text
before_install: VERSION_NAME=goodsum-1.0 PREFIX=<root>/versions/goodsum-1.0 STATUS=0
after_install: VERSION_NAME=goodsum-1.0 PREFIX=<root>/versions/goodsum-1.0 STATUS=0
```

After a download failure, `after_install` saw `STATUS=1`. [test test/hooks.bats:11-32] asserts the order
`before: <prefix>`, then python-build's output, then `after: 0`, then the rehash.

**`pyenv.d/install/latest.bash`**, shipped with pyenv and on the hook path by default (M1), resolves `:latest`
[src pyenv.d/install/latest.bash:1-25]. For each `DEFINITIONS` entry that contains `:`:

- `DEFINITION_PREFIX` is the text before the **first** `:`. `DEFINITION_TYPE` is `DEFINITION_PREFIX` up to its first
  `-` [src pyenv.d/install/latest.bash:6-7].
- The candidates are `python-build --definitions | grep -F "<prefix>" | grep "^<type>"`. Note that `<type>` is used
  as a **regex**, without escaping. The pipeline then deletes names that match `-dev$`, `-src$`, `(b|rc)[0-9]+$` or
  `[0-9]+t$`, and sorts with `sort -t. -k1,1r -k 2,2nr -k 3,3nr`. The **first** line wins
  [src pyenv.d/install/latest.bash:9-16].
- The entry is replaced by the winner, or by an **empty string** if nothing matched [src pyenv.d/install/latest.bash:16-22].

This differs from `pyenv latest -k` (M1) in five ways. Each was observed:

| Difference | `:latest` hook | `pyenv latest -k` | Probe |
|---|---|---|---|
| Prefix boundary | none: `grep -F` substring plus `^<type>` | must end at `-` or `.` | `pyenv install 3.1:latest` resolved to **3.14.7**, while `pyenv latest -k 3.1` gives `3.1.5`. |
| Sort keys | 3 keys, no 4th, no locale pinning | 4 keys | `pyenv install pypy3.10:latest` installed **pypy3.10-7.3.12**, because the keys tie and the whole line breaks the tie in ascending order. `pyenv latest -k pypy3.10` gives `pypy3.10-7.3.19`. |
| Alpha releases | `a[0-9]+` **not** removed | removed | No CPython alpha definition exists at this pin. The only `a<digits>` name is `pypy3.3-5.2-alpha1` [mechanical: `ls \| grep -E 'a[0-9]+t?$'`]. |
| `-latest` names | not removed | removed | follows from [src pyenv.d/install/latest.bash:13] |
| No match | empty definition, so python-build prints its usage and the exit code is 1 | error message, exit 1 | `pyenv install 3.15:latest` (the only 3.15 release is `3.15.0rc2`) and `pyenv install 3.13t:latest`. Both printed **python-build's** usage to stderr and exited 1. |

Also [probe]: `3.12:latest` → 3.12.14, and `3:latest` → 3.14.7. The hook's output then still goes through the prefix
resolution below, which leaves an exact name unchanged. `test/pyenv.bats:110-128` ("install resolves :latest") asserts
only exit 0. Its heredoc is ignored, because `assert_success` reads no stdin.

### Per-version loop

For each definition `i`, in argument order [src bin/pyenv-install:189-332]:

1. **Prefix resolution.** `DEFINITION="$(pyenv-latest -f -k "$DEFINITION")"` [src bin/pyenv-install:198].
   - Known mode: the candidates are every definition name, including those from plugin directories.
   - `-f` means that a failed match leaves the argument unchanged and still succeeds (M1 `pyenv latest`).
   - [probe]: `3.12` → `3.12.14`, `3` → `3.14.7`, `3.13t` → `3.13.15t`, `pypy3.10` → `pypy3.10-7.3.19`.
   - Because prereleases are excluded, `3.15` matches nothing and stays `3.15`. python-build then reports
     `definition not found: 3.15`.
   - Exception: with the `t` suffix, a prerelease free-threaded build survives. `pyenv latest -k 3t` prints
     `3.15.0rc2t` and `3.15t` prints `3.15.0rc2t` [probe], so `pyenv install 3t` would build a release candidate.
   - [test test/pyenv.bats:90-107] stubs `pyenv-latest -r -k`, which the code never passes, and asserts only exit 0.
     It therefore does not pin this step.
2. **Names** [src bin/pyenv-install:204-206]:
   - `VERSION_NAME` is the basename of `DEFINITION`, so a definition-file path installs under its file name.
   - `-g` appends `-debug`.
   - `PREFIX` is `$PYENV_ROOT/versions/${VERSION_ALIAS:-$VERSION_NAME}`.
   - With `-g` and an alias, the alias wins and gets **no** `-debug` [probe: `pyenv install -g 3.12:dbg` installed
     into `<root>/versions/dbg`].
3. **`PREFIX_EXISTS`** is set to 1 if `PREFIX` is a directory [src bin/pyenv-install:208].
   - It is **never reset** for later arguments.
   - See "Failure, cleanup and exit codes".
4. **Already installed?** The test is whether `$PREFIX/bin` is a directory, not merely `$PREFIX`
   [src bin/pyenv-install:212].
   - **Neither `-f` nor `-s`.** The script writes `pyenv: <PREFIX> already exists` to stderr, then runs
     `read -p "continue with installation? (y/N) "` [src bin/pyenv-install:214-215].
     - Only the exact replies `y`, `Y`, `yes` and `YES` continue [src bin/pyenv-install:217-218].
     - Any other reply sets this version's status to 1 and **continues with the next argument**
       [src bin/pyenv-install:219].
     - The prompt text is printed (to stderr) **only when stdin is a terminal**. That is bash's `read -p` behavior.
       [probe]: absent with piped input, present on a tty.
     - **EOF on stdin** (for example `< /dev/null`) makes `read` fail under `set -e`. The **whole run** then ends
       with exit 1, and later arguments are not attempted. [probe]: `pyenv install <existing> <other> < /dev/null`
       printed one "already exists" line and installed nothing.
   - **`-s` given.** The version is skipped silently, with status 0 [src bin/pyenv-install:221-226] [probe].
   - **`-f` given, or the reply was yes.** The build runs into the **existing** prefix. Nothing is removed first
     [src bin/pyenv-install:212-227].
   - A prefix directory without `bin/` is not "installed", so it gets no prompt [src bin/pyenv-install:212].
5. **Keep.** If `PYENV_BUILD_ROOT` is non-empty, the loop exports
   `PYTHON_BUILD_BUILD_PATH=$PYENV_BUILD_ROOT/${VERSION_ALIAS:-$VERSION_NAME}` and passes `-k`
   [src bin/pyenv-install:230-233]. This happens even without `-k`: [probe] `PYENV_BUILD_ROOT=/x pyenv install …`
   left `/x/3.12.99/Python-3.12.99` and the tarball behind.
6. **Cache.** If `PYTHON_BUILD_CACHE_PATH` is empty and `$PYENV_ROOT/cache` is a directory, the loop exports
   `PYTHON_BUILD_CACHE_PATH=$PYENV_ROOT/cache` [src bin/pyenv-install:237-239]. Nothing creates that directory.
7. **Bootstrap Python** (`PYENV_BOOTSTRAP_VERSION`) [src bin/pyenv-install:241-287]:
   - **Unset, and `VERSION_NAME` starts with `2.` or `3.`.** The loop tries `pyenv-whence python<v>` for `<v>` in
     `${VERSION_NAME%-dev}`, `${VERSION_NAME%.*}` and `${VERSION_NAME%%.*}`, in that order. It takes the first
     installed version that is not `anaconda*` or `miniconda*`.
   - **`pypy*-dev` or `pypy*-src`.** The loop needs `PYENV_RPYTHON_VERSION`, or the newest installed `2.7*` or
     `pypy2.7*` version that has `pycparser`. Otherwise it exits 1 with one of these messages on stderr:
     - ``pyenv-install: <name>: PyPy requires `pycparser' in <v> to build from source.``
     - `pyenv-install: <name>: PyPy requires Python 2.7 to build from source. You can specify the version to use by setting the PYENV_BOOTSTRAP_VERSION environment variable.`
   - **If found**, the loop exports `PYENV_VERSION=<bootstrap>` for python-build. This version leaks into the
     `after_install` hooks and into `pyenv-rehash`.
8. **`before_install` hooks** run [src bin/pyenv-install:290].
9. **python-build** runs as `python-build $KEEP $VERBOSE $HAS_PATCH $DEBUG "$DEFINITION" "$PREFIX"`, with each flag
   empty or `-k`/`-v`/`-p`/`-g`. A failure records `STATUS` and raises `COMBINED_STATUS` to the maximum so far
   [src bin/pyenv-install:293-294].
10. **Status 2** (definition not found). The hint below is written to stderr [src bin/pyenv-install:297-319].
11. **`after_install` hooks** run [src bin/pyenv-install:322].
12. **On status 0**, the loop runs `pyenv-rehash`, which is silent (M1). **On any other status**, it runs `cleanup`
    and then `break`s, so the remaining arguments are **not attempted** [src bin/pyenv-install:325-330].
    [probe]: `pyenv install <failing> <good>:other` built only the first and exited 1. [test test/pyenv.bats:76-87]
    shows only the first failure line.
13. After the loop, the script runs `exit "$COMBINED_STATUS"` [src bin/pyenv-install:335].

### Unknown version

python-build first prints `python-build: definition not found: <name>` to stderr and exits 2
[src bin/python-build:2553-2556] [test test/definitions.bats:62-66]. `pyenv-install` then appends the following to
**stderr** [src bin/pyenv-install:297-319]:

- `The following versions contain` block: present only if `python-build --definitions | grep -F <DEFINITION>` is
  non-empty. `<DEFINITION>` is the name after prefix resolution.
- Upgrade hint, in priority order:
  1. `brew --prefix` succeeds and `$_PYENV_INSTALL_PREFIX/` starts with `<brew prefix>/`: `:` +
     blank line + `  brew update && brew upgrade pyenv` [test test/pyenv.bats:208-228].
  2. `$_PYENV_INSTALL_PREFIX/.git` is a directory: `:` + blank line + `  cd <prefix> && git pull && cd -`
     [test test/pyenv.bats:179-206].
  3. Otherwise: `.`

[probe] `pyenv install 3.15.0` (exit **2**):

```text
python-build: definition not found: 3.15.0

The following versions contain `3.15.0' in the name:
  3.15.0rc2
  3.15.0rc2t

See all available versions with `pyenv install --list'.

If the version you need is missing, try upgrading pyenv.
```

[probe] `pyenv install 9.9.9` (exit 2) prints the same text without the "contain" block. The output ends with a
newline after `pyenv.`, and nothing goes to stdout.

### Output of a successful install

[probe] Real `pyenv install 3.12`, with stdin from `/dev/null` and nothing a tty. Exit 0.

stderr:

```text
Downloading Python-3.12.14.tar.xz...
-> https://www.python.org/ftp/python/3.12.14/Python-3.12.14.tar.xz
Installing Python-3.12.14...
Installed Python-3.12.14 to <root>/versions/3.12.14
```

stdout: nine lines from GNU `patch`. They come from the built-in downstream patch
`patches/3.12.14/Python-3.12.14/0001-downstream-3.12-gh-153711-Avoid-use-of-dup3-and-pipe.patch`, because the patch
step's output is not redirected (see "Patches"):

```text
patching file Misc/NEWS.d/next/macOS/2026-08-05-02-02-33.gh-issue-153711.pO9fFb.rst
patching file configure
Hunk #1 succeeded at 17614 (offset 62 lines).
...
Hunk #3 succeeded at 4952 (offset 60 lines).
```

- **Message sources.** `Downloading <package-file>...` and `-> <url>` come from [src bin/python-build:584,659].
  `Installing <package>...` comes from [src bin/python-build:873], and `Installed <package> to <PREFIX_PATH>` from
  [src bin/python-build:304]. All four go to stderr. [test test/fetch.bats:51-65] asserts these four lines exactly.
- **Cache hit.** No `Downloading`/`->` lines appear [probe]. The run shows `Installing …` and `Installed …` only.
- **Order.** The lines appear per package, in definition order. Packages skipped by `--if` print nothing. On Linux the
  openssl and readline lines of every CPython definition are skipped (see the DSL table).
- **Package file name.** `<package-file>` is `<package name>` plus the extension implied by the URL, which is not
  necessarily the URL's basename. [probe]: PyPy printed `Downloading pypy3.10-v7.3.12-linux64.tar.bz2...`.
- **Post-install.** After `Installed …`, `pyenv-install` runs only `pyenv-rehash`, which prints nothing. A
  `default-packages` plugin, if present, runs as an `after_install` hook *before* the rehash.
- **`PYENV_ROOT` creation.** `pyenv-install` never creates `PYENV_ROOT` or `versions/`. [probe]: with a nonexistent
  `PYENV_ROOT`, the install created `versions/3.12.14`, through the package's `make install`, and `shims/`, through
  the rehash. No `version` file was created. A definition that only copies (the `copy` step) uses `mkdir -p`
  [src bin/python-build:1295-1298].
- **Result layout.** [probe]: `<root>/versions/3.12.14/bin` held `2to3 2to3-3.12 idle idle3 idle3.12 pip pip3 pip3.12
  pydoc pydoc3 pydoc3.12 python python-config python3 python3-config python3.12 python3.12-config python3.12-gdb.py`.
  The unversioned `python`, `pip`, `idle`, `pydoc`, `2to3` and `python-config` symlinks come from python-build
  ("Post-build"). The shims included `python3.12-gdb.py`.
- **Recorded configure arguments.** [probe]: `sysconfig.get_config_var("CONFIG_ARGS")` was
  `'--prefix=<root>/versions/3.12.14' '--enable-shared' '--libdir=<root>/versions/3.12.14/lib'
  'LDFLAGS=-L<p>/lib -Wl,-rpath,<p>/lib' 'LIBS=-L<p>/lib -Wl,-rpath,<p>/lib' 'CPPFLAGS=-I<p>/include'`, where `<p>`
  is the prefix.

### Failure, cleanup and exit codes

- `cleanup() { [ -z "${PREFIX_EXISTS}" ] && rm -rf "$PREFIX"; }` runs on SIGINT, through `trap cleanup SIGINT`, and
  after any failed python-build [src bin/pyenv-install:177-181,328].
- When `PREFIX_EXISTS` is set, the function returns 1. Under `set -e` the script then exits **1** on the spot,
  whatever python-build's status was [src bin/pyenv-install:178,328].

| Situation | stdout / stderr | Exit | `versions/<name>` afterwards | Evidence |
|---|---|---|---|---|
| All installed or skipped | as above | 0 | installed | [probe] |
| `--list`, `-h`, `--version` | stdout | 0 | — | [probe] |
| Unknown option; no argument and no local version | help → stderr | 1 | — | [probe] |
| Prompt declined (any non-yes line) | `pyenv: <PREFIX> already exists` → stderr | 1 at the end; the run continues with the next argument | untouched | [probe] |
| EOF at the prompt | same line | 1 at once; the run stops | untouched | [probe] |
| Definition not found, fresh prefix | not-found + hint → stderr | **2** | — | [probe] |
| Definition not found, `versions/<name>/` already a directory | same text | **1** | kept | [probe] |
| Build/verify/download failure, fresh prefix | BUILD FAILED block → stderr | 1 | **removed** | [probe] |
| Same, prefix existed (for example `-f` over an install) | same | 1 | **kept, partly overwritten** | [probe] |
| Ctrl+C, fresh prefix | the ^C, no BUILD FAILED block | **130** | removed | [probe, tty] |
| Ctrl+C, prefix existed | same | **1** | kept | [probe, tty] |
| A failure after an earlier argument whose prefix existed | as above | 1 | **kept: leaks** | [probe]: `pyenv install -s <existing> <failing fresh>` left `versions/3.12.97/bin` behind |

- After Ctrl+C, the build directory and the log stay in `TMPDIR` [probe].
- With several arguments, the exit code is the maximum status recorded. A failure stops the loop (step 12).

---

## `pyenv install --list` (`-l`)

- With `-l` or `--list`, positional arguments are ignored. [probe]: `pyenv install --list 3.12` printed all 1113
  lines [src bin/pyenv-install:127-135].
- **Default form.** stdout gets `Available versions:`, then every line of `python-build --definitions` indented with
  two spaces (`sed 's/^/  /'`). Exit 0 [src bin/pyenv-install:68-75,131-132] [test test/pyenv.bats:146-161].
- **`--bare`.** The raw lines without the header or indentation [src bin/pyenv-install:128-129]
  [test test/pyenv.bats:163-177].
- **Contents.** [probe]: 1113 lines, the header plus 1112 names. That equals the 1112 regular files in
  `share/python-build/` (`find -maxdepth 1 -type f`); the `patches` directory is excluded.
  - Every family is listed: CPython (292 names starting with a digit, including `-dev` and `t` variants and
    `3.15.0rc2`), `activepython-*`, `anaconda*`, `cinder-*`, `graalpy*`, `graalpython-*`, `ironpython*`, `jython*`,
    `mambaforge*`, `micropython*`, `miniconda*`, `miniforge*`, `nogil-*`, `pypy*`, `pyston-*` and `stackless*`.
  - It also includes names from plugin `share/python-build` directories [test test/pyenv.bats:258-274].
- **Where the list comes from.** `python-build --definitions` [src bin/python-build:2475-2485]:
  1. For each directory in `PYTHON_BUILD_DEFINITIONS`, it runs `ls <dir> | grep -xv patches`. The directories are
     the colon-split `$PYTHON_BUILD_DEFINITIONS`, then `${PYTHON_BUILD_ROOT:-<install prefix>}/share/python-build`
     [src bin/python-build:2497].
  2. It pipes the result through `sort_versions` and `uniq`.
  3. `sort_versions` builds a key from each name with `sed 'h; s/[+-]/./g; s/.p\([[:digit:]]\)/.z.\1/; s/$/.z/; G;
     s/\n/ /'`, sorts on the key with `LC_ALL=C sort -t. -k 1,1 -k 2,2n -k 3,3n -k 4,4n -k 5,5n`, and prints the
     original name.
  4. Effects: `2.7-dev` sorts before `2.7`, `3.4.0` before `3.4-dev` before `3.4.1`, and `jython-dev` before
     `jython-2.5.0` [test test/definitions.bats:68-95]. [probe]: `3.12.0, 3.12-dev, 3.12.1, …, 3.12.14, 3.13.0,
     3.13.0t, 3.13-dev, 3.13t-dev, 3.13.1, 3.13.1t, …`.
  5. Duplicates are removed only when they end up adjacent [test test/definitions.bats:97-110].
  6. The sort pins `LC_ALL=C`, so the order does not depend on the locale.
- **Missing roots.** A nonexistent definitions root gives empty output, exit 0 [test test/definitions.bats:14-19].
- **`pyenv install --complete`** prints the nine options, then the same `--definitions` list [probe].

---

## `pyenv uninstall` — `plugins/python-build/bin/pyenv-uninstall`

**Help** [src bin/pyenv-uninstall:1-11]. Rendered by `pyenv help uninstall`, `-h` and `--help`: stdout, exit 0
[probe]. The output ends with one empty line.

```text
Usage: pyenv uninstall [-f|--force] <version> ...

   -f  Attempt to remove the specified version without prompting
       for confirmation. If the version does not exist, do not
       display an error message.

See `pyenv versions` for a complete list of installed versions.

```

**Parsing** [src bin/pyenv-uninstall:17-47]. Options are **positional**, not parsed by `parse_options`:

- **`--complete`** (first argument only) prints `--force`, then execs `pyenv versions --bare` [probe].
- **Help.** Only a **first** argument of `-h` or `--help` prints the help (exit 0).
- **Force.** Only a **first** argument of `-f` or `--force` sets force, and it is then shifted away.
- **No version left.** The help goes to **stderr** and the exit code is 1 [probe].
- **Arguments are checked before any removal.** Any remaining argument that is empty or starts with `-` gives the help
  on stderr and exit 1, and nothing is removed.
  - [probe]: `pyenv uninstall -f u2 --bogus u3` removed nothing.
  - [probe]: `pyenv uninstall u2 -f` gives usage, exit 1.
  - [probe]: `pyenv uninstall -f ""` gives usage, exit 1.
  - [test test/pyenv.bats:342-355] expects 127 here, because in the test `pyenv-help` is not on `PATH`.

**Hooks.** The command sources `pyenv-hooks uninstall`. `before_uninstall` and `after_uninstall` register code
[src bin/pyenv-uninstall:49-64].

**Per version, in argument order** [src bin/pyenv-uninstall:66-98]:

1. `VERSION_NAME` is the basename of the argument. `PREFIX` is `$PYENV_ROOT/versions/$VERSION_NAME`.
   - **No prefix resolution.** [probe]: `pyenv uninstall 3.11`, with 3.11.15 installed, printed
     ``pyenv: version `3.11' not installed`` and exited 1.
   - **Path components are dropped.** [probe]: `pyenv uninstall -f /some/where/u4` removed `<root>/versions/u4`.
2. **Without force:**
   - If `PREFIX` is not a directory, stderr gets ``pyenv: version `<name>' not installed`` and the script exits **1 at
     once**. Later arguments are not processed [probe]. This holds for a regular file too: [probe] `afile`.
   - Otherwise the script runs `read -p "pyenv: remove <PREFIX>? (y/N) "`. The prompt is visible only on a tty
     [probe, tty]:

     ```text
     pyenv: remove <root>/versions/u2? (y/N) y
     pyenv: u2 uninstalled
     ```

   - Only `y`, `Y`, `yes` and `YES` continue. Any other reply, or EOF, gives exit **1 at once**. The remaining
     arguments are not processed. [probe]: `printf 'y\nn\n' | pyenv uninstall u1 u2` removed u1, then exited 1.
3. `before_uninstall` hooks run. They also run for a missing version when force is set [probe].
4. **If `PREFIX` is a directory** (a symlink to a directory counts), the script runs `rm -rf "$PREFIX"`, then
   `pyenv-rehash`, then prints `pyenv: <name> uninstalled` to **stdout**.
   - A symlinked version loses only the link. [probe]: the link target directory survived.
   - The rehash dropped a shim whose binary existed only in the removed version [probe].
5. `after_uninstall` hooks run.

**Order.** [test test/hooks.bats:34-94] asserts `before: <prefix>`, then the `rm -rf <prefix>` line (from a
hook-overridden `rm`), then `rehashed`, then `pyenv: 3.6.2 uninstalled`, then `after.`, repeated for each version.

**Force on a missing version or a non-directory** prints nothing and exits 0. [probe]: `-f 9.9`, `--force 9.9`, and
`-f afile`, where the file is left in place.

| Situation | Output | Exit |
|---|---|---|
| Removed (force, or `y`) | `pyenv: <name> uninstalled` on stdout | 0 |
| Missing, no force | ``pyenv: version `<name>' not installed`` on stderr | 1, stops |
| Declined or EOF | nothing (the prompt only on a tty) | 1, stops |
| Bad argument or none | help on stderr | 1 |
| `-f a b`, where `a` is missing and `b` exists | `pyenv: b uninstalled` | 0 [probe] |

---

## `pyenv latest` — what M2 adds to M1

M1's `pyenv latest` section is complete for the algorithm, the flags (`-k/--known`, and the internal
`-b/--bypass` and `-f/--force`) and the messages. M2 adds the following:

- **There is no `-q`/`--quiet`** [src libexec/pyenv-latest:14-34]. [probe]: `pyenv latest -k -q 3.12` treated `-q` as
  the prefix. It printed ``pyenv: no known versions match the prefix `-q'`` to stderr and exited 1.
- **Known-mode results** at this pin [probe]:

| Prefix | Exit | Output |
|---|---|---|
| `3` | 0 | `3.14.7` |
| `3.12` / `3.13` / `3.9` / `3.1` | 0 | `3.12.14` / `3.13.15` / `3.9.25` / `3.1.5` |
| `3.14t` | 0 | `3.14.7t` |
| `3t` / `3.15t` | 0 | `3.15.0rc2t` (a prerelease, see M1 step 6) |
| `3.15` / `3.16` / `9` | 1 | ``pyenv: no known versions match the prefix `3.15'`` (stderr) |
| `2` / `2.7` | 0 | `2.7.18` |
| `3.12.1` / `3.13.0t` / `3.12-dev` / `miniconda3-latest` | 0 | unchanged (exact match) |
| `pypy3.10` / `pypy3.11` | 0 | `pypy3.10-7.3.19` / `pypy3.11-7.3.23` |
| `pypy` / `pypy2.7` | 0 | `pypy-5.7.1` / `pypy2.7-5.10.0` |
| `miniconda3` | 0 | `miniconda3-4.7.12` |
| `anaconda3` / `miniforge3` / `graalpy` / `jython` / `stackless` | 0 | `anaconda3-2026.07-1` / `miniforge3-26.7.2-0` / `graalpy-25.0.3` / `jython-2.7.4` / `stackless-3.7.5` |
| (none) | 1 | ``pyenv: no known versions match the prefix `'`` (stderr) |

- **Plugin definitions.** Run on its own, `pyenv latest -k` does not see plugin definition directories (see `pyenv
  install`, "Plugin definition directories").
- **Use by `pyenv install`.** `pyenv install` calls `pyenv-latest -f -k <arg>` once per argument (loop step 1).

---

## python-build — `plugins/python-build/bin/python-build`

### Invocation

**Help** [src bin/python-build:1-15]. The bare usage below is printed by `python-build` with the wrong number of
arguments. It goes to stderr with exit 1 [probe] [test test/arguments.bats:5-23]:

```text
Usage: python-build [-kpv] <definition> <prefix>
       python-build --definitions
       python-build --version

  -k/--keep        Do not remove source tree after installation
  -p/--patch       Apply a patch from stdin before building
  -v/--verbose     Verbose mode: print compilation status to stdout
  -4/--ipv4        Resolve names to IPv4 addresses only
  -6/--ipv6        Resolve names to IPv6 addresses only
  --definitions    List all built-in definitions
  --version        Show version of python-build
  -g/--debug       Build a debug version

```

- `-h`/`--help` prints `python-build 2.8.6`, an empty line, then the same block, to stdout. Exit 0 [probe]
  [src bin/python-build:2504-2508].
- **Options** [src bin/python-build:2500-2538]. They are parsed by the same `parse_options`, and **unknown options
  are ignored**:
  - `-k` sets `KEEP_BUILD_PATH`;
  - `-v` sets `VERBOSE`;
  - `-p` sets `HAS_STDIN_PATCH`;
  - `-g` sets `DEBUG` and `PYTHON_CFLAGS="-O0 ${PYTHON_CFLAGS}"`;
  - `-4` and `-6` set IPv4-only or IPv6-only;
  - `--definitions` lists and exits 0;
  - `--version` prints the version and exits 0.
- **`--lib`** must be the first argument. It prints the `lib` function source plus `lib "$1";`, which defines
  `parse_options` when evaluated [src bin/python-build:54-60].
- **Argument count.** Exactly 2 positional arguments are required, otherwise usage and exit 1
  [src bin/python-build:2540].
- **Argument 1, the definition.** An empty value gives usage, exit 1. Otherwise:
  1. A path to an existing file is used as is.
  2. Otherwise the name is looked up as `<dir>/<name>` in each definitions directory, first match wins
     [test test/definitions.bats:52-60].
  3. Otherwise stderr gets `python-build: definition not found: <arg>` and the exit code is **2**
     [src bin/python-build:2542-2557].
- **Argument 2, the prefix.** An empty value gives usage, exit 1. A relative prefix is made absolute against `$PWD`
  [src bin/python-build:2559-2564] [test test/build.bats:1425-1432].

### Environment python-build sets up (Linux)

In order [src bin/python-build:2566-2857]:

1. **`TMP`.**
   - `TMP` is `${TMPDIR%/}`, or `/tmp` if `TMPDIR` is empty.
   - The script runs `mkdir -p "$TMP"` and writes, then runs, a tiny executable `python-build-test.$$` there.
   - If `$TMP` cannot be created or written, stderr gets `python-build: TMPDIR=<TMP> is set to a non-accessible
     location` and the exit code is 1. [probe]: an unwritable `/nonexistent/dir` also printed `mkdir`'s own error line
     first. [test test/build.bats:1524-1544].
   - If the test file cannot be executed: ``python-build: TMPDIR=<TMP> cannot hold executables (partition possibly
     mounted with `noexec`)``, exit 1.
2. **`MAKE`** defaults to `make` on Linux, and to `gmake` on FreeBSD before release 10 [src bin/python-build:2592-2608].
3. **`PYTHON_BUILD_CACHE_PATH`** is kept, without a trailing `/`, only if it is an existing directory. Otherwise it is
   unset [src bin/python-build:2610-2614] [test test/cache.bats:76-88].
4. **Mirror.**
   - An empty `PYTHON_BUILD_MIRROR_URL` becomes `https://pyenv.github.io/pythons`, with `PYTHON_BUILD_DEFAULT_MIRROR=1`.
     Otherwise one trailing `/` is removed.
   - A non-empty `PYTHON_BUILD_SKIP_MIRROR` unsets the mirror.
   - If SHA-256 cannot be computed and `PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM` is empty, the mirror is unset too
     [src bin/python-build:2616-2630].
5. **Download options.** `ARIA2_OPTS`, `CURL_OPTS` and `WGET_OPTS` are `PYTHON_BUILD_ARIA2_OPTS`,
   `PYTHON_BUILD_CURL_OPTS` and `PYTHON_BUILD_WGET_OPTS`, plus the IPv4 or IPv6 switch
   [src bin/python-build:2632-2634].
6. **Configure options.** Appended to `PYTHON_CONFIGURE_OPTS_ARRAY`, in this order:
   1. `--with-pydebug` if `-g` [src bin/python-build:2637-2639].
   2. `--enable-shared`, unless `$CONFIGURE_OPTS $PYTHON_CONFIGURE_OPTS` contains `--enable-framework` or
      `--disable-shared` [src bin/python-build:2641-2643] [test test/build.bats:1350-1372].
   3. `--libdir=<PREFIX_PATH>/lib`, always [src bin/python-build:2646].
   4. If `--enable-shared` appears in `CONFIGURE_OPTS`, `PYTHON_CONFIGURE_OPTS` or the array, and `LDFLAGS` has no
      `-rpath=`, `-Wl,-rpath,<PREFIX_PATH>/lib` is **prepended** to both `LDFLAGS` and `LIBS`
      [src bin/python-build:2649-2654].
   5. `--enable-framework` and `--enable-universalsdk` on a non-mac host print `python-build: framework installation
      is not supported outside of MacOS.` (or `universal installation …`) to stderr and exit 1
      [src bin/python-build:2662-2666,2689-2693].
   6. `--enable-unicode=ucs4` for definitions named `2.*`, `3.0*`, `3.1*` or `3.2*` on a non-mac host, unless
      `PYTHON_CONFIGURE_OPTS` already has `--enable-unicode=` [src bin/python-build:2712-2724]
      [test test/pyenv_ext.bats:367-388].
7. **Cleaned environment.** `PIP_REQUIRE_VENV`, `PIP_REQUIRE_VIRTUALENV`, `PYTHONHOME` and `PYTHONPATH` are unset
   [src bin/python-build:2727-2728,2850-2851].
8. **`~/.pydistutils.cfg`.** If it exists, stderr gets `WARNING: Please make sure you remove any previous custom
   paths from your <HOME>/.pydistutils.cfg file.` The word `WARNING` is bold on a tty [src bin/python-build:2731-2735].
9. **`GET_PIP_URL`** defaults per definition name: `https://bootstrap.pypa.io/pip/<X.Y>/get-pip.py` for 2.6, 2.7,
   3.2–3.9 and PyPy/pyston equivalents, otherwise `https://bootstrap.pypa.io/get-pip.py`
   [src bin/python-build:2746-2792] [test test/pyenv_ext.bats:423-466]. A set `PIP_VERSION` prints
   `WARNING: Setting PIP_VERSION=<v> is no longer supported and may cause failures during the install process.`
   to stderr.
10. **Paths.**
    - `LOG_PATH=<TMP>/python-build.<YYYYmmddHHMMSS>.<pid>.log`.
    - `PYTHON_BIN=<PREFIX_PATH>/bin/python<X.Y>`. `<X.Y>` comes from the definition's basename, with `-dev`, `-rc*`,
      `rc*` and trailing non-digits removed, so `3.13.0t` gives `3.13`.
    - `BUILD_PATH` is `$PYTHON_BUILD_BUILD_PATH`, or `<TMP>/python-build.<seed>` [src bin/python-build:2805-2837].
11. **Log descriptor.** fd 4 is opened in **append** mode on `LOG_PATH`. With `-v`, a background
    `tail -f LOG_PATH` copies the log to stdout [src bin/python-build:2840-2845].
12. **Prefix flags.** `-L<PREFIX_PATH>/lib` is prepended to `LDFLAGS` and to `LIBS`. `-I<PREFIX_PATH>/include` is
    prepended to `CPPFLAGS` [src bin/python-build:2847-2848].
13. **Run.** The script sets `trap build_failed ERR`, runs `mkdir -p BUILD_PATH`, `source`s the definition, then
    removes `BUILD_PATH` unless `-k` [src bin/python-build:2853-2857].
    - Errors are caught by the ERR trap under `set -E`. There is no `set -e`.
    - **The log file is never deleted.** [probe]: every run, successful or not, left `<tmp>/python-build.<seed>.log`
      behind (about 2.2 MB for 3.12.14).

### The definitions DSL

A definition is a bash fragment `source`d by python-build. Packages are fetched into `BUILD_PATH`, extracted, and
built in definition order. Counts: see "Source, version, and evidence method" (292 CPython definitions).

**Functions called at the top level of CPython definitions:**

| Function | Arguments | What it does on Linux | Files |
|---|---|---|---|
| `install_package` | `NAME URL[#CHECKSUM] [STEP…] [--if COND]` | Fetches a tarball (`fetch_tarball`) into `BUILD_PATH`, extracts it to `NAME/`, then runs each `build_package_<STEP>` in that directory. No STEP means `standard`. If `COND` returns non-zero, the whole call is a no-op returning 0. Prints `Installed NAME to PREFIX` on stderr [src bin/python-build:248-305,863-880]. | 263 |
| `has_tar_xz_support` | — | True if `tar Jcf - /dev/null` works and `_PYTHON_BUILD_FORCE_SKIP_XZ` is empty. Definitions use it to choose the `.tar.xz` URL over the `.tgz` URL [src bin/python-build:676-678]. | 196 |
| `prefer_openssl3` | — | Exports `PYTHON_BUILD_HOMEBREW_OPENSSL_FORMULA` (`openssl@3 openssl@1.1 openssl`) and `PYTHON_BUILD_MACPORTS_OPENSSL_FORMULA` defaults. These are read only by the Homebrew and MacPorts code, so this is a **no-op on Linux** unless Homebrew is in use (below) [src bin/python-build:1748-1754]. | 104 |
| `prefer_openssl11` | — | Same, with `openssl@1.1 openssl` [src bin/python-build:1739-1746]. | 61 |
| `prefer_openssl3_to_4` | — | Same, with `openssl@3 openssl@4 openssl@1.1 openssl` [src bin/python-build:1756-1762]. | 8 |
| `source` | path | `3.X.Yt` files run `export PYTHON_BUILD_FREE_THREADING=1`, then `source` the non-`t` file (`"${BASH_SOURCE[0]%t}"`, or a named sibling such as `3.13.0`, `3.14-dev`). `DEFINITION_PATH` stays the `t` file, so its own `patches/<name>t/` directory applies. | 29 |
| `install_git` | `NAME GIT_URL REF [STEP…]` | `git clone --depth 1 --branch REF URL NAME`. If `NAME/` exists it instead runs `git fetch --depth 1 origin +REF` and `git checkout -q -B REF origin/REF`. Prints `Cloning URL...` on stderr. With a cache, it keeps a bare clone in the cache. If `git` is absent, stderr gets ``error: please install `git` and try again`` and the exit code is 1 [src bin/python-build:256-258,680-714] [test test/fetch.bats:84-115]. Used by the 14 `-dev` definitions. | 14 |
| `require_gcc` | — | Sets `CC` to the first of `$CC`, `command -v gcc` or any `gcc-*` on `PATH` whose `--version` does not mention LLVM. If none is found, it prints an `ERROR: This package must be compiled with GCC…` block to stderr and fails [src bin/python-build:1343-1442]. Used only by the 2.1.3–2.4.6 definitions. | 10 |
| `colorize`, `echo` | — | Only `3.0.1`: a Darwin-only warning [src share/python-build/3.0.1:4-10]. | 1 |

**Build steps used by CPython definitions** (`build_package_<step>`):

| Step | What it does | Files |
|---|---|---|
| `standard` | `standard_build` then `standard_install` (see "Build steps") [src bin/python-build:1009-1012]. | 263 |
| `mac_readline` with `--if has_broken_mac_readline` | Builds a bundled readline. **Skipped on Linux**, because the condition returns 1 when not on macOS [src bin/python-build:1673-1679]. | 263 |
| `mac_openssl` with `--if has_broken_mac_openssl` | Builds a bundled OpenSSL. **Skipped on Linux** for the same reason [src bin/python-build:1786-1788]. | 246 |
| `verify_pyXY` | Post-build checks. Per-version counts: py21 1, py22 1, py23 1, py24 7, py25 7, py26 10, py27 20, py30 1, py31 6, py32 7, py33 8, py34 12, py35 12, py36 17, py37 19, py38 22, py39 26, py310 23, py311 18, py312 16, py313 17, py314 9, py315 2, py316 1. They sum to 263. | 263 |
| `ensurepip` | Bootstraps pip (see "Post-build"). | 231 |
| `ensurepip_lt21` | Same with `-s` instead of `-I` (2.7.18 only). | 1 |
| `copy_python_gdb` | Copies `Tools/gdb/libpython.py` to `<prefix>/bin/python<X.Y>-gdb.py` when the file exists [src bin/python-build:2393-2400]. | 198 |
| `python` | `"$PYTHON_BIN" setup.py install` (setuptools and pip in the 2.4–3.1 definitions) [src bin/python-build:1019-1024]. | 28 |

**Variables that definitions export or assign:**

| Variable | Files | Effect on Linux |
|---|---|---|
| `PYTHON_BUILD_CONFIGURE_WITH_OPENSSL` | 153 | Read only in the Homebrew, MacPorts and mac-OpenSSL paths [src bin/python-build:1816,1848,1868]. No effect without Homebrew. |
| `PYTHON_CFLAGS` | 64 | Appended to `CFLAGS` for `./configure`. Examples: 3.9.x adds `-DOPENSSL_NO_SSL3`, 2.7.18 adds `-std=c99`. |
| `PYTHON_BUILD_TCLTK_USE_PKGCONFIG` | 63 | Homebrew tcl-tk only [src bin/python-build:2025]. |
| `PYTHON_BUILD_CONFIGURE_WITH_OPENSSL_RPATH` | 62 | Homebrew only [src bin/python-build:1824]. |
| `PYTHON_BUILD_FREE_THREADING` | 29 | Adds `--disable-gil` on every OS [src bin/python-build:2108-2112] [probe]. |
| `PYTHON_BUILD_HOMEBREW_OPENSSL_FORMULA`, `PYTHON_BUILD_MACPORTS_OPENSSL_FORMULA` | 6 each | Homebrew and MacPorts only. |
| `PYTHON_BUILD_CONFIGURE_WITH_DSYMUTIL` | 5 | macOS only [src bin/python-build:2102-2106] [test test/build.bats:1402-1423]. |
| `src` | 1 | `3.4.10` keeps its URL in a variable. |

**Homebrew on Linux.** `can_use_homebrew` is true on Linux in two cases [src bin/python-build:128-151]
[test test/build.bats:642-800]:
- `PYTHON_BUILD_USE_HOMEBREW` is set;
- `brew` is on `PATH` and pyenv itself is installed under `brew --prefix`.

In those cases the Homebrew OpenSSL, readline, ncurses, zlib and tcl-tk logic applies, together with the
`prefer_openssl*` and `PYTHON_BUILD_CONFIGURE_WITH_OPENSSL*` settings. `PYTHON_BUILD_SKIP_HOMEBREW` disables it.
Setting both variables gives `error: mutually exclusive environment variables PYTHON_BUILD_USE_HOMEBREW and
PYTHON_BUILD_SKIP_HOMEBREW are set` and exit 1.

**DSL functions no CPython definition calls but other families do** (from the same script run over all 1112 files):
`install_script`, `install_zip`, `require_osx_version`, `require_distro` and the PyPy and GraalPy architecture helpers.
They are out of scope for a CPython source build.

**URL `#checksum` convention.**
- **Splitting.** In `install_package`/`install_zip`/`install_jar`/`install_script`, everything after the first `#` in
  the URL is the checksum, and the URL is cut there [src bin/python-build:552-554].
- **Lengths.** 64 hex digits mean SHA-256 and 32 mean MD5. The value is lowercased before comparison
  [src bin/python-build:366-374]. Another length gives this text on fd 4 (the log), and the fetch fails:
  `unexpected checksum length: <n> (<value>)` / `expected 0 (no checksum), 32 (MD5), or 64 (SHA2-256)`
  [test test/checksum.bats:146-157] [probe].
- **Counts.** Over the 292 CPython definitions, all 1007 `install_*` lines that carry a fragment use 64 hex digits.
  Sixteen files contain a URL without one:
  - the 14 `install_git` lines;
  - `openssl-1.1.1v` in 2.7.18, which is macOS-only;
  - the `$src` indirection in 3.4.10.

  [mechanical: `C:\tmp\m2ref_linux\count_urls.py`.]
- **Hosts.** The hosts used are `www.python.org` (443 lines), `ftpmirror.gnu.org` (263, readline), `www.openssl.org`
  (194), `github.com` (64), `pypi.python.org` (56), `openssl.org` (2) and the `$src` line (1).

### Download

`fetch_tarball NAME URL` [src bin/python-build:545-603]:

1. **Checksum and mirror URL.** The checksum is split off the URL. If a checksum is present and
   `PYTHON_BUILD_MIRROR_URL` is set, the mirror URL is `<PYTHON_BUILD_MIRROR_URL>/<checksum>`. One exception: with the
   **default** mirror, a URL matching `*/www.python.org/*` gets **no** mirror. So official CPython tarballs never use
   the default mirror.
2. **Archive type.** The extension picks the archive type: `.tar.gz`, or `.tar.bz2` if the URL ends in `bz2`, or
   `.tar.xz` if it ends in `xz`. If the decompressor is missing, the log (fd 4) gets ``warning: bzip2 not found;
   consider installing `bzip2` package`` or the same for `xz`.
3. **Reuse.** If `reuse_existing_tarball` succeeds, nothing is downloaded or printed. It succeeds when:
   - `NAME.<ext>` already exists in `BUILD_PATH` and verifies; or
   - `<cache>/NAME.<ext>` exists and verifies, in which case it is symlinked in [src bin/python-build:629-645].
4. **Download.** Otherwise stderr gets `Downloading NAME.<ext>...` and the script runs
   `http head MIRROR && download_tarball MIRROR || download_tarball URL`.
5. **Extraction.** `tar xzf`, `xjf` or `xJf`. If the archive's top directory is not `NAME`, the first extracted
   directory is renamed to `NAME`. The tarball is deleted unless `-k`. The output goes to the log.

`download_tarball URL FILE CHECKSUM` [src bin/python-build:647-674]:

1. With `PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM` set, the URL is rewritten with `sed
   "s|.*//${URL_BASE:-www.python.org/ftp/python}|$PYTHON_BUILD_MIRROR_URL|g"`. The mirror then mimics the python.org
   path layout instead of `<sha>` names. The checksum is **still verified** when a SHA tool exists
   [test test/mirror.bats:75-93].
2. stderr gets `-> <url>`.
3. The script runs `http get URL FILE`, with output to the log.
   - **Failure:** stderr gets `error: failed to download <FILE>` and the call returns 1 [test test/fetch.bats:26-33]
     [probe].
   - **Success:** the checksum is verified, with its messages going to the log. A mismatch returns 1.
4. With a cache, the file is moved into the cache and symlinked back.

**Which mirror call wins.**
- A failed mirror HEAD or a bad mirror checksum falls back to the original URL [test test/mirror.bats:58-113].
- The original URL is the last command in its `&&`/`||` list, so if it fails the ERR trap fires and the run ends in
  BUILD FAILED.
- **The mirror HEAD's own output is logged.** [probe]: in a `badsum` run with the default mirror,
  `https://pyenv.github.io/pythons/<sha>` was probed and the response headers (`x-cache-hits`, `content-length: 9379`,
  …) appeared in "Last 10 log lines".

**HTTP client** [src bin/python-build:401-543]:

- `PYTHON_BUILD_HTTP_CLIENT` if set; otherwise the first of **`aria2c`, `curl`, `wget`** that `type` finds.
- With none found, stderr gets ``error: please install `aria2c`, `curl`, or `wget` and try again`` and the download
  fails.

| Client | HEAD | GET |
|---|---|---|
| curl | `curl -qsILf ${CURL_OPTS} URL` | `curl -q -o FILE -sSLf ${CURL_OPTS} [--no-silent] URL`. `--no-silent` is added when fd 3 (the original stderr) is a tty. The progress meter is then tee'd to the terminal and to the log [test test/fetch.bats:35-49] [probe, tty]. |
| wget | `wget -q --spider ${WGET_OPTS} URL` | `wget ${WGET_OPTS} -nv [--show-progress --progress=bar:force:noscroll] -O FILE URL` |
| aria2c | `aria2c --dry-run --no-conf=true ${ARIA2_OPTS} URL` | `aria2c --allow-overwrite=true --no-conf=true -d DIR -o NAME [--summary-interval=5] ${ARIA2_OPTS} URL` [test test/fetch.bats:51-82] |

- A curl or aria2c installed as a snap downloads through `$HOME` and then moves the file [src bin/python-build:453-460,502-504].
- `-4` and `-6` add `--ipv4`/`--ipv6` (curl), `--inet4-only`/`--inet6-only` (wget) or `--disable-ipv6=true|false`
  (aria2c) [src bin/python-build:2632-2634].
- [probe]: `wget` rejects `file://` URLs with `Unsupported scheme.` That limits fake probes; real users are unaffected.

### Checksum verification

`verify_checksum FILE EXPECTED` [src bin/python-build:363-399]:

- It returns success when the file does not exist or `EXPECTED` is empty.
- **SHA-256 tool**, the first available of:
  1. `shasum -a 256 -b`;
  2. `openssl dgst -sha256`, preferring `$(brew --prefix openssl)/bin/openssl`;
  3. `sha256sum -b`.
- **MD5 tool:** `md5 -q`, then `openssl md5`, then `md5sum -b` [src bin/python-build:321-351].
- **No working tool** for the needed algorithm (probed once by `echo test | <tool>`) means the file is **accepted
  without checking** [test test/checksum.bats:48-59,76-87].
- **Mismatch.** The log gets the following, and the fetch fails:

  ```text

  checksum mismatch: <FILE> (file is corrupt)
  expected <expected>, got <computed>

  ```

- **Visibility.** None of this text goes to the terminal directly. The user sees it only in BUILD FAILED's "Last 10
  log lines" [probe]:

```text
Downloading pkg-1.0.tar.gz...
-> file:///home/jm/m2ref-linux/fixtures/pkg-1.0.tar.gz

BUILD FAILED (Debian n/a using python-build 2.8.6)

Inspect or clean up the working tree at <tmp>/python-build.<seed>
Results logged to <tmp>/python-build.<seed>.log

Last 10 log lines:
<tmp>/python-build.<seed> ~

checksum mismatch: pkg-1.0.tar.gz (file is corrupt)
expected 0000000000000000000000000000000000000000000000000000000000000000, got d296f0f4bbb69c3ae9fda869fa9797ee04bf46c041967d7ac07f2bbd2e604bf8
```

That run used `PYTHON_BUILD_SKIP_MIRROR=1`. The default-mirror run of the same definition first logged the mirror's
HEAD response. Exit 1. A fresh prefix is removed.

### Cache

- **Location.** `PYTHON_BUILD_CACHE_PATH`, which must be an existing directory. `pyenv install` supplies
  `$PYENV_ROOT/cache` if that directory exists. Nothing creates it [src bin/pyenv-install:237-239]
  [src bin/python-build:2610-2614].
- **Contents.** Files are stored under the **package file name**, not the checksum. [probe]: `<root>/cache/pkg-1.0.tar.gz`
  and `realroot/cache/Python-3.12.14.tar.xz` (20,820,300 bytes).
- **Hits.** A cached file that verifies, or any cached file when there is no checksum, is symlinked into the build
  directory and **nothing is printed** for the download [test test/cache.bats:23-51] [probe].
- **Invalid cached file.** It is re-downloaded (mirror first) and overwritten [test test/cache.bats:54-73].
- **Git.** `install_git` keeps a bare clone in the cache, named `sanitize(<url>)` [src bin/python-build:688-700].

### Patches

- **Built-in patches** [src bin/python-build:1308-1327]:
  - `make_package` collects every regular file directly in
    `share/python-build/patches/<definition basename>/<package NAME>/`, sorted by name with `sort -z`, into
    `NAME.patch` [test test/pyenv_ext.bats:135-163].
  - **195 of the 292** CPython definitions have a `patches/<def>/Python-*` directory. Method: a shell loop over the
    digit-named files testing `ls -d patches/<f>/Python-*`. Examples include every 3.10.x–3.14.x release and their
    `t` twins.
- **`-p`** [src bin/python-build:1313-1315]. Instead of the built-in patches, stdin is copied into `NAME.patch`, but
  only for the first package whose name matches `package_is_python` (`Python-*`, `jython-*`, `pypy-*`,
  `pypyX.Y-*`, `stackless-*`).
- **Applying** [src bin/python-build:875,2139-2148]. `build_package` calls `apply_patch`, which copies the patch to
  `mktemp "<TMP>/python-patch.XXXXXX"` and runs `patch -p0 --force -i <tmp>`. The level is `-p1` if any line starts
  with `diff --git a/` [test test/build.bats:155-213].
- **Two side effects** [probe]:
  - `patch`'s **stdout is not redirected**. On a normal install the user sees `patching file …` and `Hunk #n
    succeeded …` lines on stdout.
  - The temporary `python-patch.XXXXXX` file is **never deleted**. One was left in `TMPDIR` per install.
- **After the build**, `NAME.patch` is removed [src bin/python-build:1323-1327].
- **Custom configure command.** `PYTHON_CONFIGURE` replaces `./configure` and can be used to patch
  [test test/build.bats:1477-1507].

### Build steps (`standard`)

`build_package_standard_build NAME` [src bin/python-build:912-994]:

- **`MAKE_OPTS`.** Set to `$MAKEOPTS` if `MAKEOPTS` is defined, even if empty. Otherwise kept if `MAKE_OPTS` is
  defined. Otherwise it is `-j <cores>`.
  - `<cores>` is `getconf _NPROCESSORS_ONLN`, else `grep -c ^processor /proc/cpuinfo`, else `2`.
  - [probe]: `-j 32`; `MAKEOPTS=-j7` beat `MAKE_OPTS=-j3`.
  - Defaults to 2 when detection fails [test test/build.bats:1237-1265].
- **Variable family.** `<PKG>` is `NAME` up to the first `-`, uppercased: `Python-3.12.14` gives `PYTHON`. The
  variables `<PKG>_CONFIGURE`, `_PREFIX_PATH`, `_CONFIGURE_OPTS`, `_CONFIGURE_OPTS_ARRAY`, `_MAKE_OPTS`,
  `_MAKE_OPTS_ARRAY`, `_CFLAGS`, `_CPPFLAGS`, `_LDFLAGS`, `_MAKE_INSTALL_OPTS`, `_MAKE_INSTALL_OPTS_ARRAY` and
  `_MAKE_INSTALL_TARGET` apply to that package only.
- **For `PYTHON` only:** the Homebrew, MacPorts and FreeBSD helpers (all no-ops on plain Linux), `use_dsymutil`
  (macOS only), and **`use_free_threading`**, which adds `--disable-gil` when `PYTHON_BUILD_FREE_THREADING` is set.
- **Configure** runs in a subshell, with all output to the log:
  - `CFLAGS="${CFLAGS:+$CFLAGS }$PYTHON_CFLAGS"`; likewise `CPPFLAGS` with `PYTHON_CPPFLAGS` and `LDFLAGS` with
    `PYTHON_LDFLAGS`. These exports apply **only to configure**.
  - The command line is:

    ```text
    ${PYTHON_CONFIGURE:-./configure} --prefix="${PYTHON_PREFIX_PATH:-$PREFIX_PATH}" "${PYTHON_CONFIGURE_OPTS_ARRAY[@]}" $CONFIGURE_OPTS ${PYTHON_CONFIGURE_OPTS}
    ```

  - The last two are word-split and unquoted.
- **Build:** `"$MAKE" "${PYTHON_MAKE_OPTS_ARRAY[@]}" $MAKE_OPTS ${PYTHON_MAKE_OPTS}`, with output to the log.

`build_package_standard_install NAME` [src bin/python-build:996-1006]:

```text
"$MAKE" ${PYTHON_MAKE_INSTALL_TARGET:-install} $MAKE_INSTALL_OPTS ${PYTHON_MAKE_INSTALL_OPTS} "${PYTHON_MAKE_INSTALL_OPTS_ARRAY[@]}"
```

`MAKE_OPTS` is **not** passed to the install step.

[probe] With `CONFIGURE_OPTS=--global-opt`, `PYTHON_CONFIGURE_OPTS="--with-foo --enable-optimizations"`,
`CFLAGS=-gcflag`, `PYTHON_CFLAGS=-pycflag`, `CPPFLAGS=-gcpp`, `PYTHON_CPPFLAGS=-pycpp`, `LDFLAGS=-gld`,
`PYTHON_LDFLAGS=-pyld`, `MAKE_OPTS=-j3`, `PYTHON_MAKE_OPTS=V=1`, `MAKE_INSTALL_OPTS=DESTX=1` and
`PYTHON_MAKE_INSTALL_OPTS=Y=2`:

```text
configure argv: --prefix=<p> --enable-shared --libdir=<p>/lib --global-opt --with-foo --enable-optimizations
configure env: CC= CFLAGS=-gcflag -pycflag CPPFLAGS=-I<p>/include -gcpp -pycpp LDFLAGS=-L<p>/lib -Wl,-rpath,<p>/lib -gld -pyld LIBS=-L<p>/lib -Wl,-rpath,<p>/lib PKG_CONFIG_PATH=
make argv: -j3 V=1
make argv: install DESTX=1 Y=2
```

Further runs [probe]:
- **No variables:** `--prefix=<p> --enable-shared --libdir=<p>/lib`, then `make -j 32`, then `make install`.
- **`PYTHON_CONFIGURE_OPTS=--disable-shared`:** `--prefix=<p> --libdir=<p>/lib --disable-shared`, and `LDFLAGS`/`LIBS`
  carry **no** rpath [test test/build.bats:1350-1372].
- **`-g`:** `--prefix=<p>-debug --with-pydebug --enable-shared --libdir=<p>-debug/lib`, and `CFLAGS=-O0 ` with a
  trailing space.
- **`PYTHON_BUILD_FREE_THREADING=1`:** `--disable-gil` is appended after `--libdir`.
- **`PYTHON_MAKE_INSTALL_TARGET=altinstall`:** `make altinstall`.
- **Test-pinned order.** Package-specific flags come after global ones. Example: `YAML_CFLAGS` after `CFLAGS`, and
  `yaml_configure_opt*` after `configure_opt*` [test test/build.bats:112-153].

After every package, `fix_directory_permissions` runs `chmod go-w` on every group- or world-writable directory under
the prefix [src bin/python-build:1329-1332].

### Post-build: verification, symlinks, pip, gdb

**`build_package_symlink_version_suffix`** [src bin/python-build:2151-2190]. It runs inside `verify_python`, after
`ensurepip`, and after `get_pip`.

- It does nothing if `PYTHON_MAKE_INSTALL_TARGET` contains `altinstall`.
- Otherwise it takes the suffix from the last `ls -1 bin/python* | grep '[0-9]$' | sort` entry, for example `3.12`.
  For each file in `bin/`, it creates a relative symlink when the target name does not exist yet:
  - `python<S>-config` gives `python-config`;
  - `*-<S>` gives the name without `-<S>`;
  - `*<S>` gives the name without `<S>`.
- [probe]: it produced `python`, `python-config`, `pip`, `idle`, `pydoc` and `2to3`. [test test/pyenv_ext.bats:224-244]

**`verify_python X.Y`** [src bin/python-build:2192-2207]. After the symlinks, it requires that
`<prefix>/bin/python<X.Y>` is executable. Otherwise it prints the following to stderr and fails, which ends in BUILD
FAILED [probe]:

```text
ERROR: invalid Python executable: <prefix>/bin/python3.12

The python-build could not find proper executable of Python after successful build.
Please open an issue for future improvements.
https://github.com/pyenv/pyenv/issues
```

**Module checks** [src bin/python-build:2209-2229]:

- Each check runs `"$PYTHON_BIN" -c "import <mod>"`. Python's own traceback is **not** redirected, so it appears on
  stderr above the message.
- **`try_python_module` (warning only).** Prints `WARNING: The Python <mod> extension was not compiled[ <extra>].
  Missing the <lib>?` to stderr and continues.
- **`verify_python_module` (fatal).** Prints the following to stderr, and the ERR trap then prints BUILD FAILED:

  ```text
  ERROR: The Python <mod> extension was not compiled. Missing the <lib>?

  Please consult to the Wiki page to fix the problem.
  https://github.com/pyenv/pyenv/wiki/Common-build-problems

  ```

- `WARNING`/`ERROR` are bold (`\e[1m…\e[m`) when stderr is a tty [probe, tty].

Checks per `verify_pyXY` [src bin/python-build:2232-2390]. Each `verify_py3N` (3.1 ≤ N ≤ 16) calls the previous one,
so all accumulate:

| Version | Order: **fatal** / warn (`<lib>` text) |
|---|---|
| 2.1, 2.2, 2.3 | warn readline (GNU readline lib), **binascii**, warn zlib, warn bz2 (bzip2 lib) |
| 2.4 | warn readline, **zlib**, warn bz2 |
| 2.5 | 2.4 + warn sqlite3 (SQLite3 lib) |
| 2.6, 2.7 | 2.5 + **ssl** (OpenSSL lib) |
| 3.0–3.2 | warn bz2 (bzip2 lib), warn curses (ncurses lib), warn ctypes (libffi lib), warn readline (GNU readline lib), **ssl** (OpenSSL lib), warn sqlite3 (SQLite3 lib), warn tkinter (Tk toolkit, extra text `and GUI subsystem has been detected`) **only if `$DISPLAY` is non-empty**, **zlib** (zlib) |
| 3.3–3.16 | 3.0 list + warn lzma (lzma lib), last |
| `py3_latest` | same as 3.11 with interpreter `python3` |

[probe] The import order for `verify_py312` was `bz2, curses, ctypes, readline, ssl, sqlite3, tkinter, zlib, lzma`.
`tkinter` was checked only because `DISPLAY=:0`. The first fatal failure stops the checks, so a missing `ssl` hides
zlib and lzma. [probe] Warnings for bz2 and readline plus a fatal ssl gave (stderr, exit 1):

```text
Downloading Python-3.12.99.tar.gz...
-> file:///home/jm/m2ref-linux/fixtures/Python-3.12.99.tar.gz
Installing Python-3.12.99...
ModuleNotFoundError: No module named '_bz2'
WARNING: The Python bz2 extension was not compiled. Missing the bzip2 lib?
ModuleNotFoundError: No module named '_readline'
WARNING: The Python readline extension was not compiled. Missing the GNU readline lib?
Traceback (most recent call last):
  File "<string>", line 1, in <module>
ModuleNotFoundError: No module named '_ssl'
ERROR: The Python ssl extension was not compiled. Missing the OpenSSL lib?

Please consult to the Wiki page to fix the problem.
https://github.com/pyenv/pyenv/wiki/Common-build-problems


BUILD FAILED (Debian n/a using python-build 2.8.6)

Inspect or clean up the working tree at <tmp>/python-build.<seed>
Results logged to <tmp>/python-build.<seed>.log

Last 10 log lines:
<tmp>/python-build.<seed> ~
<tmp>/python-build.<seed>/Python-3.12.99 <tmp>/python-build.<seed> ~
...
```

The `ModuleNotFoundError` lines came from the fake interpreter. A real interpreter prints its own traceback. With
warnings only, the install succeeds: exit 0, and `Installed …` follows the warnings [probe].

**`ensurepip`** [src bin/python-build:2445-2458]:

- Runs `"$PYTHON_BIN" -I -m ensurepip [--altinstall]` with **all output discarded**. `--altinstall` is used when
  `PYTHON_MAKE_INSTALL_TARGET` contains `altinstall`. `ensurepip_lt21` uses `-s` instead of `-I`
  [test test/pyenv_ext.bats:188-222].
- On failure it falls back to `get_pip`, which prints `Installing pip from <GET_PIP_URL>...` (or `from <GET_PIP>`
  when `GET_PIP` names a file) to stderr, downloads `get-pip.py` and runs `"$PYTHON_BIN" -s get-pip.py ${GET_PIP_OPTS}`.
  - If that fails too: stderr gets `error: failed to install pip via get-pip.py`, then BUILD FAILED
    [src bin/python-build:2421-2438].
- Then the version-suffix symlinks run again, which creates `pip`.

**`copy_python_gdb`.** See the step table. [probe]: it created `python3.12-gdb.py`, mode 644.

### Output streams, verbose mode, and the log

- **File descriptors.** fd 3 is the original stderr [src bin/python-build:28]. fd 4 is the log.
- **What goes where:**
  - configure, make, extraction, checksum and HEAD output go to fd 4;
  - progress messages go to stderr;
  - warnings, errors and BUILD FAILED go to fd 3, which is stderr;
  - `patch` output goes to stdout (see "Patches").
- **`-v`** [probe]: stdout received the whole log as it grew. That includes the `pushd`/`popd` directory-stack lines,
  such as `<tmp>/python-build.<seed> ~`, and configure and make output. stderr kept the four progress lines.
- **Downloads on a tty.** When fd 3 is a tty, the download progress goes to both the terminal and the log through
  `tee`. With `-v` it goes only to the log, which `tail -f` echoes [src bin/python-build:432-438].
- **Log retention.** The log is append-only and **never removed**, after success or failure [probe].

### Failure output (`build_failed`)

The block goes to fd 3, which is stderr, and the exit code is **1** [src bin/python-build:191-216]:

```text

BUILD FAILED (<os_information> using python-build <version>)

Inspect or clean up the working tree at <BUILD_PATH>
Results logged to <LOG_PATH>

Last 10 log lines:
<tail -n 10 of LOG_PATH>
```

- **First line.** There is an empty line before `BUILD FAILED`.
- **`<os_information>`** is the first available of:
  1. `lsb_release -sir | xargs echo` (probe host: `Debian n/a`);
  2. `OS X <ver>`;
  3. `$NAME $VERSION_ID` from `/etc/os-release`;
  4. the first line of `/etc/{centos,redhat,fedora,system}-release` or `/etc/debian_version`;
  5. `uname -sr`

  [src bin/python-build:109-121].
- **`Inspect or clean up …`** appears only if `rmdir BUILD_PATH` fails, that is when the directory is not empty.
  [probe]: absent after a download failure, which left the directory empty.
- **`Results logged …`, the empty line and the tail** appear only if the log has at least one line. `Results logged
  to …` is yellow (`\e[33m`) on a tty.
- **Compiler hint.** If the tail contains `no acceptable C compiler found`, this follows [probe]:

  ```text

  Are the build dependencies for Python correctly installed?
  Please consult to the Wiki page for more info.
  https://github.com/pyenv/pyenv/wiki#suggested-build-environment
  ```

- **Directory left behind.** The build directory stays: `<TMP>/python-build.<seed>`, or `PYTHON_BUILD_BUILD_PATH`.
  [probe]: with `PYTHON_BUILD_BUILD_PATH=/x`, the message named `/x`, and `/x/Python-3.12.98` remained.
- **Prefix.** `pyenv-install` removes the prefix only if it did not exist before (see the exit-code table).

### Keep and cleanup

- **After success** [src bin/python-build:2856]:
  - `BUILD_PATH` is removed, unless `-k`.
  - With `-k`, the extracted tree **and the tarball** stay. [probe]: `pyenv install -k` left
    `<root>/sources/3.12.99/{Python-3.12.99,Python-3.12.99.tar.gz}`.
- **`PYTHON_BUILD_BUILD_PATH`** fixes the directory. `pyenv-install` sets it to `$PYENV_BUILD_ROOT/<name>` whenever
  `PYENV_BUILD_ROOT` is non-empty.
- **Always left in `TMP`:** the log file, and one `python-patch.XXXXXX` per patched package [probe].

### Dependency checks and hints

- **Linux pre-build checks.** python-build makes no pre-build dependency check on Linux. What exists:
  - the `TMPDIR` checks;
  - the bzip2 and xz warnings in the log;
  - the http-client error;
  - the `git` error for `-dev` definitions;
  - `require_gcc` in ten 2.x definitions;
  - after the fact, the `verify_py*` module checks and the "no acceptable C compiler" hint.

  [src bin/python-build:191-216,412-422,567-579,2572-2590]
- **New command `pyenv install-prerequisites [-a|--all]`** [src bin/pyenv-install-prerequisites:1-65]
  [test test/install-prerequisites.bats]:
  - **Platform.** Debian-like systems only. Without `apt-get`, stderr gets `pyenv: installing build prerequisites is
    not supported on this system` and the exit code is 1.
  - **Privileges.** As root it runs `apt-get` directly. Otherwise it uses `sudo apt-get`, or, without `sudo`, prints
    `pyenv: sudo is required to install build prerequisites` and exits 1.
  - **Commands.** It runs `apt-get update -q`, then `apt-get install -yq make build-essential libssl-dev zlib1g-dev
    libbz2-dev libreadline-dev libsqlite3-dev curl git llvm libncurses5-dev libncursesw5-dev xz-utils tk-dev
    libxml2-dev libxmlsec1-dev libffi-dev liblzma-dev libzstd-dev`. `--all` adds `libgdbm-dev`.
  - **Usage errors.** Unknown arguments print the help on stderr and exit 1. `--complete` prints `--all`.
  - Not probed.

---

## pyenv-default-packages (jawshooah/pyenv-default-packages @ fb6f71a)

Files [src default-packages:…]:
- `etc/pyenv.d/install/default-packages.bash`
- `etc/pyenv.d/virtualenv/default-packages.bash`
- `libexec/default-packages.sh`
- `install.sh`
- `README.md`

When it is installed as `$PYENV_ROOT/plugins/pyenv-default-packages`, the dispatcher puts its `etc/pyenv.d` on
`PYENV_HOOK_PATH` (M1).

- **Registration.** The install hook sources `libexec/default-packages.sh`. If `after_install` is a defined function,
  it registers `after_install 'install_default_packages $VERSION_NAME'`. Otherwise it prints `pyenv:
  pyenv-default-packages plugin requires pyenv v0.1.0 or later` to stderr
  [src default-packages:etc/pyenv.d/install/default-packages.bash:2-8]. The virtualenv hook does the same with
  `after_virtualenv` and `$VIRTUALENV_NAME`.
- **`install_default_packages <version>`** [src default-packages:libexec/default-packages.sh:2-19]:
  - It returns 0 unless `STATUS` is `0`, so nothing happens after a failed install. [probe]: no pip run after BUILD
    FAILED.
  - If `${PYENV_ROOT}/default-packages` is a regular file, it runs
    `PYENV_VERSION="<version>" pyenv-exec pip install -r "$PYENV_ROOT/default-packages"`. [probe]: the stand-in pip
    received `install -r <root>/default-packages`.
  - pip's output is not redirected.
  - If pip fails, stderr gets the following, with **two spaces** before the backtick, and the install still exits
    **0** [probe]:

    ```text
    pyenv: error installing packages from  `<root>/default-packages'
    ```

  - Without the file it does nothing [probe].
- **Comments.** The file uses pip's requirements syntax, so pip itself handles comments
  [src default-packages:README.md:42-51].
- **Timing.** The plugin runs as an `after_install` hook. It therefore runs **before** `pyenv-rehash`, with
  `PYENV_VERSION` possibly already exported as the bootstrap version, which the hook's own assignment overrides.
- **Alias installs.** The hook passes `$VERSION_NAME`, the real version name (`-debug` included), **not** the alias.
  For `pyenv install 3.12.99:dpalias` it ran pip with `PYENV_VERSION=3.12.99` [probe]. That version was not
  installed in the probe, so `pyenv-exec` resolved it by prefix to an unrelated `3.12.99-debug` (M1 `version-name`)
  and installed the packages there. When no version matches, `pyenv-exec` fails and the error line above is printed.
  This follows from [src bin/pyenv-install:204,206] and [src default-packages:etc/pyenv.d/install/default-packages.bash:5].
- **`pip` through `pyenv exec`.** `pyenv-exec pip install` also triggers pyenv's `pip-rehash` exec hook, which
  rehashes after a successful `pip install` (M1).

---

## Upstream tests

`plugins/python-build/test/` has 14 `.bats` files with 165 tests, all passing in the probe environment. The helper
does the following [test test/test_helper.bash:1-17]:
- sets `PYTHON_BUILD_HTTP_CLIENT=curl` and an empty `PYTHON_BUILD_CURL_OPTS`;
- puts `${BATS_TEST_TMPDIR}/bin` (stubs), then `<test>/../bin` (`python-build`, `pyenv-install`, `pyenv-uninstall`,
  `pyenv-install-prerequisites`), then the system directories on `PATH`.

**Stubs** [test test/test_helper.bash:19-49] [test test/stubs/stub:1-155]:
- `stub <prog> '<arg-globs> : <command>' …` installs a script named `<prog>` that consumes **one plan line per call**,
  in order.
- It matches the actual argv against the whitespace-split glob patterns. `**` skips arguments.
- On a match it runs the command. A mismatch or a missing line marks the stub failed.
- `unstub` fails the test if any plan line was not consumed, or if a call did not match.

Fixtures: `fixtures/definitions/{needs-yaml, vanilla-python, with-checksum, with-invalid-checksum, with-md5-checksum,
without-checksum}`, a `package-1.0.0.tar.gz` holding `bin/package`, and `fixtures/fake_downloader`.

| File | Tests | Covers | Stubbed programs |
|---|---|---|---|
| `arguments.bats` | 2 | python-build usage on too few or too many arguments | — |
| `build.bats` | 59 | configure/make argv and flag order; Homebrew, MacPorts and FreeBSD dependency wiring; `-p` patches; CPU count; `--disable-shared`; dSYM; `PYTHON_CONFIGURE`; TMPDIR checks; relative prefix | `make`, `curl`, `md5`, `brew`, `port`, `uname`, `sw_vers`, `sysctl`, `pkg`, `patch`, `apply`. A fake `configure` logs its argv and environment. |
| `cache.bats` | 5 | cache save, reuse, invalid cache, missing cache directory | `curl`, `shasum` |
| `checksum.bats` | 10 | SHA-256 and MD5 valid, invalid, unsupported and wrong length; tarball reuse in the build directory | `curl`, `wget`, `shasum`, `md5` |
| `compiler.bats` | 5 | `require_gcc` on OS X; `CC=clang` on OS X; micropython `CFLAGS_EXTRA` | `brew`, `gcc`, `cc`, `make`, `uname`, `sw_vers`, … |
| `definitions.bats` | 9 | `--definitions` listing, sorting, dedup; `PYTHON_BUILD_ROOT` and `PYTHON_BUILD_DEFINITIONS`; not found (exit 2) | — |
| `fetch.bats` | 6 | download failure message; tty progress (needs `script`); aria2c argv; `install_git` clone and update | `curl`, `aria2c`, `git` |
| `hooks.bats` | 3 | `before_install`/`after_install` and `*_uninstall` order and output | `pyenv-hooks`, `pyenv-rehash`, `pyenv-latest` |
| `installer.bats` | 4 | python-build's own `install.sh` | — |
| `install-prerequisites.bats` | 8 | `pyenv install-prerequisites` | `apt-get`, `id`, `sudo`, `pyenv-help` |
| `mirror.bats` | 8 | mirror HEAD/GET, fallback, default mirror, the python.org bypass, `SKIP_CHECKSUM` | `curl`, `shasum` |
| `pyenv.bats` | 24 | `pyenv install` argument handling, aliases, `--list`, the not-found hint, plugin definitions, completion, uninstall arguments | `python-build` (including `--lib`), `pyenv-latest`, `pyenv-hooks`, `pyenv-rehash`, `pyenv-local`, `pyenv-help`, `brew` |
| `pyenv_ext.bats` | 18 | built-in patch application and order; altinstall; ensurepip argv; version-suffix symlinks; PyPy and Pyston symlinks; framework and universalsdk (macOS); `MACOSX_DEPLOYMENT_TARGET`; `GET_PIP_URL` table | `make`, `patch`, `curl`, `md5`, `brew`, `uname`, `sw_vers`, `arch` |
| `version.bats` | 4 | `python-build --version` static and git | `git` |

Under `<root>/test/`, only `latest.bats` exercises a command in M2 scope. M1 covers it. No `<root>/test/*.bats` file
calls `pyenv install` or `pyenv uninstall`. `init.bats` matches "install" only through `pyenv init --install`.
[mechanical: `grep -n -E '\binstall\b|uninstall' <root>/test/*.bats`.]

**Weak tests.** These assert only the exit status, because `assert_success` ignores a heredoc. They therefore do not
pin the output they appear to show [src test/test_helper.bash:92-100]:
- `pyenv.bats` "install resolves a prefix" (lines 90-107). Its `pyenv-latest -r -k` plan never matches the real
  `-f -k` call.
- "install resolves :latest" (110-128).
- "install installs local versions by default" (130-144).

**Running them through the existing parity harness.** These facts come from `parity/bats_run.sh` in this repository:
- It copies only `<upstream>/test` and `<upstream>/pyenv.d` [src parity/bats_run.sh:20-21].
- It runs `*.bats` from `run/test` only [src parity/bats_run.sh:34-37].
- It writes `pyenv-<cmd>` wrappers into `run/libexec` for the M1 commands only (`root … local`) and for `--version`
  [src parity/bats_run.sh:25-29]. The list has no `latest`, `install` or `uninstall`, so upstream's `test/latest.bats`
  calls a missing `pyenv-latest`.
- **Not run at all.** The python-build suite is not copied or run. It needs these paths to exist:
  - `plugins/python-build/test`;
  - executables in `plugins/python-build/bin` (`python-build`, `pyenv-install`, `pyenv-uninstall`,
    `pyenv-install-prerequisites`);
  - `plugins/python-build/share/python-build` (`definitions.bats` counts it);
  - `plugins/python-build/install.sh` (`installer.bats`);
  - and, for `pyenv.bats:110-128`, a pyenv root at `test/../../..` with `libexec` and `pyenv.d`.
- **Why most tests need real python-build.** Most tests observe the argv of **external** programs found on `PATH`:
  `curl`, `make`, `shasum`, `patch`, `git`, `aria2c`, and the sub-commands `python-build --lib`, `pyenv-latest`,
  `pyenv-hooks`, `pyenv-rehash` and `pyenv-local`. They also `source` bash fixture definitions through `python-build`.
  A test passes against rpyenv only if rpyenv:
  - spawns those programs through `PATH`, with the same argv, in the same order; and
  - interprets bash definition files, or wraps upstream's `python-build`.

  This is an inference from the stub mechanism. It was **not executed** against rpyenv, which has no installer yet.
- **Fewest dependencies.** `pyenv.bats:324-368` (uninstall arguments and help) and `hooks.bats:34-94` (uninstall hooks)
  call only `pyenv-uninstall` plus stubbed `pyenv-hooks`, `pyenv-rehash` and `pyenv-help`.

---

## Facts that bear on spec §9.3

Upstream facts that differ from statements in `docs/specs/2026-09-27-rpyenv-design.md` §9.3. They are listed so the
plan can decide; this file makes no judgement.

| Spec §9.3 says | Upstream fact |
|---|---|
| "Atomic: a failure or Ctrl+C never leaves a partial `versions/<ver>`" | Upstream removes a *fresh* prefix on failure and on Ctrl+C, but keeps a pre-existing one partly overwritten (`-f`, or a yes at the prompt). After any argument whose prefix existed, it also keeps a *fresh* failing prefix [probe]. |
| `:latest` candidates exclude "pre-releases (`a`/`b`/`rc`)" … "the same filters as upstream's `install/latest.bash` hook" | The hook removes only `(b|rc)[0-9]+$`, not `a[0-9]+`. It does not remove `-latest`. Its prefix match has no boundary (`3.1:latest` → 3.14.7). It sorts on 3 keys (`pypy3.10:latest` → 7.3.12). It yields an empty definition when nothing matches [probe] [src pyenv.d/install/latest.bash:9-16]. |
| "sorted by version number" | Only the first three dot-fields are compared. Ties fall to `sort`'s default whole-line comparison, which is locale-dependent [src pyenv.d/install/latest.bash:14]. |
| Default packages: "runs `pip install -r …` in the new version" | The plugin targets `$VERSION_NAME`, which is the real version and not the alias, so an aliased install sends pip elsewhere [probe]. |
| Default packages: on failure "prints an error, but the install itself still counts as successful" | Confirmed: exit 0, with the message ``pyenv: error installing packages from  `<file>'`` [probe]. |
| "the pyenv-default-packages plugin" | The plugin's repository is `jawshooah/pyenv-default-packages`. `pyenv/pyenv-default-packages` returns 404. |

---

## Cross-cutting edge-case index

| Topic | Behavior | Section |
|---|---|---|
| Prompt and stdin | The prompt text appears only on a tty. EOF ends the whole `install` run (exit 1) and the whole `uninstall` run. A declined install moves on to the next argument. A declined uninstall stops. | install loop, uninstall |
| `-f` and `-s` together | `-s` wins for existing versions. | install options |
| "Installed?" test | `install` checks `versions/<n>/bin`. `uninstall` checks `versions/<n>` (a directory or a symlink to one). | install loop, uninstall |
| Prefix resolution | `install` calls `latest -f -k` per argument. `uninstall` does not resolve prefixes at all. The `:latest` hook differs from `latest -k` in five ways. | install, `:latest` |
| Exit 2 vs 1 | Not-found gives 2 only when the prefix did not pre-exist. Any `cleanup` with `PREFIX_EXISTS` set gives 1. | exit codes |
| Stdout during install | Only `patch` output (and `-v` log copy, and hook echoes). Everything else is on stderr. | output |
| Leftovers in `TMPDIR` | The log and `python-patch.*` always stay. The build directory stays on failure, Ctrl+C, or `-k`. | keep |
| Alias | Taken after the last `:`. `latest` is reserved. `-g` adds no `-debug` to an alias. The default-packages hook ignores the alias. | install, default-packages |
| Plugin definitions | Visible to `install`, `install --list` and install's prefix resolution, but not to standalone `pyenv latest -k`. | install |
| Free-threaded | `3.13t` resolves to `3.13.15t`. The `t` definitions source the non-`t` file with `PYTHON_BUILD_FREE_THREADING=1`, which gives `--disable-gil`. `3t` resolves to the prerelease `3.15.0rc2t`. | latest, DSL |
| Locale | `--definitions` pins `LC_ALL=C`. The `:latest` hook's `sort` and `pyenv latest`'s sort do not. | list, `:latest` |

## UNCONFIRMED items

- **A real build on a host with missing `-dev` libraries.** The exact CPython 3.12 traceback text that precedes each
  WARNING or ERROR line was not observed. The probes used a stand-in interpreter. The message lines themselves are
  `[src]` + `[probe]`.
- **Exact timing of the first install into a fresh `PYENV_ROOT`.** `bc` was missing, so the timer printed nothing. The
  log stamp is 02:47:24 and the files' mtimes are 02:48, so it took about a minute. The two timed reinstalls (64.8 s
  and 59.2 s) are measured.
- **`--` as an argument to `pyenv install`.** Derived as a usage error, not probed.
- **`aria2c` behavior.** It was not installed on the probe host. Only `[src]` and `[test]` cover it.
- **`-p` with a real patch through `pyenv install`.** Only `[test]` (python-build direct) and `[src]` cover it.
- **Behavior on a Debian trixie container.** The probe host was Debian forky/sid on WSL2. `os_information` text differs
  per host (`Debian n/a` here). Without `lsb_release`, `[src bin/python-build:114-116]` would give
  `Debian GNU/Linux 13` on trixie. Not probed.
- **The `PYENV_BOOTSTRAP_VERSION` path for CPython** (the `pyenv-whence` loop) was not probed. Only `[src]` covers it.
- **Whether `PYTHON_MAKE_INSTALL_OPTS='DOGE="such wow"'` reaches make as one argument or two.**
  [test test/build.bats:1326-1348] cannot tell the two apart, because the stub joins argv with spaces. The unquoted
  expansion at `[src bin/python-build:1004]` implies two words, `DOGE="such` and `wow"`.
