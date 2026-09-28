# pyenv behavioral reference: rpyenv M1 command set (Linux, bash)

Parity target for rpyenv on Linux. This file records facts only. Each fact carries an evidence tag, and every
`file:line` path is relative to the root of the pyenv repository at the pinned commit below.

## Source, version, and evidence method

- **Source.** GitHub `pyenv/pyenv`, branch `master`, commit `ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2`
  (committed 2026-09-27, "Fix minimum macOS version check for PyPy v7.3.8+ (#3570)"). Files were fetched at that
  commit through `gh api repos/pyenv/pyenv/contents/<path>?ref=<sha>`. The repository was not cloned.
- **Version.** `libexec/pyenv---version:15` sets `version="2.8.6"`.

Evidence tags:

| Tag | Meaning |
|---|---|
| `[src f:l]` | Derived by reading the code at that line. Not executed by itself. |
| `[test f:l]` | A value asserted by pyenv's own bats suite. |
| `[probe]` | Observed by executing the pinned scripts. See the next paragraph. |
| **UNCONFIRMED** | Not established by any of the above. |

**Probe environment.** The pinned `libexec/`, `pyenv.d/`, `test/` and `plugins/python-build/bin/` files were copied to
`/opt/pyenv` in a Debian trixie container, with `bin/pyenv -> ../libexec/pyenv`. The container had GNU bash 5.2.37,
GNU coreutils 9.7, GNU sed 4.9, GNU grep 3.11, GNU findutils 4.10.0, and gawk as `awk`. Commands ran as an unprivileged
user with `HOME=/home/tester`, `PYENV_ROOT=/home/tester/.pyenv` and `LANG=C.UTF-8`, unless a probe says otherwise.
Stdout and stderr were captured separately and inspected with `cat -A`, which shows each end of line, so trailing
newlines are known exactly. There was no `.git` directory, so `pyenv --version` printed the literal version. In the
quoted probe output, `/home/tester/.pyenv` is `PYENV_ROOT`, `/opt/pyenv` is the install prefix (`_PYENV_INSTALL_PREFIX`)
and `/home/tester/proj` is the working directory.

**Harness check.** The upstream bats files for every command in scope, plus `pyenv.bats`, `hooks.bats` and
`pip-rehash.bats`, were run with Bats 1.11.1 in the same container. Result: 206 tests, 204 ok, 2 skipped (the fish and
pwsh integration tests, because those shells were not installed), 0 failed. The probe environment therefore
reproduces the upstream expectations.

### How the bats suite observes output

- **Merged streams.** bats `run` captures stdout and stderr together in `$output`. A `[test]` literal therefore does
  not show which stream a line went to. Where a stream is stated below, it comes from `[src]` or `[probe]`.
- **Trailing newlines.** `$output` comes from command substitution, so all trailing newlines are removed. Tests
  cannot see trailing empty lines, and they cannot see an empty stdout line either.
- **Test environment.** `test/test_helper.bash:1-2` unsets `PYENV_VERSION` and `PYENV_DIR`. Setup sets
  `PYENV_ROOT=$BATS_TEST_TMPDIR/pyenv/root`, `HOME=$BATS_TEST_TMPDIR/pyenv/home` and
  `PYENV_HOOK_PATH=$PYENV_ROOT/pyenv.d`, and builds a fixed `PATH` [test test/test_helper.bash:28-41]. Most tests
  call `libexec/pyenv-<cmd>` directly, without going through the dispatcher.

### Conventions shared by all commands

- Every script except `pyenv-root` runs under `set -e`. When `PYENV_DEBUG` is non-empty it also runs `set -x`, which
  traces to stderr [src libexec/pyenv-which:14-15, and the same two lines near the top of every other script].
  `pyenv-root` is a single `echo` [src libexec/pyenv-root:1-3].
- Output lines come from `echo` or `printf '...\n'`, so each line ends with `\n`. Exceptions are noted where they occur.
- Messages quote a value with a backtick before it and an apostrophe after it, for example
  ``pyenv: version `3.12' is not installed``. The one exception is `pyenv which`'s `Note:` line, which uses straight
  quotes: `'pyenv help global'`.
- In the angle-bracket placeholders below, `<PYENV_ROOT>` is the dispatcher-normalized root and `<prefix>` is
  `_PYENV_INSTALL_PREFIX`.

---

## `pyenv` (dispatcher) — `libexec/pyenv`

**Synopsis.** `pyenv [--debug] <command> [<args>...]`, `pyenv -v|--version`, `pyenv -h|--help`, `pyenv`.

**Flags.**
- `--debug` is recognized only as the very first argument. It exports `PYENV_DEBUG=1` and is then shifted away
  [src libexec/pyenv:4-7].
- When `PYENV_DEBUG` is non-empty, the dispatcher exports
  `PS4='+(${BASH_SOURCE}:${LINENO}): ${FUNCNAME[0]:+${FUNCNAME[0]}(): }'` and runs `set -x` [src libexec/pyenv:9-13].
- `-v` or `--version` in command position runs `exec pyenv---version` [src libexec/pyenv:118-120].
- `-h` or `--help` in command position runs `exec pyenv-help`, which writes to stdout and exits 0
  [src libexec/pyenv:121-123].

**Environment setup, in order.**
1. **`PYENV_ROOT`.** If it is unset or empty, it becomes `${HOME}/.pyenv`. Otherwise exactly one trailing `/` is
   removed. It is then exported [src libexec/pyenv:58-63]. Tests: [test test/pyenv.bats:17-27]. No other
   normalization happens, so a relative value stays relative [src libexec/pyenv:58-62]. [probe]: `PYENV_ROOT=/tmp/x/`
   gives `/tmp/x`, `PYENV_ROOT=/tmp/x//` gives `/tmp/x/`, and `PYENV_ROOT=` gives `/home/tester/.pyenv`.
2. **`PYENV_DIR`.** If it is unset or empty, it becomes `$PWD` [src libexec/pyenv:65-67]. If it is not a directory,
   the dispatcher aborts [src libexec/pyenv:69-71]. Otherwise it is replaced by `$(cd "$PYENV_DIR" && echo "$PWD")`,
   an absolute logical path in which symlinks are not resolved, and exported [src libexec/pyenv:73-74].
3. **`_PYENV_INSTALL_PREFIX`.** This is the parent of the directory that contains the symlink-resolved `$0`. It is
   exported [src libexec/pyenv:79-82]. `bin/pyenv` is a symlink to `../libexec/pyenv`, so the prefix is the repository
   root.
4. **`PATH`.** Each `${_PYENV_INSTALL_PREFIX}/plugins/*/bin` is prepended in glob order [src libexec/pyenv:84-86].
   Then, only if `_PYENV_INSTALL_PREFIX != PYENV_ROOT`, each `${PYENV_ROOT}/plugins/*/bin` is prepended the same way
   [src libexec/pyenv:89-93]. Finally `${_PYENV_INSTALL_PREFIX}/libexec` is prepended and `PATH` is exported
   [src libexec/pyenv:94]. Because every match is prepended, later glob matches end up earlier in `PATH`. The
   resulting order is `libexec`, then the `PYENV_ROOT` plugin dirs in reverse glob order, then the install-prefix
   plugin dirs, then the inherited `PATH` [test test/pyenv.bats:49-58].
5. **`PYENV_HOOK_PATH`.** The value is built as follows and exported [src libexec/pyenv:96-106]:
   `<inherited>:${PYENV_ROOT}/pyenv.d`, then `:${_PYENV_INSTALL_PREFIX}/pyenv.d` if the prefix differs from
   `PYENV_ROOT`, then `:/usr/etc/pyenv.d:/usr/local/etc/pyenv.d:/etc/pyenv.d:/usr/lib/pyenv/hooks`, then one
   `:${PYENV_ROOT}/plugins/<p>/etc/pyenv.d` for each match. A single leading `:` is removed at the end.
   Tests: [test test/pyenv.bats:60-72].

**Errors.** Every error goes through `abort` [src libexec/pyenv:15-21]. With arguments, `abort` prints
`pyenv: <args>` to stderr. With none, it copies its stdin to stderr. It always exits 1.

```text
pyenv: cannot change working directory to `<PYENV_DIR>'
pyenv: no such command `<command>'
pyenv: shell integration not enabled. Run `pyenv init' for instructions.
pyenv: failed to load `realpath' builtin
pyenv: cannot find readlink - are you missing GNU coreutils?
```

- **`PYENV_DIR`** [src libexec/pyenv:69-71] [test test/pyenv.bats:41-47]. [probe]: `PYENV_DIR=/etc/passwd`, a regular
  file, gives the same message.
- **Unknown command** [src libexec/pyenv:130] [test test/pyenv.bats:11-15].
- **`shell`** gets its message only when no `pyenv-shell` exists on `PATH` [src libexec/pyenv:127-128]. pyenv ships
  only `pyenv-sh-shell`, so `pyenv shell` always prints this message unless the shell function installed by
  `pyenv init` intercepts the call. [probe] confirms stderr and exit 1.
- **`realpath` builtin.** Printed only when `PYENV_NATIVE_EXT` is non-empty and `libexec/pyenv-realpath.dylib` cannot
  be loaded [src libexec/pyenv:23,30].
- **`readlink`.** Printed when `readlink` is not found on `PATH` [src libexec/pyenv:32-33].

**No arguments.** The dispatcher runs `{ pyenv---version; pyenv-help; } | abort` [src libexec/pyenv:113-117]. The
version line and the full `pyenv help` listing therefore go to **stderr**, and the exit code is 1. [probe] confirms
that stdout is empty, stderr carries the text, and the exit code is 1. [test test/pyenv.bats:5-9] asserts only the
first line.

```text
pyenv 2.8.6
Usage: pyenv <command> [<args>]

Some useful pyenv commands are:
   --version   Display the version of pyenv
   commands    List all available pyenv commands
   exec        Run an executable with the selected Python version
   global      Set or show the global Python version(s)
   help        Display help for a command
   hooks       List hook scripts for a given pyenv command
   init        Configure the shell environment for pyenv
   latest      Print the latest installed or known version with the given prefix
   local       Set or show the local application-specific Python version(s)
   prefix      Display prefixes for Python versions
   rehash      Rehash pyenv shims (run this after installing executables)
   root        Display the root directory where versions and shims are kept
   shell       Set or show the shell-specific Python version
   shims       List existing pyenv shims
   version     Show the current Python version(s) and its origin
   version-file   Detect the file that sets the current pyenv version
   version-name   Show the current Python version
   version-origin   Explain how the current Python version is set
   versions    List all Python versions available to pyenv
   whence      List all Python versions that contain the given executable
   which       Display the full path to an executable

See `pyenv help <command>' for information on a specific command.
For full documentation, see: https://github.com/pyenv/pyenv#readme
```

**Command lookup** [src libexec/pyenv:124-144]. The command path is `command -v "pyenv-$command"`, searched on the
`PATH` built above. Any `pyenv-<name>` on that `PATH` is therefore a command: built-ins in `libexec`, plugin `bin`
dirs, and any directory in the user's `PATH`.

The argument right after the command word is checked for `--help`:
- **`sh-` commands.** For a command name starting with `sh-`, the dispatcher prints `pyenv help "<command>"` on stdout
  and exits 0 [src libexec/pyenv:136-137]. [probe]: `pyenv sh-shell --help` printed `pyenv help "sh-shell"`.
- **Other commands.** The dispatcher runs `exec pyenv-help "<command>"` [src libexec/pyenv:139]. The output is
  identical to `pyenv help <command>`.
- **`--help` in a later position** is passed through as an ordinary argument. [probe]: `pyenv which python --help`
  printed `pyenv: python: command not found` and exited 127.

Otherwise the dispatcher runs `exec "<command_path>" "$@"` with the remaining arguments [src libexec/pyenv:142].

**Environment variables.**
- **Read:** `PYENV_DEBUG`, `PYENV_ROOT`, `HOME`, `PYENV_DIR`, `PWD`, `PYENV_HOOK_PATH`, `PYENV_NATIVE_EXT`, `PATH`.
- **Exported:** `PYENV_ROOT`, `PYENV_DIR`, `_PYENV_INSTALL_PREFIX`, `PATH`, `PYENV_HOOK_PATH`. In debug mode, also
  `PYENV_DEBUG` and `PS4`.

---

## `pyenv --version` — `libexec/pyenv---version`

**Synopsis.** `pyenv --version`, `pyenv -v` (both via the dispatcher), or `pyenv---version`. There are no flags.

**Output.** One line on stdout, `pyenv <version>`, with exit code 0 [src libexec/pyenv---version:23].
- **Git revision.** `<version>` is the output of `git describe --tags HEAD` with one leading `v` removed. This applies
  only when `cd` into the script's own directory succeeds and `git remote -v` there prints a line containing the
  string `pyenv` [src libexec/pyenv---version:18-21]. The resulting form is `<tag>-<commits since tag>-g<abbrev sha>`,
  for example `pyenv 0.4.1-2-g<sha>` [test test/--version.bats:35-44].
- **Literal fallback.** In every other case, including a remote without `pyenv` in it
  [test test/--version.bats:25-33] and a repository with no tags [test test/--version.bats:46-52], `<version>` is
  the literal `2.8.6` [src libexec/pyenv---version:15].
- **Exact tag.** When `HEAD` is exactly on a tag, git describe prints just the tag, so the output would be
  `pyenv <tag>`. **UNCONFIRMED**: no test covers it and it was not probed.
- **Version-dependent.** The literal changes with every release.

[probe] (no `.git`):

```text
pyenv 2.8.6
```

**Help block** [src libexec/pyenv---version:1-10]:

```text
#!/usr/bin/env bash
# Summary: Display the version of pyenv
#
# Displays the version number of this pyenv release, including the
# current revision from git, if available.
#
# The format of the git revision is:
#   <version>-<num_commits>-<git_sha>
# where `num_commits` is the number of commits since `version` was
# tagged.
```

Rendered `pyenv help --version` [probe]. `pyenv help --usage --version` prints nothing and exits 0.

```text
Usage: pyenv --version

