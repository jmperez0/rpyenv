# M6b Packaging and Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship rpyenv v0.1.0 as a dual-mode Windows MSI (x64, ARM64) and Linux tarballs with an `install.sh`, built, tested and drafted by a tag-triggered release workflow.

**Architecture:** WiX 5.0.2 (a pinned local dotnet tool) builds `installer/rpyenv.wxs` into one MSI per Windows architecture; its custom actions only call `pyenv.exe` (`setup`, `migrate`, `setup --undo`, `migrate --restore`). Linux builds are static musl binaries packed into tarballs that a POSIX `install.sh` verifies and installs into `$PYENV_ROOT`. A reusable workflow (`build.yml`) builds and verifies every asset; `packaging.yml` runs it on pull requests and `release.yml` runs it on `v*` tags, then drafts a release with checksums and build attestations.

**Tech Stack:** Rust 1.96, WiX Toolset 5.0.2 (MS-RL) via `.config/dotnet-tools.json`, PowerShell 7, POSIX sh, Python 3 stdlib (`unittest`, `winreg`, `struct`), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-10-m6b-packaging-design.md` (approved 2026-10-10). Parent: `docs/specs/2026-09-27-rpyenv-design.md` §9.4.

## Global Constraints

- Toolchain 1.96 (`rust-toolchain.toml`); builds use `--locked`.
- WiX **5.0.2** exactly (MS-RL). Never WiX 6 (Open Source Maintenance Fee EULA).
- Asset names exactly: `rpyenv-<version>-x64.msi`, `rpyenv-<version>-arm64.msi`, `rpyenv-<version>-linux-x64.tar.gz`, `rpyenv-<version>-linux-arm64.tar.gz`, `install.sh`, `SHA256SUMS`. `<version>` is `[workspace.package] version` in `Cargo.toml` (`0.1.0`).
- MSI `UpgradeCode`: `A3154079-CE28-4416-B9A5-9FDB628BA522`. Active Setup key: `{73C5FAF3-A93A-472F-BD99-814909823229}`. Never change either.
- Actions pinned by commit SHA:
  - `actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5`
  - `dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02`
  - `actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2`
  - `actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2`
  - `actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2`
  - `actions/setup-dotnet@a98b56852c35b8e3190ac28c8c2271da59106c68 # v6.0.0`
  - `actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0`
- **Never install the MSI or run `install.sh` against the development host's real environment.**
  - MSI installs run only on CI runners (`installer/test_msi.py` refuses unless `CI=true`) or in Windows Sandbox (Task 8).
  - `install.sh` tests always use a throwaway `HOME` and `PYENV_ROOT`. The real `/home/jm/.pyenv` and `C:\Users\JM\.pyenv` are never touched.
- Building an MSI (`wix build`) and reading its tables are safe on the host.
- Python files: `encoding='utf-8'` on every text `open()`/`read_text()`/`write_text()`. Run with `python` on Windows and `python3` on Linux.
- WSL: only `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/<script>.sh`. Never put `$VARS` in an inline `bash -c` string.
- Commits: a one-line `-m`, or `-F C:\tmp\cm_<random>.txt`. No attribution lines. Never chain unrelated commands.
- **Ask the user before the first push** of `m6b-packaging` and before opening its pull request (Task 7). The user merges.

## Plan decisions (made while planning)

- **P1:** A small custom UI (`WelcomeDlg` → `OptionsDlg` → `VerifyReadyDlg`), not `WixUI_Advanced`.
  - WiX 5.0.2's `WixUI_Advanced.wxs` marks itself experimental ("Clicking Install doesn't work! … Use at your own risk").
  - It also installs per-user to `[LocalAppDataFolder]Apps\…` and per-machine to the 32-bit `[ProgramFilesFolder]`, both contrary to design §4.2.
  - The scope radio and both checkboxes live on `OptionsDlg`.
- **P2:** The exit page always shows "If its shims aren't on your PATH there, run: pyenv setup". A deferred custom action's result can't reach the UI without extra machinery (design §4.4 said "if setup fails").
- **P3:** The signing step is `installer/sign.ps1`. It is a no-op that prints "unsigned" unless `RPYENV_SIGN_TOOL` is set. Provider-specific code comes with the provider decision (design D3).
- **P4:** Design §10 risk 4 is corrected. `ci/shim_deps.py` checks crates, not DLL imports, so the static CRT is proven by a new `ci/pe_imports.py --check`.
- **P5:** `install.sh` tests use stdlib `unittest`, the repository's Python test style. Nothing to `pip install`.
- **P6:** The scope refusal (design §4.5) uses two error custom actions in the execute sequence, not `Launch` conditions. `LaunchConditions` also runs in the UI sequence while `ALLUSERS` is still 2, which would block an interactive all-users upgrade.
- **P7:** The upgrade test MSI is the workspace version with patch + 1, built by `build.ps1 -Version Next`.
- **P8:** The tarball has `bin/`, `completions/`, `LICENSE` and `README.md` at its root. `install.sh` installs only `bin/pyenv`, `bin/pyenv-shim` and the three completions.

## Review Focus

1. A Linux `PYENV_ROOT` containing spaces and non-ASCII letters. Install, upgrade and uninstall must quote every path. Pinned in Task 6 (`test_a_pyenv_root_with_spaces_and_non_ascii`).
2. A second take-over while `$PYENV_ROOT/.upstream-pyenv` exists. It must refuse before moving anything. Pinned in Task 6 (`test_a_second_take_over_is_refused`).
3. A major upgrade of a per-user MSI must not run the uninstall-time undo. The shims stay first on PATH, and the profile line and marker stay. Pinned in Task 4 (`upgrade`).
4. Uninstall with `bin\pyenv.exe` missing must still complete; the custom action's failure is ignored. Pinned in Task 4 (`missing_exe`).
5. `curl | sh` with no terminal (CI, containers) must never wait for an answer. Pinned in Task 6 (`test_without_a_terminal_it_prints_the_init_lines`, stdin closed, 60 s timeout) and in Task 7 (`ci/check_real_install.sh` on a runner without a TTY).

---

### Task 1: Static C runtime for Windows binaries

**Files:**
- Create: `ci/pe_imports.py`, `.cargo/config.toml`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `python ci/pe_imports.py --check <exe>...` exits 1 when any file imports `vcruntime*`/`msvcp*`; `--self-test` checks the parser on a synthetic PE.

- [ ] **Step 1: Write the checker with its self-test** — `ci/pe_imports.py`:

```python
"""CI guard (M6b design §7.1): Windows binaries link the C runtime statically.

Lists the DLLs a PE file imports. With --check, fails when any is the Visual C++
runtime (vcruntime*, msvcp*), which isn't on every machine (notably ARM64).
Usage: pe_imports.py --self-test | --check FILE... | FILE...
"""
import struct
import sys
from pathlib import Path

VC_RUNTIME = ("vcruntime", "msvcp")


def imports(data: bytes) -> list:
    """The imported DLL names, lowercased, in import-table order."""
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE file")
    coff = pe + 4
    (nsections,) = struct.unpack_from("<H", data, coff + 2)
    (opt_size,) = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    (magic,) = struct.unpack_from("<H", data, opt)
    directories = opt + (112 if magic == 0x20B else 96)
    (import_rva,) = struct.unpack_from("<I", data, directories + 8)
    sections = []
    for i in range(nsections):
        vsize, vaddr, rawsize, rawptr = struct.unpack_from(
            "<IIII", data, opt + opt_size + 40 * i + 8
        )
        sections.append((vaddr, max(vsize, rawsize), rawptr))

    def offset(rva):
        for vaddr, size, rawptr in sections:
            if vaddr <= rva < vaddr + size:
                return rawptr + rva - vaddr
        raise ValueError(f"RVA {rva:#x} is outside every section")

    names = []
    if import_rva == 0:
        return names
    at = offset(import_rva)
    while True:
        descriptor = struct.unpack_from("<5I", data, at)
        if descriptor == (0, 0, 0, 0, 0):
            return names
        start = offset(descriptor[3])
        names.append(data[start:data.index(b"\0", start)].decode("ascii").lower())
        at += 20


def vc_runtime(names) -> list:
    return [n for n in names if n.startswith(VC_RUNTIME)]


def synthetic_pe(dlls) -> bytes:
    """A minimal PE32+ file whose import table names `dlls` (one section at RVA 0x1000)."""
    data = bytearray(0x400)
    struct.pack_into("<I", data, 0x3C, 0x40)
    data[0x40:0x44] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x44, 0x8664, 1)
    struct.pack_into("<H", data, 0x44 + 16, 240)
    opt = 0x58
    struct.pack_into("<H", data, opt, 0x20B)
    struct.pack_into("<II", data, opt + 112 + 8, 0x1000, 20 * (len(dlls) + 1))
    section = opt + 240
    data[section:section + 8] = b".idata\0\0"
    struct.pack_into("<IIII", data, section + 8, 0x200, 0x1000, 0x200, 0x200)
    for i, dll in enumerate(dlls):
        name_rva = 0x1100 + 0x20 * i
        struct.pack_into("<5I", data, 0x200 + 20 * i, 0, 0, 0, name_rva, 0)
        start = 0x200 + name_rva - 0x1000
        data[start:start + len(dll)] = dll.encode("ascii")
    return bytes(data)


def self_test() -> None:
    names = imports(synthetic_pe(["KERNEL32.dll", "VCRUNTIME140.dll"]))
    assert names == ["kernel32.dll", "vcruntime140.dll"], names
    assert vc_runtime(names) == ["vcruntime140.dll"], vc_runtime(names)
    assert vc_runtime(imports(synthetic_pe(["KERNEL32.dll"]))) == []


def main(argv) -> int:
    if argv == ["--self-test"]:
        self_test()
        print("self-test ok")
        return 0
    check = argv[:1] == ["--check"]
    files = argv[1:] if check else argv
    if not files:
        print(__doc__, file=sys.stderr)
        return 2
    failed = False
    for f in files:
        names = imports(Path(f).read_bytes())
        bad = vc_runtime(names)
        print(f"{f}: {', '.join(names)}")
        if check and bad:
            print(f"{f} needs the Visual C++ runtime: {', '.join(bad)}")
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 2: Run the self-test**

Run: `python ci/pe_imports.py --self-test`
Expected: `self-test ok`.

- [ ] **Step 3: See today's binaries fail the check (RED)**

Run: `cargo build --workspace`, then `python ci/pe_imports.py --check target/debug/pyenv.exe target/debug/pyenv-shim.exe target/debug/pyenv-shimw.exe`.
Expected: exit 1, with a "needs the Visual C++ runtime: vcruntime140.dll" line for each binary.

- [ ] **Step 4: Link the C runtime statically** — create `.cargo/config.toml`:

```toml
# M6b design §7.1: no Windows binary may need vcruntime140.dll, which isn't on every
# machine (notably ARM64). Tests build the same way, so CI tests what ships.
[target.'cfg(all(windows, target_env = "msvc"))']
rustflags = ["-C", "target-feature=+crt-static"]
```

- [ ] **Step 5: Run the check again (GREEN)**

Run: `cargo build --workspace`, then the same `--check` command as Step 3.
Expected: exit 0. Each line lists system DLLs only (`kernel32.dll`, `ntdll.dll`, …).

- [ ] **Step 6: Guard it in CI.** In `.github/workflows/ci.yml`, add after `- run: cargo build --workspace`:

```yaml
      - name: Static C runtime (M6b design §7.1)
        if: startsWith(matrix.os, 'windows')
        run: python ci/pe_imports.py --check target/debug/pyenv.exe target/debug/pyenv-shim.exe target/debug/pyenv-shimw.exe
