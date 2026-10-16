# M7c: Anaconda, Miniconda and Miniforge on Windows — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `pyenv install miniforge3-…`, `miniconda3-…`, `anaconda3-…` work in the Windows rpyenv: names from upstream's Linux table, publisher SHA-256s in a shipped and updatable table, silent installs in place, the conda `PATH` for children, and ownership-based shims.

**Architecture:** One Rust module (`install/condacatalog.rs`) builds the Windows table from the publishers' pages; CI runs it through an example binary and commits `crates/pyenv/conda/windows.tsv`, the MSI installs that file, and `pyenv update` rebuilds a newer copy into the pyenv root. `install/winconda.rs` runs the NSIS installer into `versions\<name>` through `txn.rs`'s existing conda mode. `rpyenv-core` learns conda trees on Windows: the six activation folders on the child's `PATH`, `Library\bin` in lookups, and a package-ownership shim filter (`conda_owner.rs`).

**Tech Stack:** Rust (workspace crates `pyenv`, `rpyenv-core`), `serde_json` (already a dependency), WiX (`installer/rpyenv.wxs`), PowerShell (Windows Sandbox via `wsb`), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-16-m7c-conda-windows-design.md` (approved 2026-10-16, amended the same day while planning; read §2 decisions and §4-§6 before any task).

## Global Constraints

- Windows table arch string: `Windows-x86_64`. Row grammar: exactly `linux.tsv`'s (`<name>\t<arch>\t<verdict>`), parsed by `conda::parse_verdict`.
- First line of every Windows table: `# generated <YYYY-MM-DDTHH:MM:SSZ> by <ci|update>` (informational).
- Reading (spec D8): merge by name. Every shipped row; the root copy's rows for names the shipped copy lacks; for a name in both, the shipped row wins when it is `ok`/`ok-latest`, else the root row.
- Shipped copy: `<dir of pyenv.exe>\..\share\conda\windows.tsv`. Updated copy: `<root>\.conda_versions.tsv`.
- Installer flags, verbatim: `/S /InstallationType=JustMe /AddToPath=0 /RegisterPython=0 /NoRegistry=1 /NoShortcuts=1 /D=<target>` (`/D=` last, unquoted).
- Every hash comes from a publisher: repo.anaconda.com's `SHA256` column or Miniforge's `<asset>.sha256` file. Never GitHub's asset digest, never a hash rpyenv computed.
- A changed hash on an existing `ok` row is never taken silently: CI keeps the old one, names the row and fails, unless the row is named in `--accept`; `pyenv update` keeps the old one, warns and exits 0 (spec D8).
- Conda child `PATH` order: `root; Library\mingw-w64\bin; Library\usr\bin; Library\bin; Scripts; bin`.
- Never run a conda installer, the MSI, or rpyenv against the real `C:\Users\JM\.pyenv` on the development machine: real installs run in Windows Sandbox (Task 1, Task 9) or CI (Task 10). Tests set `PYENV`, `PYENV_ROOT`, `PYENV_HOME`, `HOME` to scratch.
- Commit messages: one line, no attribution lines.
- Linux behavior unchanged (spec D7): Linux tests must stay green (`cargo test` in WSL).

## Review Focus

1. **A pyenv root path with a space or a non-ASCII letter** (`C:\Users\José Pérez\.pyenv`): the install must refuse before downloading, naming the character and the path, not fail inside the installer. Pinned in Task 8 (`a_root_with_a_refused_character_is_refused_before_download`).
2. **`pyenv install miniforge3`** (bare family): installs the family's newest installable release. Pinned in Task 8 (`a_bare_family_installs_its_newest_installable_release`) and `latest -k` in Task 8.
3. **`pyenv update` offline or rate-limited by GitHub (HTTP 403):** the CPython cache is still written, the previous conda table is kept, exit 1 (0 with `--ignore`). Pinned in Task 5 (`a_failed_conda_read_keeps_the_previous_table`).
4. **An MSI upgrade carrying a corrected hash, on a machine whose root copy is newer:** the shipped (reviewed) row wins for that name, the root copy still contributes newer names, and a truncated root copy is ignored with a warning. Pinned in Task 2 (`shipped_rows_win_and_the_root_copy_adds_names`, `an_unreadable_copy_is_ignored_with_a_warning`).
5. **Running `conda` or `pip` with a conda env selected:** `conda` resolves to the install's `Scripts\conda.exe` (envs have none); `pip` resolves to the env's own. Pinned in Task 6 (`conda_in_an_env_runs_the_installs_conda`).

---

### Task 1: Measure the real installers in Windows Sandbox

Facts §5 and §8 of the spec depend on, measured before code. Runs only in Windows Sandbox (`wsb`), never on the development machine.

**Files:**
- Create: `installer/sandbox/conda-probe.ps1` (runs inside the Sandbox)
- Create: `installer/sandbox/run-conda-probe.ps1` (host side: starts a Sandbox, shares folders, runs the probe, stops it)
- Create: `crates/rpyenv-core/tests/fixtures/miniforge3-conda-meta/*.json` (trimmed: `name`, `version`, `files` only)
- Create: `crates/rpyenv-core/tests/fixtures/miniforge3-expected-shims.txt`
- Create: `crates/pyenv/tests/fixtures/conda-win/installer-tree.txt` (the files a Miniforge install leaves, relative paths, for the fake installer)
- Modify: `docs/superpowers/specs/2026-10-16-m7c-conda-windows-design.md` §3 (add rows) and §5 step 2 (the character set)

**Interfaces:**
- Produces: `REFUSED_CHARS` per family (a list of chars) used by Task 8's `winconda::refused_char`; the fixture folders used by Tasks 7 and 8.

- [ ] **Step 1: Write the host runner.** Model it on `installer/sandbox/run-checks.ps1` (same `wsb start`/`wsb share`/`wsb exec` calls and `Exec` helper; read that file first). It shares `C:\tmp\m7c-installers` (read-only, the three installers downloaded on the host with `curl.exe -L -o`, each checked against the listing's SHA-256 before sharing) and a writable results folder `dist\sandbox-results\conda-<stamp>`, then runs `pwsh -NoProfile -File <share>\conda-probe.ps1 -Installers <share> -Out <results>` as `ExistingLogin`, then stops the Sandbox with `-Stop`.

Installers (newest at planning time; take the newest numbered row of each family from the table Task 4 writes, or from the live listing):
```
Miniforge3-26.7.2-0-Windows-x86_64.exe   (github.com/conda-forge/miniforge/releases/download/26.7.2-0/)
Miniconda3-py313_25.9.1-3-Windows-x86_64.exe (repo.anaconda.com/miniconda/)
Anaconda3-2026.07-1-Windows-x86_64.exe   (repo.anaconda.com/archive/)
```

- [ ] **Step 2: Write the probe.** `conda-probe.ps1` does, per installer, writing everything under `-Out`:

```powershell
param([string] $Installers, [string] $Out)
$ErrorActionPreference = 'Stop'
function Snap([string] $tag) {
    reg export HKCU "$Out\$tag-hkcu.reg" /y | Out-Null
    reg export HKLM\SOFTWARE "$Out\$tag-hklm.reg" /y | Out-Null
    [Environment]::GetEnvironmentVariable('PATH', 'User') | Set-Content "$Out\$tag-userpath.txt"
    Get-ChildItem "$env:APPDATA\Microsoft\Windows\Start Menu" -Recurse -ErrorAction SilentlyContinue |
        ForEach-Object FullName | Set-Content "$Out\$tag-startmenu.txt"
}
foreach ($exe in Get-ChildItem $Installers -Filter '*-Windows-x86_64.exe') {
    $fam = ($exe.Name -split '-')[0].ToLower()
    $target = "C:\probe\$fam"
    Snap "$fam-before"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process $exe.FullName -ArgumentList '/S','/InstallationType=JustMe','/AddToPath=0','/RegisterPython=0','/NoRegistry=1','/NoShortcuts=1',"/D=$target" -PassThru
    $p.WaitForExit()
    # /S blocks? If the process exits before python.exe exists, it does not.
    "exit=$($p.ExitCode) seconds=$([int]$sw.Elapsed.TotalSeconds) python_at_exit=$(Test-Path "$target\python.exe")" |
        Set-Content "$Out\$fam-run.txt"
    Start-Sleep 20
    "python_after_20s=$(Test-Path "$target\python.exe")" | Add-Content "$Out\$fam-run.txt"
    Snap "$fam-after"
    Get-ChildItem $target -Recurse -File | ForEach-Object { $_.FullName.Substring($target.Length + 1) } |
        Set-Content "$Out\$fam-tree.txt"
    Copy-Item "$target\conda-meta" "$Out\$fam-conda-meta" -Recurse
    & "$target\python.exe" -c "import sys, ssl; print(sys.version)" *> "$Out\$fam-import.txt"
    # Path characters: one install per character into C:\probe\c<code>-a<char>b, Miniforge only
    # (same installer program for all three), exit code and whether python.exe appeared.
}
$mf = Get-ChildItem $Installers -Filter 'Miniforge3-*.exe' | Select-Object -First 1
foreach ($c in @(' ', 'é', '^', '%', '!', '=', '&', '(', ';', ',', "'", '#', '$', '@', '+', '~')) {
    $code = [int][char]$c
    $t = "C:\probe\c$code-a$($c)b"
    $p = Start-Process $mf.FullName -ArgumentList '/S','/InstallationType=JustMe','/AddToPath=0','/RegisterPython=0','/NoRegistry=1','/NoShortcuts=1',"/D=$t" -PassThru
    $p.WaitForExit()
    "$code`t$c`texit=$($p.ExitCode)`tpython=$(Test-Path "$t\python.exe")" | Add-Content "$Out\path-chars.txt"
    Remove-Item $t -Recurse -Force -ErrorAction SilentlyContinue
}
```

- [ ] **Step 3: Run it.** `pwsh -NoProfile -File installer/sandbox/run-conda-probe.ps1 -Stop`. Expected: a results folder with `*-run.txt`, `*-tree.txt`, `*-conda-meta\`, before/after snapshots, `path-chars.txt`.

- [ ] **Step 4: Diff the snapshots** on the host with a script `C:\tmp\m7c_snapdiff.py` (`python -I`, `encoding='utf-16'` for `.reg` exports): report keys added under HKCU and HKLM\SOFTWARE, Start-menu entries added, user `PATH` changed. Expected (spec): none outside the install. Any finding goes into spec §3 and, if something is registered, a new uninstall step is a spec change: stop and report it.

- [ ] **Step 5: Compute the expected shim list independently** with `C:\tmp\m7c_shims_ref.py` (Python, written from spec §6.3's text, not from Rust code): over `miniforge3-tree.txt` and `miniforge3-conda-meta\*.json`, list the stems of `.exe/.bat/.cmd` files in root, `Scripts\`, `Library\bin\` that the rule shims, minus names starting with `.`, `activate`/`deactivate`, and the names of `crates/pyenv/python-build/rehash/conda.d/default.list` (strip `#…` comments and blanks). Write them lowercased, sorted, one per line, to `crates/rpyenv-core/tests/fixtures/miniforge3-expected-shims.txt`. Also print the counts for all three families (shim, skipped) for spec §3.

- [ ] **Step 6: Save the fixtures.** Trim each `miniforge3-conda-meta\*.json` to `{"name","version","files"}` (Python, `json.dump(..., indent=None)`), into `crates/rpyenv-core/tests/fixtures/miniforge3-conda-meta/`. Copy `miniforge3-tree.txt` to `crates/pyenv/tests/fixtures/conda-win/installer-tree.txt`. Check the fixture folder size (`du -sh`); keep under 2 MB, else drop `files` entries outside root, `Scripts/`, `Library/bin/` and `site-packages/` paths (the rule needs only those) and say so in the commit.

- [ ] **Step 7: Record in the spec.** Add §3 rows (method: this probe, Sandbox build, date): registry/Start-menu/PATH result, `/S` blocking (exit vs `python.exe` presence), exit codes, the refused characters, `import ssl` result, shim and skip counts per family. Replace §5 step 2's "measured in plan Task 1" with the measured set.

- [ ] **Step 8: Commit.**
```bash
git add installer/sandbox/conda-probe.ps1 installer/sandbox/run-conda-probe.ps1 crates/rpyenv-core/tests/fixtures crates/pyenv/tests/fixtures/conda-win docs/superpowers/specs/2026-10-16-m7c-conda-windows-design.md
git commit -m "M7c task 1: Sandbox probe of the three conda installers; fixtures and measured facts in the spec"
```

---

### Task 2: Read the Windows table (two copies, newer wins)

**Files:**
- Modify: `crates/pyenv/src/install/conda.rs` (new items below `linux_table`; `LINUX_TSV` becomes `pub(crate)`)
- Test: `crates/pyenv/tests/conda_windows_table.rs` (new)

**Interfaces:**
- Consumes: `conda::parse_verdict`, `conda::LinuxRow`, `conda::Verdict` (existing). `LinuxRow` serves both tables; its name is kept to avoid churn.
- Produces:
  - `pub const WIN_ARCH: &str = "Windows-x86_64";`
  - `pub const WIN_TABLE: &str = ".conda_versions.tsv";`
  - `pub const WIN_PREFIXES: [&str; 3]` (`https://repo.anaconda.com/miniconda/`, `https://repo.anaconda.com/archive/`, `https://github.com/conda-forge/miniforge/`)
  - `pub fn generated(text: &str) -> Option<(String, String)>` (time, by)
  - `pub fn parse_windows(text: &str, trusted_only: bool) -> Result<Vec<LinuxRow>, String>`
  - `pub struct WinTable { pub rows: Vec<LinuxRow>, pub warnings: Vec<String> }`
  - `pub fn windows_table(root: &Path) -> Result<WinTable, String>` (merged per spec D8; Err text: `rpyenv has no conda version table; run pyenv update`)
  - `pub fn shipped_table_path() -> Option<PathBuf>`

