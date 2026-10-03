# pyenv-win behavioral reference: rpyenv M2 installer commands

This is rpyenv's parity target on Windows for `install`, `install --list`, `update`, `uninstall` and `latest`. The file
records facts only, and each fact carries an evidence tag. Every `file:line` path is relative to the root of the
pyenv-win repository at the pinned commit. When a path starts with `libexec\` or `bin\`, it is under `pyenv-win\`.

rpyenv's own design is already decided (spec §9.1): python.org Install Manager zips for 3.11+, per-component MSIs for
everything else, rpyenv's own catalog, and no registry writes. So this file records pyenv-win's **user-visible
contract**. Its download and install mechanism is summarized only so the plan can write allowlist rows for the
differences (see *Mechanism, for allowlist rows*).

## Source, version, and evidence method

- **Pinned upstream.** pyenv-win `856ed5a8c107879d53374f782a4b40cc794f19e0`, which is the commit the M1 reference also
  pins (`.version` is `3.1.1`). Code was read from git with `git -C C:/tmp/pyenv-win-856ed5a show HEAD:<path>`.
- **Probe copy.** A fresh `git clone` of that commit is at `C:\tmp\m2ref_win\pw`. It was checked out with
  `core.autocrlf=true`, so the `.bat`, `.vbs` and `.xml` files are CRLF on disk, like the M1 environment. In the index
  they are LF. An install from GitHub's zip would have LF files; that variant was not probed. pyenv-win derives every
  path from script locations, so the copy is self-contained: its root is `C:\tmp\m2ref_win\pw\pyenv-win`
  [src libexec\libs\pyenv-lib.vbs:52-61].
- **Probe host.** Windows 11 Pro 10.0.26200.9457, `PROCESSOR_ARCHITECTURE=AMD64`, OEM code page 850, on 2026-10-03.
- **Probe method.** `C:\tmp\m2ref_win\probe.py` runs `cmd /d /c call <root>\bin\pyenv.bat <args>` through Python
  `subprocess`, and captures stdout, stderr and the exit code as raw bytes. Each record is in
  `C:\tmp\m2ref_win\probes.jsonl`, keyed by the probe names this file cites.
  - Every probe runs in a console of its own (`CREATE_NO_WINDOW`), so each one starts at code page 850. A separate
    probe saw `Active code page: 850` before `pyenv.bat` and `Active code page: 65001` after it; the code page is not
    restored (`C:\tmp\m2ref_win\cp_probe.bat`).
  - Environment: only the standard system variables, plus `PYENV`, `PYENV_ROOT` and `PYENV_HOME` set to
    `<root>\`. `PATH` is `<root>\bin;<root>\shims;%SystemRoot%\system32;%SystemRoot%;…\Wbem;…\WindowsPowerShell\v1.0`.
  - The cwd is `C:\tmp\m2ref_win\work\local`. Its parent holds an empty `.python-version`, which stops the upward
    search, as in the pytest fixture. No global `version` file exists.
- **What was not executed.** No python.org installer, `msiexec` or `dark.exe` was run.
  - **Download-phase probes.** These ran with `http_proxy=https_proxy=http://127.0.0.1:9`. pyenv-win hands that value
    to WinHttp [src libexec\libs\pyenv-lib.vbs:13-40], so every request fails before any byte is saved. This guard was
    checked first on `update`, which then failed with `0x80072EFD` (probe `upd_deadproxy`). `install_cache` was empty
    or absent for every download probe, so no cached installer could run either.
  - **Registry.** The subkeys of `HKCU\SOFTWARE\Python\PythonCore` were `3.10 3.11 3.13 3.7 3.8`, before the probes and
    after them. The `uninstall` probes used the fake names `9.9.x`, so `unregister` only ever tried to delete keys
    that do not exist.

| Tag | Meaning |
|---|---|
| `[src f:l]` | Read from pyenv-win code at that line. Not executed. |
| `[test f:l]` | Asserted by pyenv-win's pytest suite (`tests\`). The suite was not run in this session. |
| `[probe <name>]` | Observed by running the pinned scripts in the scratch copy, as described above. `<name>` is the record in `probes.jsonl`. Isolated probes (a construct run in a standalone `.vbs`) say so inline. |
| `[disk]` | Read-only listing of the user's real pyenv-win install at `C:\Users\JM\.pyenv`, whose versions were installed by an earlier pyenv-win release of unknown version. Nothing there was executed or changed. |
| **UNCONFIRMED** | None of the above. **UNCONFIRMED (needs a real install)** marks facts that only a real `msiexec` run could show. |

The pytest output normalization (keep the text after the last `\x0c`, strip CR/LF from both ends, never assert exit
codes) is described in `pyenv-win-m1-reference.md` → *How the pyenv-win test suite observes output*. It applies here
unchanged.

## Conventions for these commands

These are the M1 conventions (`pyenv-win-m1-reference.md` → *Conventions shared by all commands*), restated where they
matter for M2:

- **Streams.** Every message goes to **stdout**, including errors. stderr carries only VBScript runtime errors, in
  the form `<full .vbs path>(<line>, <col>) <source>: <description>\r\n\r\n` [probe `upd_real`, `un_all_eof`].
- **Line endings.** Every line ends in CRLF; no probe produced a bare LF [probe: every record].
- **Exit codes.** These come from `WScript.Quit n`. A script that runs to its end exits 0, and so does one that
  aborts on a VBScript runtime error. pyenv.bat passes the code on [src bin\pyenv.bat:168, 219-220].
- **The mirror banner.** `pyenv-install.vbs` and `pyenv-update.vbs` print one line per mirror as soon as the script
  loads, before they parse arguments [src libexec\pyenv-install.vbs:19-22; libexec\pyenv-update.vbs:19-22]. So
  **every** `install` and `update` invocation starts with these lines, including `--help`, `--list`, and every error
  path:

  ```
  :: [Info] ::  Mirror: https://www.python.org/ftp/python
  :: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json
  :: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases
  ```

  There are **two** spaces after `::`. If `PYTHON_BUILD_MIRROR_URL` is non-empty, the banner is that one line,
  `:: [Info] ::  Mirror: <value>` [src libexec\libs\pyenv-install-lib.vbs:6-14; probe `list_mirror`, `dl_mirror`].
  `uninstall` and `latest` print no banner. This file writes `<banner>` for those three lines.
- **Routing.** `install`, `uninstall`, `update` and `latest` are not in pyenv.bat's built-in list. They reach
  `:plugin`, which runs `cscript //nologo "<root>\libexec\pyenv-<cmd>.vbs" <rest>` [src bin\pyenv.bat:51, 131-169].
  - `pyenv install --help` and `pyenv help install` produce identical bytes, because both run
    `pyenv-install.vbs --help` [src bin\pyenv.bat:39, 44-48; probe `install_help`, `help_install`].
  - As for every command, pyenv.bat first runs `pyenv.vbs vname` twice, and that creates `<root>\versions` if it is
    missing (M1 reference → *pyenv.bat routing order*, step 4).