```

and add `python3 ci/pe_imports.py --self-test` as the first line of the existing `Shim dependency guard` step's `run:` block.

- [ ] **Step 7: Run the whole Windows suite with the static CRT.**

Run: `cargo test --workspace --no-fail-fast > C:\tmp\m6b_t1_suite.txt 2>&1`, then count the `test result: ok` lines.
Expected: 48 binaries ok. If an e2e console test fails, rerun it alone 3 times before ruling: these are known to be load-sensitive (memory `e2e-windows-test-windows`).

- [ ] **Step 8: Commit:** `git commit -m "Windows binaries link the C runtime statically, checked by ci/pe_imports.py"` (files: `ci/pe_imports.py`, `.cargo/config.toml`, `.github/workflows/ci.yml`).

---

### Task 2: `pyenv setup --undo`

**Files:**
- Modify: `crates/pyenv/src/commands/setup_win.rs`
- Test: `crates/pyenv/tests/cli_setup_win.rs`

**Interfaces:**
- Consumes: `pathlist::{without, same}`, `winenv::{get, set_user, broadcast, expand, documents, Scope, Value}`, `pwsh_profile::{paths, remove}`, `setup_win::{MARKER, FAILED_MARKER}`.
- Produces: `pyenv setup --undo`, and `setup_win::undo(&Ctx) -> Output`. Exit 0 on success or nothing to undo; exit 1 when a step fails.

- [ ] **Step 1: Write the failing tests.** Append to `crates/pyenv/tests/cli_setup_win.rs`; `reg_set`, `reg_get` and `reg_type` already exist there:

```rust
/// M6b design §5: `setup --undo` takes back what setup added for this user, keeps the
/// rest of the Path and PYENV_ROOT's data, and is safe to run twice.
#[test]
fn setup_undo_takes_back_the_path_entry_the_profile_line_and_the_marker() {
    let f = Fixture::new();
    reg_set(&f, "user", "Path", "REG_EXPAND_SZ", r"C:\Tools");
    assert_eq!(f.pyenv(&["setup"]).code, 0);
    let r = f.pyenv(&["setup", "--undo"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(reg_get(&f, "user", "Path"), r"REG_EXPAND_SZ C:\Tools");
    for sub in ["WindowsPowerShell", "PowerShell"] {
        let p = f
            .base
            .join("Documents")
            .join(sub)
            .join("Microsoft.PowerShell_profile.ps1");
        assert!(
            !std::fs::read_to_string(&p).unwrap_or_default().contains("pyenv init"),
            "{}",
            p.display()
        );
    }
    assert!(!f.root.join(".rpyenv-setup").exists());
    assert!(f.root.join("versions").is_dir(), "PYENV_ROOT's data stays");
    let again = f.pyenv(&["setup", "--undo"]);
    assert_eq!(again.code, 0, "{}{}", again.stdout, again.stderr);
}

/// Any spelling of the shims entry goes; other entries keep their order and the value
/// keeps its type.
#[test]
fn setup_undo_keeps_other_path_entries_in_order() {
    let f = Fixture::new();
    let spelled = format!("{}\\", f.root.join("shims").display()).to_uppercase();
    reg_set(&f, "user", "Path", "REG_SZ", &format!(r"C:\A;{spelled};C:\B"));
    let r = f.pyenv(&["setup", "--undo"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(reg_get(&f, "user", "Path"), r"REG_SZ C:\A;C:\B");
}

/// M6a I5 applies to undo too: a Path it can't read is left as it is, and undo fails.
#[test]
fn setup_undo_leaves_an_unreadable_path_alone_and_fails() {
    let f = Fixture::new();
    reg_set(&f, "user", "Path", "REG_DWORD", "1");
    let r = f.pyenv(&["setup", "--undo"]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("cannot read your user PATH"), "{}", r.stderr);
    assert!(reg_type(&f, "user", "Path").contains("REG_DWORD"));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p pyenv --test cli_setup_win -- setup_undo`
Expected: 3 FAILED. `setup --undo` is rejected today, printing the help with exit 1. So the first two tests fail on the exit code, and the third fails because "cannot read your user PATH" never appears (the help text mentions PATH, which is why the test checks for that exact phrase).

- [ ] **Step 3: Implement.** In `crates/pyenv/src/commands/setup_win.rs`, replace `HELP` and `setup`, and add `undo` after `run`:

```rust
pub const HELP: &str = "Usage: pyenv setup
       pyenv setup --undo

Prepares your Windows user for rpyenv: creates PYENV_ROOT, puts its shims folder first
on your user PATH, rehashes, and adds the PowerShell line to your profiles. It's safe to
run again.

--undo takes back what setup added for this user: the shims entry on your user PATH and
the PowerShell line. Your Python versions stay.
";

pub fn setup(ctx: &Ctx, args: &[&str]) -> Output {
    match args {
        [] => run(ctx),
        ["--undo"] => undo(ctx),
        ["--help"] => {
            let mut o = Output::new();
            o.stdout.push_str(HELP);
            o
        }
        _ => Output::error(HELP).with_code(1),
    }
}
```

```rust
/// M6b design §5: takes back what `run` added for this user (the MSI's uninstall runs
/// it). Never touches PYENV_ROOT's versions or the shims folder's files.
pub fn undo(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    let mut failed = false;
    let shims = ctx.shims_dir().display().to_string();
    match winenv::get(winenv::Scope::User, "Path") {
        // M6a I5: never write over a Path that couldn't be read.
        Err(e) => {
            o.err(format!(
                "pyenv: cannot read your user PATH ({e}); left it as it is"
            ));
            failed = true;
        }
        Ok(None) => {}
        Ok(Some(v)) => {
            let (text, gone) =
                pathlist::without(&v.text, &|e| pathlist::same(&winenv::expand(e), &shims));
            if !gone.is_empty() {
                match winenv::set_user(
                    "Path",
                    &winenv::Value {
                        text,
                        expand: v.expand,
                    },
                ) {
                    Ok(()) => {
                        o.out(format!("pyenv: took {shims} off your user PATH"));
                        winenv::broadcast();
                    }
                    Err(e) => {
                        o.err(format!("pyenv: cannot change your user PATH: {e}"));
                        failed = true;
                    }
                }
            }
        }
    }
    if let Some(docs) = winenv::documents() {
        for p in crate::commands::pwsh_profile::paths(&docs) {
            match crate::commands::pwsh_profile::remove(&p) {
                Ok(true) => o.out(format!(
                    "pyenv: took the PowerShell line out of {}",
                    p.display()
                )),
                Ok(false) => {}
                Err(e) => {
                    o.err(format!("pyenv: {}: {e}", p.display()));
                    failed = true;
                }
            }
        }
    }
    for marker in [MARKER, FAILED_MARKER] {
        match std::fs::remove_file(ctx.root.join(marker)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                o.err(format!("pyenv: cannot remove {marker}: {e}"));
                failed = true;
            }
        }
    }
    if failed {
        o.code = 1;
    }
    o
}
```

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_setup_win`.
Expected: all pass (the 10 existing tests + 3 new ones).

- [ ] **Step 5: Check what pins `setup`'s help.** Search the tests, the parity goldens and the e2e tests for `Prepares your Windows user`, and update any expected text to the new `HELP`. Run: `cargo test -p pyenv --test cli_dispatch --test cli_init_win`, and the Windows parity diff if a golden changed (`python parity/diff.py --rpyenv target/debug --upstream <fresh clone of C:/tmp/pyenv-win-856ed5a>/pyenv-win --only help`).
Expected: green. The `commands` list is unchanged (no new command).

- [ ] **Step 6: Commit:** `git commit -m "pyenv setup --undo takes back what setup added for this user: the shims Path entry, the PowerShell line and the markers"`.

---

### Task 3: The MSI source and its local build

**Files:**
- Create: `.config/dotnet-tools.json`, `installer/rpyenv.wxs`, `installer/build.ps1`, `installer/sign.ps1`, `installer/test_msi_tables.ps1`
- Modify: `.gitignore` (add `/.wix/`, `/dist/`, `/dist-next/`)

**Interfaces:**
- Produces:
  - `installer/build.ps1 -Arch x64|arm64 -BinDir <dir> [-OutDir dist] [-Version ''|Next|x.y.z]` → prints the path `<OutDir>\rpyenv-<version>-<Arch>.msi`;
  - `installer/sign.ps1 <file>...` (a no-op without `RPYENV_SIGN_TOOL`);
  - `installer/test_msi_tables.ps1 <msi>`;
  - MSI public properties `LIVEREHASH`, `MIGRATE`;
  - registry markers `HKCU\Software\rpyenv\Installed` and `HKLM\Software\rpyenv\Installed`.

- [ ] **Step 1: Write the table test first** — `installer/test_msi_tables.ps1`:

```powershell
# Checks an rpyenv MSI's tables (M6b design §4) without installing it: safe on any machine.
param([Parameter(Mandatory)] [string] $Msi)
$ErrorActionPreference = 'Stop'

$installer = New-Object -ComObject WindowsInstaller.Installer
function Call($Target, [string] $Member, [System.Reflection.BindingFlags] $Kind, $Arguments) {
    $Target.GetType().InvokeMember($Member, $Kind, $null, $Target, $Arguments)
}
$database = Call $installer 'OpenDatabase' InvokeMethod @((Resolve-Path $Msi).Path, 0)
function Rows([string] $Sql) {
    $view = Call $database 'OpenView' InvokeMethod @($Sql)
    Call $view 'Execute' InvokeMethod $null | Out-Null
    $rows = @()
    while ($record = Call $view 'Fetch' InvokeMethod $null) {
        $count = Call $record 'FieldCount' GetProperty $null
        $rows += , @(1..$count | ForEach-Object { Call $record 'StringData' GetProperty @($_) })
    }
    Call $view 'Close' InvokeMethod $null | Out-Null
    , $rows
}

$failures = [System.Collections.Generic.List[string]]::new()
function Expect([bool] $Ok, [string] $What) { if (-not $Ok) { $failures.Add($What) } }

$props = @{}
foreach ($r in (Rows 'SELECT `Property`, `Value` FROM `Property`')) { $props[$r[0]] = $r[1] }
Expect ($props['ALLUSERS'] -eq '2') 'ALLUSERS=2 (one package, both scopes)'
Expect ($props['MSIINSTALLPERUSER'] -eq '1') 'MSIINSTALLPERUSER=1 (just for me by default)'
Expect ($props['ARPNOMODIFY'] -eq '1') 'ARPNOMODIFY=1'

$files = (Rows 'SELECT `FileName` FROM `File`') | ForEach-Object { ($_[0] -split '\|')[-1] }
foreach ($f in 'pyenv.exe', 'pyenv-shim.exe', 'pyenv-shimw.exe', 'pyenv.pwsh', 'pyenv.bash', 'pyenv.zsh', 'pyenv.fish') {
    Expect ($files -contains $f) "file $f"
}

$environment = Rows 'SELECT `Name`, `Value` FROM `Environment`'
$paths = @($environment | Where-Object { $_[0] -match 'PATH$' })
Expect (@($paths | Where-Object { $_[0] -match '\*' }).Count -eq 1) 'bin on the machine PATH'
Expect (@($paths | Where-Object { $_[0] -notmatch '\*' }).Count -eq 1) 'bin on the user PATH'
Expect (@($environment | Where-Object { $_[0] -match 'RPYENV_LIVE_REHASH$' }).Count -eq 2) 'live rehash, both scopes'

$registry = Rows 'SELECT `Key`, `Name`, `Value` FROM `Registry`'
Expect (@($registry | Where-Object {
            $_[0] -eq 'SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}' -and
            $_[1] -eq 'StubPath' -and $_[2] -like '*pyenv.exe" setup' }).Count -eq 1) 'Active Setup StubPath'

$sequence = @{}
foreach ($r in (Rows 'SELECT `Action`, `Condition` FROM `InstallExecuteSequence`')) { $sequence[$r[0]] = $r[1] }
foreach ($a in 'RpyenvMigrate', 'RpyenvSetup', 'RpyenvRestore', 'RpyenvUndo', 'RefuseMachineOverUser', 'RefuseUserOverMachine') {
    Expect ($sequence.ContainsKey($a)) "action $a scheduled"
}
Expect ("$($sequence['RpyenvUndo'])" -like '*NOT UPGRADINGPRODUCTCODE*') 'undo skipped during upgrades'

$targets = (Rows 'SELECT `Action`, `Target` FROM `CustomAction`') | ForEach-Object { "$($_[1])" }
foreach ($c in '" migrate', '" setup', '" migrate --restore', '" setup --undo') {
    Expect (@($targets | Where-Object { $_ -like "*pyenv.exe$c" }).Count -ge 1) "command pyenv.exe$c"
}

$dialogs = (Rows 'SELECT `Dialog` FROM `Dialog`') | ForEach-Object { $_[0] }
Expect ($dialogs -contains 'OptionsDlg') 'the Options page'

if ($failures.Count) {
    $failures | ForEach-Object { Write-Host "missing: $_" }
    exit 1
}
Write-Host "MSI tables: ok ($Msi)"
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pwsh -NoProfile -File installer/test_msi_tables.ps1 dist/rpyenv-0.1.0-x64.msi`
Expected: an error, because the path doesn't exist (`Resolve-Path` fails).

- [ ] **Step 3: Pin WiX.** Create `.config/dotnet-tools.json`:

```json
{
  "version": 1,
  "isRoot": true,
  "tools": {
    "wix": {
      "version": "5.0.2",
      "commands": ["wix"],
      "rollForward": false
    }
  }
}
```

Append to `.gitignore`:

```
/.wix/
/dist/
/dist-next/
```

- [ ] **Step 4: Write the signing hook** — `installer/sign.ps1`:

```powershell
# Signs release files (M6b design D3, plan P3). v0.1.0 ships unsigned: without
# RPYENV_SIGN_TOOL this only says so. The chosen provider's command goes here.
param([Parameter(ValueFromRemainingArguments)] [string[]] $Files)
$ErrorActionPreference = 'Stop'
if (-not $env:RPYENV_SIGN_TOOL) {
    Write-Host "unsigned (no signing configured): $($Files -join ', ')"
    exit 0
}
throw "RPYENV_SIGN_TOOL is set to '$env:RPYENV_SIGN_TOOL', but no provider is implemented yet"
```

- [ ] **Step 5: Write the build script** — `installer/build.ps1`:

```powershell
# Builds rpyenv's MSI with the pinned WiX 5.0.2 (M6b design §4). Prints the MSI's path.
param(
    [Parameter(Mandatory)] [ValidateSet('x64', 'arm64')] [string] $Arch,
    [Parameter(Mandatory)] [string] $BinDir,
    [string] $OutDir = 'dist',
    # '' = the workspace version; 'Next' = its patch + 1 (the upgrade test, plan P7).
    [string] $Version = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$workspace = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"$').Matches[0].Groups[1].Value
if ($Version -eq '') { $Version = $workspace }
elseif ($Version -eq 'Next') {
    $parts = $workspace.Split('.')
    $Version = "$($parts[0]).$($parts[1]).$([int]$parts[2] + 1)"
}
$comma = $Version.Replace('.', ',')
$bin = (Resolve-Path $BinDir).Path
Push-Location $repo
try {
    dotnet tool restore | Out-Null
    if ($LASTEXITCODE) { throw "dotnet tool restore failed ($LASTEXITCODE)" }
    dotnet tool run wix extension add WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2 | Out-Null
    if ($LASTEXITCODE) { throw "wix extension add failed ($LASTEXITCODE)" }
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $out = Join-Path (Resolve-Path $OutDir).Path "rpyenv-$Version-$Arch.msi"
    dotnet tool run wix build installer/rpyenv.wxs -arch $Arch `
        -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
        -d "Version=$Version" -d "VersionComma=$comma" -d "BinDir=$bin" -d "RepoDir=$repo" `
        -o $out
    if ($LASTEXITCODE) { throw "wix build failed ($LASTEXITCODE)" }
    $out
}
finally { Pop-Location }
```

- [ ] **Step 6: Write the package** — `installer/rpyenv.wxs`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<!--
  rpyenv's MSI (M6b design §4), built by installer/build.ps1 with WiX 5.0.2. One package
  installs just for the current user (no administrator rights) or for all users (Windows
  asks for elevation): chosen on the Options page, or on a silent install with
  ALLUSERS=1 MSIINSTALLPERUSER="".
-->
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="rpyenv"
           Manufacturer="rpyenv"
           Version="$(Version)"
           UpgradeCode="A3154079-CE28-4416-B9A5-9FDB628BA522"
           Scope="perUserOrMachine">
    <SummaryInformation Description="rpyenv $(Version): pyenv in Rust" />
    <MajorUpgrade DowngradeErrorMessage="A newer version of rpyenv is already installed." />
    <MediaTemplate EmbedCab="yes" />
    <Property Id="ARPNOMODIFY" Value="1" />
    <Property Id="ARPURLINFOABOUT" Value="https://github.com/jmperez0/rpyenv" />

    <!-- Options (design §4.3), public so a silent install can set them. -->
    <Property Id="LIVEREHASH" Secure="yes" />
    <Property Id="MIGRATE" Secure="yes" />
    <Property Id="RPYENV_SCOPE" Value="user" />

    <!-- pyenv-win in this user's profile: the migrate checkbox shows, checked. -->
    <Property Id="PYENVWIN_FOUND">
      <DirectorySearch Id="PyenvWinBin" Path="[%USERPROFILE]\.pyenv\pyenv-win\bin" Depth="0">
        <FileSearch Name="pyenv.ps1" />
      </DirectorySearch>
    </Property>
    <SetProperty Id="MIGRATE" Value="1" After="AppSearch" Sequence="ui" Condition="PYENVWIN_FOUND AND NOT MIGRATE" />

    <!-- One install per machine (design §4.5, plan P6). -->
    <Property Id="RPYENV_USER_INSTALLED">
      <RegistrySearch Root="HKCU" Key="Software\rpyenv" Name="Installed" Type="raw" />
    </Property>
    <Property Id="RPYENV_MACHINE_INSTALLED">
      <RegistrySearch Root="HKLM" Key="Software\rpyenv" Name="Installed" Type="raw" Bitness="always64" />
    </Property>
    <CustomAction Id="RefuseMachineOverUser" Error="rpyenv is already installed just for you. Uninstall it first (Settings, Apps), then install it for all users." />
    <CustomAction Id="RefuseUserOverMachine" Error="rpyenv is already installed for all users of this computer. Uninstall that first, or keep using it." />

    <StandardDirectory Id="ProgramFiles6432Folder">
      <Directory Id="INSTALLFOLDER" Name="rpyenv">
        <Directory Id="BinFolder" Name="bin" />
        <Directory Id="CompletionsFolder" Name="completions" />
      </Directory>
    </StandardDirectory>

    <Feature Id="Main" Title="rpyenv">
      <ComponentGroupRef Id="Files" />
      <ComponentGroupRef Id="Environment" />
    </Feature>

    <ComponentGroup Id="Files" Directory="BinFolder">
      <Component><File Source="$(BinDir)\pyenv.exe" /></Component>
      <Component><File Source="$(BinDir)\pyenv-shim.exe" /></Component>
      <Component><File Source="$(BinDir)\pyenv-shimw.exe" /></Component>
      <Component Directory="CompletionsFolder"><File Source="$(RepoDir)\completions\pyenv.pwsh" /></Component>
      <Component Directory="CompletionsFolder"><File Source="$(RepoDir)\completions\pyenv.bash" /></Component>
      <Component Directory="CompletionsFolder"><File Source="$(RepoDir)\completions\pyenv.zsh" /></Component>
      <Component Directory="CompletionsFolder"><File Source="$(RepoDir)\completions\pyenv.fish" /></Component>
      <Component Directory="INSTALLFOLDER"><File Source="$(RepoDir)\LICENSE" Name="LICENSE.txt" /></Component>
      <Component Directory="INSTALLFOLDER"><File Source="$(RepoDir)\README.md" /></Component>
    </ComponentGroup>

    <ComponentGroup Id="Environment" Directory="INSTALLFOLDER">
      <!-- Just for me: bin on the user PATH; the HKCU marker is the key path. -->
      <Component Id="UserEnvironment" Condition="ALLUSERS&lt;&gt;1">
        <Environment Id="UserPath" Name="PATH" Value="[BinFolder]" Part="last" Action="set" System="no" />
        <RegistryValue Root="HKCU" Key="Software\rpyenv" Name="Installed" Type="integer" Value="1" KeyPath="yes" />
      </Component>
      <Component Id="UserLiveRehash" Condition="ALLUSERS&lt;&gt;1 AND LIVEREHASH=1">
        <Environment Id="UserLiveRehash" Name="RPYENV_LIVE_REHASH" Value="1" Action="set" System="no" />
        <RegistryValue Root="HKCU" Key="Software\rpyenv" Name="LiveRehash" Type="integer" Value="1" KeyPath="yes" />
      </Component>
      <!-- For all users: bin on the machine PATH, the HKLM marker, Active Setup. -->
      <Component Id="MachineEnvironment" Condition="ALLUSERS=1">
        <Environment Id="MachinePath" Name="PATH" Value="[BinFolder]" Part="last" Action="set" System="yes" />
        <RegistryValue Root="HKLM" Key="Software\rpyenv" Name="Installed" Type="integer" Value="1" KeyPath="yes" />
      </Component>
      <Component Id="MachineLiveRehash" Condition="ALLUSERS=1 AND LIVEREHASH=1">
        <Environment Id="MachineLiveRehash" Name="RPYENV_LIVE_REHASH" Value="1" Action="set" System="yes" />
        <RegistryValue Root="HKLM" Key="Software\rpyenv" Name="LiveRehash" Type="integer" Value="1" KeyPath="yes" />
      </Component>
      <Component Id="ActiveSetup" Condition="ALLUSERS=1">
        <RegistryKey Root="HKLM" Key="SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}">
          <RegistryValue Type="string" Value="rpyenv" KeyPath="yes" />
          <RegistryValue Name="StubPath" Type="string" Value="&quot;[BinFolder]pyenv.exe&quot; setup" />
          <RegistryValue Name="Version" Type="string" Value="$(VersionComma)" />
          <RegistryValue Name="IsInstalled" Type="integer" Value="1" />
        </RegistryKey>
      </Component>
    </ComponentGroup>

    <!-- Design §4.4: pyenv.exe, hidden, as the installing user; failures only reach the log. -->
    <SetProperty Id="RpyenvMigrate" Value="&quot;[BinFolder]pyenv.exe&quot; migrate" Before="RpyenvMigrate" Sequence="execute" />
    <CustomAction Id="RpyenvMigrate" BinaryRef="Wix4UtilCA_$(sys.BUILDARCHSHORT)" DllEntry="WixQuietExec" Execute="deferred" Impersonate="yes" Return="ignore" />
    <SetProperty Id="RpyenvSetup" Value="&quot;[BinFolder]pyenv.exe&quot; setup" Before="RpyenvSetup" Sequence="execute" />
    <CustomAction Id="RpyenvSetup" BinaryRef="Wix4UtilCA_$(sys.BUILDARCHSHORT)" DllEntry="WixQuietExec" Execute="deferred" Impersonate="yes" Return="ignore" />
    <SetProperty Id="RpyenvRestore" Value="&quot;[BinFolder]pyenv.exe&quot; migrate --restore" Before="RpyenvRestore" Sequence="execute" />
    <CustomAction Id="RpyenvRestore" BinaryRef="Wix4UtilCA_$(sys.BUILDARCHSHORT)" DllEntry="WixQuietExec" Execute="deferred" Impersonate="yes" Return="ignore" />
    <SetProperty Id="RpyenvUndo" Value="&quot;[BinFolder]pyenv.exe&quot; setup --undo" Before="RpyenvUndo" Sequence="execute" />
    <CustomAction Id="RpyenvUndo" BinaryRef="Wix4UtilCA_$(sys.BUILDARCHSHORT)" DllEntry="WixQuietExec" Execute="deferred" Impersonate="yes" Return="ignore" />

    <InstallExecuteSequence>
      <Custom Action="RefuseMachineOverUser" After="AppSearch" Condition="NOT Installed AND ALLUSERS=1 AND RPYENV_USER_INSTALLED" />
      <Custom Action="RefuseUserOverMachine" After="RefuseMachineOverUser" Condition="NOT Installed AND ALLUSERS&lt;&gt;1 AND RPYENV_MACHINE_INSTALLED" />
      <Custom Action="RpyenvMigrate" After="WriteEnvironmentStrings" Condition="NOT Installed AND ALLUSERS&lt;&gt;1 AND MIGRATE=1" />
      <Custom Action="RpyenvSetup" After="RpyenvMigrate" Condition="NOT Installed AND ALLUSERS&lt;&gt;1" />
      <Custom Action="RpyenvRestore" Before="RemoveEnvironmentStrings" Condition="REMOVE=&quot;ALL&quot; AND NOT UPGRADINGPRODUCTCODE" />
      <Custom Action="RpyenvUndo" After="RpyenvRestore" Condition="REMOVE=&quot;ALL&quot; AND NOT UPGRADINGPRODUCTCODE" />
    </InstallExecuteSequence>

    <!-- Plan P1: Welcome, Options (scope and checkboxes), Ready. -->
    <UI Id="RpyenvUI">
      <TextStyle Id="WixUI_Font_Normal" FaceName="Tahoma" Size="8" />
      <TextStyle Id="WixUI_Font_Bigger" FaceName="Tahoma" Size="12" />
      <TextStyle Id="WixUI_Font_Title" FaceName="Tahoma" Size="9" Bold="yes" />
      <Property Id="DefaultUIFont" Value="WixUI_Font_Normal" />

      <DialogRef Id="ErrorDlg" />
      <DialogRef Id="FatalError" />
      <DialogRef Id="FilesInUse" />
      <DialogRef Id="MsiRMFilesInUse" />
      <DialogRef Id="PrepareDlg" />
      <DialogRef Id="ProgressDlg" />
      <DialogRef Id="ResumeDlg" />
      <DialogRef Id="UserExit" />
      <DialogRef Id="WelcomeDlg" />
      <DialogRef Id="MaintenanceWelcomeDlg" />

      <Dialog Id="OptionsDlg" Width="370" Height="270" Title="[ProductName] Setup">
        <Control Id="BannerBitmap" Type="Bitmap" X="0" Y="0" Width="370" Height="44" TabSkip="no" Text="WixUI_Bmp_Banner" />
        <Control Id="BannerLine" Type="Line" X="0" Y="44" Width="370" Height="0" />
        <Control Id="Title" Type="Text" X="15" Y="6" Width="200" Height="15" Transparent="yes" NoPrefix="yes" Text="{\WixUI_Font_Title}Options" />
        <Control Id="Description" Type="Text" X="25" Y="23" Width="280" Height="15" Transparent="yes" NoPrefix="yes" Text="Choose who rpyenv is for and what it sets up." />
        <Control Id="Scope" Type="RadioButtonGroup" X="20" Y="55" Width="330" Height="44" Property="RPYENV_SCOPE">
          <RadioButtonGroup Property="RPYENV_SCOPE">
            <RadioButton Value="user" X="0" Y="0" Width="330" Height="16" Text="Just for &amp;me (no administrator rights needed)" />
            <RadioButton Value="machine" X="0" Y="22" Width="330" Height="16" Text="For &amp;all users of this computer (Windows asks for administrator rights)" />
          </RadioButtonGroup>
        </Control>
        <Control Id="LiveRehash" Type="CheckBox" X="20" Y="110" Width="330" Height="18" Property="LIVEREHASH" CheckBoxValue="1" Text="Enable &amp;live rehash: new scripts get shims while a program runs" />
        <Control Id="Migrate" Type="CheckBox" X="20" Y="134" Width="330" Height="18" Property="MIGRATE" CheckBoxValue="1" Text="Move &amp;pyenv-win aside so rpyenv takes over (pyenv migrate)" Hidden="yes" ShowCondition="PYENVWIN_FOUND AND RPYENV_SCOPE=&quot;user&quot;" HideCondition="NOT PYENVWIN_FOUND OR RPYENV_SCOPE&lt;&gt;&quot;user&quot;" />
        <Control Id="BottomLine" Type="Line" X="0" Y="234" Width="370" Height="0" />
        <Control Id="Back" Type="PushButton" X="180" Y="243" Width="56" Height="17" Text="!(loc.WixUIBack)" />
        <Control Id="Next" Type="PushButton" X="236" Y="243" Width="56" Height="17" Default="yes" Text="!(loc.WixUINext)" />
        <Control Id="Cancel" Type="PushButton" X="304" Y="243" Width="56" Height="17" Cancel="yes" Text="!(loc.WixUICancel)">
          <Publish Event="SpawnDialog" Value="CancelDlg" />
        </Control>
      </Dialog>

      <Publish Dialog="WelcomeDlg" Control="Next" Event="NewDialog" Value="OptionsDlg" Condition="NOT Installed" />
      <Publish Dialog="WelcomeDlg" Control="Next" Event="NewDialog" Value="VerifyReadyDlg" Condition="Installed AND PATCH" />
      <Publish Dialog="OptionsDlg" Control="Back" Event="NewDialog" Value="WelcomeDlg" />
      <Publish Dialog="OptionsDlg" Control="Next" Property="MSIINSTALLPERUSER" Value="1" Order="1" Condition="RPYENV_SCOPE=&quot;user&quot;" />
      <Publish Dialog="OptionsDlg" Control="Next" Property="MSIINSTALLPERUSER" Value="{}" Order="2" Condition="RPYENV_SCOPE=&quot;machine&quot;" />
      <Publish Dialog="OptionsDlg" Control="Next" Property="MIGRATE" Value="{}" Order="3" Condition="RPYENV_SCOPE=&quot;machine&quot;" />
      <Publish Dialog="OptionsDlg" Control="Next" Event="NewDialog" Value="VerifyReadyDlg" Order="4" />
      <Publish Dialog="VerifyReadyDlg" Control="Back" Event="NewDialog" Value="OptionsDlg" Order="1" Condition="NOT Installed" />
      <Publish Dialog="VerifyReadyDlg" Control="Back" Event="NewDialog" Value="MaintenanceTypeDlg" Order="2" Condition="Installed AND NOT PATCH" />
      <Publish Dialog="VerifyReadyDlg" Control="Back" Event="NewDialog" Value="WelcomeDlg" Order="3" Condition="Installed AND PATCH" />
      <Publish Dialog="MaintenanceWelcomeDlg" Control="Next" Event="NewDialog" Value="MaintenanceTypeDlg" />
      <Publish Dialog="MaintenanceTypeDlg" Control="RepairButton" Event="NewDialog" Value="VerifyReadyDlg" />
      <Publish Dialog="MaintenanceTypeDlg" Control="RemoveButton" Event="NewDialog" Value="VerifyReadyDlg" />
      <Publish Dialog="MaintenanceTypeDlg" Control="Back" Event="NewDialog" Value="MaintenanceWelcomeDlg" />
      <Publish Dialog="ExitDialog" Control="Finish" Event="EndDialog" Value="Return" Order="999" />
    </UI>
    <UIRef Id="WixUI_Common" />
    <Property Id="WIXUI_EXITDIALOGOPTIONALTEXT" Value="Open a new terminal to use pyenv. If its shims aren't on your PATH there, run: pyenv setup" />
  </Package>
</Wix>
```

- [ ] **Step 7: Build both MSIs locally**

Run: `cargo build --release --locked -p pyenv -p pyenv-shim -p pyenv-shimw`, then `pwsh -NoProfile -File installer/build.ps1 -Arch x64 -BinDir target/release`, then `pwsh -NoProfile -File installer/build.ps1 -Arch arm64 -BinDir target/release`.
Expected: two paths printed, `dist\rpyenv-0.1.0-x64.msi` and `dist\rpyenv-0.1.0-arm64.msi`. The arm64 one contains x64 binaries here: a packaging check only, since CI builds the real ARM64 MSI.

If `wix build` reports schema or reference errors, fix the `.wxs` and ledger each change as a ruling. These are the design §10 risk 1 findings, so record the WiX error text with each.

- [ ] **Step 8: Run the table test (GREEN)**

Run: `pwsh -NoProfile -File installer/test_msi_tables.ps1 dist/rpyenv-0.1.0-x64.msi`, and the same for the arm64 MSI.
Expected: `MSI tables: ok (…)` for both.

- [ ] **Step 9: Commit:** `git commit -m "The dual-mode MSI: WiX 5.0.2 source, build and signing hook, and a table check that needs no install"` (files: `.config/dotnet-tools.json`, `.gitignore`, `installer/rpyenv.wxs`, `installer/build.ps1`, `installer/sign.ps1`, `installer/test_msi_tables.ps1`).

---

### Task 4: Install, upgrade and uninstall checks for CI runners

**Files:**
- Create: `installer/test_msi.py`

**Interfaces:**
- Consumes: Task 3's MSIs and `LIVEREHASH`/`MIGRATE`; the `HKCU\Software\rpyenv\Installed` and `HKLM\Software\rpyenv\Installed` markers; Task 2's `setup --undo`.
- Produces: `python installer/test_msi.py <dist-dir> <next-dist-dir>`. Exit 0 when every scenario passes; it refuses unless `CI=true`.

This task can't watch its tests fail locally (the global constraints forbid host installs). They first run in Task 7's CI. The local gate here is that it refuses outside CI.

- [ ] **Step 1: Write the scenarios** — `installer/test_msi.py`:

```python
"""Installs and uninstalls rpyenv's MSI and checks what each step leaves (M6b design §7.4).

CI only: it changes the current user's PATH, PowerShell profiles and installed
programs, so it refuses to run unless CI=true (GitHub Actions sets it).
Usage: python installer/test_msi.py <dist-dir> <next-dist-dir>
"""
import ctypes
import os
import subprocess
import sys
import tempfile
import winreg
from ctypes import wintypes
from pathlib import Path

LINE = 'iex ((pyenv init - pwsh) -join "`n")'
USER_DIR = Path(os.environ["LOCALAPPDATA"]) / "Programs" / "rpyenv"
MACHINE_DIR = Path(os.environ["ProgramFiles"]) / "rpyenv"
ROOT = Path(os.environ["USERPROFILE"]) / ".pyenv" / "pyenv-win"
SHIMS = ROOT / "shims"
ACTIVE_SETUP = r"SOFTWARE\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}"
MACHINE_ENV = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"


def documents() -> Path:
    class GUID(ctypes.Structure):
        _fields_ = [("a", wintypes.DWORD), ("b", wintypes.WORD), ("c", wintypes.WORD), ("d", ctypes.c_ubyte * 8)]

    folder = GUID(0xFDD39AD0, 0x238F, 0x46AF, (ctypes.c_ubyte * 8)(0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7))
    path = ctypes.c_wchar_p()
    if ctypes.windll.shell32.SHGetKnownFolderPath(ctypes.byref(folder), 0, None, ctypes.byref(path)) != 0:
        raise OSError("SHGetKnownFolderPath(Documents) failed")
    result = Path(path.value)
    ctypes.windll.ole32.CoTaskMemFree(path)
    return result


PROFILES = [documents() / sub / "Microsoft.PowerShell_profile.ps1" for sub in ("WindowsPowerShell", "PowerShell")]


def msiexec(args: str, expect=(0, 3010)) -> int:
    log = Path(tempfile.mkdtemp()) / "msiexec.log"
    code = subprocess.run(f'msiexec {args} /qn /l*v "{log}"').returncode
    if code not in expect:
        raw = log.read_bytes() if log.exists() else b""
        text = raw.decode("utf-16", "replace") if raw[:2] == b"\xff\xfe" else raw.decode("utf-8", "replace")
        raise AssertionError(f"msiexec {args}: exit {code}, expected {expect}\n{text[-6000:]}")
    return code


def reg(root, key: str, name: str):
    try:
        with winreg.OpenKey(root, key, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as k:
            return winreg.QueryValueEx(k, name)[0]
    except FileNotFoundError:
        return None


def set_user_path(value: str) -> None:
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment", 0, winreg.KEY_SET_VALUE) as k:
        winreg.SetValueEx(k, "Path", 0, winreg.REG_EXPAND_SZ, value)


def entries(path) -> list:
    return [e.rstrip("\\").lower() for e in (path or "").split(";") if e.strip()]


def user_path() -> list:
    return entries(reg(winreg.HKEY_CURRENT_USER, "Environment", "Path"))


def machine_path() -> list:
    return entries(reg(winreg.HKEY_LOCAL_MACHINE, MACHINE_ENV, "Path"))


def profile_has_line() -> bool:
    return any(LINE in p.read_text(encoding="utf-8", errors="replace") for p in PROFILES if p.exists())


def check(ok: bool, what: str) -> None:
    if not ok:
        raise AssertionError(what)


def low(p: Path) -> str:
    return str(p).lower()


def per_user(msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    check((USER_DIR / "bin" / "pyenv.exe").is_file(), "pyenv.exe in LocalAppData\\Programs")
    check((USER_DIR / "completions" / "pyenv.pwsh").is_file(), "completions installed")
    check(low(USER_DIR / "bin") in user_path(), f"bin on the user PATH: {user_path()}")
    check(user_path()[:1] == [low(SHIMS)], f"shims first on the user PATH: {user_path()}")
    check((ROOT / ".rpyenv-setup").is_file(), "setup ran (marker)")
    check(profile_has_line(), "the PowerShell line")
    check(reg(winreg.HKEY_CURRENT_USER, r"Software\rpyenv", "Installed") == 1, "HKCU marker")
    msiexec(f'/x "{msi}"')
    check(not (USER_DIR / "bin" / "pyenv.exe").exists(), "files removed")
    check(low(USER_DIR / "bin") not in user_path(), f"bin off the user PATH: {user_path()}")
    check(low(SHIMS) not in user_path(), f"shims off the user PATH (setup --undo): {user_path()}")
    check(not profile_has_line(), "the PowerShell line removed")
    check(not (ROOT / ".rpyenv-setup").exists(), "marker removed")
    check((ROOT / "versions").is_dir(), "PYENV_ROOT kept")
    check(reg(winreg.HKEY_CURRENT_USER, r"Software\rpyenv", "Installed") is None, "HKCU marker removed")


def live_rehash(msi: Path) -> None:
    msiexec(f'/i "{msi}" LIVEREHASH=1')
    check(reg(winreg.HKEY_CURRENT_USER, "Environment", "RPYENV_LIVE_REHASH") == "1", "RPYENV_LIVE_REHASH=1")
    msiexec(f'/x "{msi}"')
    check(reg(winreg.HKEY_CURRENT_USER, "Environment", "RPYENV_LIVE_REHASH") is None, "RPYENV_LIVE_REHASH removed")


def migrate(msi: Path) -> None:
    bin_dir = ROOT / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    for name in ("pyenv.ps1", "pyenv.bat", "pyenv"):
        (bin_dir / name).write_text(f"{name} from pyenv-win\n", encoding="utf-8")
    before = reg(winreg.HKEY_CURRENT_USER, "Environment", "Path") or ""
    set_user_path(f"{bin_dir};{before}")
    try:
        msiexec(f'/i "{msi}" MIGRATE=1')
        check((ROOT / ".rpyenv-migrate" / "bin" / "pyenv.ps1").is_file(), "pyenv-win's launchers backed up")
        check(not (bin_dir / "pyenv.ps1").exists(), "pyenv-win's launchers moved aside")
        check(low(bin_dir) not in user_path(), "pyenv-win's bin off the user PATH")
        msiexec(f'/x "{msi}"')
        check((bin_dir / "pyenv.ps1").is_file(), "pyenv-win's launchers restored")
        check(low(bin_dir) in user_path(), "pyenv-win's bin back on the user PATH")
        check(not (ROOT / ".rpyenv-migrate").exists(), "the migrate backup is gone")
    finally:
        set_user_path(before)
        for name in ("pyenv.ps1", "pyenv.bat", "pyenv"):
            (bin_dir / name).unlink(missing_ok=True)


def upgrade(msi: Path, next_msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    msiexec(f'/i "{next_msi}"')
    check(user_path().count(low(SHIMS)) == 1 and user_path()[:1] == [low(SHIMS)], f"shims still first, once: {user_path()}")
    check(user_path().count(low(USER_DIR / "bin")) == 1, f"bin once on the user PATH: {user_path()}")
    check((ROOT / ".rpyenv-setup").is_file(), "the upgrade didn't undo setup (marker)")
    check(profile_has_line(), "the upgrade didn't undo setup (profile line)")
    msiexec(f'/i "{msi}"', expect=(1603, 1638))  # a downgrade is refused
    msiexec(f'/x "{next_msi}"')
    check(not (USER_DIR / "bin" / "pyenv.exe").exists(), "the upgraded install uninstalls")


def scope_refusal(msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    msiexec(f'/i "{msi}" ALLUSERS=1 MSIINSTALLPERUSER=""', expect=(1603,))
    check(not (MACHINE_DIR / "bin").exists(), "no all-users install over a per-user one")
    msiexec(f'/x "{msi}"')


def all_users(msi: Path) -> None:
    msiexec(f'/i "{msi}" ALLUSERS=1 MSIINSTALLPERUSER=""')
    check((MACHINE_DIR / "bin" / "pyenv.exe").is_file(), "pyenv.exe in Program Files")
    check(low(MACHINE_DIR / "bin") in machine_path(), f"bin on the machine PATH: {machine_path()}")
    stub = reg(winreg.HKEY_LOCAL_MACHINE, ACTIVE_SETUP, "StubPath") or ""
    check(stub.lower().endswith('pyenv.exe" setup'), f"Active Setup StubPath: {stub!r}")
    check(reg(winreg.HKEY_LOCAL_MACHINE, r"Software\rpyenv", "Installed") == 1, "HKLM marker")
    msiexec(f'/x "{msi}" ALLUSERS=1')
    check(not (MACHINE_DIR / "bin" / "pyenv.exe").exists(), "files removed")
    check(low(MACHINE_DIR / "bin") not in machine_path(), "bin off the machine PATH")
    check(reg(winreg.HKEY_LOCAL_MACHINE, ACTIVE_SETUP, "StubPath") is None, "Active Setup key removed")


def missing_exe(msi: Path) -> None:
    msiexec(f'/i "{msi}"')
    (USER_DIR / "bin" / "pyenv.exe").unlink()
    msiexec(f'/x "{msi}"')  # the custom actions fail; the uninstall must not
    check(not (USER_DIR / "bin" / "pyenv-shim.exe").exists(), "uninstalled without pyenv.exe")


def only_msi(folder: str) -> Path:
    found = sorted(Path(folder).glob("*.msi"))
    if len(found) != 1:
        raise SystemExit(f"expected one .msi in {folder}, found {[f.name for f in found]}")
    return found[0].resolve()


def main(argv) -> int:
    if os.environ.get("CI") != "true":
        print("refusing: this changes the current user's PATH, profiles and installed programs; it runs on CI only")
        return 2
    if len(argv) != 2:
        print(__doc__)
        return 2
    msi, next_msi = only_msi(argv[0]), only_msi(argv[1])
    for name, run in [
        ("per_user", lambda: per_user(msi)),
        ("live_rehash", lambda: live_rehash(msi)),
        ("migrate", lambda: migrate(msi)),
        ("upgrade", lambda: upgrade(msi, next_msi)),
        ("scope_refusal", lambda: scope_refusal(msi)),
        ("all_users", lambda: all_users(msi)),
        ("missing_exe", lambda: missing_exe(msi)),  # last: it leaves setup's changes behind
    ]:
        run()
        print(f"ok {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 2: Confirm the local guard**

Run: `python installer/test_msi.py dist dist-next` (with `CI` unset).
Expected: `refusing: …`, exit 2, and nothing installed.

- [ ] **Step 3: Commit:** `git commit -m "MSI install, upgrade and uninstall checks for CI runners (refuse to run anywhere else)"`.

---

### Task 5: The static Linux tarball

**Files:**
- Create: `ci/package_linux.sh`

**Interfaces:**
- Produces: `sh ci/package_linux.sh <out-dir>` → `<out-dir>/rpyenv-<version>-linux-<x64|arm64>.tar.gz` (plan P8), and prints its path.

- [ ] **Step 1: Write the script** — `ci/package_linux.sh`:

```sh
#!/bin/sh
# Builds a static (musl) rpyenv for this machine's architecture and packs the release
# tarball (M6b design §6.1, §7.1). Needs musl-tools and the musl rustup target.
# Usage: sh ci/package_linux.sh <out-dir>
set -eu
out=${1:?usage: sh ci/package_linux.sh <out-dir>}
case $(uname -m) in
  x86_64)
    arch=x64 target=x86_64-unknown-linux-musl
    export CC_x86_64_unknown_linux_musl=musl-gcc
    ;;
  aarch64)
    arch=arm64 target=aarch64-unknown-linux-musl
    export CC_aarch64_unknown_linux_musl=musl-gcc
    ;;
  *) echo "no rpyenv build for $(uname -m)" >&2; exit 1 ;;
esac
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
cargo build --release --locked --target "$target" -p pyenv -p pyenv-shim
for b in pyenv pyenv-shim; do
  if readelf -l "target/$target/release/$b" | grep -q INTERP; then
    echo "$b is dynamically linked; the release must be static" >&2
    exit 1
  fi
done
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/bin" "$stage/completions" "$out"
cp "target/$target/release/pyenv" "target/$target/release/pyenv-shim" "$stage/bin/"
cp completions/pyenv.bash completions/pyenv.zsh completions/pyenv.fish "$stage/completions/"
cp LICENSE README.md "$stage/"
name=rpyenv-$version-linux-$arch.tar.gz
tar -C "$stage" -czf "$out/$name" bin completions LICENSE README.md
echo "$out/$name"
```

- [ ] **Step 2: Try it in WSL if musl is there** — write `C:\tmp\m6b_pkg.sh`:

```sh
#!/bin/bash
set -eu
cd "$HOME/rpyenv-linux"
git fetch -q /mnt/c/Users/JM/repos/OWN/rpyenv m6b-packaging
git checkout -q -B m6b-packaging FETCH_HEAD
. "$HOME/.cargo/env"
if ! command -v musl-gcc >/dev/null; then echo "no musl-gcc here: the packaging check runs in CI (Task 7)"; exit 0; fi
rustup target add x86_64-unknown-linux-musl
sh ci/package_linux.sh /tmp/m6b-dist
tar -tzf /tmp/m6b-dist/rpyenv-*-linux-x64.tar.gz
```

Commit the script first (Step 3), then run `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m6b_pkg.sh`.
Expected: either "no musl-gcc here…" or the listing `bin/pyenv`, `bin/pyenv-shim`, `completions/…`, `LICENSE`, `README.md`. A "dynamically linked" failure is design §10 risk 3: fix it and ledger the ruling. Don't `sudo apt install` in WSL; it would prompt for the user's password.

- [ ] **Step 3: Commit:** `git commit -m "ci/package_linux.sh: a static musl build packed as the release tarball"`.

---

### Task 6: `install.sh`

**Files:**
- Create: `install.sh`, `ci/test_install_sh.py`

**Interfaces:**
- Consumes: Task 5's tarball layout; `pyenv init --install <shell>` and `pyenv init <shell>` (existing Linux commands).
- Produces:
  - `install.sh [--version vX.Y.Z] [--init|--no-init] [--take-over] [--restore-upstream] [--uninstall]`;
  - test hooks `RPYENV_INSTALL_BASE_URL` (release folder URL) and `RPYENV_INSTALL_TTY` (where answers are read; default `/dev/tty`).

- [ ] **Step 1: Write the failing tests** — `ci/test_install_sh.py`:

```python
"""Tests for install.sh (M6b design §6.2). Each test serves a fake release over HTTP from
a temporary folder and runs the script with a throwaway HOME and PYENV_ROOT, with fake
`uname`/`curl`/`pyenv` where it needs them. Stdlib only; runs on Linux.
Run: python3 -m unittest ci/test_install_sh.py -v
"""
import hashlib
import http.server
import io
import os
import shutil
import subprocess
import tarfile
import tempfile
import threading
import unittest
from functools import partial
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SCRIPT = REPO / "install.sh"

FAKE_PYENV = """#!/bin/sh
printf '%s\\n' "$*" >> "$RPYENV_TEST_CALLS"
case "$1" in --version) echo "pyenv 2.8.8 (rpyenv {version})" ;; esac
"""


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def make_tarball(path: Path, version: str) -> None:
    with tarfile.open(path, "w:gz") as tar:
        def add(name: str, text: str, mode: int = 0o644) -> None:
            data = text.encode("utf-8")
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = mode
            tar.addfile(info, io.BytesIO(data))

        add("bin/pyenv", FAKE_PYENV.format(version=version), 0o755)
        add("bin/pyenv-shim", "#!/bin/sh\nexit 0\n", 0o755)
        for sh in ("bash", "zsh", "fish"):
            add(f"completions/pyenv.{sh}", f"# {sh} completions {version}\n")
        add("LICENSE", "MIT\n")
        add("README.md", "rpyenv\n")


class Release:
    """Tarballs for both architectures and SHA256SUMS, served over HTTP."""

    def __init__(self, base: Path, version: str):
        self.dir = base / f"release-{version}"
        self.dir.mkdir()
        sums = []
        for arch in ("x64", "arm64"):
            name = f"rpyenv-{version}-linux-{arch}.tar.gz"
            make_tarball(self.dir / name, version)
            sums.append(f"{hashlib.sha256((self.dir / name).read_bytes()).hexdigest()}  {name}\n")
        (self.dir / "SHA256SUMS").write_text("".join(sums), encoding="utf-8")
        handler = partial(QuietHandler, directory=str(self.dir))
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()


class InstallShTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        # A throwaway HOME: the real ~/.pyenv is never reached (global constraints).
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.root = self.home / ".pyenv"
        self.calls = self.tmp / "calls.log"
        self.fakes = self.tmp / "fakes"
        self.fakes.mkdir()
        self.release = Release(self.tmp, "0.9.0")
        self.addCleanup(self.release.close)
        self.fake("uname", 'case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) echo Linux ;; esac')

    def fake(self, name: str, body: str) -> None:
        path = self.fakes / name
        path.write_text("#!/bin/sh\n" + body + "\n", encoding="utf-8")
        path.chmod(0o755)

    def run_install(self, *args, root=None, tty=None, url=None):
        env = {
            "HOME": str(self.home),
            "PATH": f"{self.fakes}:{os.environ['PATH']}",
            "SHELL": "/bin/bash",
            "RPYENV_INSTALL_BASE_URL": self.release.url if url is None else url,
            "RPYENV_INSTALL_TTY": str(tty) if tty else "/nonexistent/tty",
            "RPYENV_TEST_CALLS": str(self.calls),
        }
        if root is not None:
            env["PYENV_ROOT"] = str(root)
        return subprocess.run(
            ["sh", str(SCRIPT), *args],
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=60,
        )

    def calls_text(self) -> str:
        return self.calls.read_text(encoding="utf-8") if self.calls.exists() else ""

    def upstream_checkout(self) -> None:
        (self.root / "libexec").mkdir(parents=True)
        (self.root / "libexec" / "pyenv").write_text("#!/usr/bin/env bash\n", encoding="utf-8")
        (self.root / "bin").mkdir()
        (self.root / "bin" / "pyenv").symlink_to("../libexec/pyenv")
        (self.root / ".git").mkdir()
        (self.root / "README.md").write_text("upstream\n", encoding="utf-8")
        (self.root / "versions" / "3.12.1" / "bin").mkdir(parents=True)
        (self.root / "version").write_text("3.12.1\n", encoding="utf-8")
        (self.root / "plugins" / "python-build").mkdir(parents=True)
        (self.root / "plugins" / "pyenv-virtualenv").mkdir()

    def test_fresh_install_puts_binaries_and_completions_in_pyenv_root(self):
        r = self.run_install()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(os.access(self.root / "bin" / "pyenv", os.X_OK))
        self.assertTrue((self.root / "bin" / "pyenv-shim").is_file())
        for sh in ("bash", "zsh", "fish"):
            self.assertTrue((self.root / "completions" / f"pyenv.{sh}").is_file(), sh)
        self.assertFalse((self.root / "LICENSE").exists(), "only bin and completions are installed")

    def test_rerunning_upgrades_in_place(self):
        self.assertEqual(self.run_install().returncode, 0)
        newer = Release(self.tmp, "0.9.1")
        self.addCleanup(newer.close)
        r = self.run_install(url=newer.url)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("0.9.1", (self.root / "completions" / "pyenv.bash").read_text(encoding="utf-8"))

    def test_a_tampered_tarball_is_refused_and_nothing_is_installed(self):
        tarball = self.release.dir / "rpyenv-0.9.0-linux-x64.tar.gz"
        tarball.write_bytes(tarball.read_bytes() + b"tampered")
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("checksum", r.stderr)
        self.assertFalse((self.root / "bin").exists())

    def test_arm64_machines_get_the_arm64_tarball(self):
        self.fake("uname", 'case "$1" in -s) echo Linux ;; -m) echo aarch64 ;; esac')
        r = self.run_install()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("rpyenv-0.9.0-linux-arm64.tar.gz", r.stdout)

    def test_unsupported_machines_and_systems_are_refused(self):
        self.fake("uname", 'case "$1" in -s) echo Linux ;; -m) echo armv7l ;; esac')
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("armv7l", r.stderr)
        self.fake("uname", 'case "$1" in -s) echo Darwin ;; -m) echo arm64 ;; esac')
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("Linux", r.stderr)

    def test_the_version_flag_names_that_release(self):
        self.fake("curl", 'printf "%s\\n" "$@" >> "$RPYENV_TEST_CALLS"; exit 22')
        r = self.run_install("--version", "v1.2.3", url="")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("https://github.com/jmperez0/rpyenv/releases/download/v1.2.3/SHA256SUMS", self.calls_text())

    def test_an_upstream_checkout_is_refused_without_take_over(self):
        self.upstream_checkout()
        r = self.run_install()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("--take-over", r.stderr)
        self.assertTrue((self.root / "libexec" / "pyenv").is_file(), "nothing was moved")

    def test_take_over_keeps_the_users_data_and_restore_brings_upstream_back(self):
        self.upstream_checkout()
        r = self.run_install("--take-over")
        self.assertEqual(r.returncode, 0, r.stderr)
        saved = self.root / ".upstream-pyenv"
        for kept in ("versions/3.12.1", "version", "plugins/pyenv-virtualenv"):
            self.assertTrue((self.root / kept).exists(), kept)
        for moved in ("libexec/pyenv", ".git", "README.md", "plugins/python-build"):
            self.assertTrue(os.path.lexists(saved / moved), moved)
            self.assertFalse(os.path.lexists(self.root / moved), moved)
        self.assertTrue((self.root / "bin" / "pyenv-shim").is_file())
        r = self.run_install("--restore-upstream")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((self.root / "bin" / "pyenv").is_symlink())
        self.assertTrue((self.root / "libexec" / "pyenv").is_file())
        self.assertTrue((self.root / "plugins" / "python-build").is_dir())
        self.assertFalse((self.root / "bin" / "pyenv-shim").exists())
        self.assertFalse(saved.exists())
        self.assertTrue((self.root / "versions" / "3.12.1").is_dir())

    def test_a_second_take_over_is_refused(self):
        self.upstream_checkout()
        (self.root / ".upstream-pyenv").mkdir()
        r = self.run_install("--take-over")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(".upstream-pyenv", r.stderr)
        self.assertTrue((self.root / "libexec" / "pyenv").is_file(), "nothing was moved")

    def test_without_a_terminal_it_prints_the_init_lines(self):
        r = self.run_install()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("init bash", self.calls_text())
        self.assertNotIn("--install", self.calls_text())

    def test_the_init_flag_edits_the_startup_file(self):
        r = self.run_install("--init")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("init --install bash", self.calls_text())

    def test_the_prompt_answer_decides(self):
        answers = self.tmp / "answers"
        answers.write_text("n\n", encoding="utf-8")
        r = self.run_install(tty=answers)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("[Y/n]", r.stderr)
        self.assertNotIn("--install", self.calls_text())
        answers.write_text("\n", encoding="utf-8")
        r = self.run_install(tty=answers)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("init --install bash", self.calls_text())

    def test_uninstall_removes_only_rpyenv_files(self):
        self.assertEqual(self.run_install().returncode, 0)
        (self.root / "versions" / "3.12.1").mkdir(parents=True)
        (self.root / "bin" / "other-tool").write_text("x\n", encoding="utf-8")
        r = self.run_install("--uninstall")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((self.root / "bin" / "pyenv").exists())
        self.assertFalse((self.root / "bin" / "pyenv-shim").exists())
        self.assertFalse((self.root / "completions").exists())
        self.assertTrue((self.root / "bin" / "other-tool").is_file())
        self.assertTrue((self.root / "versions" / "3.12.1").is_dir())

    def test_uninstall_refuses_an_upstream_checkout(self):
        self.upstream_checkout()
        r = self.run_install("--uninstall")
        self.assertNotEqual(r.returncode, 0)
        self.assertTrue((self.root / "bin" / "pyenv").is_symlink(), "upstream's pyenv stays")

    def test_a_pyenv_root_with_spaces_and_non_ascii(self):
        root = self.tmp / "py env ñ"
        r = self.run_install(root=root)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((root / "bin" / "pyenv").is_file())
        r = self.run_install("--uninstall", root=root)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((root / "bin" / "pyenv").exists())

    def test_shellcheck_is_clean(self):
        if shutil.which("shellcheck") is None:
            self.skipTest("shellcheck isn't installed (CI runs it)")
        r = subprocess.run(["shellcheck", "-s", "sh", str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stdout)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to verify they fail.** Write `C:\tmp\m6b_install_tests.sh`:

```sh
#!/bin/bash
set -eu
cd "$HOME/rpyenv-linux"
git fetch -q /mnt/c/Users/JM/repos/OWN/rpyenv m6b-packaging
git checkout -q -B m6b-packaging FETCH_HEAD
python3 -m unittest ci/test_install_sh.py -v 2>&1 | tail -n 25
```

Commit the test file first (`git commit -m "install.sh tests (failing: no install.sh yet)"`), then run `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m6b_install_tests.sh`.
Expected: every test except `test_shellcheck_is_clean` fails or errors, because `install.sh` doesn't exist.

- [ ] **Step 3: Write `install.sh`:**

```sh
#!/bin/sh
# Installs rpyenv on Linux (M6b design §6.2):
#   curl -fsSL https://github.com/jmperez0/rpyenv/releases/latest/download/install.sh | sh
# Options, after `sh -s --`:
#   --version vX.Y.Z    install that release instead of the latest
#   --init | --no-init  add pyenv to your shell's startup file, or don't (default: ask)
#   --take-over         move an upstream pyenv in PYENV_ROOT aside without asking
#   --restore-upstream  remove rpyenv and put a moved-aside upstream pyenv back
#   --uninstall         remove rpyenv's files (Python versions stay)
set -eu

REPO_URL=https://github.com/jmperez0/rpyenv
RPYENV_FILES="bin/pyenv bin/pyenv-shim completions/pyenv.bash completions/pyenv.zsh completions/pyenv.fish"

say() { printf 'rpyenv: %s\n' "$*"; }
die() { printf 'rpyenv: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage: install.sh [--version vX.Y.Z] [--init|--no-init] [--take-over]
                  [--restore-upstream] [--uninstall]
EOF
}

version=
init=ask
take_over=no
action=install
while [ $# -gt 0 ]; do
  case $1 in
    --version)
      [ $# -ge 2 ] || die "--version needs a release, such as v0.1.0"
      version=$2
      shift 2
      ;;
    --version=*) version=${1#--version=}; shift ;;
    --init) init=yes; shift ;;
    --no-init) init=no; shift ;;
    --take-over) take_over=yes; shift ;;
    --restore-upstream) action=restore-upstream; shift ;;
    --uninstall) action=uninstall; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
done

root=${PYENV_ROOT:-${HOME:?HOME is not set}/.pyenv}
saved=$root/.upstream-pyenv
tty_in=${RPYENV_INSTALL_TTY:-/dev/tty}

# A terminal to ask on, even under `curl | sh` (stdin is the script there).
interactive() { ( exec <"$tty_in" ) 2>/dev/null; }

# Asks a yes/no question; an empty answer is yes.
ask() {
  printf 'rpyenv: %s [Y/n] ' "$1" >&2
  reply=
  IFS= read -r reply <"$tty_in" || reply=n
  case $reply in
    ''|[Yy]*) return 0 ;;
    *) return 1 ;;
  esac
}

check_platform() {
  os=$(uname -s)
  [ "$os" = Linux ] || die "this installer is for Linux (this is $os); on Windows, use the MSI from $REPO_URL/releases"
  machine=$(uname -m)
  case $machine in
    x86_64|amd64) arch=x64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) die "no rpyenv build for $machine; there are builds for x86_64 and aarch64" ;;
  esac
}

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die "needs curl or wget to download rpyenv"
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  else
    die "needs sha256sum or shasum to check the download"
  fi
}

