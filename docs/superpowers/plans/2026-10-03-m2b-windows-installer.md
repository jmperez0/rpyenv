# M2b Windows Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Windows, `pyenv install`, `install --list`, `update`, `uninstall` and `latest` work like pyenv-win's, installing python.org's own packages. Every file is checked against python.org's published SHA-256 or its release managers' OpenPGP signature. A failure or Ctrl+C never leaves a partial version behind.

**Architecture:** Three pieces join M2a's `install` module tree in the `pyenv` binary:
- a pure-Rust OpenPGP verifier with three pinned keys;
- a pure-Rust MSI and CAB extractor that reproduces `msiexec /a`;
- a zip extractor.

On top of them:
- **Catalog:** reads and writes pyenv-win's own `.versions_cache.xml`, so an existing pyenv-win cache works unchanged.
- **Package resolver:** turns a version code into an index zip, component MSIs, or a single MSI. It fetches the reference value (SHA-256 or `.asc`) from python.org at install time.
- **Reuse from M2a:** the fetcher and the install transaction, made flavor-aware here.
- **Commands:** new pyenv-win-flavor command modules mirror pyenv-win's options and output.
- **No new dependencies** for the shims or `rpyenv-core`.

**Tech Stack:**
- Rust 1.96, edition 2021.
- New crates:
  - `pgp` 0.21.0 (rpgp, `default-features = false`);
  - `msi` 0.10.0;
  - `cab` =0.6.0 (pinned exactly, Decision 4);
  - `md-5` 0.11.0;
  - `zip` 8.6.0 (`default-features = false`, `deflate-flate2`);
  - `serde_json` 1.
- Existing: ureq 3.4.2, sha2 0.11 and flate2 1.1.
- Measured cost: `pyenv.exe` +1.9 MB, about 171 crates, no C compiler. `pyenv-shim` is unchanged.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md`: §3, §4, §9.1 (as amended 2026-10-03: unsigned files, the cache format), §9.3, §11, §12.5, §13 and §14 item 2.

**Upstream behavior:**
- `docs/parity/pyenv-win-m2-reference.md` (install, `--list`, update, uninstall, latest -k);
- `docs/parity/pyenv-win-m1-reference.md` → *Prefix resolution* and *latest*.

**Evidence:** measured while planning, 2026-10-03.
- **Spikes:**
  - the Rust MSI and OpenPGP spike (`C:\tmp\m2b_spike_rust\`);
  - the python.org data spike (`C:\tmp\m2b_spike_data\`).
- **Signature sweep:** 757 `.asc` files, signers recorded in `crates/pyenv/keys/README.md`.
- **Committed fixtures:**
  - `crates/pyenv/tests/fixtures/msi/` holds python.org's real 3.10.11 amd64 `tools.msi` with its `.asc`, and a manifest of what `msiexec /a` produced from it (105 files: path, size, SHA-256).
  - `crates/pyenv/tests/fixtures/openpgp/` holds synthetic keys for expiry, revocation and subkeys.
- Facts this plan relies on are quoted where used.

## Global Constraints

- **Toolchain:** `rust-version = "1.96"`, edition 2021 (workspace).
- **Dependencies (spec §3):**
  - `rpyenv-core` depends on "std, and platform crates only".
  - `pyenv-shim` and `pyenv-shimw` "must not link networking, TLS, or archive code", enforced by `ci/shim_deps.py`.
  - New crates go in `crates/pyenv/Cargo.toml` only.
- **Parity (spec §4):**
  - Match pyenv-win in messages, streams (**everything on stdout**), line endings (**CRLF**), exit codes and formats.
  - Don't reproduce defects.
  - Every intentional difference gets an allowlist row in `docs/parity/allowlist.md`. The next free row is **D-73**.
- **Atomic (spec §9.3):** "a failure or Ctrl+C never leaves a partial `versions/<ver>`."
- **Verified (spec §9.3, as amended):** every download is checked, before use, against python.org's published value:
  - the Install Manager index's SHA-256 for zips;
  - the release manager's `.asc` for MSIs.

  rpyenv never computes or publishes the reference value, and MD5 is never accepted. The single exception: a file python.org publishes **no** signature for installs with a warning (spec §9.1, *Unsigned files*).
  - The "unsigned" verdict comes only from python.org's folder listing.
  - A listed `.asc` that can't be fetched, or fails verification, stops the install.
- **The base URL:** `https://www.python.org/ftp/python`. Index pages, folder listings, `.asc` files and payloads all come from it.
  - `PYTHON_BUILD_MIRROR_URL` changes only the banner and `pyenv update`'s source, as in pyenv-win.
  - **Debug builds only** (`cfg(debug_assertions)`): `RPYENV_TEST_PYTHON_ORG` replaces the base, for the tier-1 fake server. Release builds ignore it.
- **Real pyenv root:** the developer machine has a real pyenv-win at `C:\Users\JM\.pyenv\pyenv-win`, which is rpyenv's default root.
  - Every run of a built `pyenv` outside test fixtures sets `PYENV_ROOT`, `PYENV` and `PYENV_HOME` to a scratch folder.
  - Never point a deleting command (`uninstall`, `install -f`, `-c`) at a path that could resolve outside scratch.
- **Python children** (ensurepip, pip) run with `-E -s` and `PYTHONNOUSERSITE=1`. Measured: plain `ensurepip` read the user site `%APPDATA%\Python\Python311` despite `-s`.
- **Tests (spec §12.5):**
  - **Tier 1:** a fake download server on every PR, both OSes. It uses the existing `crates/pyenv/tests/common/server.rs` with `RPYENV_TEST_PYTHON_ORG`.
  - **Tier 2:** a real Windows install on every PR touching the Windows installer, with downloads cached.
- **Python scripts:** every `open()` gets `encoding="utf-8"`.
- **Commits:** one-line `git commit -m` messages, with no attribution or trailer lines. Don't push.
- **Scratch:** never wildcard-delete in `C:\tmp`; delete only paths you created, by literal path.

## Decisions (rulings made while planning; each is recorded here so reviewers can check it)

1. **Package by version** (spec §9.1 table), decided per install from the version code, never from the cache.

   **Code grammar:** a version code is `X[.Y[.Z]][pre][t][-win32|-arm]`, where `pre` is `a`, `b`, `c` or `rc` plus a number. The resolver tries, in order:
   1. **≥ 3.11:** a `PythonCore` zip in the Install Manager index whose `sort-version`, free-threaded flag and arch match. Its `hash.sha256` is the reference.
   2. **≥ 3.5, not free-threaded:** component MSIs in `<base>/<X.Y.Z>/<amd64|win32|arm64><pre>/`.
      - The folder listing names the `.msi` files.
      - Drop `*_d.msi`, `*_pdb.msi`, `appendpath`, `launcher`, `path`, `pip` and `freethreaded*`.
      - Each remaining MSI's `<name>.asc` in the same listing is its reference.
   3. **< 3.5:** the single MSI `<base>/<X[.Y[.Z]]>/python-<code>[.amd64].msi`, with its `.asc` from the version folder's listing.

   **Measured facts behind the rules:**
   - The only 3.11+ final without a zip is 3.12.5.
   - 3.11, 3.12 and 3.13 pre-releases and 3.14.0a5 are component-only.
   - Pre-release components live in `<arch><pre>/` inside the final's folder (`3.10.0/amd64rc2/`).
   - arm64 folders start at 3.11.0a5.
2. **Catalog = pyenv-win's `.versions_cache.xml`** in `<root>`, read by `install --list`, `install` (check 8) and `latest -k`.
   - An existing pyenv-win cache works unchanged. The diff fixture already shares one, so `install --list` matches pyenv-win byte for byte.
   - `pyenv update` rewrites it, in the VBScript writer's format: tab indentation, CRLF, no final newline.
   - **Rows `update` writes:**
     - single-MSI codes point at the `.msi`;
     - component codes point at the `.exe` installer, as pyenv-win's rows do;
     - free-threaded codes (`3.13.0t`, `3.13.0t-win32`, `3.13.0t-arm`) point at their zip.
   - **Order:** CPython ascending; pre-releases before their final; within one version `-win32`, `-arm`, amd64, then the `t` builds in the same arch order.
   - **Never written:** PyPy and GraalPy rows (M7). `install` refuses such codes: Decision 12.
3. **`pyenv update` reads python.org directly** (spec §9.1). The algorithm:
   1. Fetch the `/ftp/python/` root listing.
   2. Fetch the listing of every version folder ≥ 2.4, with 8 parallel requests. That gives single-MSI files and component arch folders.
   3. Fetch the Install Manager index chain (`index-windows.json` → `index-windows-recent.json` → `index-windows-legacy.json`, each via `next`, relative to the page).

   It makes about 257 requests (measured: 0.09 s each sequentially; about 3 s with 8 workers). It reaches every code pyenv-win's own cache has except `3.5.0a5`, which has no component folder and so can't be installed by rpyenv anyway.

   The data spike's cheaper 4-request API route was rejected: the release-file API omits 3.9.3 and 78 old pre-releases, and is one more format to track.
4. **MSI extraction is pure Rust**, taken from the spike, where it was compared with `msiexec /a` on 12 python.org MSIs: 10,409 files with equal path, size and SHA-256, 0 differences.
   - **Rules:**
     - the source part of `DefaultDir`;
     - long names, even with Word Count bit 0;
     - `.` adds no segment;
     - only features with Level ≠ 0;
     - every file's size checked against `File.FileSize`, and its MD5 against `MsiFileHash`. This is an extraction-integrity check, never authenticity, which comes from the `.asc`.
   - **`cab` is pinned to `=0.6.0`.** Its `read_file` restarts the folder for each file (2.7.18: 710 s against 0.31 s), so `cab_for_each_sequential` patches the CFFILE record in memory and streams each folder once. That depends on how 0.6.0 parses the record.
   - **No `msiexec` fallback.** Every python.org MSI decompressed in the spike; the spec's fallback is not needed.
   - **Hardening beyond the spike:** reject Windows device names (`CON`, `NUL`, `COM1`…) and segments ending in `.` or a space.
5. **OpenPGP:**
   - **Library:** `pgp` 0.21.0 with the spike's policy wrapper: issuer membership, valid self-signatures, revocation, expiry at signature time, signing-subkey flags, binary signatures only.
   - **Keys:** exactly the three keys in `crates/pyenv/keys/`, pinned by full fingerprint. Their provenance is in that folder's README. All 757 MSI signatures on python.org are by one of them.
   - **SHA-1/DSA:** accepted for the Löwis and Baxter keys (2.4–3.4), since those are the only signatures python.org publishes for those versions.
   - **rpgp quirks worked around:** `from_armor_many` reads one block, so the keyring is split per block. `verify_bindings` fails on third-party certifications, so it is not used.
6. **Unsigned files install with a warning** (user decision 2026-10-03). When a listing has a `.msi` without its `<name>.asc`, it is downloaded and used, and one line is printed per version, before `[Installing]`:

   `:: [Warning] :: python.org publishes no signature for <file>[, <file>…]; checked only by HTTPS.`

   Measured unsigned files:
   - all 3.5.2 component MSIs;
   - the 2.7.7–2.7.9 MSIs;
   - 2.4.3c1, 2.7.14rc1 (amd64), 2.7.16rc1, 2.7.7rc1, 2.7.9rc1, 3.0a1–a4;
   - 3.5.0 `amd64a1`;
   - all 3.14.x components, which are never used because 3.14 installs from zips.
7. **Layout parity with pyenv-win's installs:**
   - **Component and single-MSI installs** run `python -E -s -m ensurepip -U --default-pip` when `Lib\ensurepip` exists, as pyenv-win does.
   - **Zip installs** already contain pip but no `Scripts\`. Measured on 3.11.0: `ensurepip` did nothing, while `pip install --no-index --no-deps --force-reinstall <Lib\ensurepip\_bundled\pip-*.whl>` created `Scripts\pip.exe`, `pip3.exe` and `pip3.11.exe`. So zip installs run that, giving the `pip` shim pyenv-win users have.
   - **Copies, on every install:**
     - `python.exe` → `pythonX.exe`, `pythonXY.exe`, `pythonX.Y.exe`;
     - the same for `pythonw.exe`;
     - when `Lib\venv\scripts\nt\python.exe` exists, it is copied to the six venv-launcher names, as pyenv-win's step 6 does. pyenv-win's `test_patched_venv_module` needs this.
   - **Free-threaded zips** have no `python.exe` (measured). `python<X.Y>t.exe` is copied to `python.exe`, and `pythonw<X.Y>t.exe` to `pythonw.exe`, before the copies above.
   - **Steps that run Python** run in the final location, after the transaction places the tree and before it commits, because pip's launchers embed the interpreter's absolute path.
   - **Order:** the copies come first, so that ensurepip finds `python.exe`.
8. **Install cache = pyenv-win's `<root>\install_cache\`.**
   - Zips go to `install_cache\<file>.zip`; MSIs and their `.asc` go to `install_cache\<code>\`. That is the folder pyenv-win itself fills with the MSIs it extracts from an `.exe`.
   - A cached file is used only after it verifies again: SHA-256 against the freshly fetched index, or `.asc` against the freshly fetched listing. Otherwise it is downloaded again.
   - `-c` empties the folder. A missing folder is not an error (allowlist).
   - `-f` keeps the cache. pyenv-win deletes the installer, but here every cached file is verified again anyway (allowlist).
9. **The transaction, per flavor.**
   - **Completeness:**
     - Linux keeps `bin/` without the marker.
     - pyenv-win: a version is "installed" when `versions\<code>` exists without the marker, matching pyenv-win's exists-means-skip rule.
   - **Carry-over:** gains `Scripts\`, alongside `Lib\site-packages`, which it already handles.
   - **"Kept the previous installation":** printed with the flavor's line ending, on stdout for pyenv-win.
   - **Staging names:** `installed::top_level` hides them for both flavors.
   - **Interface:** `Txn::begin_for` and `is_complete_for` take the flavor. `begin` and `is_complete` stay as the Linux defaults, so M2a's code is unchanged.
10. **Exit codes and failures.**
    - rpyenv **stops at the first failure and exits 1**: download, verification, extraction or a Python step. pyenv-win exits 0 after installer failures and continues. Allowlist.
    - Rehash runs after the loop unless interrupted.
    - Ctrl+C rolls back and exits 130.
11. **No registry writes** (spec §9.1).
    - `-r` keeps pyenv-win's two refusals (with `--32only`, with `-a`).
    - After the installs, `-r` prints `:: [Info] :: rpyenv does not register versions for the py launcher; --register was ignored.`
    - `uninstall` deletes no keys, so pyenv-win's alternating false error can't happen.
    - Allowlist.
12. **CPython only until M7.**
    - Codes starting `pypy` or `graalpy`, or rows with `<zipRootDir>`, fail with `:: [Error] :: rpyenv cannot install <code> yet: only CPython is supported.` and exit 1.
    - `--list` still shows them, from pyenv-win's cache.
13. **Help and the banner.**
    - `install --help` and `update --help` print the banner from `PYTHON_BUILD_MIRROR_URL` at run time, so `pyenv help install` and `pyenv install --help` stay identical, as in pyenv-win.
    - `latest`, `uninstall` and `update` gain pyenv-win help topics.
14. **Default packages on Windows too** (spec §9.3: "after a successful `pyenv install`").
    - M2a's `default_packages` module becomes cross-platform. On Windows it runs `<prefix>\python.exe -m pip install -r <root>\default-packages` after each new install.
    - A failure prints `pyenv: error installing packages from  `<file>'` on stdout, and the install still succeeds, as on Linux.
    - pyenv-win has no such plugin: allowlist row D-86.
15. **Must-handle items carried from M2a** (memory `m2-carry-forward`):
    - flavor-aware completeness (Decision 9);
    - staging names hidden for pyenv-win (Decision 9);
    - `file://` URLs percent-decoded, with `file:///C:/x` → `C:\x` (Task 3);
    - backslash and `..` guard tests for `uninstall` (Task 9);
    - the CRLF "kept" message (Decision 9).

    Left as they are, with the reason:
    - the mirror HEAD outside the watchdog is Linux-only, since Windows uses no mirror;
    - a stale `.old-<code>` still blocks `install -f`, with a message naming the path.

## Review Focus

The five input classes most likely to bite a user that no task's main tests cover. Each one's test lives in the task named.