- [ ] **Step 1: Write the failing tests** in `crates/pyenv/tests/conda_windows_table.rs`:

```rust
use pyenv::install::conda::{self, Verdict};
use std::fs;

const ROW: &str = "miniforge3-26.7.2-0\tWindows-x86_64\tok Miniforge3-26.7.2-0-Windows-x86_64 https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe#1111111111111111111111111111111111111111111111111111111111111111 miniconda verify_py3\n";

fn table(time: &str, by: &str, extra: &str) -> String {
    format!("# generated {time} by {by}\n# <name>\\t<arch>\\t<verdict>\n{ROW}{extra}")
}

#[test]
fn generated_reads_time_and_source() {
    assert_eq!(
        conda::generated(&table("2026-10-16T09:30:00Z", "ci", "")),
        Some(("2026-10-16T09:30:00Z".into(), "ci".into()))
    );
    assert_eq!(conda::generated("# <name>\n"), None);
    assert_eq!(conda::generated("# generated yesterday by ci\n"), None);
}

#[test]
fn a_windows_row_must_be_windows_and_from_a_publisher() {
    let rows = conda::parse_windows(&table("2026-10-16T09:30:00Z", "ci", ""), true).unwrap();
    assert!(matches!(rows[0].verdict, Verdict::Ok(_)));
    let linux = ROW.replace("Windows-x86_64\t", "Linux-x86_64\t");
    assert!(conda::parse_windows(&table("2026-10-16T09:30:00Z", "ci", "").replace(ROW, &linux), true).is_err());
    let foreign = ROW.replace("https://github.com/conda-forge/miniforge/", "https://example.com/");
    assert!(conda::parse_windows(&foreign, true).is_err());
    assert!(conda::parse_windows(&foreign, false).is_ok());
}

/// Review focus 4 (spec D8): a shipped `ok` row wins over a newer root copy's row for the
/// same name; the root copy adds names, and replaces a shipped row that isn't installable.
#[test]
fn shipped_rows_win_and_the_root_copy_adds_names() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let t = tempfile::tempdir().unwrap();
    let (root, shipped) = (t.path().join("root"), t.path().join("shipped"));
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&shipped).unwrap();
    let other_hash = ROW.replace(&"1".repeat(64), &"2".repeat(64));
    let newer_name = ROW.replace("26.7.2-0", "27.1.0-0");
    fs::write(shipped.join("windows.tsv"), table("2026-01-01T00:00:00Z", "ci", "anaconda-1.5.1\tWindows-x86_64\tno-binary Anaconda\n")).unwrap();
    fs::write(
        root.join(conda::WIN_TABLE),
        format!("# generated 2026-10-16T09:30:00Z by update\n{other_hash}{newer_name}anaconda-1.5.1\tWindows-x86_64\tok Anaconda-1.5.1-Windows-x86_64 https://repo.anaconda.com/archive/Anaconda-1.5.1-Windows-x86_64.exe#{} anaconda verify_py3\n", "3".repeat(64)),
    ).unwrap();
    std::env::set_var("RPYENV_TEST_CONDA_TABLES", &shipped);
    let w = conda::windows_table(&root).unwrap();
    let url = |n: &str| match &w.rows.iter().find(|r| r.name == n).unwrap().verdict {
        Verdict::Ok(i) => i.url.clone(),
        v => panic!("{n}: {v:?}"),
    };
    assert!(url("miniforge3-26.7.2-0").ends_with(&"1".repeat(64)), "the shipped hash wins");
    assert!(url("miniforge3-27.1.0-0").ends_with(&"1".repeat(64)), "the root copy adds a name");
    assert!(url("anaconda-1.5.1").ends_with(&"3".repeat(64)), "a root ok row replaces a shipped no-binary");
    assert_eq!(w.rows.len(), 3);
}

/// Review focus 4: a truncated root copy is ignored with a warning naming it.
#[test]
fn an_unreadable_copy_is_ignored_with_a_warning() {
    let t = tempfile::tempdir().unwrap();
    let (root, shipped) = (t.path().join("root"), t.path().join("shipped"));
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&shipped).unwrap();
    fs::write(root.join(conda::WIN_TABLE), "# generated 2027-01-01T00:00:00Z by update\nminiforge3-1\tWindows-x86_64\tok trunc").unwrap();
    fs::write(shipped.join("windows.tsv"), table("2026-10-16T09:30:00Z", "ci", "")).unwrap();
    std::env::set_var("RPYENV_TEST_CONDA_TABLES", &shipped);
    let w = conda::windows_table(&root).unwrap();
    assert_eq!(w.rows.len(), 1);
    assert_eq!(w.warnings.len(), 1);
    assert!(w.warnings[0].contains(conda::WIN_TABLE), "{:?}", w.warnings);
}

#[test]
fn no_copy_at_all_names_pyenv_update() {
    let t = tempfile::tempdir().unwrap();
    std::env::set_var("RPYENV_TEST_CONDA_TABLES", t.path().join("none"));
    let e = conda::windows_table(t.path()).unwrap_err();
    assert_eq!(e, "rpyenv has no conda version table; run pyenv update");
}
```

These tests set a process-wide variable (`RPYENV_TEST_CONDA_TABLES`), and tests in one binary run in parallel: add `static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());` and make each test's first line `let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());`.

- [ ] **Step 2: Run them.** `cargo test -p pyenv --test conda_windows_table` → FAIL to compile (`generated`, `parse_windows`, `windows_table`, `WIN_TABLE` not found).

- [ ] **Step 3: Implement** in `conda.rs` (below `linux_table`):

```rust
/// M7c design §4.1: the Windows table's arch, file names and download folders.
pub const WIN_ARCH: &str = "Windows-x86_64";
pub const WIN_TABLE: &str = ".conda_versions.tsv";
pub const WIN_PREFIXES: [&str; 3] = [
    "https://repo.anaconda.com/miniconda/",
    "https://repo.anaconda.com/archive/",
    "https://github.com/conda-forge/miniforge/",
];

/// `# generated <YYYY-MM-DDTHH:MM:SSZ> by <who>` on the first line: (time, who).
pub fn generated(text: &str) -> Option<(String, String)> {
    let line = text.lines().next()?.strip_prefix("# generated ")?;
    let (time, by) = line.split_once(" by ")?;
    let b = time.as_bytes();
    let shape = b.len() == 20
        && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[16] == b':' && b[19] == b'Z'
        && b.iter().enumerate().all(|(i, c)| [4, 7, 10, 13, 16, 19].contains(&i) || c.is_ascii_digit());
    (shape && !by.is_empty()).then(|| (time.to_string(), by.to_string()))
}

/// A Windows table (§4.1): `linux.tsv`'s grammar, every row `Windows-x86_64`, and, when
/// `trusted_only`, every installer from one of `WIN_PREFIXES`.
pub fn parse_windows(text: &str, trusted_only: bool) -> Result<Vec<LinuxRow>, String> {
    let rows = parse_linux_with(text, false)?;
    for r in &rows {
        if r.arch != WIN_ARCH {
            return Err(format!("{}: arch {} in a Windows table", r.name, r.arch));
        }
        if let Verdict::Ok(i) | Verdict::OkLatest(i) = &r.verdict {
            if trusted_only && !WIN_PREFIXES.iter().any(|p| i.url.starts_with(p)) {
                return Err(format!("{}: {} is outside the three conda download folders", r.name, i.url));
            }
        }
    }
    Ok(rows)
}

pub struct WinTable {
    pub rows: Vec<LinuxRow>,
    /// Copies that exist but couldn't be read, one line each.
    pub warnings: Vec<String>,
}

/// The shipped copy (§4.4): `<exe dir>\..\share\conda\windows.tsv`. Debug builds take
/// `<RPYENV_TEST_CONDA_TABLES>\windows.tsv` when that is set.
pub fn shipped_table_path() -> Option<std::path::PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("RPYENV_TEST_CONDA_TABLES").filter(|v| !v.is_empty()) {
        return Some(std::path::PathBuf::from(p).join("windows.tsv"));
    }
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.join("share").join("conda").join("windows.tsv"))
}