download() {
  if [ -n "${RPYENV_INSTALL_BASE_URL:-}" ]; then
    base=$RPYENV_INSTALL_BASE_URL
  elif [ -n "$version" ]; then
    base=$REPO_URL/releases/download/$version
  else
    base=$REPO_URL/releases/latest/download
  fi
  fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "cannot download $base/SHA256SUMS"
  line=$(grep "  rpyenv-[^ ]*-linux-$arch\.tar\.gz\$" "$tmp/SHA256SUMS" | head -n 1)
  [ -n "$line" ] || die "no linux-$arch build is listed in $base/SHA256SUMS"
  want=${line%% *}
  name=${line##* }
  say "downloading $name"
  fetch "$base/$name" "$tmp/$name" || die "cannot download $base/$name"
  got=$(sha256 "$tmp/$name")
  [ "$got" = "$want" ] || die "checksum mismatch for $name (expected $want, got $got); nothing was installed"
}

is_upstream() { [ -e "$root/libexec/pyenv" ] || [ -d "$root/.git" ]; }

confirm_take_over() {
  [ ! -e "$saved" ] || die "$saved already exists (an earlier take-over?); run with --restore-upstream first, or move it away"
  if [ "$take_over" = no ] && interactive &&
    ask "An upstream pyenv is installed in $root. Move it aside to $saved and install rpyenv there? Your Python versions stay."; then
    take_over=yes
  fi
  [ "$take_over" = yes ] || die "an upstream pyenv is installed in $root; rerun with --take-over to move it aside (your Python versions stay), or set PYENV_ROOT to another folder"
}

# Moves everything but the user's data into $saved, recording each name (design §6.2).
take_over() {
  mkdir "$saved"
  : >"$saved/moved.txt"
  for entry in "$root"/* "$root"/.[!.]* "$root"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    base=${entry##*/}
    case $base in
      versions|version|shims|cache|plugins|.upstream-pyenv) continue ;;
    esac
    mv "$entry" "$saved/$base"
    printf '%s\n' "$base" >>"$saved/moved.txt"
  done
  if [ -e "$root/plugins/python-build" ]; then
    mkdir -p "$saved/plugins"
    mv "$root/plugins/python-build" "$saved/plugins/python-build"
    printf '%s\n' plugins/python-build >>"$saved/moved.txt"
  fi
  say "moved the upstream pyenv to $saved (undo: --restore-upstream)"
}

