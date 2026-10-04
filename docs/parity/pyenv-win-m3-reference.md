# pyenv-win behavioral reference: rpyenv M3 shell integration

This is rpyenv's parity target on Windows for M3 (shell integration): `pyenv shell` in each host shell pyenv-win
supports, whatever else pyenv-win has for shell setup or completions, and how pyenv-win's pytest suite exercises both
when `parity/pyenv_win_overlay.py` points it at rpyenv. The file records facts only, and each fact carries an evidence
tag. Every `file:line` path is relative to the root of the pyenv-win repository at the pinned commit. When a path
starts with `libexec\` or `bin\`, it is under `pyenv-win\`.

The shell section of `pyenv-win-m1-reference.md` (around line 1240) covers `pyenv shell` from cmd and a short
summary of pyenv.ps1. This file re-verified it by probe, and found it correct. It adds what that section did not
cover: the effect on the caller's environment, exit codes, the PowerShell differences in full, Git Bash, the
absence of init and completions, and the overlay results.

## Source, version, and evidence method

- **Pinned upstream.** pyenv-win `856ed5a8c107879d53374f782a4b40cc794f19e0` (`.version` is `3.1.1`), read with
  `git -C C:/tmp/pyenv-win-856ed5a show HEAD:<path>`.
- **Probe copies** (all under `C:\tmp\m3ref_win\`, every one a `git clone` of that commit):
  - `pw` was checked out with `core.autocrlf=true`, so `.bat`, `.ps1` and the sh script `bin\pyenv` are CRLF on disk.
    This is the main probe root. Its `versions\` holds the empty folders `3.7.7`, `3.8.9` and `3.7.7-win32`.
  - `pw_lf` was checked out with `core.autocrlf=false`, so every file is LF, like a GitHub zip. It has the empty
    folders `3.7.7` and `3.8.9`. It was used only for the Git Bash probes.
  - `ov` and `ov2` are overlay-shaped roots: `pw\pyenv-win` plus rpyenv's `pyenv.exe`, `pyenv-shim.exe` and
    `pyenv-shimw.exe` in `bin`. `ov2` has **only** those three files in `bin`, with no `pyenv.bat` or `pyenv.ps1`.
  - `pw_overlay` is the checkout the overlay harness appends its conftest to. It is touched only by the harness.
- **Probe host.** Windows 11 Pro `10.0.26200.9457`, `AMD64`, on 2026-10-04.
  - Windows PowerShell `5.1.26100.9444` (`C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe`),
    execution policy `CurrentUser=Unrestricted`.
  - PowerShell `7.6.6`, a Microsoft Store (MSIX) install (`C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.6.0_x64__8wekyb3d8bbwe\pwsh.exe`),
    execution policy `LocalMachine=RemoteSigned`.
  - Git Bash `5.3.15(2)-release (x86_64-pc-cygwin)`, MSYS environment `UCRT64`, `C:\Program Files\Git\bin\bash.exe`.
  - Python 3.13.9, pytest 9.1.1. A child console starts at code page 850 (the console of the session itself is 65001).
  - No `HKCU\Software\Microsoft\Command Processor\AutoRun` value exists.
  - The user's `pwsh` profile exists (`Documents\PowerShell\Microsoft.PowerShell_profile.ps1`); the 5.1 one does not.
    Neither the probes nor the pytest runs passed `-NoProfile`, because the suite does not. The pwsh profile was
    loaded but never read or edited, and none of its output appears in any record.
- **Probe method.** `C:\tmp\m3ref_win\probe3.py` runs one shell command per probe through Python `subprocess`
  (`CREATE_NO_WINDOW`, stdin empty), and captures stdout, stderr and the exit code as raw bytes. Each record is in
  `C:\tmp\m3ref_win\probes3.jsonl` (with `stdout_hex` and `stderr_hex`). The runners are `run_probes_a.sh` to
  `run_probes_d.sh`, and the logs are `probes_a.log` to `probes_d2.log`.
  - **Safety.** The harness refuses to start unless its root is under `C:\tmp\m3ref_win`. Every probe sets `PYENV`,
    `PYENV_ROOT` and `PYENV_HOME` to that scratch root (with the trailing `\`). `PATH` is `<root>\bin;<root>\shims;`
    plus `%SystemRoot%` entries and the folder of `pwsh.exe`. Nothing wrote a profile, the registry or a user
    environment variable.
  - Two kinds of probe. A *direct* probe runs the same command line as the suite's `pyenv` fixture (`<shell run
    args> <root>\bin\pyenv.<ext> <args>`). A *script* probe writes `work\local\tmp.bat`, `tmp.ps1` or `tmp.sh` and
    runs it like the suite's `run` fixture, so that several commands share one shell session and the effect on the
    shell's environment can be read back.
  - The probe cwd is `C:\tmp\m3ref_win\work\local`; its parent holds an empty `.python-version`. No global `version`
    file exists. In the probe names below, `<S>` stands for `cmd`, `powershell` or `pwsh`.
- **Pytest runs.** `C:\tmp\m3ref_win\baseline.py` runs the unmodified suite on the `pw` checkout, with the same
  environment preparation as `parity/pyenv_win_run.py` (`installed_env`: the three variables name the checkout's
  `pyenv-win`, and its `bin` and `shims` lead `PATH`). `parity/pyenv_win_run.py` itself ran the overlay against
  `target\debug` (built with `cargo build --workspace`). Both selected only the shell file, with `-k`. The suite's
  `run` fixture then rewrites those three variables to each test's own root, so the real install
  (`C:\Users\JM\.pyenv\pyenv-win`) was never in play. `C:\tmp\m3ref_win\m3trace.py` is a pytest plugin that logged
  the argv of every `subprocess.run` to `trace_overlay.jsonl` and `trace_base.jsonl`.

| Tag | Meaning |
|---|---|
| `[src f:l]` | Read from pyenv-win code at that line. Not executed. |
| `[test f:l]` | Asserted by pyenv-win's pytest suite (`tests\`). |
| `[probe <name>]` | Observed by running the pinned scripts in the scratch roots, as described above. `<name>` is the record in `probes3.jsonl`. |
| `[run <name>]` | Observed in a pytest run: `baseline` (unmodified suite, pyenv-win), `overlay` (strict xfail, as CI runs it), `overlay-runxfail` (`--runxfail`, so the real failures show), `trace` (argv log). |
| **UNCONFIRMED** | None of the above. |

The pytest output normalization (keep the text after the last `\x0c`, strip CR/LF from both ends, `stderr` stripped
fully, never assert exit codes) is in `tests\test_pyenv_helpers.py:120-126` [test], and is described in
`pyenv-win-m1-reference.md`.

## Inventory: what pyenv-win has for shell setup

| Item | Exists? | Evidence |
|---|---|---|
| `pyenv shell` in cmd | Yes: `pyenv.bat` routes to `libexec\pyenv-shell.bat` | [src bin\pyenv.bat:131-169, libexec\pyenv-shell.bat] |
| `pyenv shell` in Windows PowerShell 5.1 and PowerShell 7 | Yes: `bin\pyenv.ps1`, one file for both | [src bin\pyenv.ps1:1-22] |
| `pyenv shell` in Git Bash | **No shell integration.** `bin\pyenv` is a plain sh wrapper around `pyenv.bat`, so `shell` prints, but cannot set anything | [src bin\pyenv:1-3; probe bash_*] |
| `pyenv init`, `shell-init`, `completions`, `hooks` | **No.** Each prints `pyenv: no such command '<x>'` and exits 1 | [probe cmd_list_init, cmd_list_shell_init, cmd_list_completions_shell, cmd_list_hooks] |
| Completion scripts (bash, zsh, fish, PowerShell) | **No.** No such file exists at the pinned commit | [git ls-tree below] |
| Profile editing (`.bashrc`, `$PROFILE`) | **No.** Nothing in the code writes a profile | [grep below] |
| `PYENV_SHELL` | **No.** Nothing reads or writes it | [grep below] |
| Install-time setup | `install-pyenv-win.ps1` writes user-scope `PYENV`, `PYENV_ROOT`, `PYENV_HOME` and `PATH` through `SetEnvironmentVariable(..., "User")` | [src install-pyenv-win.ps1:130-141] |

Evidence for the "No" rows:

- `git ls-tree -r --name-only HEAD` lists, under `pyenv-win\bin\`: `pyenv`, `pyenv.bat`, `pyenv.ps1` and `WiX\*`. Under
  `pyenv-win\libexec\`: `pyenv---version.bat`, `pyenv-commands.bat`, `pyenv-duplicate.bat`, `pyenv-exec.bat`,
  `pyenv-export.bat`, `pyenv-global.bat`, `pyenv-help.bat`, `pyenv-install.vbs`, `pyenv-latest.vbs`,
  `pyenv-local.bat`, `pyenv-migrate.bat`, `pyenv-rehash.bat`, `pyenv-shell.bat`, `pyenv-shims.bat`,
  `pyenv-uninstall.vbs`, `pyenv-update.vbs`, `pyenv-version-name.bat`, `pyenv-version.bat`, `pyenv-versions.bat`,
  `pyenv-vname.bat`, `pyenv-whence.bat`, `pyenv-which.bat`, `pyenv.vbs` and `libs\*.vbs`. Nothing is named
  `completions`, `init`, `*.bash`, `*.zsh`, `*.fish` or `*.psm1`.
- `git grep -n -i "completion\|autocomplete\|\binit\b\|PROMPT\|profile\|doskey\|bashrc\|PYENV_SHELL"` over the
  non-binary files (outside the vendored `.versions_cache.xml`, `bin\WiX` and `docs\_*`) finds only: `codeql-analysis.yml`
  (`github/codeql-action/init`), `README.md:104` ("Reopen the command prompt"), `install-pyenv-win.ps1:29`
  (`${env:USERPROFILE}`), `install-pyenv-win.ps1:122` (`-NoProfile`) and the usage strings in two `.vbs` files.
- `pyenv commands` prints exactly these 22 lines [probe cmd_list_commands]: `--version`, `commands`, `duplicate`,
  `exec`, `export`, `global`, `help`, `install`, `latest`, `local`, `migrate`, `rehash`, `shell`, `shims`,
  `uninstall`, `update`, `version-name`, `version`, `versions`, `vname`, `whence`, `which`. (21 command names
  plus `--version`.) No `sh-*` command exists.
- Documentation. `docs\installation.md:168-175` ("Usage with Git BASH") tells the user to append two lines to
  `~/.bash_profile` by hand: `export PATH="$HOME/.pyenv/pyenv-win/shims:$PATH"` and the same for `bin`. `docs\changelog.md:43`
  says "Fix #193: PowerShell support for `pyenv shell`", and `:17-20` records that `pyenv global` and `pyenv local` no
  longer touch `PYENV_VERSION`, that the suite also runs `powershell` and `pwsh`, and that `pyenv shell` adds `-win32`
  on 32-bit.
- Environment setup for the host shells is therefore only the User-scope variables and `PATH` entries above. It
  applies to cmd, PowerShell and Git Bash alike, and it needs a new terminal [src README.md:47; install-pyenv-win.ps1:149].

## How each host shell reaches pyenv-win

The `bin` folder is first on `PATH` (and `shims` second). `pyenv` is resolved by the host shell as follows.

| Shell | What `pyenv` resolves to | Evidence |
|---|---|---|
| cmd | `pyenv.bat`: `PATHEXT` finds `.BAT`; the extensionless sh script is not executable by cmd. In an overlay root it resolves to `pyenv.exe` instead, because `.EXE` precedes `.BAT` in `PATHEXT` | [probe ov_cmd_version, ov2_cmd] |
| Windows PowerShell 5.1 | `pyenv.ps1` (CommandType `ExternalScript`), then `pyenv.bat` and the extensionless `pyenv` (both `Application`). In an overlay root the order is `pyenv.ps1`, `pyenv.exe`, `pyenv.bat`, `pyenv` | [probe powershell_chain_resolve, ov_ps_resolve] |
| PowerShell 7 | The same order as 5.1 | [probe pwsh_chain_resolve, ov_pwsh_resolve] |
| Git Bash | The extensionless `bin\pyenv` (sh) | [probe bash_pw_lf_which] |

- With `pyenv.ps1` absent (root `ov2`, whose `bin` has only the rpyenv executables), both PowerShells resolve `pyenv`
  to `pyenv.exe` [probe ov2_ps].
- With `pyenv.ps1` present but blocked (`Set-ExecutionPolicy -Scope Process Restricted`), both PowerShells report
  `File ...\pyenv.ps1 cannot be loaded because running scripts is disabled on this system` (`PSSecurityException`,
  `UnauthorizedAccess`). They do **not** fall back to `pyenv.bat`, and `$LASTEXITCODE` stays unset
  [probe powershell_execpol_restricted, pwsh_execpol_restricted].
- The sh script `bin\pyenv` is `MSYS2_ARG_CONV_EXCL="/C" exec cmd /C call "$(cygpath -wa "$(dirname "$0")")/pyenv.bat" "$@"`
  [src bin\pyenv:3]. It needs `cygpath` and `dirname` from the MSYS environment. In the `pw` root it is a CRLF file;
  Git Bash ran it identically to the LF copy in `pw_lf`, with equal results for every probe [probe bash_pw_*, bash_pw_lf_*].

## `pyenv shell` in cmd (pyenv.bat)

Re-verified by probe; it matches `pyenv-win-m1-reference.md` → *shell*. Probes ran as `cmd /d /c call <root>\bin\pyenv.bat ...`
(the suite's form) or inside a `.bat` file.

### Route [src bin\pyenv.bat:1-169, libexec\pyenv-shell.bat]

1. `pyenv.bat` runs `chcp 65001 >nul 2>&1` first [src pyenv.bat:3]. The code page is **not** restored: a script
   that prints `chcp` before and after `call pyenv shell 3.7.7` shows `850` then `65001` [probe cmd_chain_cp].
2. `shell` is not in pyenv.bat's built-in list (`rehash global local version vname version-name versions commands
   shims which whence help --help`) [src pyenv.bat:51]. It goes to `:plugin`, which does `endlocal && endlocal`
   before running `call "<libexec>\pyenv-shell.bat" <args>` [src pyenv.bat:131-168]. The two `endlocal`s are what
   let `pyenv-shell.bat` change the caller's environment.
3. `pyenv-shell.bat` runs `setlocal`, and for `--unset` or a version it runs `endlocal && set "PYENV_VERSION=..."`
   so that the variable is set in the caller's frame [src pyenv-shell.bat:27-35].

### Behavior by form

All outputs are on stdout, with CRLF endings; stderr was empty in every cmd probe.

| Command | stdout | Exit | `PYENV_VERSION` afterwards | Evidence |
|---|---|---|---|---|
| `pyenv shell`, variable unset or empty | `no shell-specific version configured` | 0 | unchanged | [probe cmd_noargs_unset; test shell.py:31-33] |
| `pyenv shell`, variable `3.8.9` | `3.8.9` (raw, not validated; `3.9.2` is printed in the suite although that version does not exist) | 0 | unchanged | [probe cmd_noargs_set; test shell.py:36-38] |
| `pyenv shell`, variable `3.7.7 3.8.9` | `3.7.7 3.8.9` | 0 | unchanged | [probe cmd_noargs_many; test shell.py:99-101] |
| `pyenv shell 3.7.7` | nothing | 0 | **set** to `3.7.7` | [probe cmd_chain_set_unset: `a[3.7.7] rc=0`] |
| `pyenv shell 3.7.7 3.8.9` | nothing | 0 | **set** to `3.7.7 3.8.9`, in the order given (`3.8.9 3.7.7` stays in that order) | [probe cmd_many_sep; test shell.py:80-91] |
| `pyenv shell 3.9.9` (not installed) | the two-line message below | 1 | unchanged (`3.8.9` stays `3.8.9`) | [probe cmd_set_unknown, cmd_chain_fail_preset] |
| `pyenv shell 3.7.7 9.9.9` | the message for `9.9.9` only | 1 | unchanged (nothing is set, even though `3.7.7` is valid) | [probe cmd_set_one_bad; test shell.py:94-96] |
| `pyenv shell 3.7` (prefix) | the message for `3.7` | 1 | unchanged | [probe cmd_set_prefix] |
| `pyenv shell system` | the message for `system` | 1 | unchanged | [probe cmd_set_system] |
| `pyenv shell ""` | the message with an empty name: `Install python '' by typing: 'pyenv install '` | 1 | unchanged | [probe cmd_emptyarg] |
| `pyenv shell --unset` (also `--UNSET`) | nothing | 0 | **removed** | [probe cmd_unset, cmd_unset_upper, cmd_chain_set_unset: `b[] rc=0`] |
| `pyenv shell --unset extra` | nothing | 0 | **removed** (the extra argument is ignored) | [probe cmd_unset_extra] |
| `pyenv shell --unset 3.7.7` | nothing | 0 | removed | [probe cmd_unset_then_ver] |
| `pyenv shell 3.7.7 --unset` | message for `--unset` | 1 | unchanged | [probe cmd_ver_then_unset] |
| `pyenv shell --help`, `pyenv --help shell`, `pyenv help shell` | the help text below | 0 | unchanged | [probe cmd_help_a, cmd_help_b, cmd_help_c; test shell.py:21-28] |
| `pyenv shell --help 3.7.7` | the help text | 0 | unchanged | [probe cmd_help_f] |
| `pyenv shell -h` | message for `-h` | 1 | unchanged | [probe cmd_help_d] |
| `pyenv shell 3.7.7 --help` | message for `--help` | 1 | unchanged | [probe cmd_help_e] |
| `pyenv shell -- 3.7.7` | message for `--` | 1 | unchanged | [probe cmd_unset_extra] |
| `pyenv SHELL 3.7.7` | nothing | 0 | set (the plugin file name matches case-insensitively on NTFS) | [probe cmd_upper_cmd; the env effect was not read back: UNCONFIRMED] |

- **Not-installed message** (two lines, no trailing text) [src libs\pyenv-lib.vbs:265-268]:

  ```
  pyenv specific python requisite didn't meet. Project is using different version of python.
  Install python '<name>' by typing: 'pyenv install <name>'
  ```

- **Help text**, 6 lines plus a final blank line, ending `...\r\n\r\n` [src pyenv-shell.bat:10-17; probe cmd_help_a]. The suite
  asserts only the first two lines [test shell.py:28]:

  ```
  Usage: pyenv shell <version>
         pyenv shell --unset

  Sets a shell-specific Python version by setting the `PYENV_VERSION'
  environment variable in your shell. This version overrides local
  application-specific versions and the global version.

  ```