/// Both copies merged by name (§4.4, D8): every shipped row; the root copy's rows for names
/// the shipped copy lacks, or whose shipped row isn't installable. A reviewed row is never
/// replaced by one `pyenv update` read.
pub fn windows_table(root: &std::path::Path) -> Result<WinTable, String> {
    let overridden = cfg!(debug_assertions)
        && std::env::var_os("RPYENV_TEST_CONDA_TABLES").is_some_and(|v| !v.is_empty());
    let mut warnings = Vec::new();
    let mut read = |path: Option<std::path::PathBuf>| -> Option<Vec<LinuxRow>> {
        let path = path?;
        let text = std::fs::read_to_string(&path).ok()?;
        match generated(&text)
            .ok_or_else(|| "no `# generated` line".to_string())
            .and_then(|_| parse_windows(&text, !overridden))
        {
            Ok(rows) => Some(rows),
            Err(e) => {
                warnings.push(format!("rpyenv: ignored {}: {e}", path.display()));
                None
            }
        }
    };
    let shipped = read(shipped_table_path());
    let updated = read(Some(root.join(WIN_TABLE)));
    if shipped.is_none() && updated.is_none() {
        return Err("rpyenv has no conda version table; run pyenv update".to_string());
    }
    let mut rows = shipped.unwrap_or_default();
    for r in updated.unwrap_or_default() {
        match rows.iter_mut().find(|s| s.name == r.name) {
            Some(s) if matches!(s.verdict, Verdict::Ok(_) | Verdict::OkLatest(_)) => {}
            Some(s) => *s = r,
            None => rows.push(r),
        }
    }
    Ok(WinTable { rows, warnings })
}
```

Make `LINUX_TSV` `pub(crate) static` (Task 3's unit tests read it).

- [ ] **Step 4: Run the tests.** `cargo test -p pyenv --test conda_windows_table` → PASS (5 tests). Also `cargo test -p pyenv --lib conda` → still PASS.

- [ ] **Step 5: Commit.**
```bash
git add crates/pyenv/src/install/conda.rs crates/pyenv/tests/conda_windows_table.rs
git commit -m "M7c task 2: read the Windows conda table, shipped rows winning over the updated copy"
```

---

### Task 3: Build the Windows table from the publishers (`condacatalog.rs`)

**Files:**
- Create: `crates/pyenv/src/install/condacatalog.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (`pub mod condacatalog;`)
- Modify: `crates/pyenv/src/install/conda.rs` (`latest_source`'s Miniforge file extension follows the arch)
- Test: unit tests in `condacatalog.rs`; `crates/pyenv/tests/conda_catalog.rs` (fake server)

**Interfaces:**
- Consumes: `conda::{LinuxRow, Verdict, Install, parse_linux, listing_sha, sha_file, WIN_ARCH, LINUX_TSV}`, `fetch::Fetcher::get_text`.
- Produces:
  - `pub struct Sources { pub miniconda: String, pub archive: String, pub miniforge: String, pub releases: String }` and `pub fn sources() -> Sources`
  - `pub type Published = std::collections::BTreeMap<String, Option<String>>` (installer URL → publisher SHA-256)
  - `pub fn read_published(f: &Fetcher, s: &Sources) -> Result<Published, String>`
  - `pub fn listing_files(html: &str, folder: &str) -> Vec<(String, Option<String>)>`
  - `pub fn release_assets(json: &str, miniforge: &str) -> Result<Vec<(String, bool)>, String>` (URL, has a `.sha256` sidecar)
  - `pub fn swap_url(linux_url: &str) -> Option<String>`
  - `pub fn name_of(file: &str) -> Option<String>`
  - `pub fn tos(name: &str) -> bool`
  - `pub fn from_linux(linux: &[LinuxRow], p: &Published) -> Vec<LinuxRow>`
  - `pub fn refresh(old: &[LinuxRow], p: &Published) -> Vec<LinuxRow>`
  - `pub fn pin_hashes(previous: &[LinuxRow], new: &mut [LinuxRow], accept: &[String]) -> Vec<String>` (names in `accept` take the new hash and are not returned)
  - `pub fn lost(previous: &[LinuxRow], new: &[LinuxRow]) -> Vec<String>`
  - `pub fn add_newer(rows: &mut Vec<LinuxRow>, p: &Published) -> usize`
  - `pub fn render(rows: &[LinuxRow], by: &str, time: &str) -> String`
  - `pub fn utc_stamp(secs: u64) -> String`

- [ ] **Step 1: Write the failing unit tests** (bottom of `condacatalog.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::conda::{parse_linux, Verdict, LINUX_TSV};

    fn linux() -> Vec<LinuxRow> {
        parse_linux(LINUX_TSV).unwrap().into_iter().filter(|r| r.arch == "Linux-x86_64").collect()
    }

    #[test]
    fn swap_url_turns_a_linux_installer_into_its_windows_twin() {
        assert_eq!(
            swap_url("https://repo.continuum.io/miniconda/Miniconda3-latest-Linux-x86_64.sh").as_deref(),
            Some("https://repo.anaconda.com/miniconda/Miniconda3-latest-Windows-x86_64.exe")
        );
        assert_eq!(
            swap_url("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Linux-x86_64.sh#ab").as_deref(),
            Some("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe")
        );
        assert_eq!(swap_url("https://repo.anaconda.com/archive/Anaconda3-2023.03-Linux-aarch64.sh"), None);
    }

    /// Spec §3: 380 of upstream's 388 `ok` names come back from their file; the 8 below don't,
    /// and a new difference fails this test.
    #[test]
    fn naming_backwards_reproduces_upstreams_names_but_eight() {
        const KNOWN: [&str; 8] = [
            "anaconda-2.4.0", "anaconda-4.0.0", "anaconda2-5.0.0", "anaconda3-5.0.0",
            "miniconda-3.18.3", "miniconda2-4.3.27", "miniconda3-3.9-24.1.2-0", "miniconda3-4.3.27",
        ];
        let mut differ = Vec::new();
        let mut same = 0;
        for r in linux() {
            let Verdict::Ok(i) = &r.verdict else { continue };
            let file = i.url.split('#').next().unwrap().rsplit('/').next().unwrap();
            let file = file.replace("-Linux-x86_64.sh", "-Windows-x86_64.exe");
            if name_of(&file).as_deref() == Some(r.name.as_str()) { same += 1 } else { differ.push(r.name.clone()) }
        }
        differ.sort();
        assert_eq!(differ, KNOWN);
        assert_eq!(same, 380);
    }

    #[test]
    fn name_of_ignores_files_that_are_not_releases() {
        assert_eq!(name_of("Miniforge3-uninstaller-patch-Windows-x86_64.exe"), None);
        assert_eq!(name_of("Miniconda3-latest-Windows-x86_64.exe"), None);
        assert_eq!(name_of("Miniconda3-py313_26.1.0-1-Windows-x86_64.exe").as_deref(), Some("miniconda3-3.13-26.1.0-1"));
        assert_eq!(name_of("Anaconda3-2027.01-0-Windows-x86_64.exe").as_deref(), Some("anaconda3-2027.01-0"));
        assert_eq!(name_of("Miniforge3-27.1.0-0-Windows-x86_64.exe").as_deref(), Some("miniforge3-27.1.0-0"));
        assert_eq!(name_of("Miniforge3-27.1.0-0-Linux-x86_64.sh"), None);
    }

    /// Spec §3: the rule picks exactly the 75 Linux rows that carry the variable.
    #[test]
    fn the_tos_rule_picks_linuxs_75_names() {
        let mut by_rule = Vec::new();
        let mut by_row = Vec::new();
        for r in linux() {
            if tos(&r.name) { by_rule.push(r.name.clone()) }
            if let Verdict::Ok(i) | Verdict::OkLatest(i) = &r.verdict {
                if i.env.iter().any(|(k, v)| k == "CONDA_PLUGINS_AUTO_ACCEPT_TOS" && v == "true") {
                    by_row.push(r.name.clone())
                }
            }
        }
        assert_eq!(by_row.len(), 75);
        assert_eq!(by_rule, by_row);
    }

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn published(entries: &[(&str, Option<&str>)]) -> Published {
        entries.iter().map(|(u, s)| (u.to_string(), s.map(str::to_string))).collect()
    }

    #[test]
    fn from_linux_gives_ok_unsupported_and_no_binary() {
        let rows = parse_linux(&format!(
            "a3-1\tLinux-x86_64\tok A3-1-Linux-x86_64 https://repo.anaconda.com/archive/A3-1-Linux-x86_64.sh#{SHA_B} anaconda verify_py3\n\
             anaconda3-2022.10\tLinux-x86_64\tunsupported md5 checksum only\n\
             anaconda3-2023.03\tLinux-x86_64\tok A https://repo.anaconda.com/archive/Anaconda3-2023.03-Linux-x86_64.sh#{SHA_B} anaconda verify_py3\n"
        )).unwrap();
        let p = published(&[
            ("https://repo.anaconda.com/archive/A3-1-Windows-x86_64.exe", Some(SHA_A)),
            ("https://repo.anaconda.com/archive/Anaconda3-2022.10-Windows-x86_64.exe", Some(SHA_A)),
        ]);
        let w = from_linux(&rows, &p);
        assert!(matches!(&w[0].verdict, Verdict::Ok(i) if i.url.ends_with(&format!("A3-1-Windows-x86_64.exe#{SHA_A}")) && i.package == "A3-1-Windows-x86_64"));
        assert!(matches!(&w[1].verdict, Verdict::Ok(i) if i.step == "anaconda"), "{:?}", w[1]);
        assert!(matches!(&w[2].verdict, Verdict::NoBinary(_)));
        assert!(w.iter().all(|r| r.arch == "Windows-x86_64"));
        let p2 = published(&[("https://repo.anaconda.com/archive/A3-1-Windows-x86_64.exe", None)]);
        assert!(matches!(&from_linux(&rows[..1], &p2)[0].verdict, Verdict::Unsupported(w) if w == "no published SHA-256"));
    }

    /// Spec §4.2 step 6 and §4.3 step 1.
    #[test]
    fn a_changed_hash_keeps_the_pinned_one_and_is_named() {
        let row = |sha: &str| format!("m3-1\tWindows-x86_64\tok M https://repo.anaconda.com/miniconda/M-Windows-x86_64.exe#{sha} miniconda verify_py3\n");
        let prev = crate::install::conda::parse_windows(&row(SHA_A), true).unwrap();
        let mut new = crate::install::conda::parse_windows(&row(SHA_B), true).unwrap();
        assert_eq!(pin_hashes(&prev, &mut new, &[]), ["m3-1"]);
        assert_eq!(new, prev);
        // spec D8: an accepted name takes the publisher's new hash.
        let mut accepted = crate::install::conda::parse_windows(&row(SHA_B), true).unwrap();
        assert!(pin_hashes(&prev, &mut accepted, &["m3-1".to_string()]).is_empty());
        assert!(sha_of(&accepted[0]) == Some(SHA_B));
    }

    #[test]
    fn lost_names_rows_that_stopped_installing() {
        let prev = crate::install::conda::parse_windows(&format!("m3-1\tWindows-x86_64\tok M https://repo.anaconda.com/miniconda/M.exe#{SHA_A} miniconda verify_py3\n"), true).unwrap();
        let now = crate::install::conda::parse_windows("m3-1\tWindows-x86_64\tno-binary M\n", true).unwrap();
        assert_eq!(lost(&prev, &now), ["m3-1"]);
        assert!(lost(&prev, &prev).is_empty());
    }

    #[test]
    fn add_newer_adds_only_releases_newer_than_the_family() {
        let mut rows = crate::install::conda::parse_windows(&format!(
            "miniforge3-26.7.2-0\tWindows-x86_64\tok Miniforge3-26.7.2-0-Windows-x86_64 https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe#{SHA_A} miniconda verify_py3\n"
        ), true).unwrap();
        let p = published(&[
            ("https://github.com/conda-forge/miniforge/releases/download/27.1.0-0/Miniforge3-27.1.0-0-Windows-x86_64.exe", Some(SHA_B)),
            ("https://github.com/conda-forge/miniforge/releases/download/4.9.2-7/Miniforge3-4.9.2-7-Windows-x86_64.exe", Some(SHA_B)),
            ("https://github.com/conda-forge/miniforge/releases/download/x/Miniforge3-uninstaller-patch-Windows-x86_64.exe", Some(SHA_B)),
        ]);
        assert_eq!(add_newer(&mut rows, &p), 1);
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["miniforge3-26.7.2-0", "miniforge3-27.1.0-0"]);
    }

    #[test]
    fn render_round_trips_through_the_parser() {
        let rows = from_linux(&linux(), &Published::new());
        let text = render(&rows, "ci", "2026-10-16T09:30:00Z");
        assert_eq!(crate::install::conda::generated(&text), Some(("2026-10-16T09:30:00Z".into(), "ci".into())));
        assert_eq!(crate::install::conda::parse_windows(&text, true).unwrap(), rows);
    }

    #[test]
    fn utc_stamp_formats_seconds() {
        assert_eq!(utc_stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_stamp(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn listing_files_reads_windows_installers_and_their_hashes() {
        let html = format!(r#"<tr><td><a href="M-py313_25.9.1-3-Windows-x86_64.exe">M</a></td><td class="s">88.8M</td><td>2025-11-11 12:41:47</td><td>{SHA_A}</td></tr>
<tr><td><a href="M-Linux-x86_64.sh">M</a></td><td>1M</td><td>x</td><td>{SHA_B}</td></tr>
<tr><td><a href="Old-Windows-x86_64.exe">Old</a></td><td>1M</td><td>x</td><td>9e9f</td></tr>"#);
        let got = listing_files(&html, "https://repo.anaconda.com/miniconda/");
        assert_eq!(got, vec![
            ("https://repo.anaconda.com/miniconda/M-py313_25.9.1-3-Windows-x86_64.exe".to_string(), Some(SHA_A.to_string())),
            ("https://repo.anaconda.com/miniconda/Old-Windows-x86_64.exe".to_string(), None),
        ]);
    }

    #[test]
    fn release_assets_reads_the_api_page() {
        let json = r#"[{"tag_name":"26.7.2-0","assets":[{"name":"Miniforge3-26.7.2-0-Windows-x86_64.exe"},{"name":"Miniforge3-26.7.2-0-Windows-x86_64.exe.sha256"},{"name":"Miniforge3-Windows-x86_64.exe"},{"name":"Miniforge3-26.7.2-0-Linux-x86_64.sh"}]}]"#;
        let got = release_assets(json, "https://github.com/conda-forge/miniforge").unwrap();
        assert_eq!(got, vec![
            ("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe".to_string(), true),
            ("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-Windows-x86_64.exe".to_string(), false),
        ]);
        assert!(release_assets("{\"message\":\"API rate limit exceeded\"}", "x").is_err());
    }
}
```

- [ ] **Step 2: Run.** `cargo test -p pyenv --lib condacatalog` → FAIL to compile.

- [ ] **Step 3: Implement** `condacatalog.rs`:

```rust
//! The Windows conda table (M7c design §4): built from the publishers' own pages, by CI
//! (`examples/conda_windows_table.rs`) and by `pyenv update`. Every hash is the publisher's:
//! repo.anaconda.com's SHA256 column or Miniforge's `<asset>.sha256` file.

use super::conda::{self, Install, LinuxRow, Verdict, WIN_ARCH};
use super::fetch::Fetcher;
use std::collections::BTreeMap;

/// Installer URL → the publisher's SHA-256 (`None`: listed without one).
pub type Published = BTreeMap<String, Option<String>>;

pub struct Sources {
    pub miniconda: String,
    pub archive: String,
    /// `https://github.com/conda-forge/miniforge` (download URLs are built from it).
    pub miniforge: String,
    /// The releases API list, read with `?per_page=100&page=N`.
    pub releases: String,
}

/// Debug builds honor `RPYENV_TEST_CONDA` (as `conda::sources`): `<it>/miniconda/`,
/// `<it>/archive/`, `<it>/miniforge`, `<it>/miniforge-api/releases`.
pub fn sources() -> Sources {
    #[cfg(debug_assertions)]
    if let Some(b) = std::env::var("RPYENV_TEST_CONDA").ok().filter(|v| !v.is_empty()) {
        let b = b.trim_end_matches('/');
        return Sources {
            miniconda: format!("{b}/miniconda/"),
            archive: format!("{b}/archive/"),
            miniforge: format!("{b}/miniforge"),
            releases: format!("{b}/miniforge-api/releases"),
        };
    }
    Sources {
        miniconda: "https://repo.anaconda.com/miniconda/".into(),
        archive: "https://repo.anaconda.com/archive/".into(),
        miniforge: "https://github.com/conda-forge/miniforge".into(),
        releases: "https://api.github.com/repos/conda-forge/miniforge/releases".into(),
    }
}

const WIN_SUFFIX: &str = "-Windows-x86_64.exe";

fn hrefs(html: &str) -> Vec<&str> {
    html.split("href=\"").skip(1).filter_map(|s| s.split('"').next()).collect()
}

/// The Windows x64 installers a repo.anaconda.com listing names, with the SHA-256 its row
/// gives (`conda::listing_sha`), in page order, once each.
pub fn listing_files(html: &str, folder: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for f in hrefs(html) {
        if f.ends_with(WIN_SUFFIX) && !f.contains('/') && !out.iter().any(|(u, _)| u.ends_with(&format!("/{f}"))) {
            out.push((format!("{folder}{f}"), conda::listing_sha(html, f)));
        }
    }
    out
}

/// One page of Miniforge's releases API: each Windows x64 installer's download URL and
/// whether a `<asset>.sha256` sits beside it. A page that isn't a JSON array is an error
/// (GitHub's rate-limit answer is an object).
pub fn release_assets(json: &str, miniforge: &str) -> Result<Vec<(String, bool)>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("releases: {e}"))?;
    let releases = v.as_array().ok_or_else(|| format!("releases: not a list: {}", json.chars().take(120).collect::<String>()))?;
    let mut out = Vec::new();
    for r in releases {
        let tag = r["tag_name"].as_str().unwrap_or_default();
        let names: Vec<&str> = r["assets"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect()).unwrap_or_default();
        for n in &names {
            if n.ends_with(WIN_SUFFIX) && !tag.is_empty() {
                let sidecar = names.contains(&format!("{n}.sha256").as_str());
                out.push((format!("{miniforge}/releases/download/{tag}/{n}"), sidecar));
            }
        }
    }
    Ok(out)
}