1. **A python.org file that changed after its signature.** A truncated download, a tampered MSI, or a `.asc` from another file must stop the install with no version folder left, and the cached copy must be discarded. Test in Task 7: serve the real `tools.msi` with one byte flipped.
2. **A hostile archive or MSI.** Zip entries with `..`, absolute paths, drive letters, `\`, symlinks or duplicate names, and MSI names with device names or separators, must be refused, not written. Tests in Tasks 2 and 3.
3. **An `.asc` that is listed but can't be fetched.** A 404 or 500 on a listed signature must never downgrade to the unsigned warning. Test in Task 7.
4. **`uninstall` with an escaping name.** `..`, `.`, `a\b`, `..\..`, `C:\x`, a staging name, or a name that differs only in case must delete nothing outside `versions\<name>`. Test in Task 9, against a tempdir with canaries; never on the host.
5. **A cache that isn't a cache.** A garbage `.versions_cache.xml`, an empty one, one with a 2-row schema variant, or pyenv-win's real 901-row file must list correctly or fail with one clear message, never panic. Tests in Task 4.

## File structure

| File | Task | Responsibility |
|---|---|---|
| `crates/pyenv/keys/*.asc`, `README.md` | committed while planning | the three pinned release-signing keys and their provenance |
| `crates/pyenv/src/install/openpgp.rs` | 1 | detached-signature verification with the pinned keyring |
| `crates/pyenv/src/install/msi.rs` | 2 | MSI tables → admin-install layout; sequential CAB streaming |
| `crates/pyenv/src/install/mod.rs` | 2, 3 | `is_safe_win_segment` (shared name check), module list |
| `crates/pyenv/src/install/zipx.rs` | 3 | safe zip extraction |
| `crates/pyenv/src/install/fetch.rs` | 3 | `Check` (SHA-256 or caller-verified), `get_text`, `file://` decoding, `Fetcher::direct` |
| `crates/pyenv/src/install/wincatalog.rs` | 4 | version codes, pyenv-win order, `.versions_cache.xml` read/write |
| `crates/pyenv/src/install/winsource.rs` | 5 | python.org base URL, listing and index parsing, catalog build |
| `crates/pyenv/src/commands/update.rs` | 5 | `pyenv update` |
| `crates/pyenv/src/install/txn.rs` | 6 | per-flavor completeness, `Scripts` carry-over, CRLF message |
| `crates/rpyenv-core/src/installed.rs` | 6 | staging names hidden for both flavors |
| `crates/pyenv/src/install/winpkg.rs` | 7 | code → package; verified download; unpack; post steps |
| `crates/pyenv/src/commands/install_win.rs` | 8 | `pyenv install` (pyenv-win flavor) |
| `crates/pyenv/src/install/default_packages.rs` | 8 | default packages on both OSes |
| `crates/pyenv/src/commands/mod.rs`, `src/help.rs`, `src/lib.rs` | 8, 9 | command table and help topics for the pyenv-win flavor |
| `crates/pyenv/src/commands/uninstall_win.rs` | 9 | `pyenv uninstall` (pyenv-win flavor) |
| `crates/pyenv/src/commands/latest.rs` | 9 | the pyenv-win branch of `pyenv latest` |
| `parity/*`, `docs/parity/allowlist.md` | 10 | rows D-73 onward, `DELIVERED`, expected lists, goldens, cases |
| `.github/workflows/install-windows.yml`, `ci/check_install_win.py` | 11 | tier 2: real installs on Windows |

---

### Task 1: OpenPGP verifier with the pinned keyring

**Files:**
- Modify: `crates/pyenv/Cargo.toml`, `crates/pyenv/src/install/mod.rs`
- Create: `crates/pyenv/src/install/openpgp.rs`
- Test: `crates/pyenv/tests/install_openpgp.rs` (both OSes)

**Interfaces:**
- Consumes: the committed `crates/pyenv/keys/{steve_dower,martin_v_loewis,anthony_baxter}.asc`, and the fixtures `tests/fixtures/msi/tools-3.10.11-amd64.msi{,.asc}` and `tests/fixtures/openpgp/*`.
- Produces:
  - `pub const PINNED: [&str; 3]`
  - `pub fn keyring() -> String`
  - `pub fn parse_keyring(&str) -> (Vec<pgp::composed::SignedPublicKey>, Vec<String>)`
  - `pub fn verify_detached(data: &Path, asc: &[u8], keys_armored: &str) -> Result<String, String>`, which returns the signer's primary fingerprint in uppercase hex
  - `pub fn verify_python_org(data: &Path, asc: &[u8]) -> Result<String, String>`, which uses `keyring()` and also requires the signer to be in `PINNED`

This is the spec's "first Windows installer task proves the verifier on a real `core.msi`" (§9.1). The real file used is `tools.msi`, signed by the same key: it is 217 KB against `core.msi`'s 1.7 MB.

- [ ] **Step 1: Add the dependency.** In `crates/pyenv/Cargo.toml` under `[dependencies]`, after `tar = "0.4.46"`, add:

```toml
# Windows installer (M2b): OpenPGP signatures on python.org MSIs (spec §9.1). Pure Rust;
# default features off (measured: 143 crates, no C).
pgp = { version = "0.21.0", default-features = false }
```

In `crates/pyenv/src/install/mod.rs`, add `pub mod openpgp;` after `pub mod log;` (no `cfg`: it builds and is tested on both OSes).

- [ ] **Step 2: Write the failing tests.** Create `crates/pyenv/tests/install_openpgp.rs`:

```rust
//! The OpenPGP verifier (plan M2b Task 1): real python.org signature, the pinned keyring, and
//! the policy rpgp itself lacks (expiry, revocation, subkeys).

use pgp::types::KeyDetails;
use pyenv::install::openpgp::{keyring, parse_keyring, verify_detached, verify_python_org, PINNED};
use std::path::{Path, PathBuf};

fn fx(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rel)
}

fn key_file(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("keys").join(name)).unwrap()
}

const TOOLS: &str = "msi/tools-3.10.11-amd64.msi";
const DOWER: &str = "7ED10B6531D7C8E1BC296021FC624643487034E5";

#[test]
fn the_embedded_keyring_is_exactly_the_three_pinned_keys() {
    let (keys, errors) = parse_keyring(&keyring());
    assert!(errors.is_empty(), "{errors:?}");
    let mut got: Vec<String> = keys.iter().map(|k| format!("{:X}", k.fingerprint())).collect();
    got.sort();
    let mut want: Vec<String> = PINNED.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn a_real_python_org_msi_verifies_as_steve_dower() {
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    assert_eq!(verify_python_org(&fx(TOOLS), &asc).as_deref(), Ok(DOWER));
}

fn tampered(edit: impl Fn(&mut Vec<u8>)) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = std::fs::read(fx(TOOLS)).unwrap();
    edit(&mut bytes);
    let p = dir.path().join("tools.msi");
    std::fs::write(&p, bytes).unwrap();
    (dir, p)
}

#[test]
fn one_flipped_byte_fails() {
    let (_d, p) = tampered(|b| b[0x8000] ^= 1);
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    let e = verify_python_org(&p, &asc).unwrap_err();
    assert!(e.starts_with("bad signature"), "{e}");
}

#[test]
fn one_appended_byte_fails() {
    let (_d, p) = tampered(|b| b.push(0));
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    assert!(verify_python_org(&p, &asc).unwrap_err().starts_with("bad signature"));
}

#[test]
fn a_keyring_without_the_signer_fails() {
    let ring = key_file("martin_v_loewis.asc") + "\n" + &key_file("anthony_baxter.asc");
    let asc = std::fs::read(fx(&format!("{TOOLS}.asc"))).unwrap();
    let e = verify_detached(&fx(TOOLS), &asc, &ring).unwrap_err();
    assert!(e.starts_with("no key in the keyring matches"), "{e}");
}

#[test]
fn garbage_is_not_a_signature() {
    let e = verify_python_org(&fx(TOOLS), b"not a signature").unwrap_err();
    assert!(e.starts_with("bad signature armor"), "{e}");
}

fn synthetic(sig: &str, key: &str) -> Result<String, String> {
    let asc = std::fs::read(fx(&format!("openpgp/{sig}"))).unwrap();
    let ring = std::fs::read_to_string(fx(&format!("openpgp/{key}"))).unwrap();
    verify_detached(&fx("openpgp/data.bin"), &asc, &ring)
}

#[test]
fn a_signature_made_while_the_key_was_valid_stays_valid_after_it_expires() {
    assert_eq!(
        synthetic("expiring_sig_before.asc", "expiring_pubkey.asc").as_deref(),
        Ok("7832A9A8CBEB323CDC4667FB15C5BDFBB6467F16")
    );
}

#[test]
fn a_signature_made_after_the_key_expired_fails() {
    let e = synthetic("expiring_sig_after.asc", "expiring_pubkey.asc").unwrap_err();
    assert!(e.contains("expired"), "{e}");
}

#[test]
fn a_revoked_key_fails() {
    let e = synthetic("revoked_sig.asc", "revoked_pubkey.asc").unwrap_err();
    assert!(e.contains("is revoked"), "{e}");
}

#[test]
fn a_signing_subkey_reports_its_primary_key() {
    assert_eq!(
        synthetic("subkey_sig.asc", "subkey_pubkey.asc").as_deref(),
        Ok("52324C2D3F5163AE5D2C704B0E3C5FEAE0D23397")
    );
}

#[test]
fn verify_python_org_refuses_a_key_that_is_not_pinned() {
    // A valid signature by a key outside the embedded keyring never matches an issuer there.
    let asc = std::fs::read(fx("openpgp/subkey_sig.asc")).unwrap();
    let e = verify_python_org(&fx("openpgp/data.bin"), &asc).unwrap_err();
    assert!(e.starts_with("no key in the keyring matches"), "{e}");
}
```

- [ ] **Step 3: Run them and see them fail.**

Run: `cargo test -p pyenv --test install_openpgp`
Expected: compile error, `unresolved import pyenv::install::openpgp`.

- [ ] **Step 4: Implement.** Create `crates/pyenv/src/install/openpgp.rs`. Its body is the spike's `pgpverify.rs`, which was compiled and run against pgp 0.21.0. Four things are added: the module header, `PINNED`, `keyring()` and `verify_python_org`. The spike-only `verify_detached_raw` is dropped.

```rust
//! Detached OpenPGP signatures on python.org's MSIs (spec §9.1), checked against the three
//! release-signing keys in `crates/pyenv/keys/` (provenance in its README). Every MSI signature
//! python.org publishes, 757 swept on 2026-10-03, is by one of them.
//!
//! rpgp's `Signature::verify` is cryptographic only: it checks no expiry, revocation, key flags
//! or binding signatures (read in pgp 0.21.0, and measured with a synthetic expired key). The
//! policy below is rpyenv's:
//! * the issuer (fingerprint or key-id subpacket) must be the primary key or a subkey of a key in
//!   the keyring (the keyring is pinned, so trust is membership);
//! * the key needs at least one valid self-certification (third-party certifications are
//!   ignored; `verify_bindings` is not used because it fails on them), and a signing subkey a
//!   valid binding with the sign flag;
//! * only binary-document signatures (type 0x00) are accepted;
//! * the signature must predate the key's (and subkey's) expiry, if any: a signature made while
//!   the key was valid stays valid after it expires;
//! * a key with a valid self-revocation is rejected.
//! SHA-1 and DSA are accepted: they are what the Löwis and Baxter keys sign 2.4–3.4 with.

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey, SignedPublicSubKey};
use pgp::packet::{Signature, SignatureType};
use pgp::types::KeyDetails;
use pgp::types::Tag;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Full fingerprints of the embedded keys: Steve Dower, Martin v. Löwis, Anthony Baxter.
pub const PINNED: [&str; 3] = [
    "7ED10B6531D7C8E1BC296021FC624643487034E5",
    "CBC547978A3964D14B9AB36A6AF053F07D9DC8D2",
    "531F072D39700991925FED0C0EDDC5F26A45C816",
];

const KEYS: [&str; 3] = [
    include_str!("../../keys/steve_dower.asc"),
    include_str!("../../keys/martin_v_loewis.asc"),
    include_str!("../../keys/anthony_baxter.asc"),
];

/// The embedded keyring: one armored block per key.
pub fn keyring() -> String {
    KEYS.join("\n")
}

/// `verify_detached` against the embedded keyring, and the signer must be pinned.
pub fn verify_python_org(data: &Path, asc: &[u8]) -> Result<String, String> {
    let signer = verify_detached(data, asc, &keyring())?;
    if PINNED.contains(&signer.as_str()) {
        Ok(signer)
    } else {
        Err(format!("signed by {signer}, which is not a CPython release-signing key"))
    }
}
```

Then paste these items **verbatim** from `docs/superpowers/plans/2026-10-03-m2b-spike/pgpverify.rs` (the spike's file, committed with this plan), lines 24–179:
- `fpr_hex`, `expiry_from`, `issuer_matches`;
- `pub fn parse_keyring`;
- `enum Signer`;
- `pub fn verify_detached`.

They are reproduced here:

```rust
fn fpr_hex(k: &impl KeyDetails) -> String {
    format!("{:X}", k.fingerprint())
}

/// Seconds since epoch at which a key expires, from the newest self-signature that states one.
fn expiry_from(created: u32, sigs: &[&Signature]) -> Option<u64> {
    let newest = sigs
        .iter()
        .filter(|s| s.created().is_some())
        .max_by_key(|s| s.created().map(|t| t.as_secs()).unwrap_or(0))?;
    newest
        .key_expiration_time()
        .filter(|d| d.as_secs() != 0)
        .map(|d| created as u64 + d.as_secs() as u64)
}

fn issuer_matches(sig: &Signature, k: &impl KeyDetails) -> bool {
    let fps = sig.issuer_fingerprint();
    let ids = sig.issuer_key_id();
    fps.iter().any(|f| **f == k.fingerprint()) || ids.iter().any(|i| **i == k.legacy_key_id())
}

/// Every public key in an armored keyring. `from_armor_many` reads ONE armor block (measured:
/// a second block was silently ignored), so the text is split on the BEGIN line first.
pub fn parse_keyring(keys_armored: &str) -> (Vec<SignedPublicKey>, Vec<String>) {
    let mut keys = Vec::new();
    let mut parse_errors = Vec::new();
    const BEGIN: &str = "-----BEGIN PGP PUBLIC KEY BLOCK-----";
    let starts: Vec<usize> = keys_armored.match_indices(BEGIN).map(|(i, _)| i).collect();
    for (n, &st) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(keys_armored.len());
        match SignedPublicKey::from_armor_many(keys_armored[st..end].as_bytes()) {
            Ok((iter, _)) => {
                for k in iter {
                    match k {
                        Ok(k) => keys.push(k),
                        Err(e) => parse_errors.push(e.to_string()),
                    }
                }
            }
            Err(e) => parse_errors.push(e.to_string()),
        }
    }
    (keys, parse_errors)
}

enum Signer<'a> {
    Primary(&'a SignedPublicKey),
    Sub(&'a SignedPublicKey, &'a SignedPublicSubKey),
}

/// Verifies `asc` (an armored detached signature) over the file `data` against the keys in
/// `keys_armored`. Returns the signer's PRIMARY key fingerprint (uppercase hex).
pub fn verify_detached(data: &Path, asc: &[u8], keys_armored: &str) -> Result<String, String> {
    let (sig, _headers) =
        DetachedSignature::from_armor_single(asc).map_err(|e| format!("bad signature armor: {e}"))?;
    let sig = sig.signature;
    match sig.typ() {
        Some(SignatureType::Binary) => {}
        other => return Err(format!("not a binary-document signature: {other:?}")),
    }
    let sig_time = sig.created().ok_or("signature has no creation time")?.as_secs() as u64;

    let (keys, parse_errors) = parse_keyring(keys_armored);
    if keys.is_empty() {
        return Err(format!("no usable keys in keyring ({parse_errors:?})"));
    }

    let mut candidates = Vec::new();
    for k in &keys {
        if issuer_matches(&sig, k) {
            candidates.push(Signer::Primary(k));
        }
        for sk in &k.public_subkeys {
            if issuer_matches(&sig, sk) {
                candidates.push(Signer::Sub(k, sk));
            }
        }
    }
    if candidates.is_empty() {
        return Err(format!(
            "no key in the keyring matches the signature issuer {:?} / {:?}",
            sig.issuer_fingerprint().iter().map(|f| format!("{f:X}")).collect::<Vec<_>>(),
            sig.issuer_key_id().iter().map(|i| format!("{i:?}")).collect::<Vec<_>>()
        ));
    }

    let mut last_err = String::new();
    for c in candidates {
        let open = || {
            File::open(data)
                .map(BufReader::new)
                .map_err(|e| format!("open {}: {e}", data.display()))
        };
        let (primary, res) = match c {
            Signer::Primary(k) => (k, sig.verify(k, open()?)),
            Signer::Sub(k, sk) => (k, sig.verify(sk, open()?)),
        };
        if let Err(e) = res {
            last_err = format!("bad signature: {e}");
            continue;
        }
        // Cryptographically valid. Now the key policy: only self-signatures count.
        let pk = &primary.primary_key;
        let mut self_sigs: Vec<&Signature> = Vec::new();
        for u in &primary.details.users {
            for s in u.signatures.iter().filter(|s| issuer_matches(s, pk)) {
                if s.verify_certification(pk, Tag::UserId, &u.id).is_ok() {
                    self_sigs.push(s);
                }
            }
        }
        for s in primary.details.direct_signatures.iter().filter(|s| issuer_matches(s, pk)) {
            if s.verify_key(pk).is_ok() {
                self_sigs.push(s);
            }
        }
        if self_sigs.is_empty() {
            return Err(format!("key {} has no valid self-signature", fpr_hex(primary)));
        }
        let revoked = primary
            .details
            .revocation_signatures
            .iter()
            .any(|s| issuer_matches(s, pk) && s.verify_key(pk).is_ok());
        if revoked {
            return Err(format!("key {} is revoked", fpr_hex(primary)));
        }
        if let Some(exp) = expiry_from(pk.created_at().as_secs(), &self_sigs) {
            if sig_time >= exp {
                return Err(format!(
                    "signature made at {sig_time}, after key {} expired at {exp}",
                    fpr_hex(primary)
                ));
            }
        }
        if let Signer::Sub(_, sk) = c {
            let bindings: Vec<&Signature> = sk
                .signatures
                .iter()
                .filter(|b| b.verify_subkey_binding(pk, &sk.key).is_ok())
                .collect();
            if bindings.is_empty() {
                return Err(format!("subkey {} has no valid binding signature", fpr_hex(sk)));
            }
            if !bindings.iter().any(|b| b.key_flags().sign()) {
                return Err(format!("subkey {} is not a signing subkey", fpr_hex(sk)));
            }
            if let Some(exp) = expiry_from(sk.key.created_at().as_secs(), &bindings) {
                if sig_time >= exp {
                    return Err(format!("signature made after subkey {} expired", fpr_hex(sk)));
                }
            }
        }
        if (sig_time as u32) < pk.created_at().as_secs() {
            return Err("signature predates its key".into());
        }
        return Ok(fpr_hex(primary));
    }
    Err(last_err)
}
```

If `cargo fmt` reflows the pasted lines, accept the formatting. If an API detail fails to compile under edition 2021 (the spike was edition 2024), fix it minimally and record the change.

- [ ] **Step 5: Run the tests and see them pass.**

Run: `cargo test -p pyenv --test install_openpgp`
Expected: 11 passed.

- [ ] **Step 6: Mutation checks.** For each mutation, change the code, run the named test and see it fail, then restore.
  - Delete the `if revoked { … }` block: `a_revoked_key_fails` must fail.
  - Replace `sig_time >= exp` with `false` in the primary-key expiry check: `a_signature_made_after_the_key_expired_fails` must fail.
  - In `parse_keyring`, parse the whole text with one `from_armor_many` call: `the_embedded_keyring_is_exactly_the_three_pinned_keys` must fail. This reproduces the spike's measurement that only the first block is read.

- [ ] **Step 7: Check the shims and both OSes.**
  - Run `python ci/shim_deps.py`. Expected: OK, since `pgp` is linked only by `pyenv`.
  - Run `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test --workspace`. Do this on Windows and in WSL (from a script file).

- [ ] **Step 8: Commit.**

```bash
git add crates/pyenv/Cargo.toml Cargo.lock crates/pyenv/src/install/mod.rs crates/pyenv/src/install/openpgp.rs crates/pyenv/tests/install_openpgp.rs
git commit -m "Verify python.org MSI signatures against the three pinned CPython release keys"
```

---

### Task 2: MSI extraction equivalent to `msiexec /a`

**Files:**
- Modify: `crates/pyenv/Cargo.toml`, `crates/pyenv/src/install/mod.rs`
- Create: `crates/pyenv/src/install/msi.rs`
- Test: unit tests in `msi.rs`; `crates/pyenv/tests/install_msi.rs` (both OSes)

**Interfaces:**
- Consumes: `tests/fixtures/msi/tools-3.10.11-amd64.msi` and `.manifest`. The manifest was generated from `msiexec /a` output in the spike: one `path<TAB>size<TAB>sha256` line per file, with `/` separators, sorted.
- Produces:
  - `pub fn is_safe_win_segment(seg: &str) -> bool` in `install/mod.rs`, which Task 3 reuses;
  - `pub fn extract_msi(msi: &Path, target: &Path) -> Result<Vec<PathBuf>, String>`, which writes into `target` (created if needed) and returns the files written.

- [ ] **Step 1: Add the dependencies.** In `crates/pyenv/Cargo.toml` after the `pgp` line:

```toml
# MSI tables and cabinets, read in-process: no msiexec, no registry (spec §9.1, plan Decision 4).
msi = "0.10.0"
# Exact: `msi::cab_for_each_sequential` relies on how 0.6.0 parses a patched CFFILE record.
cab = "=0.6.0"
md-5 = "0.11.0"
```

- [ ] **Step 2: Add the shared name check, with its tests.** In `crates/pyenv/src/install/mod.rs`, after `child_of`, add:

```rust
/// One path segment that Windows writes as a plain file or directory name: not empty, `.` or
/// `..`; no separator, drive colon, wildcard or control character; not a DOS device name
/// (`CON`, `NUL`, `COM1`…, with or without an extension); and no trailing dot or space, which
/// Windows strips silently. Used for every name that comes from an archive or MSI table.
pub fn is_safe_win_segment(seg: &str) -> bool {
    const DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if seg.is_empty() || seg == "." || seg == ".." || seg.ends_with(['.', ' ']) {
        return false;
    }
    if seg
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*') || (c as u32) < 0x20)
    {
        return false;
    }
    let stem = seg.split('.').next().unwrap_or("").trim_end();
    !DEVICES.iter().any(|d| stem.eq_ignore_ascii_case(d))
}
```

and to its existing `#[cfg(test)] mod tests` (create one at the end of `mod.rs` if there is none):

```rust
    #[test]
    fn windows_segments() {
        for ok in ["python.exe", "Lib", "a b", "x.tar.gz", "COM10", "console.py", "nul_x"] {
            assert!(super::is_safe_win_segment(ok), "{ok}");
        }
        for bad in [
            "", ".", "..", "a/b", "a\\b", "C:", "a:b", "x*", "x?", "a|b", "a\u{1}b", "CON",
            "con", "NUL.txt", "com1.py", "LPT9", "trailing.", "trailing ",
        ] {
            assert!(!super::is_safe_win_segment(bad), "{bad:?}");
        }
    }
```

- [ ] **Step 3: Write the failing integration test.** Create `crates/pyenv/tests/install_msi.rs`:

```rust
//! The MSI extractor against `msiexec /a` (plan M2b Task 2). The manifest is what msiexec
//! wrote for python.org's 3.10.11 amd64 tools.msi: every file's path, size and SHA-256.

use pyenv::install::checksum::sha256_file;
use pyenv::install::msi::extract_msi;
use std::path::Path;

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            let size = std::fs::metadata(&p).unwrap().len();
            out.push(format!("{rel}\t{size}\t{}", sha256_file(&p).unwrap()));
        }
    }
}

#[test]
fn tools_msi_extracts_exactly_as_msiexec_did() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/msi");
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("t");
    let written = extract_msi(&fx.join("tools-3.10.11-amd64.msi"), &target).unwrap();
    assert_eq!(written.len(), 105);
    let mut got = Vec::new();
    walk(&target, &target, &mut got);
    got.sort();
    let want: Vec<String> = std::fs::read_to_string(fx.join("tools-3.10.11-amd64.manifest"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(got, want);
}

#[test]
fn a_file_that_is_not_an_msi_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.msi");
    std::fs::write(&p, b"not an msi").unwrap();
    let e = extract_msi(&p, &dir.path().join("t")).unwrap_err();
    assert!(e.starts_with("not an MSI"), "{e}");
}
```

- [ ] **Step 4: Run it and see it fail.**

Run: `cargo test -p pyenv --test install_msi`
Expected: compile error, `unresolved import pyenv::install::msi`.

- [ ] **Step 5: Implement `msi.rs`.** Add `pub mod msi;` to `install/mod.rs`. The module is the spike's `msiextract.rs`, the code compared with msiexec, with these changes:
  1. **No timing paths:** `CabMode`, `extract_msi_mode` and `cab_for_each_per_file` are gone. `extract_msi` calls the sequential reader directly.
  2. **Name checks:** `check_segment` delegates to `super::is_safe_win_segment`.
  3. **Fresh target:** `extract_msi` creates `target` first.
  4. **Unit tests** are added.

```rust
//! Administrative-install-equivalent extraction of a Windows Installer package, in pure Rust
//! (`msi` + `cab`): no msiexec, no registry, no custom actions (spec §9.1, plan Decision 4).
//! Compared with `msiexec /a` on 12 python.org MSIs in the M2b spike: 10,409 files, equal
//! path, size and SHA-256.
//!
//! Rules (each measured against msiexec):
//! * Every directory resolves under `target` by walking `Directory_Parent` up to the root row
//!   (`Directory_Parent` NULL or equal to itself), which is `target`.
//! * A `DefaultDir` is `[target][:source]`; an admin image uses the SOURCE part (the whole value
//!   when there is no colon). Each part is `short|long`; the LONG name is used, even when the
//!   summary Word Count sets bit 0 (short names). `.` adds no segment.
//! * `File.FileName` is `short|long`; the long name is written.
//! * A file is in a cabinet if its attributes have 0x4000, beside the MSI if 0x2000, otherwise
//!   per Word Count bit 1. Files beside the MSI are read flat by name (Word Count bit 1 set);
//!   python.org's MSIs have none.
//! * Only files of features with Level != 0 are written (the Condition table, INSTALLLEVEL and
//!   Component.Condition are ignored by an admin install).
//! * `Media.Cabinet` `#name` is a stream in the MSI; anything else is a file beside it.
//! * Nothing else is written: no copy of the .msi, no empty CreateFolder directories.
//! * Each written file's size must equal `File.FileSize`, and its MD5 the `MsiFileHash` row if
//!   there is one (an integrity check of the extraction; authenticity comes from the `.asc`).

use md5::{Digest as _, Md5};
use msi::{Package, Select};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

const ATTR_NONCOMPRESSED: i32 = 0x2000;
const ATTR_COMPRESSED: i32 = 0x4000;
const WC_SHORT_NAMES: i32 = 0x1;
const WC_COMPRESSED: i32 = 0x2;
```

Then copy these **verbatim** from `docs/superpowers/plans/2026-10-03-m2b-spike/msiextract.rs` (the spike's file, committed with this plan):
- `PlannedFile`, `MediaRow`, `Plan` (lines 34–64);
- `pick` (66–77);
- `str_col`, `io_err` (91–97);
- `plan_msi` (99–266);
- `rel_to_path` (268–274);
- `write_checked` (276–314);
- `CabEntry` and `parse_cffiles` (402–443);
- `cab_for_each_sequential` (445–512).

Replace the spike's `check_segment` (79–89) with:

```rust
/// Refuses any name that could escape the target or isn't a plain Windows name segment.
fn check_segment(seg: &str, what: &str) -> Result<(), String> {
    if super::is_safe_win_segment(seg) {
        Ok(())
    } else {
        Err(format!("unsafe {what} name segment {seg:?}"))
    }
}
```

and `extract_msi` with the spike's `extract_msi_mode` body (lines 331–400), minus the `mode` parameter and with this one change: before `let plan = plan_msi(...)`, add

```rust
    fs::create_dir_all(target).map_err(|e| format!("create {}: {e}", target.display()))?;
```

and replace the `match mode { … }` at lines 369–374 with

```rust
        cab_for_each_sequential(cab_bytes, &mut on_entry)
            .map_err(|e| format!("cabinet {cab_name:?}: {e}"))?;
```

Its signature is `pub fn extract_msi(msi: &Path, target: &Path) -> Result<Vec<PathBuf>, String>`.

**The spike file is the source of truth for these lines.** It is committed beside this plan exactly as it was compiled and compared with msiexec. Do not rewrite the extractor from the rules: the msiexec comparison validated that exact code.

Add unit tests at the end of `msi.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_and_the_source_part() {
        assert_eq!(pick("PYTHON~1|Python Tools", false), "Python Tools");
        assert_eq!(pick("PYTHON~1|Python Tools", true), "PYTHON~1");
        assert_eq!(pick("Lib", false), "Lib");
    }

    #[test]
    fn unsafe_segments_are_refused() {
        assert!(check_segment("..", "directory").is_err());
        assert!(check_segment("a\\b", "file").is_err());
        assert!(check_segment("CON", "file").is_err());
        assert!(check_segment("os.py", "file").is_ok());
    }

    fn planned(size: u64, md5: Option<[u8; 16]>) -> PlannedFile {
        PlannedFile {
            key: "k".into(),
            rel: vec!["f".into()],
            size,
            compressed: true,
            source_name: "f".into(),
            sequence: 1,
            md5,
            installed: true,
        }
    }

    #[test]
    fn a_size_that_differs_from_the_file_table_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let e = write_checked(&mut &b"abc"[..], &d.path().join("f"), &planned(4, None)).unwrap_err();
        assert!(e.contains("File.FileSize says 4"), "{e}");
        assert!(!d.path().join("f").exists());
    }

    #[test]
    fn an_md5_that_differs_from_msifilehash_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let e = write_checked(&mut &b"abc"[..], &d.path().join("f"), &planned(3, Some([7; 16])))
            .unwrap_err();
        assert!(e.contains("MD5 differs"), "{e}");
    }

    #[test]
    fn an_all_zero_hash_on_an_empty_file_is_a_marker_not_a_mismatch() {
        let d = tempfile::tempdir().unwrap();
        write_checked(&mut &b""[..], &d.path().join("f"), &planned(0, Some([0; 16]))).unwrap();
        assert!(d.path().join("f").is_file());
    }

    /// A two-folder MSZIP cabinet with a zero-length entry and a smart-cab duplicate, built
    /// with the `cab` crate; the sequential reader must hand back each entry's exact bytes.
    #[test]
    fn the_sequential_reader_returns_every_entry_once_and_whole() {
        let mut b = cab::CabinetBuilder::new();
        {
            let f = b.add_folder(cab::CompressionType::MsZip);
            f.add_file("a");
            f.add_file("empty");
            f.add_file("b");
        }
        b.add_folder(cab::CompressionType::None).add_file("c");
        let mut w = b.build(Cursor::new(Vec::new())).unwrap();
        let bodies: [&[u8]; 4] = [b"alpha".as_slice(), b"", &[b'x'; 70_000], b"charlie"];
        let mut i = 0;
        while let Some(mut fw) = w.next_file().unwrap() {
            fw.write_all(bodies[i]).unwrap();
            i += 1;
        }
        let bytes = w.finish().unwrap().into_inner();
        let mut got: Vec<(String, Vec<u8>)> = Vec::new();
        cab_for_each_sequential(bytes, &mut |name, r| {
            let mut v = Vec::new();
            r.read_to_end(&mut v).map_err(|e| e.to_string())?;
            got.push((name.to_string(), v));
            Ok(())
        })
        .unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("a".to_string(), b"alpha".to_vec()),
                ("b".to_string(), vec![b'x'; 70_000]),
                ("c".to_string(), b"charlie".to_vec()),
                ("empty".to_string(), Vec::new()),
            ]
        );
    }

    #[test]
    fn a_non_cabinet_is_refused() {
        let e = cab_for_each_sequential(b"PK\x03\x04".to_vec(), &mut |_, _| Ok(())).unwrap_err();
        assert_eq!(e, "not a cabinet");
    }
}
```

The `cab` builder API (`CabinetBuilder::new`, `add_folder`, `add_file`, `build`, `next_file`, `finish`) is from cab 0.6's documentation. If a name or signature differs, adjust it minimally and record the change.

- [ ] **Step 6: Run the tests and see them pass.**

Run: `cargo test -p pyenv --test install_msi` and `cargo test -p pyenv --lib install::`
Expected: both integration tests pass; all new unit tests pass.

- [ ] **Step 7: Mutation checks.** For each mutation, change the code, run the named test and see it fail, then restore.
  1. In `cab_for_each_sequential`, skip the duplicate-group branch (treat `j - i` as always 1). `the_sequential_reader_returns_every_entry_once_and_whole` must fail.
  2. In `write_checked`, remove the size comparison. `a_size_that_differs_from_the_file_table_is_an_error` must fail.
  3. In `plan_msi`, use the short name for files (`pick(&fname, true)`). `tools_msi_extracts_exactly_as_msiexec_did` must fail, because the manifest has long names.

  **Coverage notes; record them in your report, don't try to fix them here.** Two rules can't be pinned by `tools.msi`:
  - **the `target:source` rule for `DefaultDir`:** python.org's MSIs have no colon;
  - **the Level-0 feature rule:** `tools.msi` has no Level-0 features.

  Their evidence is the spike's msiexec comparison (synthetic MSIs, and 2.7.18's 16 Level-0 files), cited in the module header. Tier 2's nightly 2.7.18 install exercises the second rule on real data (Task 11).

- [ ] **Step 8: Check, both OSes, and commit.** Run `python ci/shim_deps.py`, then fmt, clippy and `cargo test --workspace` on Windows and in WSL.

```bash
git add crates/pyenv/Cargo.toml Cargo.lock crates/pyenv/src/install/mod.rs crates/pyenv/src/install/msi.rs crates/pyenv/tests/install_msi.rs
git commit -m "Extract MSIs in-process exactly as msiexec /a lays them out"
```

---

### Task 3: Zip extraction and fetch for Windows

**Files:**
- Modify: `crates/pyenv/Cargo.toml`, `crates/pyenv/src/install/mod.rs`, `crates/pyenv/src/install/fetch.rs`, `crates/pyenv/src/install/builder.rs` (the one `FetchRequest` literal)
- Create: `crates/pyenv/src/install/zipx.rs`
- Test: unit tests in `fetch.rs`; `crates/pyenv/tests/install_zip.rs` (both OSes)

**Interfaces:**
- Consumes: `is_safe_win_segment` (Task 2).
- Produces:
  - `pub fn extract_zip(archive: &Path, target: &Path) -> Result<usize, String>`, which returns the number of files written.
  - In `fetch.rs`:
    - `pub enum Check { Sha256(String), Caller }`; `FetchRequest.sha256: String` becomes `FetchRequest.check: Check`.
    - `pub fn file_url_path(url: &str) -> Option<PathBuf>`.
    - `Fetcher::direct() -> Fetcher`: no mirror, no cache.
    - `Fetcher::get_text(&self, url: &str) -> Result<String, TextError>`, where `pub struct TextError { pub status: Option<u16>, pub message: String }`.

- [ ] **Step 1: Add the dependency.** In `crates/pyenv/Cargo.toml`, after `md-5`:

```toml
# python.org's Install Manager zips (deflate only; flate2 is already linked).
zip = { version = "8.6.0", default-features = false, features = ["deflate-flate2"] }
```

- [ ] **Step 2: Write the failing zip tests.** Create `crates/pyenv/tests/install_zip.rs`:

```rust
//! Safe zip extraction (plan M2b Task 3; review focus 2).

use pyenv::install::zipx::extract_zip;
use std::io::Write;
use std::path::Path;

fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in entries {
        if name.ends_with('/') {
            w.add_directory(name.trim_end_matches('/'), opts).unwrap();
        } else {
            w.start_file(*name, opts).unwrap();
            w.write_all(body).unwrap();
        }
    }
    w.finish().unwrap();
}

#[test]
fn files_land_under_the_target_with_parents_created() {
    let d = tempfile::tempdir().unwrap();
    let z = d.path().join("p.zip");
    // python.org's zips have no directory entries (measured): parents must be created.
    zip_of(&z, &[("python.exe", b"MZ"), ("Lib/os.py", b"# os"), ("DLLs/x.pyd", b"")]);
    let t = d.path().join("t");
    assert_eq!(extract_zip(&z, &t).unwrap(), 3);
    assert_eq!(std::fs::read(t.join("python.exe")).unwrap(), b"MZ");
    assert_eq!(std::fs::read(t.join("Lib").join("os.py")).unwrap(), b"# os");
    assert!(t.join("DLLs").join("x.pyd").is_file());
}

#[test]
fn escaping_and_unsafe_names_are_refused_and_nothing_lands_outside() {
    for bad in ["../x", "a/../../x", "/abs", "\\abs", "C:/x", "a\\..\\..\\x", "CON", "a/NUL.txt", "dir./x"] {
        let d = tempfile::tempdir().unwrap();
        let z = d.path().join("p.zip");
        zip_of(&z, &[("ok.txt", b"1"), (bad, b"2")]);
        let t = d.path().join("a").join("t");
        let e = extract_zip(&z, &t).unwrap_err();
        assert!(e.contains("unsafe path"), "{bad}: {e}");
        assert!(!d.path().join("x").exists() && !d.path().join("a").join("x").exists(), "{bad}");
    }
}

#[test]
fn a_name_written_twice_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let z = d.path().join("p.zip");
    zip_of(&z, &[("Lib/a.py", b"1"), ("lib/A.py", b"2")]);
    let e = extract_zip(&z, &d.path().join("t")).unwrap_err();
    assert!(e.contains("twice"), "{e}");
}

#[test]
fn a_corrupt_zip_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let z = d.path().join("p.zip");
    std::fs::write(&z, b"PK\x03\x04garbage").unwrap();
    assert!(extract_zip(&z, &d.path().join("t")).is_err());
}
```

The zip writer used by the test needs the crate's `deflate-flate2` feature, which the dependency enables. `zip` is a normal dependency, so integration tests can use it.

- [ ] **Step 3: Run it and see it fail.**

Run: `cargo test -p pyenv --test install_zip`
Expected: compile error, `unresolved import pyenv::install::zipx`.

- [ ] **Step 4: Implement `zipx.rs`.** Add `pub mod zipx;` to `install/mod.rs`.

```rust
//! Extraction of python.org's Install Manager zips (spec §9.1). They have no root folder and
//! no directory entries (measured on 3.11.0 and 3.13.0t), so parents are created as needed.
//! Every name must be relative and made of plain Windows segments, and no path (compared
//! case-insensitively, as Windows does) may be written twice.

use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// `name` as a relative path of safe segments; `/` and `\` both separate.
fn safe_rel(name: &str) -> Option<PathBuf> {
    if name.starts_with(['/', '\\']) || name.contains(':') {
        return None;
    }
    let segs: Vec<&str> = name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if segs.is_empty() || !segs.iter().all(|s| super::is_safe_win_segment(s)) {
        return None;
    }
    Some(segs.iter().collect())
}

pub fn extract_zip(archive: &Path, target: &Path) -> Result<usize, String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut z = zip::ZipArchive::new(BufReader::new(f))
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    std::fs::create_dir_all(target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut files = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| format!("{}: {e}", archive.display()))?;
        let raw = e.name().to_string();
        let Some(rel) = safe_rel(&raw) else {
            return Err(format!("unsafe path in {}: {raw}", archive.display()));
        };
        let is_link = e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        if is_link {
            return Err(format!("unsafe path in {}: {raw} is a link", archive.display()));
        }
        let dest = target.join(&rel);
        if e.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|er| format!("{}: {er}", dest.display()))?;
            continue;
        }
        if !seen.insert(rel.to_string_lossy().to_lowercase()) {
            return Err(format!("{} writes {raw} twice", archive.display()));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|er| format!("{}: {er}", parent.display()))?;
        }
        let mut out =
            std::fs::File::create(&dest).map_err(|er| format!("{}: {er}", dest.display()))?;
        let want = e.size();
        let mut buf = vec![0u8; 1 << 16];
        let mut got: u64 = 0;
        loop {
            let n = e.read(&mut buf).map_err(|er| format!("{raw}: {er}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|er| format!("{}: {er}", dest.display()))?;
            got += n as u64;
        }
        if got != want {
            return Err(format!("{raw}: extracted {got} bytes, the zip says {want}"));
        }
        files += 1;
    }
    Ok(files)
}
```

`ZipFile::read` checks each entry's CRC-32 at its end. A corrupt entry is therefore an error, which `a_corrupt_zip_is_an_error` covers at the archive level.

- [ ] **Step 5: Run the zip tests and see them pass.**

Run: `cargo test -p pyenv --test install_zip`. Expected: 4 passed.

- [ ] **Step 6: Change `fetch.rs`: `Check`, `file://`, `direct`, `get_text`.**

1. Replace `pub sha256: String,` in `FetchRequest` with:

```rust
    /// What makes the bytes trustworthy.
    pub check: Check,
```

and add above the struct:

```rust
/// How a download is checked before it is used (spec §9.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// A SHA-256 its publisher published (python-build, python.org's index).
    Sha256(String),
    /// The caller verifies the file (a python.org `.asc`) before using it. No cache, no mirror.
    Caller,
}
```

2. In `fetch`:
   - Make the 64-hex validation apply only to `Check::Sha256(h)`.
   - Use `if let Check::Sha256(h) = &req.check` around both cache blocks, so a `Caller` request neither reads nor writes `PYTHON_BUILD_CACHE_PATH`.
   - Call `self.mirror_url(&req.url, h)` only for `Sha256`.
3. In `download`, replace `let sha256 = &req.sha256;` with nothing. In the `Attempt::Ok` arm, compare only for `Check::Sha256(want)`:

```rust
                Attempt::Ok => {
                    if let Check::Sha256(sha256) = &req.check {
                        let got = sha256_file(&part).unwrap_or_default();
                        if &got != sha256 {
                            let _ = std::fs::remove_file(&part);
                            let _ = write!(
                                log,
                                "\nchecksum mismatch: {file_name} (file is corrupt)\nexpected {sha256}, got {got}\n\n"
                            );
                            return Ok(Outcome::Mismatch);
                        }
                    }
                    if let Err(e) = std::fs::rename(&part, dest) {
                        let _ = std::fs::remove_file(&part);
                        return Err(InstallError::Message(format!(
                            "pyenv: cannot write {}: {e}",
                            dest.display()
                        )));
                    }
                    return Ok(Outcome::Done);
                }
```

4. Replace the `file://` branch of `attempt`:

```rust
        if url.starts_with("file:") {
            return match file_url_path(url) {
                Some(path) => match std::fs::copy(&path, part) {
                    Ok(_) => Attempt::Ok,
                    Err(e) => Attempt::Final(format!("{}: {e}", path.display())),
                },
                None => Attempt::Final(format!("unsupported file URL: {url}")),
            };
        }
```

and add, as a free function:

```rust
/// The local path of a `file:` URL: `file:///p`, `file://localhost/p`, and on Windows
/// `file:///C:/p` (the leading `/` before the drive dropped), percent-decoded. Other hosts and
/// invalid escapes give `None`.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(bytes).ok()?;
    let d = s.as_bytes();
    let s = if d.len() >= 3 && d[0] == b'/' && d[1].is_ascii_alphabetic() && d[2] == b':' {
        s[1..].to_string()
    } else {
        s
    };
    Some(PathBuf::from(s))
}
```

5. Factor the agent construction in `from_env` into `fn agent() -> ureq::Agent`, unchanged, and add:

```rust
    /// No mirror and no python-build cache: pyenv-win's installs fetch from python.org only,
    /// and keep their own `install_cache` (plan M2b Decisions 8 and the base-URL constraint).
    pub fn direct() -> Fetcher {
        Fetcher {
            agent: agent(),
            mirror: None,
            cache: None,
            retry_delay: Duration::from_secs(1),
            stall_timeout: Duration::from_secs(60),
        }
    }

    /// A small text resource (a folder listing, an index page): up to 3 attempts for transient
    /// failures, at most 16 MiB, decoded as UTF-8.
    pub fn get_text(&self, url: &str) -> Result<String, TextError> {
        let mut last = TextError { status: None, message: String::new() };
        for attempt in 1..=3u32 {
            if attempt > 1 {
                std::thread::sleep(self.retry_delay * (attempt - 1));
            }
            if interrupted() {
                return Err(TextError { status: None, message: "interrupted".into() });
            }
            match self.agent.get(url).call() {
                Ok(mut r) => {
                    return r
                        .body_mut()
                        .with_config()
                        .limit(16 << 20)
                        .read_to_string()
                        .map_err(|e| TextError { status: None, message: e.to_string() });
                }
                Err(ureq::Error::StatusCode(code)) if code < 500 => {
                    return Err(TextError {
                        status: Some(code),
                        message: format!("HTTP {code}"),
                    })
                }
                Err(ureq::Error::StatusCode(code)) => {
                    last = TextError { status: Some(code), message: format!("HTTP {code}") }
                }
                Err(e) if is_transient(&e) => {
                    last = TextError { status: None, message: e.to_string() }
                }
                Err(e) => return Err(TextError { status: None, message: e.to_string() }),
            }
        }
        Err(last)
    }
```

with, near `Attempt`:

```rust
/// Why `get_text` failed: the HTTP status, when there was one, and a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError {
    pub status: Option<u16>,
    pub message: String,
}
```

6. In `builder.rs`, the `FetchRequest { … sha256: <expr>, … }` literal (around line 694) becomes `check: Check::Sha256(<expr>)`, importing `Check` from `super::fetch`.

7. Add unit tests to `fetch.rs`'s `mod tests`:

```rust
    #[test]
    fn file_urls_are_percent_decoded_and_drive_letters_kept() {
        assert_eq!(file_url_path("file:///tmp/a%20b"), Some(PathBuf::from("/tmp/a b")));
        assert_eq!(file_url_path("file://localhost/x"), Some(PathBuf::from("/x")));
        assert_eq!(file_url_path("file:///C:/py%C3%B1/x.zip"), Some(PathBuf::from("C:/pyñ/x.zip")));
        assert_eq!(file_url_path("file://host/x"), None);
        assert_eq!(file_url_path("file:///bad%zz"), None);
    }

    #[test]
    fn a_caller_checked_request_never_uses_the_mirror() {
        let f = with(&[("PYTHON_BUILD_MIRROR_URL", "https://m.example")]);
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"payload").unwrap();
        let url = format!("file:///{}", src.display().to_string().replace('\\', "/").trim_start_matches('/'));
        let req = FetchRequest {
            file_name: "out.bin".into(),
            url,
            check: Check::Caller,
            dest_dir: dir.path().to_path_buf(),
        };
        let mut log = Vec::new();
        let got = f.fetch(&req, &mut log, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(got).unwrap(), b"payload");
        assert!(!String::from_utf8_lossy(&log).contains("mirror"));
    }
```

- [ ] **Step 7: Run the tests and see them pass.**

Run: `cargo test -p pyenv` on Windows. In WSL, run it from a script file.
Expected: every M2a test still passes. The Linux `cli_install` tests use `Check::Sha256` through the builder, and their checksum-mismatch test must still fail on a mismatch. The new tests pass.

- [ ] **Step 8: Mutation check.** Make `Check::Caller`'s branch compare a SHA-256 against an empty string. `a_caller_checked_request_never_uses_the_mirror` must fail with a mismatch. Restore.

- [ ] **Step 9: Check, both OSes, and commit.** Run `python ci/shim_deps.py`, then fmt, clippy and `cargo test --workspace` on both OSes.

```bash
git add crates/pyenv/Cargo.toml Cargo.lock crates/pyenv/src/install/mod.rs crates/pyenv/src/install/zipx.rs crates/pyenv/src/install/fetch.rs crates/pyenv/src/install/builder.rs crates/pyenv/tests/install_zip.rs
git commit -m "Extract zips safely; let the fetcher take caller-verified files, decode file URLs and read text"
```

---

### Task 4: Version codes and pyenv-win's version cache

**Files:**
- Create: `crates/pyenv/src/install/wincatalog.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod wincatalog;`)
- Test: `crates/pyenv/tests/install_wincatalog.rs` (both OSes)

**Interfaces:**
- Consumes: the committed fixture `tests/fixtures/pyenv-win/versions_cache.xml`, which is pyenv-win 856ed5a's own cache (901 rows, MIT, `LICENSE` beside it).
- Produces:
  - `pub enum Arch { Win32, Arm64, Amd64 }`, ordered as pyenv-win orders a version's builds, with `suffix()`, `word()` and `x64()`.
  - `pub struct Code { text, version, numeric, nums: [u64; 3], pre: Option<(String, u64)>, ft: bool, arch: Arch }` with `sort_key()`.
  - `pub fn parse_code(&str) -> Option<Code>`.
  - `pub struct Row { code, file, url, x64, web_install, msi, zip_root_dir: Option<String> }`.
  - `pub const DB_NAME: &str = ".versions_cache.xml"`.
  - `pub enum DbError { Missing, Empty, Malformed(String) }`.
  - `pub fn read_db(root) -> Result<Vec<Row>, DbError>`, `parse_db(&str)`, `render_db(&[Row]) -> String`, `write_db(root, &[Row]) -> io::Result<()>` and `sort_rows(&mut Vec<Row>)`.

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/install_wincatalog.rs`:

```rust
//! pyenv-win's version codes and `.versions_cache.xml` (plan M2b Task 4; review focus 5).

use pyenv::install::wincatalog::{
    parse_code, parse_db, read_db, render_db, sort_rows, write_db, Arch, DbError, Row,
};
use std::path::Path;

fn real_db() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pyenv-win/versions_cache.xml"),
    )
    .unwrap()
}

#[test]
fn codes_parse_into_version_build_and_arch() {
    let c = parse_code("3.10.0rc2-win32").unwrap();
    assert_eq!((c.version.as_str(), c.numeric.as_str(), c.nums), ("3.10.0rc2", "3.10.0", [3, 10, 0]));
    assert_eq!((c.pre.clone(), c.ft, c.arch), (Some(("rc".to_string(), 2)), false, Arch::Win32));
    let t = parse_code("3.13.0t-arm").unwrap();
    assert_eq!((t.version.as_str(), t.ft, t.arch), ("3.13.0", true, Arch::Arm64));
    let old = parse_code("2.4.3c1-win32").unwrap();
    assert_eq!((old.numeric.as_str(), old.pre.clone()), ("2.4.3", Some(("c".to_string(), 1))));
    assert_eq!(parse_code("2.7").unwrap().nums, [2, 7, 0]);
    assert_eq!(parse_code("3.12.1").unwrap().arch, Arch::Amd64);
    for bad in ["3.12.1-arm64", "3.12.1-amd64", "pypy3.10-v7.3.19-win64", "3", "3.12.", "3.12.1rc", "3.12.1x1", "graalpy-25.0.1-windows-amd64", ""] {
        assert!(parse_code(bad).is_none(), "{bad}");
    }
}

#[test]
fn pyenv_wins_real_cache_reads_whole() {
    let rows = parse_db(&real_db()).unwrap();
    assert_eq!(rows.len(), 901);
    let first: Vec<&str> = rows.iter().take(5).map(|r| r.code.as_str()).collect();
    assert_eq!(first, ["2.4-win32", "2.4.1-win32", "2.4.2-win32", "2.4.3c1-win32", "2.4.3-win32"]);
    assert_eq!(rows.last().unwrap().code, "graalpy-25.0.1-windows-amd64");
    let pypy = rows.iter().find(|r| r.code == "pypy3.10-v7.3.19-win64").unwrap();
    assert_eq!(pypy.zip_root_dir.as_deref(), Some("pypy3.10-v7.3.19-win64"));
    assert!(pypy.x64 && !pypy.msi);
    let msi = &rows[0];
    assert_eq!((msi.x64, msi.msi, msi.web_install), (false, true, false));
    assert_eq!(msi.url, "https://www.python.org/ftp/python/2.4/python-2.4.msi");
}

/// The order pyenv-win's own cache is written in is the order `sort_rows` produces: measured
/// against upstream's 901-row file rather than assumed.
#[test]
fn sort_rows_reproduces_pyenv_wins_order() {
    let rows = parse_db(&real_db()).unwrap();
    let mut sorted = rows.clone();
    sort_rows(&mut sorted);
    let a: Vec<&str> = rows.iter().map(|r| r.code.as_str()).collect();
    let b: Vec<&str> = sorted.iter().map(|r| r.code.as_str()).collect();
    assert_eq!(a, b);
}

#[test]
fn render_round_trips_in_the_vbscript_writers_format() {
    let rows = parse_db(&real_db()).unwrap();
    let text = render_db(&rows);
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?>\r\n<versions>\r\n\t<version x64=\"false\" webInstall=\"false\" msi=\"true\">\r\n\t\t<code>2.4-win32</code>\r\n"));
    assert!(text.ends_with("\t</version>\r\n</versions>"), "no final newline");
    assert!(!text.replace("\r\n", "").contains('\n'), "CRLF only");
    assert_eq!(parse_db(&text).unwrap(), rows);
}

#[test]
fn special_characters_are_escaped_and_unescaped() {
    let rows = vec![Row {
        code: "3.12.1".into(),
        file: "a&b<c>.exe".into(),
        url: "https://h/x?\"y\"".into(),
        x64: true,
        web_install: false,
        msi: false,
        zip_root_dir: None,
    }];
    assert_eq!(parse_db(&render_db(&rows)).unwrap(), rows);
}

#[test]
fn a_missing_empty_or_broken_cache_is_reported_not_a_panic() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(read_db(d.path()), Err(DbError::Missing));
    std::fs::write(d.path().join(".versions_cache.xml"), "<?xml version=\"1.0\"?>\n<versions>\n</versions>\n").unwrap();
    assert_eq!(read_db(d.path()), Err(DbError::Empty));
    for broken in ["garbage", "<versions><version><code>3.1</code></versions>", "<versions><version x64=\"true\"><file>f</file><URL>u</URL></version></versions>"] {
        std::fs::write(d.path().join(".versions_cache.xml"), broken).unwrap();
        assert!(matches!(read_db(d.path()), Err(DbError::Malformed(_))), "{broken}");
    }
}

#[test]
fn write_db_replaces_the_file_whole() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".versions_cache.xml"), "old").unwrap();
    let rows = parse_db(&real_db()).unwrap()[..3].to_vec();
    write_db(d.path(), &rows).unwrap();
    assert_eq!(read_db(d.path()).unwrap(), rows);
    let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
    assert_eq!(left.len(), 1, "no temporary file left");
}
```

- [ ] **Step 2: Run them and see them fail.**

Run: `cargo test -p pyenv --test install_wincatalog`
Expected: compile error, `unresolved import pyenv::install::wincatalog`.

- [ ] **Step 3: Implement.** Create `crates/pyenv/src/install/wincatalog.rs`:

```rust
//! pyenv-win's version codes and its version cache `<root>\.versions_cache.xml` (spec §9.1,
//! plan M2b Decision 2): the codes `install --list` prints, `install` checks against and
//! `latest -k` resolves among. It is pyenv-win's own format, so a cache pyenv-win wrote works
//! unchanged; `pyenv update` writes it as pyenv-win's VBScript writer does (CRLF, tabs, no
//! final newline).

use std::path::Path;

/// A build's architecture, in the order pyenv-win lists one version's builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Arch {
    Win32,
    Arm64,
    Amd64,
}

impl Arch {
    /// The code suffix: `-win32`, `-arm` (pyenv-win's name for ARM64 builds), or none.
    pub fn suffix(self) -> &'static str {
        match self {
            Arch::Win32 => "-win32",
            Arch::Arm64 => "-arm",
            Arch::Amd64 => "",
        }
    }

    /// python.org's word for it, in folder and file names.
    pub fn word(self) -> &'static str {
        match self {
            Arch::Win32 => "win32",
            Arch::Arm64 => "arm64",
            Arch::Amd64 => "amd64",
        }
    }

    pub fn x64(self) -> bool {
        self != Arch::Win32
    }

    pub fn from_word(w: &str) -> Option<Arch> {
        match w {
            "win32" => Some(Arch::Win32),
            "arm64" => Some(Arch::Arm64),
            "amd64" => Some(Arch::Amd64),
            _ => None,
        }
    }
}

/// A CPython version code: `X.Y[.Z][pre][t][-win32|-arm]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// The code as written.
    pub text: String,
    /// The version as python.org writes it: `3.10.0rc2`, `2.4.3c1`, `2.7`.
    pub version: String,
    /// The numeric part, which is also the python.org folder: `3.10.0`, `2.7`.
    pub numeric: String,
    /// Major, minor, patch (a missing patch is 0).
    pub nums: [u64; 3],
    /// `a`, `b`, `c` or `rc`, and its number.
    pub pre: Option<(String, u64)>,
    /// A free-threaded build (`t`).
    pub ft: bool,
    pub arch: Arch,
}

impl Code {
    /// pyenv-win's order: version, pre-releases before the final, then regular before
    /// free-threaded builds, then win32, arm, amd64.
    pub fn sort_key(&self) -> ([u64; 3], u8, u64, bool, Arch) {
        let (rank, n) = match &self.pre {
            None => (3, 0),
            Some((w, n)) => (
                match w.as_str() {
                    "a" => 0,
                    "b" => 1,
                    _ => 2,
                },
                *n,
            ),
        };
        (self.nums, rank, n, self.ft, self.arch)
    }

    /// The pre-release tag as python.org writes it in folder names (`rc2`), or "".
    pub fn pre_tag(&self) -> String {
        self.pre.as_ref().map(|(w, n)| format!("{w}{n}")).unwrap_or_default()
    }
}

pub fn parse_code(s: &str) -> Option<Code> {
    let (rest, arch) = if let Some(r) = s.strip_suffix("-win32") {
        (r, Arch::Win32)
    } else if let Some(r) = s.strip_suffix("-arm") {
        (r, Arch::Arm64)
    } else {
        (s, Arch::Amd64)
    };
    let (rest, ft) = match rest.strip_suffix('t') {
        Some(r) if r.ends_with(|c: char| c.is_ascii_digit()) => (r, true),
        _ => (rest, false),
    };
    let num_end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let numeric = &rest[..num_end];
    let parts: Vec<&str> = numeric.split('.').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut nums = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = p.parse().ok()?;
    }
    let tail = &rest[num_end..];
    let pre = if tail.is_empty() {
        None
    } else {
        let w_end = tail.find(|c: char| c.is_ascii_digit())?;
        let (w, n) = tail.split_at(w_end);
        if !matches!(w, "a" | "b" | "c" | "rc") || !n.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((w.to_string(), n.parse().ok()?))
    };
    Some(Code {
        text: s.to_string(),
        version: rest.to_string(),
        numeric: numeric.to_string(),
        nums,
        pre,
        ft,
        arch,
    })
}

/// One `<version>` row of the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub code: String,
    pub file: String,
    pub url: String,
    pub x64: bool,
    pub web_install: bool,
    pub msi: bool,
    pub zip_root_dir: Option<String>,
}

pub const DB_NAME: &str = ".versions_cache.xml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    Missing,
    /// The file has no `<version>` rows.
    Empty,
    Malformed(String),
}

pub fn read_db(root: &Path) -> Result<Vec<Row>, DbError> {
    let bytes = match std::fs::read(root.join(DB_NAME)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(DbError::Missing),
        Err(e) => return Err(DbError::Malformed(e.to_string())),
    };
    let text = String::from_utf8(bytes).map_err(|_| DbError::Malformed("not UTF-8".into()))?;
    parse_db(&text)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The text of `<tag>…</tag>` inside `block`, if present.
fn element(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(unescape(&block[start..end]))
}

fn attr(tag: &str, name: &str, default: bool) -> Result<bool, DbError> {
    let key = format!("{name}=\"");
    let Some(i) = tag.find(&key) else {
        return Ok(default);
    };
    let v = &tag[i + key.len()..];
    let v = &v[..v.find('"').ok_or_else(|| DbError::Malformed(format!("bad {name}")))?];
    match v {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(DbError::Malformed(format!("{name}=\"{v}\""))),
    }
}

/// The schema's defaults are `x64=false`, `webInstall=false` and `msi=true`
/// (docs/parity/pyenv-win-m2-reference.md, "Format of .versions_cache.xml").
pub fn parse_db(text: &str) -> Result<Vec<Row>, DbError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(root) = text.find("<versions") else {
        return Err(DbError::Malformed("no <versions> element".into()));
    };
    let mut rest = &text[root + "<versions".len()..];
    let mut rows = Vec::new();
    loop {
        let Some(i) = rest.find("<version") else { break };
        rest = &rest[i..];
        // `<version ` or `<version>`; `</versions>` never matches `<version`.
        let after = rest.as_bytes().get("<version".len()).copied();
        if !matches!(after, Some(b' ' | b'>' | b'\t' | b'\r' | b'\n')) {
            rest = &rest["<version".len()..];
            continue;
        }
        let tag_end = rest.find('>').ok_or_else(|| DbError::Malformed("unclosed <version".into()))?;
        let tag = &rest[..tag_end];
        let close = rest
            .find("</version>")
            .ok_or_else(|| DbError::Malformed("<version> without </version>".into()))?;
        let block = &rest[tag_end + 1..close];
        let need = |t: &str| {
            element(block, t).ok_or_else(|| DbError::Malformed(format!("a <version> without <{t}>")))
        };
        rows.push(Row {
            code: need("code")?,
            file: need("file")?,
            url: need("URL")?,
            x64: attr(tag, "x64", false)?,
            web_install: attr(tag, "webInstall", false)?,
            msi: attr(tag, "msi", true)?,
            zip_root_dir: element(block, "zipRootDir"),
        });
        rest = &rest[close + "</version>".len()..];
    }
    if !text.contains("</versions>") {
        return Err(DbError::Malformed("no </versions>".into()));
    }
    if rows.is_empty() {
        return Err(DbError::Empty);
    }
    Ok(rows)
}

/// The file pyenv-win's `SaveVersionsXML` writes: CRLF, tab indentation, no final newline
/// (reference, "Format of .versions_cache.xml").
pub fn render_db(rows: &[Row]) -> String {
    let b = |v: bool| if v { "true" } else { "false" };
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?>\r\n<versions>\r\n");
    for r in rows {
        s.push_str(&format!(
            "\t<version x64=\"{}\" webInstall=\"{}\" msi=\"{}\">\r\n",
            b(r.x64),
            b(r.web_install),
            b(r.msi)
        ));
        s.push_str(&format!("\t\t<code>{}</code>\r\n", escape(&r.code)));
        s.push_str(&format!("\t\t<file>{}</file>\r\n", escape(&r.file)));
        s.push_str(&format!("\t\t<URL>{}</URL>\r\n", escape(&r.url)));
        if let Some(z) = &r.zip_root_dir {
            s.push_str(&format!("\t\t<zipRootDir>{}</zipRootDir>\r\n", escape(z)));
        }
        s.push_str("\t</version>\r\n");
    }
    s.push_str("</versions>");
    s
}

/// Replaces the cache whole, through a temporary file in the same folder.
pub fn write_db(root: &Path, rows: &[Row]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let tmp = root.join(format!("{DB_NAME}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, render_db(rows))?;
    std::fs::rename(&tmp, root.join(DB_NAME)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// CPython rows in pyenv-win's order; other rows (PyPy, GraalPy) after them, in their order.
pub fn sort_rows(rows: &mut Vec<Row>) {
    let (mut cpython, others): (Vec<Row>, Vec<Row>) = rows
        .drain(..)
        .partition(|r| r.zip_root_dir.is_none() && parse_code(&r.code).is_some());
    cpython.sort_by_key(|r| parse_code(&r.code).map(|c| c.sort_key()));
    rows.extend(cpython);
    rows.extend(others);
}
```

- [ ] **Step 4: Run the tests and see them pass.**

Run: `cargo test -p pyenv --test install_wincatalog`. Expected: 7 passed.

If `sort_rows_reproduces_pyenv_wins_order` fails, the first differing pair shows where pyenv-win's order differs from `sort_key`, for example how a `c` pre-release or an `X.Y` code ranks. Adjust `sort_key` to match the real file and record the rule in your report. **Never edit the fixture.** Upstream's own file is the measurement.

- [ ] **Step 5: Mutation check.** Swap `Arch`'s variant order to `Amd64, Win32, Arm64`. `sort_rows_reproduces_pyenv_wins_order` must fail. Restore.

- [ ] **Step 6: Check, both OSes, and commit.** Run fmt, clippy and `cargo test --workspace` on both OSes.

```bash
git add crates/pyenv/src/install/mod.rs crates/pyenv/src/install/wincatalog.rs crates/pyenv/tests/install_wincatalog.rs crates/pyenv/tests/fixtures/pyenv-win
git commit -m "Read and write pyenv-win's version cache, ordered as pyenv-win orders it"
```

---

### Task 5: python.org sources and `pyenv update`

**Files:**
- Modify: `crates/pyenv/Cargo.toml`, `crates/pyenv/src/install/mod.rs`, `crates/pyenv/src/commands/mod.rs`
- Create: `crates/pyenv/src/install/winsource.rs`, `crates/pyenv/src/commands/update.rs`, `crates/pyenv/tests/common/winfake.rs`
- Test: unit tests in `winsource.rs`; `crates/pyenv/tests/cli_update_win.rs` (`#![cfg(windows)]`)

**Interfaces:**
- Consumes: `Fetcher::direct`, `get_text` and `TextError` (Task 3); `wincatalog::{Arch, Row, parse_code, sort_rows, write_db}` (Task 4).
- Produces, in `winsource`:
  - `pub const PYTHON_ORG`
  - `pub fn base() -> String`
  - `pub fn banner() -> String`
  - `pub fn listing_names(&str) -> Vec<String>`
  - `pub fn version_folders(&[String]) -> Vec<String>`
  - `pub fn folder_rows(base, folder, &[String]) -> Vec<Row>`
  - `pub struct IndexZip { version, ft, arch, url, file, sha256 }`
  - `pub fn index_page(json, page_url, base) -> Result<(Vec<IndexZip>, Option<String>), String>`
  - `pub fn index_zips(&Fetcher, base) -> Result<(Vec<IndexZip>, usize), (String, TextError)>`
  - `pub fn catalog(Vec<Row>, &[IndexZip]) -> Vec<Row>`
- Also produces the `commands::update::update` command and `update::HELP`, plus the test helpers `tests/common/winfake.rs`: `listing`, `index_json` and `zip_bytes`.

- [ ] **Step 1: Add the dependency.** In `crates/pyenv/Cargo.toml`, after `zip`:

```toml
# python.org's Install Manager index (JSON).
serde_json = "1"
```

- [ ] **Step 2: Write the unit tests first,** at the end of the new `crates/pyenv/src/install/winsource.rs`. Create the file with only `#[cfg(test)] mod tests { … }` below and `pub mod winsource;` in `install/mod.rs`. The other items come in Step 4.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::wincatalog::Arch;

    const NGINX: &str = "<html><body><h1>Index of /ftp/python/3.10.0/</h1><hr><pre><a href=\"../\">../</a>\n<a href=\"amd64/\">amd64/</a>   01-Oct-2021 15:16    -\n<a href=\"amd64rc2/\">amd64rc2/</a>\n<a href=\"win32/\">win32/</a>\n<a href=\"win32rc2/\">win32rc2/</a>\n<a href=\"respun/\">respun/</a>\n<a href=\"?C=M;O=A\">sort</a>\n<a href=\"python-3.10.0-amd64.exe\">python-3.10.0-amd64.exe</a>\n</pre></body></html>";

    #[test]
    fn listing_names_skip_parent_query_and_absolute_links() {
        assert_eq!(
            listing_names(NGINX),
            ["amd64/", "amd64rc2/", "win32/", "win32rc2/", "respun/", "python-3.10.0-amd64.exe"]
        );
    }

    #[test]
    fn version_folders_start_at_2_4() {
        let names: Vec<String> = ["2.3.7/", "2.4/", "2.7.18/", "3.10.0/", "3.15.0/", "doc/", "3.x/", "index-windows.json"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(version_folders(&names), ["2.4", "2.7.18", "3.10.0", "3.15.0"]);
    }

    fn codes(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.code.as_str()).collect()
    }

    #[test]
    fn a_component_folder_gives_one_code_per_arch_folder() {
        let rows = folder_rows("B", "3.10.0", &listing_names(NGINX));
        assert_eq!(codes(&rows), ["3.10.0", "3.10.0rc2", "3.10.0-win32", "3.10.0rc2-win32"]);
        let rc = &rows[1];
        assert_eq!((rc.file.as_str(), rc.url.as_str()), ("python-3.10.0rc2-amd64.exe", "B/3.10.0/python-3.10.0rc2-amd64.exe"));
        assert!(rc.x64 && !rc.msi);
        let arm = folder_rows("B", "3.12.5", &["arm64/".to_string()]);
        assert_eq!((arm[0].code.as_str(), arm[0].file.as_str()), ("3.12.5-arm", "python-3.12.5-arm64.exe"));
    }

    #[test]
    fn a_single_msi_folder_gives_its_msis_and_skips_odd_names() {
        let names: Vec<String> = ["python-2.7.18.msi", "python-2.7.18.msi.asc", "python-2.7.18.amd64.msi", "python-2.7.18rc1.msi", "python-2.4.ia64.msi", "python-3.0a2.x86.msi", "Python-2.7.18.tgz"]
            .iter().map(|s| s.to_string()).collect();
        let rows = folder_rows("B", "2.7.18", &names);
        assert_eq!(codes(&rows), ["2.7.18-win32", "2.7.18", "2.7.18rc1-win32"]);
        assert!(rows[1].x64 && rows[1].msi);
        assert_eq!(rows[1].url, "B/2.7.18/python-2.7.18.amd64.msi");
        // A component-era folder never yields single-MSI rows, and the reverse.
        assert!(folder_rows("B", "3.5.0", &["python-3.5.0.msi".to_string()]).is_empty());
        assert!(folder_rows("B", "3.4.4", &["amd64/".to_string()]).is_empty());
    }

    fn index(base: &str, next: Option<&str>, entries: &[(&str, &str, &str)]) -> String {
        let v: Vec<String> = entries
            .iter()
            .map(|(company, url, sha)| format!(
                "{{\"schema\":1,\"id\":\"x\",\"sort-version\":\"{}\",\"company\":\"{company}\",\"url\":\"{url}\",\"hash\":{{\"sha256\":\"{sha}\"}}}}",
                url.rsplit('/').next().unwrap().trim_start_matches("python-").split('-').next().unwrap().trim_end_matches('t')
            ))
            .collect();
        let next = next.map(|n| format!(",\"next\":\"{n}\"")).unwrap_or_default();
        format!("{{\"versions\":[{}]{next}}}", v.join(","))
    }

    const SHA: &str = "11a906a2f36cacaee938c048968d99aa68ec0db592693b5a0fe3b161bb280ec5";

    #[test]
    fn index_pages_yield_pythoncore_zips_and_the_next_page() {
        let b = "https://www.python.org/ftp/python";
        let json = index(b, Some("index-windows-recent.json"), &[
            ("PythonCore", "https://www.python.org/ftp/python/3.12.1/python-3.12.1-amd64.zip", SHA),
            ("PythonCore", "https://www.python.org/ftp/python/3.13.0/python-3.13.0t-arm64.zip", SHA),
            ("PythonEmbed", "https://www.python.org/ftp/python/3.12.1/python-3.12.1-embeddable-amd64.zip", SHA),
            ("PythonCore", "https://api.nuget.org/v3-flatcontainer/python/3.10.11/python.3.10.11.nupkg", SHA),
            ("PythonCore", "https://evil.example/ftp/python/3.12.1/python-3.12.1-win32.zip", SHA),
            ("PythonCore", "https://www.python.org/ftp/python/3.12.2/python-3.12.2-win32.zip", "abc"),
        ]);
        let (zips, next) = index_page(&json, &format!("{b}/index-windows.json"), b).unwrap();
        assert_eq!(next.as_deref(), Some("https://www.python.org/ftp/python/index-windows-recent.json"));
        assert_eq!(zips.len(), 2, "{zips:?}");
        assert_eq!((zips[0].version.as_str(), zips[0].ft, zips[0].arch), ("3.12.1", false, Arch::Amd64));
        assert_eq!((zips[1].version.as_str(), zips[1].ft, zips[1].arch), ("3.13.0", true, Arch::Arm64));
        assert_eq!(zips[0].file, "python-3.12.1-amd64.zip");
        assert!(index_page("not json", "u", b).is_err());
    }

    #[test]
    fn the_catalog_adds_zip_only_codes_once_and_sorts() {
        let rows = folder_rows("B", "3.13.0", &["amd64/".to_string(), "win32/".to_string()]);
        let zip = |v: &str, ft: bool, arch: Arch| IndexZip {
            version: v.into(), ft, arch,
            url: format!("B/{v}/z.zip"), file: "z.zip".into(), sha256: SHA.into(),
        };
        let cat = catalog(rows, &[zip("3.13.0", false, Arch::Amd64), zip("3.13.0", true, Arch::Amd64), zip("3.13.0", true, Arch::Win32)]);
        assert_eq!(codes(&cat), ["3.13.0-win32", "3.13.0", "3.13.0t-win32", "3.13.0t"]);
        let t = cat.iter().find(|r| r.code == "3.13.0t").unwrap();
        assert_eq!((t.url.as_str(), t.msi), ("B/3.13.0/z.zip", false));
    }
}
```

- [ ] **Step 3: Run them and see them fail.**

Run: `cargo test -p pyenv --lib install::winsource`
Expected: compile errors for the missing items.

- [ ] **Step 4: Implement `winsource.rs`** above the tests:

```rust
//! python.org as rpyenv reads it on Windows (spec §9.1, plan M2b Decisions 1–3): the base URL,
//! nginx folder listings, the Install Manager index, and the catalog `pyenv update` builds.

use super::fetch::{Fetcher, TextError};
use super::wincatalog::{parse_code, sort_rows, Arch, Row};

pub const PYTHON_ORG: &str = "https://www.python.org/ftp/python";

/// The base installs read from: python.org. Debug builds honor `RPYENV_TEST_PYTHON_ORG` (the
/// tier-1 fake server); release builds ignore it, so the trust anchor can't be redirected.
pub fn base() -> String {
    #[cfg(debug_assertions)]
    if let Some(v) = std::env::var("RPYENV_TEST_PYTHON_ORG").ok().filter(|v| !v.is_empty()) {
        return v.trim_end_matches('/').to_string();
    }
    PYTHON_ORG.to_string()
}

/// `PYTHON_BUILD_MIRROR_URL`, when set and non-empty.
pub fn mirror() -> Option<String> {
    std::env::var("PYTHON_BUILD_MIRROR_URL").ok().filter(|v| !v.is_empty())
}

/// pyenv-win's banner, printed by `install` and `update` before anything else (reference,
/// "The mirror banner"): one line per mirror, two spaces after `::`.
pub fn banner() -> String {
    match mirror() {
        Some(m) => format!(":: [Info] ::  Mirror: {m}\n"),
        None => ":: [Info] ::  Mirror: https://www.python.org/ftp/python\n:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json\n:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases\n".to_string(),
    }
}

/// `href` targets of an nginx autoindex page, in page order, without `../`, sort links,
/// absolute links or other hosts.
pub fn listing_names(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let Some(end) = rest.find('"') else { break };
        let h = &rest[..end];
        rest = &rest[end..];
        if h.is_empty() || h.starts_with(['?', '/', '#']) || h == "../" || h.contains("://") {
            continue;
        }
        out.push(h.to_string());
    }
    out
}

fn numeric_folder(name: &str) -> Option<[u64; 3]> {
    let parts: Vec<&str> = name.split('.').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut n = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        n[i] = p.parse().ok()?;
    }
    Some(n)
}

/// The version folders of the root listing that can hold a Windows package: 2.4 and later.
pub fn version_folders(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|n| n.strip_suffix('/'))
        .filter(|n| numeric_folder(n).is_some_and(|v| v >= [2, 4, 0]))
        .map(String::from)
        .collect()
}

/// The cache rows one version folder's listing offers (Decision 3): single MSIs below 3.5,
/// one code per `<arch><pre>/` component folder from 3.5 on.
pub fn folder_rows(base: &str, folder: &str, names: &[String]) -> Vec<Row> {
    let Some(v) = numeric_folder(folder) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if v < [3, 5, 0] {
        for n in names {
            let Some(stem) = n.strip_prefix("python-").and_then(|s| s.strip_suffix(".msi")) else {
                continue;
            };
            let (ver, arch) = match stem.strip_suffix(".amd64") {
                Some(v) => (v, Arch::Amd64),
                None => (stem, Arch::Win32),
            };
            let code = format!("{ver}{}", arch.suffix());
            // The file's version must be this folder's (or one of its pre-releases).
            let ok = parse_code(&code).is_some_and(|c| !c.ft && c.numeric == folder && c.arch == arch);
            if ok {
                rows.push(Row {
                    code,
                    file: n.clone(),
                    url: format!("{base}/{folder}/{n}"),
                    x64: arch.x64(),
                    web_install: false,
                    msi: true,
                    zip_root_dir: None,
                });
            }
        }
    } else {
        for n in names {
            let Some(dir) = n.strip_suffix('/') else { continue };
            let (word, pre) = split_arch_dir(dir);
            let Some(arch) = word.and_then(Arch::from_word) else { continue };
            let code = format!("{folder}{pre}{}", arch.suffix());
            if parse_code(&code).is_none() {
                continue;
            }
            let file = match arch {
                Arch::Amd64 => format!("python-{folder}{pre}-amd64.exe"),
                Arch::Arm64 => format!("python-{folder}{pre}-arm64.exe"),
                Arch::Win32 => format!("python-{folder}{pre}.exe"),
            };
            rows.push(Row {
                code,
                url: format!("{base}/{folder}/{file}"),
                file,
                x64: arch.x64(),
                web_install: false,
                msi: false,
                zip_root_dir: None,
            });
        }
    }
    rows
}

/// `amd64rc2` → (`amd64`, `rc2`); `win32` → (`win32`, ``); anything else → (None, ``).
fn split_arch_dir(dir: &str) -> (Option<&str>, &str) {
    for word in ["amd64", "win32", "arm64"] {
        if let Some(pre) = dir.strip_prefix(word) {
            let tag_end = pre.find(|c: char| c.is_ascii_digit()).unwrap_or(pre.len());
            let (w, n) = pre.split_at(tag_end);
            let pre_ok = pre.is_empty()
                || (matches!(w, "a" | "b" | "rc") && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            return if pre_ok { (Some(word), pre) } else { (None, "") };
        }
    }
    (None, "")
}

/// One `PythonCore` zip of the Install Manager index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexZip {
    /// `sort-version`: `3.12.1`, `3.15.0rc3`.
    pub version: String,
    pub ft: bool,
    pub arch: Arch,
    pub url: String,
    pub file: String,
    pub sha256: String,
}

/// The `PythonCore` zips on one index page (rows whose URL is under `base`, named
/// `python-<sort-version>[t]-<arch>.zip`, with a 64-hex SHA-256), and the next page's URL,
/// resolved against this page's folder.
pub fn index_page(json: &str, page_url: &str, base: &str) -> Result<(Vec<IndexZip>, Option<String>), String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let entries = v["versions"].as_array().ok_or("no \"versions\" array")?;
    let mut zips = Vec::new();
    for e in entries {
        if e["company"].as_str() != Some("PythonCore") {
            continue;
        }
        let (Some(url), Some(sort), Some(sha)) =
            (e["url"].as_str(), e["sort-version"].as_str(), e["hash"]["sha256"].as_str())
        else {
            continue;
        };
        if sha.len() != 64 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            continue;
        }
        let Some(rest) = url.strip_prefix(base).and_then(|r| r.strip_prefix('/')) else { continue };
        let file = rest.rsplit('/').next().unwrap_or("");
        let Some(stem) = file.strip_prefix("python-").and_then(|s| s.strip_suffix(".zip")) else {
            continue;
        };
        let Some((ver, word)) = stem.rsplit_once('-') else { continue };
        let Some(arch) = Arch::from_word(word) else { continue };
        let (ver, ft) = match ver.strip_suffix('t') {
            Some(v) if v.ends_with(|c: char| c.is_ascii_digit()) => (v, true),
            _ => (ver, false),
        };
        if ver != sort || parse_code(ver).is_none() {
            continue;
        }
        zips.push(IndexZip {
            version: ver.to_string(),
            ft,
            arch,
            url: url.to_string(),
            file: file.to_string(),
            sha256: sha.to_string(),
        });
    }
    let next = v["next"].as_str().filter(|n| !n.is_empty()).map(|n| {
        if n.contains("://") {
            n.to_string()
        } else {
            let dir = page_url.rsplit_once('/').map_or(page_url, |(d, _)| d);
            format!("{dir}/{n}")
        }
    });
    Ok((zips, next))
}

/// Every zip on the index chain from `<base>/index-windows.json` (at most 10 pages), and
/// how many pages were read. An error names the URL that failed.
pub fn index_zips(f: &Fetcher, base: &str) -> Result<(Vec<IndexZip>, usize), (String, TextError)> {
    let mut url = format!("{base}/index-windows.json");
    let mut all = Vec::new();
    let mut seen = Vec::new();
    for _ in 0..10 {
        if seen.contains(&url) {
            break;
        }
        seen.push(url.clone());
        let text = f.get_text(&url).map_err(|e| (url.clone(), e))?;
        let (zips, next) = index_page(&text, &url, base)
            .map_err(|m| (url.clone(), TextError { status: None, message: m }))?;
        all.extend(zips);
        match next {
            Some(n) => url = n,
            None => break,
        }
    }
    Ok((all, seen.len()))
}

/// The folder rows plus a row for each zip code no folder offered (free-threaded builds,
/// mostly), de-duplicated by code and sorted (Decision 2).
pub fn catalog(mut rows: Vec<Row>, zips: &[IndexZip]) -> Vec<Row> {
    for z in zips {
        let code = format!("{}{}{}", z.version, if z.ft { "t" } else { "" }, z.arch.suffix());
        if rows.iter().any(|r| r.code == code) {
            continue;
        }
        rows.push(Row {
            code,
            file: z.file.clone(),
            url: z.url.clone(),
            x64: z.arch.x64(),
            web_install: false,
            msi: false,
            zip_root_dir: None,
        });
    }
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| seen.insert(r.code.clone()));
    sort_rows(&mut rows);
    rows
}
```

- [ ] **Step 5: Run the unit tests and see them pass.**

Run: `cargo test -p pyenv --lib install::winsource`. Expected: 7 passed.

- [ ] **Step 6: Write the test helpers and the failing CLI test.** Create `crates/pyenv/tests/common/winfake.rs` and add `pub mod winfake;` to `tests/common/mod.rs`:

```rust
//! python.org as the tier-1 fake server presents it (plan M2b): nginx listings, Install
//! Manager index pages and small zips.

use std::io::Write;

/// An nginx autoindex page linking `names`.
pub fn listing(names: &[&str]) -> Vec<u8> {
    let mut s = String::from("<html><body><pre><a href=\"../\">../</a>\n");
    for n in names {
        s.push_str(&format!("<a href=\"{n}\">{n}</a>\n"));
    }
    s.push_str("</pre></body></html>\n");
    s.into_bytes()
}

/// An index page. `zips` are `(url, sha256)` for `python-<ver>[t]-<arch>.zip` URLs.
pub fn index_json(zips: &[(&str, &str)], next: Option<&str>) -> Vec<u8> {
    let rows: Vec<String> = zips
        .iter()
        .map(|(url, sha)| {
            let file = url.rsplit('/').next().unwrap();
            let ver = file.trim_start_matches("python-").rsplit_once('-').unwrap().0.trim_end_matches('t');
            format!("{{\"schema\":1,\"id\":\"pythoncore\",\"sort-version\":\"{ver}\",\"company\":\"PythonCore\",\"url\":\"{url}\",\"hash\":{{\"sha256\":\"{sha}\"}}}}")
        })
        .collect();
    let next = next.map(|n| format!(",\"next\":\"{n}\"")).unwrap_or_default();
    format!("{{\"versions\":[{}]{next}}}", rows.join(",")).into_bytes()
}

/// A deflated zip of `(name, body)` entries.
pub fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(body).unwrap();
    }
    w.finish().unwrap().into_inner()
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
```

First add to `tests/common/server.rs` a constructor whose routes can name the server's own URL. python.org's index links back to the server, so its body can only be built once the port is known:

```rust
/// Like `start`, but the routes are built from the server's own base URL (for bodies that
/// link back to the server, such as python.org's index).
pub fn start_with(make: impl FnOnce(&str) -> Vec<(String, Vec<Reply>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let routes = make(&format!("http://127.0.0.1:{port}"));
    serve(listener, routes.into_iter().collect())
}
```

Refactor `start` into a thin wrapper over a private `serve(listener: TcpListener, routes: HashMap<String, Vec<Reply>>) -> Server`, which holds the existing accept loop unchanged, so both constructors share it.

Create `crates/pyenv/tests/cli_update_win.rs`:

```rust
//! `pyenv update` (pyenv-win flavor) against the tier-1 fake python.org (plan M2b Task 5).
#![cfg(windows)]

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, listing};
use common::Fixture;

const BANNER: &str = ":: [Info] ::  Mirror: https://www.python.org/ftp/python

:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json

:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases

";
const SHA: &str = "11a906a2f36cacaee938c048968d99aa68ec0db592693b5a0fe3b161bb280ec5";

fn run(f: &Fixture, base: &str, args: &[&str]) -> common::Run {
    f.pyenv_env(args, &[("RPYENV_TEST_PYTHON_ORG", base)])
}

fn fake(root: Reply, page_2_7: Reply) -> Server {
    start_with(|host| {
        let base = format!("{host}/ftp/python");
        vec![
            ("/ftp/python/".into(), vec![root.clone()]),
            ("/ftp/python/2.7.18/".into(), vec![page_2_7.clone()]),
            ("/ftp/python/3.10.0/".into(), vec![Reply::Body(listing(&["amd64/", "amd64rc2/", "win32/"]))]),
            ("/ftp/python/3.13.0/".into(), vec![Reply::Body(listing(&["amd64/"]))]),
            ("/ftp/python/index-windows.json".into(), vec![Reply::Body(index_json(
                &[(&format!("{base}/3.13.0/python-3.13.0-amd64.zip"), SHA)],
                Some("index-windows-recent.json"),
            ))]),
            ("/ftp/python/index-windows-recent.json".into(), vec![Reply::Body(index_json(
                &[(&format!("{base}/3.13.0/python-3.13.0t-amd64.zip"), SHA)],
                None,
            ))]),
        ]
    })
}

fn ok_listing() -> Reply {
    Reply::Body(listing(&["2.3.7/", "2.7.18/", "3.10.0/", "3.13.0/"]))
}

fn ok_27() -> Reply {
    Reply::Body(listing(&["python-2.7.18.msi", "python-2.7.18.amd64.msi"]))
}

fn codes(f: &Fixture) -> Vec<String> {
    pyenv::install::wincatalog::read_db(&f.root).unwrap().into_iter().map(|r| r.code).collect()
}

#[test]
fn update_writes_the_cache_from_listings_and_the_index() {
    let f = Fixture::new();
    let s = fake(ok_listing(), ok_27());
    let r = run(&f, &s.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(
        r.stdout,
        format!("{BANNER}:: [Info] ::  Scanned 5 pages and found 7 installers.

")
    );
    assert_eq!(codes(&f), ["2.7.18-win32", "2.7.18", "3.10.0rc2", "3.10.0-win32", "3.10.0", "3.13.0", "3.13.0t"]);
    let raw = std::fs::read(f.root.join(".versions_cache.xml")).unwrap();
    assert!(raw.ends_with(b"</versions>") && raw.windows(2).any(|w| w == b"

"));
}

#[test]
fn a_failed_root_listing_writes_nothing_and_exits_1_or_0_with_ignore() {
    for (args, code) in [(&["update"][..], 1), (&["update", "--ignore"][..], 0)] {
        let f = Fixture::new();
        let s = fake(Reply::Status(503), ok_27());
        let base = s.url("/ftp/python");
        let r = run(&f, &base, args);
        assert_eq!(r.code, code, "{args:?}: {}", r.stdout);
        assert!(r.stdout.starts_with(BANNER));
        assert!(r.stdout.contains(&format!("HTTP Error downloading from mirror \"{base}/\"

Error(503): HTTP 503

")), "{}", r.stdout);
        assert!(!f.root.join(".versions_cache.xml").exists());
    }
}

#[test]
fn a_failed_version_page_stops_the_update_unless_ignored() {
    let f = Fixture::new();
    let s = fake(ok_listing(), Reply::Status(404));
    let base = s.url("/ftp/python");
    let r = run(&f, &base, &["update"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains(&format!("HTTP Error downloading from mirror page \"{base}/2.7.18/\"")));
    assert!(!f.root.join(".versions_cache.xml").exists());
    let r = run(&f, &base, &["update", "--ignore"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(!codes(&f).contains(&"2.7.18".to_string()));
    assert!(codes(&f).contains(&"3.10.0".to_string()));
}

#[test]
fn update_help_prints_the_banner_and_usage() {
    let f = Fixture::new();
    let r = f.pyenv(&["update", "--help"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, format!("{BANNER}Usage: pyenv update [--ignore]



  --ignore  Ignores any HTTP/VBScript errors that occur during downloads.



Updates the internal database of python installer URL's.



"));
}
```

Expected counts in the first test:
- **pages:** 3 folder listings (2.7.18, 3.10.0 and 3.13.0; 2.3.7 is below 2.4) plus 2 index pages, so 5;
- **codes:** 2 from the MSIs, 3 from 3.10.0's arch folders, 1 from 3.13.0's, plus the zip-only `3.13.0t`, so 7. The non-free-threaded 3.13.0 zip adds nothing, because the folder already gave that code.

The 503 case retries twice (`get_text`'s transient rule), so it takes about 3 s.

- [ ] **Step 7: Implement `commands/update.rs`.** Register `("update", update::update)` in `WIN_ONLY` in `commands/mod.rs`, and add `pub mod update;`. pyenv-win's table order doesn't matter, because `names()` sorts.

```rust
//! `pyenv update [--ignore]` (pyenv-win flavor; docs/parity/pyenv-win-m2-reference.md
//! "update"): rewrites `<root>\.versions_cache.xml` from python.org directly (spec §9.1,
//! plan M2b Decision 3).

use crate::install::fetch::{Fetcher, TextError};
use crate::install::wincatalog::write_db;
use crate::install::winsource::{
    banner, base, catalog, folder_rows, index_zips, listing_names, mirror, version_folders,
};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;

pub const HELP: &str = "Usage: pyenv update [--ignore]\n\n  --ignore  Ignores any HTTP/VBScript errors that occur during downloads.\n\nUpdates the internal database of python installer URL's.\n\n";

fn report(o: &mut Output, what: &str, url: &str, e: &TextError) {
    o.out(format!("HTTP Error downloading from {what} \"{url}\""));
    match e.status {
        Some(s) => o.out(format!("Error({s}): {}", e.message)),
        None => o.out(format!("Error: {}", e.message)),
    }
}

/// Folder listings, 8 at a time, in `folders` order.
fn fetch_all(f: &Fetcher, src: &str, folders: &[String]) -> Vec<Result<String, TextError>> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Result<String, TextError>>>> =
        folders.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(folder) = folders.get(i) else { break };
                let r = f.get_text(&format!("{src}/{folder}/"));
                *results[i].lock().unwrap() = Some(r);
            });
        }
    });
    results
        .into_iter()
        .map(|m| m.into_inner().unwrap().expect("every folder was fetched"))
        .collect()
}

pub fn update(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    o.stdout.push_str(&banner());
    let ignore = match args.first() {
        Some(&"--help") => {
            o.stdout.push_str(HELP);
            return o;
        }
        Some(&"--ignore") => true,
        _ => false,
    };
    let src = mirror().unwrap_or_else(base);
    let src = src.trim_end_matches('/').to_string();
    let f = Fetcher::direct();
    let root_url = format!("{src}/");
    let names = match f.get_text(&root_url) {
        Ok(t) => listing_names(&t),
        Err(e) => {
            report(&mut o, "mirror", &root_url, &e);
            return o.with_code(if ignore { 0 } else { 1 });
        }
    };
    let folders = version_folders(&names);
    let mut rows = Vec::new();
    let mut pages = 0;
    for (folder, r) in folders.iter().zip(fetch_all(&f, &src, &folders)) {
        match r {
            Ok(t) => {
                pages += 1;
                rows.extend(folder_rows(&src, folder, &listing_names(&t)));
            }
            Err(e) => {
                report(&mut o, "mirror page", &format!("{src}/{folder}/"), &e);
                if !ignore {
                    return o.with_code(1);
                }
            }
        }
    }
    match index_zips(&f, &src) {
        Ok((zips, n)) => {
            pages += n;
            rows = catalog(rows, &zips);
        }
        Err((url, e)) => {
            report(&mut o, "mirror", &url, &e);
            return o.with_code(if ignore { 0 } else { 1 });
        }
    }
    if let Err(e) = write_db(&ctx.root, &rows) {
        o.out(format!("pyenv: cannot write {}: {e}", ctx.root.join(crate::install::wincatalog::DB_NAME).display()));
        return o.with_code(1);
    }
    o.out(format!(":: [Info] ::  Scanned {pages} pages and found {} installers.", rows.len()));
    o
}
```

Help routing: `pyenv update --help` reaches `help::help_command` through `lib.rs`'s `--help` rule, so add the dynamic topic now. In `help.rs`'s `help_win`, before the `find` call:

```rust
        Some("update") => {
            o.stdout.push_str(&crate::install::winsource::banner());
            o.stdout.push_str(crate::commands::update::HELP);
        }
```

(Task 8 adds `install` the same way.) `update` must also appear in the help listing. `WIN_HELP_LISTING` is pyenv-win's text and already lists it, so it is unchanged.

- [ ] **Step 8: Run the CLI tests and see them pass.**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_update_win`. Expected: 4 passed.

- [ ] **Step 9: Mutation check.** In `update`, write the cache even after a failed version page without `--ignore`, by moving `write_db` before the loop's early return. `a_failed_version_page_stops_the_update_unless_ignored` must fail. Restore.

- [ ] **Step 10: Measure the real thing once.** Use a scratch root. **Never** the real one at `C:\Users\JM\.pyenv`.

```bash
mkdir -p /c/tmp/m2b_t5_root
PYENV_ROOT=C:/tmp/m2b_t5_root PYENV=C:/tmp/m2b_t5_root PYENV_HOME=C:/tmp/m2b_t5_root ./target/debug/pyenv.exe update
```

Record in your report:
- the output;
- the wall-clock time (`time`);
- the code count;
- the comparison with pyenv-win's own cache: codes only in each, compared with `tests/fixtures/pyenv-win/versions_cache.xml`, ignoring PyPy and GraalPy rows. Expected from the data spike: the only CPython code pyenv-win has that rpyenv lacks is `3.5.0a5` (both arches). Any other difference must be explained.

Then delete `C:\tmp\m2b_t5_root` by literal path.

- [ ] **Step 11: Check, both OSes, and commit.** Run fmt, clippy and `cargo test --workspace` on both OSes (the CLI test is Windows-only; the unit tests run in WSL too), and `python ci/shim_deps.py`.

```bash
git add crates/pyenv/Cargo.toml Cargo.lock crates/pyenv/src/install/mod.rs crates/pyenv/src/install/winsource.rs crates/pyenv/src/commands/mod.rs crates/pyenv/src/commands/update.rs crates/pyenv/src/help.rs crates/pyenv/tests/common crates/pyenv/tests/cli_update_win.rs
git commit -m "Add pyenv update for Windows, reading python.org's listings and Install Manager index"
```

---

### Task 6: The transaction per flavor

**Files:**
- Modify: `crates/pyenv/src/install/txn.rs`, `crates/rpyenv-core/src/installed.rs`
- Test: `crates/pyenv/tests/install_txn.rs` (both OSes), unit tests in `installed.rs`

**Interfaces:**
- Produces:
  - `pub fn is_complete_for(dir: &Path, flavor: Flavor) -> bool`
  - `pub fn Txn::begin_for(versions: &Path, name: &str, flavor: Flavor) -> Result<Txn, String>`
  - `pub fn kept_message(flavor: Flavor, old: &Path, e: &std::io::Error) -> String`
- Unchanged: `is_complete(dir)` and `Txn::begin(versions, name)`, which keep their Linux meaning.
- Changed: `installed::top_level` hides `.tmp-`/`.old-` names for both flavors.

- [ ] **Step 1: Write the failing tests.** Append to `crates/pyenv/tests/install_txn.rs`:

```rust
use pyenv::install::txn::{is_complete_for, kept_message};
use rpyenv_core::flavor::Flavor;

#[test]
fn a_pyenv_win_version_is_complete_when_its_folder_exists_without_the_marker() {
    let d = tempfile::tempdir().unwrap();
    let v = d.path().join("3.12.1");
    assert!(!is_complete_for(&v, Flavor::PyenvWin));
    std::fs::create_dir_all(&v).unwrap();
    assert!(is_complete_for(&v, Flavor::PyenvWin), "no bin\\ needed on Windows");
    assert!(!is_complete_for(&v, Flavor::Pyenv), "Linux still needs bin/");
    std::fs::write(v.join(".rpyenv-incomplete"), "").unwrap();
    assert!(!is_complete_for(&v, Flavor::PyenvWin));
}

#[test]
fn a_pyenv_win_reinstall_carries_scripts_and_site_packages_and_recovers() {
    let d = tempfile::tempdir().unwrap();
    let versions = d.path().join("versions");
    let old = versions.join("3.12.1");
    std::fs::create_dir_all(old.join("Scripts")).unwrap();
    std::fs::write(old.join("Scripts").join("black.exe"), "b").unwrap();
    std::fs::create_dir_all(old.join("Lib").join("site-packages").join("userpkg")).unwrap();
    std::fs::write(old.join("python.exe"), "old").unwrap();
    let mut t = pyenv::install::txn::Txn::begin_for(&versions, "3.12.1", Flavor::PyenvWin).unwrap();
    let stage = t.stage_dir().to_path_buf();
    std::fs::write(stage.join("python.exe"), "new").unwrap();
    t.place(&stage).unwrap();
    t.commit().unwrap();
    assert_eq!(std::fs::read_to_string(old.join("python.exe")).unwrap(), "new");
    assert!(old.join("Scripts").join("black.exe").is_file());
    assert!(old.join("Lib").join("site-packages").join("userpkg").is_dir());
    assert!(!versions.join(".old-3.12.1").exists() && !versions.join(".tmp-3.12.1").exists());
}

#[test]
fn begin_for_pyenv_win_finishes_a_carry_over_left_by_a_killed_install() {
    let d = tempfile::tempdir().unwrap();
    let versions = d.path().join("versions");
    std::fs::create_dir_all(versions.join("3.12.1")).unwrap();
    std::fs::create_dir_all(versions.join(".old-3.12.1").join("Scripts")).unwrap();
    std::fs::write(versions.join(".old-3.12.1").join("Scripts").join("x.exe"), "").unwrap();
    let _t = pyenv::install::txn::Txn::begin_for(&versions, "3.12.1", Flavor::PyenvWin).unwrap();
    assert!(versions.join("3.12.1").join("Scripts").join("x.exe").is_file());
    assert!(!versions.join(".old-3.12.1").exists());
}

#[test]
fn the_kept_message_ends_in_the_flavors_line_ending() {
    let e = std::io::Error::other("busy");
    let p = std::path::Path::new("v/.old-x");
    assert_eq!(kept_message(Flavor::Pyenv, p, &e), format!("pyenv: kept the previous installation at {}: busy\n", p.display()));
    assert!(kept_message(Flavor::PyenvWin, p, &e).ends_with(": busy\r\n"));
}
```

In `crates/rpyenv-core/src/installed.rs`'s `mod tests`, add (follow the module's existing tempdir helper; if there is none, use `std::env::temp_dir().join(<unique name>)` and remove it at the end by that literal path):

```rust
    #[test]
    fn staging_names_are_hidden_for_pyenv_win_too() {
        let d = tempfile_dir("staging_win");
        for n in ["3.12.1", ".tmp-3.12.2", ".old-3.12.1", ".hidden"] {
            fs::create_dir_all(d.join(n)).unwrap();
        }
        let names = names(&d, Flavor::PyenvWin);
        assert!(names.contains(&"3.12.1".to_string()) && names.contains(&".hidden".to_string()));
        assert!(!names.iter().any(|n| n.starts_with(".tmp-") || n.starts_with(".old-")));
        fs::remove_dir_all(&d).unwrap();
    }
```

(`tempfile_dir` is whatever helper the module already uses; if `rpyenv-core` has no `tempfile` dev-dependency, don't add one — use `std::env::temp_dir().join(format!("rpyenv-installed-{}-staging_win", std::process::id()))`.)

- [ ] **Step 2: Run them and see them fail.**

Run: `cargo test -p pyenv --test install_txn`, then `cargo test -p rpyenv-core installed`.
Expected: compile errors for `is_complete_for`, `begin_for` and `kept_message`. The `installed` test fails, because `.tmp-3.12.2` is listed.

- [ ] **Step 3: Implement.**

In `txn.rs`:
- add `use rpyenv_core::flavor::Flavor;`;
- replace `is_complete` with the pair below;
- add a `flavor: Flavor` field to `Txn`;
- rename `begin` to `begin_for` with the extra parameter, and add a `begin` that forwards;
- use `is_complete_for(&target, flavor)` in the recovery branch;
- use `kept_message` in `commit`;
- add `"Scripts"` to `carry_over`.

```rust
/// "Installed" for `pyenv install`, minus interrupted installs (plan M2b Decision 9): upstream
/// pyenv's `bin/` test, or pyenv-win's "the folder exists".
pub fn is_complete_for(dir: &Path, flavor: Flavor) -> bool {
    let present = match flavor {
        Flavor::Pyenv => dir.join("bin").is_dir(),
        Flavor::PyenvWin => dir.is_dir(),
    };
    present && !dir.join(MARKER).exists()
}

/// The Linux flavor's test (M2a's callers).
pub fn is_complete(dir: &Path) -> bool {
    is_complete_for(dir, Flavor::Pyenv)
}

/// Why a reinstall kept `.old-<name>`, in the flavor's line ending.
pub fn kept_message(flavor: Flavor, old: &Path, e: &std::io::Error) -> String {
    format!(
        "pyenv: kept the previous installation at {}: {e}{}",
        old.display(),
        flavor.eol()
    )
}
```

```rust
    /// The Linux flavor's transaction (M2a's callers).
    pub fn begin(versions: &Path, name: &str) -> Result<Txn, String> {
        Txn::begin_for(versions, name, Flavor::Pyenv)
    }

    pub fn begin_for(versions: &Path, name: &str, flavor: Flavor) -> Result<Txn, String> {
        // … the old `begin` body, with `is_complete(&target)` → `is_complete_for(&target, flavor)`
        //   and `flavor` stored in the returned `Txn` …
    }
```

In `commit`, the `Err(e)` arm becomes:

```rust
                Err(e) => rpyenv_core::textout::write(
                    // pyenv-win prints everything on stdout (reference, "Conventions").
                    self.flavor == Flavor::Pyenv,
                    &kept_message(self.flavor, &old, &e),
                ),
```

In `carry_over`, `let mut dirs: Vec<PathBuf> = vec![PathBuf::from("bin")];` becomes:

```rust
    // `bin/` (pyenv) and `Scripts\` (pyenv-win): console scripts pip installed.
    let mut dirs: Vec<PathBuf> = vec![PathBuf::from("bin"), PathBuf::from("Scripts")];
```

Update the doc comment of `carry_over` to mention `Scripts\`. If `Flavor::eol` doesn't exist under that name, use the existing equivalent in `rpyenv_core::flavor` and record it.

In `installed.rs`'s `top_level`, move `names.retain(|n| !is_staging_name(n));` above the `if flavor == Flavor::Pyenv` block, and update its comment: the staging names are hidden for both flavors, with an allowlist row (Task 10).

- [ ] **Step 4: Run the tests and see them pass.** Run `cargo test -p pyenv --test install_txn` and `cargo test -p rpyenv-core`, then the M2a suites in WSL. `Txn::begin` callers must be unchanged and green.

- [ ] **Step 5: Mutation check.** Make `is_complete_for` ignore the flavor and use the `bin` test. `a_pyenv_win_version_is_complete_when_its_folder_exists_without_the_marker` must fail. Restore.

- [ ] **Step 6: Check, both OSes, and commit.** Run fmt, clippy and `cargo test --workspace` on both OSes. In WSL also run the parity unit tests: `python3 -m unittest discover -s parity -p "test_*.py"`.

```bash
git add crates/pyenv/src/install/txn.rs crates/pyenv/tests/install_txn.rs crates/rpyenv-core/src/installed.rs
git commit -m "Make the install transaction flavor-aware and hide staging names on Windows too"
```

---

### Task 7: The Windows install engine

**Files:**
- Create: `crates/pyenv/src/install/winpkg.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod winpkg;`)
- Test: `crates/pyenv/tests/install_winpkg.rs` (both OSes; the engine runs Python only when Python is in the package, and the fixtures contain none)

**Interfaces:**
- Consumes:
  - `openpgp::verify_python_org` (Task 1);
  - `msi::extract_msi` (Task 2);
  - `zipx::extract_zip`, `fetch::{Check, FetchRequest, Fetcher}` and `child_of` (Task 3);
  - `wincatalog::{Arch, Code, parse_code}` (Task 4);
  - `winsource::{index_zips, listing_names}` (Task 5);
  - `txn::{Txn, is_complete_for}` (Task 6);
  - `checksum::sha256_file`.
- Produces:
  - `pub fn skip_component(&str) -> bool`
  - `pub struct Remote { name, url, asc: Option<String> }`
  - `pub enum Package { Zip { file, url, sha256 }, Components { folder_url, msis: Vec<Remote> }, SingleMsi(Remote) }`
  - `pub fn resolve(&Code, base, &Fetcher) -> Result<Package, String>`
  - `pub enum Kind { Zip, Msi }`
  - `pub struct Fetched { files: Vec<PathBuf>, unsigned: Vec<String>, kind: Kind }`
  - `pub fn fetch_package(&Package, code_text, cache_root, &Fetcher, announce: &mut dyn FnMut(&str, &Path)) -> Result<Fetched, InstallError>`
  - `pub fn unpack(&Fetched, stage) -> Result<(), String>`
  - `pub fn finish(&Code, prefix, Kind) -> Result<(), String>`
  - `pub enum Done { Installed, Skipped }`
  - `pub struct Job<'a> { root, code, force, base, fetcher }`
  - `pub fn install(&Job, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError>`

`say` receives whole output lines without line endings, in pyenv-win's words (reference, "Output of an install"):

```
:: [Downloading] ::  3.10.11 ...
:: [Downloading] ::  From <zip URL, or the component folder URL, or the single MSI's URL>
:: [Downloading] ::  To   <install_cache file or folder>
:: [Warning] :: python.org publishes no signature for <file>[, <file>…]; checked only by HTTPS.
:: [Installing] ::  3.10.11 ...
:: [Info] :: completed! 3.10.11
```

The `[Downloading]` lines appear only when a payload must be downloaded. A cached, re-verified package prints none, as pyenv-win prints none for a cached installer. The `[Warning]` line appears only for unsigned files (Decision 6).

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/install_winpkg.rs`:

```rust
//! The Windows install engine against the tier-1 fake python.org (plan M2b Task 7; review focus
//! 1 and 3). The MSI cases install python.org's real tools.msi, verified with the real
//! signature; the zip cases install a small zip whose SHA-256 the fake index publishes.

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, listing, sha256, zip_bytes};
use pyenv::install::fetch::Fetcher;
use pyenv::install::wincatalog::parse_code;
use pyenv::install::winpkg::{install, resolve, skip_component, Done, Job, Package};
use pyenv::install::InstallError;
use std::path::{Path, PathBuf};

fn fx(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/msi").join(name)).unwrap()
}

fn tools() -> Vec<u8> {
    fx("tools-3.10.11-amd64.msi")
}

fn tools_asc() -> Vec<u8> {
    fx("tools-3.10.11-amd64.msi.asc")
}

/// python.org with 3.10.11 amd64 offering only tools.msi (and the given asc reply), 3.12.1 as
/// an index zip, and 2.7.18 as a single MSI (served as tools.msi's bytes, so it is unsigned
/// unless a signature is listed).
fn python_org(msi: Vec<u8>, asc: Option<Reply>, zip: Vec<u8>, zip_sha: String) -> Server {
    start_with(move |host| {
        let base = format!("{host}/ftp/python");
        let mut names = vec!["tools.msi", "tools_d.msi", "path.msi"];
        if asc.is_some() {
            names.push("tools.msi.asc");
        }
        vec![
            ("/ftp/python/3.10.11/amd64/".into(), vec![Reply::Body(listing(&names))]),
            ("/ftp/python/3.10.11/amd64/tools.msi".into(), vec![Reply::Body(msi.clone())]),
            ("/ftp/python/3.10.11/amd64/tools.msi.asc".into(), vec![asc.clone().unwrap_or(Reply::Status(404))]),
            ("/ftp/python/index-windows.json".into(), vec![Reply::Body(index_json(
                &[(&format!("{base}/3.12.1/python-3.12.1-amd64.zip"), &zip_sha)],
                None,
            ))]),
            ("/ftp/python/3.12.1/python-3.12.1-amd64.zip".into(), vec![Reply::Body(zip.clone())]),
            ("/ftp/python/2.7.18/".into(), vec![Reply::Body(listing(&["python-2.7.18.amd64.msi"]))]),
            ("/ftp/python/2.7.18/python-2.7.18.amd64.msi".into(), vec![Reply::Body(msi.clone())]),
        ]
    })
}

fn small_zip() -> Vec<u8> {
    zip_bytes(&[
        ("python.exe", b"MZ-python"),
        ("pythonw.exe", b"MZ-pythonw"),
        ("Lib/os.py", b"# os"),
        ("Lib/venv/scripts/nt/python.exe", b"MZ-venvlauncher"),
    ])
}

fn fetcher() -> Fetcher {
    let mut f = Fetcher::direct();
    f.retry_delay = std::time::Duration::ZERO;
    f
}

struct Run {
    root: tempfile::TempDir,
    lines: Vec<String>,
    result: Result<Done, InstallError>,
}

fn run(s: &Server, code: &str, force: bool, root: Option<tempfile::TempDir>) -> Run {
    let root = root.unwrap_or_else(|| tempfile::tempdir().unwrap());
    let c = parse_code(code).unwrap();
    let f = fetcher();
    let base = s.url("/ftp/python");
    let job = Job { root: root.path(), code: &c, force, base: &base, fetcher: &f };
    let mut lines = Vec::new();
    let result = install(&job, &mut |l| lines.push(l.to_string()));
    Run { root, lines, result }
}

fn version(r: &Run, code: &str) -> PathBuf {
    r.root.path().join("versions").join(code)
}

fn leftovers(r: &Run) -> Vec<String> {
    std::fs::read_dir(r.root.path().join("versions"))
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

#[test]
fn components_skip_debug_path_launcher_pip_and_free_threaded() {
    for skip in ["core_d.msi", "lib_pdb.msi", "appendpath.msi", "launcher.msi", "path.msi", "pip.msi", "freethreaded.msi", "freethreaded_d.msi"] {
        assert!(skip_component(skip), "{skip}");
    }
    for keep in ["core.msi", "exe.msi", "lib.msi", "dev.msi", "tcltk.msi", "test.msi", "tools.msi", "doc.msi", "ucrt.msi"] {
        assert!(!skip_component(keep), "{keep}");
    }
}

#[test]
fn resolve_picks_the_zip_the_components_or_the_single_msi() {
    let zip = small_zip();
    let s = python_org(tools(), Some(Reply::Body(tools_asc())), zip.clone(), sha256(&zip));
    let base = s.url("/ftp/python");
    let f = fetcher();
    match resolve(&parse_code("3.12.1").unwrap(), &base, &f).unwrap() {
        Package::Zip { file, sha256: h, .. } => assert_eq!((file.as_str(), h), ("python-3.12.1-amd64.zip", sha256(&zip))),
        p => panic!("{p:?}"),
    }
    match resolve(&parse_code("3.10.11").unwrap(), &base, &f).unwrap() {
        Package::Components { msis, .. } => {
            let names: Vec<&str> = msis.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, ["tools.msi"]);
            assert_eq!(msis[0].asc.as_deref(), Some(format!("{base}/3.10.11/amd64/tools.msi.asc").as_str()));
        }
        p => panic!("{p:?}"),
    }
    assert!(matches!(resolve(&parse_code("2.7.18").unwrap(), &base, &f).unwrap(), Package::SingleMsi(r) if r.asc.is_none()));
    let e = resolve(&parse_code("3.10.11-arm").unwrap(), &base, &f).unwrap_err();
    assert_eq!(e, "python.org has no Windows installer for 3.10.11-arm");
    let e = resolve(&parse_code("3.10.11t").unwrap(), &base, &f).unwrap_err();
    assert_eq!(e, "python.org has no free-threaded package for 3.10.11t");
}

#[test]
fn a_signed_msi_installs_exactly_its_files_and_is_cached() {
    let s = python_org(tools(), Some(Reply::Body(tools_asc())), small_zip(), "0".repeat(64));
    let r = run(&s, "3.10.11", false, None);
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?} {:?}", r.result, r.lines);
    let v = version(&r, "3.10.11");
    assert!(v.join("Tools").join("demo").join("beer.py").is_file());
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert_eq!(leftovers(&r), ["3.10.11"]);
    let base = s.url("/ftp/python");
    let cache = r.root.path().join("install_cache").join("3.10.11");
    assert_eq!(
        r.lines,
        [
            ":: [Downloading] ::  3.10.11 ...".to_string(),
            format!(":: [Downloading] ::  From {base}/3.10.11/amd64/"),
            format!(":: [Downloading] ::  To   {}", cache.display()),
            ":: [Installing] ::  3.10.11 ...".to_string(),
            ":: [Info] :: completed! 3.10.11".to_string(),
        ]
    );
    assert!(cache.join("tools.msi").is_file() && cache.join("tools.msi.asc").is_file());
    assert_eq!(s.hits("/ftp/python/3.10.11/amd64/tools_d.msi"), 0, "debug MSIs are never fetched");
    // Again with -f: the cached MSI is verified against a fresh signature and reused.
    let again = run(&s, "3.10.11", true, Some(r.root));
    assert!(matches!(again.result, Ok(Done::Installed)), "{:?}", again.lines);
    assert_eq!(s.hits("/ftp/python/3.10.11/amd64/tools.msi"), 1, "the MSI was downloaded once");
    assert_eq!(s.hits("/ftp/python/3.10.11/amd64/tools.msi.asc"), 2, "the signature is fetched every time");
    assert!(!again.lines.iter().any(|l| l.contains("[Downloading]")), "{:?}", again.lines);
}

#[test]
fn a_tampered_cached_msi_is_downloaded_again() {
    let s = python_org(tools(), Some(Reply::Body(tools_asc())), small_zip(), "0".repeat(64));
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("install_cache").join("3.10.11");
    std::fs::create_dir_all(&cache).unwrap();
    let mut bad = tools();
    bad[0x8000] ^= 1;
    std::fs::write(cache.join("tools.msi"), bad).unwrap();
    let r = run(&s, "3.10.11", false, Some(root));
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?} {:?}", r.result, r.lines);
    assert_eq!(s.hits("/ftp/python/3.10.11/amd64/tools.msi"), 1, "the tampered copy was replaced");
    assert_eq!(std::fs::read(cache.join("tools.msi")).unwrap(), tools());
}

#[test]
fn an_installed_version_is_skipped_silently_without_force() {
    let s = python_org(tools(), Some(Reply::Body(tools_asc())), small_zip(), "0".repeat(64));
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("versions").join("3.10.11")).unwrap();
    let r = run(&s, "3.10.11", false, Some(root));
    assert!(matches!(r.result, Ok(Done::Skipped)));
    assert!(r.lines.is_empty());
    assert_eq!(s.hits("/ftp/python/3.10.11/amd64/"), 0);
}

#[test]
fn a_tampered_msi_is_refused_leaves_nothing_and_is_not_cached() {
    let mut bad = tools();
    bad[0x8000] ^= 1;
    let s = python_org(bad, Some(Reply::Body(tools_asc())), small_zip(), "0".repeat(64));
    let r = run(&s, "3.10.11", false, None);
    match &r.result {
        Err(InstallError::Message(m)) => assert!(m.starts_with("signature check failed for tools.msi: bad signature"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(leftovers(&r).is_empty(), "{:?}", leftovers(&r));
    assert!(!r.root.path().join("install_cache").join("3.10.11").join("tools.msi").exists());
}

#[test]
fn a_listed_signature_that_cannot_be_fetched_never_downgrades_to_unsigned() {
    let s = python_org(tools(), Some(Reply::Status(500)), small_zip(), "0".repeat(64));
    let r = run(&s, "3.10.11", false, None);
    match &r.result {
        Err(InstallError::Message(m)) => assert!(m.starts_with("cannot download tools.msi.asc"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(!r.lines.iter().any(|l| l.contains("[Warning]")));
    assert!(leftovers(&r).is_empty());
}

#[test]
fn an_unsigned_msi_installs_with_one_warning() {
    let s = python_org(tools(), None, small_zip(), "0".repeat(64));
    let r = run(&s, "2.7.18", false, None);
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?} {:?}", r.result, r.lines);
    let warnings: Vec<&String> = r.lines.iter().filter(|l| l.contains("[Warning]")).collect();
    assert_eq!(warnings, [":: [Warning] :: python.org publishes no signature for python-2.7.18.amd64.msi; checked only by HTTPS."]);
}

#[test]
fn a_zip_installs_with_its_copies_and_a_bad_hash_is_refused() {
    let zip = small_zip();
    let s = python_org(tools(), None, zip.clone(), sha256(&zip));
    let r = run(&s, "3.12.1", false, None);
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?} {:?}", r.result, r.lines);
    let v = version(&r, "3.12.1");
    let cases: [(&str, &[u8]); 5] = [
        ("python3.exe", b"MZ-python"),
        ("python312.exe", b"MZ-python"),
        ("python3.12.exe", b"MZ-python"),
        ("pythonw3.exe", b"MZ-pythonw"),
        ("pythonw3.12.exe", b"MZ-pythonw"),
    ];
    for (name, body) in cases {
        assert_eq!(std::fs::read(v.join(name)).unwrap(), body, "{name}");
    }
    let nt = v.join("Lib").join("venv").join("scripts").join("nt");
    for name in ["python3.exe", "python312.exe", "python3.12.exe", "pythonw3.exe", "pythonw312.exe", "pythonw3.12.exe"] {
        assert_eq!(std::fs::read(nt.join(name)).unwrap(), b"MZ-venvlauncher", "{name}");
    }
    assert!(r.root.path().join("install_cache").join("python-3.12.1-amd64.zip").is_file());

    let s = python_org(tools(), None, zip.clone(), "0".repeat(64));
    let r = run(&s, "3.12.1", false, None);
    match &r.result {
        Err(InstallError::Message(m)) => assert!(m.contains("checksum mismatch"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(leftovers(&r).is_empty());
}

/// Free-threaded zips have no `python.exe` (measured on 3.13.0t): it is a copy of
/// `python3.13t.exe`, and the usual copies follow from it.
#[test]
fn a_free_threaded_zip_gets_python_exe_from_python3_13t_exe() {
    let zip = zip_bytes(&[("python3.13t.exe", b"MZ-t"), ("pythonw3.13t.exe", b"MZ-tw"), ("Lib/os.py", b"")]);
    let sha = sha256(&zip);
    let s = start_with(move |host| {
        let base = format!("{host}/ftp/python");
        vec![
            ("/ftp/python/index-windows.json".into(), vec![Reply::Body(index_json(
                &[(&format!("{base}/3.13.0/python-3.13.0t-amd64.zip"), &sha)],
                None,
            ))]),
            ("/ftp/python/3.13.0/python-3.13.0t-amd64.zip".into(), vec![Reply::Body(zip.clone())]),
        ]
    });
    let r = run(&s, "3.13.0t", false, None);
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?} {:?}", r.result, r.lines);
    let v = version(&r, "3.13.0t");
    let cases: [(&str, &[u8]); 4] = [("python.exe", b"MZ-t"), ("python3.13.exe", b"MZ-t"), ("pythonw.exe", b"MZ-tw"), ("pythonw313.exe", b"MZ-tw")];
    for (name, body) in cases {
        assert_eq!(std::fs::read(v.join(name)).unwrap(), body, "{name}");
    }
}

#[test]
fn a_tampered_cached_zip_is_downloaded_again() {
    let zip = small_zip();
    let s = python_org(tools(), None, zip.clone(), sha256(&zip));
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("install_cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("python-3.12.1-amd64.zip"), b"tampered").unwrap();
    let r = run(&s, "3.12.1", false, Some(root));
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?}", r.lines);
    assert_eq!(s.hits("/ftp/python/3.12.1/python-3.12.1-amd64.zip"), 1);
}
```

- [ ] **Step 2: Run them and see them fail.**

Run: `cargo test -p pyenv --test install_winpkg`
Expected: compile error, `unresolved import pyenv::install::winpkg`.

- [ ] **Step 3: Implement.** Create `crates/pyenv/src/install/winpkg.rs`:

```rust
//! pyenv-win flavor installs (spec §9.1, plan M2b Decisions 1 and 6–8): which python.org
//! package a version code means, its verified download into `install_cache`, extraction into
//! the transaction's staging folder, and the steps that need the final location.

use super::checksum::sha256_file;
use super::fetch::{Check, FetchRequest, Fetcher};
use super::txn::{is_complete_for, Txn};
use super::wincatalog::{Arch, Code};
use super::winsource::{index_zips, listing_names};
use super::{child_of, interrupted, msi, openpgp, zipx, InstallError};
use rpyenv_core::flavor::Flavor;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Component MSIs a plain install leaves out (Decision 1): debug builds, PATH and launcher
/// changes, pip (ensurepip installs it), and the free-threaded build (only zips install it).
pub fn skip_component(name: &str) -> bool {
    let stem = name.strip_suffix(".msi").unwrap_or(name);
    stem.ends_with("_d")
        || stem.ends_with("_pdb")
        || matches!(stem, "appendpath" | "launcher" | "path" | "pip")
        || stem.starts_with("freethreaded")
}

/// A python.org file and, when the listing names one, its signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
    pub asc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Package {
    Zip { file: String, url: String, sha256: String },
    /// The MSIs of one `<arch><pre>/` folder.
    Components { folder_url: String, msis: Vec<Remote> },
    SingleMsi(Remote),
}

fn no_installer(code: &Code) -> String {
    format!("python.org has no Windows installer for {}", code.text)
}

/// A folder listing; a 404 means python.org has no such package.
fn listing(f: &Fetcher, url: &str, code: &Code) -> Result<Vec<String>, String> {
    match f.get_text(url) {
        Ok(t) => Ok(listing_names(&t)),
        Err(e) if e.status == Some(404) => Err(no_installer(code)),
        Err(e) => Err(format!("cannot read {url}: {}", e.message)),
    }
}

fn remote(folder_url: &str, name: &str, names: &[String]) -> Remote {
    let asc = format!("{name}.asc");
    Remote {
        name: name.to_string(),
        url: format!("{folder_url}{name}"),
        asc: names.contains(&asc).then(|| format!("{folder_url}{asc}")),
    }
}

/// What `code` installs from (Decision 1). Index pages and listings come from `base`.
pub fn resolve(code: &Code, base: &str, f: &Fetcher) -> Result<Package, String> {
    if code.nums >= [3, 11, 0] {
        let (zips, _) =
            index_zips(f, base).map_err(|(url, e)| format!("cannot read {url}: {}", e.message))?;
        if let Some(z) = zips
            .iter()
            .find(|z| z.version == code.version && z.ft == code.ft && z.arch == code.arch)
        {
            return Ok(Package::Zip {
                file: z.file.clone(),
                url: z.url.clone(),
                sha256: z.sha256.clone(),
            });
        }
    }
    if code.ft {
        return Err(format!("python.org has no free-threaded package for {}", code.text));
    }
    if code.nums >= [3, 5, 0] {
        let folder_url = format!("{base}/{}/{}{}/", code.numeric, code.arch.word(), code.pre_tag());
        let names = listing(f, &folder_url, code)?;
        let msis: Vec<Remote> = names
            .iter()
            .filter(|n| n.ends_with(".msi") && !skip_component(n))
            .map(|n| remote(&folder_url, n, &names))
            .collect();
        if msis.is_empty() {
            return Err(no_installer(code));
        }
        return Ok(Package::Components { folder_url, msis });
    }
    if code.arch == Arch::Arm64 {
        return Err(no_installer(code));
    }
    let folder_url = format!("{base}/{}/", code.numeric);
    let name = format!(
        "python-{}{}.msi",
        code.version,
        if code.arch == Arch::Amd64 { ".amd64" } else { "" }
    );
    let names = listing(f, &folder_url, code)?;
    if !names.contains(&name) {
        return Err(no_installer(code));
    }
    Ok(Package::SingleMsi(remote(&folder_url, &name, &names)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Msi,
}

/// The verified local files of a package, in install order.
#[derive(Debug)]
pub struct Fetched {
    pub files: Vec<PathBuf>,
    /// Files python.org publishes no signature for (Decision 6).
    pub unsigned: Vec<String>,
    pub kind: Kind,
}

/// The reason a failed fetch gives: the first line of its log, or a generic one.
fn fetch_error(file: &str, log: &[u8]) -> InstallError {
    let text = String::from_utf8_lossy(log);
    let detail = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("download failed");
    InstallError::Message(format!("cannot download {file}: {detail}"))
}

fn download(f: &Fetcher, url: &str, file: &str, dir: &Path, check: Check) -> Result<PathBuf, InstallError> {
    let req = FetchRequest { file_name: file.to_string(), url: url.to_string(), check, dest_dir: dir.to_path_buf() };
    let mut log = Vec::new();
    match f.fetch(&req, &mut log, &mut |_| {}) {
        Ok(p) => Ok(p),
        Err(InstallError::Failed) => Err(fetch_error(file, &log)),
        Err(e) => Err(e),
    }
}

/// Puts every file of `pkg` in `cache_root` verified (Decisions 6 and 8): a cached file is
/// used only if it verifies against the reference fetched now; otherwise it is downloaded. The
/// `.asc` is always fetched fresh, so a cached MSI can't be swapped for another signed one.
/// `announce(from, to)` is called once, before the first payload download.
pub fn fetch_package(
    pkg: &Package,
    code_text: &str,
    cache_root: &Path,
    f: &Fetcher,
    announce: &mut dyn FnMut(&str, &Path),
) -> Result<Fetched, InstallError> {
    std::fs::create_dir_all(cache_root)
        .map_err(|e| InstallError::Message(format!("{}: {e}", cache_root.display())))?;
    match pkg {
        Package::Zip { file, url, sha256 } => {
            let path = child_of(cache_root, file)
                .ok_or_else(|| InstallError::Message(format!("invalid file name: {file}")))?;
            let cached = sha256_file(&path).ok().as_deref() == Some(sha256.as_str());
            if !cached {
                let _ = std::fs::remove_file(&path);
                announce(url, &path);
                download(f, url, file, cache_root, Check::Sha256(sha256.clone()))?;
            }
            Ok(Fetched { files: vec![path], unsigned: Vec::new(), kind: Kind::Zip })
        }
        Package::Components { folder_url, msis } => {
            let dir = child_of(cache_root, code_text)
                .ok_or_else(|| InstallError::Message(format!("invalid version name: {code_text}")))?;
            fetch_msis(msis, folder_url, &dir, f, announce)
        }
        Package::SingleMsi(r) => {
            let dir = child_of(cache_root, code_text)
                .ok_or_else(|| InstallError::Message(format!("invalid version name: {code_text}")))?;
            fetch_msis(std::slice::from_ref(r), &r.url, &dir, f, announce)
        }
    }
}

fn fetch_msis(
    msis: &[Remote],
    from: &str,
    dir: &Path,
    f: &Fetcher,
    announce: &mut dyn FnMut(&str, &Path),
) -> Result<Fetched, InstallError> {
    std::fs::create_dir_all(dir).map_err(|e| InstallError::Message(format!("{}: {e}", dir.display())))?;
    let mut announced = false;
    let mut files = Vec::new();
    let mut unsigned = Vec::new();
    for r in msis {
        let path = child_of(dir, &r.name)
            .ok_or_else(|| InstallError::Message(format!("invalid file name: {}", r.name)))?;
        let mut fetch_msi = |announced: &mut bool| -> Result<(), InstallError> {
            if !*announced {
                announce(from, dir);
                *announced = true;
            }
            let _ = std::fs::remove_file(&path);
            download(f, &r.url, &r.name, dir, Check::Caller).map(|_| ())
        };
        match &r.asc {
            Some(asc_url) => {
                let asc_name = format!("{}.asc", r.name);
                let _ = std::fs::remove_file(dir.join(&asc_name));
                let asc_path = download(f, asc_url, &asc_name, dir, Check::Caller)?;
                let asc = std::fs::read(&asc_path)
                    .map_err(|e| InstallError::Message(format!("{}: {e}", asc_path.display())))?;
                if !(path.is_file() && openpgp::verify_python_org(&path, &asc).is_ok()) {
                    fetch_msi(&mut announced)?;
                    if let Err(e) = openpgp::verify_python_org(&path, &asc) {
                        let _ = std::fs::remove_file(&path);
                        return Err(InstallError::Message(format!(
                            "signature check failed for {}: {e}",
                            r.name
                        )));
                    }
                }
            }
            None => {
                // Never trusted from the cache: HTTPS is its only check (Decision 6).
                fetch_msi(&mut announced)?;
                unsigned.push(r.name.clone());
            }
        }
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        files.push(path);
    }
    Ok(Fetched { files, unsigned, kind: Kind::Msi })
}

/// Extracts the package into `stage`.
pub fn unpack(fetched: &Fetched, stage: &Path) -> Result<(), String> {
    for file in &fetched.files {
        if interrupted() {
            return Err("interrupted".into());
        }
        match fetched.kind {
            Kind::Zip => zipx::extract_zip(file, stage).map(|_| ())?,
            Kind::Msi => msi::extract_msi(file, stage).map(|_| ())?,
        }
    }
    Ok(())
}

fn copy_if(src: &Path, dests: &[PathBuf]) -> Result<(), String> {
    if !src.is_file() {
        return Ok(());
    }
    for d in dests {
        std::fs::copy(src, d).map_err(|e| format!("cannot copy {} to {}: {e}", src.display(), d.display()))?;
    }
    Ok(())
}

/// Runs the version's own Python in `prefix` with the user site and PYTHON* variables shut out
/// (measured: plain ensurepip read the user site despite `-s`).
fn python(prefix: &Path, args: &[&str], what: &str) -> Result<(), String> {
    let out = Command::new(prefix.join("python.exe"))
        .args(["-E", "-s"])
        .args(args)
        .current_dir(prefix)
        .env("PYTHONNOUSERSITE", "1")
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    Err(format!("{what} failed ({}): {last}", out.status))
}

/// The steps that need the final location (Decision 7): executable copies, then pip.
pub fn finish(code: &Code, prefix: &Path, kind: Kind) -> Result<(), String> {
    let (x, y) = (code.nums[0], code.nums[1]);
    if code.ft {
        copy_if(&prefix.join(format!("python{x}.{y}t.exe")), &[prefix.join("python.exe")])?;
        copy_if(&prefix.join(format!("pythonw{x}.{y}t.exe")), &[prefix.join("pythonw.exe")])?;
    }
    let names = |stem: &str| [format!("{stem}{x}.exe"), format!("{stem}{x}{y}.exe"), format!("{stem}{x}.{y}.exe")];
    for stem in ["python", "pythonw"] {
        let dests: Vec<PathBuf> = names(stem).iter().map(|n| prefix.join(n)).collect();
        copy_if(&prefix.join(format!("{stem}.exe")), &dests)?;
    }
    let nt = prefix.join("Lib").join("venv").join("scripts").join("nt");
    let mut venv: Vec<PathBuf> = names("python").iter().map(|n| nt.join(n)).collect();
    venv.extend(names("pythonw").iter().map(|n| nt.join(n)));
    copy_if(&nt.join("python.exe"), &venv)?;
    match kind {
        Kind::Msi => {
            if prefix.join("Lib").join("ensurepip").is_dir() {
                python(prefix, &["-m", "ensurepip", "-U", "--default-pip"], "ensurepip")?;
            }
        }
        Kind::Zip => {
            // pip is installed but `Scripts\` isn't (measured on 3.11.0): reinstall the bundled
            // wheel offline so pip's launchers exist, as pyenv-win's installs have them.
            let bundled = prefix.join("Lib").join("ensurepip").join("_bundled");
            let wheel = std::fs::read_dir(&bundled).ok().and_then(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .find(|p| p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("pip-") && n.ends_with(".whl")
                    }))
            });
            if let Some(w) = wheel.filter(|_| !prefix.join("Scripts").join("pip.exe").exists()) {
                let w = w.to_string_lossy().into_owned();
                python(
                    prefix,
                    &["-m", "pip", "install", "--no-index", "--no-deps", "--force-reinstall",
                      "--no-warn-script-location", "--disable-pip-version-check", &w],
                    "pip",
                )?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Installed,
    /// Already installed and no `-f`: pyenv-win's silent skip.
    Skipped,
}

pub struct Job<'a> {
    pub root: &'a Path,
    pub code: &'a Code,
    pub force: bool,
    pub base: &'a str,
    pub fetcher: &'a Fetcher,
}

/// One version, end to end. Errors leave no `versions\<code>` behind (the transaction rolls
/// back) and no unverified file in the cache.
pub fn install(job: &Job, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    let text = job.code.text.as_str();
    let versions = job.root.join("versions");
    let msg = InstallError::Message;
    if !super::is_plain_name(text) {
        return Err(msg(format!("invalid version name: {text}")));
    }
    if is_complete_for(&versions.join(text), Flavor::PyenvWin) && !job.force {
        return Ok(Done::Skipped);
    }
    let pkg = resolve(job.code, job.base, job.fetcher).map_err(msg)?;
    let cache = job.root.join("install_cache");
    let fetched = fetch_package(&pkg, text, &cache, job.fetcher, &mut |from, to| {
        say(&format!(":: [Downloading] ::  {text} ..."));
        say(&format!(":: [Downloading] ::  From {from}"));
        say(&format!(":: [Downloading] ::  To   {}", to.display()));
    })?;
    if !fetched.unsigned.is_empty() {
        say(&format!(
            ":: [Warning] :: python.org publishes no signature for {}; checked only by HTTPS.",
            fetched.unsigned.join(", ")
        ));
    }
    say(&format!(":: [Installing] ::  {text} ..."));
    let mut txn = Txn::begin_for(&versions, text, Flavor::PyenvWin).map_err(msg)?;
    let stage = txn.stage_dir().to_path_buf();
    unpack(&fetched, &stage).map_err(msg)?;
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    txn.place(&stage)
        .map_err(|e| msg(format!("cannot move {} into place: {e}", stage.display())))?;
    finish(job.code, &txn.target(), fetched.kind).map_err(msg)?;
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    txn.commit()
        .map_err(|e| msg(format!("cannot finish installing {text}: {e}")))?;
    say(&format!(":: [Info] :: completed! {text}"));
    Ok(Done::Installed)
}
```

Every `?` after `Txn::begin_for` drops `txn`, which rolls the version back (M2a's `Drop`).

- [ ] **Step 4: Run the tests and see them pass.**

Run: `cargo test -p pyenv --test install_winpkg` on Windows and in WSL.
Expected: 11 passed on both. Nothing in these fixtures runs Python: `tools.msi` has no `Lib\ensurepip`, and the zip has no bundled wheel.

- [ ] **Step 5: Mutation checks.** For each mutation, change the code, run the named test and see it fail, then restore.
  1. In `fetch_msis`, accept a failed verification by turning the `if let Err(e) = verify…` block into `let _ = …`. `a_tampered_msi_is_refused_leaves_nothing_and_is_not_cached` must fail.
  2. Make a failed `.asc` download fall through to the unsigned path: replace `download(…asc…)?` with a `match` that treats `Err` as `None`. `a_listed_signature_that_cannot_be_fetched_never_downgrades_to_unsigned` must fail.
  3. Trust a cached MSI without verifying it: use `if !path.is_file()` instead of the full condition. `a_tampered_cached_msi_is_downloaded_again` must fail.

- [ ] **Step 6: Check, both OSes, and commit.** Run fmt, clippy, `cargo test --workspace` (both OSes) and `python ci/shim_deps.py`.

```bash
git add crates/pyenv/src/install/mod.rs crates/pyenv/src/install/winpkg.rs crates/pyenv/tests/install_winpkg.rs
git commit -m "Install python.org's Windows packages: resolve, verify, extract, finish in place"
```

---

### Task 8: `pyenv install` for the pyenv-win flavor

**Files:**
- Create: `crates/pyenv/src/commands/install_win.rs`
- Modify: `crates/pyenv/src/commands/mod.rs`, `crates/pyenv/src/help.rs`, `crates/pyenv/src/install/default_packages.rs`, `crates/pyenv/src/install/mod.rs`
- Test: `crates/pyenv/tests/cli_install_win.rs` (`#![cfg(windows)]`)

**Interfaces:**
- Consumes:
  - `winpkg::{install, Done, Job}` (Task 7);
  - `wincatalog::{read_db, parse_code, DbError, DB_NAME, Row}` (Task 4);
  - `winsource::{banner, base}` (Task 5);
  - `Fetcher::direct` (Task 3);
  - `rpyenv_core::{winresolve::resolve, select::win_select, installed::is_staging_name}`;
  - `crate::commands::rehash::rehash`;
  - `crate::install::{watch_interrupt, interrupted, InstallError}`.
- Produces: `commands::install_win::{install, HELP}`, registered as `install` for the pyenv-win flavor.

The command follows the reference's *Options* and *Order of checks* tables exactly. The differences are Decisions 10–12.

- [ ] **Step 1: Write the failing tests.** Create `crates/pyenv/tests/cli_install_win.rs`:

```rust
//! `pyenv install` (pyenv-win flavor) against the tier-1 fake python.org (plan M2b Task 8).
#![cfg(windows)]

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, listing, sha256, zip_bytes};
use common::Fixture;

const BANNER: &str = ":: [Info] ::  Mirror: https://www.python.org/ftp/python\r\n:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json\r\n:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases\r\n";
const HELP_LINES: usize = 17;

fn db(f: &Fixture, codes: &[(&str, bool)]) {
    let rows: Vec<pyenv::install::wincatalog::Row> = codes
        .iter()
        .map(|(c, zip_root)| pyenv::install::wincatalog::Row {
            code: c.to_string(),
            file: format!("{c}.exe"),
            url: format!("https://www.python.org/ftp/python/x/{c}.exe"),
            x64: !c.ends_with("-win32"),
            web_install: false,
            msi: false,
            zip_root_dir: zip_root.then(|| c.to_string()),
        })
        .collect();
    pyenv::install::wincatalog::write_db(&f.root, &rows).unwrap();
}

fn fake() -> Server {
    let zip = zip_bytes(&[("python.exe", b"MZ"), ("pythonw.exe", b"MZ"), ("Lib/os.py", b"")]);
    let sha = sha256(&zip);
    start_with(move |host| {
        let base = format!("{host}/ftp/python");
        vec![
            ("/ftp/python/index-windows.json".into(), vec![Reply::Body(index_json(
                &[(&format!("{base}/3.12.1/python-3.12.1-amd64.zip"), &sha),
                  (&format!("{base}/3.12.2/python-3.12.2-amd64.zip"), &"0".repeat(64))],
                None,
            ))]),
            ("/ftp/python/3.12.1/python-3.12.1-amd64.zip".into(), vec![Reply::Body(zip.clone())]),
            ("/ftp/python/3.12.2/python-3.12.2-amd64.zip".into(), vec![Reply::Body(zip.clone())]),
        ]
    })
}

fn run(f: &Fixture, s: &Server, args: &[&str]) -> common::Run {
    let base = s.url("/ftp/python");
    f.pyenv_env(args, &[("RPYENV_TEST_PYTHON_ORG", base.as_str())])
}

#[test]
fn help_and_help_install_are_identical_and_start_with_the_banner() {
    let f = Fixture::new();
    let a = f.pyenv(&["install", "--help"]);
    let b = f.pyenv(&["help", "install"]);
    assert_eq!((a.code, b.code), (0, 0));
    assert_eq!(a.stdout, b.stdout);
    assert!(a.stdout.starts_with(&format!("{BANNER}Usage: pyenv install [-s] [-f] <version>")));
    assert_eq!(a.stdout.matches("\r\n").count(), 3 + HELP_LINES);
}

#[test]
fn the_mirror_variable_replaces_the_banner() {
    let f = Fixture::new();
    let r = f.pyenv_env(&["install", "--help"], &[("PYTHON_BUILD_MIRROR_URL", "https://m.example/py")]);
    assert!(r.stdout.starts_with(":: [Info] ::  Mirror: https://m.example/py\r\nUsage:"), "{}", r.stdout);
}

#[test]
fn pre_checks_in_pyenv_wins_order() {
    let f = Fixture::new();
    let cases: [(&[&str], &str); 3] = [
        (&["install", "--32only", "--64only", "3.12.1"], "pyenv-install: only --32only or --64only may be specified, not both.\r\n"),
        (&["install", "-r", "--32only", "3.12.1"], "pyenv-install: --register not supported for 32 bits.\r\n"),
        (&["install", "-r", "-a"], "pyenv-install: --register not supported for all versions.\r\n"),
    ];
    for (args, msg) in cases {
        let r = f.pyenv(args);
        assert_eq!((r.code, r.stdout.clone()), (1, format!("{BANNER}{msg}")), "{args:?}");
    }
    let r = f.pyenv(&["install", "3.12.1"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, format!("{BANNER}pyenv-install: no definitions in local database\r\n\r\nPlease update the local database cache with `pyenv update'.\r\n"));
}

#[test]
fn list_prints_the_cache_in_document_order() {
    let f = Fixture::new();
    db(&f, &[("3.12.1-win32", false), ("3.12.1", false), ("pypy3.10-v7.3.19-win64", true)]);
    let r = f.pyenv(&["install", "--list"]);
    assert_eq!((r.code, r.stdout), (0, format!("{BANNER}3.12.1-win32\r\n3.12.1\r\npypy3.10-v7.3.19-win64\r\n")));
}

#[test]
fn an_unknown_version_is_reported_before_anything_installs() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "3.12.1", "9.9.9"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, format!("{BANNER}pyenv-install: definition not found: 9.9.9\r\n\r\nSee all available versions with `pyenv install --list`.\r\nDoes the list seem out of date? Update it using `pyenv update`.\r\n"));
    assert!(!f.root.join("versions").join("3.12.1").exists());
}

#[test]
fn a_prefix_installs_the_newest_known_version_then_rehashes() {
    let f = Fixture::new();
    db(&f, &[("3.12.0rc1", false), ("3.12.1", false), ("3.12.1-win32", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "3.12"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(r.stdout.ends_with(":: [Installing] ::  3.12.1 ...\r\n:: [Info] :: completed! 3.12.1\r\n"), "{}", r.stdout);
    let v = f.root.join("versions").join("3.12.1");
    assert!(v.join("python3.12.exe").is_file());
    assert!(f.root.join("shims").join("python.exe").is_file(), "rehash ran");
    // Installed now: a second run prints only the banner and exits 0.
    let again = run(&f, &s, &["install", "3.12.1"]);
    assert_eq!((again.code, again.stdout), (0, BANNER.to_string()));
}

#[test]
fn a_failure_stops_the_run_exits_1_and_leaves_no_version() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false), ("3.12.2", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "3.12.2", "3.12.1"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains(":: [Error] :: cannot download python-3.12.2-amd64.zip: checksum mismatch"), "{}", r.stdout);
    assert!(r.stdout.contains(":: [Error] :: couldn't install 3.12.2\r\n"));
    assert!(!r.stdout.contains("3.12.1 ..."), "stops at the first failure");
    assert!(!f.root.join("versions").join("3.12.2").exists());
}

#[test]
fn pypy_and_graalpy_codes_are_refused() {
    let f = Fixture::new();
    db(&f, &[("pypy3.10-v7.3.19-win64", true)]);
    let r = f.pyenv(&["install", "pypy3.10-v7.3.19-win64"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.ends_with(":: [Error] :: rpyenv cannot install pypy3.10-v7.3.19-win64 yet: only CPython is supported.\r\n"), "{}", r.stdout);
}

#[test]
fn no_version_and_none_selected_prints_help_and_exits_0() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let r = f.pyenv(&["install"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.starts_with(&format!("{BANNER}Usage: pyenv install")));
}

#[test]
fn register_is_ignored_with_one_info_line() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "-r", "3.12.1"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(r.stdout.contains(":: [Info] :: rpyenv does not register versions for the py launcher; --register was ignored.\r\n"));
}

/// Spec §9.3 (Decision 14): default packages run after a new install, and a failure still
/// counts as a successful install. The fixture's python.exe isn't a real program, so pip fails.
// allowlist D-86
#[test]
fn default_packages_run_after_a_new_install_and_a_failure_still_succeeds() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    std::fs::write(f.root.join("default-packages"), "six
").unwrap();
    let s = fake();
    let r = run(&f, &s, &["install", "3.12.1"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let file = f.root.join("default-packages");
    assert!(
        r.stdout.contains(&format!(":: [Info] :: completed! 3.12.1

pyenv: error installing packages from  `{}'

", file.display())),
        "{}",
        r.stdout
    );
}

#[test]
fn clear_empties_the_cache_and_tolerates_none() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let r = f.pyenv(&["install", "-c"]);
    assert_eq!((r.code, r.stdout.clone()), (0, BANNER.to_string()), "no install_cache: not an error");
    let cache = f.root.join("install_cache");
    std::fs::create_dir_all(cache.join("3.10.11")).unwrap();
    std::fs::write(cache.join("x.zip"), "x").unwrap();
    let r = f.pyenv(&["install", "--clear"]);
    assert_eq!((r.code, r.stdout), (0, BANNER.to_string()));
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
}
```

- [ ] **Step 2: Run them and see them fail.**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_install_win`.
Expected: failures. `pyenv install` is not a pyenv-win command yet, so the output is `pyenv: no such command 'install'`.

- [ ] **Step 3: Implement.** Create `crates/pyenv/src/commands/install_win.rs`:

```rust
//! `pyenv install` for the pyenv-win flavor (docs/parity/pyenv-win-m2-reference.md "install";
//! plan M2b Decisions 10–12). Lines stream to stdout as they happen, in CRLF.

use crate::install::fetch::Fetcher;
use crate::install::wincatalog::{parse_code, read_db, DbError, Row, DB_NAME};
use crate::install::winpkg::{self, Job};
use crate::install::winsource::{banner, base};
use crate::install::{interrupted, watch_interrupt, InstallError};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{installed, select, winresolve};

pub const HELP: &str = "Usage: pyenv install [-s] [-f] <version> [<version> ...] [-r|--register]\n       pyenv install [-f] [--32only|--64only] -a|--all\n       pyenv install [-f] -c|--clear\n       pyenv install -l|--list\n\n  -l/--list              List all available versions\n  -a/--all               Installs all known version from the local version DB cache\n  -c/--clear             Removes downloaded installers from the cache to free space\n  -f/--force             Install even if the version appears to be installed already\n  -s/--skip-existing     Skip the installation if the version appears to be installed already\n  -r/--register          Register version for py launcher\n  -q/--quiet             Install using /quiet. This does not show the UI nor does it prompt for inputs\n  --32only               Installs only 32bit Python using -a/--all switch, no effect on 32-bit windows.\n  --64only               Installs only 64bit Python using -a/--all switch, no effect on 32-bit windows.\n  --dev                  Installs precompiled standard libraries, debug symbols, and debug binaries (only applies to web installer).\n  --help                 Help, list of options allowed on pyenv install\n\n";

/// One line on stdout, CRLF, flushed (pyenv-win prints everything on stdout).
fn say(line: &str) {
    rpyenv_core::textout::write(false, &format!("{line}\r\n"));
}

/// `\n`-separated text, each line CRLF.
fn say_text(text: &str) {
    rpyenv_core::textout::write(false, &text.replace('\n', "\r\n"));
}

#[derive(Default)]
struct Opts {
    list: bool,
    force: bool,
    all: bool,
    clear: bool,
    only32: bool,
    only64: bool,
    register: bool,
}

/// pyenv-win's `Check32Bit`: on a 32-bit host every name gets `-win32` unless it has it.
fn check32(code: &str, arch_suffix: &str) -> String {
    if arch_suffix == "-win32" && !code.to_ascii_lowercase().ends_with("-win32") {
        format!("{code}-win32")
    } else {
        code.to_string()
    }
}

/// `-c`: every file, then every folder, of `install_cache` (reference "--clear"). A missing
/// folder is not an error (allowlist).
fn clear(ctx: &Ctx) -> i32 {
    let cache = ctx.root.join("install_cache");
    let Ok(rd) = std::fs::read_dir(&cache) else {
        return 0;
    };
    let entries: Vec<std::path::PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    let mut status = 0;
    for p in entries.iter().filter(|p| !p.is_dir()) {
        if let Err(e) = std::fs::remove_file(p) {
            say(&format!("pyenv: Error deleting file {}: {e}", p.display()));
            status = 1;
        }
    }
    for p in entries.iter().filter(|p| p.is_dir()) {
        if let Err(e) = std::fs::remove_dir_all(p) {
            say(&format!("pyenv: Error deleting folder {}: {e}", p.display()));
            status = 1;
        }
    }
    status
}

pub fn install(ctx: &Ctx, args: &[&str]) -> Output {
    say_text(&banner());
    let db = read_db(&ctx.root);
    let codes: Vec<String> = match &db {
        Ok(rows) => rows.iter().map(|r| r.code.clone()).collect(),
        Err(_) => Vec::new(),
    };
    let mut o = Opts::default();
    let mut wanted: Vec<String> = Vec::new();
    for a in args {
        match *a {
            "--help" => {
                say_text(HELP);
                return Output::new();
            }
            "-l" | "--list" => o.list = true,
            "-f" | "--force" => o.force = true,
            // Accepted with no effect, as in pyenv-win (reference "Options").
            "-s" | "--skip-existing" | "-q" | "--quiet" | "--dev" => {}
            "-a" | "--all" => o.all = true,
            "-c" | "--clear" => o.clear = true,
            "--32only" => o.only32 = true,
            "--64only" => o.only64 = true,
            "-r" | "--register" => o.register = true,
            v => {
                let r = winresolve::resolve(v, &codes, ctx.arch_suffix);
                if !wanted.contains(&r) {
                    wanted.push(r);
                }
            }
        }
    }
    if ctx.arch_suffix == "-win32" {
        o.only32 = false;
        o.only64 = false;
    }
    let fail = |m: &str| {
        say(m);
        Output::new().with_code(1)
    };
    if o.only32 && o.only64 {
        return fail("pyenv-install: only --32only or --64only may be specified, not both.");
    }
    if o.register && o.only32 {
        return fail("pyenv-install: --register not supported for 32 bits.");
    }
    if o.register && o.all {
        return fail("pyenv-install: --register not supported for all versions.");
    }
    let rows: Vec<Row> = match db {
        Ok(r) => r,
        Err(DbError::Missing | DbError::Empty) => {
            say("pyenv-install: no definitions in local database");
            say("");
            return fail("Please update the local database cache with `pyenv update'.");
        }
        Err(DbError::Malformed(m)) => {
            return fail(&format!("pyenv-install: cannot read {}: {m}", ctx.root.join(DB_NAME).display()));
        }
    };
    if o.list {
        for r in &rows {
            say(&r.code);
        }
        return Output::new();
    }
    if o.clear {
        return Output::new().with_code(clear(ctx));
    }
    if o.all {
        wanted = rows
            .iter()
            .filter(|r| !(o.only64 && !r.x64) && !(o.only32 && r.x64))
            .map(|r| check32(&r.code, ctx.arch_suffix))
            .filter(|c| codes.contains(c))
            .fold(Vec::new(), |mut v, c| {
                if !v.contains(&c) {
                    v.push(c);
                }
                v
            });
    } else if wanted.is_empty() {
        match select::win_select(ctx).first() {
            Some(s) => wanted.push(winresolve::resolve(&s.name, &codes, ctx.arch_suffix)),
            None => {
                say_text(HELP);
                return Output::new();
            }
        }
    }
    if let Some(missing) = wanted.iter().find(|w| !codes.contains(w)) {
        say(&format!("pyenv-install: definition not found: {missing}"));
        say("");
        say("See all available versions with `pyenv install --list`.");
        return fail("Does the list seem out of date? Update it using `pyenv update`.");
    }
    watch_interrupt();
    let fetcher = Fetcher::direct();
    let base = base();
    let mut status = 0;
    for w in &wanted {
        if interrupted() {
            return Output::new().with_code(130);
        }
        let row = rows.iter().find(|r| &r.code == w).expect("checked above");
        let code = parse_code(w).filter(|_| row.zip_root_dir.is_none());
        let Some(code) = code.filter(|_| !installed::is_staging_name(w)) else {
            say(&format!(":: [Error] :: rpyenv cannot install {w} yet: only CPython is supported."));
            status = 1;
            break;
        };
        let job = Job { root: &ctx.root, code: &code, force: o.force, base: &base, fetcher: &fetcher };
        match winpkg::install(&job, &mut |l| say(l)) {
            Ok(_) => {}
            Err(InstallError::Interrupted) => return Output::new().with_code(130),
            Err(e) => {
                if let InstallError::Message(m) = e {
                    say(&format!(":: [Error] :: {m}"));
                }
                say(&format!(":: [Error] :: couldn't install {w}"));
                status = 1;
                break;
            }
        }
    }
    if interrupted() {
        return Output::new().with_code(130);
    }
    if o.register {
        say(":: [Info] :: rpyenv does not register versions for the py launcher; --register was ignored.");
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    if r.code != 0 {
        r.emit(ctx.flavor);
        status = status.max(r.code);
    }
    Output::new().with_code(status)
}
```

Default packages (Decision 14):
- In `install/mod.rs`, drop `#[cfg(unix)]` from `pub mod default_packages;`.
- In `default_packages.rs`, find the interpreter per OS. Replace `let bin = prefix.join("bin");` and the `let python = …;` expression with:

```rust
    // pyenv-win versions keep python.exe in the prefix itself; pyenv's in `bin/`.
    let python = if cfg!(windows) {
        Some(prefix.join("python.exe")).filter(|p| p.is_file())
    } else {
        let bin = prefix.join("bin");
        ["python", "python3"]
            .iter()
            .map(|n| bin.join(n))
            .find(|p| p.exists())
            .or_else(|| {
                std::fs::read_dir(&bin)
                    .ok()?
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().is_some_and(|n| {
                            let n = n.to_string_lossy();
                            n.starts_with("python") && n.ends_with(|c: char| c.is_ascii_digit())
                        })
                    })
                    .max()
            })
    };
```

  The rest of the function is unchanged.
- In `install_win.rs`'s loop, the `Ok(_)` arm becomes:

```rust
            Ok(winpkg::Done::Installed) => {
                let prefix = ctx.versions_dir().join(w);
                if let Some(line) = crate::install::default_packages::run(&ctx.root, &prefix) {
                    say(&line);
                }
            }
            Ok(winpkg::Done::Skipped) => {}
```

In `commands/mod.rs`:
- add `pub mod install_win;`;
- add `("install", install_win::install)` to `WIN_ONLY`;
- update `WIN_ONLY`'s doc comment.

In `help.rs`'s `help_win`, next to the `update` arm from Task 5, add:

```rust
        Some("install") => {
            o.stdout.push_str(&crate::install::winsource::banner());
            o.stdout.push_str(crate::commands::install_win::HELP);
        }
```

- [ ] **Step 4: Run the tests and see them pass.**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_install_win`. Expected: 12 passed.

- [ ] **Step 5: Real install, measured once on this machine.** Use a scratch root, **never** the real one.

```bash
mkdir -p /c/tmp/m2b_t8_root
export PYENV_ROOT=C:/tmp/m2b_t8_root PYENV=C:/tmp/m2b_t8_root PYENV_HOME=C:/tmp/m2b_t8_root
./target/debug/pyenv.exe update
time ./target/debug/pyenv.exe install 3.10.11
time ./target/debug/pyenv.exe install 3.12.10
C:/tmp/m2b_t8_root/versions/3.10.11/python.exe -c "import ssl, sqlite3, ctypes, lzma, bz2, tkinter, venv; print('ok')"
C:/tmp/m2b_t8_root/versions/3.12.10/Scripts/pip.exe --version
C:/tmp/m2b_t8_root/versions/3.10.11/Scripts/pip.exe --version
```

Record each command's full output and time in your report.
- **The 3.10.11 install exercises** component MSIs, the real Dower signatures and ensurepip.
- **The 3.12.10 install exercises** the index zip and the pip launchers.
- **If a Python step fails** (ensurepip, or the pip reinstall), stop and report DONE_WITH_CONCERNS with the output. Those commands were measured only on 3.11.0 during planning.

Then `unset` the variables and delete `C:\tmp\m2b_t8_root` by literal path.

- [ ] **Step 6: Mutation check.** Remove the `break` after a failed install, so the loop continues as pyenv-win's does. `a_failure_stops_the_run_exits_1_and_leaves_no_version` must fail. Restore.

- [ ] **Step 7: Check, both OSes, and commit.** Run fmt, clippy, `cargo test --workspace` on both OSes, and `python ci/shim_deps.py`.

```bash
git add crates/pyenv/src/commands/install_win.rs crates/pyenv/src/commands/mod.rs crates/pyenv/src/help.rs crates/pyenv/src/install/mod.rs crates/pyenv/src/install/default_packages.rs crates/pyenv/tests/cli_install_win.rs
git commit -m "Add pyenv install for Windows with pyenv-win's options, checks and output"
```

---

### Task 9: `pyenv uninstall` and `pyenv latest` for the pyenv-win flavor

**Files:**
- Create: `crates/pyenv/src/commands/uninstall_win.rs`
- Modify:
  - `crates/rpyenv-core/src/winresolve.rs` (`find_latest`);
  - `crates/pyenv/src/commands/latest.rs` (the pyenv-win branch);
  - `crates/pyenv/src/commands/mod.rs` (`latest` moves to `COMMANDS`; `uninstall` joins `WIN_ONLY`);
  - `crates/pyenv/src/help.rs` (pyenv-win topics `latest` and `uninstall`).
- Test: `crates/pyenv/tests/cli_uninstall_win.rs` and `crates/pyenv/tests/cli_latest_win.rs` (`#![cfg(windows)]`); a unit test in `winresolve.rs`.

**Interfaces:**
- Consumes:
  - `install::{is_plain_name, child_of}`;
  - `installed::{names, is_staging_name}`;
  - `wincatalog::read_db` (Task 4);
  - `crate::commands::rehash::rehash`.
- Produces:
  - `rpyenv_core::winresolve::find_latest(prefix, candidates, arch) -> Option<String>`, which is pyenv-win's `FindLatestVersion`. `resolve` becomes `find_latest(…).unwrap_or(prefix)`.
  - `commands::uninstall_win::{uninstall, HELP}`.
  - `commands::latest::WIN_HELP`.

- [ ] **Step 1: Write the failing tests.**

In `crates/rpyenv-core/src/winresolve.rs`'s `mod tests`:

```rust
    #[test]
    fn find_latest_distinguishes_a_match_from_no_match() {
        let i = v(&["3.12.1", "3.12.10"]);
        assert_eq!(find_latest("3.12.1", &i, ""), Some("3.12.1".to_string()));
        assert_eq!(find_latest("3.12", &i, ""), Some("3.12.10".to_string()));
        assert_eq!(find_latest("9", &i, ""), None);
        assert_eq!(resolve("9", &i, ""), "9");
    }
```

Create `crates/pyenv/tests/cli_latest_win.rs`. Its cases are pyenv-win's own `tests/test_pyenv_feature_latest.py`, plus `-k`:

```rust
//! `pyenv latest` (pyenv-win flavor; pyenv-win-m1-reference.md "latest", m2 reference
//! "latest"). The installed cases are pyenv-win's own test_pyenv_feature_latest.py.
#![cfg(windows)]

mod common;
use common::Fixture;

const HELP: &str = "Usage: pyenv latest [-k|--known] [-q|--quiet] <prefix>\r\n\r\n  -k/--known      Select from all known versions instead of installed\r\n  -q/--quiet      Do not print an error message on resolution failure\r\n\r\n";

fn with(versions: &[&str]) -> Fixture {
    let f = Fixture::new();
    for v in versions {
        f.version(v);
    }
    f
}

#[test]
fn help_and_no_arguments() {
    let f = with(&[]);
    let r = f.pyenv(&["latest", "--help"]);
    assert_eq!((r.code, r.stdout), (0, HELP.to_string()));
    let r = f.pyenv(&["latest"]);
    assert_eq!((r.code, r.stdout), (1, HELP.to_string()));
    let r = f.pyenv(&["latest", "-q"]);
    assert_eq!((r.code, r.stdout), (1, String::new()));
    let r = f.pyenv(&["latest", "-k"]);
    assert_eq!((r.code, r.stdout), (1, "pyenv-latest: missing <prefix> argument\r\n".to_string()));
}

#[test]
fn installed_versions_resolve_as_pyenv_win_does() {
    let f = with(&["3.1.4", "3.11.0", "3.2.0", "3.2.5", "3.9.1"]);
    for (p, want) in [("3.1", "3.1.4"), ("3.2", "3.2.5"), ("3.2.5", "3.2.5")] {
        let r = f.pyenv(&["latest", p]);
        assert_eq!((r.code, r.stdout), (0, format!("{want}\r\n")), "{p}");
    }
    let r = f.pyenv(&["latest", "1"]);
    assert_eq!((r.code, r.stdout), (1, "pyenv-latest: no installed versions match the prefix '1'.\r\n".to_string()));
    let r = f.pyenv(&["latest", "-q", "1"]);
    assert_eq!((r.code, r.stdout), (1, String::new()));
}

#[test]
fn the_architecture_suffix_must_match() {
    let f = with(&["3.1.0-win32", "3.1.4"]);
    let r = f.pyenv_env(&["latest", "3.1"], &[("PYENV_FORCE_ARCH", "X86")]);
    assert_eq!(r.stdout, "3.1.0-win32\r\n");
    let r = f.pyenv_env(&["latest", "3.1"], &[("PYENV_FORCE_ARCH", "AMD64")]);
    assert_eq!(r.stdout, "3.1.4\r\n");
}

#[test]
fn known_reads_the_version_cache() {
    let f = with(&[]);
    let rows: Vec<pyenv::install::wincatalog::Row> = ["3.12.0rc1", "3.12.1", "3.12.10", "3.12.10-win32"]
        .iter()
        .map(|c| pyenv::install::wincatalog::Row {
            code: c.to_string(), file: String::new(), url: String::new(),
            x64: true, web_install: false, msi: false, zip_root_dir: None,
        })
        .collect();
    let r = f.pyenv(&["latest", "-k", "3.12"]);
    assert_eq!((r.code, r.stdout), (1, "pyenv-latest: no known versions match the prefix '3.12'.\r\n".to_string()), "no cache: no candidates");
    pyenv::install::wincatalog::write_db(&f.root, &rows).unwrap();
    let r = f.pyenv(&["latest", "-k", "3.12"]);
    assert_eq!((r.code, r.stdout), (0, "3.12.10\r\n".to_string()));
    let r = f.pyenv(&["latest", "--known", "1"]);
    assert_eq!((r.code, r.stdout), (1, "pyenv-latest: no known versions match the prefix '1'.\r\n".to_string()));
}
```

Create `crates/pyenv/tests/cli_uninstall_win.rs`:

```rust
//! `pyenv uninstall` (pyenv-win flavor; reference "uninstall"; review focus 4).
#![cfg(windows)]

mod common;
use common::Fixture;
use std::io::Write;
use std::process::Stdio;

const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> [<version> ...]\r\n       pyenv uninstall [-f|--force] [-a|--all]\r\n\r\n   -f/--force  Attempt to remove the specified version without prompting\r\n               for confirmation. If the version does not exist, do not\r\n               display an error message.\r\n\r\n   -a/--all    *Caution* Attempt to remove all installed versions.\r\n\r\nSee `pyenv versions` for a complete list of installed versions.\r\n\r\n";

fn with(versions: &[&str]) -> Fixture {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("versions")).unwrap();
    for v in versions {
        f.version(v);
        std::fs::write(f.root.join("versions").join(v).join("python.exe"), "").unwrap();
    }
    f
}

fn with_stdin(f: &Fixture, args: &[&str], input: &[u8]) -> (i32, String) {
    let mut child = f
        .command(std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")), &f.work, &[])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), common::decode(&out.stdout))
}

#[test]
fn help_and_no_arguments_print_help_and_exit_0() {
    let f = with(&["3.12.1"]);
    for args in [&["uninstall", "--help"][..], &["uninstall"][..]] {
        let r = f.pyenv(args);
        assert_eq!((r.code, r.stdout), (0, HELP.to_string()), "{args:?}");
    }
}

#[test]
fn messages_and_exit_codes() {
    let f = with(&[]);
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!((r.code, r.stdout), (1, "pyenv: No valid versions of python installed.\r\n".to_string()));
    let f = with(&["3.12.1"]);
    let r = f.pyenv(&["uninstall", "bad!name"]);
    assert_eq!((r.code, r.stdout), (1, "pyenv: Unrecognized python version: bad!name\r\n".to_string()));
    let r = f.pyenv(&["uninstall", "9.9"]);
    assert_eq!((r.code, r.stdout), (0, "pyenv: version '9.9' not installed\r\n".to_string()));
    let r = f.pyenv(&["uninstall", "9.9", "9.8"]);
    assert_eq!((r.code, r.stdout), (0, String::new()), "several missing: silent");
}

// allowlist D-80
#[test]
fn every_version_is_removed_with_no_false_errors_and_shims_follow() {
    let f = with(&["9.9.4", "9.9.5", "9.9.6", "3.12.1"]);
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    let r = f.pyenv(&["uninstall", "9.9.4", "9.9.5", "9.9.6"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(r.stdout, "pyenv: Successfully uninstalled 9.9.4\r\npyenv: Successfully uninstalled 9.9.5\r\npyenv: Successfully uninstalled 9.9.6\r\n");
    for v in ["9.9.4", "9.9.5", "9.9.6"] {
        assert!(!f.root.join("versions").join(v).exists());
    }
    assert!(f.root.join("versions").join("3.12.1").is_dir());
}

#[test]
fn on_x86_names_get_the_win32_suffix() {
    let f = with(&["9.9.7-win32"]);
    let r = f.pyenv_env(&["uninstall", "9.9.7"], &[("PYENV_FORCE_ARCH", "X86")]);
    assert_eq!((r.code, r.stdout), (0, "pyenv: Successfully uninstalled 9.9.7-win32\r\n".to_string()));
}

/// Review focus 4: the root sits three levels inside a tempdir with a canary at every level;
/// no argument may remove anything but a version folder.
// allowlist D-80
#[test]
fn escaping_names_remove_nothing() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("a").join("b").join("root");
    let mut canaries = vec![t.path().join("canary")];
    for dir in [t.path().join("a"), t.path().join("a").join("b"), root.clone(), root.join("versions")] {
        std::fs::create_dir_all(&dir).unwrap();
        canaries.push(dir.join("canary"));
    }
    for c in &canaries {
        std::fs::write(c, "").unwrap();
    }
    std::fs::create_dir_all(root.join("versions").join("3.12.1")).unwrap();
    std::fs::create_dir_all(root.join("versions").join(".tmp-3.12.2")).unwrap();
    let pyenv = std::path::Path::new(env!("CARGO_BIN_EXE_pyenv"));
    for arg in ["..", ".", "a\\b", "..\\..", "C:\\x", ".tmp-3.12.2", ".old-3.12.1", "versions", "..\\versions"] {
        let out = std::process::Command::new(pyenv)
            .args(["uninstall", "-f", arg])
            .env_clear()
            .env("PYENV_ROOT", &root)
            .env("PYENV", &root)
            .env("PYENV_HOME", &root)
            .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
            .current_dir(t.path())
            .output()
            .unwrap();
        let text = common::decode(&out.stdout);
        assert!(!text.contains("Successfully"), "{arg}: {text}");
        for c in &canaries {
            assert!(c.is_file(), "{arg} removed {}", c.display());
        }
        assert!(root.join("versions").join("3.12.1").is_dir(), "{arg}");
        assert!(root.join("versions").join(".tmp-3.12.2").is_dir(), "{arg}");
    }
}

// allowlist D-81
#[test]
fn all_asks_first_and_eof_or_no_keeps_everything() {
    for (input, gone) in [(&b"n\r\n"[..], false), (b"\r\n", false), (b"", false), (b"x\r\nY\r\n", true)] {
        let f = with(&["9.9.3", "9.9.8"]);
        let (code, out) = with_stdin(&f, &["uninstall", "-a"], input);
        assert_eq!(code, 0, "{input:?}: {out}");
        let prompts = out.matches("pyenv: Confirm uninstall all? (Y/N): ").count();
        assert_eq!(prompts, if input.starts_with(b"x") { 2 } else { 1 }, "{input:?}: {out}");
        assert_eq!(!f.root.join("versions").join("9.9.3").exists(), gone, "{input:?}");
    }
    let f = with(&["9.9.3"]);
    let r = f.pyenv(&["uninstall", "-a", "-f"]);
    assert_eq!((r.code, r.stdout), (0, "pyenv: Successfully uninstalled 9.9.3\r\n".to_string()));
}

#[test]
fn an_install_in_progress_keeps_its_version() {
    let f = with(&["3.12.1"]);
    let _held = pyenv::install::txn::Txn::begin_for(
        &f.root.join("versions"),
        "3.12.1",
        rpyenv_core::flavor::Flavor::PyenvWin,
    )
    .unwrap();
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.starts_with("pyenv: an install of 3.12.1 is in progress"), "{}", r.stdout);
    assert!(f.root.join("versions").join("3.12.1").is_dir());
}

// allowlist D-80
#[test]
fn a_read_only_file_does_not_stop_the_removal() {
    let f = with(&["3.12.1"]);
    let p = f.root.join("versions").join("3.12.1").join("python.exe");
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&p, perm).unwrap();
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!((r.code, r.stdout), (0, "pyenv: Successfully uninstalled 3.12.1\r\n".to_string()));
}
```

`pyenv` and `rpyenv_core` are already dependencies of the test crate (`cli_uninstall.rs` uses `pyenv::install::txn`). If `rpyenv_core` isn't reachable from integration tests, use `pyenv::install::txn`'s re-export, or add `rpyenv-core.workspace = true` to `[dev-dependencies]`.

- [ ] **Step 2: Run them and see them fail.**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_latest_win --test cli_uninstall_win`, then `cargo test -p rpyenv-core winresolve`.
Expected:
- `latest` and `uninstall` are not pyenv-win commands yet, so the output is `no such command`;
- `find_latest` is unresolved.

- [ ] **Step 3: Implement `find_latest`.** In `winresolve.rs`, rename `resolve`'s body to `pub fn find_latest(prefix: &str, installed: &[String], arch: &str) -> Option<String>`, returning `best.map(|(c, _)| c.clone())`, and make `resolve`:

```rust
/// The resolved name, or `prefix` unchanged when nothing qualifies. `installed` is in
/// directory order; on a numeric tie the first one wins (allowlist D-05).
pub fn resolve(prefix: &str, installed: &[String], arch: &str) -> String {
    find_latest(prefix, installed, arch).unwrap_or_else(|| prefix.to_string())
}
```

with `find_latest`'s doc comment: "pyenv-win's `FindLatestVersion`: the newest qualifying candidate, or `None`; `latest` has no fall-back to the argument (M1 reference, *latest*)."

- [ ] **Step 4: Implement `latest`'s pyenv-win branch.** In `commands/latest.rs`:

```rust
/// pyenv-win's `pyenv-latest.vbs` help (M2 reference, "latest").
pub const WIN_HELP: &str = "Usage: pyenv latest [-k|--known] [-q|--quiet] <prefix>\n\n  -k/--known      Select from all known versions instead of installed\n  -q/--quiet      Do not print an error message on resolution failure\n\n";

/// pyenv-win's `pyenv latest` (M1 reference, "latest"): the last non-option argument is the
/// prefix; `--help` wins wherever it is; no arguments at all is the help with exit 1.
fn latest_win(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    let (mut known, mut quiet, mut prefix) = (false, false, "");
    for a in args {
        match *a {
            "--help" => {
                o.stdout.push_str(WIN_HELP);
                return o;
            }
            "-k" | "--known" => known = true,
            "-q" | "--quiet" => quiet = true,
            p => prefix = p,
        }
    }
    if args.is_empty() {
        o.stdout.push_str(WIN_HELP);
        return o.with_code(1);
    }
    if prefix.is_empty() {
        if !quiet {
            o.out("pyenv-latest: missing <prefix> argument");
        }
        return o.with_code(1);
    }
    let candidates: Vec<String> = if known {
        crate::install::wincatalog::read_db(&ctx.root)
            .map(|rows| rows.into_iter().map(|r| r.code).collect())
            .unwrap_or_default()
    } else {
        installed::names(&ctx.versions_dir(), ctx.flavor)
    };
    match rpyenv_core::winresolve::find_latest(prefix, &candidates, ctx.arch_suffix) {
        Some(v) => o.out(v),
        None => {
            if !quiet {
                let kind = if known { "known" } else { "installed" };
                o.out(format!("pyenv-latest: no {kind} versions match the prefix '{prefix}'."));
            }
            o.code = 1;
        }
    }
    o
}
```

At the top of `pub fn latest`, add `if ctx.flavor == rpyenv_core::flavor::Flavor::PyenvWin { return latest_win(ctx, args); }`.

In `commands/mod.rs`, move `("latest", latest::latest)` from `LINUX_ONLY` into `COMMANDS`.

In `help.rs`'s `PYENV_WIN` topic table, add:
- `topic("latest", None, None, commands::latest::WIN_HELP)`;
- `topic("uninstall", None, None, commands::uninstall_win::HELP)`.

These topics can be static because neither command prints a banner.

- [ ] **Step 5: Implement `uninstall_win.rs`.** Add `pub mod uninstall_win;` and `("uninstall", uninstall_win::uninstall)` to `WIN_ONLY`.

```rust
//! `pyenv uninstall` for the pyenv-win flavor (docs/parity/pyenv-win-m2-reference.md
//! "uninstall"). No registry keys are touched (plan M2b Decision 11), so pyenv-win's
//! alternating false error can't happen; names never reach outside `versions\` (review
//! focus 4). Lines stream to stdout in CRLF as each version goes.

use crate::install::{child_of, is_plain_name};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::installed;
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> [<version> ...]\n       pyenv uninstall [-f|--force] [-a|--all]\n\n   -f/--force  Attempt to remove the specified version without prompting\n               for confirmation. If the version does not exist, do not\n               display an error message.\n\n   -a/--all    *Caution* Attempt to remove all installed versions.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";

fn say(line: &str) {
    rpyenv_core::textout::write(false, &format!("{line}\r\n"));
}

/// pyenv-win's `IsVersion`: `^[a-zA-Z_0-9-.]+$`.
fn is_version(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// `versions\<name>` for a name that can only be a version folder: one plain component, not
/// `.`/`..`, not an installer staging name.
fn target(versions: &Path, name: &str) -> Option<PathBuf> {
    if !is_plain_name(name) || installed::is_staging_name(name) {
        return None;
    }
    child_of(versions, name)
}

/// pyenv-win's `-a` prompt: `y` yes; `n`, an empty line or EOF no; anything else asks again.
fn confirm() -> bool {
    let stdin = std::io::stdin();
    loop {
        rpyenv_core::textout::write(false, "pyenv: Confirm uninstall all? (Y/N): ");
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        match line.trim().chars().next().map(|c| c.to_ascii_lowercase()) {
            Some('y') => return true,
            Some('n') | None => return false,
            Some(_) => {}
        }
    }
}

/// The install lock M2a's transaction holds (`<root>\.locks\install-<name>`): a version being
/// installed is not pulled away.
fn locked(root: &Path, name: &str) -> bool {
    let p = root.join(".locks").join(format!("install-{name}"));
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p)
        .is_ok_and(|l| matches!(l.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
}

pub fn uninstall(ctx: &Ctx, args: &[&str]) -> Output {
    let (mut force, mut all) = (false, false);
    let mut names: Vec<String> = Vec::new();
    for a in args {
        match *a {
            "--help" => {
                let mut o = Output::new();
                o.stdout.push_str(HELP);
                return o;
            }
            "-f" | "--force" => force = true,
            "-a" | "--all" => all = true,
            v if is_version(v) => {
                // pyenv-win's `Check32Bit`.
                let lower = v.to_ascii_lowercase();
                names.push(if ctx.arch_suffix == "-win32" && !lower.ends_with("-win32") {
                    format!("{v}-win32")
                } else {
                    v.to_string()
                });
            }
            v => {
                let mut o = Output::new();
                o.out(format!("pyenv: Unrecognized python version: {v}"));
                return o.with_code(1);
            }
        }
    }
    if names.is_empty() && !all {
        let mut o = Output::new();
        o.stdout.push_str(HELP);
        return o;
    }
    let versions = ctx.versions_dir();
    let installed_names = installed::names(&versions, Flavor::PyenvWin);
    if installed_names.is_empty() {
        let mut o = Output::new();
        o.out("pyenv: No valid versions of python installed.");
        return o.with_code(1);
    }
    if all {
        if !force && !confirm() {
            return Output::new();
        }
        names = installed_names.into_iter().filter(|n| is_version(n)).collect();
    }
    let single = names.len() == 1;
    let mut status = 0;
    for n in &names {
        let Some(p) = target(&versions, n).filter(|p| p.is_dir()) else {
            if single {
                say(&format!("pyenv: version '{n}' not installed"));
            }
            continue;
        };
        if locked(&ctx.root, n) {
            say(&format!(
                "pyenv: an install of {n} is in progress ({})",
                ctx.root.join(".locks").join(format!("install-{n}")).display()
            ));
            status = 1;
            continue;
        }
        let is_link = p.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false);
        let gone = if is_link { std::fs::remove_dir(&p) } else { std::fs::remove_dir_all(&p) };
        match gone {
            Ok(()) => say(&format!("pyenv: Successfully uninstalled {n}")),
            Err(e) => {
                say(&format!("pyenv: Error uninstalling version {n}: {e}"));
                status = 1;
            }
        }
    }
    if status == 0 {
        let r = crate::commands::rehash::rehash(ctx, &[]);
        if r.code != 0 {
            r.emit(ctx.flavor);
            status = r.code;
        }
    }
    Output::new().with_code(status)
}
```

`a_read_only_file_does_not_stop_the_removal` relies on `std::fs::remove_dir_all` deleting read-only files on Windows. That was measured while planning with Rust 1.96.0: a read-only file in a subfolder was removed, and the folder was gone. The allowlist row D-80 states it.

- [ ] **Step 6: Run the tests and see them pass.**

Run: `cargo build --workspace`, then `cargo test -p pyenv --test cli_latest_win --test cli_uninstall_win`, then `cargo test -p rpyenv-core`.
Expected: all pass. The Linux `latest` and `uninstall` suites in WSL must stay green, since only Windows routes changed.

- [ ] **Step 7: Mutation checks.** For each mutation, change the code, run the named test and see it fail, then restore.
  - Remove `|| installed::is_staging_name(name)` from `target`. `escaping_names_remove_nothing` must fail.
  - Make `confirm` treat EOF as yes. `all_asks_first_and_eof_or_no_keeps_everything` must fail.

- [ ] **Step 8: Check, both OSes, and commit.** Run fmt, clippy and `cargo test --workspace` on both OSes, and `python ci/shim_deps.py`.

```bash
git add crates/rpyenv-core/src/winresolve.rs crates/pyenv/src/commands/latest.rs crates/pyenv/src/commands/uninstall_win.rs crates/pyenv/src/commands/mod.rs crates/pyenv/src/help.rs crates/pyenv/tests/cli_latest_win.rs crates/pyenv/tests/cli_uninstall_win.rs
git commit -m "Add pyenv uninstall and pyenv latest for Windows, as pyenv-win has them"
```

---

### Task 10: Parity wiring

**Files:**
- Modify:
  - `docs/parity/allowlist.md`;
  - `parity/allowlist.py`;
  - `parity/expected/pyenv-win.txt`;
  - `parity/diff_cases.py`;
  - `parity/golden/windows/*` (regenerated);
  - the test files from Tasks 4–9, which get `// allowlist D-NN` citations.
- Test: `python parity/coverage.py`, `python parity/diff.py …`, the pyenv-win pytest overlay, and the parity unit tests.

**Interfaces:**
- Consumes: every command from Tasks 5, 8 and 9.
- Produces: allowlist rows **D-73 to D-86**, `M2b` in `DELIVERED`, and Windows differential cases.

This task follows spec §12's checklist item 3:
1. `DELIVERED`;
2. the bats wrapper, which is Linux-only and not needed here;
3. prune the expected lists;
4. regenerate the goldens and review `git diff parity/golden`.

- [ ] **Step 1: Add the rows.** Append to `docs/parity/allowlist.md`'s table, in this order and with this text. The `rpyenv` cell of D-80 must state what Task 9 measured for read-only files.

```
| D-73 | Windows | `install` | Downloads python.org's `.exe` installer and runs `dark.exe -x` plus `msiexec /a` per component MSI (3.5+), or `msiexec /a` on the single `.msi` (≤ 3.4); nothing is verified | Installs the Install Manager zip (3.11+, checked against the index's SHA-256) or python.org's component or single MSIs (checked against the release manager's `.asc`, with three pinned keys), extracted in-process. Debug MSIs (`*_d`, `*_pdb`) are not installed. Zip installs have no `Lib\test` or `Tools`, because python.org's zips have none | Spec §9.1 and §9.3: no installer runs, nothing is registered, and every file is checked against what python.org publishes. |
| D-74 | Windows | `install` | After an installer, MSI or ensurepip failure: `[Error]` lines, the next version is attempted, and the exit code is 0 | Stops at the first failure of any kind with `:: [Error] :: <reason>` and `:: [Error] :: couldn't install <code>`, exit 1 | A failed install must not report success. |
| D-75 | Windows | `install`, `install -f` | A failure leaves the partial version folder, which later runs skip; `-f` deletes the version and its cached installer before downloading | Nothing partial is left: the version is staged and renamed into place. `-f` keeps the old version until the new one is complete, then carries over the `Scripts\` and `Lib\site-packages` entries the new one lacks; the download cache is kept and checked again | Spec §9.3 (atomic); the old version must survive a failed reinstall. |
| D-76 | Windows | `install -r`, `--register` | Writes PEP 514 keys under `HKCU\SOFTWARE\Python\PythonCore\<code>` | Writes no keys; prints `:: [Info] :: rpyenv does not register versions for the py launcher; --register was ignored.` | Spec §9.1: nothing is written to the registry. |
| D-77 | Windows | `install -c` | With no `install_cache` folder: the banner only, exit 1 | The banner only, exit 0 | There is nothing to clear, so nothing failed. |
| D-78 | Windows | `install` | (nothing is verified) | For a file python.org publishes no `.asc` for: `:: [Warning] :: python.org publishes no signature for <files>; checked only by HTTPS.` before `[Installing]` | Spec §9.1 *Unsigned files* (decided 2026-10-03). |
| D-79 | Windows | `update` | Reads python.org's HTML through MSHTML's `htmlfile`, which fails on current Windows (`This command is not supported.` on stderr, exit 0, cache unchanged); errors read `Error(0x<HEX>): <text>`; writes no free-threaded rows | Reads python.org's folder listings and the Install Manager index; adds `t` codes for free-threaded zips and no PyPy or GraalPy rows (M7); errors read `Error(<status>): <reason>` or `Error: <reason>`; the page count is rpyenv's own | Upstream's `update` doesn't work; spec §9.1. |
| D-80 | Windows | `uninstall` | Deletes `HKCU\…\PythonCore\<name>` keys, and a missing key leaks into the next version's result (a false `Error (-2147024894)` for every second version, exit 1); `..` passes `IsVersion` and removes `versions\..`; a read-only file needs `-f` | Touches no registry keys; `.`, `..` and the installer's staging names are "not installed"; read-only files are removed with or without `-f` (measured while planning: Rust 1.96's `remove_dir_all` deletes them on Windows) | Defects, and spec §9.3: deletion stays inside `versions\<name>`. |
| D-81 | Windows | `uninstall -a` | EOF at the prompt prints a VBScript runtime error on stderr, exit 0 | Nothing printed after the prompt, exit 0 | Script error leak. |
| D-82 | Windows | `install` | Installs PyPy and GraalPy rows from their zips | `:: [Error] :: rpyenv cannot install <code> yet: only CPython is supported.`, exit 1 | Other Python families arrive in M7 (spec §15.1). |
| D-83 | Windows | `install`, `latest -k` | A cache that fails pyenv-win's XSD prints MSXML's `Validation error in DB cache(0x…) on line …` | `install` prints `pyenv-install: cannot read <path>: <reason>`; `latest -k` treats it as no versions | MSXML-specific text. |
| D-84 | Windows | `install` | Has no free-threaded versions | `3.13.0t`, `3.13.0t-win32`, `3.13.0t-arm` and later install from python.org's free-threaded zips; `python.exe` is a copy of `python3.13t.exe` | python.org publishes them; pyenv-win's file-name rule can't express them. |
| D-85 | Windows | `install` | `[Downloading] ::  From <installer URL>` and `To   <installer file>`, once per version, only when the installer isn't cached | `From` the zip's URL, the component folder's URL or the single MSI's URL; `To` the cached zip or the `install_cache\<code>` folder | rpyenv downloads several files per version. |
| D-86 | Windows | `install` (default packages) | (no default-packages plugin) | After each new install, if `<root>\default-packages` exists, runs `<prefix>\python.exe -m pip install -r` on it; a failure prints `` pyenv: error installing packages from  `<file>' `` and the install still succeeds | Spec §9.3: built in on both OSes. |
```

Row **D-57** hides `.tmp-*`/`.old-*` on Linux. Change its OS cell to `both`, and add to its upstream cell: on Windows, pyenv-win lists those folders because they pass `IsVersion`.

- [ ] **Step 2: Deliver `M2b`.** In `parity/allowlist.py`, `DELIVERED = ("M1", "M2a", "M2b")`.

In `parity/expected/pyenv-win.txt`, remove the 9 entries tagged `M2b`: the four `test_pyenv_feature_install.py` entries and five `test_pyenv_feature_latest.py` entries.

The 3 `test_patched_venv_module` cases install 3.9.13, 3.10.11 and 3.11.3 over the network. They exercise the component MSIs, ensurepip, the zip with pip launchers, and the venv-launcher copies.

- [ ] **Step 3: Add the Windows differential cases** to `parity/diff_cases.py`, after the M2a block:

```python
    # M2b (Windows): install, update, uninstall, latest. The fixture root holds pyenv-win's
    # own .versions_cache.xml (diff.py copies it), which both tools read.
    Case("install --help (Windows)", ("install", "--help"), os="Windows"),
    Case("help install (Windows)", ("help", "install"), os="Windows"),
    Case("install --list (Windows)", ("install", "--list"), os="Windows"),
    Case("install an unknown version (Windows)", ("install", "9.9.9"), os="Windows"),
    Case("install with --32only and --64only", ("install", "--32only", "--64only", "3.12.1"), os="Windows"),
    Case("install -r with --32only", ("install", "-r", "--32only", "3.12.1"), os="Windows"),
    Case("install -r -a", ("install", "-r", "-a"), os="Windows"),
    Case("install the selected version, already installed", ("install",), os="Windows"),
    Case("install -c with no install_cache", ("install", "-c"), os="Windows", allow=("D-77",)),
    Case("update --help", ("update", "--help"), os="Windows"),
    Case("uninstall --help (Windows)", ("uninstall", "--help"), os="Windows"),
    Case("uninstall with no arguments (Windows)", ("uninstall",), os="Windows"),
    Case("uninstall an unrecognized name", ("uninstall", "bad!name"), os="Windows"),
    Case("uninstall a missing version (Windows)", ("uninstall", "9.9"), os="Windows"),
    Case("uninstall two installed versions", ("uninstall", "{v0}", "{v1}"), os="Windows", allow=("D-80",)),
    Case("latest --help (Windows)", ("latest", "--help"), os="Windows"),
    Case("latest with no arguments (Windows)", ("latest",), os="Windows"),
    Case("latest 3 (Windows)", ("latest", "3"), os="Windows"),
    Case("latest -k 3.12 (Windows)", ("latest", "-k", "3.12"), os="Windows"),
    Case("latest -k 1 (Windows)", ("latest", "-k", "1"), os="Windows"),
    Case("latest -q 1 (Windows)", ("latest", "-q", "1"), os="Windows"),
    Case("versions with a staging folder (Windows)", ("versions",), os="Windows",
         files=(("root/versions/.tmp-3.12.1/python.exe", ""),), allow=("D-57",)),
```

Every case that names no row must be byte-identical with pyenv-win.
- **If one isn't:** first check whether rpyenv is wrong, and fix it in the command's code with a test.
- **If the difference is intended:** add it to an existing row's text, or add a row, and list it in the case.
- **Never** run a case that downloads. `install` of a version not yet installed would make pyenv-win download over the network, so no such case exists.

- [ ] **Step 4: Cite the rows that tests cover.** `parity/coverage.py` counts a row as covered by a differential case or a `// allowlist D-NN` comment above a Rust test. Add citations as follows:

| Row | Test |
|---|---|
| D-73 | `install_winpkg.rs`: `resolve_picks_the_zip_the_components_or_the_single_msi`, `a_signed_msi_installs_exactly_its_files_and_is_cached`, `components_skip_debug_path_launcher_pip_and_free_threaded` |
| D-74 | `cli_install_win.rs`: `a_failure_stops_the_run_exits_1_and_leaves_no_version` |
| D-75 | `install_txn.rs`: `a_pyenv_win_reinstall_carries_scripts_and_site_packages_and_recovers`; `install_winpkg.rs`: `a_tampered_msi_is_refused_leaves_nothing_and_is_not_cached` |
| D-76 | `cli_install_win.rs`: `register_is_ignored_with_one_info_line` |
| D-78 | `install_winpkg.rs`: `an_unsigned_msi_installs_with_one_warning` |
| D-79 | `cli_update_win.rs`: all four tests |
| D-81 | `cli_uninstall_win.rs`: `all_asks_first_and_eof_or_no_keeps_everything` (already cited) |
| D-82 | `cli_install_win.rs`: `pypy_and_graalpy_codes_are_refused` |
| D-83 | `install_wincatalog.rs`: `a_missing_empty_or_broken_cache_is_reported_not_a_panic` |
| D-84 | `install_winpkg.rs`: `a_free_threaded_zip_gets_python_exe_from_python3_13t_exe` |
| D-85 | `install_winpkg.rs`: `a_signed_msi_installs_exactly_its_files_and_is_cached` |
| D-86 | `cli_install_win.rs`: `default_packages_run_after_a_new_install_and_a_failure_still_succeeds` (already cited) |

D-77 and D-80 are also covered by cases.

- [ ] **Step 5: Regenerate and review the goldens.** On Windows:
  1. Run `python parity/diff.py --update-golden` with the upstream path that `parity.yml` uses locally: `C:/tmp/pyenv-win-856ed5a/pyenv-win`. Check `parity/diff.py`'s argument names first.
  2. Review `git diff parity/golden/windows`. The only changes expected are:
     - the new cases' goldens;
     - `commands`, which now lists `install`, `latest`, `uninstall` and `update` (D-16).
  3. Run `python parity/diff.py …` without the flag. Expected: every case reads `same` or `allowed`, with no `differs`.

- [ ] **Step 6: Run the pyenv-win suite through the overlay** (`parity/pyenv_win_run.py`, as `parity.yml` runs it). It needs network access for `test_patched_venv_module`.
  - Expected: the 9 tests removed from the expected list pass, and nothing else changes.
  - Record the suite's wall-clock time.
  - If a `test_patched_venv_module` case fails because of rpyenv, fix rpyenv with a test.
  - If it fails only because the network is slow, report DONE_WITH_CONCERNS with the timing, so the controller can rule.

- [ ] **Step 7: Check everything.**
  - Windows: `python parity/coverage.py` reports every row covered; `python -m unittest discover -s parity -p "test_*.py"` passes; fmt, clippy and `cargo test --workspace` pass.
  - WSL: the Linux diff still reads `23 allowed, 36 same`. The staging row's OS change must not alter Linux results; if it does, explain why.
  - WSL: bats still reports 0 problems.

- [ ] **Step 8: Commit.**

```bash
git add docs/parity/allowlist.md parity crates/pyenv/tests
git commit -m "Wire M2b into parity: rows D-73 to D-86, Windows cases, expected lists and goldens"
```

---

### Task 11: Tier 2, real installs on Windows

**Files:**
- Create: `.github/workflows/install-windows.yml`, `ci/check_install_win.py`, `ci/test_check_install_win.py`
- Test: `python -m unittest ci/test_check_install_win.py`, plus real local installs, measured.

**Interfaces:**
- Consumes: the release `pyenv.exe` and `pyenv-shim.exe`.
- Produces: tier 2 of spec §12.5:
  - every PR: 3.10.11 (component MSIs) and 3.12.10 (index zip);
  - nightly and on demand, also: 2.7.18 (single MSI with MSZIP merge modules), 3.4.4 (Löwis, DSA/SHA-1), 3.5.2 (unsigned), 3.12.5 (the 3.11+ component gap), 3.13.0t (free-threaded) and 3.14.0 (newest zip).

The spec says one pinned version on every PR. Running two costs little with the download cache, and covers both package kinds; record this as a ruling. Downloads are cached by version and re-verified on every use (Decision 8).

- [ ] **Step 1: Write the checker's own unit tests first.** Create `ci/test_check_install_win.py`:

```python
"""Tests for ci/check_install_win.py's pure parts (plan M2b Task 11)."""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_install_win as c  # noqa: E402


class Parse(unittest.TestCase):
    def test_codes(self):
        self.assertEqual(c.parse("3.10.11"), (3, 10, False))
        self.assertEqual(c.parse("3.13.0t-win32"), (3, 13, True))
        self.assertEqual(c.parse("2.7.18-win32"), (2, 7, False))
        self.assertEqual(c.parse("3.12.0rc1-arm"), (3, 12, False))

    def test_modules_per_version(self):
        self.assertIn("tkinter", c.modules(3, 10))
        self.assertIn("lzma", c.modules(3, 10))
        self.assertIn("Tkinter", c.modules(2, 7))
        self.assertNotIn("lzma", c.modules(2, 7))
        self.assertEqual(c.modules(2, 5), ["zlib", "Tkinter"])

    def test_copies(self):
        self.assertEqual(c.copies(3, 12), ["python.exe", "python3.exe", "python312.exe", "python3.12.exe"])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run it and see it fail.** Run `python -m unittest ci/test_check_install_win.py`. Expected: `ModuleNotFoundError: check_install_win`.

- [ ] **Step 3: Write `ci/check_install_win.py`:**

```python
"""Checks one real `pyenv install` on Windows (spec §12.5 tier 2; plan M2b Task 11).

Usage: python ci/check_install_win.py <PYENV_ROOT> <version code>

Fails (exit 1) unless:
- the version is complete: no marker, no staging leftovers;
- its executables and their copies exist;
- its standard modules import;
- pip runs where the version has ensurepip;
- the `python` shim reaches it;
- a venv works (3.3+).
"""
import os
import re
import subprocess
import sys
import tempfile


def parse(code):
    """(major, minor, free-threaded) of a pyenv-win version code."""
    m = re.match(r"^(\d+)\.(\d+)(?:\.\d+)?(?:(?:a|b|c|rc)\d+)?(t)?(?:-win32|-arm)?$", code)
    if not m:
        raise SystemExit(f"not a version code: {code}")
    return int(m.group(1)), int(m.group(2)), bool(m.group(3))


def modules(x, y):
    if x == 2 and y < 6:
        return ["zlib", "Tkinter"]
    if x == 2:
        return ["zlib", "bz2", "ctypes", "ssl", "sqlite3", "Tkinter"]
    mods = ["zlib", "bz2", "ctypes", "ssl", "sqlite3", "tkinter"]
    if (x, y) >= (3, 3):
        mods += ["lzma", "venv"]
    return mods


def copies(x, y):
    return ["python.exe", f"python{x}.exe", f"python{x}{y}.exe", f"python{x}.{y}.exe"]


def run(cmd, env=None):
    p = subprocess.run(cmd, capture_output=True, text=True, env=env)
    return p.returncode, (p.stdout + p.stderr).strip()


def main(root, code):
    x, y, _ft = parse(code)
    versions = os.path.join(root, "versions")
    prefix = os.path.join(versions, code)
    fails = []
    if not os.path.isdir(prefix):
        raise SystemExit(f"FAIL: {prefix} does not exist")
    if os.path.exists(os.path.join(prefix, ".rpyenv-incomplete")):
        fails.append("the incomplete marker is still there")
    stray = [n for n in os.listdir(versions) if n.startswith((".tmp-", ".old-"))]
    if stray:
        fails.append(f"staging leftovers: {stray}")
    for name in copies(x, y):
        if not os.path.isfile(os.path.join(prefix, name)):
            fails.append(f"missing {name}")
    py = os.path.join(prefix, "python.exe")
    env = dict(os.environ, PYTHONNOUSERSITE="1")
    env.pop("PYTHONHOME", None)
    env.pop("PYTHONPATH", None)
    rc, out = run([py, "-E", "-s", "-c", "import " + ", ".join(modules(x, y))], env)
    if rc:
        fails.append(f"imports failed: {out}")
    has_pip = (x, y) >= (3, 4) or (x == 2 and os.path.isdir(os.path.join(prefix, "Lib", "ensurepip")))
    if has_pip:
        rc, out = run([os.path.join(prefix, "Scripts", "pip.exe"), "--version"], env)
        if rc:
            fails.append(f"Scripts\\pip.exe --version failed: {out}")
    shim = os.path.join(root, "shims", "python.exe")
    rc, out = run([shim, "-c", "import sys; print(sys.prefix)"], dict(env, PYENV_VERSION=code))
    if rc or os.path.normcase(os.path.normpath(out)) != os.path.normcase(os.path.normpath(prefix)):
        fails.append(f"the python shim gave rc {rc}: {out}")
    if (x, y) >= (3, 3):
        with tempfile.TemporaryDirectory() as d:
            venv = os.path.join(d, "venv")
            rc, out = run([py, "-E", "-s", "-m", "venv", venv], env)
            if rc:
                fails.append(f"venv failed: {out}")
            else:
                rc, out = run([os.path.join(venv, "Scripts", "python.exe"), "-c", "pass"], env)
                if rc:
                    fails.append(f"the venv's python failed: {out}")
    for f in fails:
        print(f"FAIL: {f}")
    print(f"{code}: {'OK' if not fails else f'{len(fails)} failure(s)'}")
    return 1 if fails else 0


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2]))
```

- [ ] **Step 4: Run the unit tests and see them pass.** Run `python -m unittest ci/test_check_install_win.py` on Windows and in WSL. Expected: 3 passed.

- [ ] **Step 5: Measure real installs locally.** This is the tier-2 dry run. Use a scratch root, **never** the real one.

```bash
cargo build --release --workspace
mkdir -p /c/tmp/m2b_t11_root
export PYENV_ROOT=C:/tmp/m2b_t11_root PYENV=C:/tmp/m2b_t11_root PYENV_HOME=C:/tmp/m2b_t11_root
./target/release/pyenv.exe update
for v in 3.10.11 3.12.10 2.7.18 3.4.4 3.5.2 3.12.5 3.13.0t 3.14.0; do
  time ./target/release/pyenv.exe install $v
  python ci/check_install_win.py C:/tmp/m2b_t11_root $v
done
```

- **Record in your report:** each install's output, its time and its check result.
- **Expected:**
  - 3.5.2 prints exactly one `[Warning]` line;
  - every check prints OK;
  - 2.7.18 extracts its MSZIP merge modules; the spike measured only extraction, not running.
- **When a version fails a check:**
  - Report DONE_WITH_CONCERNS with the evidence. Don't drop the version from the matrix.
  - Two causes are already suspected:
    - 2.7.18's `PrivateCRT` feature is Level 0, so an admin install skips its VC90 runtime, exactly as pyenv-win's `msiexec /a` does. Whether `python.exe` then starts depends on the host having that runtime.
    - 3.4.4 is similar.
  - The controller rules on each failure: fix it, or document it with an allowlist row if pyenv-win behaves the same.

Then `unset` the variables and delete `C:\tmp\m2b_t11_root` by literal path.

- [ ] **Step 6: Write the workflow.** Create `.github/workflows/install-windows.yml`. The `uses:` pins below are the ones `ci.yml` and `install-linux.yml` already use (read 2026-10-03); don't introduce new ones. Matrix values go through `env:`, never inline `${{ }}` in `run:`.

```yaml
name: install (Windows, tier 2)

# Real installs of python.org packages (spec §12.5 tier 2; plan M2b Task 11). Every PR: one
# component-MSI version and one index-zip version. Nightly and on demand: every package kind.
on:
  pull_request:
  schedule:
    - cron: "41 3 * * *"
  workflow_dispatch:

permissions:
  contents: read

jobs:
  matrix:
    runs-on: ubuntu-latest
    outputs:
      versions: ${{ steps.m.outputs.versions }}
    steps:
      - id: m
        env:
          EVENT: ${{ github.event_name }}
        run: |
          if [ "$EVENT" = "pull_request" ]; then
            echo 'versions=["3.10.11","3.12.10"]' >> "$GITHUB_OUTPUT"
          else
            echo 'versions=["2.7.18","3.4.4","3.5.2","3.10.11","3.12.5","3.12.10","3.13.0t","3.14.0"]' >> "$GITHUB_OUTPUT"
          fi

  install:
    needs: matrix
    runs-on: windows-2025
    strategy:
      fail-fast: false
      matrix:
        version: ${{ fromJSON(needs.matrix.outputs.versions) }}
    env:
      VERSION: ${{ matrix.version }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - run: cargo build --release --workspace
      - uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        with:
          path: ${{ runner.temp }}\root\install_cache
          key: install-cache-windows-${{ matrix.version }}
      - name: Install and check
        shell: pwsh
        run: |
          $root = Join-Path $env:RUNNER_TEMP 'root'
          $env:PYENV_ROOT = $root; $env:PYENV = $root; $env:PYENV_HOME = $root
          $pyenv = 'target\release\pyenv.exe'
          & $pyenv update
          if ($LASTEXITCODE) { exit 1 }
          $start = Get-Date
          & $pyenv install $env:VERSION
          $code = $LASTEXITCODE
          $secs = [int]((Get-Date) - $start).TotalSeconds
          "| version | seconds | exit |`n|---|---|---|`n| $env:VERSION | $secs | $code |" >> $env:GITHUB_STEP_SUMMARY
          if ($code) { exit $code }
          python ci/check_install_win.py $root $env:VERSION
          if ($LASTEXITCODE) { exit 1 }
```

Validate that the YAML parses: `python -c "import yaml,sys; yaml.safe_load(open(sys.argv[1], encoding='utf-8'))" .github/workflows/install-windows.yml`. If PyYAML is missing, use the same check `install-linux.yml`'s task used.

- [ ] **Step 7: Check and commit.** Run the unit tests (both OSes) and `cargo test --workspace` (both OSes).

```bash
git add .github/workflows/install-windows.yml ci/check_install_win.py ci/test_check_install_win.py
git commit -m "Add tier-2 Windows installs: every PR one MSI and one zip version, nightly every package kind"
```