- **Paths** [src libexec\libs\pyenv-lib.vbs:53-60]:

  | Item | Path |
  |---|---|
  | Root | parent of `libexec` |
  | Versions | `<root>\versions\<code>` |
  | Download cache | `<root>\install_cache\<file>` |
  | Version DB | `<root>\.versions_cache.xml` |
  | WiX `dark.exe` | `<root>\bin\WiX\dark.exe` |

  `PYENV`, `PYENV_ROOT` and `PYENV_HOME` are not read by these commands.
- **Environment variables read:**

  | Variable | Read at | Effect |
  |---|---|---|
  | `PYTHON_BUILD_MIRROR_URL` | pyenv-install-lib.vbs:8 | Replaces the 3-mirror list. It is printed in the banner and used by `update`; `install` downloads from the URL stored in the DB, so the mirror does **not** change download URLs [probe `dl_mirror`]. |
  | `http_proxy`, then `https_proxy` | pyenv-lib.vbs:18-20 | WinHttp proxy (`setProxy 2`), after removing the `http://`/`https://` prefix, a trailing `/`, and anything up to `@`. |
  | `PYENV_FORCE_ARCH` | pyenv-lib.vbs:432, 444 | Overrides the host arch: `AMD64`, `X86`, `ARM64`, case-insensitive. |
  | `PROCESSOR_ARCHITECTURE` | pyenv-lib.vbs:433, 445 (**System** env); pyenv-install.vbs:218-220 (**Process** env) | Host arch. `--register` reads the process value, so a 32-bit cscript sees `x86`. |
  | `PYENV_VERSION` | pyenv-lib.vbs:144 | The version used by `pyenv install` with no version argument. |

## install

### Help text [src libexec\pyenv-install.vbs:24-44; probe `install_help`]

`pyenv install --help` prints `<banner>`, then the following, and exits 0. It is 20 CRLF lines and 1397 bytes in
total, banner included, and the last line is empty:

```
Usage: pyenv install [-s] [-f] <version> [<version> ...] [-r|--register]
       pyenv install [-f] [--32only|--64only] -a|--all
       pyenv install [-f] -c|--clear
       pyenv install -l|--list

  -l/--list              List all available versions
  -a/--all               Installs all known version from the local version DB cache
  -c/--clear             Removes downloaded installers from the cache to free space
  -f/--force             Install even if the version appears to be installed already
  -s/--skip-existing     Skip the installation if the version appears to be installed already
  -r/--register          Register version for py launcher
  -q/--quiet             Install using /quiet. This does not show the UI nor does it prompt for inputs
  --32only               Installs only 32bit Python using -a/--all switch, no effect on 32-bit windows.
  --64only               Installs only 64bit Python using -a/--all switch, no effect on 32-bit windows.
  --dev                  Installs precompiled standard libraries, debug symbols, and debug binaries (only applies to web installer).
  --help                 Help, list of options allowed on pyenv install

```

### Options [src libexec\pyenv-install.vbs:378-401]

Arguments are scanned left to right with a case-sensitive `Select Case`. Options and versions can be mixed in any
order.

| Argument | Variable | What it actually does |
|---|---|---|
| `--help` | — | Prints the help and exits 0 **as soon as it is reached**. Version arguments before it have already been resolved, which reads the DB. |
| `-l`, `--list` | `optList` | Lists the DB (see *install --list*). Version arguments are resolved but otherwise ignored [probe `list_withver`]. |
| `-f`, `--force` | `optForce` | Before each install, deletes `versions\<code>` and `install_cache\<file>` [src :524; pyenv-install-lib.vbs:142-149]. With `-c`, it is passed to `Delete` as the *force* flag. |
| `-s`, `--skip-existing` | `optSkip` | **No effect.** It is set and never read. An existing version is always skipped (see *Already installed*). |
| `-q`, `--quiet` | `optQuiet` | **No effect.** It is stored in the parameter array (`IP_Quiet`) and never read [src :521; the only reader is commented out at :335]. |
| `--dev` | `optDev` | **No effect**, for the same reason (`IP_Dev`) [src :522, 336]. |
| `-a`, `--all` | `optAll` | Installs every DB entry (see *--all*). |
| `-c`, `--clear` | `optClear` | Empties `install_cache` (see *--clear*). |
| `--32only`, `--64only` | `opt32`, `opt64` | Filters `--all` by the DB's `x64` attribute. Both are reset to False when the arch is X86 [src :402-405]. Without `-a`, their only effect is the error checks below. |
| `-r`, `--register` | `optReg` | Writes PEP 514 keys after each successful install (see *Registry*). |
| anything else | — | A version: `installVersions.Item(TryResolveVersion(arg, True))`. |

- **Not options.** `-32`, `-64`, `-L`, `--version` and so on become version strings, which then fail the DB check
  with *definition not found* [probe `inst_dash32`, `inst_upperL`].
- **Duplicates.** Versions are the keys of a `Scripting.Dictionary`, so a version that resolves twice installs once,
  in the order first seen.

### Order of checks and their messages [src libexec\pyenv-install.vbs:402-501]

Each check below runs only if the previous ones passed. Every message is preceded by `<banner>`.