Displays the version number of this pyenv release, including the
current revision from git, if available.

The format of the git revision is:
  <version>-<num_commits>-<git_sha>
where `num_commits` is the number of commits since `version` was
tagged.

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv root` — `libexec/pyenv-root`

The script runs `echo "$PYENV_ROOT"` [src libexec/pyenv-root:3]. It has no flags, ignores extra arguments and always
exits 0. The value is the dispatcher-normalized `PYENV_ROOT` described above [test test/pyenv.bats:17-27].

**Help block** [src libexec/pyenv-root:1-2]:

```text
#!/usr/bin/env bash
# Summary: Display the root directory where versions and shims are kept
```

Rendered `pyenv help root` and `pyenv root --help` [probe] (both identical). `pyenv help --usage root` prints nothing
and exits 0.

```text
Usage: pyenv root

Display the root directory where versions and shims are kept

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv commands` — `libexec/pyenv-commands`

**Synopsis.** `pyenv commands [--sh|--no-sh]` [src libexec/pyenv-commands:3].

**Flags.**
- `--complete` as the first argument prints `--sh` and `--no-sh` and exits 0 [src libexec/pyenv-commands:9-13].
- Only the first argument is examined for `--sh` or `--no-sh` [src libexec/pyenv-commands:15-21]. Any other arguments
  are ignored.

**Algorithm** [src libexec/pyenv-commands:23-48]:
- `PATH` is split on `:`. Each directory is globbed for `pyenv-*` with nullglob. There is **no executable check**.
- The `pyenv-` prefix is stripped. In default mode, a leading `sh-` is also stripped, so `sh-shell` becomes `shell`.
- `--sh` lists only the `pyenv-sh-*` names, with `pyenv-sh-` removed.
- `--no-sh` omits names that start with `sh-`.
- Output is piped through `sort -u`, which sorts by locale collation.
- Because the dispatcher has already prepended `libexec` and the plugin dirs, the list covers built-ins, plugin
  commands, and every `pyenv-*` file on the user's `PATH` [test test/commands.bats:5-39].

`--version` appears in the list because the file is named `pyenv---version`.

[probe] `pyenv commands` (C.UTF-8, no plugin commands present). Exit 0.

```text
--version
commands
completions
exec
global
help
hooks
init
latest
local
prefix
rehash
root
shell
shims
version
version-file
version-file-read
version-file-write
version-name
version-origin
versions
whence
which
```

[probe] `pyenv commands --sh`:

```text
rehash
shell
```

**Locale.**
- **Sort order.** Under `en_US.UTF-8`, `sort` ignores punctuation at the first comparison level, so `--version` is
  not first. [probe]: the listing began `commands`, `completions`, `exec`. The same effect moves `--version` in the
  `pyenv help` listing (see below).
- **`realpath.dylib`.** If the optional native extension `libexec/pyenv-realpath.dylib` exists
  [src libexec/pyenv:23], `realpath.dylib` would be listed as well. This follows from the missing executable check;
  **UNCONFIRMED**, because the extension was not built.

**Help block** [src libexec/pyenv-commands:1-3]:

```text
#!/usr/bin/env bash
# Summary: List all available pyenv commands
# Usage: pyenv commands [--sh|--no-sh]
```

Rendered `pyenv help commands` [probe]. `--usage` prints `Usage: pyenv commands [--sh|--no-sh]`.

```text
Usage: pyenv commands [--sh|--no-sh]

List all available pyenv commands

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv help` — `libexec/pyenv-help`

**Synopsis.** `pyenv help [--usage] COMMAND` [src libexec/pyenv-help:5].

**Flags.**
- `--complete` as the first argument prints `--usage` and then the output of `pyenv-commands`
  [src libexec/pyenv-help:19-22].
- `--usage` is recognized only as the first argument [src libexec/pyenv-help:146-150].

### Mode 1: no command, or the command `pyenv`

[src libexec/pyenv-help:152-160]. This is also the output of `pyenv --help` and `pyenv -h`. Everything goes to stdout
and the exit code is 0 [probe].

```text
Usage: pyenv <command> [<args>]

Some useful pyenv commands are:
<one line per documented command>

See `pyenv help <command>' for information on a specific command.
For full documentation, see: https://github.com/pyenv/pyenv#readme
```

- **Line format.** Each command line is `printf "   %-9s   %s\n" "$command" "$summary"` [src libexec/pyenv-help:106]:
  three spaces, the name left-justified in a 9-column field, three spaces, then the summary. Longer names are not
  truncated. They push the summary to the right, for example
  `   version-origin   Explain how the current Python version is set`.
- **Candidates.** The command list is `$(exec pyenv-commands | sort -u)`, word-split [src libexec/pyenv-help:157].
- **Filter.** Only commands whose comment block has a non-empty `Summary:` are printed [src libexec/pyenv-help:105].
  `completions`, `version-file-read` and `version-file-write` have no Summary, so they are never listed.
- **`--usage` without a command.** Only the first line, `Usage: pyenv <command> [<args>]`, is printed, and the exit
  code is **1** [probe]. `[ -z "$usage" ] || exit` [src libexec/pyenv-help:154] exits with the status of the failed
  `[`.

[probe] listing with a standard install layout (`libexec` plus the bundled `plugins/python-build/bin`), C.UTF-8:

```text
Usage: pyenv <command> [<args>]

Some useful pyenv commands are:
   --version   Display the version of pyenv
   commands    List all available pyenv commands
   exec        Run an executable with the selected Python version
   global      Set or show the global Python version(s)
   help        Display help for a command
   hooks       List hook scripts for a given pyenv command
   init        Configure the shell environment for pyenv
   install     Install a Python version using python-build
   install-prerequisites   Install Python build prerequisites on Debian-based systems
   latest      Print the latest installed or known version with the given prefix
   local       Set or show the local application-specific Python version(s)
   prefix      Display prefixes for Python versions
   rehash      Rehash pyenv shims (run this after installing executables)
   root        Display the root directory where versions and shims are kept
   shell       Set or show the shell-specific Python version
   shims       List existing pyenv shims
   uninstall   Uninstall Python versions
   version     Show the current Python version(s) and its origin
   version-file   Detect the file that sets the current pyenv version
   version-name   Show the current Python version
   version-origin   Explain how the current Python version is set
   versions    List all Python versions available to pyenv
   whence      List all Python versions that contain the given executable
   which       Display the full path to an executable

See `pyenv help <command>' for information on a specific command.
For full documentation, see: https://github.com/pyenv/pyenv#readme
```

The same layout under `LANG=en_US.UTF-8` [probe] moves `--version` between `uninstall` and `version`:

```text
Usage: pyenv <command> [<args>]

Some useful pyenv commands are:
   commands    List all available pyenv commands
   exec        Run an executable with the selected Python version
   global      Set or show the global Python version(s)
   help        Display help for a command
   hooks       List hook scripts for a given pyenv command
   init        Configure the shell environment for pyenv
   install     Install a Python version using python-build
   install-prerequisites   Install Python build prerequisites on Debian-based systems
   latest      Print the latest installed or known version with the given prefix
   local       Set or show the local application-specific Python version(s)
   prefix      Display prefixes for Python versions
   rehash      Rehash pyenv shims (run this after installing executables)
   root        Display the root directory where versions and shims are kept
   shell       Set or show the shell-specific Python version
   shims       List existing pyenv shims
   uninstall   Uninstall Python versions
   --version   Display the version of pyenv
   version     Show the current Python version(s) and its origin
   version-file   Detect the file that sets the current pyenv version
   version-name   Show the current Python version
   version-origin   Explain how the current Python version is set
   versions    List all Python versions available to pyenv
   whence      List all Python versions that contain the given executable
   which       Display the full path to an executable

See `pyenv help <command>' for information on a specific command.
For full documentation, see: https://github.com/pyenv/pyenv#readme
```

The `install`, `install-prerequisites` and `uninstall` lines come from the python-build plugin. Its scripts have
`# Summary: Install a Python version using python-build` (`plugins/python-build/bin/pyenv-install:3`),
`# Summary: Install Python build prerequisites on Debian-based systems`
(`plugins/python-build/bin/pyenv-install-prerequisites:3`) and `# Summary: Uninstall Python versions`
(`plugins/python-build/bin/pyenv-uninstall:3`). Without the plugin those three lines are absent [probe].

### Mode 2: `pyenv help <command>`

[src libexec/pyenv-help:161-173, 116-137]

- **Lookup.** `command -v pyenv-<command>` is tried first, then `pyenv-sh-<command>` [src libexec/pyenv-help:24-27].
- **Not found.** The message goes to stderr and the exit code is 1 [src libexec/pyenv-help:170-171]
  [test test/help.bats:12-15]:

  ```text
  pyenv: no such command `<command>'
  ```

- **Output.** The Usage section is printed first. If the block has a Summary but no Usage, the line
  `Usage: pyenv <command>` is printed instead. Next, if the extended help is non-empty, the output continues with an
  empty line, the help text, and another empty line [src libexec/pyenv-help:122-132]. When the extended help is
  empty, the Summary is used in its place [src libexec/pyenv-help:120] [test test/help.bats:36-52].
- **Trailing empty line.** Whenever help text or a Summary exists, the output therefore ends with `\n\n` [probe].
  When only a Usage exists, as in `version-file-read`, the output is just the usage lines.
- **Summary hidden.** If the extended help is non-empty, the Summary is not printed at all. [probe]: `pyenv help
  version-name` shows the `-f/--force` line, not its Summary.
- **Undocumented command.** With neither Usage nor Summary, the message goes to stderr and the exit code is 1
  [src libexec/pyenv-help:133-136]:

  ```text
  Sorry, this command isn't documented yet.
  ```

### Mode 3: `pyenv help --usage <command>`

The command prints the Usage section only if one exists. Otherwise it prints nothing. The exit code is 0
[src libexec/pyenv-help:139-144]. [probe]: the output was empty for `root`, `--version`, `version-name`,
`version-origin` and `rehash`. Commands use this mode internally for their own usage errors, sending the output to
stderr before exiting 1. `pyenv version` does this, for example [src libexec/pyenv-version:27-28].

### Comment-block parsing

[src libexec/pyenv-help:29-90] [test test/help.bats:17-115]

1. **Block extent.** The block is read from line 1 for as long as lines start with `#`. `sed` quits at the first line
   that does not [src libexec/pyenv-help:32-35].
2. **Line transform** [src libexec/pyenv-help:37-42]:
   - A line that is exactly `#` becomes an empty line.
   - A line starting with `# ` is kept without those two characters.
   - Any other `#` line is dropped, such as the shebang `#!/usr/bin/env bash`.
3. **Summary.** A line starting `Summary:` sets the summary to the text from column 10 onward, which is everything
   after `Summary: ` [src libexec/pyenv-help:50-53].
4. **Usage.** A line starting `Usage:` starts the usage section [src libexec/pyenv-help:55-59]. While in usage, a
   following line that is empty, all spaces, or starts with 7 spaces is appended to usage
   [src libexec/pyenv-help:61-64]. Any other line is appended to the extended help and ends the usage section
   [src libexec/pyenv-help:66-69]. So in `pyenv local`, the flag line `  -f/--force ...` (two spaces) goes to the
   extended help.
5. **Trimming.** Leading and trailing newlines are removed from both usage and help [src libexec/pyenv-help:76-86].
   Trailing spaces inside lines are kept. [probe]: `pyenv help which` keeps the trailing space in
   `...search command in the `.
6. **Tools.** The awk program runs under `gawk` if it is on `PATH`, else `awk` [src libexec/pyenv-help:49]. `sed` runs
   with `LC_ALL= LC_CTYPE=C` [src libexec/pyenv-help:30-31].

**Help block** [src libexec/pyenv-help:1-13]:

```text
#!/usr/bin/env bash
#
# Summary: Display help for a command
#
# Usage: pyenv help [--usage] COMMAND
#
# Parses and displays help contents from a command's source file.
#
# A command is considered documented if it starts with a comment block
# that has a `Summary:' or `Usage:' section. Usage instructions can
# span multiple lines as long as subsequent lines are indented.
# The remainder of the comment block is displayed as extended
# documentation.
```

Rendered `pyenv help help` [probe]:

```text
Usage: pyenv help [--usage] COMMAND

Parses and displays help contents from a command's source file.

A command is considered documented if it starts with a comment block
that has a `Summary:' or `Usage:' section. Usage instructions can
span multiple lines as long as subsequent lines are indented.
The remainder of the comment block is displayed as extended
documentation.

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv version-file` — `libexec/pyenv-version-file`

**Synopsis.** `pyenv version-file [<dir>]` [src libexec/pyenv-version-file:2]. There are no flags.

**Search** (`find_local_version_file`) [src libexec/pyenv-version-file:9-36]:
1. A non-absolute `root` is first resolved with `$(CDPATH= cd -- "$root" && pwd)`, which gives a logical path.
2. One trailing `/` is stripped.
3. The loop checks whether `$root/.python-version` is a regular file (`-f`, which follows symlinks). If it is, the
   function prints that path and returns 0. Otherwise it drops the last path component and repeats.
4. `/.python-version` is the last candidate checked.
5. `//name` UNC roots stop before testing `//.python-version` [src libexec/pyenv-version-file:19,26].

**With `<dir>`** [src libexec/pyenv-version-file:38-39]. Only that search is performed.
- **Not found.** Nothing is printed and the exit code is 1 [test test/version-file.bats:72-75].
- **Nonexistent relative dir.** [probe] printed the following to stderr and exited 1:

  ```text
  /opt/pyenv/libexec/pyenv-version-file: line 13: cd: nosuchdir: No such file or directory
  ```

- **Nonexistent absolute dir.** [probe] exited 1 with no output. The source comment calls nonexistent paths undefined
  behavior [src libexec/pyenv-version-file:11-12].

**Without `<dir>`** [src libexec/pyenv-version-file:40-46]. The search starts at `PYENV_DIR`, which defaults to `$PWD`
when unset. If that finds nothing and `PYENV_DIR` differs from `$PWD`, the search is repeated from `$PWD`. If that also
fails, the command prints `${PYENV_ROOT}/version` **whether or not the file exists** and exits 0
[test test/version-file.bats:15-64].