/// Both listings, every releases page, and each sidecar `.sha256` (8 at a time).
pub fn read_published(f: &Fetcher, s: &Sources) -> Result<Published, String> {
    let mut p = Published::new();
    for folder in [&s.miniconda, &s.archive] {
        let html = f.get_text(folder).map_err(|e| format!("{folder}: {}", e.message))?;
        let files = listing_files(&html, folder);
        if files.is_empty() {
            return Err(format!("{folder}: lists no Windows installers: has its layout changed?"));
        }
        p.extend(files);
    }
    let mut assets = Vec::new();
    for page in 1..=20 {
        let url = format!("{}?per_page=100&page={page}", s.releases);
        let text = f.get_text(&url).map_err(|e| format!("{url}: {}", e.message))?;
        let got = release_assets(&text, &s.miniforge)?;
        let empty = text.trim() == "[]";
        assets.extend(got);
        if empty { break }
    }
    if assets.is_empty() {
        return Err(format!("{}: lists no Windows installers", s.releases));
    }
    let shas = std::sync::Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..8 {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some((url, sidecar)) = assets.get(i) else { break };
                let sha = sidecar.then(|| f.get_text(&format!("{url}.sha256")).ok().and_then(|t| conda::sha_file(&t))).flatten();
                shas.lock().unwrap().push((url.clone(), sha));
            });
        }
    });
    p.extend(shas.into_inner().unwrap());
    Ok(p)
}

/// A Linux x86_64 installer URL → its Windows twin (§4.2 step 2): fragment dropped,
/// `repo.continuum.io` → `repo.anaconda.com`. `None` for any other arch.
pub fn swap_url(linux_url: &str) -> Option<String> {
    let url = linux_url.split('#').next()?;
    let stem = url.strip_suffix("-Linux-x86_64.sh")?;
    Some(format!("{stem}{WIN_SUFFIX}").replacen("https://repo.continuum.io/", "https://repo.anaconda.com/", 1))
}

fn release_shape(s: &str) -> bool {
    let mut parts = s.split('-');
    let (Some(v), build, None) = (parts.next(), parts.next(), parts.next()) else { return false };
    let nums: Vec<&str> = v.split('.').collect();
    nums.len() >= 2
        && nums.iter().all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && build.is_none_or(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
}

/// Upstream's name for a Windows installer file (§4.3 step 2), or `None` when it isn't a
/// release: `Miniconda3-py313_26.1.0-1-…` → `miniconda3-3.13-26.1.0-1`;
/// `Miniforge3-27.1.0-0-…` → `miniforge3-27.1.0-0`.
pub fn name_of(file: &str) -> Option<String> {
    let stem = file.strip_suffix(WIN_SUFFIX)?;
    let (fam, rest) = stem.split_once('-')?;
    if let Some((py, release)) = rest.strip_prefix("py").and_then(|r| r.split_once('_')) {
        let (major, minor) = py.split_at_checked(1)?;
        return (major.bytes().all(|b| b.is_ascii_digit()) && !minor.is_empty()
            && minor.bytes().all(|b| b.is_ascii_digit()) && release_shape(release))
            .then(|| format!("{}-{major}.{minor}-{release}", fam.to_ascii_lowercase()));
    }
    release_shape(rest).then(|| format!("{}-{rest}", fam.to_ascii_lowercase()))
}

/// The rows that get `CONDA_PLUGINS_AUTO_ACCEPT_TOS=true` (§5 step 4): `miniconda3` with a
/// conda release of 25 or later, and `miniconda3-latest`.
pub fn tos(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("miniconda3-") else { return false };
    if rest == "latest" { return true }
    let release = match rest.split_once('-') {
        Some((py, r)) if py.contains('.') && !r.is_empty() => r,
        _ => rest,
    };
    release.split('.').next().and_then(|m| m.parse::<u64>().ok()).is_some_and(|m| m >= 25)
}

fn step_for(name: &str) -> &'static str {
    if conda::family(name).starts_with("anaconda") { "anaconda" } else { "miniconda" }
}

fn win_row(name: &str, verdict: Verdict) -> LinuxRow {
    LinuxRow { name: name.to_string(), arch: WIN_ARCH.to_string(), verdict }
}

fn ok_row(name: &str, url: &str, sha: &str, step: &str, env: Vec<(String, String)>) -> LinuxRow {
    let file = url.rsplit('/').next().unwrap_or_default();
    win_row(name, Verdict::Ok(Install {
        package: file.trim_end_matches(".exe").to_string(),
        url: format!("{url}#{sha}"),
        step: step.to_string(),
        verify: "verify_py3".to_string(),
        env,
    }))
}

fn verdict_for(name: &str, url: Option<&str>, p: &Published, step: &str, env: Vec<(String, String)>) -> LinuxRow {
    match url.and_then(|u| p.get(u).map(|s| (u, s))) {
        Some((u, Some(sha))) => ok_row(name, u, sha, step, env),
        Some(_) => win_row(name, Verdict::Unsupported("no published SHA-256".into())),
        None => win_row(name, Verdict::NoBinary(conda::family(name).to_string())),
    }
}

/// §4.2: one Windows row per Linux x86_64 row, in the same order.
pub fn from_linux(linux: &[LinuxRow], p: &Published) -> Vec<LinuxRow> {
    let by_name: BTreeMap<String, String> = p.keys().filter_map(|u| Some((name_of(u.rsplit('/').next()?)?, u.clone()))).collect();
    let tos_env = |n: &str| if tos(n) { vec![("CONDA_PLUGINS_AUTO_ACCEPT_TOS".to_string(), "true".to_string())] } else { Vec::new() };
    linux.iter().filter(|r| r.arch == "Linux-x86_64").map(|r| match &r.verdict {
        // The URL swap first (it keeps upstream's twins and re-releases); a miss falls back
        // to the file named backwards, as rows without a URL do.
        Verdict::Ok(i) => {
            let url = swap_url(&i.url).filter(|u| p.contains_key(u)).or_else(|| by_name.get(&r.name).cloned());
            verdict_for(&r.name, url.as_deref(), p, &i.step, i.env.clone())
        }
        Verdict::OkLatest(i) => {
            let url = swap_url(&i.url);
            let unversioned = url.as_deref().and_then(|u| u.rsplit('/').next()).map(str::to_string);
            let served = url.as_deref().is_some_and(|u| p.contains_key(u))
                || unversioned.as_deref().is_some_and(|f| p.keys().any(|k| k.contains("/releases/download/") && k.ends_with(&format!("/{f}"))));
            match (url, served) {
                (Some(u), true) => win_row(&r.name, Verdict::OkLatest(Install {
                    package: unversioned.unwrap_or_default().trim_end_matches(".exe").to_string(),
                    url: u, step: i.step.clone(), verify: i.verify.clone(), env: i.env.clone(),
                })),
                _ => win_row(&r.name, Verdict::NoBinary(conda::family(&r.name).to_string())),
            }
        }
        _ => verdict_for(&r.name, by_name.get(&r.name).map(String::as_str), p, step_for(&r.name), tos_env(&r.name)),
    }).collect()
}

fn sha_of(r: &LinuxRow) -> Option<&str> {
    match &r.verdict { Verdict::Ok(i) => i.url.rsplit_once('#').map(|(_, s)| s), _ => None }
}

/// §4.3 step 1: every `ok` row's hash read again from `p` (a row whose file `p` doesn't list
/// is kept as it was).
pub fn refresh(old: &[LinuxRow], p: &Published) -> Vec<LinuxRow> {
    old.iter().map(|r| match &r.verdict {
        Verdict::Ok(i) => {
            let url = i.url.split('#').next().unwrap_or_default();
            match p.get(url) {
                Some(Some(sha)) => ok_row(&r.name, url, sha, &i.step, i.env.clone()),
                _ => r.clone(),
            }
        }
        _ => r.clone(),
    }).collect()
}

/// §4.2 steps 6-7, §4.3 step 1: a row whose hash differs from `previous` keeps the
/// previous row, unless its name is in `accept` (D8). Returns the kept names, so the caller
/// can report them.
pub fn pin_hashes(previous: &[LinuxRow], new: &mut [LinuxRow], accept: &[String]) -> Vec<String> {
    let mut changed = Vec::new();
    for r in new.iter_mut().filter(|r| !accept.contains(&r.name)) {
        if let (Some(now), Some(old)) = (sha_of(r), previous.iter().find(|o| o.name == r.name)) {
            if let Some(pinned) = sha_of(old) {
                if pinned != now {
                    changed.push(r.name.clone());
                    *r = old.clone();
                }
            }
        }
    }
    changed
}

/// §4.2 step 5: names `ok` in `previous` that are no longer `ok`.
pub fn lost(previous: &[LinuxRow], new: &[LinuxRow]) -> Vec<String> {
    previous.iter()
        .filter(|o| matches!(o.verdict, Verdict::Ok(_)))
        .filter(|o| !new.iter().any(|n| n.name == o.name && matches!(n.verdict, Verdict::Ok(_))))
        .map(|o| o.name.clone()).collect()
}

/// §4.3 step 2: releases newer than their family's newest row, appended in name order.
pub fn add_newer(rows: &mut Vec<LinuxRow>, p: &Published) -> usize {
    let mut added = Vec::new();
    for (url, sha) in p {
        let (Some(sha), Some(name)) = (sha, url.rsplit('/').next().and_then(name_of)) else { continue };
        if rows.iter().any(|r| r.name == name) { continue }
        let fam = conda::family(&name);
        let newest = rows.iter().filter(|r| conda::family(&r.name) == fam).filter_map(|r| conda::release_key(&r.name)).max();
        let Some(key) = conda::release_key(&name) else { continue };
        if newest.is_some_and(|n| key > n) {
            let env = if tos(&name) { vec![("CONDA_PLUGINS_AUTO_ACCEPT_TOS".to_string(), "true".to_string())] } else { Vec::new() };
            added.push(ok_row(&name, url, sha, step_for(&name), env));
        }
    }
    added.sort_by(|a, b| a.name.cmp(&b.name));
    let n = added.len();
    rows.extend(added);
    n
}

fn verdict_text(v: &Verdict) -> String {
    let install = |k: &str, i: &Install| {
        let mut s = format!("{k} {} {} {} {}", i.package, i.url, i.step, i.verify);
        for (n, val) in &i.env { s.push_str(&format!(" {n}={val}")) }
        s
    };
    match v {
        Verdict::Ok(i) => install("ok", i),
        Verdict::OkLatest(i) => install("ok-latest", i),
        Verdict::NoBinary(w) => format!("no-binary {w}"),
        Verdict::Unsupported(w) => format!("unsupported {w}"),
    }
}