remove_rpyenv_files() {
  # shellcheck disable=SC2086 # RPYENV_FILES is a list of words
  for f in $RPYENV_FILES; do
    rm -f "$root/$f"
  done
  rmdir "$root/bin" "$root/completions" 2>/dev/null || true
}

restore_upstream() {
  [ -f "$saved/moved.txt" ] || die "nothing to restore: $saved/moved.txt not found"
  remove_rpyenv_files
  while IFS= read -r entry; do
    [ -z "$entry" ] || [ ! -e "$root/$entry" ] || die "cannot restore $entry: $root/$entry exists; move it away and rerun"
  done <"$saved/moved.txt"
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    mkdir -p "$(dirname "$root/$entry")"
    mv "$saved/$entry" "$root/$entry"
  done <"$saved/moved.txt"
  rm "$saved/moved.txt"
  rmdir "$saved/plugins" 2>/dev/null || true
  rmdir "$saved" 2>/dev/null || say "kept $saved: it holds files the take-over didn't put there"
  say "removed rpyenv and put the upstream pyenv back in $root"
}

install_files() {
  mkdir -p "$tmp/stage"
  tar -xzf "$tmp/$name" -C "$tmp/stage"
  [ -f "$tmp/stage/bin/pyenv" ] || die "$name has no bin/pyenv"
  mkdir -p "$root/bin" "$root/completions"
  # shellcheck disable=SC2086 # RPYENV_FILES is a list of words
  for f in $RPYENV_FILES; do
    [ -f "$tmp/stage/$f" ] || continue
    # A rename replaces even a running binary; the old install stays until this point.
    cp "$tmp/stage/$f" "$root/$f.new"
    mv -f "$root/$f.new" "$root/$f"
  done
  chmod 755 "$root/bin/pyenv" "$root/bin/pyenv-shim"
}