**Output.** The path followed by `\n`. [probe]: `cd / && PYENV_DIR=/home/tester/proj/sub pyenv version-file` printed
`/home/tester/proj/.python-version`.

**Edge cases.**
- **Directory.** A directory named `.python-version` is skipped because of the `-f` test.
- **Empty file.** An **empty** `.python-version` is still returned [probe]. It therefore shadows the parent
  directories and the global file, and `pyenv version-name` then yields `system` (see `version-name`).

**Environment.** Reads `PYENV_DIR`, `PWD` and `PYENV_ROOT`. Exports nothing.

**Help block** [src libexec/pyenv-version-file:1-3] (Usage precedes Summary here):

```text
#!/usr/bin/env bash
# Usage: pyenv version-file [<dir>]
# Summary: Detect the file that sets the current pyenv version
```

Rendered `pyenv help version-file` [probe]:

```text
Usage: pyenv version-file [<dir>]

Detect the file that sets the current pyenv version

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv version-file-read` — `libexec/pyenv-version-file-read`

**Synopsis.** `pyenv version-file-read <file>` [src libexec/pyenv-version-file-read:2]. There are no flags.

**Algorithm** [src libexec/pyenv-version-file-read:23-43]:
1. **Guard.** The file must exist and be non-empty (`[ -s ]`). Otherwise the command exits 1 with no output
   [test test/version-file-read.bats:10-24].
2. **Separators.** `\r` is appended to `IFS` [src libexec/pyenv-version-file-read:26].
3. **Reading.** The loop is `while read -n 1024 -r version _ || [[ $version ]]`. Each read takes the first word
   (split on space, tab and `\r`) as the version and discards the rest of the line. A last line without `\n` is still
   read [test test/version-file-read.bats:62-66].
4. **Skipped entries.** An entry is skipped if it is empty or starts with `#` [src libexec/pyenv-version-file-read:29-31].
   That covers blank lines, whitespace-only lines, and lines whose first word starts with `#`; the `#` may follow
   leading spaces [test test/version-file-read.bats:74-84].