| # | Condition | Output after the banner | Exit |
|---|---|---|---|
| 1 | `--32only` and `--64only`, with the arch not X86 | `pyenv-install: only --32only or --64only may be specified, not both.` | 1 [probe `inst_both`] |
| 2 | `-r` with `--32only` (even without `-a`) | `pyenv-install: --register not supported for 32 bits.` | 1 [probe `inst_reg32`] |
| 3 | `-r` with `-a` | `pyenv-install: --register not supported for all versions.` | 1 [probe `inst_regall`] |
| 4 | `.versions_cache.xml` missing, or with no `<version>` | three lines: `pyenv-install: no definitions in local database`, an empty line, ``Please update the local database cache with `pyenv update'.`` (the quote is a backtick, then an apostrophe) | 1 [probe `nocache_install`, `nocache_list`] |
| 4' | DB fails schema validation | `Validation error in DB cache(0x<HEX>) on line <n>, pos <m>:`, then MSXML's reason text, which ends in its own CRLF, so an empty line follows | 1 [src pyenv-install-lib.vbs:203-208; probe `badcache_list`, which printed `0xC00CE012`, `line 0, pos 0`, and a two-line reason] |
| 5 | `-l` | the list | 0 |
| 6 | `-c` | — | see *--clear* |
| 7 | no version, `-a` not given, and no current version from `PYENV_VERSION`, `.python-version` or the global `version` file | the help text | **0** [probe `inst_noargs`, `inst_flagsonly`] |
| 8 | any requested version not a DB key (case-sensitive, exact) | four lines: `pyenv-install: definition not found: <version>`, an empty line, ``See all available versions with `pyenv install --list`.``, ``Does the list seem out of date? Update it using `pyenv update`.`` | 1 [probe `inst_unknown`] |

- **Check 8 runs over all versions before anything installs.** `pyenv install 3.12.1 9.9.9` reports only `9.9.9` and
  installs nothing [probe `inst_two_one_bad`]. The `<version>` in the message is the **resolved** string, or the
  argument itself when nothing resolved.
- **Check 4 does not cover `latest -k`.** There a missing DB yields no candidates, which is *no known versions*
  [probe `nocache_latestk`].

### Version spellings and resolution

Each version argument goes through `TryResolveVersion(arg, known=True)`, which is `FindLatestVersion` against the DB
codes [src libexec\libs\pyenv-install-lib.vbs:447-516]. The algorithm is in `pyenv-win-m1-reference.md` → *Prefix
resolution*. In short:

- A candidate qualifies if it starts with the argument and either equals `argument + arch suffix` or continues with
  `.`.
- Its name must match `^(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:([a-z]+)(\d*))?([\.-](?:amd64|arm64|win32))?$`.
- It must have no pre-release part, and its arch group must equal the suffix: `""` on AMD64, `-win32` on X86, and
  `-arm64` on ARM64.
- The newest qualifying candidate wins. If none qualifies, the argument is used unchanged and must then be an exact DB
  key.

Observed resolutions, from the version shown in the download line (the dead-proxy probes) or from `latest -k`:

| Argument | Arch | Resolves to | Evidence |
|---|---|---|---|
| `3.12.1` | AMD64 | `3.12.1` (file `python-3.12.1-amd64.exe`) | probe `dl_3121_amd` |
| `3.12` | AMD64 | `3.12.10` | probe `dl_312` |
| `3` | AMD64 | `3.14.2` (the DB also has `3.15.0a5`, which is skipped as a pre-release) | probe `dl_3` |
| `3.12.1` | X86 | `3.12.1-win32` (file `python-3.12.1.exe`) | probe `dl_x86` |
| `3.12.1-win32` | AMD64 or X86 | `3.12.1-win32`, used literally | probe `dl_win32name`, `dl_x86_3` |
| `3.12.1-arm` | any | `3.12.1-arm`, used literally (file `python-3.12.1-arm64.exe`) | probe `dl_armname` |
| `3.12.1-arm64` | AMD64 | not found: *definition not found: 3.12.1-arm64* | probe `inst_arm64name` |
| `3.12.1` | **ARM64** | `3.12.1`, the **AMD64** build (`python-3.12.1-amd64.exe`) | probe `dl_arm` |
| `3.12` | **ARM64** | not found: *definition not found: 3.12* | probe `dl_arm312` |
| `3.15.0a5` | AMD64 | `3.15.0a5`, used literally (pre-releases install only by exact name) | probe `dl_pre` |
| `2.7.18` | AMD64 | `2.7.18` (`python-2.7.18.amd64.msi`) | probe `dl_2718` |
| `pypy3.10-v7.3.19-win64` | AMD64 | itself (`.zip`) | probe `dl_pypy` |
| `3.12:latest` | AMD64 | not found: *definition not found: 3.12:latest*. There is no `:latest` syntax. | probe `inst_latestsyntax` |

- **ARM64 hosts.** The DB names ARM builds `-arm`, but `GetArchPostfix` expects `-arm64`
  [src pyenv-lib.vbs:437; pyenv-install-lib.vbs:88-89]. A bare version on ARM64 therefore resolves to nothing, falls
  back to the literal argument, and installs the AMD64 build if that exact code exists. Prefixes never resolve on
  ARM64.
- **No `-win32` mapping for explicit versions.** `Check32Bit` is used only by `--all`. On X86, `3.12.1` reaches
  `3.12.1-win32` only through prefix resolution [src :399, 466].
- **No resolution ties in this DB.** Resolution would hit a runtime error on two qualifying codes with equal
  major.minor.patch (see the M1 reference), and a mechanical check of all 901 codes found no such pair. The
  two-component codes (`2.5`, `3.2`, …) have no `x.y.0` twin [probe: Python grouping of the cache codes].

### No version argument [src libexec\pyenv-install.vbs:480-489; libexec\libs\pyenv-lib.vbs:139-191]

- If no version argument is given and `-a` is absent, `GetCurrentVersionNoError()` is used. It checks
  `PYENV_VERSION` (split on spaces), then the first `.python-version` found walking up from the cwd, then
  `<root>\version`.
- **Only the first version of that source is installed**, after `TryResolveVersion(…, True)`.
  - With `.python-version` = `9.9.8\r\n9.9.7\r\n`, the output is *definition not found: 9.9.8* [probe
    `inst_noargs_local`].
  - With `PYENV_VERSION="9.9.6 9.9.5"`, it is *definition not found: 9.9.6* [probe `inst_noargs_env`].
- If nothing is set, the help text is printed and the exit code is **0** [probe `inst_noargs`].

### Already installed, `-f`, and partial installs [src libexec\pyenv-install.vbs:279-349, 524]

- **The skip.** `extract` returns **silently** if `versions\<code>` exists, whatever is inside it
  [src :297]. Without `-f` the run then prints only `<banner>`, runs `Rehash`, and exits **0**. No "already
  installed" message exists [probe `inst_already`, with a fake `versions\3.12.1` holding an empty `python.exe`].
  - `-s`, `-q` and `--dev` change nothing [probe `inst_already_s`].
  - On X86, the same `3.12.1` resolves to `3.12.1-win32`, which was not present, so a download started
    [probe `inst_already_x86`].
- **`-f`** deletes `versions\<code>` and the cached installer **before** the download. If the download then fails,
  the old version is gone and nothing replaces it [probe `inst_force`: `versions\3.12.1` was deleted, then
  `:: [ERROR] ::`, exit 1].
  - `-f` does **not** delete the extracted-MSI folder `install_cache\<code>`. A forced reinstall of an `.exe`-type
    version reuses the MSIs from that folder [src :88, 524; pyenv-install-lib.vbs:142-149].
- **Partial installs.** A failed `msiexec`/`dark.exe`/ensurepip step leaves `versions\<code>` in whatever state it
  reached. Nothing removes it. A later `pyenv install <code>` then skips it silently [src :297, 341-348].
  UNCONFIRMED (needs a real install).

### Output of an install

Per version, in order [src libexec\pyenv-install.vbs:69-75, 297-348; probe `dl_*` for the download lines]:

```
<banner>
:: [Downloading] ::  3.12.1 ...
:: [Downloading] ::  From https://www.python.org/ftp/python/3.12.1/python-3.12.1-amd64.exe
:: [Downloading] ::  To   C:\tmp\m2ref_win\pw\pyenv-win\install_cache\python-3.12.1-amd64.exe
:: [Installing] ::  3.12.1 ...
:: [Info] :: completed! 3.12.1
```

- Spacing: two spaces after `::` in `[Downloading]` and `[Installing]`; **one** in `[Info] :: completed!` and in
  `[Error] ::`; three spaces after `To` [src :71-73, 301, 342, 347].
- The `[Downloading]` lines are printed only when `install_cache\<file>` is absent. When the file is present it is
  used as is, with no hash or size check [src :299].
- `[Installing]` and `completed!` are [src] only. **UNCONFIRMED (needs a real install).**

Failure lines and exit codes:

| Failure | Printed | Exit | Continues with next version? |
|---|---|---|---|
| WinHttp `Open`/`Send` error | `:: [ERROR] :: <Err.Description>`; the system text ends in its own CRLF, so an empty line follows [probe `dl_3121_amd`: `A connection with the server could not be established`] | 1 | no, `WScript.Quit 1` [src pyenv-install-lib.vbs:115-125] |
| HTTP status ≠ 200 | `:: [ERROR] :: <status> :: <statusText>` | 1 | no [src pyenv-install-lib.vbs:128-131] |
| Web layout failed | `:: [Error] :: error extracting the web portion from the installer.` then `:: [Error] :: couldn't install <code>` | **0** | yes [src :90-93, 347] |
| `dark.exe -x` failed | `:: [Error] :: error extracting the embedded portion from the installer.` + `couldn't install` | **0** | yes [src :96-99] |
| moving the MSIs failed | `:: [Error] :: error moving the extracted embedded portion from the installer.` + `couldn't install` | **0** | yes [src :101-104] |
| a component `msiexec /a` failed | `:: [Error] :: error installing "<msi base name, lower-case>" component MSI.` + `couldn't install` | **0** | yes [src :132-135] |
| ensurepip failed | `:: [Error] :: error installing pip.` + `couldn't install` | **0** | yes [src :145-148, 322-323] |
| single-`.msi` `msiexec /a` failed | `:: [Error] :: couldn't install <code>` | **0** | yes [src :313, 347] |