shell_setup() {
  shell_name=${SHELL:-sh}
  shell_name=${shell_name##*/}
  if [ "$init" = ask ]; then
    if interactive && ask "Add pyenv to your $shell_name startup file?"; then
      init=yes
    else
      init=no
    fi
  fi
  if [ "$init" = yes ]; then
    PYENV_ROOT=$root "$root/bin/pyenv" init --install "$shell_name" ||
      say "could not edit your $shell_name startup file; see: $root/bin/pyenv init $shell_name"
  else
    say "to finish, add pyenv to your shell's startup file:"
    PYENV_ROOT=$root "$root/bin/pyenv" init "$shell_name" || true
  fi
}

uninstall() {
  if [ -L "$root/bin/pyenv" ] || [ -e "$root/libexec/pyenv" ]; then
    die "$root holds an upstream pyenv, not rpyenv; nothing was removed"
  fi
  [ -e "$root/bin/pyenv" ] || [ -e "$root/bin/pyenv-shim" ] || die "rpyenv isn't installed in $root"
  remove_rpyenv_files
  say "removed rpyenv from $root; your Python versions stay in $root/versions"
  say "remove the pyenv lines (PYENV_ROOT, PATH, pyenv init) from your shell's startup file"
  if [ -d "$saved" ]; then
    say "to bring back the upstream pyenv: rerun with --restore-upstream"
  fi
}

case $action in
  uninstall) uninstall; exit 0 ;;
  restore-upstream) restore_upstream; exit 0 ;;