5. **Path-safety check.** An entry that equals `..` or contains `/` must pass `is_version_safe`
   [src libexec/pyenv-version-file-read:8-21]. `cd "$PYENV_ROOT/versions/$version"` must succeed, and the resulting
   logical `$PWD` must start with `$PYENV_ROOT/versions/`. An entry that fails is skipped with a stderr message
   [src libexec/pyenv-version-file-read:32-35]. `<file>` is printed as it was given, so it may be relative:

   ```text
   pyenv: invalid version `<version>' ignored in `<file>'
   ```

6. **Result.** The accepted entries are joined with `:` and followed by `\n`. The exit code is 0
   [src libexec/pyenv-version-file-read:37-40]. If nothing was accepted, the exit code is 1 and stdout is empty
   [src libexec/pyenv-version-file-read:43].

Only the path-safety check applies. Plain names are **not** checked for existence.

**Edge cases** [probe] unless tagged otherwise.

| File content | stdout | stderr | exit |
|---|---|---|---|
| `3.12.1\r\n3.11.9\r\n` (CRLF) | `3.12.1:3.11.9` | none | 0 |
| `  3.12.1 extra words\n# comment\n  #3.11.9\n\n2.7.18` (no final newline) | `3.12.1:2.7.18` | none | 0 |
| `\t3.10.4\tfoo\n` | `3.10.4` | none | 0 (exit code not captured; the source's only stdout path exits 0, `:40`) |
| `\r\n` | none | none | 1 (observed through `pyenv local`) |
| empty file (0 bytes) | none | none | 1 [src libexec/pyenv-version-file-read:23,43] |
| `\n` (1 byte) | none | none | 1 [test test/version-file-read.bats:20-24] |
| `..\n../3.12.1\n3.12/../3.12.1\n3.12.1\n`, with no `3.12` directory | `3.12.1` | three `invalid version` lines, see below | 0 |
| `..\n` | none | ``pyenv: invalid version `..' ignored in `my-version'`` | 1 [test test/version-file-read.bats:86-90] |
| `3.10.3/envs/../test`, existing and inside `versions` | `3.10.3/envs/../test` | none | 0 [test test/version-file-read.bats:104-110] |
| a single 1500-character word | 1501 characters: the first 1024 characters, `:`, then the remaining 476 | none | 0 |

The three `invalid version` lines for the mixed file were:

```text
pyenv: invalid version `..' ignored in `.python-version'
pyenv: invalid version `../3.12.1' ignored in `.python-version'
pyenv: invalid version `3.12/../3.12.1' ignored in `.python-version'
```

- **Long words.** The 1500-character case splits because `read -n 1024` returns after 1024 characters, and the next
  read continues on the same line.
- **`.`** An entry of `.` contains no `/` and is not `..`, so it is not checked and is accepted. This follows from
  [src libexec/pyenv-version-file-read:11] and was not executed for a file.

**Help block** [src libexec/pyenv-version-file-read:1-2]. It has a Usage but no Summary, so the command is not in the
`pyenv help` listing.

```text
#!/usr/bin/env bash
# Usage: pyenv version-file-read <file>
```

Rendered `pyenv help version-file-read` [probe]. It ends with a single `\n`:

```text
Usage: pyenv version-file-read <file>
```

---

## `pyenv version-file-write` — `libexec/pyenv-version-file-write`

**Synopsis.** `pyenv version-file-write [-f|--force] <file> <version> [...]`
[src libexec/pyenv-version-file-write:2].

**Flags.** Any number of leading `-f`/`--force` flags are accepted [src libexec/pyenv-version-file-write:9-20].

**Behavior.**
- **Arguments.** If the file or the first version is empty, the usage goes to stderr and the exit code is 1
  [src libexec/pyenv-version-file-write:27-30] [test test/version-file-write.bats:10-15]:

  ```text
  Usage: pyenv version-file-write [-f|--force] <file> <version> [...]
  ```

- **Validation.** Unless forced, the versions are checked with `pyenv-prefix "${versions[@]}" >/dev/null`
  [src libexec/pyenv-version-file-write:33]. `pyenv-prefix`'s stderr message passes through, the exit code is 1, and
  the file is **not touched**, because the check runs before the file is truncated
  [test test/version-file-write.bats:17-22]. The possible messages are:
  - ``pyenv: version `<v>' not installed``
  - ``pyenv: system version not found in PATH``
- **Writing.** The file is truncated [src libexec/pyenv-version-file-write:37]. Then each version is appended as
  `<version>\n` [src libexec/pyenv-version-file-write:38-40]. Nothing goes to stdout, and the exit code is 0.
- **Missing parent directory.** The redirection fails. [probe] printed the following to stderr and exited 1:

  ```text
  /opt/pyenv/libexec/pyenv-version-file-write: line 37: /tmp/newroot/version: No such file or directory
  ```

**Help block** [src libexec/pyenv-version-file-write:1-4]:

```text
#!/usr/bin/env bash
# Usage: pyenv version-file-write [-f|--force] <file> <version> [...]
#
#   -f/--force    Don't verify that the versions exist
```

Rendered `pyenv help version-file-write` [probe]:

```text
Usage: pyenv version-file-write [-f|--force] <file> <version> [...]

  -f/--force    Don't verify that the versions exist

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv version-name` — `libexec/pyenv-version-name`

**Synopsis.** `pyenv version-name [-f|--force]`. The `-f` flag is internal: it prints missing versions as they are
instead of failing [src libexec/pyenv-version-name:4, 9-20].

**Algorithm.**
1. **Read the file.** If `PYENV_VERSION` is empty or unset, the command runs
   `PYENV_VERSION_FILE="$(pyenv-version-file)"` and then
   `PYENV_VERSION="$(pyenv-version-file-read "$PYENV_VERSION_FILE" || true)"`
   [src libexec/pyenv-version-name:23-26]. Any `invalid version` messages from `version-file-read` pass through to
   stderr.
2. **Hooks.** The `version-name` hooks are sourced next. They may change `PYENV_VERSION`
   [src libexec/pyenv-version-name:28-33] [test test/version-name.bats:33-40].
3. **System.** If `PYENV_VERSION` is empty or exactly `system`, the command prints `system` and exits 0
   [src libexec/pyenv-version-name:35-38]. The existence of a system Python is not checked
   [test test/version-name.bats:20-23].
4. **Resolve each entry.** `PYENV_VERSION` is split on `:` with globbing disabled
   [src libexec/pyenv-version-name:47-53]. For each entry `version`, the first matching rule applies:
   1. `normalised` is `version` with one leading `python-` removed [src libexec/pyenv-version-name:55]. Once any entry
      has had the prefix removed, the flag `normalization_done` stays set for all later entries
      [src libexec/pyenv-version-name:49,56].
   2. If `version` is exactly `system`, or `$PYENV_ROOT/versions/<normalised>` is a directory (`-d`, which follows
      symlinks), `normalised` is accepted [src libexec/pyenv-version-name:57-58].
   3. Else, if `normalization_done` is set and `$PYENV_ROOT/versions/<version>` is a directory, `version` is accepted
      unchanged [src libexec/pyenv-version-name:59-60]. [probe]: an install named `python-3.10` resolved to
      `python-3.10` [test test/version-name.bats:144-149].
   4. Else, if `pyenv-latest -b <normalised>` succeeds, its output is accepted [src libexec/pyenv-version-name:61-62].
      This is where prefix resolution happens (see `pyenv latest`) [test test/version-name.bats:130-142].
   5. Else, if `normalization_done` is set, `pyenv-latest -b <version>` is tried
      [src libexec/pyenv-version-name:63-64].
   6. Else, with `-f`, `normalised` is accepted [src libexec/pyenv-version-name:66-67].
   7. Otherwise a message goes to stderr and the entry is marked as failed [src libexec/pyenv-version-name:69].
      `<version>` is the entry as written, before `python-` removal. `<origin>` is the output of `pyenv-version-origin`.

      ```text
      pyenv: version `<version>' is not installed (set by <origin>)
      ```
5. **Print.** The accepted entries are joined with `:` and printed to stdout, followed by `\n`. This happens **even
   when some or all entries failed**, and when all failed the output is an empty line
   [src libexec/pyenv-version-name:78-82]. [probe] with `PYENV_VERSION=8.8:9.9`: stdout is `\n`.
6. **Exit code.** 1 if any entry failed, else 0 [src libexec/pyenv-version-name:84-86].

**Tests.**
- The `(set by PYENV_VERSION environment variable)` suffix: [test test/version-name.bats:79-82].
- Partial failure, with the message for the missing entry and the found one still printed:
  [test test/version-name.bats:89-120].
- Precedence is `PYENV_VERSION`, then the local file, then the global file [test test/version-name.bats:54-77].

**Probe results.** Installed versions were `3.12.1 3.12.9 3.12.10 3.12.0rc1 3.12-dev 3.12.2t 3.11.9 3.1.4 2.7.18
pypy3.10-7.3.9 pypy3.10-7.3.17 miniconda3-4.7.12 miniconda3-24.1.2-0 miniconda3-latest`.

| `PYENV_VERSION` | stdout | stderr | exit |
|---|---|---|---|
| unset, no files | `system` | none | 0 |
| `3.12` | `3.12.10` | none | 0 |
| `3` | `3.12.10` | none | 0 |
| `3.1` | `3.1.4` (not `3.10`-anything) | none | 0 |
| `3.12t` | `3.12.2t` | none | 0 |
| `python-3.12` | `3.12.10` | none | 0 |
| `python-3.12.1` | `3.12.1` | none | 0 |
| `pypy3.10` | `pypy3.10-7.3.17` | none | 0 |
| `miniconda3` | `miniconda3-24.1.2-0` | none | 0 |
| `4` | empty line | ``pyenv: version `4' is not installed (set by PYENV_VERSION environment variable)`` | 1 |
| `system:3.12` | `system:3.12.10` | none | 0 |
| `:3.12.1` | `:3.12.1` | none | 0 |
| `3.12.1:` | `3.12.1` | none | 0 |
| `3.12.1::3.11.9` | `3.12.1::3.11.9` | none | 0 |
| `3.12.1 3.11.9` (space) | empty line | ``pyenv: version `3.12.1 3.11.9' is not installed (set by PYENV_VERSION environment variable)`` | 1 |
| `..` | `..` | none | 0 |
| `../../../../etc` | `../../../../etc` | none | 0 |
| `.` | `.` | none | 0 |
| `3.14`, when only `3.14.0rc1` is installed | empty line | ``pyenv: version `3.14' is not installed (set by PYENV_VERSION environment variable)`` | 1 |

- **Empty fields.** In the `:3.12.1` case the empty first field is accepted, because `$PYENV_ROOT/versions/` is a
  directory. `3.12.1:` loses its trailing empty field to bash word splitting.
- **No path-safety check on `PYENV_VERSION`.** The `..` and `/` filtering exists only in `pyenv-version-file-read`.
  `PYENV_VERSION` is used as given and passes whenever `-d $PYENV_ROOT/versions/<value>` is true.

**File-based cases** [probe]:
- **Invalid entries.** With a `.python-version` containing only `..`, stdout is `system`. Stderr is
  ``pyenv: invalid version `..' ignored in `/home/tester/proj/.python-version'``, and the exit code is 0.
- **Empty file.** An empty local `.python-version` gives `system` with exit 0, even if the global file names a version.
  This follows from `version-file` returning the empty file.
- **CRLF.** A CRLF local file `3.12.1\r\n3.11.9\r\n` gives `3.12.1:3.11.9`.

**Environment.** Reads `PYENV_VERSION`, `PYENV_ROOT`, and, through its helpers, `PYENV_DIR`, `PWD` and
`PYENV_HOOK_PATH`. Exports nothing.

**Help block** [src libexec/pyenv-version-name:1-4]:

```text
#!/usr/bin/env bash
# Summary: Show the current Python version
#
#   -f/--force    (Internal) If a version doesn't exist, print it as is rather than produce an error
```

Rendered `pyenv help version-name` [probe]. The Summary is not shown, because the extended help is non-empty.
`--usage` prints nothing.

```text
Usage: pyenv version-name

  -f/--force    (Internal) If a version doesn't exist, print it as is rather than produce an error

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv latest` — `libexec/pyenv-latest`

**Synopsis.** `pyenv latest [-k|--known] <prefix>` [src libexec/pyenv-latest:3].

**Flags.** Flags are parsed only before the prefix [src libexec/pyenv-latest:14-34]:
- `-k`/`--known` selects candidates from `python-build --definitions` instead of installed versions.
- `-b`/`--bypass` is internal. On failure it prints the prefix unchanged instead of the error message.
- `-f`/`--force` is internal. It behaves like `-b` but also exits 0.

Only the first argument after the flags is used as the prefix [src libexec/pyenv-latest:36].

**Algorithm, and what "newest" means.**
1. **Exact directory.** In installed mode, if `$PYENV_ROOT/versions/<prefix>` is a directory, the command prints
   `<prefix>` unchanged and exits 0 [src libexec/pyenv-latest:41-45].
   - [probe]: `pyenv latest 3.12-dev` printed `3.12-dev`.
   - [probe]: `pyenv latest` with no argument printed an **empty line** and exited 0, because
     `$PYENV_ROOT/versions/` is a directory.
2. **Candidates.** In installed mode the candidates come from `pyenv-versions --bare --skip-envs`
   [src libexec/pyenv-latest:46]. That list includes symlink aliases and dot-directories and excludes envs. In known
   mode they come from `python-build --definitions` [src libexec/pyenv-latest:48].
3. **Exact match.** If the prefix equals a candidate line exactly, the command prints it and exits 0
   [src libexec/pyenv-latest:51-54] [test test/latest.bats:51-63].
4. **`t` suffix.** If the prefix matches `^(.*[0-9])t$`, the `t` is stripped and saved as a required suffix
   [src libexec/pyenv-latest:56-60].
5. **Prefix filter.** The candidates are kept only if they match the ERE `^<prefix>[-.].*<suffix>$`, with `<prefix>`
   escaped literally [src libexec/pyenv-latest:62-68]. The prefix therefore ends only at a `-` or `.` boundary.
   - [probe]: `3.1` matched `3.1.4` and never `3.10.x`.
   - [probe]: `3.12.1` did not match `3.12.10`, and the exit code was 1.
   - [probe]: `pypy` matched nothing, while `pypy3` matched `pypy3.10-7.3.17`.
6. **Exclusions.** The command drops candidates that end in `-dev`, `-src`, `-latest` or `(a|b|rc)[0-9]+`. Without
   the `t` suffix it also drops those ending in `<digit>t` [src libexec/pyenv-latest:70-74]
   [test test/latest.bats:79-98].
   - With the `t` suffix, a prerelease `t` build survives, because `(a|b|rc)[0-9]+$` does not match a name ending in `a1t`. [probe]:
     `pyenv latest 3.13t`, with only `3.13.0a1t` installed, printed `3.13.0a1t`.
   - Without the suffix, a prefix whose only matches are prereleases fails. [probe]: `pyenv latest 3.14`, with only
     `3.14.0rc1` installed, failed.
7. **Sort key** [src libexec/pyenv-latest:77-84]. A candidate that starts with `[[:alnum:]]+-`, such as
   `miniconda3-24.1.2-0`, gets the key `<name>.<rest>..|<candidate>`. The dash after the leading name becomes `.`.
   [probe]: `substr($0,0,RLENGTH-1)` gives the full name `miniconda3` in both gawk and mawk. Any other candidate gets
   the key `<candidate>...|<candidate>`.
8. **Sort** [src libexec/pyenv-latest:85-90]. The keys are sorted with `sort -t. -k1,1r -k2,2nr -k3,3nr -k4,4nr`, and
   the first line wins after the key is cut off.
   - Field 1 is compared as text, descending. Fields 2 to 4 are compared as numbers, descending.
   - GNU `-n` uses the leading number of a field, so `10t` is 10, `4-debug` is 4, and an empty field is 0.
   - Only four dot-fields take part.
   - Ties fall to `sort`'s last-resort comparison of the whole line, ascending and **locale-collated**. [probe]:
     between `3.10.4` and `3.10.4-debug`, `pyenv latest 3.10` printed `3.10.4-debug` under C.UTF-8, and a direct
     `sort` check with `LC_ALL=en_US.UTF-8` put `3.10.4` first.
   - Tests: `3.10.8` beats `3.10.6` and `3.5.6` [test test/latest.bats:65-77], and `3t` resolves to `3.13.5t`
     [test test/latest.bats:100-112].
9. **Result** [src libexec/pyenv-latest:92-105]. On success the winner is printed and the exit code is 0. When nothing
   matches:
   - **Default.** The message goes to stderr and the exit code is 1 [test test/latest.bats:27-49]. `installed` becomes
     `known` with `-k`:

     ```text
     pyenv: no installed versions match the prefix `<prefix>'
     ```

   - **`-b`.** `<prefix>` is printed on stdout and the exit code is 1 [test test/latest.bats:114-121].
   - **`-f`.** `<prefix>` is printed on stdout and the exit code is 0 [test test/latest.bats:123-130].
   - **`t` suffix.** After step 4, `<prefix>` in the message or echo lacks the stripped `t`. This follows from
     [src libexec/pyenv-latest:59] and was not executed.

**Missing `python-build`.** With `-k` and no `python-build` on `PATH`, [probe] printed the following to stderr and
exited **127**:

```text
/opt/pyenv/libexec/pyenv-latest: line 48: python-build: command not found
```

**Where prefix resolution is used.**
- `pyenv-version-name`, through `pyenv-latest -b` [src libexec/pyenv-version-name:61,63]. This reaches `pyenv version`,
  the `pyenv versions` markers, and the `pyenv exec` version.
- `pyenv-prefix`, through `pyenv-latest -f` [src libexec/pyenv-prefix:48]. This reaches `pyenv which`, `pyenv whence`,
  and the validation in `local`/`global`/`version-file-write`.
- It is **not** used by `pyenv-version-file-read` or `pyenv-version-origin`.
- `python-` removal happens only in `pyenv-version-name`.

**Help block** [src libexec/pyenv-latest:1-8]:

```text
#!/usr/bin/env bash
# Summary: Print the latest installed or known version with the given prefix
# Usage: pyenv latest [-k|--known] <prefix>
#
#   -k/--known      Select from all known versions instead of installed
#   -b/--bypass     (internal) On a resolution failure, do not print an error message
#                   but rather print the argument unchanged
#   -f/--force      (internal) Same as -b but also do not return a failure exit code
```

Rendered `pyenv help latest` [probe]. `--usage` prints `Usage: pyenv latest [-k|--known] <prefix>`.

```text
Usage: pyenv latest [-k|--known] <prefix>

  -k/--known      Select from all known versions instead of installed
  -b/--bypass     (internal) On a resolution failure, do not print an error message
                  but rather print the argument unchanged
  -f/--force      (internal) Same as -b but also do not return a failure exit code

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv version-origin` — `libexec/pyenv-version-origin`

**Synopsis.** `pyenv version-origin`. There are no flags.

**Algorithm.**
1. `PYENV_VERSION_ORIGIN` is unset on entry, so any value from the environment is ignored
   [src libexec/pyenv-version-origin:6] [test test/version-origin.bats:53-56].
2. The `version-origin` hooks are sourced. They may set `PYENV_VERSION_ORIGIN`
   [src libexec/pyenv-version-origin:8-13] [test test/version-origin.bats:34-39].
3. The output is chosen in this order [src libexec/pyenv-version-origin:15-21]:
   - `$PYENV_VERSION_ORIGIN`, if it is non-empty;
   - else, if `PYENV_VERSION` is non-empty, the literal `PYENV_VERSION environment variable`;
   - else, the output of `pyenv-version-file`. That is the local file path, or `<PYENV_ROOT>/version` even when that
     file does not exist [test test/version-origin.bats:10-32].

The exit code is always 0.

**Edge case.** The origin names the local file even when that file is empty or all its entries were rejected [probe].

**Help block** [src libexec/pyenv-version-origin:1-2]:

```text
#!/usr/bin/env bash
# Summary: Explain how the current Python version is set
```

Rendered `pyenv help version-origin` [probe]. `--usage` prints nothing.

```text
Usage: pyenv version-origin

Explain how the current Python version is set

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv version` — `libexec/pyenv-version`

**Synopsis.** `pyenv version [--bare]` [src libexec/pyenv-version:3].

**Algorithm.**
1. **Resolve first.** `IFS=: PYENV_VERSION_NAMES=($(pyenv-version-name)) || exitcode=$?` runs with globbing disabled,
   **before** any argument is parsed [src libexec/pyenv-version:15]. Its stderr passes through.
2. **Arguments** [src libexec/pyenv-version:20-31]:
   - `--complete` prints `--bare` and exits 0.
   - `--bare` selects bare output.
   - Anything else sends the usage to stderr and exits 1. Any messages from step 1 have already been printed.
     [probe]: `PYENV_VERSION=9.9 pyenv version --foo` printed the not-installed line and then the usage line, both on
     stderr, and exited 1.

   ```text
   Usage: pyenv version [--bare]
   ```

3. **Output.** Each name gets one line [src libexec/pyenv-version:32-38]. The line is `<name> (set by <origin>)`, or
   just `<name>` with `--bare`. `<origin>` is recomputed by `pyenv-version-origin` for every line.
4. **Exit code.** The exit status of `pyenv-version-name` [src libexec/pyenv-version:15,40].

**Outputs** [test test/version.bats:14-38] [probe]:

```text
system (set by <PYENV_ROOT>/version)
3.3.3 (set by PYENV_VERSION environment variable)
3.3.3 (set by <dir>/.python-version)
3.3.3 (set by <PYENV_ROOT>/version)
```

**Missing versions** [test test/version.bats:40-72].
- The stderr lines come first: one ``pyenv: version `<v>' is not installed (set by <origin>)`` for each missing
  version, in order. The found versions are then printed on stdout, and the exit code is 1.
- When all versions are missing, stdout is empty. The empty line from `version-name` produces no array element, so no
  line is printed [test test/version.bats:83-90] [probe].

**Other cases** [probe]:
- `PYENV_VERSION=system:3.12` printed `system (set by PYENV_VERSION environment variable)` and then
  `3.12.10 (set by PYENV_VERSION environment variable)`.
- With an empty or comment-only global file and no local file, the output was
  `system (set by <PYENV_ROOT>/version)` with exit 0.
- A comment-only local `.python-version` **shadows** a global file that names `3.12.1`. The output was
  `system (set by /home/tester/proj/.python-version)` with exit 0.

**Help block** [src libexec/pyenv-version:1-5]:

```text
#!/usr/bin/env bash
# Summary: Show the current Python version(s) and its origin
# Usage: pyenv version [--bare]
#
#     --bare    show just the version name. An alias to `pyenv version-name'
```

Rendered `pyenv help version` [probe]. `--usage` prints `Usage: pyenv version [--bare]`.

```text
Usage: pyenv version [--bare]

    --bare    show just the version name. An alias to `pyenv version-name'

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv local` — `libexec/pyenv-local`

**Synopsis.** `pyenv local [-f|--force] [<version> [...]]` and `pyenv local --unset` [src libexec/pyenv-local:5-6].

**Flags.**
- `--complete` as the first argument prints `--unset`, `system`, then `pyenv-versions --bare`
  [src libexec/pyenv-local:35-39].
- Any number of leading `-f`/`--force` flags are accepted [src libexec/pyenv-local:41-52].

**Modes** [src libexec/pyenv-local:54-74].
- **Unset.** If the first non-flag argument is `--unset`, the command runs `rm -f .python-version` in `$PWD`. Any
  further arguments are ignored. There is no output and the exit code is 0, whether or not the file existed
  [test test/local.bats:73-78] [probe].
- **Set.** Otherwise, if at least one version is given, the command runs
  `pyenv-version-file-write ${FORCE:+-f }.python-version <versions...>`.
  - **Target.** The file written is `./.python-version` in `$PWD`. That is neither `PYENV_DIR` nor a
    `.python-version` found in a parent directory [probe].
  - **Validation.** Each version must resolve through `pyenv-prefix`, so a prefix is accepted. The **literal
    argument** is written, not the resolved version. [probe]: `pyenv local 3.12` wrote `3.12\n`.
  - **Several versions.** They are written one per line [probe], for example `3.12.1\n3.11.9\n`.
  - **Output.** Nothing on success, with exit 0 [test test/local.bats:44-49].
- **Show.** With no versions given, the file is located with `pyenv-version-file "$PWD"`. This walks up from `$PWD`,
  **ignores `PYENV_DIR`** [test test/local.bats:36-42], and never falls back to the global file.
  - The file is read with `pyenv-version-file-read`, split on `:` with globbing disabled, and printed one version per
    line [test test/local.bats:16-34].
  - **No file.** The message goes to stderr and the exit code is 1 [src libexec/pyenv-local:71-72]
    [test test/local.bats:10-14]:

    ```text
    pyenv: no local version configured for this directory
    ```

**Errors when setting.** The validation messages from `pyenv-prefix` go to stderr, the exit code is 1, and the file is
left untouched.
- **Missing version.** Note that this message has **no** "is" and **no** `(set by ...)` suffix, unlike the
  `version-name` message [test test/local.bats:51-55]:

  ```text
  pyenv: version `<version>' not installed
  ```

- **Several versions with one missing.** `pyenv-prefix` exits at the first missing version, so only that one is
  reported [src libexec/pyenv-prefix:51-56]. [probe]: `pyenv local 3.12.1 9.9` printed
  ``pyenv: version `9.9' not installed`` and exited 1.
- **`system` without a system Python.** The message is ``pyenv: system version not found in PATH`` [probe].
- **`system` whose Python is not in a `bin` or `sbin` directory.** The message is
  ``pyenv: version `system' not installed`` (see `pyenv prefix`) [probe].
- **`python-` prefix.** `python-3.12.1` is rejected with ``pyenv: version `python-3.12.1' not installed``, even though
  `pyenv version-name` accepts that form [probe]. `pyenv-prefix` does not strip `python-`.
- **Forcing.** `-f`/`--force` skips validation. [probe]: `pyenv local -f 9.9` wrote `9.9\n`.

**Edge cases.**
- **`..`** `pyenv local ..` is accepted without output and exits 0, because `-d $PYENV_ROOT/versions/..` is true. It
  writes `..\n`, which every later read rejects with the `invalid version` message [probe].
- **Unusable file.** An empty, CR-only, comment-only or all-invalid (`..`) `.python-version` in show mode makes the
  command exit **1** with **no** `no local version configured` message. Stdout is empty. Stderr is empty, or carries
  only the `invalid version` lines [probe]. The reason is that the assignment
  `IFS=: versions=($(pyenv-version-file-read ...))` fails under `set -e` [src libexec/pyenv-local:65].
- **CRLF.** A CRLF file shows one version per line with the CRs removed [probe].

**Environment.** Reads `PWD` and `PYENV_ROOT`. `PYENV_DIR` is not used in show mode. Exports nothing.

**Help block** [src libexec/pyenv-local:1-28]:

```text
#!/usr/bin/env bash
#
# Summary: Set or show the local application-specific Python version(s)
#
# Usage: pyenv local [-f|--force] [<version> [...]]
#        pyenv local --unset
#
#   -f/--force    Do not verify that the versions being set exist
#
# Sets the local application-specific Python version(s) by writing the
# version name to a file named `.python-version'.
#
# When you run a Python command, pyenv will look for a `.python-version'
# file in the current directory and each parent directory. If no such
# file is found in the tree, pyenv will use the global Python version
# specified with `pyenv global'. A version specified with the
# `PYENV_VERSION' environment variable takes precedence over local
# and global versions.
#
# <version> can be specified multiple times and should be a version
# tag known to pyenv.  The special version string `system' will use
# your default system Python.  Run `pyenv versions' for a list of
# available Python versions.
#
# Example: To enable the python2.7 and python3.7 shims to find their
#          respective executables you could set both versions with:
#
# 'pyenv local 3.7.0 2.7.15'
```

Rendered `pyenv help local` [probe]:

```text
Usage: pyenv local [-f|--force] [<version> [...]]
       pyenv local --unset

  -f/--force    Do not verify that the versions being set exist

Sets the local application-specific Python version(s) by writing the
version name to a file named `.python-version'.

When you run a Python command, pyenv will look for a `.python-version'
file in the current directory and each parent directory. If no such
file is found in the tree, pyenv will use the global Python version
specified with `pyenv global'. A version specified with the
`PYENV_VERSION' environment variable takes precedence over local
and global versions.

<version> can be specified multiple times and should be a version
tag known to pyenv.  The special version string `system' will use
your default system Python.  Run `pyenv versions' for a list of
available Python versions.

Example: To enable the python2.7 and python3.7 shims to find their
         respective executables you could set both versions with:

'pyenv local 3.7.0 2.7.15'

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

Rendered `pyenv help --usage local` [probe]:

```text
Usage: pyenv local [-f|--force] [<version> [...]]
       pyenv local --unset
```

---

## `pyenv global` — `libexec/pyenv-global`

**Synopsis.** `pyenv global <version> <version2> <..>` [src libexec/pyenv-global:5]. The command has no `-f` and no
`--unset`.

**Flags.** `--complete` as the first argument prints `system` and then `pyenv-versions --bare`
[src libexec/pyenv-global:27-30].

**Set** [src libexec/pyenv-global:35-36]. The command runs
`pyenv-version-file-write "$PYENV_ROOT/version" <versions...>`. The validation, the literal writing and the error
messages are the same as for `pyenv local`, and the file is `<PYENV_ROOT>/version`
[test test/global.bats:27-39]:
- `pyenv global 9.9` prints ``pyenv: version `9.9' not installed`` and exits 1.
- `pyenv global 3.12.1 3.11.9` writes `3.12.1\n3.11.9\n` [probe].

**Show** [src libexec/pyenv-global:37-53]. The command tries `pyenv-version-file-read` on `<PYENV_ROOT>/version`, then
on `<PYENV_ROOT>/global`, then on `<PYENV_ROOT>/default`. If all three fail, it uses `system`. The result is split on
`:` and printed one version per line. The exit code is **always 0**.
- **No version set.** The output is the single line `system` with exit 0 [test test/global.bats:5-9]. This also
  happens with an empty or comment-only `version` file [probe].
- **Legacy files.** `pyenv global` reads `PYENV_ROOT/global` and `PYENV_ROOT/default`, but `pyenv-version-name` and
  `pyenv-version-file` never read them. [probe]: with only `PYENV_ROOT/global` containing `3.11.9`, `pyenv global`
  printed `3.11.9` while `pyenv version-name` printed `system`.
- **`PYENV_VERSION`.** It is ignored. [probe]: `PYENV_VERSION=3.12.10 pyenv global` printed `system`.

**Argument quirks** [probe].
- **`--unset`.** `pyenv global --unset` treats `--unset` as a version. It printed
  ``pyenv: version `--unset' not installed`` and exited 1.
- **`-f`.** `pyenv global -f 3.12.1` exited 0 and wrote `-f\n3.12.1\n`.
  - Mechanism, derived from source and not traced: the `-f` word is passed as a version to
    `pyenv-version-file-write`, which parses flags only before the file argument [src libexec/pyenv-global:36]
    [src libexec/pyenv-version-file-write:9-23].
  - `pyenv-prefix` then runs `pyenv-latest -f -f` [src libexec/pyenv-prefix:48]. That treats both words as flags and
    leaves an empty prefix. `$PYENV_ROOT/versions/` is a directory, so validation succeeds
    [src libexec/pyenv-latest:25-29,41-45].
- **Missing root.** If `PYENV_ROOT` does not exist, the write fails with the bash `No such file or directory` message
  shown under `version-file-write`.

**Help block** [src libexec/pyenv-global:1-20]:

```text
#!/usr/bin/env bash
#
# Summary: Set or show the global Python version(s)
#
# Usage: pyenv global <version> <version2> <..>
#
# Sets the global Python version(s). You can override the global version at
# any time by setting a directory-specific version with `pyenv local'
# or by setting the `PYENV_VERSION' environment variable.
#
# <version> can be specified multiple times and should be a version
# tag known to pyenv.  The special version string `system' will use
# your default system Python.  Run `pyenv versions' for a list of
# available Python versions.
#
# Example: To enable the python2.7 and python3.7 shims to find their
#          respective executables you could set both versions with:
#
# 'pyenv global 3.7.0 2.7.15'
#
```

Rendered `pyenv help global` [probe]. `--usage` prints `Usage: pyenv global <version> <version2> <..>`.

```text
Usage: pyenv global <version> <version2> <..>

Sets the global Python version(s). You can override the global version at
any time by setting a directory-specific version with `pyenv local'
or by setting the `PYENV_VERSION' environment variable.

<version> can be specified multiple times and should be a version
tag known to pyenv.  The special version string `system' will use
your default system Python.  Run `pyenv versions' for a list of
available Python versions.

Example: To enable the python2.7 and python3.7 shims to find their
         respective executables you could set both versions with:

'pyenv global 3.7.0 2.7.15'

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv prefix` — `libexec/pyenv-prefix`

**Synopsis.** `pyenv prefix [<version>...]` [src libexec/pyenv-prefix:3].

**Flags.** `--complete` as the first argument prints `system` and then `pyenv-versions --bare`
[src libexec/pyenv-prefix:13-16].

**Algorithm.**
1. **Version list** [src libexec/pyenv-prefix:18-26].
   - With arguments, they are joined with `:` into `PYENV_VERSION`, which is exported. `PYENV_VERSION` from the
     environment is ignored in this case.
   - Otherwise a non-empty `PYENV_VERSION` is used.
   - Otherwise the command runs `PYENV_VERSION="$(pyenv-version-name)"`. If `version-name` fails, the assignment fails
     under `set -e`, so the command exits 1 after `version-name`'s message. This follows from `set -e` and was not
     probed.
2. **Each entry**, split on `:` with globbing disabled [src libexec/pyenv-prefix:28-59]:
   - **`system`.** The command runs `PYENV_VERSION=system pyenv-which python --skip-advice`, then `python3`, then
     `python2`, with stderr silenced [src libexec/pyenv-prefix:36-38]. The prefix is the found path with the shortest
     suffix matching `/bin/*` or `/sbin/*` removed, or `/` if that leaves nothing [src libexec/pyenv-prefix:39-42].
     - Examples: `/usr/bin/python` gives `/usr`. `/bin/python` gives `/` [test test/prefix.bats:25-52].
     - **No system Python.** The message goes to stderr and the exit code is 1 [src libexec/pyenv-prefix:44-45]
       [test test/prefix.bats:54-57]:

       ```text
       pyenv: system version not found in PATH
       ```

     - **Python outside `bin`/`sbin`.** If the found Python is not in a directory named `bin` or `sbin`, nothing is
       stripped. The file path is then not a directory, and the result is ``pyenv: version `system' not installed``
       with exit 1. [probe]: this happened with Python at `/home/tester/sysbin/python`.
   - **Other versions.** The entry is resolved with `version="$(pyenv-latest -f "$version")"`, so a prefix works and
     `python-` is not stripped. The prefix is `${PYENV_ROOT}/versions/<resolved>` [src libexec/pyenv-prefix:48-49].
   - **Directory check.** If the prefix is not a directory, the message goes to stderr and the exit code is 1
     [src libexec/pyenv-prefix:51-56] [test test/prefix.bats:14-23]. `<version>` is the value after `pyenv-latest -f`,
     which is the argument unchanged when nothing matched:

     ```text
     pyenv: version `<version>' not installed
     ```

3. **Output.** The prefixes are joined with `:` and printed on one line on stdout [src libexec/pyenv-prefix:62-66].

**Probe results** [probe]:

| Command | stdout | exit |
|---|---|---|
| `pyenv prefix 3.12` | `/home/tester/.pyenv/versions/3.12.10` | 0 |
| `pyenv prefix 3.12.1 3.11.9` | `/home/tester/.pyenv/versions/3.12.1:/home/tester/.pyenv/versions/3.11.9` | 0 |
| `pyenv prefix 3.12.10 system`, Python at `/home/tester/sys/bin/python` | `/home/tester/.pyenv/versions/3.12.10:/home/tester/sys` | 0 |
| `pyenv prefix`, nothing set, no system Python | none | 1 |

In the last row, stderr is `pyenv: system version not found in PATH`.

**Help block** [src libexec/pyenv-prefix:1-7]:

```text
#!/usr/bin/env bash
# Summary: Display prefixes for Python versions
# Usage: pyenv prefix [<version>...]
#
# Displays the directories where the given Python versions are installed,
# separated by colons. If no version is given, `pyenv prefix' displays the
# locations of the currently selected versions.
```

Rendered `pyenv help prefix` [probe]:

```text
Usage: pyenv prefix [<version>...]

Displays the directories where the given Python versions are installed,
separated by colons. If no version is given, `pyenv prefix' displays the
locations of the currently selected versions.

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv versions` — `libexec/pyenv-versions`

**Synopsis.** `pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]` [src libexec/pyenv-versions:3].

**Arguments.** They are processed left to right [src libexec/pyenv-versions:20-36]:
- `--complete` prints `--bare`, `--skip-aliases` and `--skip-envs`, then exits 0.
- `--executables` switches to rehash mode and stops argument parsing (see `pyenv rehash`).
- `--bare`, `--skip-aliases` and `--skip-envs` set their options.
- Any other argument sends the usage to stderr and exits 1 [probe]:

  ```text
  Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]
  ```

**Algorithm.**
1. **Versions directory.** `versions_dir` is `$PYENV_ROOT/versions`, passed through `realpath` if it exists
   [src libexec/pyenv-versions:86-88]. Without the native extension, the fallback `realpath` follows only the symlink
   chain of the final component, using logical `cd` [src libexec/pyenv-versions:69-83].
2. **Current versions.** In non-bare mode only, the output of `pyenv-version-name || true` is split on `:`
   [src libexec/pyenv-versions:95-115]. Its stderr passes through and its failure is ignored.
3. **System line.** In non-bare mode only, the command prints a line for `system` **first**, provided
   `PYENV_VERSION=system pyenv-which python --skip-advice`, or the same for `python3` or `python2`, succeeds
   [src libexec/pyenv-versions:156-161].
4. **Entries.** The entries are `"$versions_dir"/*` with dotglob and nullglob, so dot-directories such as `.venv` are
   included [src libexec/pyenv-versions:163-164] [test test/versions.bats:222-227]. They are sorted by
   `sort --version-sort` on the full paths if that is supported, and left in glob order if not
   [src libexec/pyenv-versions:165-178] [test test/versions.bats:229-269].
5. **Each entry that is a directory.** The `-d` test follows symlinks, so plain files and dangling symlinks are
   skipped [src libexec/pyenv-versions:180-197] [test test/versions.bats:165-171].
   - **`--skip-aliases`.** The entry is skipped if it is a symlink whose realpath's parent is `versions_dir`, or whose
     realpath has the form `<versions_dir>/*/envs/*`. Symlinks to directories outside `versions` are kept
     [test test/versions.bats:185-195]. The check applies only to top-level entries, so a symlinked env such as
     `3.12.9/envs/linkenv` is still listed [probe].
   - The entry is printed with `print_version <basename> <path>`.
   - **Envs.** Unless `--skip-envs` is given, each `"$path/envs/"*` that is a directory is printed as
     `<entry>/envs/<env>`. The name is computed as `${env_path#${PYENV_ROOT}/versions/}`. Envs are in **glob order,
     not version-sorted**, and they follow their parent entry.
6. **`print_version`** [src libexec/pyenv-versions:131-153].
   - `--bare` prints the name.
   - Otherwise the text is the name, or `<name> --> <readlink target>` when the path is a symlink. The target is the
     raw link text, one level only.
   - The line is `* <text> (set by <origin>)` when the name is exactly one of the current versions, and `  <text>`
     otherwise. `<origin>` comes from `pyenv-version-origin`.
7. **Nothing printed.** If no line at all was printed in non-bare mode, the warning goes to stderr and the exit code
   is 1 [src libexec/pyenv-versions:200-203] [test test/versions.bats:37-41]:

   ```text
   Warning: no Python detected on the system
   ```

   Otherwise the exit code is 0. `--bare` with nothing installed prints nothing and exits 0
   [test test/versions.bats:43-47].

**Probe results.**
- **No versions, system Python present:** `* system (set by <PYENV_ROOT>/version)` [test test/versions.bats:30-35].
- **No system Python but versions present:** there is no system line, and the version lines are printed without a
  `*` [probe].

Full listing [probe]. The layout was:
- installed: `3.9.1 3.10.0 3.12.10 3.12.9 3.12.0rc1 3.12-dev 2.7.18 pypy3.10-7.3.17 miniconda3-latest .venv "with space"`;
- envs `zeta`, `alpha`, `Beta` under `3.12.9`, plus an env symlink `linkenv -> ../../3.10.0`;
- aliases `3.12 -> 3.12.10`, `alpha -> 3.12.9/envs/alpha` and `ext -> /home/tester/external`;
- a dangling `dangling -> /nonexistent`, a plain file `plainfile`, and a system Python.

`pyenv versions`:

```text
* system (set by /home/tester/.pyenv/version)
  .venv
  2.7.18
  3.9.1
  3.10.0
  3.12 --> 3.12.10
  3.12-dev
  3.12.0rc1
  3.12.9
  3.12.9/envs/Beta
  3.12.9/envs/alpha
  3.12.9/envs/linkenv --> ../../3.10.0
  3.12.9/envs/zeta
  3.12.10
  alpha --> 3.12.9/envs/alpha
  ext --> /home/tester/external
  miniconda3-latest
  pypy3.10-7.3.17
  with space
```

`pyenv versions --skip-aliases`:

```text
* system (set by /home/tester/.pyenv/version)
  .venv
  2.7.18
  3.9.1
  3.10.0
  3.12-dev
  3.12.0rc1
  3.12.9
  3.12.9/envs/Beta
  3.12.9/envs/alpha
  3.12.9/envs/linkenv --> ../../3.10.0
  3.12.9/envs/zeta
  3.12.10
  ext --> /home/tester/external
  miniconda3-latest
  pypy3.10-7.3.17
  with space
```

`pyenv versions --skip-envs`:

```text
* system (set by /home/tester/.pyenv/version)
  .venv
  2.7.18
  3.9.1
  3.10.0
  3.12 --> 3.12.10
  3.12-dev
  3.12.0rc1
  3.12.9
  3.12.10
  alpha --> 3.12.9/envs/alpha
  ext --> /home/tester/external
  miniconda3-latest
  pypy3.10-7.3.17
  with space
```

`pyenv versions --bare --skip-aliases --skip-envs`:

```text
.venv
2.7.18
3.9.1
3.10.0
3.12-dev
3.12.0rc1
3.12.9
3.12.10
ext
miniconda3-latest
pypy3.10-7.3.17
with space
```

**Markers** [probe].
- **Alias named by the version.** `PYENV_VERSION=3.12`, with a `3.12` alias present, marks the alias line, because
  `3.12` exists as a directory and is not resolved:
  `* 3.12 --> 3.12.10 (set by PYENV_VERSION environment variable)`.
- **Prefix in a file.** A `.python-version` of `3.12` with no `3.12` entry marks the resolved `3.12.10` line:
  `* 3.12.10 (set by /home/tester/proj/.python-version)`.
- **Several markers.** `PYENV_VERSION=3.12.9/envs/alpha:alpha` marks two lines:

```text
  system
  .venv
  2.7.18
  3.9.1
  3.10.0
  3.12 --> 3.12.10
  3.12-dev
  3.12.0rc1
  3.12.9
  3.12.9/envs/Beta
* 3.12.9/envs/alpha (set by PYENV_VERSION environment variable)
  3.12.9/envs/linkenv --> ../../3.10.0
  3.12.9/envs/zeta
  3.12.10
* alpha --> 3.12.9/envs/alpha (set by PYENV_VERSION environment variable)
  ext --> /home/tester/external
  miniconda3-latest
  pypy3.10-7.3.17
  with space
```

- **Missing version.** `PYENV_VERSION=9.9` produces no `*` line. The stderr message
  ``pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)`` appears, and the exit code is
  **0**.

**Sort order.**
- **Versions.** `sort --version-sort` (GNU `filevercmp`) gave the order shown above. For example, `3.12` came before
  `3.12-dev`, before `3.12.0rc1`, before `3.12.9`, before `3.12.10`, and `.venv` came first. A direct probe gave the
  same order under `LC_ALL=C` and `LANG=en_US.UTF-8` [probe].
- **Envs.** Env order is bash glob order and therefore **locale-dependent**. [probe]: C.UTF-8 gave
  `Beta alpha linkenv zeta`, and `LANG=en_US.UTF-8` gave `alpha Beta linkenv zeta`.

**Aliases and root layout.**
- **Envs under an alias.** Envs appear under symlink aliases too [probe]. With `3.12 -> 3.12.10` and
  `3.12.10/envs/foo`, `--bare` listed `3.12`, `3.12/envs/foo`, `3.12.10` and `3.12.10/envs/foo`.
- **Symlinked `versions` directory.** If `$PYENV_ROOT/versions` is itself a symlink, `versions_dir` resolves elsewhere
  and the prefix strip fails. Envs are then printed as **absolute paths** [probe]. With
  `versions -> /tmp/data/versions`, `--bare` printed `/tmp/data/versions/3.12/envs/foo`.
- **Symlinked `PYENV_ROOT`.** A `PYENV_ROOT` that is itself a symlink printed normal names with the fallback realpath
  [probe]. With the native realpath extension the result is **UNCONFIRMED**.

**Environment.** Reads `PYENV_ROOT` and `PYENV_NATIVE_EXT`, plus the variables its helpers read. Exports nothing.

**Help block** [src libexec/pyenv-versions:1-13]:

```text
#!/usr/bin/env bash
# Summary: List all Python versions available to pyenv
# Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]
#
# Lists all Python versions found in `$PYENV_ROOT/versions/*'.
#
#   --bare            List just the names, omit `system'
#   --skip-aliases    Skip symlinks to other versions and to virtual environments
#   --skip-envs       Skip virtual environments (under <version>/envs)
#   --executables     Internal. Overrides other options.
#                     Optimally get a deduplicated list of all executable names in Pyenv-managed
#                     versions and environments for `pyenv rehash'
#
```

Rendered `pyenv help versions` [probe]:

```text
Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]

Lists all Python versions found in `$PYENV_ROOT/versions/*'.

  --bare            List just the names, omit `system'
  --skip-aliases    Skip symlinks to other versions and to virtual environments
  --skip-envs       Skip virtual environments (under <version>/envs)
  --executables     Internal. Overrides other options.
                    Optimally get a deduplicated list of all executable names in Pyenv-managed
                    versions and environments for `pyenv rehash'

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv which` — `libexec/pyenv-which`

**Synopsis.** `pyenv which <command> [--nosystem] [--skip-advice]` [src libexec/pyenv-which:5].

**Flags.**
- `--complete` as the first argument runs `exec pyenv-shims --short` [src libexec/pyenv-which:18-20].
- The command name is **always `$1`**, even when `$1` is a flag [src libexec/pyenv-which:24]. [probe]:
  `pyenv which --nosystem ls` printed `pyenv: --nosystem: command not found` and exited 127.
- All arguments, including `$1`, are then scanned [src libexec/pyenv-which:26-41]. `--skip-advice` and `--nosystem`
  are recognized anywhere. Other words are ignored.
- **`--nosystem`** leaves out the implicit `system` fallback.
- **`--skip-advice`** suppresses only the advice block. The "not installed" and "command not found" lines are still
  printed [test test/which.bats:195-206].

**Missing command.** With no command, the usage goes to stderr and the exit code is 1 [src libexec/pyenv-which:60-63]:

```text
Usage: pyenv which <command> [--nosystem] [--skip-advice]
```

**Version list** [src libexec/pyenv-which:65-71]. The list is `PYENV_VERSION` split on `:` if that is non-empty. The
entries are used raw here: no `python-` removal, and prefixes are resolved later by `pyenv-prefix`. If `PYENV_VERSION`
is empty, the list is the output of `pyenv-version-name -f`, which is normalized and prefix-resolved and keeps missing
entries verbatim.

**The `python-` form** [probe]. The same `python-` spelling behaves differently depending on where it comes from:
- `PYENV_VERSION=python-3.12.1 pyenv which python`, with `3.12.1` installed, **fails**. It prints
  ``pyenv: version `python-3.12.1' is not installed (set by PYENV_VERSION environment variable)``, then the not-found
  block, and exits 127.
- `python-3.12.1` in `.python-version` is normalized by `version-name`, so the same lookup succeeds.
- `PYENV_VERSION=python-3.12.1 pyenv exec python` also succeeds. `pyenv exec` normalizes the value first and exports
  `PYENV_VERSION=3.12.1`.

**Search order** [src libexec/pyenv-which:75-93]. Each listed version is tried in order. Then `system` is tried,
unless `--nosystem` was given. The first candidate that passes `-x` wins.
- **Non-system entry.** `version_path=$(pyenv-prefix "$version" 2>/dev/null)` must succeed; otherwise the entry is
  recorded as nonexistent and skipped. The candidate is `$version_path/bin/<command>`. `$version` is then set to the
  basename of `version_path`, for use by hooks [test test/which.bats:176-193].
- **`system` entry** [src libexec/pyenv-which:44-58, 76-80].
  - A search path is built from `PATH`, with `~` replaced by `$HOME`.
  - Every exact occurrence of `${PYENV_ROOT}/shims` is removed from it, together with every directory listed in
    `_PYENV_SHIM_PATHS_<PROGRAM>`.
  - `<PROGRAM>` is the command name uppercased, with `-` turned into `_` and any other character outside
    `[A-Z0-9_]` also turned into `_` [test test/which.bats:208-232].
  - The candidate is `command -v <command>`, run with that search path.
  - An explicit `system` inside the version list is tried at its own position, and again at the end.
- **Hooks.** The `which` hooks are sourced after the search, with `$version`, `$PYENV_COMMAND` and
  `$PYENV_COMMAND_PATH` visible [src libexec/pyenv-which:95-100].

**Found.** The command prints `<path>\n` on stdout and exits 0 [src libexec/pyenv-which:102-103].

**Not found** [src libexec/pyenv-which:104-126]. Everything goes to **stderr**, and the exit code is **127**:

```text
pyenv: version `<v>' is not installed (set by <origin>)
pyenv: <command>: command not found

The `<command>' command exists in these Python versions:
  <version>

Note: See 'pyenv help global' for tips on allowing multiple
      Python versions to be found at the same time.
```

- **"is not installed" lines.** One line per nonexistent version, in order. They are printed **only** when the command
  was not found anywhere. If a later entry or `system` has the command, missing versions are silent [probe].
- **Advice block.** The part from the empty line through the second `Note` line is printed only without
  `--skip-advice`, and only if `pyenv-whence <command>` is non-empty.
- **Version lines.** Each line of the whence output gets a two-space indent (`sed 's/^/  /g'`). Envs and aliases
  appear there too, in `pyenv versions` order [probe].
- **`Note` lines.** The second `Note` line starts with 6 spaces. The first uses straight quotes.

Tests: [test test/which.bats:60-135].

[probe] with `PYENV_VERSION=9.9:8.8 pyenv which tool`, where `tool` exists in `3.11.9` and in the env
`3.12.1/envs/venv1`:

```text
pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)
pyenv: version `8.8' is not installed (set by PYENV_VERSION environment variable)
pyenv: tool: command not found

The `tool' command exists in these Python versions:
  3.11.9
  3.12.1/envs/venv1

Note: See 'pyenv help global' for tips on allowing multiple
      Python versions to be found at the same time.
```

With a missing version from a file, the origin is the file path [probe]:
``pyenv: version `9.9' is not installed (set by /home/tester/proj/.python-version)``.

**Exit codes.** 0 when found. 1 for the usage error. 127 when not found.

**Quirks** [probe].
- **Directories count as found.** Only `-x` is tested. A non-executable regular file is not found, but a
  **directory** named like the command in a version's `bin` counts as found, and its path is printed.
- **Shell builtins.** For `system`, bash `command -v` returns the bare name for builtins. `pyenv which echo` under
  `system` therefore fails the `-x` test and prints `pyenv: echo: command not found` with exit 127, even though
  `/usr/bin/echo` exists.
- **Relative `PATH` entries.** A relative `PATH` entry yields a relative result. `relbin` in `PATH` gave
  `relbin/relprog`.
- **Paths use `PYENV_ROOT` as given.** The printed path is built from `PYENV_ROOT` as given and is not resolved. A
  symlinked root gave `/home/tester/link-root/versions/3.12.10/bin/python`.

**Environment.**
- **Read:** `PYENV_VERSION`, `PYENV_ROOT`, `PATH`, `HOME`, `_PYENV_SHIM_PATHS_<PROGRAM>`, and, through
  `version-name`, `PYENV_DIR`.
- **Exported:** nothing.

**Help block** [src libexec/pyenv-which:1-12]. Line 9 ends with a trailing space:

```text
#!/usr/bin/env bash
#
# Summary: Display the full path to an executable
#
# Usage: pyenv which <command> [--nosystem] [--skip-advice]
#
# Displays the full path to the executable that pyenv will invoke when
# you run the given command.
# Use --nosystem argument in case when you don't need to search command in the 
# system environment.
# Internal switch --skip-advice used to skip printing an error message on a
# failed search.
```

Rendered `pyenv help which` [probe]. The line ending `...search command in the ` keeps its trailing space.

```text
Usage: pyenv which <command> [--nosystem] [--skip-advice]

Displays the full path to the executable that pyenv will invoke when
you run the given command.
Use --nosystem argument in case when you don't need to search command in the 
system environment.
Internal switch --skip-advice used to skip printing an error message on a
failed search.

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv whence` — `libexec/pyenv-whence`

**Synopsis.** `pyenv whence [--path] <command>` [src libexec/pyenv-whence:3].

**Flags.**
- `--complete` as the first argument prints `--path`, then `pyenv-shims --short` [src libexec/pyenv-whence:9-12].
- `--path` is recognized only as the first argument [src libexec/pyenv-whence:14-19].

**Missing command.** With no command, including a bare `pyenv whence --path`, the usage goes to stderr and the exit
code is 1 [src libexec/pyenv-whence:31-35] [probe]:

```text
Usage: pyenv whence [--path] <command>
```

**Algorithm** [src libexec/pyenv-whence:21-29].
- The command iterates over `pyenv-versions --bare`: versions, envs and aliases, in `versions` order.
- For each entry, the candidate is `"$(pyenv-prefix "$version")/bin/<command>"`.
- If the candidate passes `-x`, the command prints the version name, or with `--path` the full candidate path.

**Output and exit code** [src libexec/pyenv-whence:37-38]. The result is printed with one entry per line, and the exit
code is 0. With **no matches** nothing is printed and the exit code is **1**. The reason is that the last command,
`[ -n "$result" ] && echo`, fails [probe].

[test test/whence.bats:5-23] [probe]:

```text
$ pyenv whence python
3.11.9
3.12.1
$ pyenv whence --path python
/home/tester/.pyenv/versions/3.11.9/bin/python
/home/tester/.pyenv/versions/3.12.1/bin/python
$ pyenv whence tool
3.11.9
3.12.1/envs/venv1
```

**Aliases** [probe]. An alias is listed separately from its target. With `3.12 -> 3.12.10`, `pyenv whence python`
printed `3.12` and then `3.12.10`.

**Help block** [src libexec/pyenv-whence:1-3]:

```text
#!/usr/bin/env bash
# Summary: List all Python versions that contain the given executable
# Usage: pyenv whence [--path] <command>
```

Rendered `pyenv help whence` [probe]:

```text
Usage: pyenv whence [--path] <command>

List all Python versions that contain the given executable

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv exec` — `libexec/pyenv-exec`

**Synopsis.** `pyenv exec <command> [arg1 arg2...]` [src libexec/pyenv-exec:5]. The only flag is `--complete` as the
first argument, which runs `exec pyenv-shims --short` [src libexec/pyenv-exec:20-22].

**Steps, in execution order** [src] [probe]:
1. **Resolve the version.** `PYENV_VERSION="$(pyenv-version-name -f)"` [src libexec/pyenv-exec:24].
   - This runs first, before the usage check. With `-f`, missing versions are kept verbatim, so this step itself
     prints no "not installed" messages.
   - The `version-name` hooks run here.
   - `invalid version` messages from reading the file can appear on stderr.
2. **Usage check.** An empty command sends the usage to stderr and exits 1 [src libexec/pyenv-exec:27-30]:

   ```text
   Usage: pyenv exec <command> [arg1 arg2...]
   ```

3. **Shim path.** If `_PYENV_SHIM_PATH` is non-empty, it is prepended to `_PYENV_SHIM_PATHS_<PROGRAM>`, with `:` as
   the separator when that variable was already set, and the result is exported. `_PYENV_SHIM_PATH` is then unset
   [src libexec/pyenv-exec:32-38] [test test/exec.bats:120-141]. `<PROGRAM>` is derived as in `pyenv which`.
4. **Find the command.** `PYENV_COMMAND_PATH="$(pyenv-which "$PYENV_COMMAND")"` runs next, and `PYENV_BIN_PATH` is set
   to its directory part [src libexec/pyenv-exec:40-41].
   - **Not found.** `pyenv-which` has already printed its full stderr output. The failed assignment ends `pyenv exec`
     under `set -e` with exit **127** [probe] [test test/exec.bats:5-25].
   - **Which `PYENV_VERSION` `pyenv-which` sees.** If `PYENV_VERSION` came from the caller's environment, it is
     already exported, so `pyenv-which` sees the resolved value. Otherwise it is not exported yet, so `pyenv-which`
     runs `pyenv-version-name -f` again. [probe]: the `version-name` hooks ran twice in that case. The
     `(set by ...)` origin is then the file [test test/exec.bats:15-25].
5. **Export.** `export PYENV_VERSION` runs **unconditionally** [src libexec/pyenv-exec:43]. The child therefore always
   receives `PYENV_VERSION` set to the `version-name -f` output.
   - [probe]: with nothing configured, the child saw `PYENV_VERSION=system`.
   - [probe]: from `PYENV_VERSION=3.12` or a file containing `3.12`, the child saw `3.12.10`.
   - [probe]: `3.12.1:system` stayed `3.12.1:system`.
6. **Exec hooks.** The `exec` hooks are sourced [src libexec/pyenv-exec:45-50]. At that point `"$@"` **still includes
   the command name**, `PYENV_VERSION` is exported, `PYENV_COMMAND_PATH` and `PYENV_BIN_PATH` are set, and `PATH` is
   not yet modified [probe]. Hooks may replace `PYENV_COMMAND_PATH` and `PYENV_BIN_PATH`.
7. **Drop the command name.** `shift 1` [src libexec/pyenv-exec:52].
8. **`PATH` prepend.** The prepend happens **only if the string `PYENV_BIN_PATH` starts with the string
   `PYENV_ROOT`** [src libexec/pyenv-exec:53-56]. The test is `"${PYENV_BIN_PATH#${PYENV_ROOT}}" != "${PYENV_BIN_PATH}"`.
   - It is a plain string-prefix test, not a path-component test. The unquoted `${PYENV_ROOT}` in the pattern also
     means glob characters in it act as a pattern. These two points follow from the source and were not probed.
   - When the test passes, the command runs `export PATH="${PYENV_BIN_PATH}:${PATH}"`.
   - The `PATH` extended here is the **dispatcher's** `PATH`, which already begins with `<prefix>/libexec` and the
     plugin `bin` dirs. [probe] child `PATH`:
     `/home/tester/.pyenv/versions/3.12.1/bin:/opt/pyenv/libexec:/opt/pyenv/plugins/python-build/bin:<original PATH>`.
   - **System commands.** A command found by `system` normally lives outside `PYENV_ROOT`. In that case `PATH` is left
     as the dispatcher's `PATH`, with no version bin added [test test/exec.bats:92-118] [probe].
   - **Multi-version values.** With `PYENV_VERSION=system:custom`, a command found in system leaves `PATH` unchanged.
     With `custom:system`, a command found in `custom` gets `custom`'s bin prepended [test test/exec.bats:111-117].
9. **Run.** `exec "$PYENV_COMMAND_PATH" "$@"` [src libexec/pyenv-exec:57].
   - **argv[0]** is `PYENV_COMMAND_PATH` exactly as `pyenv-which` printed it. That is normally the absolute path
     `<PYENV_ROOT>/versions/<v>/bin/<command>` [test test/exec.bats:54-75], or for `system` the absolute `PATH` hit.
   - The name the user typed, for example the shim's basename, is not preserved as argv[0].
   - All remaining arguments are passed through verbatim, including `--` [test test/exec.bats:54-75].
   - **Relative `PATH` entry.** When `system` resolves through a relative `PATH` entry, `pyenv-which` returns a
     relative path. [probe]: for a bash-script child, `$0` was nevertheless absolute. argv[0] for a binary child in
     that case is **UNCONFIRMED**.

**Built-in exec hook `pip-rehash`** [src pyenv.d/exec/pip-rehash.bash:1-16]. This hook is shipped in
`<prefix>/pyenv.d/exec` and is active by default through `PYENV_HOOK_PATH`.
- **When it applies.** The command basename is reduced first: `pip2`, `pip3`, `pip3.12`, `easy_install3` and similar
  names become `pip` or `easy_install`. The hook then applies if the reduced name is `pip`, `easy_install` or `conda`.
  It also applies if the joined arguments `"$*"` contain `<space>-m<space>pip<space>`. `"$*"` includes the command
  name, because the hook runs before `shift`, so `python -m pip install x` matches.
- **What it does.** `PYENV_COMMAND_PATH` becomes the wrapper `<prefix>/pyenv.d/exec/pip-rehash/<pip|easy_install|conda>`
  and `PYENV_BIN_PATH` becomes the wrapper's directory. The hook also exports
  `PYENV_REHASH_REAL_COMMAND=<original basename>`.
- **The wrapper** [src pyenv.d/exec/pip-rehash/pip:6-31]:
  1. It removes its own directory from `PATH` and runs `pyenv-which "$PYENV_REHASH_REAL_COMMAND"`.
  2. It prepends that command's directory to `PATH`.
  3. It runs the real command as a **child**, not with `exec`.
  4. If the child succeeds, it runs `pyenv-rehash`. For `pip` this happens only when an argument equals `install` or
     `uninstall`. For `easy_install` it always happens [src pyenv.d/exec/pip-rehash/easy_install:22-25]. For `conda`
     it happens when the first argument is `install`, `remove` or `uninstall`
     [src pyenv.d/exec/pip-rehash/conda:22-27].
  5. It exits with the child's status.
- [probe]: `pyenv exec pip install foo` ran the real pip with `$0=/home/tester/.pyenv/versions/3.12.1/bin/pip` and
  that bin first in `PATH`. Tests: [test test/pip-rehash.bats:10-60].

**Environment.**
- **Read:** `PYENV_VERSION`, `_PYENV_SHIM_PATH`, `_PYENV_SHIM_PATHS_<PROGRAM>`, `PYENV_ROOT`, `PATH`, and, through its
  helpers, `PYENV_DIR` and `PYENV_HOOK_PATH`.
- **Exported to the child:**
  - `PYENV_VERSION`, always.
  - `PATH`: the dispatcher's `PATH`, plus the bin prepend under the condition in step 8.
  - `_PYENV_SHIM_PATHS_<PROGRAM>`, when `_PYENV_SHIM_PATH` was set. `_PYENV_SHIM_PATH` itself is removed.
  - Everything the dispatcher exported: `PYENV_ROOT`, `PYENV_DIR`, `_PYENV_INSTALL_PREFIX` and `PYENV_HOOK_PATH`.
  - `PYENV_REHASH_REAL_COMMAND`, when the pip-rehash hook applies.

**Help block** [src libexec/pyenv-exec:1-14]:

```text
#!/usr/bin/env bash
#
# Summary: Run an executable with the selected Python version
#
# Usage: pyenv exec <command> [arg1 arg2...]
#
# Runs an executable by first preparing PATH so that the selected Python
# version's `bin' directory is at the front.
#
# For example, if the currently selected Python version is 2.7.6:
#   pyenv exec pip install -r requirements.txt
#
# is equivalent to:
#   PATH="$PYENV_ROOT/versions/2.7.6/bin:$PATH" pip install -r requirements.txt
```

Rendered `pyenv help exec` [probe]. `--usage` prints `Usage: pyenv exec <command> [arg1 arg2...]`.

```text
Usage: pyenv exec <command> [arg1 arg2...]

Runs an executable by first preparing PATH so that the selected Python
version's `bin' directory is at the front.

For example, if the currently selected Python version is 2.7.6:
  pyenv exec pip install -r requirements.txt

is equivalent to:
  PATH="$PYENV_ROOT/versions/2.7.6/bin:$PATH" pip install -r requirements.txt

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv rehash` — `libexec/pyenv-rehash`

**Synopsis.** `pyenv rehash`. There are no flags, arguments are ignored, and there is no `--complete`.

**Directories and lock.**
- `SHIM_PATH` is `${PYENV_ROOT}/shims`. The prototype is `${SHIM_PATH}/.pyenv-shim`, which is **also the lock file**
  [src libexec/pyenv-rehash:7-8]. The shims directory is created with `mkdir -p` [src libexec/pyenv-rehash:11].
- **Writability.** If the directory is not writable (`-w`), the message goes to stderr and the exit code is 1
  [src libexec/pyenv-rehash:54-57] [test test/rehash.bats:13-20] [probe]:

  ```text
  pyenv: cannot rehash: <PYENV_ROOT>/shims isn't writable
  ```

- **Lock acquisition** [src libexec/pyenv-rehash:15-43, 59-80]:
  - A lock file older than 2 minutes is deleted first (`find ... -mmin +2 -exec rm -f {} \;`)
    [test test/rehash.bats:44-53].
  - The lock is created with `set -o noclobber` and `echo -n > .pyenv-shim`.
  - After the first failure, writability is checked again with `mktemp` inside the shims dir. If that check fails,
    the command prints the same `isn't writable` message and exits 1.
  - Otherwise the command retries every 0.1 s, or every 1 s if `sleep 0.1` fails, while
    `SECONDS <= start + PYENV_REHASH_TIMEOUT`. The default timeout is 60 seconds.
- **Timeout.** On timeout the command prints two lines to stderr and exits 1 [src libexec/pyenv-rehash:72-78]
  [test test/rehash.bats:22-33] [probe]:

  ```text
  pyenv: cannot rehash: couldn't acquire lock <PYENV_ROOT>/shims/.pyenv-shim for <PYENV_REHASH_TIMEOUT> seconds. Last error message:
  <prefix>/libexec/pyenv-rehash: line 22: <PYENV_ROOT>/shims/.pyenv-shim: cannot overwrite existing file
  ```

  - The second line is bash's own noclobber error text. It includes the script path and line number, and its wording
    is locale-dependent; the test forces `LANG=C` and matches with a glob.
  - `PYENV_REHASH_TIMEOUT=0` still makes one attempt and prints the same message with `for 0 seconds` [probe].
  - When the timeout expires before any failed attempt set the writability flag, the command exits 1 silently
    [src libexec/pyenv-rehash:72-79]. That happens, for example, with a negative timeout, where the loop never runs.
    This follows from the source and was not probed.
- **Release.** The lock is released by an `EXIT` trap, including on failure [src libexec/pyenv-rehash:23, 49-52].
  [probe]: the lock was absent after a failed rehash.

**Prototype shim.** Here is the exact content written [src libexec/pyenv-rehash:86-99] [probe]. It has 12 lines, each
ending in `\n`.

```text
#!/usr/bin/env bash
set -e
[ -n "$PYENV_DEBUG" ] && set -x

program="${0##*/}"

