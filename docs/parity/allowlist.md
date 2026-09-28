# Intentional differences from upstream

rpyenv matches the contract of pyenv on Linux and pyenv-win on Windows, but not their bugs (design spec §4, "Parity policy"). Every intentional difference is listed here with its reason. The differential tests (M1c) read this file.

| ID | OS | Command | Upstream | rpyenv | Reason |
|---|---|---|---|---|---|
| D-01 | both | `--version` | `pyenv 2.8.6` (Linux), `pyenv 3.1.1` (Windows) | `pyenv 2.8.6 (rpyenv 0.1.0)`, `pyenv 3.1.1 (rpyenv 0.1.0)` | Identifies rpyenv, and keeps the upstream version it matches parseable. |
| D-02 | Windows | `--version` | Warns when `PYENV`, `PYENV_ROOT` or `PYENV_HOME` is unset | No warnings | Advice specific to pyenv-win's installer; rpyenv doesn't need these variables. |
| D-03 | both | version files | A leading UTF-8 BOM becomes part of the first version name | The BOM is ignored | Files saved by some Windows editors would otherwise never match an installed version. |
| D-04 | Linux | version files | A word longer than 1024 characters is split in two (`read -n 1024`) | The word is kept whole | Mis-parsing artifact of bash `read`. |
| D-05 | Windows | prefix resolution | Two installed versions equal in major, minor and patch (`3.9`, `3.9.0`) raise a VBScript runtime error | The first in directory order wins | Crash. |
| D-06 | Linux | prefix resolution | Ties are broken by `sort`'s locale collation | Ties are broken by byte order (C locale) | Deterministic result. |
| D-07 | Linux | `versions` | Envs in bash glob order, which depends on the locale | Envs in byte order (C locale) | Deterministic output. |
| D-17 | Windows | `versions` and others | A missing `versions` folder is created as a side effect | Nothing is created | Read-only commands shouldn't change the disk. |
| D-08 | Windows | `help` | Drops the `)` after "executables" (batch parsing bug) | `)` is present | Help-text typo. |
| D-09 | Windows | `--help` | `pyenv --help` prints `no such command '--help'` and exits 1 | Prints the help listing, exit 0 | Defect in pyenv.bat's routing. |
| D-10 | Windows | all | `pyenv VERSION` prints nothing (routing is half case-insensitive) | Command names are case-insensitive | Defect. |
| D-12 | Windows | `root`, `prefix`, `version-file`, `version-origin`, `version-file-read`, `version-file-write` | `pyenv: no such command '<name>'` | Implemented. Messages, streams and quoting follow upstream pyenv; version selection, prefix resolution (`TryResolveVersion`), the empty-local-file fallback and version-file reading follow pyenv-win. `version-origin` prints `%PYENV_VERSION%`; `prefix` with nothing selected prints pyenv-win's no-version message on stdout. | Spec §4: commands pyenv-win lacks follow upstream pyenv's contract, but must agree with the pyenv-win commands beside them about which version is selected. |
| D-16 | Windows | `commands` | Lists pyenv-win's `libexec` files | Lists rpyenv's commands, in the same NTFS name order | Different command set. |
| D-19 | Linux | `--debug`, `PYENV_DEBUG` | bash `set -x` trace on stderr | No trace (rpyenv's own debug output comes later) | A bash trace has no equivalent in a binary. |
| D-20 | Windows | `pyenv` (no arguments) | First line is `pyenv ` plus the raw `.version` file | `pyenv 3.1.1 (rpyenv 0.1.0)`, then an empty line | pyenv-win's output depends on the checkout's line endings. |
| D-15 | Windows | `version` | The PATH warning looks for `shims\python.bat` | Looks for `shims\python.exe` | rpyenv shims are executables. |
| D-18 | Linux | `version-file <dir>` | A missing relative `<dir>` prints bash's `cd` error | Exit 1, no message | bash-specific text. |
| D-21 | Linux | `local`, `global`, `version-file-write` | An unwritable file prints bash's redirection error | `` pyenv: cannot write `<file>': <reason> `` | bash-specific text. |
| D-22 | Windows | `version` | With an extra argument (`pyenv version <x>`), the PATH check is skipped, because pyenv.bat routes on `%1%2` | The PATH check always runs; extra arguments are ignored | Batch-routing artifact, not behavior scripts rely on. |
| D-11 | Windows | `local --unset`, `global --unset` | A missing file prints a VBScript runtime error to stderr (exit 0) | Silent, exit 0 | Script error leak. |
| D-14 | Linux | `global -f`, `global --force` | Written into the version file as a version | Accepted as a force flag, as for `local` | File corruption. |
| D-13 | Windows | `prefix` | (pyenv-win lacks the command; upstream joins with `:`) | Joined with `;` | `:` is ambiguous with drive letters. |
| D-23 | Windows | `local --unset`, `global --unset` | Any removal failure prints a VBScript runtime error to stderr (exit 0) | `pyenv: cannot remove '<file>': <reason>` on stdout, exit 1 | Script error leak; a failed unset must not report success. |
| D-24 | Linux | `local --unset` | `rm -f` error text from bash | `` pyenv: cannot remove `<file>': <reason> ``, exit 1 | bash-specific text (exit code matches). |
| D-25 | Windows | `version`, `version-origin` | A `.python-version` at a drive root shows as `Q:\\.python-version` (doubled backslash) | `Q:\.python-version` | Path-joining artifact. |
| D-26 | Windows | `local`, `global` | An unwritable file raises a VBScript runtime error on stderr (exit 0) | `pyenv: cannot write '<file>': <reason>` on stdout, exit 1 | Script error leak. |
| D-27 | Linux | `versions` | When `$PYENV_ROOT/versions` is a symlink, env entries are printed as absolute paths | Env entries are printed as `<version>/envs/<name>`, as without the symlink | realpath artifact; the name must not depend on how the directory is linked. |
| D-28 | Linux | `local`, `global` | A single argument containing `:` (`3.12:3.12.10`) is split and each part validated, then written as one word | Rejected: `` version `3.12:3.12.10' not installed `` | Accidental affordance of joining arguments with `:`; pass separate arguments instead. |
| D-29 | Linux | `which`, `whence`, `exec`, shims | A directory named like the command in a version's `bin` counts as found (`-x`) | Only runnable regular files count | Running a directory fails; mis-parsing. |
| D-30 | Linux | `which`, `exec`, shims (`system`) | Removes exact `${PYENV_ROOT}/shims` entries and `_PYENV_SHIM_PATHS_<PROGRAM>` dirs from `PATH` | Also removes the shims dir spelled with a trailing `/`, and passes over any hit that resolves to rpyenv's shim binary | A symlink to a shim elsewhere on `PATH` would otherwise make the shim run itself forever. |
| D-31 | Windows | `which`, `whence --path` | Every path component is printed in on-disk letter case (`GetFile().Path`) | The file name is in on-disk case; the folders are printed as built from the root | Cosmetic; the same file. |
| D-32 | Windows | `whence` | A version with the command in both `Scripts` and `bin` is listed twice | Each version is listed once | Duplicate output. |