- **Validation.** `pyenv.vbs shell` applies `Check32Bit` to each argument and then `GetBinDir` (a folder named exactly
  the argument must exist under `versions\`; no prefix match, no `system`) [src pyenv.vbs:409-430; libs\pyenv-lib.vbs:261-271,
  450-455]. `pyenv-shell.bat` runs that cscript twice on success: once with stdout discarded, then once more to
  capture the line [src pyenv-shell.bat:31-34].
- **32-bit.** With `PYENV_FORCE_ARCH=X86`, `-win32` is appended unless the name already ends in it, case-insensitively,
  and the stored value keeps the user's case: `3.7.7` gives `3.7.7-win32`, and `3.7.7-WIN32` stays `3.7.7-WIN32`. `3.8.9`
  with only `3.8.9` installed fails with a message for `3.8.9-win32` [probe cmd_x86]. On AMD64, `3.7.7-win32` is accepted
  as is (given a folder of that name) [probe cmd_amd64_win32].
- **Control transfer.** Run without `call`, `pyenv shell 3.7.7` ends the calling batch file: a following `echo` is
  never executed, and nothing is printed [probe cmd_chain_nocall]. Run with `call`, the rest of the file runs.
- **Failure and `&&`.** `call pyenv shell 9.9.9 && echo ok || echo fail` prints the message, then `fail`
  [probe cmd_chain_fail_and]. The exit code is 1.
- **Scope.** The variable is set in the frame of the caller. Inside a caller's own `setlocal`, it is visible until the
  matching `endlocal` and then reverts: `in[3.7.7]` then `out[]` [probe cmd_endlocal_nested].
- **Environment leakage.** Comparing `set` before and after `call pyenv shell 3.7.7` gives exactly one added line,
  `PYENV_VERSION=3.7.7` [probe cmd_chain_envdiff]. The `skip`, `skip_arg`, `exe` and `cmdline` variables do not
  leak.
- **No-arg print is an unquoted `echo`** [src pyenv-shell.bat:24]. A value of `a&echo INJECTED` prints `a` and then
  runs `echo INJECTED`; `x>m3_redirect.txt` creates that file; `(a)` prints `(a` [probe cmd_chain_metachar,
  cmd_chain_metachar2, cmd_chain_paren]. (These closed the M1 reference's UNCONFIRMED item about metacharacters.)
- **Other commands see the value.** After `call pyenv shell 3.7.7`, `pyenv version` prints `3.7.7 (set by %PYENV_VERSION%)`
  (a literal `%PYENV_VERSION%`), `pyenv version-name` prints `3.7.7`, and `pyenv versions` marks `* 3.7.7 (set by %PYENV_VERSION%)`
  [probe cmd_chain_version; src libs\pyenv-lib.vbs:144-148; test version.py:56-57].
- **Percent expansion timing.** `@call pyenv shell 3.7.7 3.8.9 && echo [%PYENV_VERSION%]` prints `[]`, because cmd
  expands `%PYENV_VERSION%` when it parses the line, before the call runs [probe cmd_chain_many]. This is why the
  suite chains `&& call pyenv shell` instead [test shell.py:48].

## `pyenv shell` in PowerShell (pyenv.ps1)

Both PowerShells ran the same `pyenv.ps1`, and every probe gave equal stdout and exit codes in `powershell` and
`pwsh` (the stderr differs only in how errors are rendered; `pwsh` adds ANSI colour codes) [probe `<S>_*`].

### Code [src bin\pyenv.ps1:1-22]

```powershell
$OutputEncoding = [console]::InputEncoding = [console]::OutputEncoding = New-Object System.Text.UTF8Encoding
If (($Args.Count -ge 2) -and ($Args[0] -eq "shell")) {
    if ($Args[1] -eq "--help") {
        pyenv.bat @Args
        Exit $LastExitCode
    } elseif ($Args[1] -eq "--unset") {
        If (Test-Path Env:PYENV_VERSION) { Remove-Item Env:PYENV_VERSION }
    } else {
        $Output = (cscript //nologo "$PSScriptRoot\..\libexec\pyenv.vbs" @Args)
        if ($LastExitCode -ne 0) {
            $Output -join [Environment]::NewLine
            Exit $LastExitCode
        }
        $Env:PYENV_VERSION = $Output
    }
} Else {
    pyenv.bat @Args
    Exit $LastExitCode
}
```

(Condensed; the real file has the braces on separate lines.) So `shell` is handled by the script itself only when it
has **two or more arguments** and the first is `shell` (`-eq` is case-insensitive). Everything else, including
`pyenv shell` with no arguments, `pyenv --help shell` and `pyenv help shell`, is passed to `pyenv.bat`.

### Behavior by form

| Command | stdout | Exit (when `pyenv.ps1` is the whole `-Command`) | `$env:PYENV_VERSION` afterwards | Evidence |
|---|---|---|---|---|
| `pyenv shell` (no arguments) | as cmd: `no shell-specific version configured`, or the raw value | 0 | unchanged | [probe `<S>`_noargs_unset, `<S>`_noargs_set, `<S>`_noargs_many] |
| `pyenv shell 3.7.7` | nothing | 0 | **set** to `3.7.7` | [probe `<S>`_chain_set_unset, `<S>`_set_ok] |
| `pyenv shell 3.7.7 3.8.9` | nothing | 0 | **set** to `3.7.7 3.8.9` | [probe `<S>`_chain_many] |
| `pyenv shell 9.9.9` | the two-line not-installed message (joined with the platform newline, so CRLF) | 1 | unchanged (`3.8.9` stays) | [probe `<S>`_set_unknown, `<S>`_chain_fail_preset] |
| `pyenv shell 3.7.7 9.9.9` | the message for `9.9.9` | 1 | unchanged | [probe `<S>`_set_one_bad] |
| `pyenv shell --unset` | nothing | 0 | **removed**: `Test-Path Env:PYENV_VERSION` is `False` | [probe `<S>`_unset, `<S>`_chain_set_unset] |
| `pyenv shell --unset extra`, `--UNSET` | nothing | 0 | removed | [probe `<S>`_two_arg_unset_like, `<S>`_unset_upper] |
| `pyenv shell --unset 3.7.7` | nothing | 0 | removed | [probe `<S>`_unset_then_ver] |
| `pyenv shell 3.7.7 --unset` | message for `--unset` | 1 | unchanged | [probe `<S>`_ver_then_unset] |
| `pyenv shell --help`, `pyenv --help shell`, `pyenv help shell` | the help text (the same bytes as cmd) | 0 | unchanged | [probe `<S>`_help_a, `<S>`_help_b, `<S>`_help_c] |
| `pyenv shell --help extra` | the help text | 0 | unchanged | [probe `<S>`_two_arg_unset_like, `<S>`_help_f] |
| `pyenv shell 3.7.7 --help` | message for `--help` | 1 | unchanged | [probe `<S>`_help_e] |
| `pyenv shell -h` | message for `-h` | 1 | unchanged | [probe `<S>`_help_d] |
| `pyenv shell ""` | the no-argument output (PowerShell drops the empty argument, so `$Args.Count` is 1) | 0 | unchanged | [probe `<S>`_emptyarg] |
| `pyenv shell -- 3.7.7` | nothing | 0 | **set** to `3.7.7` (PowerShell consumes `--`; in cmd the same words fail) | [probe `<S>`_two_arg_unset_like] |
| `pyenv SHELL 3.7.7` | nothing | 0 | (not read back: **UNCONFIRMED**) | [probe `<S>`_upper_cmd] |

- **The caller's environment really changes.** `$env:PYENV_VERSION` is process-wide, so it persists in the session
  after the script returns, and a child started afterwards sees it: `pyenv shell 3.7.7; cmd /c "echo child=%PYENV_VERSION%"`
  prints `child=3.7.7` [probe `<S>`_chain_childenv]. `--unset` removes it from the process environment, not just
  empties it.
- **Only the `.ps1` does this.** `pyenv.bat shell 3.7.7` typed in PowerShell runs a child cmd, prints nothing,
  returns 0 and leaves `$env:PYENV_VERSION` empty [probe `<S>`_chain_pyenvbat].
- **Exit code propagation.** `Exit $LastExitCode` leaves only the script, not the host. After a failing
  `pyenv shell 9.9.9`, the next statement of a `-Command` script sees `$LASTEXITCODE` = 1 and `$?` = `False`, and
  the host process still exits 0 unless that is the last statement [probe `<S>`_chain_fail_exit,
  `<S>`_chain_fail_preset]. As a direct `-Command <root>\bin\pyenv.ps1 shell 9.9.9`, the process exits 1
  [probe `<S>`_set_unknown; run trace].
- **Output capture.** `$x = (pyenv shell 9.9.9)` captures the message as one string with an embedded CRLF; on
  success, `pyenv shell 3.7.7 | Out-String` yields an empty string [probe `<S>`_chain_oneargs].
- **Replacing a many-version value.** With `PYENV_VERSION=3.7.7 3.8.9`, `pyenv shell 3.8.9` leaves `3.8.9`
  [probe `<S>`_multi_out].
- **32-bit.** `PYENV_FORCE_ARCH=X86` behaves as in cmd: `3.7.7` stores `3.7.7-win32`, `3.7.7-WIN32` is kept as
  typed, and `3.8.9` (not installed as `-win32`) fails with the message for `3.8.9-win32`, leaving the old value
  [probe `<S>`_x86]. On AMD64, `3.7.7-win32` is accepted [probe `<S>`_amd64_win32].
- **Console encodings.** The first line of the script sets `[console]::InputEncoding` and `OutputEncoding` to UTF-8
  for the whole process. In `powershell` the code pages go `850` to `65001` (`InputEncoding` too), while
  `$OutputEncoding` read from the caller stays `20127` (the assignment is script-scoped). In `pwsh`, `OutputEncoding`
  goes `850` to `65001` and `$OutputEncoding` was already `65001` [probe `<S>`_chain_enc].
- **Reliance on `PATH`.** The `shell` branches of `pyenv.ps1` find libexec through `$PSScriptRoot`. But `pyenv.bat @Args`
  is found through `PATH`. With `PATH` stripped to `C:\Windows\system32;C:\Windows`, `& <root>\bin\pyenv.ps1 shell 3.7.7`
  and `shell --unset` still work, while `pyenv.ps1 shell` and `pyenv.ps1 --version` fail with
  `The term 'pyenv.bat' is not recognized as the name of a cmdlet, function, script file, or operable program`
  [probe `<S>`_nopath]. The branches that use cscript need `cscript.exe` on `PATH`.
- **Execution policy.** A blocked `pyenv.ps1` is not skipped: see the resolution table above.
- **Differences from cmd.** cmd and PowerShell have separate code for the same command (the `.bat` plugin versus `pyenv.ps1`).
  The observed differences: `--` and the empty argument (above); a failure prints the vbs message through
  `$Output -join [Environment]::NewLine`; success makes one cscript call (cmd runs the validation twice); and `--unset` in both
  ignores extra arguments.

## `pyenv shell` in Git Bash

`bin\pyenv` forwards to `pyenv.bat` in a child cmd, and a child process cannot change its parent's environment, so
`shell` has no effect on the bash session. Observed identically in the CRLF root `pw` and the LF root `pw_lf`
[probe bash_pw_*, bash_pw_lf_*; each ran `bash -c <script>` with `pyenv` on `PATH`]:

| Command (in a bash script) | Output | Exit | `$PYENV_VERSION` after |
|---|---|---|---|
| `pyenv --version` | `pyenv 3.1.1` | 0 | |
| `pyenv shell` | `no shell-specific version configured` | 0 | |
| `pyenv shell 3.7.7` | nothing | 0 | still empty (`[]`) |
| `pyenv shell` straight after `pyenv shell 3.7.7` | `no shell-specific version configured` | 0 | |
| `export PYENV_VERSION=3.8.9; pyenv shell --unset` | nothing | 0 | still `3.8.9` |
| `export PYENV_VERSION=3.8.9; pyenv shell` | `3.8.9` | 0 | `3.8.9` |
| `pyenv shell 9.9.9` | the two-line not-installed message | 1 | |
| `pyenv shell --help`, `pyenv help shell` | the help text | 0 | |

All stdout lines end in CRLF. The success of `shell 3.7.7` is silent, so the user gets no sign that nothing happened.
The documented Git Bash setup is only the two `export PATH=...` lines [src docs\installation.md:168-175].

## `pyenv` with no arguments, `help`, `--help`, `--version`

These are context for the suite's "features list" test and for `pyenv help shell`:

- `pyenv` alone prints `pyenv 3.1.1`, a blank line, then the usage text with a `shell        Set or show the shell-specific
  Python version` entry, ending `For full documentation, see: https://github.com/pyenv-win/pyenv-win#readme` [probe cmd_list_bare]. Exit 0.
- `pyenv help` prints a shorter usage list that also has the `shell` entry, with a trailing blank line. Exit 0
  [probe cmd_list_help; src libexec\pyenv-help.bat:12].
- `pyenv --help` prints `pyenv: no such command '--help'`, exit 1 [probe cmd_list___help]. (`pyenv --help shell` is a
  different path and works: it is routed to the plugin.)
- `pyenv --version` prints `pyenv 3.1.1`, exit 0; an unknown command prints `pyenv: no such command 'bogus'`, exit 1
  [probe cmd_list___version, cmd_list_bogus].

## The pyenv-win pytest suite: shell tests

### How each shell is invoked

`tests\conftest.py` defines these fixtures (the command lines were logged by `m3trace.py`):

- `shell` is `"cmd"` by default [src tests\conftest.py:12-14]. `tests\test_pyenv_feature_shell.py:8-13` overrides it with a
  module-scoped fixture parametrized over `["cmd", "powershell", "pwsh"]`, and skips a shell when
  `shutil.which(shell)` is `None`. Every test in that file (including those that only use the `pyenv` fixture)
  therefore runs three times. Test ids end in `[cmd]`, `[powershell]`, `[pwsh]`, or `[cmd-<lambda>]` when
  `settings` is parametrized.
- `arch` is a session fixture parametrized `AMD64`, `X86` (`PYENV_FORCE_ARCH`), so every test also runs twice. A
  session after the first one is **skipped** when `request.session.testsfailed` is non-zero
  [src tests\conftest.py:63-70]. The 27 ids for the shell file thus become 54 tests.
- `run_args` [src tests\conftest.py:95-102]:

  | Shell | `run_args` | Resulting command line (from `subprocess.run`) |
  |---|---|---|
  | cmd | `['cmd', '/d', '/c', 'call']` | `cmd /d /c call "<root>\bin\pyenv.bat" <args>` |
  | powershell | `['powershell', '-Command']` | `powershell -Command <root>\bin\pyenv.ps1 <args>`, with every space of the path escaped as a backtick-space |
  | pwsh | `['pwsh', '-Command']` | the same, with `pwsh` |

- `pyenv_file` is `<root>\bin\pyenv.bat` or `pyenv.ps1` by `shell_ext`, and for the PowerShells the path has `' '` replaced by
  `` '` ' `` [src tests\conftest.py:55-60]. The roots have spaces by construction (`pyenv dir with spaces`, `local dir with
  spaces`) [src tests\conftest.py:30-37].
- `pyenv(*args)` runs `run(pyenv_file, *args)`; `pyenv.shell(...)` runs `run(pyenv_file, 'shell', ...)`. `run` copies the
  parent environment, rewrites `PYENV`, `PYENV_ROOT`, `PYENV_HOME` (and the matching `PATH` entry) to the test's root, removes
  every `PATH` folder containing a `python.exe`, and drops `PYTHONPATH` and `VIRTUAL_ENV`. `kwargs["env"]` is merged on top
  [src tests\conftest.py:105-131].
- The tests that need a persisting environment write `tmp.bat` or `tmp.ps1` into `local dir with spaces\` and run it as
  `run(tmp_bat)`. The script bodies are [test shell.py:48, 51, 72, 75, 86, 89]:

  | Shell | Body (examples) |
  |---|---|
  | cmd | `@call pyenv shell <v> && call pyenv shell` |
  | powershell, pwsh | `& pyenv shell <v>; & pyenv shell` and `pyenv global --unset; pyenv local --unset; pyenv shell` |

  The script path for the PowerShells is escaped the same way. In these scripts `pyenv` is **not** a path: it is resolved from
  `PATH` by the shell, so the resolution order in the table above decides which program runs.

- Logged command lines, unmodified suite (`trace_base.jsonl`, AMD64; the first rows of each shell) [run trace]:

  ```
  ['cmd', '/d', '/c', 'call', '...\pyenv dir with spaces\bin\pyenv.bat', 'shell', '3.7.8']                 rc 1
  ['cmd', '/d', '/c', 'call', '...\local dir with spaces\tmp.bat']                                          rc 0
  ['powershell', '-Command', '...\pyenv` dir` with` spaces\bin\pyenv.ps1', 'shell', '3.7.8']              rc 1
  ['powershell', '-Command', '...\local` dir` with` spaces\tmp.ps1']                                      rc 0
  ['pwsh', '-Command', '...\pyenv` dir` with` spaces\bin\pyenv.ps1', '--help', 'shell']                   rc 0
  ```

  The same three forms on the **overlay** (`trace_overlay.jsonl`) differ in the direct rows only: the file is `...\pyenv dir with
  spaces\bin\pyenv.exe`, without backtick escapes, for all three shells [run trace].
- The CI workflows are not the suite's own shell matrix: `pytest.yml` runs `.github\scripts\build.sh` under Git Bash, which
  exports `PYENV*`, adds `bin` and `shims` to `PATH`, and runs `python -m pytest` on the checkout; `pytest_ps1.yml` installs
  with `install-pyenv-win.ps1` under `pwsh`, then runs `.github\scripts\ps1.sh` [src .github\workflows\pytest.yml,
  pytest_ps1.yml, .github\scripts\build.sh, ps1.sh].

### Per-test assertions

All nine test functions in `tests\test_pyenv_feature_shell.py`, each run in `cmd`, `powershell` and `pwsh`.
`Native(v)` is `v` on AMD64 and `v-win32` on X86; `Arch(v)` is `v` unchanged. `not_installed_output(v)` is the two-line message above
[test tests\test_pyenv_helpers.py:104-107]. Results are the AMD64 session; see the overlay section for the X86 session.

| Id (without the shell suffix) | Lines | What it asserts | pyenv-win baseline | rpyenv overlay (cmd / powershell / pwsh) |
|---|---|---|---|---|
| `test_shell_help` | 21-28 | For `--help shell`, `help shell` and `shell --help`: the first two stdout lines joined with CRLF are the two Usage lines, and stderr is `""` | pass x3 | FAIL / FAIL / FAIL |
| `test_no_shell_version` | 31-33 | With `PYENV_VERSION=""`, `pyenv shell` prints `no shell-specific version configured` | pass x3 | FAIL / FAIL / FAIL |
| `test_shell_version_defined` | 36-38 | With `PYENV_VERSION=Native("3.9.2")`, `pyenv shell` prints that value (no install needed) | pass x3 | FAIL / FAIL / FAIL |
| `test_shell_set_installed_version` | 41-53 | The root has 3.7.7 and 3.8.9; with `PYENV_VERSION=Native("3.8.9")` preset, a script runs `pyenv shell <3.7.7>` then `pyenv shell`, and the output is `Native("3.7.7")` (the set replaced the preset in the same shell) | pass x3 | FAIL / pass / pass |
| `test_shell_set_unknown_version` | 56-58 | Only 3.8.9 installed: `pyenv shell Native("3.7.8")` prints the not-installed message | pass x3 | FAIL / FAIL / FAIL |
| `test_shell_unset_unaffected` | 61-77 | Global and local both 3.7.7, preset `PYENV_VERSION=3.8.9`: a script runs `global --unset`, `local --unset`, `shell`, and prints `Native("3.8.9")` (unsetting global and local does not touch the shell variable) | pass x3 | FAIL / pass / pass |
| `test_shell_set_many_versions` | 80-91 | A script runs `pyenv shell 3.7.7 3.8.9` then `pyenv shell`, and prints `3.7.7 3.8.9` (Native names, one space) | pass x3 | FAIL / pass / pass |
| `test_shell_set_many_versions_one_not_installed` | 94-96 | Only 3.7.7 installed: `pyenv shell 3.7.7 3.8.9` prints the not-installed message for `3.8.9` | pass x3 | FAIL / FAIL / FAIL |
| `test_shell_many_versions_defined` | 99-101 | With `PYENV_VERSION="3.7.7 3.8.9"`, `pyenv shell` prints it | pass x3 | FAIL / FAIL / FAIL |

Other tests that touch shell behavior or the `shell` command's neighbours; none touches completions or init (the suite has none):

| Test | Lines | Assertion about shell setup |
|---|---|---|
| `test_pyenv.py::test_check_pyenv_features_list` | 15-32 | `pyenv` with no arguments: stderr `""`, and the text contains `commands`, `duplicate`, `local`, `global`, `shell`, `install`, `uninstall`, `rehash`, `version`, `vname`, `versions`, `version-name`, `exec`, `which`, `whence` |
| `test_pyenv.py::test_check_pyenv_path` | 3-5 | The `pyenv` bin folder is in `%PATH%` as seen by cmd (`run('echo', '%PATH%')`) |
| `test_pyenv.py::test_check_pyenv_version` | 8-12 | `.version` content appears in the output of bare `pyenv` |
| `test_pyenv_feature_commands.py` | 3-11 | Placeholders (`pass`); `test_check_pyenv_commands_list` only runs `pyenv()` |
| `test_pyenv_feature_{version,versions,version_name,which,rehash}.py` | `version.py:56-57`, `versions.py:71-76, 119`, `version_name.py:64`, `which.py:51, 72, 119`, `rehash.py:51` | They pass `PYENV_VERSION` through `env=` as the input that `pyenv shell` would have set; `pyenv version` and `versions` print `(set by %PYENV_VERSION%)` literally |
| `tests\bat_files\test_install.bat` | 45-61 | A manual script (not collected by pytest) that runs `pyenv shell 3.7.2` and `pyenv shell --unset` |

`test_check_pyenv_features_list`, `test_check_pyenv_version` and `test_check_pyenv_commands_list` ran against rpyenv through the overlay and **passed** (AMD64 and X86, 6 passed in total) [run overlay]; `test_check_pyenv_path` was not run;
`pyenv.exe` with no arguments already prints a `shell` entry, and its help text has the pyenv-win wording.

## Overlay results against rpyenv

### What the overlay does [src parity\pyenv_win_overlay.py, parity\pyenv_win_run.py]

- `pyenv_win_run.py` appends the overlay to a throwaway copy's `tests\conftest.py` once, builds the environment with
  `installed_env` (`PYENV`, `PYENV_ROOT` and `PYENV_HOME` all name `<checkout>\pyenv-win`, whose `bin` and `shims` lead
  `PATH`), and runs `pytest -p no:cacheprovider -q -rfEX --rootdir <tests> <tests>`. Extra arguments go to pytest.
- The overlay redefines two fixtures. `pyenv_file` returns `<root>\bin\pyenv.exe` for **every** shell
  (`str(Path(bin_path, "pyenv.exe"))`: no `shell_ext`, no backtick escaping). `tmp_pyenv` runs the same `pyenv_setup` and then copies
  `pyenv.exe`, `pyenv-shim.exe` and `pyenv-shimw.exe` from `RPYENV_BIN` into the test root's `bin`.
- `pyenv_setup` itself copies into every root: `bin\pyenv.bat`, `bin\pyenv.ps1`, `libexec\*` (so `pyenv.vbs`,
  `pyenv-shell.bat` and the rest), `libexec\libs\*.vbs`, `.versions_cache.xml`, `..\.version` and `bin\WiX\*`
  [src tests\test_pyenv_helpers.py:41-72]. So an overlay root contains **both** pyenv-win's and rpyenv's entry points.
- `pytest_collection_modifyitems` marks the ids listed in `parity\expected\pyenv-win.txt` as strict xfails, with the `AMD64`/`X86`
  arch parameter removed from the key. A listed id that does not exist, or an invalid allowlist reason, aborts the run
  with a `UsageError`.

### Results

Run with `python parity\pyenv_win_run.py --pyenv-win C:/tmp/m3ref_win/pw_overlay --rpyenv target\debug -k test_pyenv_feature_shell`
(rpyenv built from the working tree of branch `m3-shell-integration`, `cargo build --workspace`).

| Run | Outcome |
|---|---|
| Unmodified suite on pyenv-win (`baseline.py -k test_pyenv_feature_shell`) | **54 passed**, 0 failed, in 228 s [run baseline] |
| Overlay, strict xfail, as CI runs it (`overlay_strict.txt`) | **12 passed, 42 xfailed**, in 141 s; no XPASS (a strict xpass would have failed the run). Exit 0 [run overlay] |
| Overlay with `--runxfail` (`overlay_runxfail.txt`), so the real failures show | **21 failed, 6 passed, 27 skipped** in 60 s. The X86 session is skipped because AMD64 had failures (the `arch` fixture rule above) [run overlay-runxfail] |

Per test id, AMD64 session (the X86 session in the strict run gave the same: the same 21 ids xfailed and the same 6 ids passed, 12 passed
and 42 xfailed over both sessions):

| Test id | cmd | powershell | pwsh |
|---|---|---|---|
| `test_shell_help` | FAIL | FAIL | FAIL |
| `test_no_shell_version` | FAIL | FAIL | FAIL |
| `test_shell_version_defined` | FAIL | FAIL | FAIL |
| `test_shell_set_installed_version[...<lambda>]` | FAIL | pass | pass |
| `test_shell_set_unknown_version[...<lambda>]` | FAIL | FAIL | FAIL |
| `test_shell_unset_unaffected[...<lambda>]` | FAIL | pass | pass |
| `test_shell_set_many_versions[...<lambda>]` | FAIL | pass | pass |
| `test_shell_set_many_versions_one_not_installed[...<lambda>]` | FAIL | FAIL | FAIL |
| `test_shell_many_versions_defined` | FAIL | FAIL | FAIL |
| Total | 9 fail, 0 pass | 6 fail, 3 pass | 6 fail, 3 pass |

Grand total of the shell file against rpyenv today: **21 fail, 6 pass** per arch session (27 ids), which is 42 xfailed and 12 passed over both.

### Failure output

Under `--runxfail` the failures are (stdout, stderr) pairs:

- **cmd, all nine ids.** stdout is `pyenv: no such command 'shell'`, stderr is `""` (exit 1). Examples (pytest `E` lines):

  ```
  assert ("pyenv: no s... 'shell'", '') == ('Usage: pyen... --unset', '')      test_shell_help
  assert ("pyenv: no s... 'shell'", '') == ('no shell-sp...nfigured', '')      test_no_shell_version
  assert ("pyenv: no s... 'shell'", '') == ('3.7.7 3.8.9', '')                 test_shell_set_many_versions
  ```

  For the script-driven ids (`set_installed_version`, `unset_unaffected`, `set_many_versions`) the output is the same line: `call pyenv`
  resolves to `pyenv.exe` (`.EXE` before `.BAT`) [probe ov_cmd_version, which printed `pyenv 3.1.1 (rpyenv 0.1.0)` and then
  `pyenv: no such command 'shell'`].
- **powershell and pwsh, the six direct ids.** stdout is `""` and the pytest message is
  `assert ('', 'C:\\tmp...undException') == (<expected>, '')`. The full stderr (5.1) is:

  ```
  C:\...\pyenv : The term 'C:\...\pyenv' is not recognized as the name of a cmdlet, function, script file, or operable program. ...
  At line:1 char:1
  + C:\...\pyenv dir with ...
      + CategoryInfo          : ObjectNotFound: (...:String) [], CommandNotFoundException
  ```

  pwsh prints `... is not recognized as a name of a cmdlet, function, script file, or executable program.` with ANSI colour codes.
  The command line is `powershell -Command C:\...\pyenv dir with spaces\bin\pyenv.exe shell ...`. PowerShell splits the unescaped path at its first
  space, so **`pyenv.exe` is never started** [run trace; probe in `overlay_trace_run.txt`].
- **Experiment, in the scratch checkout only.** With the backtick escaping restored in `pyenv_file` (appended to `pw_overlay\tests\conftest.py`, never to
  the repo), the six direct powershell ids still fail, now with stdout `pyenv: no such command 'shell'` and stderr `""`, the same text as cmd
  [run, experiment on `pw_overlay`; `--runxfail -k "AMD64 and test_pyenv_feature_shell and powershell"`: 6 failed, 3 passed]. The same
  experiment was not repeated for `pwsh` (**UNCONFIRMED**, but the path handling is identical in the logged command lines).

### Cross-check with `parity\expected\pyenv-win.txt`

The file has 21 rows tagged `M3 shell integration (`pyenv shell`)`, and they are exactly the 21 failing ids above:

- 9 for cmd: `test_no_shell_version[cmd]`, `test_shell_help[cmd]`, `test_shell_many_versions_defined[cmd]`, `test_shell_version_defined[cmd]`, and
  `test_shell_set_installed_version[cmd-<lambda>]`, `test_shell_set_many_versions[cmd-<lambda>]`,
  `test_shell_set_many_versions_one_not_installed[cmd-<lambda>]`, `test_shell_set_unknown_version[cmd-<lambda>]`, `test_shell_unset_unaffected[cmd-<lambda>]`.
- 6 for powershell and 6 for pwsh: `test_no_shell_version`, `test_shell_help`, `test_shell_many_versions_defined`, `test_shell_version_defined`,
  `test_shell_set_many_versions_one_not_installed[...-<lambda>]`, `test_shell_set_unknown_version[...-<lambda>]`.

(Compared by listing `grep -n M3 parity/expected/pyenv-win.txt` against the `FAILED` lines of the runxfail run: 21 and 21, with no id on one side only. No row is stale.)

### Why the other six already pass

The six passing ids are `test_shell_set_installed_version`, `test_shell_unset_unaffected` and `test_shell_set_many_versions`, each in `powershell` and
`pwsh`. All three write a `tmp.ps1` and run `pyenv shell ...` or `pyenv global --unset` **by bare name**.

- PowerShell resolves bare `pyenv` to `bin\pyenv.ps1` first, ahead of `pyenv.exe`, in an overlay-shaped root (`pyenv.ps1`, `pyenv.exe`, `pyenv.bat`, `pyenv`)
  [probe ov_ps_resolve, ov_pwsh_resolve]. `pyenv_setup` put that `pyenv.ps1` there, whatever rpyenv does.
- `pyenv.ps1 shell <v>...` is handled inside the script: cscript runs `<root>\libexec\pyenv.vbs`, which `pyenv_setup` also copied, and
  `$Env:PYENV_VERSION` is set. `pyenv shell` with no arguments, `pyenv global --unset` and `pyenv local --unset` go through
  `pyenv.bat` to pyenv-win's own scripts. So no rpyenv code runs in these six. Proof: in the `ov` root, `pyenv --version` in
  PowerShell printed `pyenv ` (pyenv-win's batch, with no `.version` file in that scratch root), while `pyenv.exe --version` printed
  `pyenv 3.1.1 (rpyenv 0.1.0)`; and `pyenv global --unset` returned 0 [probe ov_ps_resolve].
- If `pyenv.ps1` and `pyenv.bat` were absent from the root (`ov2`), PowerShell resolves bare `pyenv` to `pyenv.exe`, and
  `pyenv shell 3.7.7` prints `pyenv: no such command 'shell'`, exit 1, leaving `PYENV_VERSION` empty [probe ov2_ps]. The same happens in cmd [probe ov2_cmd].
- The three **cmd** script tests fail instead because bare `pyenv` in cmd resolves to `pyenv.exe`, not `pyenv.bat` (above).
- The six direct powershell/pwsh ids fail first for a reason unrelated to `shell`: the unescaped path in the overlay's `pyenv_file`.
  The script-driven ids are not affected by it, because the test escapes the script path itself [test shell.py:50, 74, 88].
- `PYENV_VERSION` is also an **input** to other commands that already exist in `pyenv.exe`, with their tests passing: `pyenv version`
  prints `(set by %PYENV_VERSION%)`, etc. The `shell` tests are the only ones that need a **producer**.

## Open points

1. **`pyenv shell --HELP`** (upper-case): the plugin tests `"%1" == "--help"` (case-sensitive) [src pyenv-shell.bat:9], so cmd
   would probably treat it as a version name and fail. Not probed. **UNCONFIRMED**.
2. **`pyenv SHELL 3.7.7`** (upper-case command): the exit code was 0 and the output empty in cmd and both PowerShells, but the
   resulting variable was not read back. **UNCONFIRMED**.
3. **Overlay powershell experiment for `pwsh`** was not run; the Windows PowerShell result is assumed to carry over. **UNCONFIRMED**.
4. **The `X86` session of the shell file with `--runxfail`** never ran against rpyenv, because the `arch` fixture skips it after any
   AMD64 failure. The strict-xfail run did run both sessions (12 passed, 42 xfailed), so the pass/fail split is confirmed for X86, but the
   individual failure texts for X86 were not captured.
5. **A zip (LF) install of pyenv-win** was probed only for Git Bash (`pw_lf`). The cmd and PowerShell probes ran on the CRLF checkout. M2 noted the same.
6. **Windows PowerShell with the default (`Restricted`) execution policy on a clean machine**, and a `pyenv.ps1` marked as downloaded
   (zone identifier) under `RemoteSigned`, were not probed. Only a process-scope `Restricted` was, so the exact `RemoteSigned` message is **UNCONFIRMED**.
7. **Non-ASCII values** in `PYENV_VERSION` through `pyenv shell` (code page 65001 for cmd, UTF-8 setup in the ps1) were not probed.
8. **Real versions.** The roots had empty folders for versions. Whether `GetBinDir` or anything else in `shell` reads the folder's contents
   (for example `python.exe`) was not tested with a real install; the source reads only `FolderExists` [src libs\pyenv-lib.vbs:265].
9. **The user's `pwsh` profile** ran during every `pwsh` probe and test (the suite does not pass `-NoProfile`); its content was not read, so any influence
   it has beyond the absence of stray output is **UNCONFIRMED**. A host without a profile may differ in nothing observed here.
10. **`pyenv shell` under a Windows Terminal or ConEmu AutoRun** (the `\x0c` filter in `do_run` and the `skip=` logic in `pyenv.bat`) was not exercised; no
    AutoRun value exists on the probe host.