- **Exit status of a failed install.** After any `[Error]` the loop continues, `Rehash` runs, and the script ends with
  **exit 0**. Only download failures and the pre-checks exit 1.
- **Several versions, download failure.** With `pyenv install 3.11.0 3.12.1`, the first download error stops
  everything: `3.12.1` is never attempted [probe `dl_two`].

### Exit-code summary for `install`

| Case | Exit |
|---|---|
| success, already installed, `--list`, `--help`, no version and none selected | 0 |
| installer, MSI or ensurepip failure | 0 [src] |
| pre-check errors (table above), download errors | 1 |
| `--clear` with any delete error | 1 |
| VBScript runtime error, for example `CopyFile` when `python.exe` is missing after extraction | 0, with the message on stderr [src; M1 *Conventions*] |

### Rehash after install [src libexec\pyenv-install.vbs:529; libexec\libs\pyenv-lib.vbs:375-427]

- `Rehash` runs once after the install loop. It runs even when every version was skipped as already installed, and
  even after `[Error]` failures. It does not run after `--list`, `--clear`, `--help`, or any `WScript.Quit` path.
- It deletes **every file** in `shims` and regenerates the shims from all installed versions. The rules are in the M1
  reference → *rehash*.
- [probe `inst_already`]: with only the fake `3.12.1` (`python.exe`, `Scripts\pip.exe`), `shims` afterwards held
  `pip`, `pip.bat`, `python` and `python.bat`.

### Mechanism, by package type [src libexec\pyenv-install.vbs:77-209, 279-339]

The DB row decides the path: `msi="true"`, else `webInstall="true"`, else a `.zip` file, else an embedded-payload
`.exe`.