pub fn render(rows: &[LinuxRow], by: &str, time: &str) -> String {
    let mut s = format!("# generated {time} by {by}\n# <name>\\t<arch>\\t<verdict>\n");
    for r in rows { s.push_str(&format!("{}\t{}\t{}\n", r.name, r.arch, verdict_text(&r.verdict))) }
    s
}

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since 1970 (civil-from-days, H. Hinnant).
pub fn utc_stamp(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Now, as `utc_stamp`.
pub fn now() -> String {
    utc_stamp(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}
```

In `conda.rs`: make `release_key` `pub(crate)`; in `latest_source` replace the hard-coded `.sh`:

```rust
    let ext = if arch.starts_with("Windows") { "exe" } else { "sh" };
    ...
        .ok_or_else(|| refuse(&format!("Miniforge3-{arch}.{ext}"), &latest))?;
    let file = format!("Miniforge3-{tag}-{arch}.{ext}");
```

- [ ] **Step 4: Run.** `cargo test -p pyenv --lib condacatalog` → PASS (12 tests). `cargo test -p pyenv --lib conda` → PASS.

- [ ] **Step 5: Fake-server test of `read_published`** in `crates/pyenv/tests/conda_catalog.rs`, with `common::server::start` (read `tests/common/server.rs` and `tests/conda_latest.rs` for the `Reply` shapes): routes `/miniconda/` (one Windows row with a hash), `/archive/` (one row), `/miniforge-api/releases?per_page=100&page=1` (one release with a sidecar), `…page=2` (`[]`), `/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe.sha256` (`<sha>  Miniforge3-…exe`). Assert the map has 3 entries with the right hashes; then serve the rate-limit object on page 1 and assert `Err` containing `not a list`. Set `RPYENV_TEST_CONDA` and call `condacatalog::read_published(&Fetcher::direct(), &condacatalog::sources())`.

- [ ] **Step 6: Run** `cargo test -p pyenv --test conda_catalog` → PASS.

- [ ] **Step 7: Commit.**
```bash
git add crates/pyenv/src/install/condacatalog.rs crates/pyenv/src/install/mod.rs crates/pyenv/src/install/conda.rs crates/pyenv/tests/conda_catalog.rs
git commit -m "M7c task 3: build the Windows conda table from the publishers' pages, with pinned hashes"
```

---

### Task 4: CI writes `crates/pyenv/conda/windows.tsv`

**Files:**
- Create: `crates/pyenv/examples/conda_windows_table.rs`
- Create: `crates/pyenv/conda/windows.tsv` (generated by the example, live network)
- Modify: `.github/workflows/sync-python-build.yml` (the "Rebuild the conda table" step, the results lines, the PR body)

**Interfaces:**
- Consumes: Task 3's `sources`, `read_published`, `from_linux`, `pin_hashes`, `lost`, `render`, `now`; `conda::{parse_linux, parse_windows}`.
- Produces: the committed table; exit codes 0 (written), 1 (written, but hashes changed or rows lost; the names on stderr), 2 (couldn't read the publishers; nothing written).

- [ ] **Step 1: Write the example.**

```rust
//! CI's Windows conda table (M7c design §4.2): `cargo run -p pyenv --example
//! conda_windows_table -- <linux.tsv> <windows.tsv> [--accept <name>[,<name>...]]`. Reads
//! the publishers, rebuilds <windows.tsv> from <linux.tsv>'s x86_64 rows, keeps every pinned
//! hash (§4.2 step 6) except the rows a maintainer accepts (§4.2 step 7, D8).
//! Exit 0: written. 1: written, but some rows changed hash or stopped installing (named on
//! stderr, starting `changed hash, kept the pinned one:` / `no longer installable:`).
//! 2: the publishers couldn't be read; nothing written.
use pyenv::install::{conda, condacatalog as cc, fetch::Fetcher};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (paths, accept): (Vec<String>, Vec<String>) = match args.iter().position(|a| a == "--accept") {
        Some(i) => (args[..i].to_vec(), args.get(i + 1).map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default()),
        None => (args.clone(), Vec::new()),
    };
    let [linux_path, win_path] = paths.as_slice() else {
        eprintln!("usage: conda_windows_table <linux.tsv> <windows.tsv> [--accept <name>[,<name>...]]");
        std::process::exit(2);
    };
    let linux = conda::parse_linux(&std::fs::read_to_string(linux_path).expect("linux.tsv")).expect("linux.tsv parses");
    let previous = std::fs::read_to_string(win_path).ok()
        .map(|t| conda::parse_windows(&t, true).expect("the previous windows.tsv parses"))
        .unwrap_or_default();
    let published = match cc::read_published(&Fetcher::direct(), &cc::sources()) {
        Ok(p) => p,
        Err(e) => { eprintln!("{e}"); std::process::exit(2) }
    };
    let mut rows = cc::from_linux(&linux, &published);
    let unknown: Vec<&String> = accept.iter().filter(|a| !previous.iter().any(|r| &r.name == *a)).collect();
    if !unknown.is_empty() {
        eprintln!("--accept names rows the previous table doesn't have: {unknown:?}");
        std::process::exit(2);
    }
    let changed = cc::pin_hashes(&previous, &mut rows, &accept);
    if !accept.is_empty() { println!("accepted the publisher's current hash for: {}", accept.join(" ")) }
    let lost = cc::lost(&previous, &rows);
    std::fs::write(win_path, cc::render(&rows, "ci", &cc::now())).expect("write windows.tsv");
    let mut count = std::collections::BTreeMap::new();
    for r in &rows {
        let k = match r.verdict { conda::Verdict::Ok(_) => "ok", conda::Verdict::OkLatest(_) => "ok-latest", conda::Verdict::NoBinary(_) => "no-binary", conda::Verdict::Unsupported(_) => "unsupported" };
        *count.entry(k).or_insert(0) += 1;
    }
    println!("{} rows: {count:?}", rows.len());
    if !changed.is_empty() { eprintln!("changed hash, kept the pinned one: {}", changed.join(" ")) }
    if !lost.is_empty() { eprintln!("no longer installable: {}", lost.join(" ")) }
    if !changed.is_empty() || !lost.is_empty() { std::process::exit(1) }
}
```

- [ ] **Step 2: Generate the table** (live network, read-only): `cargo run -p pyenv --example conda_windows_table -- crates/pyenv/conda/linux.tsv crates/pyenv/conda/windows.tsv`. Expected: exit 0 and a count line near spec §3's probe (about 428 installable of 445; `ok` + `ok-latest`). If GitHub answers 403 (unauthenticated rate limit), rerun later; don't add a token.

- [ ] **Step 3: Check the table against the probe.** Run `python -I C:\tmp\m7c_probe.py C:\Users\JM\repos\OWN\rpyenv` again and compare: every name the probe calls `ok` must be `ok` or `ok-latest` in `windows.tsv`, and the URL-swap rows must include the 8 twins/re-releases where a Windows file exists. Record the per-family counts for spec §3 ("plan Task 4 reports the final per-family counts").

- [ ] **Step 4: Prove the pin works on the real table.** Copy `windows.tsv` to `C:\tmp\m7c_win_pinned.tsv`, change one `ok` row's hash to 64 `0`s with a Python one-liner, rerun the example with that copy as `<windows.tsv>`: expected exit 1, stderr `changed hash, kept the pinned one: <that row>`, and the file still holds the zeros for it. Delete only `C:\tmp\m7c_win_pinned.tsv`.

- [ ] **Step 4b: Prove `--accept`.** With the same zero-hash copy: `… -- crates/pyenv/conda/linux.tsv C:\tmp\m7c_win_pinned.tsv --accept <that row>` → exit 0 and the row now holds the publisher's hash; `--accept no-such-name` → exit 2. Then delete the copy. Add a short "Accepting a changed conda hash" paragraph to `CONTRIBUTING.md` if it exists, else to the README's maintainer notes (read both; pick the one that already has maintainer content): when CI fails with `changed hash, kept the pinned one`, check why (release notes, the publisher's announcement, `Get-AuthenticodeSignature` on the new installer), then run the example with `--accept <name>` and open a PR saying why.

- [ ] **Step 5: Wire the sync job.** In `.github/workflows/sync-python-build.yml`, step "Rebuild the conda table" (read the whole job first; Rust is already set up for its later `cargo build --workspace`; if the toolchain step comes later, move it before this step). After the Linux line add:

```bash
          set +e
          cargo run -q -p pyenv --example conda_windows_table -- crates/pyenv/conda/linux.tsv crates/pyenv/conda/windows.tsv 2> "$RUNNER_TEMP/conda-win.err" | tee -a "$log"
          win=$?
          set -e
          cat "$RUNNER_TEMP/conda-win.err" | tee -a "$log"
          if [ "$win" = 2 ]; then
            git checkout -- crates/pyenv/conda/windows.tsv
            exit 1
          fi
          # 1: the table is written (new releases arrive) but a hash changed or a row stopped
          # installing; the PR names them and the last step fails (M7c design §4.2 steps 5-6).
          echo "conda_win_attention=$win" >> "$GITHUB_OUTPUT"
```

In the PR body section, when `steps.conda.outputs.conda_win_attention == '1'`, add the two stderr lines verbatim under a heading `**Windows conda table needs attention**`. In "Fail when a table couldn't be rebuilt", also fail when `conda_win_attention == '1'` with `::error::Windows conda table: a hash changed or a row stopped installing; see the PR body`. Keep `set -eo pipefail` semantics of the existing step: the `set +e` window covers only the example run.

- [ ] **Step 6: Test the workflow edit with stubs**, as M7b's follow-up did (`C:\tmp\m7b_sync2_extract.py` / `m7b_sync2_test.sh` are the pattern): extract the step's script, run it in WSL `bash -eo pipefail` in a scratch git repo with a stub `cargo` that exits 0, 1 (with the two stderr lines) and 2, and a stub `python3`; assert: exit 0 → table kept, no attention; 1 → table kept, `conda_win_attention=1`; 2 → table restored, step exits 1. WSL scripts run as `wsl -d Debian --exec /usr/bin/bash -l /mnt/c/tmp/<x>.sh` from PowerShell.

- [ ] **Step 7: Commit.**
```bash
git add crates/pyenv/examples/conda_windows_table.rs crates/pyenv/conda/windows.tsv .github/workflows/sync-python-build.yml
git commit -m "M7c task 4: CI builds and pins the Windows conda table (crates/pyenv/conda/windows.tsv)"
```

---

### Task 5: `pyenv update` rebuilds the conda table

**Files:**
- Modify: `crates/pyenv/src/commands/update.rs` (after `write_db`, before the final `Scanned` line)
- Test: `crates/pyenv/tests/cli_update_win.rs` (read it first; add tests in its style)

**Interfaces:**
- Consumes: `conda::{windows_table, linux_table, WIN_TABLE}`, Task 3's `read_published`, `sources`, `from_linux`, `refresh`, `pin_hashes`, `add_newer`, `render`, `now`.
- Produces: `<root>\.conda_versions.tsv`; output lines `:: [Info] ::  conda: <n> installable versions (<k> new)`, `:: [Warning] :: conda: the publisher changed <names> after rpyenv pinned them; they won't install until an rpyenv release carries the reviewed hash` (exit 0, spec D8), `:: [Warning] :: conda: <source error>; kept the previous table`.

- [ ] **Step 1: Write the failing tests** (Windows-only file; reuse its fake python.org server helper and add conda routes from Task 3 Step 5):

```rust
#[test]
fn update_writes_the_conda_table_with_newer_releases() {
    // python.org routes as the existing tests; conda routes: listings with one miniconda3 25.x
    // and one Miniforge newer than the shipped table (RPYENV_TEST_CONDA_TABLES holds a
    // windows.tsv with miniforge3-26.7.2-0 only).
    // Expect: exit 0; <root>\.conda_versions.tsv starts with "# generated " and " by update";
    // it holds miniforge3-27.1.0-0 as `ok`; stdout has ":: [Info] ::  conda: " with "(1 new)".
}

/// Review focus 3.
#[test]
fn a_failed_conda_read_keeps_the_previous_table() {
    // Serve 403 {"message":"API rate limit exceeded"} for the releases API.
    // Pre-write <root>\.conda_versions.tsv with known bytes.
    // Expect: exit 1; .versions_cache.xml written; .conda_versions.tsv byte-identical;
    // stdout has ":: [Warning] :: conda: " and "kept the previous table".
    // Same with --ignore: exit 0, same file kept.
}

#[test]
fn a_changed_hash_is_kept_named_and_update_warns() {
    // Shipped table has miniforge3-26.7.2-0 with hash A; the sidecar serves hash B.
    // Expect (spec D8): exit 0; written table still has A for it; stdout has
    // ":: [Warning] :: conda: the publisher changed miniforge3-26.7.2-0 after rpyenv pinned them".
}
```

Write each body fully, following the file's existing helpers (fixture root, `pyenv_env`, `RPYENV_TEST_PYTHON_ORG`); compare file bytes with `std::fs::read`.

- [ ] **Step 2: Run.** `cargo test -p pyenv --test cli_update_win` → the three new tests FAIL (no conda table written / no lines).

- [ ] **Step 3: Implement** in `update.rs`, after the `write_db` block (the CPython cache is already on disk, so a conda failure never costs it):

```rust
    // M7c design §4.3: the conda table, rebuilt from the publishers. A failed read keeps the
    // previous table; a changed hash is kept pinned and fails the command.
    let mut conda_failed = false;
    match condacatalog::read_published(&f, &condacatalog::sources()) {
        Err(e) => {
            o.out(format!(":: [Warning] :: conda: {e}; kept the previous table"));
            conda_failed = true;
        }
        Ok(p) => {
            // D8: the merged table (reviewed rows win) is both the base and the pin baseline.
            let (base, previous) = match conda::windows_table(&ctx.root) {
                Ok(t) => (condacatalog::refresh(&t.rows, &p), t.rows),
                Err(_) => match conda::linux_table() {
                    Ok(l) => (condacatalog::from_linux(&l, &p), Vec::new()),
                    Err(e) => { o.out(format!(":: [Warning] :: conda: {e}")); return o.with_code(1) }
                },
            };
            let mut rows = base;
            let changed = condacatalog::pin_hashes(&previous, &mut rows, &[]);
            let added = condacatalog::add_newer(&mut rows, &p);
            let path = ctx.root.join(conda::WIN_TABLE);
            let tmp = ctx.root.join(format!("{}.tmp", conda::WIN_TABLE));
            let text = condacatalog::render(&rows, "update", &condacatalog::now());
            if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)) {
                let _ = std::fs::remove_file(&tmp);
                o.out(format!("pyenv: cannot write {}: {e}", path.display()));
                return o.with_code(1);
            }
            let installable = rows.iter().filter(|r| matches!(r.verdict, conda::Verdict::Ok(_) | conda::Verdict::OkLatest(_))).count();
            o.out(format!(":: [Info] ::  conda: {installable} installable versions ({added} new)"));
            if !changed.is_empty() {
                // D8: a warning, exit 0: the user can't act on it; a release can.
                o.out(format!(":: [Warning] :: conda: the publisher changed {} after rpyenv pinned them; they won't install until an rpyenv release carries the reviewed hash", changed.join(" ")));
            }
        }
    }
```

and at the end, before returning `o` after the `Scanned` line: `if conda_failed && !ignore { return o.with_code(1) }` (only a failed read sets `conda_failed`; a changed hash never fails `update`, spec D8).

- [ ] **Step 4: Run.** `cargo test -p pyenv --test cli_update_win` → PASS (all, old and new).

- [ ] **Step 5: Commit.**
```bash
git add crates/pyenv/src/commands/update.rs crates/pyenv/tests/cli_update_win.rs
git commit -m "M7c task 5: pyenv update rebuilds the conda table from the publishers, keeping pinned hashes"
```

---

### Task 6: Runtime: the conda `PATH`, lookups, and envs on Windows

**Files:**
- Modify: `crates/rpyenv-core/src/venv.rs` (`conda_win_dirs`, `base_prefix`'s `PyenvWin` arm)
- Modify: `crates/rpyenv-core/src/launch.rs` (`win_child_path`)
- Modify: `crates/rpyenv-core/src/lookup.rs` (`win_hits`, `which_win_with`)
- Test: unit tests in those files (their existing `mod tests`)

**Interfaces:**
- Produces:
  - `pub fn conda_win_dirs(prefix: &Path) -> [PathBuf; 6]` in `rpyenv_core::venv`
  - `pub fn is_conda_win_install(dir: &Path) -> bool` (`conda-meta\` and `Scripts\conda.exe`)

- [ ] **Step 1: Failing tests.** In `launch.rs` tests (next to `win_child_path_matches_pyenv_win`):

```rust
#[test]
fn win_child_path_gives_a_conda_tree_its_six_folders() {
    let (_t, mut ctx) = win_ctx(&[]);
    ctx.path = Some("C:\\Windows".into());
    let v = ctx.versions_dir().join("miniforge3-26.7.2-0");
    std::fs::create_dir_all(v.join("conda-meta")).unwrap();
    let got = win_child_path(&ctx, &["miniforge3-26.7.2-0".to_string()], None);
    let want: Vec<String> = ["", "Library\\mingw-w64\\bin", "Library\\usr\\bin", "Library\\bin", "Scripts", "bin"]
        .iter().map(|s| if s.is_empty() { v.display().to_string() } else { v.join(s).display().to_string() }).collect();
    assert_eq!(got.to_string_lossy(), format!("{};C:\\Windows;", want.join(";")));
}
```

In `lookup.rs` tests:

```rust
#[test]
fn a_conda_tree_finds_files_in_library_bin_last() {
    // version miniforge3-x with conda-meta\, Library\bin\tool.exe, and Scripts\pip.exe
    // which_win("tool") → Library\bin\tool.exe; which_win("pip") → Scripts\pip.exe.
    // A non-conda version with Library\bin\tool.exe: not found (CPython layout unchanged).
}

/// Review focus 5.
#[test]
fn conda_in_an_env_runs_the_installs_conda() {
    // miniforge3-x\conda-meta, miniforge3-x\Scripts\conda.exe,
    // miniforge3-x\envs\e1\conda-meta, envs\e1\python.exe, envs\e1\Scripts\pip.exe.
    // PYENV_VERSION=miniforge3-x/envs/e1:
    //   which_win_runnable("conda") → miniforge3-x\Scripts\conda.exe
    //   which_win_runnable("pip")   → envs\e1\Scripts\pip.exe
}
```

In `venv.rs` tests:

```rust
#[test]
fn windows_conda_installs_are_their_own_base_and_envs_point_at_theirs() {
    // PyenvWin ctx; miniforge3-x\{python.exe, Scripts\conda.exe, conda-meta\};
    // envs\e1\{python.exe, conda-meta\}.
    // base_prefix("miniforge3-x") == Ok(versions\miniforge3-x)
    // base_prefix("miniforge3-x/envs/e1") == Ok(versions\miniforge3-x)
    // A CPython 3.12.0 with python.exe and no pyvenv.cfg: Err(NotVenv).
}
```

Write the bodies with the existing helpers in each module (`win_ctx`, `touch`, `exe`); assert exact paths.

- [ ] **Step 2: Run.** `cargo test -p rpyenv-core` → the new tests FAIL.

- [ ] **Step 3: Implement.**

`venv.rs`:
```rust
/// Conda's activation folders on Windows, in conda's order (M7c design §6.2).
pub fn conda_win_dirs(prefix: &Path) -> [PathBuf; 6] {
    [
        prefix.to_path_buf(),
        prefix.join("Library").join("mingw-w64").join("bin"),
        prefix.join("Library").join("usr").join("bin"),
        prefix.join("Library").join("bin"),
        prefix.join("Scripts"),
        prefix.join("bin"),
    ]
}

/// A conda install on Windows (not an env): `conda-meta\` and `Scripts\conda.exe`.
pub fn is_conda_win_install(dir: &Path) -> bool {
    dir.join("conda-meta").is_dir() && dir.join("Scripts").join("conda.exe").is_file()
}
```
`base_prefix`, `Flavor::PyenvWin` arm:
```rust
        Flavor::PyenvWin => {
            if !(p.join("python.exe").is_file() || p.join("Scripts").join("python.exe").is_file()) {
                return Err(VenvError::NoPython(name.to_string()));
            }
            if is_conda_win_install(&p) {
                Some(p.clone())
            } else if p.join("conda-meta").is_dir() {
                // An env: versions\<install>\envs\<env>. No canonicalize (`\\?\` paths).
                p.parent().filter(|e| e.file_name().is_some_and(|n| n == "envs")).and_then(Path::parent).map(Path::to_path_buf)
            } else {
                read_cfg(&p, Flavor::PyenvWin).and_then(|c| c.home)
            }
        }
```
`launch.rs` `win_child_path`, inside `for n in names`:
```rust
        let v = ctx.versions_dir().join(n);
        if v.join("conda-meta").is_dir() {
            for d in crate::venv::conda_win_dirs(&v) {
                add(d.as_os_str());
            }
        } else {
            add(v.as_os_str());
            add(v.join("Scripts").as_os_str());
            add(v.join("bin").as_os_str());
        }
```
`lookup.rs` `win_hits`: build the folder list as a `Vec` and push `version_dir.join("Library").join("bin")` last when `version_dir.join("conda-meta").is_dir()`. In `which_win_with`, after the `system_site_packages` fallback inside the loop, before moving on to the next selected version:
```rust
        // M7c design §6.1: an env has no conda of its own; `conda` is its install's.
        if program.eq_ignore_ascii_case("conda") && dir.join("conda-meta").is_dir() && !crate::venv::is_conda_win_install(&dir) {
            if let Some(base) = dir.parent().filter(|e| e.file_name().is_some_and(|n| n == "envs")).and_then(Path::parent) {
                if let Some(path) = win_hits(base, program, &exts).into_iter().find(|h| !runnable_only || is_runnable_win(h)) {
                    return Ok(Found { path, warnings: Vec::new() });
                }
            }
        }
```

- [ ] **Step 4: Run.** `cargo test -p rpyenv-core` → PASS. Also in WSL: `cargo test -p rpyenv-core` (Linux unchanged).

- [ ] **Step 5: Commit.**
```bash
git add crates/rpyenv-core/src/venv.rs crates/rpyenv-core/src/launch.rs crates/rpyenv-core/src/lookup.rs
git commit -m "M7c task 6: conda trees on Windows get conda's PATH, Library\\bin lookups and env bases"
```

---

### Task 7: Shims by package ownership (`conda_owner.rs`)

**Files:**
- Create: `crates/rpyenv-core/src/conda_owner.rs`
- Modify: `crates/rpyenv-core/src/lib.rs` (`pub mod conda_owner;`)
- Modify: `crates/rpyenv-core/src/shimset.rs` (`shims_win`)
- Test: `crates/rpyenv-core/tests/conda_owner_win.rs` (uses Task 1's fixtures); unit tests in `conda_owner.rs`

**Interfaces:**
- Consumes: `conda_list::names()` (existing), Task 1 fixtures.
- Produces:
  - `pub struct Owners` with `pub fn read(prefix: &Path) -> Owners` and `pub fn shim(&self, rel: &str, stem: &str) -> bool` (`rel` is the file's path under the version, `/`-separated, any case)

- [ ] **Step 1: Failing unit tests** in `conda_owner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tree(meta: &[(&str, &[&str])]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("conda-meta")).unwrap();
        for (i, (name, files)) in meta.iter().enumerate() {
            let json = serde_json::json!({"name": name, "version": "1", "files": files});
            std::fs::write(t.path().join("conda-meta").join(format!("{name}-{i}.json")), json.to_string()).unwrap();
        }
        t
    }

    #[test]
    fn python_related_owners_get_shims_and_tools_do_not() {
        let t = tree(&[
            ("python", &["python.exe", "Lib/site.py"]),
            ("conda", &["Scripts/conda.exe", "Lib/site-packages/conda/__init__.py"]),
            ("requests", &["Lib/site-packages/requests/__init__.py", "Scripts/req.exe"]),
            ("m2-cygpath", &["Library/usr/bin/cygpath.exe"]),
            ("libcurl", &["Library/bin/curl.exe"]),
            ("menuinst", &["Lib/site-packages/menuinst/__init__.py", "Library/bin/menu.exe"]),
        ]);
        let o = Owners::read(t.path());
        assert!(o.shim("python.exe", "python"));
        assert!(o.shim("Scripts/conda.exe", "conda"));
        assert!(o.shim("Scripts/req.exe", "req"));
        assert!(o.shim("Library/bin/menu.exe", "menu"));
        assert!(!o.shim("Library/bin/curl.exe", "curl"));
        // Unowned: only in Scripts (where pip writes).
        assert!(o.shim("Scripts/mytool.exe", "mytool"));
        assert!(!o.shim("pre_uninstall.bat", "pre_uninstall"));
        assert!(!o.shim("Library/bin/x.exe", "x"));
    }

    #[test]
    fn default_list_names_never_get_shims_even_when_python_related() {
        let t = tree(&[("pkg", &["Lib/site-packages/pkg/__init__.py", "Scripts/curl.exe"])]);
        assert!(!Owners::read(t.path()).shim("Scripts/curl.exe", "curl"));
    }

    #[test]
    fn paths_compare_without_case() {
        let t = tree(&[("python", &["python.exe"])]);
        assert!(Owners::read(t.path()).shim("PYTHON.EXE", "PYTHON"));
    }
}
```

`tempfile` and `serde_json` must be dev-dependencies of `rpyenv-core`; check its `Cargo.toml` (`serde_json` is already a `pyenv` dependency; adding it to `rpyenv-core` runtime is needed for `read`: the shims crate must not link it, so check spec §3's shim-size rule: `rpyenv-core` is linked by the shims). **If `rpyenv-core` must stay free of serde_json, parse the two fields by hand**: `"name": "<s>"` and the `"files": [ ... ]` string array, with a small scanner (no escapes beyond `\\` and `\"`, which conda paths don't use; a file that doesn't parse is skipped). Decide by reading `crates/rpyenv-core/Cargo.toml` and the spec's §3 shim dependency rule; record the choice in the commit message.

- [ ] **Step 2: Run.** `cargo test -p rpyenv-core conda_owner` → FAIL.

- [ ] **Step 3: Implement** `conda_owner.rs`:

```rust
//! Which conda executables get shims on Windows (parent spec §15.1 resolution 3, M7c design
//! §6.3): by the package that owns each file, read from `conda-meta\*.json`; upstream's
//! `default.list` names never.

use std::collections::HashMap;
use std::path::Path;

pub struct Owners {
    /// Lowercased `/`-path → whether its package is Python-related.
    by_path: HashMap<String, bool>,
}

impl Owners {
    pub fn read(prefix: &Path) -> Owners {
        let mut by_path = HashMap::new();
        let Ok(rd) = std::fs::read_dir(prefix.join("conda-meta")) else { return Owners { by_path } };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "json") { continue }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let Some((name, files)) = parse_meta(&text) else { continue };
            let related = name == "python" || name == "conda"
                || files.iter().any(|f| f.to_ascii_lowercase().contains("site-packages/"));
            for f in files {
                by_path.insert(f.replace('\\', "/").to_ascii_lowercase(), related);
            }
        }
        Owners { by_path }
    }

    /// `rel`: the file under the version (`Scripts/pip.exe`); `stem`: its name without
    /// extension.
    pub fn shim(&self, rel: &str, stem: &str) -> bool {
        if crate::conda_list::names().contains(stem.to_ascii_lowercase().as_str()) {
            return false;
        }
        let key = rel.replace('\\', "/").to_ascii_lowercase();
        match self.by_path.get(&key) {
            Some(related) => *related,
            None => key.starts_with("scripts/") && !key[8..].contains('/'),
        }
    }
}
```
plus `fn parse_meta(text: &str) -> Option<(String, Vec<String>)>` (serde_json or the hand scanner per Step 1's decision).

`shimset.rs` `shims_win`: per `entry`, compute `let owners = entry.path.join("conda-meta").is_dir().then(|| crate::conda_owner::Owners::read(&entry.path));` and the folder list: `[root, Scripts, Library\bin]` when `owners.is_some()`, else the existing `[root, Scripts, bin]`. After the existing name/extension/activation checks, when `owners` is `Some(o)`: compute `rel` (`""`, `"Scripts/"` or `"Library/bin/"` + file name) and `continue` unless `o.shim(&rel, stem)`.

- [ ] **Step 4: Fixture test** `crates/rpyenv-core/tests/conda_owner_win.rs` (`#![cfg(windows)]`): build `<tmp>\versions\miniforge3-x` from `tests/fixtures/miniforge3-conda-meta/*.json` (copy them into `conda-meta\`, then create every listed file that lies in root, `Scripts/` or `Library/bin/` as an empty file), call `shimset::shims_win(&versions, &[])`, collect the lowercased stems, sort, and assert equality with `tests/fixtures/miniforge3-expected-shims.txt` (Task 1 Step 5's independent list).

- [ ] **Step 5: Run.** `cargo test -p rpyenv-core` → PASS, including `conda_owner_win`. WSL: `cargo test -p rpyenv-core` → PASS.

- [ ] **Step 6: Commit.**
```bash
git add crates/rpyenv-core/src/conda_owner.rs crates/rpyenv-core/src/lib.rs crates/rpyenv-core/src/shimset.rs crates/rpyenv-core/tests/conda_owner_win.rs crates/rpyenv-core/Cargo.toml
git commit -m "M7c task 7: Windows conda shims by package ownership, upstream's default.list as second filter"
```

---

### Task 8: Install on Windows (`winconda.rs`), list, resolve, `latest -k`

**Files:**
- Create: `crates/pyenv/src/install/winconda.rs`
- Create: `crates/pyenv/tests/fake/fake_conda_installer.rs` and a `[[bin]]` entry in `crates/pyenv/Cargo.toml`
- Modify: `crates/pyenv/src/install/mod.rs` (`pub mod winconda;`)
- Modify: `crates/pyenv/src/commands/install_win.rs` (resolve, list, dispatch)
- Modify: `crates/pyenv/src/commands/latest.rs` (`latest_win`)
- Test: `crates/pyenv/tests/cli_install_conda_win.rs` (`#![cfg(windows)]`), unit tests in `winconda.rs`

**Interfaces:**
- Consumes: `conda::{windows_table, WinTable, Verdict, Install, latest_source, newest_installable, family, is_conda_name, WIN_ARCH}`, `txn::Txn::{begin_for, place_empty, target, commit}`, `winpkg::{fetch_package, Package, Done}`, `rpyenv_core::venv::conda_win_dirs`, `rpyenv_core::winproc::Job` (Windows only), `install::{interrupted, InstallError, is_plain_name}`.
- Produces:
  - `pub struct CondaJob<'a> { pub root: &'a Path, pub name: &'a str, pub force: bool, pub fetcher: &'a Fetcher, pub table: &'a WinTable }`
  - `pub fn install_conda(job: &CondaJob, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError>`
  - `pub fn refused_char(path: &Path) -> Option<char>`
  - `pub fn resolve_name(rows: &[LinuxRow], wanted: &str) -> String`
  - `pub fn installer_args(target: &Path) -> Vec<String>`

- [ ] **Step 1: The fake installer.** `crates/pyenv/tests/fake/fake_conda_installer.rs`, a multi-call program chosen by its own file name, modelled on Task 1's `installer-tree.txt` (read it: create the same top-level folders and, at least, every file this list names under `conda-meta/` for `python` and `conda`, empty `envs\`, `Library\bin\`, `Scripts\`):

```rust
//! A fake conda installer for M7c's Windows tests: by its own file name it is
//! - `*-Windows-x86_64.exe`: the NSIS installer. Checks `/S` and that `/D=` is the last
//!   argument; appends `args: …`, `marker: yes|no` (`<D>\.rpyenv-incomplete`), `tos: …` to
//!   `%FAKECONDA_LOG%`; exits `%FAKECONDA_EXIT%` before writing anything when set; else
//!   writes the tree (Task 1's measurement): `python.exe` and `Scripts\conda.exe` (copies of
//!   itself), `conda-meta\` records for python and conda, empty `envs\` and `Library\bin\`.
//! - `python.exe`: logs `python: <args>`, exits `%FAKECONDA_PYTHON_EXIT%` (default 0).
//! - `conda.exe`: `--version` prints `conda 25.1.1`; `install …` logs `conda: <args>`, exits
//!   `%FAKECONDA_INSTALL_EXIT%` (default 0).
//! - with `%FAKECONDA_SLEEP%` (seconds), the installer sleeps after writing (Ctrl+C tests).
use std::io::Write;

fn log(line: &str) {
    if let Some(p) = std::env::var_os("FAKECONDA_LOG") {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
        writeln!(f, "{line}").unwrap();
    }
}

fn code(var: &str) -> i32 {
    std::env::var(var).ok().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn main() {
    let me = std::env::current_exe().unwrap();
    let name = me.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if name == "python.exe" {
        log(&format!("python: {}", args.join(" ")));
        std::process::exit(code("FAKECONDA_PYTHON_EXIT"));
    }
    if name == "conda.exe" {
        if args.first().map(String::as_str) == Some("--version") {
            println!("conda 25.1.1");
            return;
        }
        log(&format!("conda: {}", args.join(" ")));
        std::process::exit(code("FAKECONDA_INSTALL_EXIT"));
    }
    // The installer.
    let d = args.last().and_then(|a| a.strip_prefix("/D=")).map(std::path::PathBuf::from);
    let Some(d) = d.filter(|_| args.first().map(String::as_str) == Some("/S")) else {
        log(&format!("bad args: {}", args.join(" ")));
        std::process::exit(2);
    };
    log(&format!("args: {}", args.join(" ")));
    log(&format!("marker: {}", if d.join(".rpyenv-incomplete").exists() { "yes" } else { "no" }));
    log(&format!("tos: {}", std::env::var("CONDA_PLUGINS_AUTO_ACCEPT_TOS").unwrap_or_else(|_| "unset".into())));
    if std::env::var_os("FAKECONDA_EXIT").is_some() {
        std::process::exit(code("FAKECONDA_EXIT"));
    }
    for sub in ["conda-meta", "envs", "Library\\bin", "Scripts"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    std::fs::copy(&me, d.join("python.exe")).unwrap();
    std::fs::copy(&me, d.join("Scripts").join("conda.exe")).unwrap();
    std::fs::write(d.join("conda-meta").join("python-3.12.0-0.json"), r#"{"name":"python","version":"3.12.0","files":["python.exe"]}"#).unwrap();
    std::fs::write(d.join("conda-meta").join("conda-25.1.1-0.json"), r#"{"name":"conda","version":"25.1.1","files":["Scripts/conda.exe","Lib/site-packages/conda/__init__.py"]}"#).unwrap();
    std::fs::write(d.join("conda-meta").join("history"), "+install\n").unwrap();
    if let Some(s) = std::env::var("FAKECONDA_SLEEP").ok().and_then(|v| v.parse().ok()) {
        std::thread::sleep(std::time::Duration::from_secs(s));
    }
}
```

`Cargo.toml`:
```toml
# Test-only multi-call fake of a conda NSIS installer (M7c plan Task 8); never packaged:
# the MSI and tarball name their files.
[[bin]]
name = "fake-conda-installer"
path = "tests/fake/fake_conda_installer.rs"
test = false
doc = false
```

- [ ] **Step 2: Failing CLI tests** in `crates/pyenv/tests/cli_install_conda_win.rs`. Shared helpers: a server (`common::server::start`) serving the fake installer's bytes at `/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe`; a table folder for `RPYENV_TEST_CONDA_TABLES` with `windows.tsv` = `# generated 2026-10-16T09:30:00Z by ci` + rows pointing at the server with `parse_windows(…, false)` accepted URLs and the fake's SHA-256 (`common::fakepypy::sha256`); a `.versions_cache.xml` written as `cli_install_win.rs` does (read it) so `install` passes its database check; env `RPYENV_TEST_PYTHON_ORG` (closes the network), `FAKECONDA_LOG`. Tests:

```rust
#[test] fn a_conda_name_installs_in_place_with_the_silent_flags() {}
// exit 0; versions\miniforge3-26.7.2-0\python.exe exists; no .rpyenv-incomplete left;
// log has "args: /S /InstallationType=JustMe /AddToPath=0 /RegisterPython=0 /NoRegistry=1 /NoShortcuts=1 /D=<target>"
// and "marker: yes"; log has "conda: install --yes pip conda=25.1.1" (step miniconda);
// log has `python: -c import sys, ssl`; shims\python.exe and shims\conda.exe exist.

#[test] fn a_tos_row_sets_the_variable_and_prints_the_notice() {}
// row with CONDA_PLUGINS_AUTO_ACCEPT_TOS=true: log "tos: true"; stdout has TERMS_NOTICE's first words.

#[test] fn a_hash_mismatch_is_refused_and_leaves_nothing() {} // also asserts spec §4.4's extra line: "if the publisher replaced this installer, a later rpyenv release will carry the reviewed hash"
#[test] fn an_installer_failure_deletes_the_folder() {}          // FAKECONDA_EXIT=3
#[test] fn a_failed_import_check_deletes_the_folder() {}         // FAKECONDA_PYTHON_EXIT=1
#[test] fn force_reinstall_keeps_envs() {}                       // envs\e1\x before, -f, still there
#[test] fn an_unsupported_row_is_refused_with_the_newest_hint() {}
#[test] fn a_root_with_a_refused_character_is_refused_before_download() {} // Review focus 1: root under "<tmp>\a b"; server hit count for the installer is 0; message names ' ' and the path
#[test] fn a_bare_family_installs_its_newest_installable_release() {}       // Review focus 2: `install miniforge3` with 26.7.2-0 and 4.9.2-7 rows → 26.7.2-0
#[test] fn install_list_shows_installable_conda_names_after_cpython() {}
#[test] fn latest_known_resolves_a_conda_family() {}            // `latest -k miniconda3` → newest by release
#[test] fn no_table_names_pyenv_update() {}
```

Write every body in full in the file's style; each asserts the exact output line(s) it names.

- [ ] **Step 3: Run.** `cargo test -p pyenv --test cli_install_conda_win` → FAIL.

- [ ] **Step 4: Implement `winconda.rs`.**

```rust
//! Anaconda, Miniconda and Miniforge on Windows (M7c design §5): the publisher-checked
//! installer, run silently into `versions\<name>` (conda can't be moved after installing),
//! pip as upstream's miniconda step, and an import check with conda's PATH.

use super::conda::{self, LinuxRow, Verdict, WinTable};
use super::fetch::Fetcher;
use super::txn::Txn;
use super::winpkg::{fetch_package, Done, Package};
use super::{interrupted, InstallError};
use rpyenv_core::flavor::Flavor;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

/// Characters the installers refuse in `/D=` (M7c Task 1, spec §5 step 2).
const REFUSED: &[char] = &[/* Task 1's measured set, e.g. ' ', '^', '%', '!', '=' */];

/// The first character of `path` an installer refuses: anything outside ASCII, or one of
/// `REFUSED` (per Task 1's measurement).
pub fn refused_char(path: &Path) -> Option<char> {
    path.to_string_lossy().chars().find(|c| !c.is_ascii() || REFUSED.contains(c))
}

pub fn installer_args(target: &Path) -> Vec<String> {
    ["/S", "/InstallationType=JustMe", "/AddToPath=0", "/RegisterPython=0", "/NoRegistry=1", "/NoShortcuts=1"]
        .iter().map(|s| s.to_string())
        .chain(std::iter::once(format!("/D={}", target.display())))
        .collect()
}

/// An exact name, else a bare family's newest installable row (design §6.1).
pub fn resolve_name(rows: &[LinuxRow], wanted: &str) -> String {
    if rows.iter().any(|r| r.name == wanted) || conda::family(wanted) != wanted {
        return wanted.to_string();
    }
    conda::newest_installable(rows, wanted, conda::WIN_ARCH).unwrap_or_else(|| wanted.to_string())
}

/// Waits for `child`, polling `stop` every 200 ms; when it turns true the child (and, on
/// Windows, its process tree through the job) is ended and `None` returned.
pub(crate) fn wait_or_kill(child: &mut Child, stop: &dyn Fn() -> bool) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(Some(s));
        }
        if stop() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn path_env(prefix: &Path) -> std::ffi::OsString {
    let mut p = std::ffi::OsString::new();
    for d in rpyenv_core::venv::conda_win_dirs(prefix) {
        p.push(d.as_os_str());
        p.push(";");
    }
    if let Some(sys) = std::env::var_os("PATH") {
        p.push(sys);
    }
    p
}

pub struct CondaJob<'a> {
    pub root: &'a Path,
    pub name: &'a str,
    pub force: bool,
    pub fetcher: &'a Fetcher,
    pub table: &'a WinTable,
}

pub fn install_conda(job: &CondaJob, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    match install_one(job, say) {
        Err(_) if interrupted() => Err(InstallError::Interrupted),
        r => r,
    }
}

fn install_one(job: &CondaJob, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    let msg = InstallError::Message;
    let name = job.name;
    if !super::is_plain_name(name) {
        return Err(msg(format!("invalid version name: {name}")));
    }
    let rows = &job.table.rows;
    let verdict = conda::linux_verdict(rows, name, conda::WIN_ARCH)
        .ok_or_else(|| msg(format!("definition not found: {name}")))?;
    let install = match verdict {
        Verdict::Ok(i) => i.clone(),
        Verdict::OkLatest(i) => conda::latest_source(name, i, conda::WIN_ARCH, job.fetcher).map_err(msg)?,
        Verdict::NoBinary(_) => return Err(msg(format!("rpyenv cannot install {name}: the publisher has no Windows x64 installer for it"))),
        Verdict::Unsupported(why) => {
            let hint = conda::newest_installable(rows, name, conda::WIN_ARCH)
                .map(|n| format!("; the newest {} rpyenv can install here is {n}", conda::family(name)))
                .unwrap_or_default();
            return Err(msg(format!("rpyenv cannot install this conda definition ({why}){hint}")));
        }
    };
    let versions = job.root.join("versions");
    let target = versions.join(name);
    if let Some(c) = refused_char(&target) {
        return Err(msg(format!("conda's installer can't install into {} (it refuses the character {c:?}); set PYENV_ROOT to a path without it", target.display())));
    }
    if super::txn::is_complete_for(&target, Flavor::PyenvWin) && !job.force {
        return Ok(Done::Skipped);
    }
    let mut txn = Txn::begin_for(&versions, name, Flavor::PyenvWin).map_err(msg)?;
    let (url, sha) = install.url.split_once('#').ok_or_else(|| msg(format!("{name}: no SHA-256")))?;
    let file = format!("{}.exe", install.package);
    // Spec §4.4 (D8): a hash failure gets one more line. Match the mismatch error text
    // `fetch` produces (read fetch.rs's Check::Sha256 failure message) rather than every error.
    let fetched = fetch_package(
        &Package::Zip { file, url: url.to_string(), sha256: sha.to_string() },
        name, &job.root.join("install_cache"), job.fetcher,
        &mut |from, to| {
            say(&format!(":: [Downloading] ::  {name} ..."));
            say(&format!(":: [Downloading] ::  From {from}"));
            say(&format!(":: [Downloading] ::  To   {}", to.display()));
        },
    )
    .map_err(|e| match e {
        InstallError::Message(m) if is_hash_mismatch(&m) => msg(format!("{m}\nif the publisher replaced this installer, a later rpyenv release will carry the reviewed hash")),
        e => e,
    })?;
    if install.env.iter().any(|(k, _)| k == "CONDA_PLUGINS_AUTO_ACCEPT_TOS") {
        say(crate::commands::install::TERMS_NOTICE);
    }
    say(&format!(":: [Installing] ::  {name} ..."));
    txn.place_empty().map_err(|e| msg(format!("cannot prepare {}: {e}", target.display())))?;
    let mut cmd = Command::new(&fetched.files[0]);
    cmd.args(installer_args(&target)).stdin(Stdio::null());
    for (k, v) in &install.env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| msg(format!("cannot run {}: {e}", fetched.files[0].display())))?;
    #[cfg(windows)]
    let _job = rpyenv_core::winproc::Job::new().filter(|j| j.assign(&child));
    let status = wait_or_kill(&mut child, &interrupted).map_err(|e| msg(e.to_string()))?
        .ok_or(InstallError::Interrupted)?;
    if !status.success() {
        return Err(msg(format!("{name}: the installer exited with {status}")));
    }
    let run = |exe: &Path, args: &[&str]| -> Result<std::process::Output, InstallError> {
        let mut c = Command::new(exe);
        c.args(args).env("PATH", path_env(&target)).stdin(Stdio::null());
        for (k, v) in &install.env {
            c.env(k, v);
        }
        c.output().map_err(|e| msg(format!("cannot run {}: {e}", exe.display())))
    };
    if install.step == "miniconda" {
        let conda_exe = target.join("Scripts").join("conda.exe");
        let out = run(&conda_exe, &["--version"])?;
        let v = String::from_utf8_lossy(&out.stdout).split_whitespace().nth(1).map(str::to_string)
            .ok_or_else(|| msg(format!("{} --version gave no version", conda_exe.display())))?;
        let out = run(&conda_exe, &["install", "--yes", "pip", &format!("conda={v}")])?;
        if !out.status.success() {
            return Err(msg(format!("{name}: conda install pip failed ({})", out.status)));
        }
    }
    let out = run(&target.join("python.exe"), &["-c", "import sys, ssl"])?;
    if !out.status.success() {
        return Err(msg(format!("{name}: python.exe can't import ssl with conda's PATH; the install is broken")));
    }
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    txn.commit().map_err(|e| msg(format!("cannot finish installing {name}: {e}")))?;
    say(&format!(":: [Info] :: completed! {name}"));
    Ok(Done::Installed)
}
```

`TERMS_NOTICE` in `commands/install.rs` becomes `pub const`. `install.rs` (Linux) is compiled on Windows? If it is `#[cfg(unix)]`, move `TERMS_NOTICE` to `conda.rs` and use it from both.

Unit tests in `winconda.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_args_put_d_last() {
        let a = installer_args(Path::new("C:\\r\\versions\\m"));
        assert_eq!(a.last().unwrap(), "/D=C:\\r\\versions\\m");
        assert_eq!(a[0], "/S");
    }
    #[test]
    fn refused_char_finds_non_ascii() {
        assert_eq!(refused_char(Path::new("C:\\Users\\José\\x")), Some('é'));
        assert_eq!(refused_char(Path::new("C:\\Users\\jm\\x")), None);
    }
    #[test]
    fn wait_or_kill_ends_a_child_when_asked() {
        let mut c = Command::new(if cfg!(windows) { "ping" } else { "sleep" })
            .args(if cfg!(windows) { &["-n", "30", "127.0.0.1"][..] } else { &["30"][..] })
            .stdout(Stdio::null()).spawn().unwrap();
        let t = std::time::Instant::now();
        assert!(wait_or_kill(&mut c, &|| true).unwrap().is_none());
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
    }
}
```

- [ ] **Step 5: Wire `install_win.rs`.** After `read_db`:
  - `let conda_table = || crate::install::conda::windows_table(&ctx.root);` read once, lazily, into an `Option<Result<WinTable, String>>`; print each `warnings` line once.
  - Argument loop: `v if conda::is_conda_name(v) => wanted.push(winconda::resolve_name(&rows_of_table, v))` (an `Err` table keeps `v` as is).
  - `-l`: after the CPython codes, the table's `Ok`/`OkLatest` names in table order (a missing table adds nothing).
  - `missing` check: skip conda names; for a conda name with no table, print the table error and fail; with no row, the existing `definition not found` text.
  - Dispatch first: `if conda::is_conda_name(w) { winconda::install_conda(&CondaJob { root: &ctx.root, name: w, force: o.force, fetcher: &fetcher, table }, &mut |l| say(l)) } else if pypy…`.
  - The `only CPython and PyPy are supported` message becomes `only CPython, PyPy and conda are supported`.
- [ ] **Step 6: Wire `latest_win`:** when `known && conda::is_conda_name(prefix)`: table rows → `resolve_name`; print it when a row with that name is `Ok`/`OkLatest`, else the existing "no known versions match" line and exit 1.
- [ ] **Step 7: Run.** `cargo test -p pyenv --test cli_install_conda_win` → PASS; `cargo test -p pyenv --lib winconda` → PASS; `cargo test -p pyenv` (whole crate) → PASS; `cargo clippy --workspace --all-targets -- -D warnings`; WSL `cargo test` → PASS.
- [ ] **Step 8: Commit.**
```bash
git add crates/pyenv/src/install/winconda.rs crates/pyenv/src/install/mod.rs crates/pyenv/src/commands/install_win.rs crates/pyenv/src/commands/latest.rs crates/pyenv/src/commands/install.rs crates/pyenv/src/install/conda.rs crates/pyenv/tests/fake crates/pyenv/tests/cli_install_conda_win.rs crates/pyenv/Cargo.toml
git commit -m "M7c task 8: install conda on Windows silently in place; list, bare family names, latest -k"
```

---

### Task 9: Ship the table in the MSI

**Files:**
- Modify: `installer/rpyenv.wxs` (a `share\conda` folder and one component)
- Modify: `installer/test_msi.py` (assert the file after a just-for-me and an all-users install)
- Modify: `installer/test_msi_tables.ps1` if it counts files or components (read it)
- Modify: `installer/sandbox/checks.ps1` and `CHECKLIST.md` (a check that `pyenv install --list` shows conda names)

- [ ] **Step 1: Failing test.** In `test_msi.py`, next to `check((USER_DIR / "completions" / "pyenv.pwsh").is_file(), "completions installed")`, add `check((USER_DIR / "share" / "conda" / "windows.tsv").is_file(), "conda table installed")`, and the same for `MACHINE_DIR` where `pyenv.exe in Program Files` is checked. Build the MSI as `build.yml` does (`./installer/build.ps1 -Arch x64 -BinDir target/release -OutDir dist`) after `cargo build --release --locked -p pyenv -p pyenv-shim -p pyenv-shimw`. `test_msi.py` installs MSIs: run it **only in CI or the Sandbox**, never on the development machine (Global Constraints). Locally, check the table instead: `pwsh -NoProfile -File installer/test_msi_tables.ps1 dist\rpyenv-*-x64.msi` and an `msiinfo`/`Orca`-free check via the `msi` crate is not needed; the CI run is the test.
- [ ] **Step 2: Implement** in `rpyenv.wxs`:
```xml
      <Directory Id="INSTALLFOLDER" Name="rpyenv">
        <Directory Id="BinFolder" Name="bin" />
        <Directory Id="CompletionsFolder" Name="completions" />
        <Directory Id="ShareFolder" Name="share">
          <Directory Id="ShareCondaFolder" Name="conda" />
        </Directory>
      </Directory>
```
and in `ComponentGroup Id="Files"`: `<Component Directory="ShareCondaFolder"><File Source="$(RepoDir)\crates\pyenv\conda\windows.tsv" /></Component>`.
- [ ] **Step 3: Sandbox check.** In `installer/sandbox/checks.ps1`, in the just-for-me step after the install checks: run `pyenv install --list` with a scratch `PYENV_ROOT` that has a `.versions_cache.xml` copied from the fixture the other checks use (read the file for how it sets up roots), and PASS when a line matches `^miniforge3-\d`. Add the step to `CHECKLIST.md`.
- [ ] **Step 4: Run** `pwsh -NoProfile -File installer/sandbox/run-checks.ps1 -Msi dist\rpyenv-<v>-x64.msi -Steps just-for-me -Stop` → the new check PASSES, the others still PASS.
- [ ] **Step 5: Commit.**
```bash
git add installer/rpyenv.wxs installer/test_msi.py installer/test_msi_tables.ps1 installer/sandbox/checks.ps1 installer/sandbox/CHECKLIST.md
git commit -m "M7c task 9: the MSI installs share\\conda\\windows.tsv; Sandbox checks conda names are listed"
```

---

### Task 10: Real installs in CI (`install-windows.yml`)

**Files:**
- Modify: `.github/workflows/install-windows.yml` (a `conda` job; read the file and `ci/check_install_win.py` first)
- Create: `ci/check_conda_win.py` (+ `ci/test_check_conda_win.py` for its pure helpers)

- [ ] **Step 1: The job.** `windows-latest`, x64. Steps: checkout; Rust; `cargo build --release -p pyenv -p pyenv-shim -p pyenv-shimw`; copy `crates/pyenv/conda/windows.tsv` to `target/share/conda/windows.tsv` (the release binary finds `<exe>\..\..\share\conda\windows.tsv` = `target\share\conda\windows.tsv`); scratch `PYENV`, `PYENV_ROOT`, `PYENV_HOME`, `HOME` under `$RUNNER_TEMP`; `pyenv update` (CPython cache and a fresh conda table: also exercises §4.3 live); then `python ci/check_conda_win.py <name>` for each of: the newest numbered `miniforge3`, the newest numbered `miniconda3` (ToS path), `miniforge3-latest`. Names come from `pyenv latest -k miniforge3` / `miniconda3`, so the job follows the table.
- [ ] **Step 2: `ci/check_conda_win.py <name>`** runs, failing with the step that failed:
  1. `pyenv install <name>`; exit 0; `versions\<name>\python.exe` and `conda-meta\` exist.
  2. Shims: `shims\python.exe`, `shims\pip.exe`, `shims\conda.exe` exist; `shims\cygpath.exe` and `shims\curl.exe` don't.
  3. `pyenv shell <name>` equivalent (`PYENV_VERSION=<name>`): `conda create --yes -n ci-env python` through `shims\conda.exe`; then `PYENV_VERSION=<name>/envs/ci-env` and `shims\python.exe -c "import sys, ssl; print(sys.prefix)"` prints the env's folder.
  4. `pyenv install -f <name>`; `envs\ci-env\python.exe` still exists.
  5. `pyenv uninstall -f <name>`; the folder is gone.
  Helpers that parse output (`prefix_matches(expected, printed)`) get unit tests in `ci/test_check_conda_win.py` (`python -m pytest ci/test_check_conda_win.py`).
- [ ] **Step 3: Run the helpers' tests locally** (`python -m pytest ci/test_check_conda_win.py`) → PASS. The job itself runs on the PR.
- [ ] **Step 4: Commit.**
```bash
git add .github/workflows/install-windows.yml ci/check_conda_win.py ci/test_check_conda_win.py
git commit -m "M7c task 10: CI installs Miniforge, Miniconda and miniforge3-latest on Windows for real"
```

---

### Task 11: Allowlist, README, parent spec

**Files:**
- Modify: `parity/allowlist.py` (entries from D-110; read D-107 to D-109 for the shape)
- Modify: `README.md` (Windows conda section)
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` (§14 M7c line, §15.1 resolution 1 note)

- [ ] **Step 1: Allowlist entries**, one per Windows conda behavior pyenv-win lacks: D-110 conda names in `install --list`/`install`/`latest -k`; D-111 `pyenv update` writes `.conda_versions.tsv` and its output lines; D-112 conda child `PATH`; D-113 ownership shims; D-114 `Library\bin` lookups and the env `conda` fallback; D-115 conda envs as `activate`/`base_prefix` targets on Windows. Each with the spec section it comes from.
- [ ] **Step 2: Run** `python parity/coverage.py` → 100% (the count rises with the new entries; every entry referenced).
- [ ] **Step 3: README:** under the conda bullet, a Windows paragraph: names as on Linux; `pyenv update` refreshes the conda list; the trust note (spec §4.3's last paragraph); the path-character limit (Task 1's set); §6.4's limits (`conda activate`, `activate.d`, `conda install` without `-n` targets the base).
- [ ] **Step 4: Parent spec:** §14 M7c points at this spec; §15.1 resolution 1 gets "For conda on Windows, superseded by M7c (2026-10-16): a shipped, updatable table rebuilt from the publishers; hashes pinned (spec D2-D4)".
- [ ] **Step 5: Full checks.** Windows: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. WSL (`wsl -d Debian --exec /usr/bin/bash -l /mnt/c/tmp/<x>.sh`, a script that runs fmt, clippy and test with scratch roots): all pass.
- [ ] **Step 6: Commit.**
```bash
git add parity/allowlist.py README.md docs/specs/2026-09-27-rpyenv-design.md
git commit -m "M7c task 11: allowlist D-110 to D-115, README Windows conda, parent spec pointers"
```