esac
check_platform
if is_upstream; then
  confirm_take_over
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM
download
if is_upstream; then
  take_over
fi
install_files
shell_setup
say "installed $(PYENV_ROOT=$root "$root/bin/pyenv" --version 2>/dev/null || echo rpyenv) in $root"
```

- [ ] **Step 4: Run them to verify they pass**

Run: `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m6b_install_tests.sh`, after committing `install.sh`, since the script tests committed code.
Expected: `OK` (16 tests; `test_shellcheck_is_clean` may be skipped in WSL).

- [ ] **Step 5: Commit:** `git commit -m "install.sh: checksummed install into PYENV_ROOT, upstream take-over and restore, shell setup, uninstall"`.

---

### Task 7: Release and packaging workflows

**Files:**
- Create: `.github/workflows/build.yml`, `.github/workflows/packaging.yml`, `.github/workflows/release.yml`, `ci/check_real_install.sh`

**Interfaces:**
- Consumes: Tasks 1, 3, 4, 5 and 6 (`ci/pe_imports.py`, `installer/build.ps1`, `installer/sign.ps1`, `installer/test_msi_tables.ps1`, `installer/test_msi.py`, `ci/package_linux.sh`, `install.sh`, `ci/test_install_sh.py`).
- Produces: artifacts `msi-x64`, `msi-arm64`, `tar-x64` and `tar-arm64`; on a `v*` tag, a draft release.

- [ ] **Step 1: Write the real-tarball check** — `ci/check_real_install.sh`:

```sh
#!/bin/sh
# Installs the real tarball with install.sh into a throwaway HOME, with no terminal, then
# uninstalls it (M6b design §7.4). Usage: sh ci/check_real_install.sh <dist-dir>
set -eu
dist=$(cd "${1:?usage: sh ci/check_real_install.sh <dist-dir>}" && pwd)
(cd "$dist" && sha256sum rpyenv-*.tar.gz >SHA256SUMS)
port=8765
python3 -m http.server --bind 127.0.0.1 --directory "$dist" "$port" >/dev/null 2>&1 &
server=$!
trap 'kill "$server"' EXIT
i=0
until curl -fs "http://127.0.0.1:$port/SHA256SUMS" >/dev/null; do
  i=$((i + 1))
  [ "$i" -lt 50 ] || { echo "the test server didn't start" >&2; exit 1; }
  sleep 0.1