| DB row | Codes in this DB | Install steps |
|---|---|---|
| `msi="true"` | 2.4 – 3.4.4 (229 rows) | `msiexec /quiet /a "<file>" TargetDir="<versions\code>"` (window style 9, wait). On exit 0: delete `*.msi` from the target root, then run `"<target>\python" -E -s -m ensurepip -U --default-pip` if `Lib\ensurepip` exists. No `pythonX*.exe` copies. |
| `.exe`, `webInstall="false"` | 3.5.0a1 and later CPython (518 rows) | `deepExtract(web=False)`, below. |
| `webInstall="true"` | none in this DB | `"<file>" /quiet /layout "<install_cache\code>-webinstall"`, then the same steps as deepExtract. |
| `.zip` (`zipRootDir`) | 48 `pypy…-win64`, 14 `graalpy…-windows-amd64` | `Shell.Application` `CopyHere` (flag 4, no progress dialog) into `versions\`, then `MoveFolder versions\<zipRootDir> → versions\<code>`. No ensurepip, no copies. Every zip row in this DB has `<code>` equal to `<zipRootDir>`, so that move's source and destination are the same path; what FSO does then is UNCONFIRMED (needs a real install). |

`deepExtract` [src :77-180]:

1. **Extract**, unless `install_cache\<code>` already exists:
   `"<root>\bin\WiX\dark.exe" -x "<install_cache\code>" "<installer>"`, then
   `cmd /D /C move "<cache>"\AttachedContainer\*.msi "<cache>"`.
2. **Filter.** Delete every file in `install_cache\<code>` that is not `.msi`, plus `appendpath.msi`, `launcher.msi`,
   `path.msi` and **`pip.msi`**. Delete every subfolder.
3. **Install components.** For each remaining MSI, in FSO enumeration order, run
   `msiexec /quiet /a "<msi>" TargetDir="<versions\code>"` (hidden, wait). Then delete the copy of that `.msi` that
   the administrative install leaves in the target root.
4. **pip.** If `Lib\ensurepip` exists, run `"<target>\python" -E -s -m ensurepip -U --default-pip` (hidden, wait).
   Its output is not shown.
5. **Extra executables.** Copy `python.exe` to `pythonX.exe`, `pythonXY.exe` and `pythonX.Y.exe`, and `pythonw.exe` to
   `pythonwX.exe`, `pythonwXY.exe` and `pythonwX.Y.exe`. X and Y are the first two dot-fields of the **code** (for
   example `3.12.1-win32` gives `3`, `312`, `3.12`).
6. **venv launcher copies.** If `Lib\venv\scripts\nt\python.exe` exists, copy it to `python{X,XY,X.Y}.exe` **and** to
   `pythonw{X,XY,X.Y}.exe` in the same folder. The `pythonw*` copies are copies of `python.exe`, not of
   `pythonw.exe`.

Other mechanism facts:

- **No integrity check.** Nothing verifies a hash, signature or size, for the download or for a cached file
  [src :299; pyenv-install-lib.vbs:111-140].
- **Download.** A synchronous WinHttp `GET`, saved whole with `ADODB.Stream`. No progress is printed.
- **The cache persists.** Installers stay in `install_cache\` and extracted MSIs in `install_cache\<code>\`. For
  example, the real install holds `install_cache\3.14.2\{core,dev,doc,exe,lib,tcltk,test}.msi` and
  `install_cache\python-3.14.2-amd64.exe` [disk].
- **No product registration.** `msiexec /a` is an administrative install, which registers no product. What it leaves
  in the registry is UNCONFIRMED (needs a real install).

### Resulting layout of `versions\<code>`

[disk]: the real install's `3.14.2` (AMD64) and `3.10.11`. These were installed by an earlier pyenv-win, so this
layout is consistent with the code above but is not proven to come from 856ed5a.

- **Root files.** `python.exe`, `python3.exe`, `python314.exe`, `python3.14.exe`, `pythonw.exe`, `pythonw3.exe`,
  `pythonw314.exe`, `pythonw3.14.exe`, `python3.dll`, `python314.dll`, `vcruntime140.dll`, `vcruntime140_1.dll`,
  `LICENSE.txt` and `NEWS.txt`. The four `python*.exe` have the same size, as do the four `pythonw*.exe`.
- **Folders.** `DLLs`, `Doc`, `include`, `Lib`, `libs`, `Scripts` and `tcl`, plus `Tools` in 3.10.11. `Lib\test` is
  present because `test.msi` is installed. `etc` and `share` come from later pip installs.
- **Missing.** No `py.exe` launcher, and no PATH changes (`launcher`, `path` and `appendpath` are filtered out).
- **venv launchers.**
  - 3.10.11: `Lib\venv\scripts\nt` holds `python.exe`, `python3.exe`, `python310.exe`, `python3.10.exe`, the same
    four `pythonw*`, `activate.bat` and `deactivate.bat`.
  - 3.14.2: that folder holds `venvlauncher.exe`, `venvwlauncher.exe`, `activate.bat` and `deactivate.bat`, and no
    copies, because `Lib\venv\scripts\nt\python.exe` does not exist [disk; src :172].
- **pip.** `Scripts\pip*.exe` come from ensurepip, because `pip.msi` is discarded [src :118, 144-145].
- **File version.** `python.exe` reports `3.14.2150.1013` and `3.10.11150.1013` [disk: PowerShell `FileVersionRaw`].
  `--register` derives its values from this format.
- Layout of an X86 (`-win32`), ARM (`-arm`), single-`.msi` (≤ 3.4) or zip (PyPy/GraalPy) install: UNCONFIRMED (needs
  a real install).

### Registry: `-r` / `--register` [src libexec\pyenv-install.vbs:211-277, 343-345]

PEP 514 keys are written only with `-r`, only after a successful install, and only for versions this run installed.
A version that already existed was skipped and is not registered.

- **When it does nothing.**
  - If the **process** `PROCESSOR_ARCHITECTURE` is exactly `x86` (a 32-bit cscript), it prints
    `Python registration not supported in 32 bits` and writes nothing. `PYENV_FORCE_ARCH=X86` does not trigger this.
  - If the code contains `pypy`, it prints `Registering pypy versions is not supported yet`. GraalPy is **not**
    excluded.
- **Values.**
  - `fileVersion = GetFileVersion("<target>\python.exe")`.
  - `sysVersion = "<f0>.<f1>"`, and `featureVersion = "<f0>.<f1>.<f2>.0"`; for 3.14.2 that is `3.14.2150.0`.
  - If the code contains `-win32`: `bitDepth=32` and `Version` = the code without `-win32`. Otherwise `bitDepth=64` and
    `Version` = the code. That includes `-arm`, which is registered as 64-bit with `Version` `3.12.1-arm`.
- **Keys and values written**, all `REG_SZ`, with `K = HKCU\SOFTWARE\Python\PythonCore\<code>\`. The tag is the full
  code, for example `3.12.1` or `3.12.1-win32`, not PEP 514's usual `3.12` / `3.12-32`.

  | Value | Data |
  |---|---|
  | `K\DisplayName` | `Python <sysVersion> (<bitDepth>-bit)` |
  | `K\SupportUrl` | `https://github.com/pyenv-win/pyenv-win/issues` |
  | `K\SysArchitecture` | `<bitDepth>bit` |
  | `K\SysVersion` | `<sysVersion>` |
  | `K\Version` | as above |
  | `K\InstalledFeatures\{dev,exe,lib,pip,tools}` | `<featureVersion>` |
  | `K\InstallPath\` (default value) | `<target>\` |
  | `K\InstallPath\ExecutablePath` | `<target>\python.exe` |
  | `K\InstallPath\WindowedExecutablePath` | `<target>\pythonw.exe` |
  | `K\PythonPath\` (default value) | `<target>\Lib\;<target>\DLLs\` |

  The `PythonCore` key itself gets no `DisplayName` or `SupportUrl`; those writes are commented out [src :250-254].
- Nothing is printed on success.
- Whether the py launcher lists these tags: UNCONFIRMED (needs a real install).

### `-a` / `--all` [src libexec\pyenv-install.vbs:461-478]

This mode was never executed. It would install about 900 versions.

- It replaces any version arguments with every DB key `v` for which `Check32Bit(v)` is still a DB key. On X86, each
  key gets `-win32` appended unless it already ends in it.
- `--64only` keeps rows with `x64="true"`, which includes the `-arm`, PyPy and GraalPy rows. `--32only` keeps
  `x64="false"`.
- Without a filter on AMD64, it installs every row: win32, arm, amd64, PyPy and GraalPy.
- Pre-releases are included.

### `-c` / `--clear` [src libexec\pyenv-install.vbs:436-459]

- It deletes every file, then every subfolder, of `install_cache`, and exits with `delError` (0 or 1). Per failure it
  prints `pyenv: Error (<n>) deleting file <name>: <desc>` or `… deleting folder …`.
- It runs only after the DB loaded successfully (check 4).
- [probe `inst_clear`]: a file and a subfolder were removed. Output was `<banner>` only, with exit 0.
- [probe `inst_clear_nocache`]: with no `install_cache` folder, the output was `<banner>` only, with exit **1**. The
  `On Error Resume Next` path hides the message, because building it raises a second error.

## install --list

[src libexec\pyenv-install.vbs:421-435; libexec\libs\pyenv-install-lib.vbs:178-233; test
tests\test_pyenv_feature_install.py:7-34]

- **Output.** `<banner>`, then every `<code>` of `<root>\.versions_cache.xml`, one per line with CRLF, in **document
  order**. Exit 0, and stderr is empty.
  - [probe `list_l`]: 3 + 901 lines, 11 741 bytes. The lines after the banner equal the DB's `<code>` sequence
    exactly.
  - The first entries are `2.4-win32`, `2.4.1-win32`, `2.4.2-win32`, `2.4.3c1-win32`, `2.4.3-win32`. The last is
    `graalpy-25.0.1-windows-amd64`.
- **No filtering, no sorting.** Every arch, pre-releases, PyPy and GraalPy are included. `PYENV_FORCE_ARCH=X86` and
  `ARM64` produce byte-identical output [probe `list_long`, `list_arm`].
- **Order in this DB:**
  - CPython ascending, with each version's pre-releases before the final release.
  - Within one version: `-win32`, then `-arm`, then the AMD64 name.
  - Then PyPy (48 rows), then GraalPy (14 rows).

  This is the order of the file that upstream's CI writes [src .github\scripts\update_versions_cache.py:267-316].
- **What the test asserts.** stderr is `""`. The output contains `Mirror: https://www.python.org/ftp/python`, contains
  the substrings `2.7.17-win32`, `2.7.17`, …, `3.9.1-win32`, `3.9.1`, and contains `graalpy` and `pypy`
  [test tests\test_pyenv_feature_install.py:7-34].
- **The DB file at this commit** [`.versions_cache.xml`, git]:
  - 901 `<version>` rows with unique codes.
  - AMD64 CPython: 371 (112 `.msi`, 259 `.exe`).
  - `-win32`: 376 (117 `.msi`, 259 `.exe`).
  - `-arm`: 92 (all `.exe`, from `3.11.0a5-arm`).
  - PyPy: 48; GraalPy: 14.
  - `webInstall="true"`: 0. No free-threaded (`t`) builds, because the file regex cannot match them
    [src pyenv-install-lib.vbs:67].

### Format of `.versions_cache.xml`

- **Schema** [src libexec\libs\pyenv-install-lib.vbs:152-175]: `<versions>` holds `<version x64= webInstall= msi=>`
  elements, each with `<code>`, `<file>`, `<URL>` and an optional `<zipRootDir>`. The attribute defaults are
  `x64=false`, `webInstall=false` and `msi=true`.