export PYENV_ROOT="/home/tester/.pyenv"
SHIM_PATH=${0%/*}
if [[ $SHIM_PATH != "/home/tester/.pyenv/shims" ]]; then
  export _PYENV_SHIM_PATH="$SHIM_PATH"
fi
exec "/opt/pyenv/libexec/pyenv" exec "$program" "$@"
```

- **Substituted values.** Two values are fixed at rehash time. `/home/tester/.pyenv` is `PYENV_ROOT`, inserted
  without escaping. `/opt/pyenv/libexec/pyenv` is `$(command -v pyenv)`. Through the dispatcher, `<prefix>/libexec` is
  first on `PATH`, so this is `<prefix>/libexec/pyenv`, not `bin/pyenv`.
- **Mode.** The prototype gets `chmod +x`, and shims are copies made with `cp`. [probe] mode: `755`.
- **Behavior of a shim.** It exports `PYENV_ROOT`. If its own directory is not `<PYENV_ROOT>/shims`, for example when
  it is reached through a symlink elsewhere, it exports `_PYENV_SHIM_PATH=<that dir>`. It then runs
  `exec <pyenv> exec <basename of $0> "$@"` [test test/rehash.bats:333-356].

**Which names become shims** [src libexec/pyenv-rehash:208] [src libexec/pyenv-versions:42-51].
- The names come from `make_shims $(pyenv-versions --executables)`. That lists the basenames of **every** entry in
  `$PYENV_ROOT/versions/*/bin/*` and `$PYENV_ROOT/versions/*/envs/*/bin/*`, with dotglob and nullglob, deduplicated
  with `sort -u`.
- **No executable test.** The entries are not filtered by type or mode. [probe]: a non-executable file, a
  subdirectory, a dangling symlink, a dotfile (`.dotfile`), and a tool in a hidden version dir (`.hidden`) all became
  shims.
- **Depth.** Only those two glob levels are scanned. [probe]: `lib/.../bin/deep` did not become a shim.
- **Aliases.** Symlinked version dirs are followed by the glob.
- **Word splitting.** `$(...)` is unquoted, so a name containing whitespace is split into several shims. [probe]: a
  file `has space` produced shims `has` and `space`.
- **Glob characters.** With nullglob active, a name containing glob characters would be expanded against the current
  directory, or dropped if nothing matched. This follows from the source and was not probed.

**Installing and removing shims.**
- **Existing shims** [src libexec/pyenv-rehash:109-126]. For each registered name, an existing regular file or
  dangling symlink whose content differs from the prototype is deleted and re-copied. That covers truncation, extra
  bytes, an embedded NUL, and an unreadable file. Identical shims are left untouched, so their mtime is kept.
  Directories are left alone [test test/rehash.bats:91-203].
- **Rehash hooks.** The `rehash` hooks are sourced after `make_shims` [src libexec/pyenv-rehash:212-218]. They can call
  `make_shims` and `register_shim`, and they see `SHIM_PATH`.
- **Final passes.** `install_registered_shims` runs, then `remove_stale_shims` [src libexec/pyenv-rehash:220-221].
  Every **non-hidden** entry of `$SHIM_PATH/*` that was not registered is removed with `rm -f`. Hidden files are never
  removed, whether that is `.keep` or the lock [test test/rehash.bats:174-189].
- **Unregistered directory.** If `shims/` contains an unregistered **directory**, [probe] printed
  `rm: cannot remove '<PYENV_ROOT>/shims/mdir': Is a directory` on stderr, and the rehash exited **1** under
  `set -e`. Stale entries that sort after the directory were not removed. The lock was still released.
- **Success.** Stdout is empty and the exit code is 0 [test test/rehash.bats:5-11,55-75] [probe].

**Built-in rehash hooks.** These are shipped in `<prefix>/pyenv.d/rehash` and are active by default.
- **`source.bash`** [src pyenv.d/rehash/source.bash:1-31]. After registration, every existing shim whose basename is
  listed in `source.d/*.list` is overwritten with a "source shim". The default list is `activate`, `activate.csh`,
  `activate.fish`, `activate.nu` and `gettext.sh` [src pyenv.d/rehash/source.d/default.list:1-7]. The source shim has
  no shebang. Its exact content [probe]:

```text
[ -n "$PYENV_DEBUG" ] && set -x
export PYENV_ROOT="/home/tester/.pyenv"
program="$("/opt/pyenv/libexec/pyenv" which "${BASH_SOURCE##*/}")"
if [ -e "${program}" ]; then
  . "${program}" "$@"
