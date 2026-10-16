# M7c: Anaconda, Miniconda and Miniforge on Windows — design

Date: 2026-10-16. Status: agreed in brainstorming, awaiting review of this file.
Parent spec: `docs/specs/2026-09-27-rpyenv-design.md` (§5.1, §9, §14 M7, §15.1).
Pattern: M7b's spec, `docs/superpowers/specs/2026-10-15-m7b-conda-linux-design.md`
(merged, PR #26), whose D1 split conda by OS and left Windows to this spec.

## 1. Goal

On Windows x64 (and Windows ARM64 under emulation), `pyenv install` installs Anaconda,
Miniconda and Miniforge from their official installers, under the names upstream pyenv
uses on Linux, each download checked against the publisher's SHA-256; conda Pythons
get the `PATH` they need to load their DLLs; and only Python-related conda executables
get shims.

Success means:

- `pyenv install miniforge3-26.7.2-0`, `miniconda3-3.13-25.9.1-3`,
  `anaconda3-2026.07-1` (and `miniconda3-latest`, `miniforge3-latest`) install and run.
- A `.python-version` that names a conda install means the same release on Linux and
  Windows.
- `install --list` shows the conda names this machine can install.
- `conda create -n <env>` through the `conda` shim, then `pyenv shell <install>/envs/<env>`
  or `pyenv activate <install>/envs/<env>`, runs the env's Python.
- `cygpath`, `curl` and the other non-Python tools inside a conda install get no shim.
- `pyenv update` refreshes the conda table from the publishers, without an rpyenv release.

pyenv-win has no conda support (M7b §3: 0 matches), so every conda behavior on Windows is
an rpyenv extension, recorded in the parity allowlist.

## 2. Decisions (user, 2026-10-16)

| # | Question | Decision |
|---|---|---|
| D1 | Which names Windows offers | Upstream's Linux names (the x86_64 rows of `crates/pyenv/conda/linux.tsv`), kept where a Windows x64 installer with a publisher SHA-256 exists, plus newer releases `pyenv update` finds (§4.3). Chosen over every installer the publishers list under new names, and over current releases only. |
| D2 | Where URLs and hashes come from | A CI-built table, as M7a's Windows PyPy table: hashes read from the publishers (repo.anaconda.com's SHA256 column, Miniforge's `.sha256` files) and pinned at PR review. Chosen over embedding names only and reading hashes at install time (nothing pinned), and over reading the publishers on every `install --list` (network, page-layout breakage on users' machines). |
| D3 | How the table reaches users (user, after D2) | **A file, shipped and updatable:** the CI-built table is packaged with each release and installed with it; `pyenv update` writes a newer copy into the pyenv root, as it already rewrites `.versions_cache.xml`. Not embedded in the binary. |
| D4 | Where `pyenv update` gets a newer table | **Rebuilt from the publishers on the user's machine**, by the same rules CI uses. Every hash still comes from the publisher (parent §15.1: rpyenv is never the hash authority). Chosen over downloading the CI-built file from the rpyenv repo (makes the repo a hash source) and over shipping only (new conda releases wait for an rpyenv release). |
| D5 | Windows ARM64 hosts | As M7a D7: x64 rows are listed and installable; they run under emulation. No publisher ships a Windows ARM64 conda installer (§3). |
| D6 | `-latest` names | As M7b D2: the hash is read at install time from the publisher (`repo.anaconda.com/miniconda/` SHA256 column; the latest Miniforge release's versioned `.sha256`). A name with no such source is refused. |
| D7 | Linux | Unchanged. Its table comes from upstream's definitions, which update with each rpyenv release, as upstream's do. |
| D8 | How a reviewed hash change reaches users (user, after a side-agent note) | **Reviewed rows win; maintainers accept explicitly; `update` warns.** For a name the shipped table has as `ok`/`ok-latest`, its row is used, whatever the root copy says; the root copy only adds names (or replaces a shipped row that isn't installable). A maintainer accepts a publisher's changed hash with `conda_windows_table --accept <name>` in a reviewed PR, and the next release fixes every machine. `pyenv update` reports a changed hash as a warning and exits 0 (the user can't act; a release can). Chosen over an accepted-changes list file (still needs a release to reach users) and over the earlier "newer copy wins", under which a root copy re-stamped by every `update` would outrank every later release, so a corrected hash could never arrive. |

## 3. Measurements this design rests on (2026-10-16)

| Fact | Method | Result |
|---|---|---|
| Windows architectures published | `curl` of `repo.anaconda.com/miniconda/` and `/archive/`, `gh api` of Miniforge's latest release | Miniconda: 472 `Windows-x86_64` and 188 `Windows-x86` mentions, no ARM64. Anaconda: x86_64 and x86, no ARM64. Miniforge 26.7.2-0: `Miniforge3-26.7.2-0-Windows-x86_64.exe` with `.sha256`, and unversioned `Miniforge3-Windows-x86_64.exe` without. |
| Hash columns | `curl` of both listing pages | Header `Filename / Size / Last Modified / SHA256`; every Windows row carries a 64-hex SHA-256, including releases upstream's Linux definitions hash with MD5 only (e.g. `Anaconda3-2022.10-Windows-x86_64.exe`). |
| Coverage of Linux's names | `C:\tmp\m7c_probe.py` (live sources): each x86_64 name of `linux.tsv` joined with the Windows installers, named backwards (§4.3) | Of 445 names: 428 have a Windows x64 installer with a published SHA-256; 15 have no file under the backwards name; 2 (`mambaforge-pypy3`, `miniforge-pypy3`) have no published hash. 40 names Linux refuses (MD5 only) have a SHA-256 on Windows. |
| Backwards naming against upstream | Same script: every Linux `ok` row's file name, named backwards, compared with the row's name | 380 of 388 match. The 8 that differ: 3 old names that source their `*2` twin (`anaconda-2.4.0` → `Anaconda2-2.4.0`), 4 point re-releases (`anaconda3-5.0.0` → `Anaconda3-5.0.0.1`), and `miniconda3-3.9-24.1.2-0`, whose upstream definition points at the `py312` installer. |
| Names the publishers have that Linux lacks | Same script | 120, mostly older Miniforge builds (`miniforge3-4.9.2-1` … `-7`), and one non-release, `miniforge3-uninstaller-patch`. |
| ToS rule for names `update` adds | Python over `linux.tsv`'s x86_64 rows: `miniconda3`, conda release 25 or later, or `miniconda3-latest`, against rows carrying `CONDA_PLUGINS_AUTO_ACCEPT_TOS=true` | 75 and 75, no name in one set only. |
| Where the MSI installs | `installer/rpyenv.wxs` | `[ProgramFiles6432Folder]\rpyenv\` with `bin\` and `completions\`; the pyenv root (versions, `.versions_cache.xml`) is elsewhere. |

The probe joins by backwards naming only; §4.2's URL swap recovers the twins and
re-releases, so plan Task 2 reports the final per-family counts from the real builder.

## 4. The Windows table

### 4.1 Format

`windows.tsv`, UTF-8, LF, in `linux.tsv`'s line grammar (`<name>\t<arch>\t<verdict>`,
read by the same parser) with the arch `Windows-x86_64` (amended 2026-10-16, while
planning: one parser for both tables):

```
# generated 2026-10-16T09:30:00Z by ci
# <name>\t<arch>\t<verdict>
miniforge3-26.7.2-0	Windows-x86_64	ok Miniforge3-26.7.2-0-Windows-x86_64 https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe#<sha256> miniconda verify_py3
miniconda3-latest	Windows-x86_64	ok-latest Miniconda3-latest-Windows-x86_64 https://repo.anaconda.com/miniconda/Miniconda3-latest-Windows-x86_64.exe miniconda verify_py3_latest CONDA_PLUGINS_AUTO_ACCEPT_TOS=true
anaconda3-2021.04	Windows-x86_64	ok Anaconda3-2021.04-Windows-x86_64 https://repo.anaconda.com/archive/Anaconda3-2021.04-Windows-x86_64.exe#<sha256> anaconda verify_py3
mambaforge-pypy3	Windows-x86_64	unsupported no published SHA-256
anaconda-1.5.1	Windows-x86_64	no-binary Anaconda
```

- Verdicts as Linux's: `ok`, `ok-latest`, `no-binary`, `unsupported <reason>`.
- `ok`/`ok-latest` carry Linux's fields: package (the installer's file stem), URL, step,
  verify, then exported variables. The step (`anaconda` or `miniconda`) picks the
  post-install step (§5); it is copied from the Linux row, and upstream gives Miniforge
  `miniconda`. The verify field is kept for the grammar; Windows checks `python.exe`
  instead (§5 step 6). A trailing `CONDA_PLUGINS_AUTO_ACCEPT_TOS=true`, as in `linux.tsv`,
  marks the rows whose installer gets that variable (§5 step 4).
- The first line's `generated` time and source (`ci` or `update`) say where a copy came
  from; they don't decide which copy is read (§4.4, D8).

### 4.2 Built by CI

The builder is Rust (`install/condacatalog.rs`), shared with `pyenv update` (§4.3), run
in CI as `cargo run --example conda_windows_table` by the sync job's existing "Rebuild
the conda table" step, so one PR carries both tables (amended 2026-10-16, while
planning: one implementation of the page readers and naming rules instead of a Python
and a Rust copy):

1. Read the publishers once: both repo.anaconda.com listing pages (file → SHA-256), and
   Miniforge's GitHub releases (asset names, and the text of each Windows `.sha256`).
2. For each x86_64 row of `linux.tsv`:
   - **The row has a URL** (`ok`, `ok-latest`): replace `-Linux-x86_64.sh` with
     `-Windows-x86_64.exe` in it. `repo.continuum.io` URLs are rewritten to
     `repo.anaconda.com`, which serves the same folders.
   - **No URL** (`unsupported`, `no-binary`): name the publishers' Windows installers
     backwards (§4.3) and take the one whose name equals the row's.
3. Verdict: the file exists with a 64-hex SHA-256 → `ok`; it exists without one →
   `unsupported no published SHA-256`; no file → `no-binary`. `ok-latest` rows stay
   `ok-latest` when the unversioned file exists.
4. The trailing `CONDA_PLUGINS_AUTO_ACCEPT_TOS=true` is copied from the Linux row (75
   x86_64 `miniconda3` rows carry it, measured with `grep` on `linux.tsv`).
5. Fail the job, naming the rows, if a Linux `ok` row ends `no-binary` on Windows and the
   previous `windows.tsv` had it `ok` (a file disappeared or a page changed layout).
6. **A changed hash is never taken by CI either** (added 2026-10-16 after a side-agent
   note: without it, "pinned at review" held only if a reviewer spotted one changed line
   in a weekly PR). A row whose publisher hash differs from the previous `windows.tsv`
   keeps the previous hash; the rest of the table is written, so new releases still
   arrive; the PR body names each such row under "changed hash, kept the pinned one";
   and the job fails. The same function (`condacatalog::pin_hashes`) applies §4.3 step 1's
   rule in `pyenv update`.
7. **Accepting a change (D8):** `conda_windows_table --accept <name>[,<name>…]` takes the
   publisher's current hash for those rows instead of pinning them. A maintainer runs it
   locally after checking why the file changed (release notes, the publisher's
   announcement, the installer's Authenticode signature) and opens a PR whose diff shows
   the old and new hash; the commit message says why. That is the only documented way a
   pinned hash changes.

### 4.3 Rebuilt by `pyenv update`

On Windows, after the CPython cache, `pyenv update` rebuilds the conda table with the
same reads as §4.2 step 1, by the same module (`install/condacatalog.rs`):

1. Start from the table as read (§4.4: reviewed rows win). Re-read every `ok` row's hash
   from the publisher; a hash that differs is **not** taken: `update` keeps the old hash
   and prints `:: [Warning] :: conda: the publisher changed <names> after rpyenv pinned
   them; they won't install until an rpyenv release carries the reviewed hash`, and exits
   0 (D8: the user can't act on it; a release can).
2. Add releases newer than the newest row of their family, named backwards:
   - `Miniconda3-py<X><Y>_<release>-Windows-x86_64.exe` → `miniconda3-<X>.<Y>-<release>`
   - `<Family>-<release>-Windows-x86_64.exe` → `<family lowercased>-<release>`
   - Only when `<release>` matches the family's release shape (`\d+(\.\d+)+(-\d+)?`);
     anything else (`uninstaller-patch`) is ignored.
   - New `miniconda3` rows whose conda release is 25 or later get the variable, the rule that
     reproduces Linux's 75 (§8, tested).
3. Write `<root>\.conda_versions.tsv` atomically (temp file, rename) with
   `# generated <now> by update`.
4. A page that can't be fetched or parsed: `update` names the source, keeps the previous
   file and exits 1. The CPython cache it already wrote stays.

The README says what D6 of M7b says for `-latest`: a hash added by `update` comes from
the publisher's page at that moment, so it catches a broken download, not a compromised
publisher; a shipped row's hash was pinned at PR review.

### 4.4 Where the copies live and which one is read

- **Shipped:** `[INSTALLFOLDER]\share\conda\windows.tsv`, a new MSI component. Found
  relative to the running `pyenv.exe` (`<exe dir>\..\share\conda\windows.tsv`).
- **Updated:** `<root>\.conda_versions.tsv`, beside `.versions_cache.xml`, so an MSI
  repair or upgrade never touches it.
- **Read by** `install`, `install --list`, `latest -k`, merged by name (D8): every row
  of the shipped copy, then the updated copy's rows for names the shipped copy lacks;
  for a name in both, the shipped row wins when it is `ok`/`ok-latest`, else the
  updated row is used. A copy that doesn't parse is ignored with a warning naming it.
- **A download that fails its hash** for a conda row adds one line to the usual
  mismatch error: `if the publisher replaced this installer, a later rpyenv release will
  carry the reviewed hash`.
- **Neither present** (a bare `cargo build`): conda names are refused with
  `rpyenv has no conda version table; run pyenv update`, and `install --list` shows no
  conda lines. Tests point at a table with `RPYENV_TEST_CONDA_TABLES` (M7b's variable,
  which then names a folder holding `windows.tsv`).

## 5. Install flow (`install/winconda.rs`)

Called where `winpkg::install_pypy` is, for names `conda::is_conda_name` accepts:

1. **Resolve** the name in the table (§4.4). `no-binary` and `unsupported` are refused
   with Linux's texts and its newest-installable hint (`conda::newest_installable`, over
   the Windows rows). `ok-latest` reads its hash now (D6).
2. **Check the target** `<root>\versions\<name>`: refuse, naming the character, paths
   the installers reject. The character set per family is measured in plan Task 1 (§8),
   not guessed.
3. **Download** into the cache and check the SHA-256; a mismatch is refused as for every
   other download.
4. **Run the installer in place:**
   `<file>.exe /S /InstallationType=JustMe /AddToPath=0 /RegisterPython=0 /NoRegistry=1 /NoShortcuts=1 /D=<target>`
   (parent §15.1 resolution 2; `/D=` last and unquoted, as NSIS requires).
   - Into the final folder, through `txn.rs`'s conda mode: the completion marker is
     written last, and a folder without it counts as not installed (M7b §4.3).
   - Rows carrying the variable: it is set for the installer and step 5, after
     M7b's one-line terms notice.
   - Ctrl+C: rpyenv ends the installer process, waits for it, and deletes the folder.
5. **Post-install, rows whose step is `miniconda`** (Miniconda and Miniforge, as
   upstream), as upstream's `build_package_miniconda`:
   `Scripts\conda.exe install --yes pip conda=<installed conda version>`.
6. **Verify:** `python.exe -c "import sys, ssl"` with §6's `PATH`; a DLL that can't load
   fails the install here.
7. **Finish:** marker, then rehash.

On failure at steps 4 to 6 the folder is deleted, the installer's output is kept in the
install log, and the error names the step. `install -f` carries only `envs\` and names
what it left, as M7b D7. `uninstall` deletes the folder: the flags register nothing.

## 6. Runtime

### 6.1 What counts as conda

A version or env folder with `conda-meta\` (`venv::is_conda`). Envs are
`<install>/envs/<name>`, already accepted by version names and `pyenv versions`.
`venv.rs`'s Linux spellings get Windows ones: `bin/conda` → `Scripts\conda.exe`, and
`conda` run in an env resolves to its install's `Scripts\conda.exe`.

**A bare family name** (`pyenv install miniforge3`, `pyenv latest -k miniconda3`)
resolves to the family's newest installable row, ranked as the M7b refusal hint ranks
(`conda::newest_installable`: conda release, then Python). pyenv-win's resolver only
reads CPython codes, so without this the most common spelling would fail (added
2026-10-16, while planning).

### 6.2 The child's `PATH`

`win_child_path` adds, for a conda tree, conda's activation order in place of
`root; Scripts; bin`:

```
<root>; <root>\Library\mingw-w64\bin; <root>\Library\usr\bin; <root>\Library\bin; <root>\Scripts; <root>\bin
```

For installs and envs alike (each env has its own `Library\bin`). The child's
environment only, never the caller's (parent §15.1 point 2).

`pyenv activate <install>/envs/<name>` sets `PYENV_VERSION`, `VIRTUAL_ENV` and
`CONDA_DEFAULT_ENV`, as on Linux; it never touches `PATH` (pyenv-virtualenv doesn't), so
the shims' launch plan supplies the six folders. What Windows needs is
`venv::base_prefix` to recognize conda trees: an install with `Scripts\conda.exe` is its
own base, an env (`conda-meta\`, no `Scripts\conda.exe`) has `<env>\..\..` as base
(amended 2026-10-16, while planning: the earlier sentence said `activate` sets `PATH`;
it doesn't, on either OS).

### 6.3 Which executables get shims

The parent spec's rule (§15.1 resolution 3), new module `rpyenv-core/src/conda_owner.rs`:

- Read `conda-meta\*.json` once per conda version; map each `files` entry to its package.
- Folders searched: the root, `Scripts`, `Library\bin`.
- Shim when the owner is `python`, `conda`, or a package with files under
  `site-packages`; or nothing owns it and it is in `Scripts` (where pip writes).
- Never: names starting with `.`, `activate`/`deactivate`, and names on upstream's
  `default.list` (`conda_list.rs`, the second filter).
- Shims come from versions, not envs, as upstream and Linux.

The shim's own lookup (`lookup::which_win_runnable`) and `pyenv which` also search
`Library\bin` of a conda tree, so every shim the rule makes finds its file.

### 6.4 Not supported (README)

`conda activate` (needs conda's shell hook); `activate.d` scripts (some packages, e.g.
GDAL, set variables there); `conda install` from a selected env targets the base unless
given `-n`, as upstream, which sets no `CONDA_PREFIX`.

## 7. Packaging

- `installer/rpyenv.wxs`: `share\conda` under `INSTALLFOLDER` with one file component,
  sourced from `$(RepoDir)\crates\pyenv\conda\windows.tsv` as the MSI already sources
  `README.md` (`build.yml` unchanged).
- The Linux tarball is unchanged (D7).
- `crates/pyenv/conda/windows.tsv` is committed, so a release built from a tag ships the
  table that PR review saw.

## 8. Testing

**Task 1, measured before any code (Windows Sandbox, never the development machine):**

- Registry and file snapshot before and after silent installs of Miniforge, Miniconda
  and Anaconda with §5's flags: no new HKCU/HKLM keys outside the install, no Start-menu
  entries, no `PATH` change.
- Which target-path characters each installer refuses silently, and its exit code.
- Whether `/S` blocks until the install finishes.
- The ownership rule on the three installs: shim and skip counts, and no `default.list`
  name gets a shim. Each install's `conda-meta` is saved as a test fixture.

**Unit tests** (Rust), each able to fail:

- URL swap: Linux row → Windows URL; `repo.continuum.io` rewritten; missing file →
  `no-binary`.
- Backwards naming over every Linux `ok` row: the 380 matches stay matches, and the 8
  known differences (§3) are listed by name in the test; a new difference fails it.
- The ToS rule (`miniconda3`, conda 25 or later, or `-latest`) picks exactly Linux's 75 `miniconda3` names.
- Copy merge (D8): a shipped `ok` row wins over the updated copy's row for the same name
  even when the updated copy is newer; the updated copy adds names and replaces shipped
  rows that aren't installable; neither copy → the refusal text.
- `update`: saved copies of both listing pages and a `.sha256`; a changed layout fails
  and keeps the old file; a changed hash on an `ok` row warns, exits 0 and keeps the old
  hash; `--accept` takes the new one;
  `uninstaller-patch` is not added.
- The ownership rule against Task 1's fixtures.
- `win_child_path` order for a conda tree; lookup finds `Library\bin` files.

**CLI tests** with a fake installer that copies Task 1's measurements (the files a real
installer leaves, including an empty `envs\`, and its exit codes; M7b's lesson): hash
mismatch, refused path, installer failure deletes the folder, Ctrl+C, `install -f` keeps
`envs\`, an `unsupported` refusal with its hint.

**Real installs in CI** (`install-windows.yml`, x64): the newest numbered `miniforge3`
and `miniconda3` (the latter takes the ToS path), and `miniforge3-latest`. Each:
`conda create` an env through the shim, run its Python, `install -f`, the env survives;
`python`, `pip`, `conda` have shims, `cygpath` and `curl` don't. Anaconda (~1 GB) is not
installed in CI: its rows are checked by the table tests and Task 1 installs it once.

**Packaging:** `installer/test_msi.py` asserts `share\conda\windows.tsv` is in the MSI;
`installer/sandbox/run-checks.ps1` gains a check that `pyenv install --list` shows
conda names after the MSI installs. The scheduled sync job exercises `pyenv update`
against the live sites.

**Parity:** allowlist entries from D-110 for each Windows conda behavior;
`parity/coverage.py` stays at 100%.

## 9. Changes to other documents

- Parent §15.1 resolution 1: Windows conda superseded by this spec (D2-D4).
- Parent §14: M7c is this spec.
- README: Windows conda section, the trust note (§4.3), §6.4's limits.