- **Header.** `<?xml version="1.0" encoding="utf-8" standalone="no"?>`, with tab indentation.
  - The committed file has LF endings and a final LF [git].
  - The VBScript writer (`SaveVersionsXML`) produces the same header and the same tab layout with **CRLF** endings
    and **no** final newline. [probe, isolated: a scratch `.vbs` that loaded both libs and called `SaveVersionsXML` on
    7 sample file names; the result was `C:\tmp\m2ref_win\save_out.xml`.]
- **How codes are built** [src pyenv-install-lib.vbs:66-70, 80-93, 266-279]. The input is the installer file name,
  matched by `^python-(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:([a-z]+)(\d*))?([\.-]amd64)?([\.-]arm64)?(-webinstall)?\.(exe|msi)$`.
  - The name becomes `major[.minor][.patch][pre][num]` with a suffix: `-arm` if an arm64 group matched, otherwise
    `-win32` if there was no amd64 group, otherwise nothing.
  - So `python-3.12.1-arm64.exe` gives `3.12.1-arm`, `python-3.12.1.exe` gives `3.12.1-win32`,
    `python-2.7.18.amd64.msi` gives `2.7.18`, and `python-3.12.0a1-amd64.exe` gives `3.12.0a1`.
  - Zip rows use the zip root name as the code.
  - `x64` is true for amd64 **or** arm64, `webInstall` for `-webinstall`, and `msi` for `.msi`.

## update

### Help [src libexec\pyenv-update.vbs:24-32; probe `update_help`]

`<banner>`, then the following, exit 0 (9 lines, 379 bytes, the last line empty):

```
Usage: pyenv update [--ignore]

  --ignore  Ignores any HTTP/VBScript errors that occur during downloads.

Updates the internal database of python installer URL's.

```

### Options [src libexec\pyenv-update.vbs:132-138]

- Only `arg(0)` is examined. `--help` prints the help; `--ignore` sets `optIgnore`. Anything else, and any second
  argument, is ignored without a message [probe `upd_bogus_arg` ran a normal update].

### What it scrapes [src libexec\pyenv-update.vbs:147-232]