done
home=$(mktemp -d)
HOME=$home RPYENV_INSTALL_BASE_URL=http://127.0.0.1:$port RPYENV_INSTALL_TTY=/nonexistent \
  sh install.sh --no-init </dev/null
"$home/.pyenv/bin/pyenv" --version
"$home/.pyenv/bin/pyenv" --version | grep -qi rpyenv
test -x "$home/.pyenv/bin/pyenv-shim"
HOME=$home sh install.sh --uninstall
test ! -e "$home/.pyenv/bin/pyenv"
echo "real install: ok"
```

- [ ] **Step 2: Write the reusable build** — `.github/workflows/build.yml`:

```yaml
name: Build packages
# Builds and verifies every release asset (M6b design §7). Called by packaging.yml (pull
# requests) and release.yml (tags, manual dry runs).
on:
  workflow_call:

permissions:
  contents: read

jobs:
  windows:
    name: msi (${{ matrix.arch }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: x64
            runner: windows-2025
          - arch: arm64
            runner: windows-11-arm
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - uses: actions/setup-dotnet@a98b56852c35b8e3190ac28c8c2271da59106c68 # v6.0.0
        with:
          dotnet-version: "8.0.x"
      - uses: actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97 # v7.0.0
        with:
          python-version: "3.12"
      - run: cargo build --release --locked -p pyenv -p pyenv-shim -p pyenv-shimw
      - name: Static C runtime
        run: python ci/pe_imports.py --check target/release/pyenv.exe target/release/pyenv-shim.exe target/release/pyenv-shimw.exe
      - name: Sign the executables (skipped until a provider is set up)
        shell: pwsh
        env:
          RPYENV_SIGN_TOOL: ${{ secrets.RPYENV_SIGN_TOOL }}
        run: ./installer/sign.ps1 target/release/pyenv.exe target/release/pyenv-shim.exe target/release/pyenv-shimw.exe
      - name: Build the MSI
        shell: pwsh
        run: ./installer/build.ps1 -Arch ${{ matrix.arch }} -BinDir target/release -OutDir dist
      - name: Build a higher-version MSI for the upgrade test
        shell: pwsh
        run: ./installer/build.ps1 -Arch ${{ matrix.arch }} -BinDir target/release -OutDir dist-next -Version Next
      - name: Sign the MSI (skipped until a provider is set up)
        shell: pwsh
        env:
          RPYENV_SIGN_TOOL: ${{ secrets.RPYENV_SIGN_TOOL }}
        run: ./installer/sign.ps1 (Get-ChildItem dist/*.msi).FullName
      - name: Check the MSI's tables
        shell: pwsh
        run: ./installer/test_msi_tables.ps1 (Get-ChildItem dist/*.msi).FullName
      - name: Install, upgrade and uninstall
        run: python installer/test_msi.py dist dist-next
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: msi-${{ matrix.arch }}
          path: dist/*.msi
          if-no-files-found: error

  linux:
    name: tarball (${{ matrix.arch }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: x64
            runner: ubuntu-latest
          - arch: arm64
            runner: ubuntu-24.04-arm
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
          targets: x86_64-unknown-linux-musl, aarch64-unknown-linux-musl
      - run: sudo apt-get update && sudo apt-get install -y musl-tools shellcheck
      - run: sh ci/package_linux.sh dist
      - name: install.sh tests
        run: python3 -m unittest ci/test_install_sh.py -v
      - run: shellcheck -s sh install.sh
      - name: install.sh with the real tarball, no terminal
        run: sh ci/check_real_install.sh dist
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: tar-${{ matrix.arch }}
          path: dist/*.tar.gz
          if-no-files-found: error
```

- [ ] **Step 3: Write the pull-request trigger** — `.github/workflows/packaging.yml`:

```yaml
name: Packaging
# Builds and verifies the release assets on pull requests that touch packaging (M6b
# design §7.3), so a break shows before a tag.
on:
  pull_request:
    paths:
      - "installer/**"
      - "install.sh"
      - "ci/package_linux.sh"
      - "ci/check_real_install.sh"
      - "ci/test_install_sh.py"
      - "ci/pe_imports.py"
      - ".cargo/**"
      - ".config/dotnet-tools.json"
      - "Cargo.toml"
      - "Cargo.lock"
      - ".github/workflows/build.yml"
      - ".github/workflows/packaging.yml"
      - ".github/workflows/release.yml"

permissions:
  contents: read

jobs:
  build:
    uses: ./.github/workflows/build.yml
    secrets: inherit
```

- [ ] **Step 4: Write the release** — `.github/workflows/release.yml`:

```yaml
name: Release
# A v* tag builds, verifies and drafts the release (M6b design §7.2); run it by hand for a
# dry run that publishes nothing.
on:
  push:
    tags: ["v*"]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  version:
    runs-on: ubuntu-latest
    outputs:
      version: ${{ steps.v.outputs.version }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - id: v
        run: |
          version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
          echo "version=$version" >>"$GITHUB_OUTPUT"
          if [ "$GITHUB_REF_TYPE" = tag ] && [ "$GITHUB_REF_NAME" != "v$version" ]; then
            echo "the tag $GITHUB_REF_NAME doesn't match the workspace version $version" >&2
            exit 1
          fi

  build:
    needs: version
    uses: ./.github/workflows/build.yml
    secrets: inherit

  publish:
    needs: [version, build]
    if: github.ref_type == 'tag'
    runs-on: ubuntu-latest
    permissions:
      contents: write
      id-token: write
      attestations: write
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2
        with:
          path: assets
          merge-multiple: true
      - name: Checksums
        run: |
          cp install.sh assets/
          cd assets
          sha256sum rpyenv-* install.sh >SHA256SUMS
          cat SHA256SUMS
      - uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2
        with:
          subject-path: "assets/*"
      - name: Draft the release
        env:
          GH_TOKEN: ${{ github.token }}
          VERSION: ${{ needs.version.outputs.version }}
        run: gh release create "$GITHUB_REF_NAME" assets/* --draft --generate-notes --title "rpyenv $VERSION"
```

- [ ] **Step 5: Lint the workflows.** Download actionlint into the session scratchpad: `gh release download --repo rhysd/actionlint --pattern "*windows_amd64.zip" --dir <scratchpad>`, unzip it there, then run `<scratchpad>\actionlint.exe .github/workflows/build.yml .github/workflows/packaging.yml .github/workflows/release.yml .github/workflows/ci.yml`.
Expected: no findings. Fix any, and ledger each as a ruling.

- [ ] **Step 6: Commit:** `git commit -m "Workflows: build.yml builds and verifies every asset; packaging.yml on pull requests; release.yml drafts a release from a v* tag"`.

- [ ] **Step 7: Run the packaging checks on CI.** Ask the user (AskUserQuestion) to approve pushing `m6b-packaging` and opening a **draft** pull request. Then `git push -u origin m6b-packaging` and `gh pr create --draft --base main --head m6b-packaging --title "M6b: packaging and release" --body-file C:\tmp\pr_m6b_draft.md`. The body is one paragraph naming the design and plan files.
Expected: the `Packaging` workflow runs four jobs: `msi (x64)`, `msi (arm64)`, `tarball (x64)` and `tarball (arm64)`.

- [ ] **Step 8: Make them green.** For each failing job, read its log (`gh run view <id> --log-failed`), fix, commit and push. These are the first real runs of Task 4's scenarios (and Task 5 on CI), so each fix records RED (the CI failure) and GREEN (the next run).
Expected: all four jobs green, plus the existing `CI` workflow (15 checks). The `windows-11-arm` label is design §10 risk 2: if it is unavailable, ledger it and ask the user before changing runners.

---

### Task 8: Interactive checks in Windows Sandbox (with the user)

**Files:**
- Create: `installer/sandbox/new-wsb.ps1`, `installer/sandbox/fake-pyenv-win.ps1`, `installer/sandbox/CHECKLIST.md`

**Interfaces:**
- Consumes: `dist\rpyenv-<version>-x64.msi` (Task 3, or the `msi-x64` artifact from Task 7).
- Produces: the verified results that Task 9 writes into the spec (silent all-users without elevation, the Active Setup console flash).

- [ ] **Step 1: Write the sandbox config generator** — `installer/sandbox/new-wsb.ps1`:

```powershell
# Writes dist\sandbox.wsb: a Windows Sandbox that sees this repository's dist folder and
# installer\sandbox read-only (M6b design §7.4). The host's own setup is never touched.
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$dist = Join-Path $repo 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$wsb = @"
<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$dist</HostFolder>
      <SandboxFolder>C:\rpyenv-dist</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
    <MappedFolder>
      <HostFolder>$PSScriptRoot</HostFolder>
      <SandboxFolder>C:\rpyenv-sandbox</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>explorer.exe C:\rpyenv-dist</Command>
  </LogonCommand>
</Configuration>
"@
$out = Join-Path $dist 'sandbox.wsb'
Set-Content -Path $out -Value $wsb -Encoding utf8
$out
```

- [ ] **Step 2: Write the pyenv-win fixture** — `installer/sandbox/fake-pyenv-win.ps1`:

```powershell
# Inside the Sandbox only: a stand-in pyenv-win (its launchers, and bin on the user PATH),
# so the installer offers `pyenv migrate`.
$ErrorActionPreference = 'Stop'
$bin = Join-Path $env:USERPROFILE '.pyenv\pyenv-win\bin'
New-Item -ItemType Directory -Force $bin | Out-Null
foreach ($n in 'pyenv.ps1', 'pyenv.bat', 'pyenv') { Set-Content (Join-Path $bin $n) "$n from pyenv-win" }
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', "$bin;$path", 'User')
"fake pyenv-win in $bin"
```

- [ ] **Step 3: Write the checklist** — `installer/sandbox/CHECKLIST.md`:

```markdown
# rpyenv MSI: interactive checks in Windows Sandbox

Start: `pwsh -NoProfile -File installer/sandbox/new-wsb.ps1`, then open the printed
`dist\sandbox.wsb`. Inside the Sandbox, the MSI is in `C:\rpyenv-dist`. Record each
result (as seen, plus anything unexpected).

1. **Just for me.** Double-click the x64 MSI.
   - Expect: the Welcome page, then Options with "Just for me" selected, "Enable live
     rehash" unchecked and no pyenv-win checkbox. Install raises no UAC prompt. The last
     page says to open a new terminal.
   - In a new PowerShell: `pyenv --version` works; `$env:Path` starts with
     `…\.pyenv\pyenv-win\shims` and contains `…\AppData\Local\Programs\rpyenv\bin\`;
     `Get-Content $PROFILE` contains the `iex ((pyenv init - pwsh) …` line.
2. **Uninstall from Settings, Apps.**
   - Expect: it completes. `…\Programs\rpyenv` is gone, both PATH entries are gone, the
     profile line is gone, and `…\.pyenv\pyenv-win\versions` stays.
3. **Migrate.** In PowerShell run `C:\rpyenv-sandbox\fake-pyenv-win.ps1`, then install
   again, just for me.
   - Expect: the Options page shows "Move pyenv-win aside", checked. After install,
     `…\pyenv-win\bin\pyenv.ps1` is gone and in `…\pyenv-win\.rpyenv-migrate\bin\`.
     Uninstall puts it back.
4. **For all users.** Install, choosing "For all users".
   - Expect: a UAC prompt, or note that the Sandbox runs without UAC. Files land in
     `C:\Program Files\rpyenv\bin`, and that folder is on the machine PATH.
5. **Active Setup.** After step 4, run:
   `reg delete "HKCU\Software\Microsoft\Active Setup\Installed Components\{73C5FAF3-A93A-472F-BD99-814909823229}" /f`
   (it may already be absent), then `runonce.exe /AlternateShellStartup`.
   - Record: does a console window flash? Does `$env:Path` (new terminal) start with the
     shims, and does `…\pyenv-win\.rpyenv-setup` exist?
6. **Silent all users from a non-elevated prompt.** Uninstall, then from a normal
   (non-admin) Command Prompt run:
   `msiexec /i C:\rpyenv-dist\rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER="" /l*v %TEMP%\all.log`,
   then `echo %ERRORLEVEL%`.
   - Record: the exit code, and whether anything was installed (expected: fails without
     a prompt, nothing installed; if the Sandbox account is elevated, record that
     instead).
```

- [ ] **Step 4: Check that Windows Sandbox is available.**

Run: `Test-Path "$env:windir\System32\WindowsSandbox.exe"`.
Expected: `True`. If `False`, ask the user to enable "Windows Sandbox" in Optional features (admin, then a reboot). Don't enable it yourself.

- [ ] **Step 5: Run the checklist with the user.** Build `dist\rpyenv-<version>-x64.msi` (Task 3 Step 7), run `new-wsb.ps1`, and launch with `Start-Process (pwsh -NoProfile -File installer/sandbox/new-wsb.ps1)`. Ask the user (AskUserQuestion) to walk steps 1–6 and report each result. Ledger each result. Any failure is a finding: fix it with a test in Task 3 or 4 where one can pin it.

- [ ] **Step 6: Commit:** `git commit -m "Windows Sandbox checklist for the MSI's interactive behavior"` (files: `installer/sandbox/*`).

---

### Task 9: Documentation as built

**Files:**
- Modify: `README.md`, `docs/specs/2026-09-27-rpyenv-design.md`, `docs/superpowers/specs/2026-10-10-m6b-packaging-design.md`

- [ ] **Step 1: README.** Replace the block from `## Installing (planned)` up to, but not including, `## Roadmap` with:

````markdown
## Installing

Releases: https://github.com/jmperez0/rpyenv/releases. v0.1.0 is **unsigned**: Windows
SmartScreen warns before running the MSI ("More info", then "Run anyway"), and on a PC
with Smart App Control turned on, Windows blocks unsigned programs, rpyenv's shims
included.

### Windows

Run `rpyenv-<version>-x64.msi` (or `-arm64.msi`). The Options page asks:

- **Just for me** (no administrator rights) or **For all users** (Windows asks for
  administrator rights). Just for me installs to `%LOCALAPPDATA%\Programs\rpyenv`; for
  all users, to `%ProgramFiles%\rpyenv`. Each user's Python versions stay in their own
  `PYENV_ROOT` either way.
- **Enable live rehash** (off by default): new scripts get shims while a program runs.
- **Move pyenv-win aside** (shown when pyenv-win is installed): runs `pyenv migrate`.

Just for me also runs `pyenv setup`, which puts the shims first on your PATH and adds
the PowerShell line. For all users, each user is set up at their next sign-in, or on
their first `pyenv` command.

Silent installs:

```powershell
msiexec /i rpyenv-<version>-x64.msi /qn                                    # just for me
sudo msiexec /i rpyenv-<version>-x64.msi /qn ALLUSERS=1 MSIINSTALLPERUSER=""  # all users, elevated
# optional: LIVEREHASH=1, MIGRATE=1
```

Uninstall from Settings, Apps. It runs `pyenv migrate --restore` (if pyenv-win was moved
aside) and `pyenv setup --undo` for you. Your Python versions stay. Other users of an
all-users install can run `pyenv setup --undo` themselves first.

### Linux

```sh
curl -fsSL https://github.com/jmperez0/rpyenv/releases/latest/download/install.sh | sh
```

It installs `pyenv` into `$PYENV_ROOT/bin` (default `~/.pyenv/bin`), checks the
download against `SHA256SUMS`, and offers to add pyenv to your shell's startup file.
Options go after `sh -s --`: `--version vX.Y.Z`, `--init` or `--no-init`, `--uninstall`.

If an upstream pyenv is installed in `~/.pyenv`, the script offers to move it aside to
`~/.pyenv/.upstream-pyenv` (your Python versions, `version` file and plugins stay), or
do it with `--take-over`. `--restore-upstream` puts it back.

### Verifying a download

```sh
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify rpyenv-<version>-linux-x64.tar.gz --repo jmperez0/rpyenv
```

### Migrating from pyenv-win

The MSI offers it, or run `pyenv migrate` later. It:

- takes pyenv-win's `bin` off your `PATH`,
- moves pyenv-win's `pyenv.bat`, `pyenv.ps1`, and `pyenv` into a backup folder,
- rebuilds the shims as `.exe` files,
- adds the PowerShell profile line,
- links your pyenv-win-venv envs in as virtualenvs, leaving their files where they are.

Installed versions, the global `version` file, and `.python-version` files stay as they
are. `pyenv migrate --restore` undoes exactly what the migration did.
````

- [ ] **Step 2: The parent spec.**
  - In §9.4, replace "A fully silent install is expected to have no way to show a UAC prompt; verify this in M6." with the Task 8 step 6 result, and add one paragraph "As built (M6b)": WiX 5.0.2; the custom Options page (plan P1); `setup --undo`; the scope refusal; Active Setup GUID `{73C5FAF3-A93A-472F-BD99-814909823229}`; the Task 8 step 5 console-flash result.
  - Add **§9.5 Installing rpyenv itself on Linux**, summarizing design §6 (tarball layout, `install.sh` options, take-over keep-list, checksums).
  - In §17, answer question 2: "Answered 2026-10-10 (M6b design §9): v0.1.0 ships unsigned; the release workflow's `installer/sign.ps1` hook takes SignPath Foundation or Azure Artifact Signing later."

- [ ] **Step 3: The design doc.** In §10, rewrite risk 4 per plan P4: `ci/shim_deps.py` checks crates; the static CRT is checked by `ci/pe_imports.py`.

- [ ] **Step 4: Commit:** `git commit -m "Docs for M6b as built: installing on Windows and Linux, verifying downloads, spec 9.4 and 9.5"`.

- [ ] **Step 5: Final checks before review.** `cargo fmt --all --check`; clippy `-D warnings` on Windows and on Linux (WSL); `cargo test --workspace --no-fail-fast` on both; the pyenv-win overlay (`python parity/pyenv_win_run.py --pyenv-win <fresh clone of C:/tmp/pyenv-win-856ed5a> --rpyenv target/debug`, baseline 225 passed, 3 skipped, 56 xfailed); the Packaging and CI workflows green on the draft PR.

---

## After the plan

A final whole-branch review (one reviewer on the most capable model), the fix pass, and the user's decision on minors. Then mark the PR ready. After the user merges, a **dry run** of `release.yml` from main (`gh workflow run release.yml --ref main`) must be green before anyone pushes the `v0.1.0` tag. The tag itself is the user's call.