fi
```

  [test test/rehash.bats:205-223] confirms that these shims survive a second rehash unchanged.
- **`conda.bash`** [src pyenv.d/rehash/conda.bash:7-72]. If any `versions/*/bin/conda` or `versions/*/envs/*/bin/conda`
  exists, the hook deregisters every name listed in `conda.d/default.list`. The list covers coreutils names, `curl`,
  `sed`, `openssl`, `sqlite3`, `xz*`, Qt tools and others [src pyenv.d/rehash/conda.d/default.list:1-195]. Those
  shims are then removed as stale. [probe]: with `conda`, `curl` and `xz` in a version, only the `conda` and `python`
  shims remained.
- **Recursion case.** Shims for listed names are created by `make_shims`, which runs before this hook. [probe]: a
  version that also shipped `sed`, with the shims dir on `PATH`, produced `bash: warning: shell level (1000) too high`
  and `Argument list too long` errors during rehash.
  - Likely mechanism, not traced: a `sed` call resolves to the new `shims/sed` shim, which calls back into pyenv.
    `sed` is called at [src pyenv.d/rehash/conda.bash:21] and [src libexec/pyenv-which:77].

**Environment.**
- **Read:** `PYENV_ROOT`, `PYENV_REHASH_TIMEOUT`, `PYENV_DEBUG`, `PATH`, and `PYENV_HOOK_PATH` through `pyenv-hooks`.
- **Exported:** nothing.

**Help block** [src libexec/pyenv-rehash:1-2]:

```text
#!/usr/bin/env bash
# Summary: Rehash pyenv shims (run this after installing executables)
```

Rendered `pyenv help rehash` [probe]. `--usage` prints nothing.

```text
Usage: pyenv rehash

Rehash pyenv shims (run this after installing executables)

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## `pyenv shims` — `libexec/pyenv-shims`

**Synopsis.** `pyenv shims [--short]` [src libexec/pyenv-shims:3].

**Flags.**
- `--complete` prints `--short` [src libexec/pyenv-shims:9-12].
- `--short` is recognized as the first argument only. Other arguments are ignored. [probe]: `pyenv shims --foo` gave
  the full listing.

**Behavior** [src libexec/pyenv-shims:14-22].
- For each `$PYENV_ROOT/shims/*` (nullglob, **no dotfiles**), the command prints the full path, or the basename with
  `--short`. The output is piped through `sort`, which is locale-collated.
- A missing or empty shims dir gives empty output and exit 0 [test test/shims.bats:5-9].
- The exit code is always 0.

**Help block** [src libexec/pyenv-shims:1-3]:

```text
#!/usr/bin/env bash
# Summary: List existing pyenv shims
# Usage: pyenv shims [--short]
```

Rendered `pyenv help shims` [probe]:

```text
Usage: pyenv shims [--short]

List existing pyenv shims

```

_(The output ends with an empty line: its last two bytes are `\n\n`.)_

---

## Hooks — `libexec/pyenv-hooks` and hook call sites

- **`pyenv hooks <command>`.**
  - For each directory in `PYENV_HOOK_PATH`, split on `:` and taken in order, the command prints every
    `<dir>/<command>/*.bash`, in glob order within the directory. Each path is passed through `realpath`, so symlinks
    are resolved [src libexec/pyenv-hooks:55-62] [test test/hooks.bats:10-70].
  - With no command, the usage goes to stderr and the exit code is 1 [test test/hooks.bats:5-8]:

    ```text
    Usage: pyenv hooks <command>
    ```

  - `--complete` prints `exec`, `rehash`, `version-name`, `version-origin` and `which`
    [src libexec/pyenv-hooks:9-16].
- **Call sites.** Hooks are `source`d in the calling script's shell, with the caller's original `IFS`, so they can
  read and change the caller's variables. The call sites are:
  - `exec` [src libexec/pyenv-exec:45-50]
  - `rehash` [src libexec/pyenv-rehash:212-218]
  - `version-name` [src libexec/pyenv-version-name:28-33]
  - `version-origin` [src libexec/pyenv-version-origin:8-13]
  - `which` [src libexec/pyenv-which:95-100]

  Tests `carries original IFS within hooks` in `exec.bats`, `rehash.bats`, `version-name.bats`, `version-origin.bats`
  and `which.bats` cover the `IFS` behavior.
- **Order within `pyenv exec`** [probe]. The `version-name` hooks run once, or twice when `PYENV_VERSION` was not in the
  environment. Then the `which` hooks run, then the `exec` hooks.
- **Built-in hooks.** The shipped hooks live in `<prefix>/pyenv.d`, which is on `PYENV_HOOK_PATH` by default
  [src libexec/pyenv:96-100]. They are `exec/pip-rehash.bash`, `rehash/conda.bash`, `rehash/source.bash`, and
  `install/latest.bash`, which is used only by `pyenv install`.

---

## Cross-cutting edge-case index

| Topic | Behavior | Section |
|---|---|---|
| Empty version file | `version-file-read` exits 1 with no output. `version-name` gives `system`. The empty local file still shadows the global one. `pyenv local` exits 1 silently. `pyenv global` prints `system`. | version-file, version-file-read, version-name, local, global |
| Comment lines | A line is skipped when its first word starts with `#`, leading whitespace allowed. A `#` inside a word is kept: `3.12#foo` reads as `3.12#foo` [probe]. A comment-only local file still shadows the global file. | version-file-read, version |
| CRLF | `\r` is added to `IFS`, so CRs vanish. A CR-only line counts as blank. | version-file-read |
| Several versions | File: one word per line, joined with `:`. `PYENV_VERSION`: split on `:`. Output joins with `:` (`version-name`, `prefix`) or prints one per line (`version`, `local`, `global`). Writing puts one per line. | version-name, version, local, global, prefix |
| Empty `:` fields | A leading or middle empty field is accepted by `version-name`. A trailing one is dropped. | version-name |
| `system` | `version-name` never checks it. `prefix`, `local` and `global` require a system Python in a `bin` or `sbin` dir. `versions` shows the line only if `pyenv which python`, `python3` or `python2` works. `which` and `exec` search `PATH` minus shims. | several |
| `..` or `/` | Filtered only when read from a version file. `PYENV_VERSION` and `pyenv local ..` are not filtered. | version-file-read, version-name, local |
| Prefix matching | `pyenv-latest`: the prefix ends at a `-` or `.` boundary, prereleases and dev builds are excluded, fields 2–4 sort numerically, and ties are locale-collated. It is used by `version-name` (`-b`) and `prefix` (`-f`), not by file reading. | latest |
| `python-` prefix | Stripped only in `version-name`. `local` rejects it [probe]. `prefix` and `global` go through the same `pyenv-prefix` check [src libexec/pyenv-prefix:48-56]. `which` rejects it in `PYENV_VERSION` but accepts it in a file [probe]. `exec` accepts both [probe]. | version-name, local, which |
| `versions` order | `system` first, then `sort --version-sort` of names. Envs follow their parent in glob order, which is locale-dependent. | versions |

## Locale and platform dependencies

| Where | Mechanism | Observed effect |
|---|---|---|
| `pyenv commands`, `pyenv help` listing | `sort -u` [src libexec/pyenv-commands:48] [src libexec/pyenv-help:157] | `--version` is first under C/C.UTF-8. Under en_US.UTF-8 it falls between `uninstall` and `version` [probe]. |
| `pyenv versions` envs | bash glob order [src libexec/pyenv-versions:190] | C.UTF-8 gives `Beta alpha linkenv zeta`. en_US.UTF-8 gives `alpha Beta linkenv zeta` [probe]. |
| `pyenv versions` entries | `sort --version-sort` [src libexec/pyenv-versions:174] | Same in C and en_US.UTF-8 for the probed set [probe]. If `sort` lacks `-V`, glob order is used [test test/versions.bats:253-269]. |
| `pyenv latest` ties | `sort` last-resort comparison [src libexec/pyenv-latest:87] | `3.10.4-debug` versus `3.10.4` flips between C and en_US.UTF-8 [probe]. |
| `pyenv shims` | `sort` [src libexec/pyenv-shims:22] | Locale-collated. Not separately probed. |
| rehash lock error line | bash noclobber message [src libexec/pyenv-rehash:22] | Wording is localized. The test forces `LANG=C` [test test/rehash.bats:26-27]. |
| `pyenv --version` | git checkout detection [src libexec/pyenv---version:18-21] | In a clone with a `pyenv` remote the output is `pyenv <git describe>`. Otherwise it is `pyenv 2.8.6`. |
| realpath | optional `libexec/pyenv-realpath.dylib` builtin [src libexec/pyenv:23] | Without it, logical-`cd` fallbacks are used. Behavior with the dylib was not probed (**UNCONFIRMED**). |

## UNCONFIRMED items

- `pyenv --version` output when `HEAD` is exactly on a tag. git would print just the tag.
- The effect of the native `pyenv-realpath.dylib` extension on `pyenv versions` names, on `pyenv hooks` paths, and on
  whether `realpath.dylib` appears in `pyenv commands`.
- argv[0] for a **binary**, not a script, executed by `pyenv exec` through a relative `PATH` entry.