1. **HTML mirrors.** For each mirror, it GETs the URL and writes the response into an MSHTML `htmlfile` object.
   - If the page has links (python.org's `/ftp/python/`), it follows every link whose last path segment matches
     `^(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:([a-z]+)(\d*))?$`.
   - On each such version page, it collects every link text that matches the installer-file regex above.
2. **JSON mirrors.** If the page has no links (the PyPy `versions.json` and the GitHub releases API), it regex-scans
   the body for `download_url": "https://…/(pypy<X.Y>-v<…>|graalpy-<…>)-(win64|windows-amd64)?(windows-aarch64)?.zip"`.
   Those become zip rows.
3. **Filtering.** Rows below 2.4 are dropped. A `-webinstall` row is dropped when the same file name without
   `-webinstall` also exists.
4. **Saving.** The rows are sorted with `SymanticQuickSort` and written with `SaveVersionsXML` to
   `<root>\.versions_cache.xml`, which **replaces the file whole**. Then it prints
   `:: [Info] ::  Scanned <pages> pages and found <n> installers.`.

### Failure behavior [src libexec\pyenv-update.vbs:95-105, 152-170]

- **Mirror request failed.** It prints the following two lines, plus the empty line that a system error text brings:

  ```
  HTTP Error downloading from mirror "<mirror>"
  Error(0x<HEX>): <description>
  ```

  For a non-200 status, the second line is `Error(<status>): <statusText>`.
  - Without `--ignore`: exit 1.
  - With `--ignore`: the whole update stops (`Exit Sub`), the DB is not written, and the exit code is **0**.
  - [probe `upd_deadproxy`, `upd_deadproxy_ignore`]: `Error(0x80072EFD): A connection with the server could not be
    established`, with exit 1 and exit 0 respectively.
- **Version page failed.** The same text says `mirror page`. Without `--ignore` it exits 1; with `--ignore` that page
  is skipped.
- **On this host, `pyenv update` cannot work at all.**
  - Even with a working network, the first `htmlfile.write` raises a runtime error. It prints `<banner>`, then on
    stderr `…\libexec\pyenv-update.vbs(172, 13) htmlfile: This command is not supported.\r\n\r\n`, and exits **0**.
  - The DB is left byte-identical [probe `upd_real`, `upd_ignore_real`; `cmp` against the committed file].
  - The same error occurs in a standalone `.vbs` calling `CreateObject("htmlfile").write`, under both the 64-bit and
    the 32-bit cscript [probe, isolated: `C:\tmp\m2ref_win\htmlprobe.vbs`].
  - Upstream replaced it in CI: `.github\scripts\update_versions_cache.py` regenerates the committed DB weekly
    [src .github\workflows\update_cache.yml:3-6, 27-29], and says "This replaces the VBScript pyenv-update.vbs that
    doesn't work in GitHub Actions" [src .github\scripts\update_versions_cache.py:2-5]. That script prints the same
    banner and summary lines.
  - On which Windows builds `htmlfile.write` still works: UNCONFIRMED.
- The success output (`Scanned … pages …`) and the row order that the VBScript sort produces for PyPy/GraalPy rows:
  UNCONFIRMED, because no update completed.

## uninstall

### Help [src libexec\pyenv-uninstall.vbs:18-31; probe `uninstall_help`, `uninstall_noargs`]

`pyenv uninstall --help` and `pyenv uninstall` with no arguments both print the following and exit **0**. There is no
banner; the output is 11 lines and 440 bytes, the last line empty:

```
Usage: pyenv uninstall [-f|--force] <version> [<version> ...]
       pyenv uninstall [-f|--force] [-a|--all]

   -f/--force  Attempt to remove the specified version without prompting
               for confirmation. If the version does not exist, do not
               display an error message.

   -a/--all    *Caution* Attempt to remove all installed versions.

See `pyenv versions` for a complete list of installed versions.

```

### Arguments [src libexec\pyenv-uninstall.vbs:56-70]

- `--help`, `-f`/`--force`, `-a`/`--all`; matching is case-sensitive.
- Anything else must pass `IsVersion` (`^[a-zA-Z_0-9-.]+$`). Otherwise the command prints
  `pyenv: Unrecognized python version: <arg>` and exits 1 immediately [probe `un_bad`, with `bad!name`].
  `--msi`, used by the stale `tests\bat_files\test_uninstall.bat`, passes `IsVersion` and is treated as a version
  name.
- **No prefix resolution.** Names are folder names, except that on X86 (`PYENV_FORCE_ARCH` or the system arch)
  `Check32Bit` appends `-win32` [src :104, 117].
  - [probe `un_x86`]: `uninstall 9.9.7` on X86 removed `9.9.7-win32`, and printed that name.
  - [probe `un_x86_amd`]: an explicit `-win32` name on AMD64 is used literally.

### Messages and exit codes [src libexec\pyenv-uninstall.vbs:72-137]

| Case | Output | Exit |
|---|---|---|
| `versions` has no subfolders | `pyenv: No valid versions of python installed.` | 1 [probe `un_empty`] |
| exactly one requested version, not installed | `pyenv: version '<name>' not installed` | **0** [probe `un_missing`] |
| the same with `-f` | the same message (the help text's promise is not implemented) | 0 [probe `un_missing_f`] |
| several versions, some missing | missing ones are skipped silently | per the rest [probe `un_two_one_missing`] |
| each folder deleted | `pyenv: Successfully uninstalled <name>` | — |
| `DeleteFolder` failed (for example a read-only file without `-f`) | `pyenv: Error (<n>) uninstalling version <name>: <desc>` | 1 [probe `un_ro`: `Error (70) … Permission denied`, the folder stayed] |
| the same with `-f` | the read-only file is deleted too (`DeleteFolder …, force`) | 0 [probe `un_ro_f`] |

- **Effects.** After each successful delete it calls `unregister <name>`. If no delete error occurred, `Rehash` runs;
  then the script exits with `delError` [src :128, 135-137].
  - [probe `un_one`]: shims were regenerated from the remaining versions.
- **No prompt for named versions.** `-f` only matters for read-only files and for `--all`.

### Registry cleanup and the stale-error defect [src libexec\pyenv-uninstall.vbs:33-42, 115-134]

- **What `unregister` does.** It calls `RegDelete` on `HKCU\SOFTWARE\Python\PythonCore\<name>\InstallPath\`, then
  `…\InstalledFeatures\`, `…\PythonPath\`, and `…\<name>\`. This runs for **every** uninstalled version, whether or
  not it was installed with `-r`.
- **The defect.** The comment says "No problem removing keys that do not exist", but `RegDelete` on a missing key
  raises. That error escapes `unregister` into the caller's `On Error Resume Next`, which leaves `Err` set.
  - The next version's `DeleteFolder` succeeds, but `Err.Number <> 0` is still true, so the command reports an error
    for a version it **did** delete.
  - It then skips that version's `unregister`, sets `delError = 1`, skips `Rehash`, and exits 1.
- **Observed** [probe `un_two`: `uninstall 9.9.4 9.9.5 9.9.6`, none registered; all three folders were deleted]:

  ```
  pyenv: Successfully uninstalled 9.9.4
  pyenv: Error (-2147024894) uninstalling version 9.9.5: Invalid root in registry key "HKCU\SOFTWARE\Python\PythonCore\9.9.4\InstallPath\".
  pyenv: Successfully uninstalled 9.9.6
  ```

  The exit code was 1. The pattern alternates (OK, error, OK, …), and the error names the **previous** version's key.
- **When it does not show.** A single unregistered version shows nothing wrong (probe `un_one`, exit 0). Versions
  registered with `-r` should not trigger it, because every key exists; that case is [src] only.

### `-a` / `--all` [src libexec\pyenv-uninstall.vbs:82-101]

- **The prompt.** Without `-f` it prompts on stdout with no newline: `pyenv: Confirm uninstall all? (Y/N): `. It
  reads a line and takes the lower-cased first character after trimming.
  - `y` proceeds.
  - `n` exits 0 silently [probe `un_all_n`].
  - An empty line exits 0 silently [probe `un_all_empty`].
  - Any other answer prompts again [probe `un_all_retry`: `x`, then `No`, gave two prompts and exit 0].
  - At EOF, stdout shows the prompt and stderr shows `…\pyenv-uninstall.vbs(90, 17) Microsoft VBScript runtime
    error: Input past end of file`; the exit code is 0 [probe `un_all_eof`].
- **What it removes.** Every subfolder of `versions` whose name passes `IsVersion`. `bad dir` was kept
  [probe `un_all_y`].
  - The stale-error defect applies here too: `un_all_y` with `9.9.3` and `9.9.8` printed `Successfully uninstalled
    9.9.3`, then the error line for `9.9.8`, and exited 1.
- `-a -f` skips the prompt [probe `un_all_f`].

## latest

`pyenv latest` is described fully in `pyenv-win-m1-reference.md` → *latest* and → *Prefix resolution*. Its
`parity/expected` rows say it arrives in M2. This section adds the `-k` (known-DB) behavior that `install` shares.

- **`-k` reads the DB.** It uses `.versions_cache.xml` with the same algorithm `install` uses. With no DB, every
  prefix gives *no known versions match* [probe `nocache_latestk`].
- **Observed `latest -k`** [probe `latk_*`]:

  | Prefix | AMD64 | X86 | ARM64 |
  |---|---|---|---|
  | `3` | `3.14.2` | `3.14.2-win32` | no match, exit 1 |
  | `3.12` | `3.12.10` | `3.12.10-win32` | no match |
  | `3.12.1` | `3.12.1` | `3.12.1-win32` | no match |
  | `3.13` | `3.13.11` | — | — |
  | `2` | `2.7.18` | — | — |
  | `2.5` | `2.5.4` | — | — |
  | `3.0` | `3.0.1` | — | — |
  | `2.4` | — | `2.4.4-win32` | — |
  | `3.12.1-win32` | no match | no match | — |
  | `3.12.1-arm` | no match | — | no match |
  | `pypy`, `graalpy` | no match | — | — |

  Each result is one line plus CRLF, with exit 0. A miss prints
  `pyenv-latest: no known versions match the prefix '<p>'.` and exits 1.
- **Suffixed names.** An arch-suffixed full name never "resolves to itself" under `latest -k`, although `install`
  accepts it literally (see the table under *Version spellings*). This is because `latest` has no
  fall-back-to-argument step.

## Arch and naming

- **Version directory names.** These are the DB codes, verbatim [src libexec\pyenv-install.vbs:519].

  | Build | Name |
  |---|---|
  | AMD64 | `3.12.1` |
  | 32-bit | `3.12.1-win32` |
  | ARM64 | `3.12.1-arm` |
  | Pre-release | `3.15.0a5`, `3.15.0a5-win32`, `3.15.0a5-arm` |
  | PyPy | `pypy3.10-v7.3.19-win64` |
  | GraalPy | `graalpy-25.0.1-windows-amd64` |

  No DB code ends in `-arm64` or `-amd64`, although the resolution regex recognizes `-amd64`, `-arm64` and `-win32`
  suffixes [src pyenv-install-lib.vbs:62, 88-89]. `Check32Bit` knows only `-win32` [src pyenv-lib.vbs:450-455].
- **Host arch.** `PYENV_FORCE_ARCH`, else the **system** `PROCESSOR_ARCHITECTURE`
  [src libexec\libs\pyenv-lib.vbs:429-447]. The effects:

  | Command | Effect of the arch |
  |---|---|
  | `install <prefix>` | The suffix that resolution requires: AMD64 `""`, X86 `-win32`, ARM64 `-arm64` (never matches; see above). |
  | `install <exact code>` | None. Any arch's code installs on any host [probe `dl_armname`, `dl_win32name`]. |
  | `install -a` | X86 maps every key to its `-win32` twin; `--32only`/`--64only` are ignored on X86. |
  | `install -l` | None [probe `list_long`, `list_arm`]. |
  | `install -r` | Uses the **process** `PROCESSOR_ARCHITECTURE`, not `PYENV_FORCE_ARCH`. |
  | `uninstall <v>` | X86 appends `-win32` (`Check32Bit`). |
  | `latest` | The same suffix rule as `install <prefix>`. |

- **Documentation.** The changelog states that `pyenv install 2.7.17` installs 64-bit on x64 and 32-bit on x86, and
  that `-win32` selects 32-bit on 64-bit hosts [src docs\changelog.md:91-92].

## Mechanism, for allowlist rows

Facts only: each pyenv-win fact is tagged in the sections above, and the rpyenv column is quoted from spec §9.1.

| Aspect | pyenv-win 856ed5a | rpyenv (spec §9.1) |
|---|---|---|
| Source of versions | `.versions_cache.xml` in the root, committed and refreshed by upstream CI; local `pyenv update` is broken on this host | rpyenv's own catalog, refreshed by `pyenv update` |
| Packages | `.exe` with `dark.exe -x` and per-component `msiexec /a` (3.5+); single `.msi` with `msiexec /a` (≤ 3.4); `.zip` for PyPy/GraalPy | Install Manager zips for 3.11+; component MSIs read in Rust for the rest |
| Components | all embedded MSIs except `appendpath`, `launcher`, `path` and `pip`; pip via ensurepip | — |
| Integrity | none | SHA-256 (index) or MD5 (MSIs) |
| Registry | PEP 514 keys only with `-r`; `uninstall` always tries to delete them | none |
| Atomicity | none; partial folders persist and are later skipped | `.tmp-<ver>` and a rename |
| Output | the banner, `[Downloading]`, `[Installing]`, `completed!`, `[Error]` | — |
| `:latest` | not supported | supported (§9.3) |
| Cache | `install_cache\` kept forever; `-c` clears it | — |

## pyenv-win's tests for these commands

| File | Test | What it needs |
|---|---|---|
| tests\test_pyenv_feature_install.py:7-34 | `test_check_pyenv_install_list` | `pyenv install -l`: stderr `""`, and stdout containing the `Mirror: https://www.python.org/ftp/python` banner text, the listed `X.Y.Z` and `X.Y.Z-win32` codes for 2.7.17 and 3.1.4 through 3.9.1, `graalpy` and `pypy`. |
| tests\test_pyenv_feature_install.py:37-39 | `test_check_pyenv_installation` | An empty placeholder (`pass`); it always passes. |
| tests\test_pyenv_feature_install.py:42-51 | `test_patched_venv_module[3.9.13-python39]`, `[3.10.11-python310]`, `[3.11.3-python311]` | A **real** install over the network of `Native(version)`, with `check=True`. Then `pyenv rehash`, `pyenv global <v>`, `pyenv exec python3X -m venv <tmp>\venv`, and `<venv>\Scripts\pip.exe --version` with stderr `""`. Skipped when the session arch ≠ `PROCESSOR_ARCHITECTURE`, so on an AMD64 runner only the AMD64 session runs it. It exercises the `Lib\venv\scripts\nt\pythonXY.exe` copies (install step 6); that this is the reason the test exists is inferred from its name and the code comment. |
| tests\test_pyenv_feature_latest.py:5-54 | `test_latest_help`, `test_latest_edge_cases`, `test_latest_arch_cases`, `test_latest_quiet`, `test_latest_sort` | `latest` against fake installed versions; `-k` only for the "no known versions" messages (`-k 1`, `-k 3.2.16`). |
| tests\test_pyenv_feature_uninstall.py | — | An **empty file** (0 bytes) [git ls-tree]. No uninstall test exists. |
| tests\test_pyenv.py:15-32 | `test_check_pyenv_features_list` | `install` and `uninstall` appear in `pyenv` help output (M1 territory). |
| tests\bat_files\test_install.bat, test_uninstall.bat | — | Manual scripts, not collected by pytest. They run real installs of 3.5.2, 2.7.15, 3.7.2, 3.9.0 and 3.10.0, and use `uninstall --msi` (not an option; see *uninstall*). |
| — | — | **No test covers `pyenv update`.** |

- **Mocking.** There is none. No test mocks downloads, installers, WinHttp or the registry
  [src: grep of `tests\` for `install`, `update`, `latest` and `versions_cache`].
  - The only installer-touching test (`test_patched_venv_module`) downloads and installs for real.
  - The suite's fake versions are empty files created by `pyenv_setup` [src tests\test_pyenv_helpers.py:41-101].
  - `pyenv_setup` copies the real `.versions_cache.xml`, `libexec\*`, `libexec\libs\*.vbs` and `bin\WiX\*` into each
    test's root [src tests\test_pyenv_helpers.py:55-72].
- **Running them against rpyenv through the overlay** (`parity/pyenv_win_overlay.py`, `parity/pyenv_win_run.py`). The
  overlay replaces the `pyenv_file` fixture with `bin\pyenv.exe` and copies rpyenv's executables into each test's
  `bin`. pyenv-win's setup still builds the root, so `<root>\.versions_cache.xml` (pyenv-win's DB) is present.
  - `test_check_pyenv_install_list` can run as is. It passes only if `pyenv.exe install -l` prints the literal text
    `Mirror: https://www.python.org/ftp/python` somewhere on stdout, prints those version names, and writes nothing
    to stderr. rpyenv's catalog needs `-win32` names and PyPy/GraalPy entries for that.
  - `test_patched_venv_module` can run only with network access, and performs three real installs per AMD64 session.
    It needs `pyenv.exe install` to exit 0, and the venv launcher copies (or an equivalent) for 3.9-3.11.
  - The `latest` tests run without network or catalog dependence, apart from `-k` returning no match for `1` and
    `3.2.16`.
- **Current xfail entries** [`parity/expected/pyenv-win.txt:14-22`]:

  ```
  test_pyenv_feature_install.py::test_check_pyenv_install_list | M2 installer
  test_pyenv_feature_install.py::test_patched_venv_module[3.10.11-python310] | M2 installer
  test_pyenv_feature_install.py::test_patched_venv_module[3.11.3-python311] | M2 installer
  test_pyenv_feature_install.py::test_patched_venv_module[3.9.13-python39] | M2 installer
  test_pyenv_feature_latest.py::test_latest_arch_cases[<lambda>] | M2 `pyenv latest` arrives with the installer
  test_pyenv_feature_latest.py::test_latest_edge_cases[<lambda>] | M2 `pyenv latest` arrives with the installer
  test_pyenv_feature_latest.py::test_latest_help | M2 `pyenv latest` arrives with the installer
  test_pyenv_feature_latest.py::test_latest_quiet | M2 `pyenv latest` arrives with the installer
  test_pyenv_feature_latest.py::test_latest_sort[<lambda>] | M2 `pyenv latest` arrives with the installer
  ```

  There are no entries for uninstall or update, since no such tests exist.

## UNCONFIRMED

Needs a real install:

- The `[Installing]` and `completed!` lines, as actually printed.
- The final layout of `-win32`, `-arm`, `≤ 3.4 .msi` and zip installs.
- Whether `msiexec /a` leaves any registry or Installer state.
- What `-r` produces in practice, and whether the py launcher lists pyenv-win's tags.
- Partial-install leftovers after an `msiexec` failure.
- Whether PyPy/GraalPy installs succeed at all, given the same-path `MoveFolder`.

Not established by any method used here:

- On which Windows builds `pyenv update`'s `htmlfile.write` works.
- The success output and row order of a VBScript-written DB.
- The behavior of an LF-checkout (GitHub zip) install.
- Non-English error descriptions.
