# M2a Install Core and Linux Source Builds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `pyenv install`, `pyenv install --list`, `pyenv uninstall` and `pyenv latest` work on Linux. They build CPython from upstream python-build's definitions, with every download checked against its SHA-256, and a failure or Ctrl+C never leaves a partial version behind.

**Architecture:**
- A new `install` module tree in the `pyenv` binary holds the installer pieces:
  - HTTP download with checksum and cache;
  - tarball extraction;
  - an install transaction;
  - a python-build definitions interpreter;
  - the build executor;
  - a build pre-flight check.
- The shims and `rpyenv-core` gain no dependencies (spec §3).
- **Vendored data:** upstream's CPython definitions and patches are vendored under `crates/pyenv/python-build/`. A build script compiles them into the binary, and a scheduled workflow keeps the copy current.
- **Scope:** the new commands are registered for the Linux flavor only. Windows (`pyenv-win` flavor) gets its installer in M2b.

**Tech Stack:**
- Rust 1.96.
- ureq 3.4.2 (rustls, OS certificate store through `platform-verifier`, proxy from the environment).
- sha2 0.11, md-5 0.11, flate2 1.1, bzip2 0.6, lzma-rust2 0.21 (pure-Rust xz), tar 0.4, ctrlc 3.5.
- Python 3 stdlib for the CI scripts.
- All verified 2026-10-03: a spike downloaded `Python-3.12.10.tar.xz` over HTTPS, matched python-build's SHA-256 `07ab6974…46eaea`, and extracted it with modes intact on Windows and Linux.

**Spec:** `docs/specs/2026-09-27-rpyenv-design.md` (§3, §4, §9.2, §9.3, §12.5, §13, §14 item 2, §15.2). Upstream behavior: `docs/parity/pyenv-m2-reference.md` (Linux), with `docs/parity/pyenv-m1-reference.md` for `pyenv latest`.

## Global Constraints

- **Toolchain:** Rust `rust-version = "1.96"` (workspace). Edition 2021.
- **Dependencies (spec §3):**
  - `rpyenv-core` depends on "std, and platform crates only".
  - `pyenv-shim` and `pyenv-shimw` "must not link networking, TLS, or archive code". `ci/shim_deps.py` enforces it.
  - New crates go in `crates/pyenv/Cargo.toml` only.
- **Parity (spec §4):**
  - Match upstream in messages, streams, exit codes and formats.
  - Don't reproduce defects.
  - Every intentional difference gets an allowlist row in `docs/parity/allowlist.md` (next free row: D-55).
- **Atomic (spec §9.3):** "a failure or Ctrl+C never leaves a partial `versions/<ver>`."
- **Verified (spec §9.3):** "every download is checked against a published SHA-256 (from python.org's index, upstream `python-build`, or rpyenv's own catalog) before it is used."
- **Variables (spec §9.2):** respect upstream's `PYTHON_CONFIGURE_OPTS`, `PYTHON_CFLAGS`, `MAKE_OPTS`, `PYTHON_MAKE_OPTS` and `PYTHON_BUILD_MIRROR_URL`.
- **On failure (spec §9.2):** "keep the build directory and log, and print the last lines and the path, as upstream does."
- **Pre-flight (spec §9.2, §15.2):**
  - Check for a compiler, `make`, and key headers.
  - Return structured results: dependency, required or optional, and what exactly is missing.
  - Print the package names for the detected distribution.
- **Default packages (spec §9.3):** after a successful install, run `pip install -r $PYENV_ROOT/default-packages` in the new version if that file exists. A failure prints an error but the install still succeeds.
- **Tests (spec §12.5):** "Tier 1. Fake download server | Every PR, both OSes" and "Tier 3. Real Linux source build | Nightly, plus PRs that change Linux installer code (path filter)".
- **Line endings:** every Python `open()` gets `encoding="utf-8"`.
- **Commits:**
  - one-line `git commit -m` messages, with no attribution or trailer lines;
  - don't push.
- **WSL (implementers):**
  - Run WSL commands only from script files: `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/<x>.sh`.
  - Never put `$VARS` inside `wsl … -- bash -c "…"`.
  - Never wildcard-delete in `C:\tmp`.

## Decisions (rulings made while planning; each is recorded here so reviewers can check it)

1. **Definitions are vendored and compiled in.**
   - **What:** `crates/pyenv/python-build/` holds the 292 CPython definitions (names starting with a digit) and their `patches/<def>/` directories from pyenv `ab74141` (2.8.6), plus upstream's `LICENSE` and an `UPSTREAM` file naming the commit.
   - **Embedding:** `crates/pyenv/build.rs` embeds the definitions as text and the patches as one gzip-compressed tar.
   - **Plugin definitions:** `$PYENV_ROOT/plugins/*/share/python-build` directories are still read at run time, as upstream does.
   - **Keeping it current:** a weekly workflow (Task 10) compares the copy with the latest upstream release and opens a pull request.
   - **Why:** upstream ships its definitions with pyenv too; a new Python needs a pyenv upgrade either way.
2. **Only CPython in M2.**
   - `install --list` shows the vendored CPython names plus plugin definitions.
   - Other families (PyPy, conda, and the rest) arrive in M7.
   - Allowlist row.
3. **A definitions interpreter, not bash.**
   - It accepts exactly the constructs measured over all 292 definitions:
     - `install_package`, `install_git`;
     - `export`, with `${VAR:+…}` and `${VAR:-…}`;
     - `if has_tar_xz_support; then … else … fi`;
     - `source` of a sibling;
     - `prefer_openssl*`, `require_gcc`;
     - and 3.0.1's Darwin-only `echo`/`colorize` block.
   - Anything else is an error that names the line.
   - Allowlist row for definitions using other bash.
4. **Atomicity on Linux.** CPython's prefix is compiled in (rpath, shebangs), so the build can't run under a temporary name. The install transaction is:
   1. Build with the final `--prefix`.
   2. `make install DESTDIR=<versions>/.tmp-<name>`.
   3. Move the staged `<prefix>` tree into place. An existing version, under `-f` or a "yes" at the prompt, is first renamed to `versions/.old-<name>`.
   4. Run the post-install steps in place: symlinks, module checks, `ensurepip`, gdb, later packages.
   5. **Success:** delete `.old-<name>` and the marker file `versions/<name>/.rpyenv-incomplete`.
   6. **Failure or Ctrl+C:** remove `versions/<name>` and put `.old-<name>` back.

   - **A killed process** (SIGKILL, power loss) can leave a version that still has the marker. `pyenv install` then treats it as not installed and replaces it.
   - **Staging names:** `pyenv versions`, rehash and `latest` skip rpyenv's own `.tmp-*` and `.old-*` names on the Linux flavor. Upstream lists dot-directories (`shopt -s dotglob` in `pyenv-versions`, probed 2026-10-03), so only these two prefixes are hidden, keeping the difference as small as possible; allowlist row.
5. **Upstream defects not reproduced**, each with an allowlist row (spec §4):
   - **`PREFIX_EXISTS` leak:** after one existing prefix, later failures keep partial prefixes.
   - **Not-found exit code:** "definition not found" exits 1 instead of 2 when the directory pre-existed. rpyenv always exits 2.
   - **Ctrl+C exit code:** Ctrl+C exits 1 instead of 130 over an existing prefix. rpyenv always exits 130.
   - **`-f` over an existing prefix:** upstream builds into the existing tree. rpyenv swaps it (Decision 4).
   - **`patch` output:** upstream prints it on stdout. rpyenv sends it to the log.
   - **Temp files:** upstream leaves `python-patch.*` files in `TMPDIR`. rpyenv removes them.
   - **The `:latest` hook:** rpyenv matches at a version boundary, drops alpha releases, sorts like `pyenv latest`, and reports "no match" itself (spec §9.3 as amended).
   - **default-packages:** rpyenv runs pip in the installed directory, alias included.
6. **Downloads are in-process** (ureq), not curl/wget/aria2c.
   - `PYTHON_BUILD_HTTP_CLIENT`, `PYTHON_BUILD_CURL_OPTS`, `PYTHON_BUILD_WGET_OPTS` and `PYTHON_BUILD_ARIA2_OPTS` are ignored.
   - `-4`/`-6` are accepted and ignored.
   - Proxies come from `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`/`NO_PROXY`.
   - Up to 3 attempts for connection errors, timeouts and HTTP 5xx. A 4xx error or a checksum mismatch is final.
   - Allowlist row.
7. **Checksums.**
   - A 64-hex fragment is SHA-256.
   - A 32-hex fragment (MD5) or a missing fragment is refused, except for `install_git`: "rpyenv requires a SHA-256 checksum for <url>". This follows spec §9.3.
   - All 1007 checksummed `install_*` lines in the vendored definitions use SHA-256 (measured in the reference). Only plugin or hand-written definitions can hit the refusal.
   - **No `get-pip.py` fallback.** After a failed `ensurepip`, upstream downloads `get-pip.py`, which has no published
     checksum. rpyenv ends with `error: failed to install pip via ensurepip` and BUILD FAILED instead.
   - Allowlist row.
8. **Pre-flight policy.**
   - **Refuses before downloading** when missing: the C compiler, `make`, `patch` (only when the definition has patches), and the headers whose modules upstream's `verify_py*` makes fatal (OpenSSL and zlib).
   - **Warns** for the modules upstream only warns about (bz2, readline/libedit, sqlite3, ctypes/libffi, curses, lzma, tkinter), each with the distribution's package names.
   - `RPYENV_SKIP_PREFLIGHT=1` skips the check; the refusal message says so.
   - This is M2's "system" route (spec §15.2: "fail and print the exact install command"). The stricter "Required" list in §15.2 belongs to M8.
   - Spec §13 gains `RPYENV_SKIP_PREFLIGHT`. Allowlist row, since upstream has no pre-flight.
9. **Messages that name rpyenv.**
   - `pyenv install --version` prints `python-build 2.8.6 (rpyenv <version>)`.
   - BUILD FAILED reads `(<os> using rpyenv <version>)`.
   - Allowlist row.
10. **Milestone tags.**
    - `parity/allowlist.py` learns the tags `M2a` and `M2b`.
    - This plan adds `M2a` to `DELIVERED`.
    - Expected failures that wait for the Windows installer are re-tagged `M2b`.

## Review Focus

The five input classes most likely to bite a user that no task's main tests cover. Each one's test lives in the task named.

1. **A hostile or broken tarball.**
   - Entries with `..`, absolute paths, or symlinks pointing outside the build directory must be refused, not written outside it.
   - An archive without a top directory must still be renamed to the package name.
   - Test in Task 2.
2. **`PYENV_ROOT` on a different filesystem from `TMPDIR`.**
   - Staging lives under `versions/`, so the final rename never crosses filesystems.
   - The build directory may be anywhere.
   - Test in Task 2: the transaction's rename stays inside `versions/`.
3. **Ctrl+C during the download, during `make`, and during `ensurepip`.**
   - Exit 130.
   - No `versions/<name>`, `.tmp-<name>` or `.old-<name>` left behind.
   - A pre-existing version restored.
   - Test in Task 5 (Unix, with SIGINT sent to the process group).
4. **A name with a space or non-ASCII letter in `PYENV_ROOT`.**
   - Every fixture path already has `py env ñ`.
   - Configure, make, the rpath and the shebangs must survive it.
   - Test in Task 5: a fake build under the fixture root.
5. **An install while another `pyenv install` of the same name runs.**
   - The second gets a clear error instead of corrupting the first.
   - A lock: `versions/.lock-<name>`, created with `create_new`.
   - Test in Task 2.

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/pyenv/Cargo.toml` | new dependencies, build script | 1, 3 |
| `crates/pyenv/src/install/mod.rs` | module list, `InstallError`, the interrupt flag | 1 |
| `crates/pyenv/src/install/checksum.rs` | parse `#<hex>` fragments; verify a file | 1 |
| `crates/pyenv/src/install/fetch.rs` | HTTP agent, mirror rules, cache, retries | 1 |
| `crates/pyenv/tests/common/server.rs` | tier-1 local HTTP server for tests | 1 |
| `crates/pyenv/src/install/archive.rs` | safe tar.gz/bz2/xz extraction, top-dir rename | 2 |
| `crates/pyenv/src/install/txn.rs` | lock, staging, swap, rollback, marker | 2 |
| `crates/rpyenv-core/src/installed.rs` | skip `.tmp-*`/`.old-*` staging names (Linux flavor) | 2 |
| `crates/pyenv/python-build/**` | vendored definitions, patches, LICENSE, UPSTREAM | 3 |
| `crates/pyenv/build.rs` | embed definitions and patches | 3 |
| `crates/pyenv/src/install/defs.rs` | definition lookup, listing order, DSL interpreter | 3 |
| `ci/sync_python_build.py`, `ci/test_sync_python_build.py` | vendored-copy comparison and update | 3 |
| `crates/pyenv/src/commands/latest.rs` | `pyenv latest` | 4 |
| `crates/pyenv/src/install/log.rs` | build log, verbose tee, BUILD FAILED block | 5 |
| `crates/pyenv/src/install/builder.rs` | python-build's environment and steps (Unix) | 5 |
| `crates/pyenv/src/install/verify.rs` | `verify_pyXY` module tables | 5 |
| `crates/pyenv/tests/common/fakebuild.rs` | fake CPython source tarballs for tests | 5 |
| `crates/pyenv/src/install/preflight.rs` | dependency probe and distro package names | 6 |
| `crates/pyenv/src/commands/install.rs` | `pyenv install` | 7 |
| `crates/pyenv/src/install/default_packages.rs` | built-in default-packages | 7 |
| `crates/pyenv/src/commands/uninstall.rs` | `pyenv uninstall` | 8 |
| `parity/*`, `docs/parity/allowlist.md`, goldens | parity wiring | 9 |
| `.github/workflows/install-linux.yml`, `.github/workflows/sync-python-build.yml` | tier 3 and sync | 10 |

---
### Task 1: Downloads with checksum, mirror, cache and retries; the tier-1 test server

**Files:**
- Modify: `crates/pyenv/Cargo.toml`, `crates/pyenv/src/lib.rs`
- Create: `crates/pyenv/src/install/mod.rs`, `crates/pyenv/src/install/checksum.rs`, `crates/pyenv/src/install/fetch.rs`
- Create: `crates/pyenv/tests/common/server.rs`, `crates/pyenv/tests/install_fetch.rs`
- Modify: `crates/pyenv/tests/common/mod.rs` (add `pub mod server;`)

**Interfaces:**
- Produces:
  - `pyenv::install::{watch_interrupt() -> (), interrupted() -> bool, InstallError}`
  - `pyenv::install::checksum::{split_url(&str) -> (&str, Option<&str>), sha256_of_fragment(url: &str, fragment: Option<&str>) -> Result<String, String>, sha256_file(&Path) -> io::Result<String>}`
  - `pyenv::install::fetch::Fetcher::{from_env(env: &dyn Fn(&str) -> Option<String>, cache: Option<PathBuf>) -> Fetcher, fetch(&self, req: &FetchRequest, log: &mut dyn Write, say: &mut dyn FnMut(&str)) -> Result<PathBuf, InstallError>}`
  - `FetchRequest { file_name: String, url: String, sha256: String, dest_dir: PathBuf }`
  - Test helper `common::server::{start(routes: Vec<(&str, Vec<Reply>)>) -> Server, Reply, Server::{url(&self, path) -> String, hits(&self, path) -> usize}}`

- [ ] **Step 1: Add the dependencies**

In `crates/pyenv/Cargo.toml`, replace the `[dependencies]` table with:

```toml
[dependencies]
rpyenv-core.workspace = true
# Installer (spec §3: the CLI may link HTTP/TLS and archive code; the shims may not).
ureq = { version = "3.4.2", default-features = false, features = ["rustls", "platform-verifier"] }
sha2 = "0.11.0"
ctrlc = "3.5.2"
```

Add `pub mod install;` to `crates/pyenv/src/lib.rs`, next to the other `pub mod` lines.

- [ ] **Step 2: Write the failing unit tests for checksums**

Create `crates/pyenv/src/install/checksum.rs`:

```rust
//! python-build's `URL#<checksum>` convention, restricted to SHA-256 (spec §9.3, plan Decision 7).

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Splits `url#fragment` at the first `#`, as python-build does.
pub fn split_url(url: &str) -> (&str, Option<&str>) {
    match url.split_once('#') {
        Some((u, f)) => (u, Some(f)),
        None => (url, None),
    }
}

/// The SHA-256 named by a URL's fragment, lowercased. A missing fragment, an MD5 (32 hex
/// digits) or anything else is refused: every download is checked against a SHA-256.
pub fn sha256_of_fragment(url: &str, fragment: Option<&str>) -> Result<String, String> {
    match fragment {
        Some(f) if f.len() == 64 && f.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(f.to_ascii_lowercase())
        }
        _ => Err(format!("rpyenv requires a SHA-256 checksum for {url}")),
    }
}

/// Lowercase hex SHA-256 of a file's bytes.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn the_fragment_starts_after_the_first_hash() {
        assert_eq!(split_url("https://x/a.tgz#ab#cd"), ("https://x/a.tgz", Some("ab#cd")));
        assert_eq!(split_url("https://x/a.tgz"), ("https://x/a.tgz", None));
    }

    #[test]
    fn a_sha256_fragment_is_lowercased() {
        let upper = ABC.to_ascii_uppercase();
        assert_eq!(sha256_of_fragment("u", Some(&upper)), Ok(ABC.to_string()));
    }

    #[test]
    fn md5_missing_and_malformed_fragments_are_refused() {
        let msg = Err("rpyenv requires a SHA-256 checksum for https://x/a.tgz".to_string());
        assert_eq!(sha256_of_fragment("https://x/a.tgz", None), msg);
        assert_eq!(sha256_of_fragment("https://x/a.tgz", Some(&"0".repeat(32))), msg);
        assert_eq!(sha256_of_fragment("https://x/a.tgz", Some(&"g".repeat(64))), msg);
    }

    #[test]
    fn hashes_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, "abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), ABC);
    }
}
```

Create `crates/pyenv/src/install/mod.rs`:

```rust
//! The installer (spec §9): downloads, extraction, the install transaction, python-build
//! definitions and the Linux source build.

pub mod checksum;
pub mod fetch;

use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Records Ctrl+C instead of dying, so each step can stop and roll back (spec §9.3).
/// Child processes in the same process group get the signal themselves.
pub fn watch_interrupt() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::SeqCst));
    });
}

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// Ctrl+C: roll back, then exit 130.
    Interrupted,
    /// The reason is already on stderr or in the log; exit 1.
    Failed,
    /// One line for stderr; exit 1.
    Message(String),
}
```

- [ ] **Step 3: Run the unit tests and see them fail to build**

Run: `cargo test -p pyenv --lib install::checksum`
Expected: compile error, `file not found for module fetch`. Create an empty `crates/pyenv/src/install/fetch.rs`, then run again.
Expected: 4 passed. These are pure functions, so the code above is complete. The RED step for this task is Step 5.

- [ ] **Step 4: Write the tier-1 test server**

Create `crates/pyenv/tests/common/server.rs`:

```rust
//! A tiny HTTP/1.1 server on 127.0.0.1 for installer tests (spec §12.5 tier 1): each path
//! has a queue of replies, and the last one repeats. No network is used.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum Reply {
    /// 200 with this body (HEAD gets the headers only).
    Body(Vec<u8>),
    /// This status with an empty body.
    Status(u16),
    /// 200 announcing the full length but sending only the first half, then closing.
    Truncated(Vec<u8>),
}

pub struct Server {
    port: u16,
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn hits(&self, path: &str) -> usize {
        *self.hits.lock().unwrap().get(path).unwrap_or(&0)
    }
}

pub fn start(routes: Vec<(&str, Vec<Reply>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let queues: Arc<Mutex<HashMap<String, Vec<Reply>>>> = Arc::new(Mutex::new(
        routes.into_iter().map(|(p, r)| (p.to_string(), r)).collect(),
    ));
    let hits = Arc::new(Mutex::new(HashMap::new()));
    let hits2 = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let mut parts = request.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            *hits2.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
            let reply = {
                let mut q = queues.lock().unwrap();
                match q.get_mut(&path) {
                    Some(v) if v.len() > 1 => v.remove(0),
                    Some(v) if v.len() == 1 => v[0].clone(),
                    _ => Reply::Status(404),
                }
            };
            let head = method == "HEAD";
            let _ = match reply {
                Reply::Body(b) => {
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len());
                    if head { Ok(()) } else { stream.write_all(&b) }
                }
                Reply::Status(code) => write!(
                    stream,
                    "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ),
                Reply::Truncated(b) => {
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len());
                    if head { Ok(()) } else { stream.write_all(&b[..b.len() / 2]) }
                }
            };
        }
    });
    Server { port, hits }
}
```

Add `pub mod server;` to `crates/pyenv/tests/common/mod.rs`, after the `#![allow(dead_code)]` line.

- [ ] **Step 5: Write the failing fetch tests**

Create `crates/pyenv/tests/install_fetch.rs`:

```rust
//! Tier-1 download tests (spec §12.5) against a local server.

mod common;

use common::server::{start, Reply};
use pyenv::install::fetch::{FetchRequest, Fetcher};
use pyenv::install::InstallError;
use std::collections::HashMap;
use std::path::PathBuf;

const BODY: &[u8] = b"pretend tarball";

fn sum() -> String {
    // Computed rather than hard-coded, so the test pins the behavior, not a constant.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("b");
    std::fs::write(&p, BODY).unwrap();
    pyenv::install::checksum::sha256_file(&p).unwrap()
}

fn fetcher(vars: &[(&str, &str)], cache: Option<PathBuf>) -> Fetcher {
    let map: HashMap<String, String> =
        vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut f = Fetcher::from_env(&|k| map.get(k).cloned(), cache);
    f.retry_delay = std::time::Duration::ZERO;
    f
}

struct Got {
    result: Result<PathBuf, InstallError>,
    said: Vec<String>,
    log: String,
}

fn get(f: &Fetcher, url: String, sha: &str, dest: &std::path::Path) -> Got {
    let mut said = Vec::new();
    let mut log = Vec::new();
    let req = FetchRequest {
        file_name: "pkg-1.0.tar.gz".into(),
        url,
        sha256: sha.into(),
        dest_dir: dest.to_path_buf(),
    };
    let result = f.fetch(&req, &mut log, &mut |s: &str| said.push(s.to_string()));
    Got { result, said, log: String::from_utf8_lossy(&log).into_owned() }
}

const NO_MIRROR: &[(&str, &str)] = &[("PYTHON_BUILD_SKIP_MIRROR", "1")];

#[test]
fn downloads_verifies_and_says_what_it_fetched() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let d = tempfile::tempdir().unwrap();
    let g = get(&fetcher(NO_MIRROR, None), s.url("/pkg-1.0.tar.gz"), &sum(), d.path());
    let file = g.result.unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), BODY);
    assert_eq!(file, d.path().join("pkg-1.0.tar.gz"));
    assert_eq!(
        g.said,
        vec!["Downloading pkg-1.0.tar.gz...".to_string(), format!("-> {}", s.url("/pkg-1.0.tar.gz"))]
    );
}

#[test]
fn a_checksum_mismatch_fails_and_leaves_no_file() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let d = tempfile::tempdir().unwrap();
    let wrong = "0".repeat(64);
    let g = get(&fetcher(NO_MIRROR, None), s.url("/pkg-1.0.tar.gz"), &wrong, d.path());
    assert_eq!(g.result, Err(InstallError::Failed));
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0, "no file or .part left");
    assert!(g.log.contains("checksum mismatch: pkg-1.0.tar.gz (file is corrupt)\n"), "{}", g.log);
    assert!(g.log.contains(&format!("expected {wrong}, got {}\n", sum())), "{}", g.log);
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1, "a mismatch is not retried");
    assert!(g.said.contains(&"error: failed to download pkg-1.0.tar.gz".to_string()), "{:?}", g.said);
}

#[test]
fn a_server_error_is_retried() {
    let s = start(vec![(
        "/pkg-1.0.tar.gz",
        vec![Reply::Status(503), Reply::Truncated(BODY.to_vec()), Reply::Body(BODY.to_vec())],
    )]);
    let d = tempfile::tempdir().unwrap();
    let g = get(&fetcher(NO_MIRROR, None), s.url("/pkg-1.0.tar.gz"), &sum(), d.path());
    assert!(g.result.is_ok(), "{:?} {}", g.said, g.log);
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 3);
}

#[test]
fn not_found_is_final_and_reported() {
    let s = start(vec![]);
    let d = tempfile::tempdir().unwrap();
    let g = get(&fetcher(NO_MIRROR, None), s.url("/pkg-1.0.tar.gz"), &sum(), d.path());
    assert_eq!(g.result, Err(InstallError::Failed));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1);
    assert_eq!(g.said.last().unwrap(), "error: failed to download pkg-1.0.tar.gz");
}

#[test]
fn a_mirror_serves_the_file_by_checksum_and_falls_back_when_it_lacks_it() {
    let sha = sum();
    let mirror_path = format!("/m/{sha}");
    let s = start(vec![
        (mirror_path.as_str(), vec![Reply::Body(BODY.to_vec())]),
        ("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())]),
    ]);
    let d = tempfile::tempdir().unwrap();
    let base = s.url("/m/");
    let f = fetcher(&[("PYTHON_BUILD_MIRROR_URL", base.as_str())], None);
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sha, d.path());
    assert!(g.result.is_ok());
    assert_eq!(g.said[1], format!("-> {}", s.url(&mirror_path)));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 0);

    let other = "1".repeat(64);
    let d2 = tempfile::tempdir().unwrap();
    let g2 = get(&f, s.url("/pkg-1.0.tar.gz"), &other, d2.path());
    // The mirror lacks it (404 on HEAD), the original is fetched, and its checksum fails.
    assert_eq!(g2.result, Err(InstallError::Failed));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1);
}

#[test]
fn a_verified_cache_hit_prints_nothing_and_skips_the_network() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let cache = tempfile::tempdir().unwrap();
    let f = fetcher(NO_MIRROR, Some(cache.path().to_path_buf()));
    let d1 = tempfile::tempdir().unwrap();
    assert!(get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d1.path()).result.is_ok());
    assert!(cache.path().join("pkg-1.0.tar.gz").is_file());
    let d2 = tempfile::tempdir().unwrap();
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d2.path());
    assert!(g.result.is_ok());
    assert!(g.said.is_empty(), "{:?}", g.said);
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1);
}

#[test]
fn an_invalid_cached_file_is_downloaded_again() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(cache.path().join("pkg-1.0.tar.gz"), b"stale").unwrap();
    let d = tempfile::tempdir().unwrap();
    let g = get(&fetcher(NO_MIRROR, Some(cache.path().to_path_buf())), s.url("/pkg-1.0.tar.gz"), &sum(), d.path());
    assert!(g.result.is_ok());
    assert_eq!(std::fs::read(cache.path().join("pkg-1.0.tar.gz")).unwrap(), BODY);
}
```

- [ ] **Step 6: Run the fetch tests and see them fail**

Run: `cargo test -p pyenv --test install_fetch`
Expected: compile errors, because `Fetcher` and `FetchRequest` don't exist yet.

- [ ] **Step 7: Implement `fetch.rs`**

Create `crates/pyenv/src/install/fetch.rs`:

```rust
//! Downloads with python-build's mirror and cache rules (docs/parity/pyenv-m2-reference.md,
//! "Download" and "Cache"), done in-process (plan Decision 6).

use super::checksum::sha256_file;
use super::{interrupted, InstallError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Large enough for any CPython tarball.
const MAX_DOWNLOAD: u64 = 4 << 30;
const DEFAULT_MIRROR: &str = "https://pyenv.github.io/pythons";

pub struct FetchRequest {
    /// `<package name><extension>`, as python-build names the file.
    pub file_name: String,
    /// Without the `#` fragment.
    pub url: String,
    pub sha256: String,
    pub dest_dir: PathBuf,
}

struct Mirror {
    base: String,
    default: bool,
    /// `PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM`: the mirror mimics python.org's layout.
    skip_checksum: bool,
}

pub struct Fetcher {
    agent: ureq::Agent,
    mirror: Option<Mirror>,
    cache: Option<PathBuf>,
    /// Pause before attempts 2 and 3 (zero in tests).
    pub retry_delay: Duration,
}

enum Attempt {
    Ok,
    /// Worth another try: connection errors, timeouts, HTTP 5xx, short bodies.
    Transient(String),
    Final(String),
}

impl Fetcher {
    /// `env` reads a variable. `cache` is `PYTHON_BUILD_CACHE_PATH` as `pyenv install` resolved
    /// it (an existing directory, or None).
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>, cache: Option<PathBuf>) -> Fetcher {
        let set = |k: &str| env(k).filter(|v| !v.is_empty());
        let mirror = if set("PYTHON_BUILD_SKIP_MIRROR").is_some() {
            None
        } else {
            Some(match set("PYTHON_BUILD_MIRROR_URL") {
                Some(m) => Mirror {
                    base: m.strip_suffix('/').unwrap_or(&m).to_string(),
                    default: false,
                    skip_checksum: set("PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM").is_some(),
                },
                None => Mirror { base: DEFAULT_MIRROR.into(), default: true, skip_checksum: false },
            })
        };
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(ureq::Proxy::try_from_env())
            .timeout_connect(Some(Duration::from_secs(30)))
            .build()
            .into();
        Fetcher { agent, mirror, cache, retry_delay: Duration::from_secs(1) }
    }

    /// The mirror URL for `url`, if any (reference "Download", step 1).
    pub fn mirror_url(&self, url: &str, sha256: &str) -> Option<String> {
        let m = self.mirror.as_ref()?;
        if m.default && url.contains("/www.python.org/") {
            return None;
        }
        if m.skip_checksum {
            let rest = url.split_once("//www.python.org/ftp/python")?.1;
            return Some(format!("{}{rest}", m.base));
        }
        Some(format!("{}/{sha256}", m.base))
    }

    /// Puts a verified `<dest_dir>/<file_name>` in place, from the cache, the mirror or `url`.
    /// Progress lines go to `say` (stderr); checksum and HTTP detail go to `log`.
    pub fn fetch(
        &self,
        req: &FetchRequest,
        log: &mut dyn Write,
        say: &mut dyn FnMut(&str),
    ) -> Result<PathBuf, InstallError> {
        let dest = req.dest_dir.join(&req.file_name);
        if let Some(cache) = &self.cache {
            let cached = cache.join(&req.file_name);
            if sha256_file(&cached).ok().as_deref() == Some(req.sha256.as_str()) {
                std::fs::copy(&cached, &dest).map_err(|e| InstallError::Message(format!(
                    "pyenv: cannot copy {} from the cache: {e}", req.file_name
                )))?;
                return Ok(dest);
            }
        }
        say(&format!("Downloading {}...", req.file_name));
        let mut fetched = false;
        if let Some(m) = self.mirror_url(&req.url, &req.sha256) {
            if self.agent.head(&m).call().is_ok() {
                say(&format!("-> {m}"));
                fetched = self.download(&m, &dest, &req.sha256, &req.file_name, log)?;
            } else {
                let _ = writeln!(log, "mirror HEAD failed: {m}");
            }
        }
        if !fetched {
            say(&format!("-> {}", req.url));
            if !self.download(&req.url, &dest, &req.sha256, &req.file_name, log)? {
                say(&format!("error: failed to download {}", req.file_name));
                return Err(InstallError::Failed);
            }
        }
        if let Some(cache) = &self.cache {
            let _ = std::fs::copy(&dest, cache.join(&req.file_name));
        }
        Ok(dest)
    }

    /// Up to 3 attempts. Ok(false) when the file couldn't be fetched or didn't verify.
    fn download(
        &self,
        url: &str,
        dest: &Path,
        sha256: &str,
        file_name: &str,
        log: &mut dyn Write,
    ) -> Result<bool, InstallError> {
        let part = dest.with_file_name(format!("{file_name}.part"));
        for attempt in 1..=3 {
            if attempt > 1 {
                std::thread::sleep(self.retry_delay * (attempt - 1));
            }
            let outcome = self.attempt(url, &part);
            let _ = std::fs::remove_file(dest);
            if interrupted() {
                let _ = std::fs::remove_file(&part);
                return Err(InstallError::Interrupted);
            }
            match outcome {
                Attempt::Ok => {
                    let got = sha256_file(&part).unwrap_or_default();
                    if got == sha256 {
                        std::fs::rename(&part, dest).map_err(|e| {
                            InstallError::Message(format!("pyenv: cannot write {}: {e}", dest.display()))
                        })?;
                        return Ok(true);
                    }
                    let _ = std::fs::remove_file(&part);
                    let _ = write!(
                        log,
                        "\nchecksum mismatch: {file_name} (file is corrupt)\nexpected {sha256}, got {got}\n\n"
                    );
                    return Ok(false);
                }
                Attempt::Transient(e) => {
                    let _ = writeln!(log, "{url}: {e} (attempt {attempt} of 3)");
                }
                Attempt::Final(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = writeln!(log, "{url}: {e}");
                    return Ok(false);
                }
            }
        }
        let _ = std::fs::remove_file(&part);
        Ok(false)
    }

    fn attempt(&self, url: &str, part: &Path) -> Attempt {
        if let Some(path) = url.strip_prefix("file://") {
            return match std::fs::copy(path, part) {
                Ok(_) => Attempt::Ok,
                Err(e) => Attempt::Final(e.to_string()),
            };
        }
        let mut resp = match self.agent.get(url).call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(code)) if code >= 500 => {
                return Attempt::Transient(format!("HTTP {code}"))
            }
            Err(ureq::Error::StatusCode(code)) => return Attempt::Final(format!("HTTP {code}")),
            Err(e) => return Attempt::Transient(e.to_string()),
        };
        let Ok(mut out) = std::fs::File::create(part) else {
            return Attempt::Final(format!("cannot create {}", part.display()));
        };
        let mut reader = resp.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            if interrupted() {
                return Attempt::Final("interrupted".into());
            }
            match reader.read(&mut buf) {
                Ok(0) => return Attempt::Ok,
                Ok(n) => {
                    if let Err(e) = out.write_all(&buf[..n]) {
                        return Attempt::Final(e.to_string());
                    }
                }
                Err(e) => return Attempt::Transient(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(vars: &[(&str, &str)]) -> Fetcher {
        let vars: Vec<(String, String)> = vars.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        Fetcher::from_env(&|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()), None)
    }

    #[test]
    fn the_default_mirror_skips_python_org() {
        let f = with(&[]);
        assert_eq!(f.mirror_url("https://www.python.org/ftp/python/3.12.0/P.tgz", "ab"), None);
        assert_eq!(
            f.mirror_url("https://ftpmirror.gnu.org/r.tgz", "ab"),
            Some("https://pyenv.github.io/pythons/ab".into())
        );
    }

    #[test]
    fn a_custom_mirror_is_used_for_python_org_too_and_loses_one_trailing_slash() {
        let f = with(&[("PYTHON_BUILD_MIRROR_URL", "https://m.example/x/")]);
        assert_eq!(
            f.mirror_url("https://www.python.org/ftp/python/3.12.0/P.tgz", "ab"),
            Some("https://m.example/x/ab".into())
        );
    }

    #[test]
    fn skip_checksum_mirrors_the_python_org_layout() {
        let f = with(&[
            ("PYTHON_BUILD_MIRROR_URL", "https://m.example/py"),
            ("PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM", "1"),
        ]);
        assert_eq!(
            f.mirror_url("https://www.python.org/ftp/python/3.12.0/P.tgz", "ab"),
            Some("https://m.example/py/3.12.0/P.tgz".into())
        );
    }

    #[test]
    fn skip_mirror_disables_it() {
        assert_eq!(with(&[("PYTHON_BUILD_SKIP_MIRROR", "1")]).mirror_url("https://a/b", "ab"), None);
    }
}
```

- [ ] **Step 8: Run all the Task 1 tests and see them pass**

Run: `cargo test -p pyenv --lib install::` and `cargo test -p pyenv --test install_fetch`
Expected: 8 unit tests passed and 7 integration tests passed, on Windows and in WSL. Fetching is cross-platform, and M2b reuses it.

- [ ] **Step 9: Check that a test can fail**

Temporarily change `if code >= 500` to `if code >= 600` in `attempt`, then run `cargo test -p pyenv --test install_fetch a_server_error_is_retried`.
Expected: FAIL, with `hits == 1`. Restore the line.

- [ ] **Step 10: Lint, the shim guard, and commit**

Run:
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python ci/shim_deps.py`, which must print no unexpected crates for either shim
- `python -m unittest discover -s parity -p "test_*.py"`

Then commit:

```bash
git add crates/pyenv Cargo.lock
git commit -m "Download installer files with python-build's mirror and cache rules, checking each against its SHA-256"
```

---

### Task 2: Safe extraction, the install transaction, and hidden staging names

**Files:**
- Modify: `crates/pyenv/Cargo.toml` (add `flate2`, `bzip2`, `lzma-rust2`, `tar`)
- Create: `crates/pyenv/src/install/archive.rs`, `crates/pyenv/src/install/txn.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod archive; pub mod txn;`)
- Modify: `crates/rpyenv-core/src/installed.rs` (skip `.tmp-*`/`.old-*` on the Linux flavor)
- Create: `crates/pyenv/tests/install_txn.rs`
- Modify: `crates/pyenv/tests/cli_prefix_versions.rs` (one test)

**Interfaces:**
- Produces:
  - `archive::{Kind::{Gz, Bz2, Xz}, Kind::of_url(&str) -> Kind, Kind::ext(self) -> &'static str, extract(archive: &Path, kind: Kind, build_dir: &Path, name: &str) -> Result<PathBuf, String>}`. The returned path is `build_dir/<name>`.
  - `txn::{Txn::begin(versions: &Path, name: &str) -> Result<Txn, String>, Txn::stage_dir(&self) -> &Path, Txn::target(&self) -> PathBuf, Txn::place(&mut self, staged_prefix: &Path) -> io::Result<()>, Txn::placed(&self) -> bool, Txn::commit(self) -> io::Result<()>, MARKER: &str, is_complete(dir: &Path) -> bool}`
  - Dropping a `Txn` without `commit` rolls back.

- [ ] **Step 1: Add the archive crates**

Append to `[dependencies]` in `crates/pyenv/Cargo.toml`:

```toml
flate2 = "1.1.10"
bzip2 = "0.6.1"
lzma-rust2 = "0.21.0"
tar = "0.4.46"
```

- [ ] **Step 2: Write the failing tests**

Create `crates/pyenv/tests/install_txn.rs`:

```rust
//! Extraction safety (review focus 1) and the install transaction (spec §9.3, review focus
//! 2 and 5).

use pyenv::install::archive::{extract, Kind};
use pyenv::install::txn::{is_complete, Txn, MARKER};
use std::path::Path;

fn tgz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (name, data) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        // Raw name bytes: `append_data` would refuse `..` before the extractor can.
        let raw = &mut h.as_old_mut().name;
        raw[..name.len()].copy_from_slice(name.as_bytes());
        h.set_cksum();
        b.append(&h, *data).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

fn tgz_with_symlink(link: &str, target: &str, then_file: &str) -> Vec<u8> {
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    b.append_link(&mut h, link, target).unwrap();
    let mut f = tar::Header::new_gnu();
    f.set_size(1);
    f.set_mode(0o644);
    b.append_data(&mut f, then_file, &b"x"[..]).unwrap();
    b.into_inner().unwrap().finish().unwrap()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn the_top_directory_is_renamed_to_the_package_name() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz(&[("Other-1.0/configure", b"#!/bin/sh\n")]));
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    let out = extract(&a, Kind::Gz, &build, "Python-3.12.0").unwrap();
    assert_eq!(out, build.join("Python-3.12.0"));
    assert!(out.join("configure").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(out.join("configure")).unwrap().permissions().mode() & 0o111, 0o111);
    }
}

#[test]
fn an_archive_without_a_top_directory_still_gets_the_package_name() {
    let d = tempfile::tempdir().unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz(&[("configure", b"x"), ("README", b"y")]));
    let out = extract(&a, Kind::Gz, d.path(), "pkg-1.0").unwrap();
    assert!(out.join("configure").is_file() && out.join("README").is_file());
}

#[test]
fn parent_and_absolute_paths_are_refused() {
    for evil in ["../evil", "/tmp/rpyenv-evil-abs", "a/../../evil"] {
        let d = tempfile::tempdir().unwrap();
        let build = d.path().join("build");
        std::fs::create_dir(&build).unwrap();
        let a = write(d.path(), "a.tar.gz", &tgz(&[("ok/file", b"1"), (evil, b"2")]));
        let err = extract(&a, Kind::Gz, &build, "pkg").unwrap_err();
        assert!(err.contains("unsafe path"), "{evil}: {err}");
        assert!(!d.path().join("evil").exists() && !Path::new("/tmp/rpyenv-evil-abs").exists());
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_that_leaves_the_tree_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let build = d.path().join("build");
    std::fs::create_dir(&build).unwrap();
    let a = write(d.path(), "a.tar.gz", &tgz_with_symlink("top/out", "../../outside", "top/out/f"));
    let err = extract(&a, Kind::Gz, &build, "pkg").unwrap_err();
    assert!(err.contains("unsafe link"), "{err}");
    assert!(!d.path().join("outside").exists());
}

#[test]
fn staging_lives_inside_versions_and_commit_leaves_only_the_version() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    let mut t = Txn::begin(&versions, "3.12.0").unwrap();
    assert_eq!(t.stage_dir().parent().unwrap(), versions, "review focus 2: same filesystem");
    let staged = t.stage_dir().join("prefix");
    std::fs::create_dir_all(staged.join("bin")).unwrap();
    t.place(&staged).unwrap();
    assert!(t.target().join(MARKER).is_file());
    assert!(!is_complete(&t.target()));
    t.commit().unwrap();
    let names: Vec<String> = std::fs::read_dir(&versions).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["3.12.0".to_string()]);
    assert!(is_complete(&versions.join("3.12.0")));
}

#[test]
fn dropping_without_commit_restores_the_previous_version() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir_all(versions.join("3.12.0/bin")).unwrap();
    std::fs::write(versions.join("3.12.0/bin/old"), "old").unwrap();
    {
        let mut t = Txn::begin(&versions, "3.12.0").unwrap();
        let staged = t.stage_dir().join("p");
        std::fs::create_dir_all(staged.join("bin")).unwrap();
        std::fs::write(staged.join("bin/new"), "new").unwrap();
        t.place(&staged).unwrap();
        assert!(versions.join("3.12.0/bin/new").is_file());
    }
    assert!(versions.join("3.12.0/bin/old").is_file());
    assert!(!versions.join("3.12.0/bin/new").exists());
    let names: Vec<String> = std::fs::read_dir(&versions).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["3.12.0".to_string()], "no .tmp, .old or .lock left");
}

#[test]
fn dropping_a_fresh_install_removes_it() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    {
        let mut t = Txn::begin(&versions, "3.12.0").unwrap();
        let staged = t.stage_dir().join("p");
        std::fs::create_dir_all(&staged).unwrap();
        t.place(&staged).unwrap();
    }
    assert_eq!(std::fs::read_dir(&versions).unwrap().count(), 0);
}

#[test]
fn a_second_install_of_the_same_name_is_refused_while_the_first_runs() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    let _first = Txn::begin(&versions, "3.12.0").unwrap();
    let err = Txn::begin(&versions, "3.12.0").err().unwrap();
    assert_eq!(err, format!(
        "pyenv: another install of 3.12.0 is in progress ({})",
        versions.join(".lock-3.12.0").display()
    ));
    assert!(Txn::begin(&versions, "3.11.0").is_ok(), "other names are independent");
}

#[cfg(unix)]
#[test]
fn a_lock_left_by_a_dead_process_is_taken_over() {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    std::fs::create_dir(&versions).unwrap();
    // PID 2^22+1 is above Linux's pid_max default, so no such process exists.
    std::fs::write(versions.join(".lock-3.12.0"), "4194305\n").unwrap();
    assert!(Txn::begin(&versions, "3.12.0").is_ok());
}
```

Append to `crates/pyenv/tests/cli_prefix_versions.rs`:

```rust
/// The installer's staging names (`versions/.tmp-*`, `.old-*`) are invisible; other dot
/// directories still list, as upstream's `dotglob` lists them (plan Decision 4).
#[cfg(unix)]
#[test]
fn installer_staging_names_are_not_versions() {
    let f = Fixture::new();
    f.version("3.12.0").version(".tmp-3.13.0").version(".old-3.11.0").version(".hidden");
    let r = f.pyenv(&["versions", "--bare"]);
    let mut lines: Vec<&str> = r.stdout.lines().collect();
    lines.sort_unstable();
    assert_eq!((lines, r.code), (vec![".hidden", "3.12.0"], 0));
}
```

Use the file's existing `use common::Fixture;` import; add it if the file doesn't have one.

- [ ] **Step 3: Run them and see them fail**

Run: `cargo test -p pyenv --test install_txn` and `cargo test -p pyenv --test cli_prefix_versions installer_staging`
Expected: compile errors (no `archive` or `txn` module). The `versions` test fails because it prints `.tmp-3.13.0` and `.old-3.11.0` too.

- [ ] **Step 4: Implement `archive.rs`**

```rust
//! python-build's tarball extraction (reference "Download", step 5), refusing entries that
//! would land outside the build directory (review focus 1).

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Gz,
    Bz2,
    Xz,
}

impl Kind {
    /// python-build picks the type from the URL's end: `xz`, `bz2`, else gzip.
    pub fn of_url(url: &str) -> Kind {
        if url.ends_with("xz") {
            Kind::Xz
        } else if url.ends_with("bz2") {
            Kind::Bz2
        } else {
            Kind::Gz
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Kind::Gz => ".tar.gz",
            Kind::Bz2 => ".tar.bz2",
            Kind::Xz => ".tar.xz",
        }
    }
}

fn reader(archive: &Path, kind: Kind) -> std::io::Result<Box<dyn std::io::Read>> {
    let f = std::io::BufReader::new(std::fs::File::open(archive)?);
    Ok(match kind {
        Kind::Gz => Box::new(flate2::read::GzDecoder::new(f)),
        Kind::Bz2 => Box::new(bzip2::read::BzDecoder::new(f)),
        Kind::Xz => Box::new(lzma_rust2::XzReader::new(f, true)),
    })
}

fn safe_relative(p: &Path) -> bool {
    p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Whether a link at `entry` (relative) pointing to `target` stays inside the root.
fn link_stays_inside(entry: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth: i32 = entry.components().count() as i32 - 1;
    for c in target.components() {
        match c {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return false,
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

/// Extracts into `build_dir/<name>` and returns that path. An existing `build_dir/<name>`
/// is replaced.
pub fn extract(archive: &Path, kind: Kind, build_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let scratch = build_dir.join(format!(".extract-{name}"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let result = (|| {
        let mut ar = tar::Archive::new(reader(archive, kind).map_err(|e| e.to_string())?);
        ar.set_preserve_permissions(true);
        for entry in ar.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path().map_err(|e| e.to_string())?.into_owned();
            if !safe_relative(&path) {
                return Err(format!("unsafe path in {}: {}", archive.display(), path.display()));
            }
            if let Some(target) = entry.link_name().map_err(|e| e.to_string())? {
                let hard = entry.header().entry_type() == tar::EntryType::Link;
                let ok = if hard { safe_relative(&target) } else { link_stays_inside(&path, &target) };
                if !ok {
                    return Err(format!(
                        "unsafe link in {}: {} -> {}",
                        archive.display(),
                        path.display(),
                        target.display()
                    ));
                }
            }
            entry.unpack_in(&scratch).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(e);
    }
    let dest = build_dir.join(name);
    let _ = std::fs::remove_dir_all(&dest);
    let tops: Vec<PathBuf> = std::fs::read_dir(&scratch)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    let moved = match tops.as_slice() {
        [only] if only.is_dir() => std::fs::rename(only, &dest).and_then(|_| std::fs::remove_dir(&scratch)),
        _ => std::fs::rename(&scratch, &dest),
    };
    moved.map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest)
}
```

- [ ] **Step 5: Implement `txn.rs`**

```rust
//! The install transaction (spec §9.3, plan Decision 4): a per-name lock, a staging directory
//! inside `versions/`, a swap that keeps the previous version until commit, and rollback on
//! drop.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Present in `versions/<name>` from the move until commit: a version that still has it
/// was interrupted and isn't complete.
pub const MARKER: &str = ".rpyenv-incomplete";

/// "Installed" for `pyenv install`: upstream's `bin/` test, minus interrupted installs.
pub fn is_complete(dir: &Path) -> bool {
    dir.join("bin").is_dir() && !dir.join(MARKER).exists()
}

pub struct Txn {
    versions: PathBuf,
    name: String,
    lock: PathBuf,
    stage: PathBuf,
    old: Option<PathBuf>,
    placed: bool,
    done: bool,
}

fn holder_alive(lock: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(lock) else { return true };
    let Ok(pid) = text.trim().parse::<u32>() else { return true };
    if cfg!(target_os = "linux") {
        Path::new("/proc").join(pid.to_string()).exists()
    } else {
        true
    }
}

impl Txn {
    pub fn begin(versions: &Path, name: &str) -> Result<Txn, String> {
        std::fs::create_dir_all(versions).map_err(|e| format!("pyenv: cannot create {}: {e}", versions.display()))?;
        let lock = versions.join(format!(".lock-{name}"));
        let busy = || format!("pyenv: another install of {name} is in progress ({})", lock.display());
        let open = || std::fs::OpenOptions::new().write(true).create_new(true).open(&lock);
        let mut f = match open() {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if holder_alive(&lock) {
                    return Err(busy());
                }
                let _ = std::fs::remove_file(&lock);
                open().map_err(|_| busy())?
            }
            Err(e) => return Err(format!("pyenv: cannot create {}: {e}", lock.display())),
        };
        let _ = writeln!(f, "{}", std::process::id());
        let stage = versions.join(format!(".tmp-{name}"));
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage).map_err(|e| format!("pyenv: cannot create {}: {e}", stage.display()))?;
        Ok(Txn { versions: versions.to_path_buf(), name: name.to_string(), lock, stage, old: None, placed: false, done: false })
    }

    pub fn stage_dir(&self) -> &Path {
        &self.stage
    }

    pub fn target(&self) -> PathBuf {
        self.versions.join(&self.name)
    }

    pub fn placed(&self) -> bool {
        self.placed
    }

    /// Moves the staged prefix to `versions/<name>`, setting any existing version aside.
    pub fn place(&mut self, staged_prefix: &Path) -> std::io::Result<()> {
        let target = self.target();
        if target.symlink_metadata().is_ok() {
            let old = self.versions.join(format!(".old-{}", self.name));
            let _ = std::fs::remove_dir_all(&old);
            std::fs::rename(&target, &old)?;
            self.old = Some(old);
        }
        std::fs::rename(staged_prefix, &target)?;
        std::fs::write(target.join(MARKER), "")?;
        self.placed = true;
        Ok(())
    }

    pub fn commit(mut self) -> std::io::Result<()> {
        std::fs::remove_file(self.target().join(MARKER))?;
        if let Some(old) = self.old.take() {
            let _ = std::fs::remove_dir_all(&old);
        }
        let _ = std::fs::remove_dir_all(&self.stage);
        let _ = std::fs::remove_file(&self.lock);
        self.done = true;
        Ok(())
    }
}

impl Drop for Txn {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let target = self.target();
        if self.placed {
            let _ = std::fs::remove_dir_all(&target);
        }
        if let Some(old) = self.old.take() {
            let _ = std::fs::rename(&old, &target);
        }
        let _ = std::fs::remove_dir_all(&self.stage);
        let _ = std::fs::remove_file(&self.lock);
    }
}
```

`remove_dir_all` on a symlinked version removes only the link, which matches upstream's `rm -rf` of a symlink.

- [ ] **Step 6: Hide the staging names on the Linux flavor**

In `crates/rpyenv-core/src/installed.rs`, change `top_level` so the Linux flavor drops the installer's staging names before sorting:

```rust
pub fn top_level(versions_dir: &Path, flavor: Flavor) -> Vec<VersionEntry> {
    let mut names = subdirs(versions_dir);
    if flavor == Flavor::Pyenv {
        // The installer's staging names (plan Decision 4). Upstream lists other dot entries
        // (`dotglob`), so only these are hidden (allowlist row).
        names.retain(|n| !is_staging_name(n));
        sort_version_names(&mut names, &versions_dir.to_string_lossy());
    }
```

Add, in the same file:

```rust
/// `versions/.tmp-<name>` and `versions/.old-<name>`: the installer's staging (plan M2a, Decision 4).
pub fn is_staging_name(name: &str) -> bool {
    name.starts_with(".tmp-") || name.starts_with(".old-")
}
```

Then check every other caller of `subdirs` and `std::fs::read_dir(versions_dir)` in `rpyenv-core`. Run `grep -n "read_dir\|subdirs(" crates/rpyenv-core/src/*.rs`. Each one that lists versions on the Linux flavor must skip `is_staging_name` too, including `latest`'s candidate listing and the rehash snapshot. `shimset.rs:34` already skips every dot name for executables; leave it. Say in the report which call sites you changed.

- [ ] **Step 7: Run the tests and see them pass**

Run: `cargo test -p pyenv --test install_txn`, `cargo test -p pyenv --test cli_prefix_versions`, and `cargo test --workspace`
Expected: all pass on both OSes. The Unix-only tests are compiled out on Windows.

- [ ] **Step 8: Check that the tests can fail**

1. Temporarily make `link_stays_inside` return `true`. `a_symlink_that_leaves_the_tree_is_refused` must fail. Restore it.
2. Temporarily remove the `if let Some(old) = self.old.take() { let _ = std::fs::rename(&old, &target); }` block from `Drop`. `dropping_without_commit_restores_the_previous_version` must fail. Restore it.

- [ ] **Step 9: Lint and commit**

Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `python ci/shim_deps.py`.

```bash
git add crates Cargo.lock
git commit -m "Extract tarballs safely and install through a locked, staged transaction that rolls back"
```

---

### Task 3: Vendored definitions, the definitions interpreter, and the sync script

**Files:**
- Create: `ci/sync_python_build.py`, `ci/test_sync_python_build.py`
- Create, by running the sync script: `crates/pyenv/python-build/{share/<defs>, share/patches/<def>/…, LICENSE, UPSTREAM}`
- Modify: `.gitattributes` (add `crates/pyenv/python-build/** -text`)
- Create: `crates/pyenv/build.rs`
- Modify: `crates/pyenv/Cargo.toml` (`build = "build.rs"`, `[build-dependencies]`)
- Create: `crates/pyenv/src/install/defs.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod defs;`)
- Create: `crates/pyenv/tests/install_defs.rs`

**Interfaces:**
- Produces:
  - `defs::{names(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<String>` (with plugins), `known(env) -> Vec<String>` (without), `sort_versions(&mut Vec<String>), find(root: &Path, arg: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<Found>, Found { name: String, text: String, origin: Origin }, Origin::{Builtin, Dir(PathBuf), Path(PathBuf)}}`
  - `defs::parse(found: &Found, env: &dyn Fn(&str) -> Option<String>, sibling: &dyn Fn(&str) -> Option<Found>) -> Result<Definition, String>`
  - `Definition { packages: Vec<Package>, vars: Vec<(String, String)>, require_gcc: bool }`
  - `Package { name: String, fetch: Fetch, steps: Vec<String>, condition: Option<String> }`
  - `Fetch::{Tarball { url: String, fragment: Option<String> }, Git { url: String, reference: String }}`
  - `defs::patches_for(def: &str, package: &str) -> Vec<(String, Vec<u8>)>`: the built-in patches, sorted by file name.
  - `defs::UPSTREAM_VERSION: &str` (`"2.8.6"`).

- [ ] **Step 1: Write the sync script's failing test**

Create `ci/test_sync_python_build.py`:

```python
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sync_python_build as s  # noqa: E402


def tree(root, files):
    for rel, text in files.items():
        p = os.path.join(root, *rel.split("/"))
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(text)


class Sync(unittest.TestCase):
    def upstream(self, d, extra=None):
        files = {
            "LICENSE": "MIT\n",
            "libexec/pyenv---version": 'version="2.8.6"\n',
            "plugins/python-build/share/python-build/3.12.0": "install_package a\n",
            "plugins/python-build/share/python-build/3.12.0t": "source x\n",
            "plugins/python-build/share/python-build/pypy3.10-7.3.12": "pypy\n",
            "plugins/python-build/share/python-build/patches/3.12.0/Python-3.12.0/0001.patch": "p\r\n",
            "plugins/python-build/share/python-build/patches/pypy3.10-7.3.12/x.patch": "no\n",
        }
        files.update(extra or {})
        tree(d, files)

    def test_copies_cpython_definitions_and_their_patches_only(self):
        with tempfile.TemporaryDirectory() as up, tempfile.TemporaryDirectory() as dest:
            self.upstream(up)
            changes = s.sync(up, dest, "abc123", write=True)
            self.assertEqual(sorted(os.listdir(os.path.join(dest, "share"))), ["3.12.0", "3.12.0t", "patches"])
            self.assertEqual(os.listdir(os.path.join(dest, "share", "patches")), ["3.12.0"])
            with open(os.path.join(dest, "share", "patches", "3.12.0", "Python-3.12.0", "0001.patch"), "rb") as f:
                self.assertEqual(f.read(), b"p\r\n", "bytes are copied exactly")
            with open(os.path.join(dest, "UPSTREAM"), encoding="utf-8") as f:
                self.assertEqual(f.read(), "pyenv abc123 2.8.6\n")
            self.assertTrue(changes)
            self.assertEqual(s.sync(up, dest, "abc123", write=False), [], "a second run finds no change")

    def test_reports_added_changed_and_removed_files(self):
        with tempfile.TemporaryDirectory() as up, tempfile.TemporaryDirectory() as dest:
            self.upstream(up)
            s.sync(up, dest, "abc123", write=True)
            tree(up, {"plugins/python-build/share/python-build/3.12.1": "new\n",
                      "plugins/python-build/share/python-build/3.12.0": "changed\n"})
            os.remove(os.path.join(up, "plugins/python-build/share/python-build/3.12.0t"))
            self.assertEqual(
                s.sync(up, dest, "def456", write=False),
                ["changed share/3.12.0", "added share/3.12.1", "removed share/3.12.0t", "changed UPSTREAM"],
            )


if __name__ == "__main__":
    unittest.main()
```

Run: `python -m unittest discover -s ci -p "test_sync*.py"`
Expected: `ModuleNotFoundError: No module named 'sync_python_build'`.

- [ ] **Step 2: Implement the sync script**

Create `ci/sync_python_build.py`:

```python
"""Keeps crates/pyenv/python-build in step with upstream pyenv (plan M2a, Decision 1).

  python ci/sync_python_build.py --upstream <pyenv checkout> --commit <sha> [--write]

Copies the CPython definitions (file names starting with a digit) and their
`patches/<name>/` directories byte for byte, plus pyenv's LICENSE, and records the commit
and version in UPSTREAM. Without --write it only lists the differences and exits 1 if any.
"""
import argparse
import os
import re
import shutil
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEST = os.path.join(REPO, "crates", "pyenv", "python-build")


def wanted(upstream):
    """{relative path under DEST: bytes} for everything that should be vendored."""
    share = os.path.join(upstream, "plugins", "python-build", "share", "python-build")
    out = {}
    names = sorted(n for n in os.listdir(share) if n[:1].isdigit() and os.path.isfile(os.path.join(share, n)))
    for n in names:
        with open(os.path.join(share, n), "rb") as f:
            out["share/" + n] = f.read()
        pdir = os.path.join(share, "patches", n)
        for dirpath, _, files in os.walk(pdir):
            for fn in files:
                full = os.path.join(dirpath, fn)
                rel = os.path.relpath(full, share).replace(os.sep, "/")
                with open(full, "rb") as f:
                    out["share/" + rel] = f.read()
    with open(os.path.join(upstream, "LICENSE"), "rb") as f:
        out["LICENSE"] = f.read()
    return out


def version_of(upstream):
    with open(os.path.join(upstream, "libexec", "pyenv---version"), encoding="utf-8") as f:
        m = re.search(r'^version="([^"]+)"', f.read(), re.M)
    return m.group(1) if m else "unknown"


def present(dest):
    out = {}
    for dirpath, _, files in os.walk(dest):
        for fn in files:
            full = os.path.join(dirpath, fn)
            rel = os.path.relpath(full, dest).replace(os.sep, "/")
            with open(full, "rb") as f:
                out[rel] = f.read()
    return out


def sync(upstream, dest, commit, write):
    want = wanted(upstream)
    want["UPSTREAM"] = f"pyenv {commit} {version_of(upstream)}\n".encode("utf-8")
    have = present(dest) if os.path.isdir(dest) else {}
    changes = []
    for rel in sorted(set(want) | set(have), key=lambda r: (r == "UPSTREAM", r)):
        if rel not in have:
            changes.append(f"added {rel}")
        elif rel not in want:
            changes.append(f"removed {rel}")
        elif have[rel] != want[rel]:
            changes.append(f"changed {rel}")
    order = {"changed": 0, "added": 1, "removed": 2}
    changes.sort(key=lambda c: (c.endswith("UPSTREAM"), order[c.split()[0]], c))
    if write:
        if os.path.isdir(dest):
            shutil.rmtree(dest)
        for rel, data in want.items():
            p = os.path.join(dest, *rel.split("/"))
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "wb") as f:
                f.write(data)
    return changes


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--upstream", required=True)
    p.add_argument("--commit", required=True)
    p.add_argument("--write", action="store_true")
    a = p.parse_args(argv)
    changes = sync(a.upstream, DEST, a.commit, a.write)
    for c in changes:
        print(c)
    print(f"{len(changes)} difference(s)")
    return 0 if a.write or not changes else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

Run: `python -m unittest discover -s ci -p "test_sync*.py"`
Expected: 2 tests OK.

- [ ] **Step 3: Vendor pyenv ab74141**

1. Add `crates/pyenv/python-build/** -text` to `.gitattributes`, so patch bytes, including CRLF ones, are never converted.
2. Download the pinned tarball and run the script from Git Bash:

```bash
curl -fsSL -o /c/tmp/m2a_pyenv.tar.gz https://github.com/pyenv/pyenv/archive/ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2.tar.gz
mkdir -p /c/tmp/m2a_pyenv && tar -xzf /c/tmp/m2a_pyenv.tar.gz -C /c/tmp/m2a_pyenv
python ci/sync_python_build.py --upstream /c/tmp/m2a_pyenv/pyenv-ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2 --commit ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2 --write
```

3. Expected checks, all derived from the files rather than assumed:
   - `ls crates/pyenv/python-build/share | grep -c '^[0-9]'` prints `292`.
   - `ls crates/pyenv/python-build/share/patches | wc -l` prints `199`.
   - `UPSTREAM` reads `pyenv ab74141ab4bcd34aa9b11f363dd8ad4f6fc01cd2 2.8.6`.
   - Running the script again without `--write` prints `0 difference(s)` and exits 0.

   If the counts differ, report the numbers and the method instead of editing the script to match.

- [ ] **Step 4: Write the embedding build script**

In `crates/pyenv/Cargo.toml`, add `build = "build.rs"` under `[package]` and:

```toml
[build-dependencies]
flate2 = "1.1.10"
tar = "0.4.46"
```

Create `crates/pyenv/build.rs`:

```rust
//! Embeds the vendored python-build data (plan M2a, Decision 1): definitions as text, the
//! patches as one gzip-compressed tar.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let share = manifest.join("python-build").join("share");
    println!("cargo::rerun-if-changed=python-build");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let mut names: Vec<String> = fs::read_dir(&share)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut defs = String::from("pub static DEFINITIONS: &[(&str, &str)] = &[\n");
    for n in &names {
        defs.push_str(&format!("    ({n:?}, include_str!({:?})),\n", share.join(n)));
    }
    defs.push_str("];\n");
    fs::write(out.join("defs.rs"), defs).unwrap();

    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let mut b = tar::Builder::new(gz);
    let patches = share.join("patches");
    add_dir(&mut b, &patches, Path::new(""));
    let bytes = b.into_inner().unwrap().finish().unwrap();
    fs::File::create(out.join("patches.tar.gz")).unwrap().write_all(&bytes).unwrap();
}

fn add_dir(b: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>, dir: &Path, rel: &Path) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let r = rel.join(e.file_name());
        if p.is_dir() {
            add_dir(b, &p, &r);
        } else {
            b.append_path_with_name(&p, &r).unwrap();
        }
    }
}
```

- [ ] **Step 5: Write the failing interpreter tests**

Create `crates/pyenv/tests/install_defs.rs`:

```rust
//! python-build definitions: listing order, lookup and the interpreter (plan Decision 3).

use pyenv::install::defs::{self, Fetch, Found, Origin};
use std::path::Path;

fn no_env(_: &str) -> Option<String> {
    None
}

fn builtin(name: &str) -> Found {
    defs::find(Path::new("/nonexistent-root"), name, &no_env).unwrap()
}

fn parse_builtin(name: &str) -> defs::Definition {
    let root = Path::new("/nonexistent-root");
    defs::parse(&builtin(name), &no_env, &|n| defs::find(root, n, &no_env)).unwrap()
}

#[test]
fn every_vendored_definition_parses() {
    // Mechanical: all 292, so a construct the interpreter misses fails here, by name.
    let root = Path::new("/nonexistent-root");
    let names = defs::names(root, &no_env);
    assert_eq!(names.len(), 292);
    let mut failures = Vec::new();
    for n in &names {
        let found = defs::find(root, n, &no_env).unwrap();
        match defs::parse(&found, &no_env, &|s| defs::find(root, s, &no_env)) {
            Ok(d) => {
                let python = d.packages.iter().filter(|p| p.name.starts_with("Python-")).count();
                if python != 1 {
                    failures.push(format!("{n}: {python} Python packages"));
                }
            }
            Err(e) => failures.push(format!("{n}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_current_definition_takes_the_xz_branch_and_skips_nothing_on_its_own() {
    let d = parse_builtin("3.12.10");
    let py = d.packages.iter().find(|p| p.name == "Python-3.12.10").unwrap();
    match &py.fetch {
        Fetch::Tarball { url, fragment } => {
            assert_eq!(url, "https://www.python.org/ftp/python/3.12.10/Python-3.12.10.tar.xz");
            assert_eq!(fragment.as_deref(), Some("07ab697474595e06f06647417d3c7fa97ded07afc1a7e4454c5639919b46eaea"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(py.steps, ["standard", "verify_py312", "copy_python_gdb", "ensurepip"]);
    let openssl = d.packages.iter().find(|p| p.name.starts_with("openssl-")).unwrap();
    assert_eq!(openssl.condition.as_deref(), Some("has_broken_mac_openssl"));
}

#[test]
fn a_free_threaded_definition_sources_its_twin_with_the_flag_set() {
    let d = parse_builtin("3.13.3t");
    assert!(d.vars.contains(&("PYTHON_BUILD_FREE_THREADING".into(), "1".into())));
    assert!(d.packages.iter().any(|p| p.name == "Python-3.13.3"));
}

#[test]
fn cflags_exports_expand_like_bash() {
    let d = parse_builtin("3.8.20");
    assert!(d.vars.contains(&("PYTHON_CFLAGS".into(), "-DOPENSSL_NO_SSL3".into())), "{:?}", d.vars);
    let with_env = |k: &str| (k == "PYTHON_CFLAGS").then(|| "-O1".to_string());
    let root = Path::new("/nonexistent-root");
    let d2 = defs::parse(&builtin("2.7.18"), &with_env, &|n| defs::find(root, n, &no_env)).unwrap();
    assert!(d2.vars.contains(&("PYTHON_CFLAGS".into(), "-O1 -std=c99".into())), "{:?}", d2.vars);
}

#[test]
fn the_darwin_block_of_3_0_1_is_skipped_and_its_else_branch_kept() {
    let d = parse_builtin("3.0.1");
    let names: Vec<&str> = d.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["openssl-1.0.2k", "readline-8.0", "Python-3.0.1", "setuptools-1.4.2", "pip-1.3.1"]);
}

#[test]
fn the_continued_src_assignment_of_3_4_10_picks_xz() {
    let d = parse_builtin("3.4.10");
    let py = d.packages.iter().find(|p| p.name == "Python-3.4.10").unwrap();
    assert!(matches!(&py.fetch, Fetch::Tarball { url, .. } if url.ends_with("Python-3.4.10.tar.xz")));
}

#[test]
fn dev_definitions_clone_with_git() {
    let d = parse_builtin("3.14-dev");
    let py = d.packages.iter().find(|p| p.name == "Python-3.14-dev").unwrap();
    assert_eq!(py.fetch, Fetch::Git { url: "https://github.com/python/cpython".into(), reference: "3.14".into() });
}

#[test]
fn old_definitions_require_gcc() {
    assert!(parse_builtin("2.4.6").require_gcc);
    assert!(!parse_builtin("2.7.18").require_gcc);
}

#[test]
fn other_bash_is_refused_with_its_line() {
    let f = Found { name: "x".into(), text: "install_package a b\nfor i in 1; do :; done\n".into(), origin: Origin::Path("/x".into()) };
    let err = defs::parse(&f, &no_env, &|_| None).unwrap_err();
    assert_eq!(err, "x: line 2: rpyenv cannot interpret this definition line: for i in 1; do :; done");
}

#[test]
fn the_listing_sorts_like_python_build() {
    let mut v: Vec<String> = ["3.12.1", "3.12-dev", "3.12.0", "2.7-dev", "2.7", "3.13.0t", "3.13.0", "3.13-dev", "3.13t-dev"]
        .iter().map(|s| s.to_string()).collect();
    defs::sort_versions(&mut v);
    assert_eq!(v, ["2.7-dev", "2.7", "3.12.0", "3.12-dev", "3.12.1", "3.13.0", "3.13.0t", "3.13-dev", "3.13t-dev"]);
}

#[test]
fn plugin_definitions_are_listed_and_found_first() {
    let root = tempfile::tempdir().unwrap();
    let d = root.path().join("plugins/fake/share/python-build");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("3.12.99"), "install_package \"Python-3.12.99\" \"http://x/P.tgz#0\" standard\n").unwrap();
    std::fs::write(d.join("3.12.10"), "plugin copy\n").unwrap();
    let names = defs::names(root.path(), &no_env);
    assert!(names.contains(&"3.12.99".to_string()));
    assert_eq!(names.iter().filter(|n| *n == "3.12.10").count(), 1, "adjacent duplicates removed");
    let found = defs::find(root.path(), "3.12.10", &no_env).unwrap();
    assert_eq!(found.text, "plugin copy\n");
    assert!(matches!(found.origin, Origin::Dir(_)));
}

#[test]
fn a_definition_file_path_is_used_as_is() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("my-def");
    std::fs::write(&p, "require_gcc\n").unwrap();
    let found = defs::find(Path::new("/nonexistent-root"), p.to_str().unwrap(), &no_env).unwrap();
    assert_eq!(found.name, "my-def");
}

#[test]
fn built_in_patches_come_sorted() {
    let p = defs::patches_for("3.12.14", "Python-3.12.14");
    assert!(!p.is_empty());
    let names: Vec<&str> = p.iter().map(|(n, _)| n.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(defs::patches_for("3.12.14", "readline-8.2").is_empty());
}
```

Run: `cargo test -p pyenv --test install_defs`
Expected: compile errors, because `defs` doesn't exist.

- [ ] **Step 6: Implement `defs.rs`**

```rust
//! python-build definitions (docs/parity/pyenv-m2-reference.md, "The definitions DSL"):
//! where they are found, the order `--list` prints them in, and an interpreter for exactly
//! the constructs the CPython definitions use (plan Decision 3).

use std::io::Read;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/defs.rs"));
static PATCHES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/patches.tar.gz"));

/// The python-build release the vendored definitions come from (UPSTREAM).
pub const UPSTREAM_VERSION: &str = "2.8.6";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    /// A directory from `PYTHON_BUILD_DEFINITIONS` or a plugin.
    Dir(PathBuf),
    /// A definition file named on the command line.
    Path(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Found {
    /// The definition's basename: the version name it installs as.
    pub name: String,
    pub text: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Tarball { url: String, fragment: Option<String> },
    Git { url: String, reference: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub fetch: Fetch,
    pub steps: Vec<String>,
    /// `--if <function>`: the package is skipped when it is false (all Linux ones are).
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Definition {
    pub packages: Vec<Package>,
    /// Exported and plain assignments, in order, already expanded.
    pub vars: Vec<(String, String)>,
    pub require_gcc: bool,
}

/// `PYTHON_BUILD_DEFINITIONS` (colon-separated), then, for `pyenv install` (`plugins`), each
/// `$PYENV_ROOT/plugins/*/share/python-build`.
fn dirs(root: &Path, env: &dyn Fn(&str) -> Option<String>, plugins: bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = env("PYTHON_BUILD_DEFINITIONS")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if plugins {
        let mut found: Vec<PathBuf> = std::fs::read_dir(root.join("plugins"))
            .map(|rd| rd.filter_map(Result::ok).map(|e| e.path().join("share").join("python-build")).filter(|p| p.is_dir()).collect())
            .unwrap_or_default();
        found.sort();
        out.extend(found);
    }
    out
}

/// `python-build --definitions`: every definition dir's files except `patches`, then the
/// built-in names, sorted with `sort_versions`, adjacent duplicates removed.
pub fn names(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    list(dirs(root, env, true))
}

/// Standalone `pyenv latest -k`: the same list without plugin directories, which only
/// `pyenv install` adds (reference "Plugin definition directories").
pub fn known(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    list(dirs(Path::new(""), env, false))
}

fn list(dirs: Vec<PathBuf>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            v.extend(rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != "patches"));
        }
    }
    v.extend(DEFINITIONS.iter().map(|(n, _)| n.to_string()));
    sort_versions(&mut v);
    v.dedup();
    v
}

/// python-build's `sort_versions`: the key replaces `+`/`-` with `.`, `.p<digit>` with
/// `.z.<digit>`, appends `.z`, then sorts `-t. -k1,1 -k2,2n -k3,3n -k4,4n -k5,5n` in C order.
pub fn sort_versions(v: &mut Vec<String>) {
    fn key(name: &str) -> String {
        let mut k = name.replace(['+', '-'], ".");
        if let Some(i) = k.find(".p").filter(|&i| k[i + 2..].starts_with(|c: char| c.is_ascii_digit())) {
            k.replace_range(i..i + 2, ".z.");
        }
        k.push_str(".z");
        k
    }
    // `sort -n` reads a leading number; text without one compares as 0.
    fn num(field: &str) -> u64 {
        let digits: String = field.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().unwrap_or(0)
    }
    v.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        let fa: Vec<&str> = ka.split('.').collect();
        let fb: Vec<&str> = kb.split('.').collect();
        let f = |v: &Vec<&str>, i: usize| v.get(i).copied().unwrap_or("");
        f(&fa, 0).cmp(f(&fb, 0))
            .then_with(|| (1..5).map(|i| num(f(&fa, i)).cmp(&num(f(&fb, i)))).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| format!("{ka} {a}").cmp(&format!("{kb} {b}")))
    });
}

/// Like python-build's argument 1: an existing file path, else the first definitions dir
/// that has the name, else the built-in copy.
pub fn find(root: &Path, arg: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<Found> {
    let p = Path::new(arg);
    if p.is_file() {
        let text = std::fs::read_to_string(p).ok()?;
        let name = p.file_name()?.to_string_lossy().into_owned();
        return Some(Found { name, text, origin: Origin::Path(p.to_path_buf()) });
    }
    if arg.is_empty() || arg.contains('/') {
        return None;
    }
    for d in dirs(root, env, true) {
        let f = d.join(arg);
        if f.is_file() {
            let text = std::fs::read_to_string(&f).ok()?;
            return Some(Found { name: arg.to_string(), text, origin: Origin::Dir(d) });
        }
    }
    DEFINITIONS.iter().find(|(n, _)| *n == arg).map(|(n, t)| Found { name: n.to_string(), text: t.to_string(), origin: Origin::Builtin })
}

/// The built-in patches for `<def>/<package>/`, sorted by file name (python-build `sort -z`).
pub fn patches_for(def: &str, package: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(PATCHES));
    let Ok(entries) = ar.entries() else { return out };
    for mut e in entries.filter_map(Result::ok) {
        let Ok(path) = e.path().map(|p| p.into_owned()) else { continue };
        let comps: Vec<String> = path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if comps.len() == 3 && comps[0] == def && comps[1] == package && e.header().entry_type().is_file() {
            let mut data = Vec::new();
            if e.read_to_end(&mut data).is_ok() {
                out.push((comps[2].clone(), data));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Splits a shell line into words, honoring double and single quotes and expanding
/// `$VAR`, `${VAR}`, `${VAR:+word}` and `${VAR:-word}` (in double quotes and bare words).
fn words(line: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Vec<String>, ()> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut any = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match (quote, c) {
            (None, ' ' | '\t') => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
                i += 1;
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
                i += 1;
            }
            (Some(q), c) if c == q => {
                quote = None;
                i += 1;
            }
            (Some('\''), c) => {
                cur.push(c);
                i += 1;
            }
            (_, '$') => {
                let (text, used) = expand(&chars[i..], lookup)?;
                cur.push_str(&text);
                any = true;
                i += used;
            }
            (_, c) => {
                cur.push(c);
                any = true;
                i += 1;
            }
        }
    }
    if quote.is_some() {
        return Err(());
    }
    if any {
        out.push(cur);
    }
    Ok(out)
}

/// Expands one `$…` at the start of `s`; returns the text and the chars consumed.
fn expand(s: &[char], lookup: &dyn Fn(&str) -> Option<String>) -> Result<(String, usize), ()> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    if s.get(1) == Some(&'{') {
        let close = s.iter().position(|&c| c == '}').ok_or(())?;
        // A nested `${…}` inside the word part closes later.
        let mut depth = 0;
        let mut end = 0;
        for (j, &c) in s.iter().enumerate().skip(1) {
            if c == '{' { depth += 1; }
            if c == '}' { depth -= 1; if depth == 0 { end = j; break; } }
        }
        let end = if end == 0 { close } else { end };
        let body: String = s[2..end].iter().collect();
        let value = if let Some((name, word)) = body.split_once(":+") {
            match lookup(name).filter(|v| !v.is_empty()) {
                Some(_) => words(&format!("\"{word}\""), lookup)?.concat(),
                None => String::new(),
            }
        } else if let Some((name, word)) = body.split_once(":-") {
            match lookup(name).filter(|v| !v.is_empty()) {
                Some(v) => v,
                None => words(&format!("\"{word}\""), lookup)?.concat(),
            }
        } else if body.chars().all(ident) {
            lookup(&body).unwrap_or_default()
        } else {
            return Err(());
        };
        return Ok((value, end + 1));
    }
    let n = s[1..].iter().take_while(|&&c| ident(c)).count();
    if n == 0 {
        return Err(());
    }
    let name: String = s[1..1 + n].iter().collect();
    Ok((lookup(&name).unwrap_or_default(), 1 + n))
}

const DARWIN_IF: &str = r#"if [[ "Darwin" == "$(uname -s)" ]]; then"#;
const SOURCE_TWIN: &str = r#"source "${BASH_SOURCE[0]%t}""#;
const SOURCE_SIBLING: &str = r#"source "$(dirname "${BASH_SOURCE[0]}")"/"#;

/// Interprets a definition. `env` is the process environment; `sibling` finds a definition
/// named by `source` (in the same place as `found`).
pub fn parse(
    found: &Found,
    env: &dyn Fn(&str) -> Option<String>,
    sibling: &dyn Fn(&str) -> Option<Found>,
) -> Result<Definition, String> {
    let mut def = Definition::default();
    run(found, env, sibling, &mut def, 0)?;
    Ok(def)
}

fn run(
    found: &Found,
    env: &dyn Fn(&str) -> Option<String>,
    sibling: &dyn Fn(&str) -> Option<Found>,
    def: &mut Definition,
    depth: u32,
) -> Result<(), String> {
    if depth > 4 {
        return Err(format!("{}: `source` nests too deeply", found.name));
    }
    // Join backslash continuations, remembering each logical line's first physical line.
    let mut lines: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (i, raw) in found.text.lines().enumerate() {
        let (n, mut acc) = pending.take().unwrap_or((i + 1, String::new()));
        let t = raw.trim_end();
        if let Some(head) = t.strip_suffix('\\') {
            acc.push_str(head);
            acc.push(' ');
            pending = Some((n, acc));
        } else {
            acc.push_str(t);
            lines.push((n, acc));
        }
    }
    if let Some(p) = pending {
        lines.push(p);
    }
    // Branch state for `if … then … else … fi`: whether the current branch runs.
    let mut stack: Vec<(bool, bool)> = Vec::new(); // (condition, in_else)
    let active = |stack: &Vec<(bool, bool)>| stack.iter().all(|&(c, e)| c != e);
    for (n, line) in &lines {
        let t = line.trim();
        let bad = || format!("{}: line {n}: rpyenv cannot interpret this definition line: {t}", found.name);
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        match t {
            "if has_tar_xz_support; then" => { stack.push((true, false)); continue; }
            _ if t == DARWIN_IF => { stack.push((false, false)); continue; }
            "else" => { let top = stack.last_mut().ok_or_else(bad)?; top.1 = true; continue; }
            "fi" => { stack.pop().ok_or_else(bad)?; continue; }
            _ => {}
        }
        if !active(&stack) {
            continue;
        }
        let lookup = |k: &str| {
            def.vars.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone()).or_else(|| env(k))
        };
        if t == SOURCE_TWIN || t.starts_with(SOURCE_SIBLING) {
            let target = if t == SOURCE_TWIN {
                found.name.strip_suffix('t').ok_or_else(bad)?.to_string()
            } else {
                t[SOURCE_SIBLING.len()..].to_string()
            };
            let f = sibling(&target).ok_or_else(|| format!("{}: line {n}: cannot find `{target}' to source", found.name))?;
            run(&f, env, sibling, def, depth + 1)?;
            continue;
        }
        // `has_tar_xz_support && A || B` (3.4.10): rpyenv always reads xz, so A runs.
        let stmt = match t.strip_prefix("has_tar_xz_support") {
            Some(rest) => {
                let rest = rest.trim_start().strip_prefix("&&").ok_or_else(bad)?;
                rest.split(" || ").next().ok_or_else(bad)?.trim().to_string()
            }
            None => t.to_string(),
        };
        let w = words(&stmt, &lookup).map_err(|_| bad())?;
        let Some(first) = w.first() else { continue };
        match first.as_str() {
            "prefer_openssl3" | "prefer_openssl11" | "prefer_openssl3_to_4" => {}
            "require_gcc" => def.require_gcc = true,
            "export" | _ if first.contains('=') || first == "export" => {
                let assign = if first == "export" { w.get(1).ok_or_else(bad)? } else { first };
                let (k, v) = assign.split_once('=').ok_or_else(bad)?;
                if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || w.len() > if first == "export" { 2 } else { 1 } {
                    return Err(bad());
                }
                def.vars.push((k.to_string(), v.to_string()));
            }
            "install_package" | "install_git" => {
                let git = first == "install_git";
                let fixed = if git { 4 } else { 3 };
                if w.len() < fixed {
                    return Err(bad());
                }
                let mut rest: Vec<String> = w[fixed..].to_vec();
                let condition = match rest.iter().position(|s| s == "--if") {
                    Some(i) => {
                        let c = rest.get(i + 1).cloned().ok_or_else(bad)?;
                        rest.truncate(i);
                        Some(c)
                    }
                    None => None,
                };
                let steps = if rest.is_empty() { vec!["standard".to_string()] } else { rest };
                let fetch = if git {
                    Fetch::Git { url: w[2].clone(), reference: w[3].clone() }
                } else {
                    let (u, f) = super::checksum::split_url(&w[2]);
                    Fetch::Tarball { url: u.to_string(), fragment: f.map(str::to_string) }
                };
                def.packages.push(Package { name: w[1].clone(), fetch, steps, condition });
            }
            _ => return Err(bad()),
        }
    }
    if !stack.is_empty() {
        return Err(format!("{}: `if` without `fi`", found.name));
    }
    Ok(())
}
```

Notes for the implementer:
- The match arm `"export" | _ if …` is written for clarity of intent. If clippy rejects it, split it into an `if first == "export" || first.contains('=')` check before the `match`. Keep the behavior: an `export` line or a plain `NAME=value` line is an assignment, and anything else on such a line is an error.
- `sibling` lookups for `Origin::Builtin` and `Origin::Dir` use `defs::find(root, name, env)`. For `Origin::Path`, the caller passes a closure that looks in the definition file's own directory.
- `words` treats a `$(…)` command substitution as an error. The only command substitutions in the CPython definitions are on the `source` and Darwin lines, which are matched as whole lines before `words` runs.

- [ ] **Step 7: Run the tests and see them pass**

Run: `cargo test -p pyenv --test install_defs`, then `cargo test --workspace` on Windows and in WSL.
Expected: all pass. If `every_vendored_definition_parses` lists failures, fix the interpreter for the construct it names. Never skip a definition to make the test pass.

- [ ] **Step 8: Check that the test can fail**

Temporarily delete the `DARWIN_IF` arm. `every_vendored_definition_parses` must fail and name `3.0.1`. Restore the arm.

- [ ] **Step 9: Lint, the size note, and commit**

1. Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `python ci/shim_deps.py`.
2. Record in the report the release `pyenv` binary size before and after this task, from `cargo build --release -p pyenv` and the file size. The patches are about 7.4 MB uncompressed.
3. Commit:

```bash
git add .gitattributes ci/sync_python_build.py ci/test_sync_python_build.py crates/pyenv Cargo.lock
git commit -m "Vendor python-build's CPython definitions at pyenv ab74141 and interpret them"
```

---
### Task 4: `pyenv latest`, in installed and known mode

**Files:**
- Modify: `crates/rpyenv-core/src/latest.rs` (expose the match without the directory shortcut)
- Create: `crates/pyenv/src/commands/latest.rs`
- Modify: `crates/pyenv/src/commands/mod.rs` (a `LINUX_ONLY` table)
- Modify: `crates/pyenv/src/help.rs` (the `latest` topic)
- Create: `crates/pyenv/tests/cli_latest.rs`

**Interfaces:**
- Consumes:
  - `defs::known(env) -> Vec<String>` (Task 3);
  - `installed::names(&Path, Flavor) -> Vec<String>`;
  - `installed::is_staging_name` (Task 2).
- Produces:
  - `rpyenv_core::latest::best(prefix: &str, candidates: &[String]) -> Option<String>`: steps 3–8 of M1's algorithm.
  - `commands::latest::resolve_known(prefix: &str, names: &[String]) -> Option<String>`, which Task 7 uses.

- [ ] **Step 1: Write the failing tests**

Create `crates/pyenv/tests/cli_latest.rs`:

```rust
//! `pyenv latest` (docs/parity/pyenv-m1-reference.md "pyenv latest", pyenv-m2-reference.md
//! "pyenv latest — what M2 adds"). Linux flavor only until M2b.
#![cfg(unix)]

mod common;
use common::Fixture;

#[test]
fn installed_mode_picks_the_newest_matching_version() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.12.10").version("3.12.9").version("3.13.0");
    let r = f.pyenv(&["latest", "3.12"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("3.12.10\n", "", 0));
}

#[test]
fn installed_mode_ignores_the_installers_staging() {
    let f = Fixture::new();
    f.version("3.12.1").version(".tmp-3.12.20");
    assert_eq!(f.pyenv(&["latest", "3.12"]).stdout, "3.12.1\n");
}

#[test]
fn known_mode_reads_the_vendored_definitions() {
    let f = Fixture::new();
    for (prefix, want) in [("3.12", "3.12.14"), ("3.1", "3.1.5"), ("3.14t", "3.14.7t"), ("3t", "3.15.0rc2t"), ("2", "2.7.18"), ("3.12-dev", "3.12-dev")] {
        let r = f.pyenv(&["latest", "-k", prefix]);
        assert_eq!((r.stdout.as_str(), r.code), (format!("{want}\n").as_str(), 0), "{prefix}");
    }
}

#[test]
fn a_failed_match_reports_or_falls_back() {
    let f = Fixture::new();
    let r = f.pyenv(&["latest", "-k", "3.15"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "pyenv: no known versions match the prefix `3.15'\n", 1));
    let r = f.pyenv(&["latest", "3.15"]);
    assert_eq!(r.stderr, "pyenv: no installed versions match the prefix `3.15'\n");
    let r = f.pyenv(&["latest", "-b", "3.15"]);
    assert_eq!((r.stdout.as_str(), r.code), ("3.15\n", 1));
    let r = f.pyenv(&["latest", "-f", "3.15"]);
    assert_eq!((r.stdout.as_str(), r.code), ("3.15\n", 0));
    let r = f.pyenv(&["latest", "-k", "-q", "3.12"]);
    assert_eq!(r.stderr, "pyenv: no known versions match the prefix `-q'\n", "no -q option exists");
}

#[test]
fn standalone_known_mode_does_not_see_plugin_definitions() {
    let f = Fixture::new();
    f.file(&f.root.join("plugins/fake/share/python-build/3.12.99"), "x\n");
    assert_eq!(f.pyenv(&["latest", "-k", "3.12"]).stdout, "3.12.14\n");
}

#[test]
fn no_prefix_prints_an_empty_line_in_installed_mode() {
    let f = Fixture::new();
    let r = f.pyenv(&["latest"]);
    assert_eq!((r.stdout.as_str(), r.code), ("\n", 0));
}

#[test]
fn help_matches_upstream() {
    let f = Fixture::new();
    let r = f.pyenv(&["help", "latest"]);
    assert_eq!(r.stdout, "Usage: pyenv latest [-k|--known] <prefix>\n\n  -k/--known      Select from all known versions instead of installed\n  -b/--bypass     (internal) On a resolution failure, do not print an error message\n                  but rather print the argument unchanged\n  -f/--force      (internal) Same as -b but also do not return a failure exit code\n\n");
    assert_eq!(f.pyenv(&["help", "--usage", "latest"]).stdout, "Usage: pyenv latest [-k|--known] <prefix>\n");
}
```

Run: `cargo test -p pyenv --test cli_latest` in WSL.
Expected: every test fails, with `pyenv: no such command `latest'`.

- [ ] **Step 2: Split the core matcher**

In `crates/rpyenv-core/src/latest.rs`, move everything after the directory/exact check of `latest` into a new public function, and make `latest` call it:

```rust
/// `pyenv latest <prefix>` over `candidates`, the installed names in
/// `versions --bare --skip-envs` order. None when nothing matches.
pub fn latest(prefix: &str, candidates: &[String], versions_dir: &Path) -> Option<String> {
    if versions_dir.join(prefix).is_dir() {
        return Some(prefix.to_string());
    }
    best(prefix, candidates)
}

/// Steps 3–8 of upstream's algorithm (M1 reference): the exact match, the `t` suffix, the
/// prefix filter, the exclusions and the sort. Known mode uses this directly.
pub fn best(prefix: &str, candidates: &[String]) -> Option<String> {
    if candidates.iter().any(|c| c == prefix) {
        return Some(prefix.to_string());
    }
    // … the rest of the former body, unchanged …
}
```

Run `cargo test -p rpyenv-core`. It must stay green, because this is a pure refactor.

- [ ] **Step 3: Implement the command**

Create `crates/pyenv/src/commands/latest.rs`:

```rust
//! `pyenv latest [-k|--known] [-b|--bypass] [-f|--force] <prefix>` (M1 reference).

use crate::install::defs;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::installed;
use rpyenv_core::latest::{best, latest as latest_installed};

/// The known-mode winner for `prefix` among `names` (definitions), for `pyenv install`.
pub fn resolve_known(prefix: &str, names: &[String]) -> Option<String> {
    best(prefix, names)
}

/// The prefix as upstream's messages show it: a trailing `t` after a digit is stripped.
fn shown(prefix: &str) -> &str {
    match prefix.strip_suffix('t') {
        Some(b) if b.ends_with(|c: char| c.is_ascii_digit()) => b,
        _ => prefix,
    }
}

pub fn latest(ctx: &Ctx, args: &[&str]) -> Output {
    let (mut known, mut bypass, mut force) = (false, false, false);
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match *a {
            "-k" | "--known" => known = true,
            "-b" | "--bypass" => bypass = true,
            "-f" | "--force" => force = true,
            _ => break,
        }
        i += 1;
    }
    let prefix = args.get(i).copied().unwrap_or("");
    let found = if known {
        let env = |k: &str| std::env::var(k).ok();
        best(prefix, &defs::known(&env))
    } else {
        let names: Vec<String> = installed::names(&ctx.versions_dir(), ctx.flavor)
            .into_iter()
            .filter(|n| !installed::is_staging_name(n))
            .collect();
        latest_installed(prefix, &names, &ctx.versions_dir())
    };
    let mut o = Output::new();
    match found {
        Some(v) => o.out(v),
        None if bypass || force => {
            o.out(shown(prefix));
            o.code = if force { 0 } else { 1 };
        }
        None => {
            let kind = if known { "known" } else { "installed" };
            o.err(format!("pyenv: no {kind} versions match the prefix `{}'", shown(prefix)));
            o.code = 1;
        }
    }
    o
}
```

If `installed::names` already lists only top-level, non-env names in `versions --bare --skip-envs` order, keep it. Otherwise use whatever `prefix.rs` passes to `latest::latest`, which is the same list. Check both call sites (`prefix.rs:47`, `select.rs:98`), and say in the report which list you used.

In `crates/pyenv/src/commands/mod.rs`:
1. Add `pub mod latest;`.
2. Add the Linux-only table, mirroring `WIN_ONLY`:

```rust
/// pyenv (Linux) only, until M2b brings pyenv-win's installer commands.
const LINUX_ONLY: &[(&str, Command)] = &[("latest", latest::latest)];
```

3. Change `table` to chain `LINUX_ONLY` when `flavor == Flavor::Pyenv`.

In `crates/pyenv/src/help.rs`, add this to the `PYENV` topics, in the table's alphabetical position:

```rust
    topic("latest", Some("Print the latest installed or known version with the given prefix"), Some("Usage: pyenv latest [-k|--known] <prefix>"),
        "Usage: pyenv latest [-k|--known] <prefix>\n\n  -k/--known      Select from all known versions instead of installed\n  -b/--bypass     (internal) On a resolution failure, do not print an error message\n                  but rather print the argument unchanged\n  -f/--force      (internal) Same as -b but also do not return a failure exit code\n\n"),
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p pyenv --test cli_latest` in WSL, then `cargo test --workspace` on both OSes.
Expected: 7 passed in WSL. On Windows the file compiles to nothing (`#![cfg(unix)]`), and the rest stays green. `cli_dispatch`'s `commands` and `help` listing tests may now fail on Linux because `latest` joins the list. Update their expected strings to include `latest`, in upstream's order: `sort -u` for `commands`, and for `help` the order of the existing listing.

- [ ] **Step 5: Check that a test can fail**

Temporarily return `Some(prefix.to_string())` at the top of `best`. `known_mode_reads_the_vendored_definitions` must fail. Restore.

- [ ] **Step 6: Lint and commit**

Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`.

```bash
git add crates
git commit -m "Add pyenv latest, resolving against installed versions or the vendored definitions"
```

---

### Task 5: The source build (python-build's steps)

**Files:**
- Create: `crates/pyenv/src/install/log.rs`, `crates/pyenv/src/install/verify.rs`, `crates/pyenv/src/install/builder.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod log; #[cfg(unix)] pub mod verify; #[cfg(unix)] pub mod builder;`)
- Create: `crates/pyenv/tests/common/fakebuild.rs`, `crates/pyenv/tests/install_build.rs`
- Modify: `crates/pyenv/tests/common/mod.rs` (add `pub mod fakebuild;`)
- Modify: `crates/pyenv/Cargo.toml` (`[dev-dependencies]` adds `flate2 = "1.1.10"` and `tar = "0.4.46"`)

**Interfaces:**
- Consumes:
  - `Fetcher`, `FetchRequest` (Task 1);
  - `archive::{extract, Kind}`, `Txn` (Task 2);
  - `defs::{Definition, Fetch, Found, Origin, Package, patches_for}` (Task 3).
- Produces:
  - `log::{BuildLog::open(&Path, verbose: bool) -> io::Result<BuildLog>, BuildLog::line(&self, &str), BuildLog::run(&self, &mut Command) -> io::Result<ExitStatus>, failed_block(os: &str, build_path: &Path, log_path: &Path) -> Vec<String>}`. `BuildLog` implements `Write`.
  - `verify::{plan(step: &str) -> Option<(String, Vec<Check>)>, Check { module, lib, extra, fatal, needs_display }}`.
  - `builder::{Options { keep, verbose, debug, stdin_patch: Option<Vec<u8>> }, Job { found, definition, prefix, opts, env, fetcher }, run(job: &Job, txn: &mut Txn, say: &mut dyn FnMut(&str)) -> Result<(), InstallError>, xy_of(name: &str) -> Option<String>, patch_files(found: &Found, package: &str) -> Vec<(String, Vec<u8>)>}`.
  - `Options::debug` makes `Plan` prefix `PYTHON_CFLAGS` with `-O0`. Callers pass the plain environment; there is no overlay.
  - `builder::run` places the version through `txn` but does not commit. The caller (Task 7) commits after it returns `Ok`.

- [ ] **Step 1: Write the fake CPython source**

Create `crates/pyenv/tests/common/fakebuild.rs`:

```rust
//! A fake `Python-<v>` source tarball for tier-1 build tests (spec §12.5): `configure`
//! records its arguments and flags, `make install` honors DESTDIR, and the stand-in
//! interpreter answers `-c "import X"` and `-m ensurepip`. Unix only: it needs `sh` and `make`.

use std::io::Write;

const CONFIGURE: &str = r#"#!/bin/sh
prefix=
for a in "$@"; do case "$a" in --prefix=*) prefix="${a#--prefix=}";; esac; done
{ printf 'args:'; for a in "$@"; do printf ' %s' "$a"; done; printf '\n'
  printf 'CFLAGS=%s\nCPPFLAGS=%s\nLDFLAGS=%s\nLIBS=%s\n' "$CFLAGS" "$CPPFLAGS" "$LDFLAGS" "$LIBS"; } > rpyenv-config.txt
if [ -n "$FAKE_CONFIGURE_FAIL" ]; then echo 'configure: error: no acceptable C compiler found in $PATH'; exit 1; fi
printf 'all:\n\t@sleep $${FAKE_MAKE_SLEEP:-0}\ninstall:\n\tmkdir -p "$(DESTDIR)%s/bin" "$(DESTDIR)%s/lib"\n\tcp python3.12 "$(DESTDIR)%s/bin/python3.12"\n\tchmod 755 "$(DESTDIR)%s/bin/python3.12"\n\tcp rpyenv-config.txt "$(DESTDIR)%s/lib/rpyenv-config.txt"\n' "$prefix" "$prefix" "$prefix" "$prefix" "$prefix" > Makefile
"#;

const PYTHON: &str = r#"#!/bin/sh
case "$1" in
  -c) mod="${2#import }"
      for m in $FAKE_PY_MISSING; do
        if [ "$m" = "$mod" ]; then echo "ModuleNotFoundError: No module named '_$mod'" >&2; exit 1; fi
      done
      exit 0;;
  -m) if [ "$2" = "pip" ]; then
        printf '%s\n' "$*" >> "${FAKE_PIP_LOG:-/dev/null}"
        [ -n "$FAKE_PIP_FAIL" ] && exit 1
        exit 0
      fi;;
  -I|-s)
      if [ "$2" = "-m" ] && [ "$3" = "ensurepip" ]; then
        sleep "${FAKE_PIP_SLEEP:-0}"
        [ -n "$FAKE_PY_NO_PIP" ] && exit 1
        d=$(dirname "$0"); printf '#!%s\n' "$d/python3.12" > "$d/pip3.12"; chmod 755 "$d/pip3.12"; exit 0
      fi;;
esac
exit 0
"#;

/// `Python-<version>.tar.gz` bytes: `configure`, `python3.12`, `README`, `Tools/gdb/libpython.py`.
pub fn tarball(version: &str) -> Vec<u8> {
    let top = format!("Python-{version}");
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (name, body, mode) in [
        ("configure", CONFIGURE, 0o755),
        ("python3.12", PYTHON, 0o755),
        ("README", "original\n", 0o644),
        ("Tools/gdb/libpython.py", "# gdb\n", 0o644),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(mode);
        b.append_data(&mut h, format!("{top}/{name}"), body.as_bytes()).unwrap();
    }
    let gz = b.into_inner().unwrap();
    let mut out = gz.finish().unwrap();
    out.flush().unwrap();
    out
}

/// A patch that changes README's one line, in python-build's built-in patch layout.
pub const README_PATCH: &str = "--- README\n+++ README\n@@ -1 +1 @@\n-original\n+patched\n";
```

- [ ] **Step 2: Write the failing build tests**

Create `crates/pyenv/tests/install_build.rs`:

```rust
//! Tier-1 source-build tests (spec §12.5) with a fake CPython and a local server.
#![cfg(unix)]

mod common;

use common::fakebuild::{tarball, README_PATCH};
use common::server::{start, Reply};
use pyenv::install::builder::{run, Job, Options};
use pyenv::install::defs::{self, Found};
use pyenv::install::fetch::Fetcher;
use pyenv::install::txn::{is_complete, Txn};
use pyenv::install::InstallError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct Env(HashMap<String, String>);
impl Env {
    fn get(&self, k: &str) -> Option<String> {
        self.0.get(k).cloned()
    }
}

struct Built {
    result: Result<(), InstallError>,
    said: Vec<String>,
    root: PathBuf,
    tmp: PathBuf,
    _dirs: Vec<tempfile::TempDir>,
}

/// Writes definition `3.12.99` (and a built-in-style patch when `patch`) next to a fake
/// tarball served locally, then builds it into `<root>/versions/3.12.99`.
fn build(steps: &str, vars: &[(&str, &str)], opts: Options, patch: bool, preexisting: bool) -> Built {
    let body = tarball("3.12.99");
    let sha = {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t");
        std::fs::write(&p, &body).unwrap();
        pyenv::install::checksum::sha256_file(&p).unwrap()
    };
    let server = start(vec![("/Python-3.12.99.tar.gz", vec![Reply::Body(body)])]);
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("py env ñ").join("root");
    let defdir = base.path().join("defs");
    let tmp = base.path().join("tmp");
    std::fs::create_dir_all(root.join("versions")).unwrap();
    std::fs::create_dir_all(&defdir).unwrap();
    if preexisting {
        std::fs::create_dir_all(root.join("versions/3.12.99/bin")).unwrap();
        std::fs::write(root.join("versions/3.12.99/bin/old"), "").unwrap();
    }
    let text = format!(
        "install_package \"Python-3.12.99\" \"{}#{sha}\" {steps}\n",
        server.url("/Python-3.12.99.tar.gz")
    );
    let def = defdir.join("3.12.99");
    std::fs::write(&def, &text).unwrap();
    if patch {
        let pd = defdir.join("patches/3.12.99/Python-3.12.99");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(pd.join("0001-readme.patch"), README_PATCH).unwrap();
    }
    let mut map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    map.insert("TMPDIR".into(), tmp.display().to_string());
    map.insert("PYTHON_BUILD_SKIP_MIRROR".into(), "1".into());
    map.entry("PATH".into()).or_insert_with(|| std::env::var("PATH").unwrap());
    let env = Env(map);
    let found = defs::find(&root, def.to_str().unwrap(), &|k| env.get(k)).unwrap();
    let definition = defs::parse(&found, &|k| env.get(k), &|_| None::<Found>).unwrap();
    let fetcher = Fetcher::from_env(&|k| env.get(k), None);
    let job = Job {
        found: &found,
        definition: &definition,
        prefix: root.join("versions/3.12.99"),
        opts,
        env: &|k| env.get(k),
        fetcher: &fetcher,
    };
    let mut txn = Txn::begin(&root.join("versions"), "3.12.99").unwrap();
    let mut said = Vec::new();
    let result = run(&job, &mut txn, &mut |s: &str| said.push(s.to_string()));
    if result.is_ok() {
        txn.commit().unwrap();
    } else {
        drop(txn);
    }
    Built { result, said, root, tmp, _dirs: vec![base] }
}

fn opts() -> Options {
    Options { keep: false, verbose: false, debug: false, stdin_patch: None }
}

fn config(b: &Built) -> String {
    std::fs::read_to_string(b.root.join("versions/3.12.99/lib/rpyenv-config.txt")).unwrap()
}

#[test]
fn a_standard_build_installs_with_python_builds_flags_and_messages() {
    let b = build("standard verify_py312 copy_python_gdb ensurepip", &[], opts(), true, false);
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let v = b.root.join("versions/3.12.99");
    assert!(is_complete(&v));
    let p = v.display().to_string();
    let c = config(&b);
    assert!(c.starts_with(&format!("args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no\n")), "{c}");
    assert!(c.contains(&format!("LDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib\n")), "{c}");
    assert!(c.contains(&format!("LIBS=-L{p}/lib -Wl,-rpath,{p}/lib\n")), "{c}");
    assert!(c.contains(&format!("CPPFLAGS=-I{p}/include\n")), "{c}");
    assert_eq!(
        &b.said[..],
        &["Downloading Python-3.12.99.tar.gz...".to_string(), b.said[1].clone(), "Installing Python-3.12.99...".to_string(), format!("Installed Python-3.12.99 to {p}")]
    );
    // Version-suffix symlinks, pip from ensurepip at the final prefix, gdb helper.
    assert_eq!(std::fs::read_link(v.join("bin/python")).unwrap(), Path::new("python3.12"));
    assert_eq!(std::fs::read_link(v.join("bin/pip")).unwrap(), Path::new("pip3.12"));
    assert_eq!(std::fs::read_to_string(v.join("bin/pip3.12")).unwrap(), format!("#!{p}/bin/python3.12\n"));
    assert!(v.join("bin/python3.12-gdb.py").is_file());
    // The build tree is gone; the log stays, as upstream's does.
    let left: Vec<String> = std::fs::read_dir(&b.tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].starts_with("python-build.") && left[0].ends_with(".log"), "{left:?}");
}

#[test]
fn built_in_patches_are_applied_and_their_output_stays_in_the_log() {
    let b = build("standard", &[], Options { keep: true, ..opts() }, true, false);
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let src = std::fs::read_dir(&b.tmp).unwrap().map(|e| e.unwrap().path()).find(|p| p.is_dir()).unwrap();
    assert_eq!(std::fs::read_to_string(src.join("Python-3.12.99/README")).unwrap(), "patched\n");
    assert!(!b.said.iter().any(|s| s.contains("patching file")));
    let tmp_files: Vec<String> = std::fs::read_dir(&b.tmp).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(!tmp_files.iter().any(|n| n.starts_with("python-patch.")), "{tmp_files:?}");
}

#[test]
fn user_flags_combine_as_python_build_does() {
    let vars = [
        ("CONFIGURE_OPTS", "--global-opt"),
        ("PYTHON_CONFIGURE_OPTS", "--with-foo --enable-optimizations"),
        ("CFLAGS", "-gcflag"),
        ("PYTHON_CFLAGS", "-pycflag"),
        ("CPPFLAGS", "-gcpp"),
        ("PYTHON_CPPFLAGS", "-pycpp"),
        ("LDFLAGS", "-gld"),
        ("PYTHON_LDFLAGS", "-pyld"),
    ];
    let b = build("standard", &vars, opts(), false, false);
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let p = b.root.join("versions/3.12.99").display().to_string();
    let c = config(&b);
    assert!(c.starts_with(&format!("args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no --global-opt --with-foo --enable-optimizations\n")), "{c}");
    assert!(c.contains("CFLAGS=-gcflag -pycflag\n"), "{c}");
    assert!(c.contains(&format!("CPPFLAGS=-I{p}/include -gcpp -pycpp\n")), "{c}");
    assert!(c.contains(&format!("LDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib -gld -pyld\n")), "{c}");
}

#[test]
fn disable_shared_drops_shared_and_the_rpath() {
    let b = build("standard", &[("PYTHON_CONFIGURE_OPTS", "--disable-shared")], opts(), false, false);
    let p = b.root.join("versions/3.12.99").display().to_string();
    let c = config(&b);
    assert!(c.starts_with(&format!("args: --prefix={p} --libdir={p}/lib --with-ensurepip=no --disable-shared\n")), "{c}");
    assert!(c.contains(&format!("LDFLAGS=-L{p}/lib\n")), "{c}");
}

#[test]
fn a_missing_optional_module_warns_and_a_missing_ssl_fails_and_rolls_back() {
    let b = build("standard verify_py312", &[("FAKE_PY_MISSING", "bz2")], opts(), false, false);
    assert_eq!(b.result, Ok(()));
    let b = build("standard verify_py312 ensurepip", &[("FAKE_PY_MISSING", "bz2 ssl")], opts(), false, false);
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(!b.root.join("versions/3.12.99").exists());
    let names: Vec<String> = std::fs::read_dir(b.root.join("versions")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(names.is_empty(), "{names:?}");
    let all = b.said.join("\n");
    assert!(all.contains("BUILD FAILED ("), "{all}");
    assert!(all.contains(" using rpyenv "), "{all}");
}

#[test]
fn a_failed_build_over_an_existing_version_restores_it() {
    let b = build("standard", &[("FAKE_CONFIGURE_FAIL", "1")], opts(), false, true);
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(b.root.join("versions/3.12.99/bin/old").is_file(), "previous version restored");
    let all = b.said.join("\n");
    assert!(all.contains("Inspect or clean up the working tree at "), "{all}");
    assert!(all.contains("Last 10 log lines:"), "{all}");
    assert!(all.contains("no acceptable C compiler found"), "{all}");
    assert!(all.contains("Are the build dependencies for Python correctly installed?"), "{all}");
}

#[test]
fn a_failed_ensurepip_does_not_fall_back_to_get_pip() {
    let b = build("standard ensurepip", &[("FAKE_PY_NO_PIP", "1")], opts(), false, false);
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(b.said.iter().any(|s| s == "error: failed to install pip via ensurepip"), "{:?}", b.said);
}

#[test]
fn debug_builds_add_pydebug_and_o0() {
    let b = build("standard", &[], Options { debug: true, ..opts() }, false, false);
    let c = config(&b);
    assert!(c.contains(" --with-pydebug --enable-shared "), "{c}");
    assert!(c.contains("CFLAGS=-O0\n") || c.contains("CFLAGS=-O0 \n"), "{c}");
}

#[test]
fn free_threading_adds_disable_gil() {
    let b = build("standard", &[("PYTHON_BUILD_FREE_THREADING", "1")], opts(), false, false);
    let c = config(&b);
    assert!(c.contains(" --disable-gil"), "{c}");
}
```

Run: `cargo test -p pyenv --test install_build` in WSL.
Expected: compile errors, because `builder` doesn't exist yet.

- [ ] **Step 3: Implement `verify.rs`**

```rust
//! python-build's `verify_pyXY` post-build checks (bin/python-build:2192-2390): which
//! modules are fatal and which only warn, in upstream's order.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub module: &'static str,
    /// The "Missing the …?" text.
    pub lib: &'static str,
    /// Extra words after "was not compiled".
    pub extra: Option<&'static str>,
    pub fatal: bool,
    /// tkinter is checked only when `$DISPLAY` is non-empty.
    pub needs_display: bool,
}

const fn warn(module: &'static str, lib: &'static str) -> Check {
    Check { module, lib, extra: None, fatal: false, needs_display: false }
}
const fn fatal(module: &'static str, lib: &'static str) -> Check {
    Check { module, lib, extra: None, fatal: true, needs_display: false }
}

/// For a `verify_pyXY` step: the `X.Y` whose `bin/python<X.Y>` must exist, and the checks.
pub fn plan(step: &str) -> Option<(String, Vec<Check>)> {
    let tag = step.strip_prefix("verify_py")?;
    if tag == "3_latest" {
        return Some(("3".into(), py3(11)));
    }
    let (major, minor) = tag.split_at(1);
    let minor: u32 = minor.parse().ok()?;
    let xy = format!("{major}.{minor}");
    let checks = match (major, minor) {
        ("2", 1..=3) => vec![warn("readline", "GNU readline lib"), fatal("binascii", "binascii"), warn("zlib", "zlib"), warn("bz2", "bzip2 lib")],
        ("2", 4..=7) => {
            let mut v = vec![warn("readline", "GNU readline lib"), fatal("zlib", "zlib"), warn("bz2", "bzip2 lib")];
            if minor >= 5 {
                v.push(warn("sqlite3", "SQLite3 lib"));
            }
            if minor >= 6 {
                v.push(fatal("ssl", "OpenSSL lib"));
            }
            v
        }
        ("3", 0..=16) => py3(minor),
        _ => return None,
    };
    Some((xy, checks))
}

fn py3(minor: u32) -> Vec<Check> {
    let mut v = vec![
        warn("bz2", "bzip2 lib"),
        warn("curses", "ncurses lib"),
        warn("ctypes", "libffi lib"),
        warn("readline", "GNU readline lib"),
        fatal("ssl", "OpenSSL lib"),
        warn("sqlite3", "SQLite3 lib"),
        Check { module: "tkinter", lib: "Tk toolkit", extra: Some("and GUI subsystem has been detected"), fatal: false, needs_display: true },
        fatal("zlib", "zlib"),
    ];
    if minor >= 3 {
        v.push(warn("lzma", "lzma lib"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn py312_checks_in_upstreams_order() {
        let (xy, c) = plan("verify_py312").unwrap();
        assert_eq!(xy, "3.12");
        let order: Vec<&str> = c.iter().map(|c| c.module).collect();
        assert_eq!(order, ["bz2", "curses", "ctypes", "readline", "ssl", "sqlite3", "tkinter", "zlib", "lzma"]);
        assert_eq!(c.iter().filter(|c| c.fatal).map(|c| c.module).collect::<Vec<_>>(), ["ssl", "zlib"]);
    }

    #[test]
    fn py27_has_fatal_zlib_and_ssl() {
        let (xy, c) = plan("verify_py27").unwrap();
        assert_eq!(xy, "2.7");
        assert_eq!(c.iter().filter(|c| c.fatal).map(|c| c.module).collect::<Vec<_>>(), ["zlib", "ssl"]);
    }

    #[test]
    fn rolling_and_unknown_steps() {
        assert_eq!(plan("verify_py3_latest").unwrap().0, "3");
        assert!(plan("verify_py99").is_none());
        assert!(plan("standard").is_none());
    }
}
```

- [ ] **Step 4: Implement `log.rs`**

```rust
//! The build log (python-build's fd 4) with `-v`'s copy to stdout, and the BUILD FAILED
//! block (reference "Output streams…" and "Failure output").

use std::fs::File;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

pub struct BuildLog {
    file: Arc<Mutex<File>>,
    verbose: bool,
    pub path: PathBuf,
}

impl BuildLog {
    pub fn open(path: &Path, verbose: bool) -> std::io::Result<BuildLog> {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(BuildLog { file: Arc::new(Mutex::new(file)), verbose, path: path.to_path_buf() })
    }

    pub fn line(&self, s: &str) {
        let mut me: &BuildLog = self;
        let _ = writeln!(me, "{s}");
    }

    /// Runs `cmd` with stdout and stderr copied to the log (and to stdout with `-v`).
    pub fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
        let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let pumps: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
            .map(|mut src| {
                let file = self.file.clone();
                let verbose = self.verbose;
                std::thread::spawn(move || {
                    let mut buf = [0u8; 8192];
                    while let Ok(n) = src.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        let _ = file.lock().unwrap().write_all(&buf[..n]);
                        if verbose {
                            let _ = std::io::stdout().lock().write_all(&buf[..n]);
                        }
                    }
                })
            })
            .collect();
        let status = child.wait();
        for p in pumps {
            let _ = p.join();
        }
        status
    }
}

impl Write for &BuildLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.lock().unwrap().write_all(buf)?;
        if self.verbose {
            std::io::stdout().lock().write_all(buf)?;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.lock().unwrap().flush()
    }
}

fn tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(f) = File::open(path) else { return Vec::new() };
    let lines: Vec<String> = std::io::BufReader::new(f).lines().map_while(Result::ok).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// python-build's `build_failed` (bin/python-build:191-216), naming rpyenv (plan Decision 9).
pub fn failed_block(os: &str, build_path: &Path, log_path: &Path) -> Vec<String> {
    let mut out = vec![String::new(), format!("BUILD FAILED ({os} using rpyenv {})", env!("CARGO_PKG_VERSION")), String::new()];
    // `rmdir` succeeds only on an empty directory; then the line is left out.
    if std::fs::remove_dir(build_path).is_err() && build_path.exists() {
        out.push(format!("Inspect or clean up the working tree at {}", build_path.display()));
    }
    let last = tail(log_path, 10);
    if !last.is_empty() {
        out.push(format!("Results logged to {}", log_path.display()));
        out.push(String::new());
        out.push("Last 10 log lines:".into());
        let hint = last.iter().any(|l| l.contains("no acceptable C compiler found"));
        out.extend(last);
        if hint {
            out.extend([
                String::new(),
                "Are the build dependencies for Python correctly installed?".into(),
                "Please consult to the Wiki page for more info.".into(),
                "https://github.com/pyenv/pyenv/wiki#suggested-build-environment".into(),
            ]);
        }
    }
    out
}
```

- [ ] **Step 5: Implement `builder.rs`**

```rust
//! python-build's Linux source build (docs/parity/pyenv-m2-reference.md "python-build"),
//! staged through the install transaction (spec §9.3, plan Decision 4).

use super::archive::{extract, Kind};
use super::checksum::sha256_of_fragment;
use super::defs::{patches_for, Definition, Fetch, Found, Origin, Package};
use super::fetch::{FetchRequest, Fetcher};
use super::log::{failed_block, BuildLog};
use super::txn::Txn;
use super::verify::plan as verify_plan;
use super::{interrupted, InstallError};
use std::io::IsTerminal;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Options {
    pub keep: bool,
    pub verbose: bool,
    pub debug: bool,
    /// `-p`: the patch read from stdin, for the first `Python-*` package.
    pub stdin_patch: Option<Vec<u8>>,
}

pub struct Job<'a> {
    pub found: &'a Found,
    pub definition: &'a Definition,
    /// The final prefix, `versions/<name>`.
    pub prefix: PathBuf,
    pub opts: Options,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub fetcher: &'a Fetcher,
}

type R<T> = Result<T, InstallError>;

/// `X.Y` from a definition name: `3.12.10` → `3.12`, `3.13-dev` → `3.13`, `3.13.0t` → `3.13`.
pub fn xy_of(name: &str) -> Option<String> {
    let mut parts = name.split(|c: char| !c.is_ascii_digit());
    let x = parts.next().filter(|s| !s.is_empty())?;
    let rest = &name[x.len()..];
    let rest = rest.strip_prefix('.')?;
    let y: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!y.is_empty()).then(|| format!("{x}.{y}"))
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn join(parts: &[&str]) -> String {
    parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ")
}

/// `YYYYmmddHHMMSS.<pid>` in UTC (upstream uses local time; only file names differ).
fn seed() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}{:02}{:02}{:02}.{}", rem / 3600, rem % 3600 / 60, rem % 60, std::process::id())
}

fn tmp_dir(env: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, String> {
    let raw = env("TMPDIR").filter(|v| !v.is_empty()).unwrap_or_else(|| "/tmp".into());
    let tmp = PathBuf::from(raw.trim_end_matches('/'));
    let shown = tmp.display().to_string();
    let probe = tmp.join(format!("python-build-test.{}", std::process::id()));
    let made = std::fs::create_dir_all(&tmp).and_then(|_| std::fs::write(&probe, "#!/bin/sh\nexit 0\n"));
    if made.is_err() {
        return Err(format!("python-build: TMPDIR={shown} is set to a non-accessible location"));
    }
    let _ = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755));
    let ran = Command::new(&probe).status().map(|s| s.success()).unwrap_or(false);
    let _ = std::fs::remove_file(&probe);
    if !ran {
        return Err(format!("python-build: TMPDIR={shown} cannot hold executables (partition possibly mounted with `noexec`)"));
    }
    Ok(tmp)
}

fn os_information() -> String {
    if let Ok(o) = Command::new("lsb_release").arg("-sir").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).split_whitespace().collect::<Vec<_>>().join(" ");
            if !s.is_empty() {
                return s;
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim_matches('"').to_string());
        if let (Some(n), Some(v)) = (get("NAME"), get("VERSION_ID")) {
            return format!("{n} {v}");
        }
    }
    Command::new("uname").arg("-sr").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// The configure, make and install commands python-build composes (reference
/// "Environment python-build sets up" and "Build steps").
struct Plan {
    configure: Vec<String>,
    configure_env: Vec<(String, String)>,
    make: String,
    make_args: Vec<String>,
    install_args: Vec<String>,
    altinstall: bool,
    xy: Option<String>,
}

impl Plan {
    fn new(job: &Job) -> Result<Plan, String> {
        let env = job.env;
        let var = |k: &str| -> String {
            job.definition.vars.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone()).or_else(|| env(k)).unwrap_or_default()
        };
        let p = job.prefix.display().to_string();
        let (conf_opts, py_conf_opts) = (var("CONFIGURE_OPTS"), var("PYTHON_CONFIGURE_OPTS"));
        let user = format!("{conf_opts} {py_conf_opts}");
        if user.contains("--enable-framework") {
            return Err("python-build: framework installation is not supported outside of MacOS.".into());
        }
        if user.contains("--enable-universalsdk") {
            return Err("python-build: universal installation is not supported outside of MacOS.".into());
        }
        let mut array: Vec<String> = Vec::new();
        if job.opts.debug {
            array.push("--with-pydebug".into());
        }
        if !user.contains("--disable-shared") {
            array.push("--enable-shared".into());
        }
        array.push(format!("--libdir={p}/lib"));
        let name = &job.found.name;
        // Upstream's `2.*|3.0*|3.1*|3.2*`, without catching `3.10`–`3.19`.
        let old = name.starts_with("2.")
            || ["3.0", "3.1", "3.2"].iter().any(|v| name == v || name.starts_with(&format!("{v}.")) || name.starts_with(&format!("{v}-")));
        if old && !py_conf_opts.contains("--enable-unicode=") {
            array.push("--enable-unicode=ucs4".into());
        }
        if !var("PYTHON_BUILD_FREE_THREADING").is_empty() {
            array.push("--disable-gil".into());
        }
        if !user.contains("--with-ensurepip") {
            // CPython's own ensurepip under DESTDIR would write shebangs into the build tree;
            // rpyenv runs ensurepip at the final prefix instead (plan, Task 5).
            array.push("--with-ensurepip=no".into());
        }
        let shared = array.iter().any(|a| a == "--enable-shared") || user.contains("--enable-shared");
        let user_ld = var("LDFLAGS");
        let rpath = if shared && !user_ld.contains("-rpath=") { format!("-Wl,-rpath,{p}/lib") } else { String::new() };
        let ldflags = join(&[&format!("-L{p}/lib"), &rpath, &user_ld]);
        let libs = join(&[&format!("-L{p}/lib"), &rpath, &var("LIBS")]);
        let cppflags = join(&[&format!("-I{p}/include"), &var("CPPFLAGS")]);
        let py_cflags = if job.opts.debug { format!("-O0 {}", var("PYTHON_CFLAGS")) } else { var("PYTHON_CFLAGS") };
        let configure_env = vec![
            ("CFLAGS".into(), join(&[&var("CFLAGS"), py_cflags.trim_end()])),
            ("CPPFLAGS".into(), join(&[&cppflags, &var("PYTHON_CPPFLAGS")])),
            ("LDFLAGS".into(), join(&[&ldflags, &var("PYTHON_LDFLAGS")])),
            ("LIBS".into(), libs),
        ];
        let configure_cmd = env("PYTHON_CONFIGURE").filter(|v| !v.is_empty()).unwrap_or_else(|| "./configure".into());
        let mut configure = words(&configure_cmd);
        configure.push(format!("--prefix={p}"));
        configure.extend(array);
        configure.extend(words(&conf_opts));
        configure.extend(words(&py_conf_opts));
        let make_opts = match (env("MAKEOPTS"), env("MAKE_OPTS")) {
            (Some(v), _) => v,
            (None, Some(v)) => v,
            (None, None) => format!("-j {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2)),
        };
        let mut make_args = words(&make_opts);
        make_args.extend(words(&var("PYTHON_MAKE_OPTS")));
        let target = env("PYTHON_MAKE_INSTALL_TARGET").filter(|v| !v.is_empty()).unwrap_or_else(|| "install".into());
        let mut install_args = vec![target.clone()];
        install_args.extend(words(&var("MAKE_INSTALL_OPTS")));
        install_args.extend(words(&var("PYTHON_MAKE_INSTALL_OPTS")));
        Ok(Plan {
            configure,
            configure_env,
            make: env("MAKE").filter(|v| !v.is_empty()).unwrap_or_else(|| "make".into()),
            make_args,
            install_args,
            altinstall: target.contains("altinstall"),
            xy: xy_of(name),
        })
    }
}

/// A child process with python-build's cleaned environment (reference step 7).
fn command(program: &str, env: &dyn Fn(&str) -> Option<String>) -> Command {
    let mut c = Command::new(program);
    for k in ["PIP_REQUIRE_VENV", "PIP_REQUIRE_VIRTUALENV", "PYTHONHOME", "PYTHONPATH"] {
        c.env_remove(k);
    }
    if let Some(path) = env("PATH") {
        c.env("PATH", path);
    }
    c
}

fn logged(log: &BuildLog, cmd: &mut Command) -> R<()> {
    let status = log.run(cmd).map_err(|e| {
        log.line(&format!("{cmd:?}: {e}"));
        InstallError::Failed
    })?;
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    if status.success() { Ok(()) } else { Err(InstallError::Failed) }
}

/// The patches python-build applies to `package`: `<definition dir>/patches/<def>/<package>/*`,
/// sorted by name; the built-in ones for vendored definitions.
pub fn patch_files(found: &Found, package: &str) -> Vec<(String, Vec<u8>)> {
    let def = &found.name;
    let dir = match &found.origin {
        Origin::Builtin => return patches_for(def, package),
        Origin::Dir(d) => d.clone(),
        Origin::Path(p) => p.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    let pdir = dir.join("patches").join(def).join(package);
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(&pdir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).ok()?)))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `apply_patch`: `patch -p0|-p1 --force -i <tmp>` in the package directory, output to the log,
/// the temporary file removed afterwards (plan Decision 5).
fn apply_patch(log: &BuildLog, dir: &Path, tmp: &Path, text: &[u8], env: &dyn Fn(&str) -> Option<String>) -> R<()> {
    let file = tmp.join(format!("python-patch.{}", std::process::id()));
    std::fs::write(&file, text).map_err(|_| InstallError::Failed)?;
    let level = if String::from_utf8_lossy(text).lines().any(|l| l.starts_with("diff --git a/")) { "-p1" } else { "-p0" };
    let r = logged(log, command("patch", env).current_dir(dir).args([level, "--force", "-i"]).arg(&file));
    let _ = std::fs::remove_file(&file);
    r
}

fn symlink_version_suffix(prefix: &Path, plan: &Plan) {
    if plan.altinstall {
        return;
    }
    let bin = prefix.join("bin");
    let Ok(rd) = std::fs::read_dir(&bin) else { return };
    let mut names: Vec<String> = rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let Some(version_bin) = names.iter().filter(|n| n.starts_with("python") && n.ends_with(|c: char| c.is_ascii_digit())).last() else { return };
    let suffix = version_bin["python".len()..].to_string();
    if suffix.is_empty() {
        return;
    }
    for name in &names {
        let link = if *name == format!("python{suffix}-config") {
            "python-config".to_string()
        } else if let Some(s) = name.strip_suffix(&format!("-{suffix}")) {
            s.to_string()
        } else if let Some(s) = name.strip_suffix(&suffix) {
            s.to_string()
        } else {
            continue;
        };
        if !link.is_empty() && bin.join(&link).symlink_metadata().is_err() {
            let _ = std::os::unix::fs::symlink(name, bin.join(&link));
        }
    }
}

fn colorize(word: &str) -> String {
    if std::io::stderr().is_terminal() { format!("\x1b[1m{word}\x1b[m") } else { word.to_string() }
}

fn verify(job: &Job, plan: &Plan, step: &str, python: &Path, say: &mut dyn FnMut(&str)) -> R<()> {
    let (xy, checks) = verify_plan(step).ok_or_else(|| InstallError::Message(format!("rpyenv cannot run build step `{step}'")))?;
    symlink_version_suffix(&job.prefix, plan);
    let exe = job.prefix.join("bin").join(format!("python{xy}"));
    if std::fs::metadata(&exe).map(|m| m.permissions().mode() & 0o111 == 0).unwrap_or(true) {
        say(&format!("{}: invalid Python executable: {}", colorize("ERROR"), exe.display()));
        say("");
        say("The python-build could not find proper executable of Python after successful build.");
        say("Please open an issue for future improvements.");
        say("https://github.com/pyenv/pyenv/issues");
        return Err(InstallError::Failed);
    }
    let display = (job.env)("DISPLAY").is_some_and(|d| !d.is_empty());
    for c in checks {
        if c.needs_display && !display {
            continue;
        }
        let ok = command(&python.display().to_string(), job.env)
            .args(["-c", &format!("import {}", c.module)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        if ok {
            continue;
        }
        let extra = c.extra.map(|e| format!(" {e}")).unwrap_or_default();
        if c.fatal {
            say(&format!("{}: The Python {} extension was not compiled. Missing the {}?", colorize("ERROR"), c.module, c.lib));
            say("");
            say("Please consult to the Wiki page to fix the problem.");
            say("https://github.com/pyenv/pyenv/wiki/Common-build-problems");
            say("");
            return Err(InstallError::Failed);
        }
        say(&format!("{}: The Python {} extension was not compiled{extra}. Missing the {}?", colorize("WARNING"), c.module, c.lib));
    }
    Ok(())
}

fn fix_directory_permissions(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            if m.is_dir() {
                let mode = m.permissions().mode();
                if mode & 0o022 != 0 {
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode & !0o022));
                }
                fix_directory_permissions(&p);
            }
        }
    }
}

/// Builds every package of the definition, in order, and places the version through `txn`
/// (not committed). Prints python-build's progress lines through `say` (stderr).
pub fn run(job: &Job, txn: &mut Txn, say: &mut dyn FnMut(&str)) -> R<()> {
    let tmp = tmp_dir(job.env).map_err(|m| { say(&m); InstallError::Failed })?;
    let seed = seed();
    let log_path = tmp.join(format!("python-build.{seed}.log"));
    let build_path = (job.env)("PYTHON_BUILD_BUILD_PATH").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| tmp.join(format!("python-build.{seed}")));
    let plan = Plan::new(job).map_err(|m| { say(&m); InstallError::Failed })?;
    if let Some(home) = (job.env)("HOME") {
        let cfg = Path::new(&home).join(".pydistutils.cfg");
        if cfg.exists() {
            say(&format!("{}: Please make sure you remove any previous custom paths from your {} file.", colorize("WARNING"), cfg.display()));
        }
    }
    let log = BuildLog::open(&log_path, job.opts.verbose).map_err(|e| InstallError::Message(format!("pyenv: cannot write {}: {e}", log_path.display())))?;
    std::fs::create_dir_all(&build_path).map_err(|e| InstallError::Message(format!("pyenv: cannot create {}: {e}", build_path.display())))?;
    let result = packages(job, txn, &plan, &build_path, &tmp, &log, say);
    match &result {
        Ok(()) if !job.opts.keep => {
            let _ = std::fs::remove_dir_all(&build_path);
        }
        Err(InstallError::Failed) => {
            for l in failed_block(&os_information(), &build_path, &log_path) {
                say(&l);
            }
        }
        Err(InstallError::Message(m)) => say(m),
        _ => {}
    }
    match result {
        Err(InstallError::Message(_)) => Err(InstallError::Failed),
        other => other,
    }
}

fn packages(job: &Job, txn: &mut Txn, plan: &Plan, build_path: &Path, tmp: &Path, log: &BuildLog, say: &mut dyn FnMut(&str)) -> R<()> {
    let python = job.prefix.join("bin").join(format!("python{}", plan.xy.clone().unwrap_or_default()));
    let mut stdin_patch = job.opts.stdin_patch.clone();
    for pkg in &job.definition.packages {
        match pkg.condition.as_deref() {
            None => {}
            // macOS-only bundled builds: false on Linux (reference, DSL table).
            Some("has_broken_mac_openssl" | "has_broken_mac_readline") => continue,
            Some(other) => return Err(InstallError::Message(format!("rpyenv cannot evaluate `--if {other}' for {}", pkg.name))),
        }
        let src = fetch_package(job, pkg, build_path, log, say)?;
        say(&format!("Installing {}...", pkg.name));
        let patch = if pkg.name.starts_with("Python-") && stdin_patch.is_some() {
            stdin_patch.take()
        } else {
            let files = patch_files(job.found, &pkg.name);
            (!files.is_empty()).then(|| files.into_iter().flat_map(|(_, b)| b).collect())
        };
        if let Some(text) = patch {
            apply_patch(log, &src, tmp, &text, job.env)?;
        }
        if job.definition.require_gcc && (job.env)("CC").is_none() {
            // Only the 2.1–2.4 definitions; a missing gcc fails at configure with its own error.
            log.line("require_gcc: using gcc from PATH");
        }
        for step in &pkg.steps {
            run_step(job, txn, plan, pkg, step, &src, &python, log, say)?;
        }
        if txn.placed() {
            fix_directory_permissions(&job.prefix);
        }
        say(&format!("Installed {} to {}", pkg.name, job.prefix.display()));
    }
    if !txn.placed() {
        return Err(InstallError::Message(format!("rpyenv: {} installed nothing", job.found.name)));
    }
    Ok(())
}

fn fetch_package(job: &Job, pkg: &Package, build_path: &Path, log: &BuildLog, say: &mut dyn FnMut(&str)) -> R<PathBuf> {
    match &pkg.fetch {
        Fetch::Tarball { url, fragment } => {
            let sha = sha256_of_fragment(url, fragment.as_deref()).map_err(InstallError::Message)?;
            let kind = Kind::of_url(url);
            let file_name = format!("{}{}", pkg.name, kind.ext());
            let req = FetchRequest { file_name, url: url.clone(), sha256: sha, dest_dir: build_path.to_path_buf() };
            let mut w: &BuildLog = log;
            let archive = job.fetcher.fetch(&req, &mut w, say)?;
            let dir = extract(&archive, kind, build_path, &pkg.name).map_err(|e| {
                log.line(&e);
                InstallError::Failed
            })?;
            if !job.opts.keep {
                let _ = std::fs::remove_file(&archive);
            }
            Ok(dir)
        }
        Fetch::Git { url, reference } => {
            let dir = build_path.join(&pkg.name);
            if Command::new("git").arg("--version").stdout(Stdio::null()).status().is_err() {
                return Err(InstallError::Message("error: please install `git` and try again".into()));
            }
            say(&format!("Cloning {url}..."));
            if dir.is_dir() {
                logged(log, command("git", job.env).current_dir(&dir).args(["fetch", "--depth", "1", "origin", &format!("+{reference}")]))?;
                logged(log, command("git", job.env).current_dir(&dir).args(["checkout", "-q", "-B", reference, &format!("origin/{reference}")]))?;
            } else {
                logged(log, command("git", job.env).current_dir(build_path).args(["clone", "--depth", "1", "--branch", reference, url, &pkg.name]))?;
            }
            Ok(dir)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_step(job: &Job, txn: &mut Txn, plan: &Plan, pkg: &Package, step: &str, src: &Path, python: &Path, log: &BuildLog, say: &mut dyn FnMut(&str)) -> R<()> {
    match step {
        "standard" => {
            let mut conf = command(&plan.configure[0], job.env);
            conf.current_dir(src).args(&plan.configure[1..]);
            for (k, v) in &plan.configure_env {
                conf.env(k, v);
            }
            logged(log, &mut conf)?;
            logged(log, command(&plan.make, job.env).current_dir(src).args(&plan.make_args))?;
            let mut install = command(&plan.make, job.env);
            install.current_dir(src).args(&plan.install_args);
            let staging = pkg.name.starts_with("Python-") && !txn.placed();
            if staging {
                install.arg(format!("DESTDIR={}", txn.stage_dir().display()));
            }
            logged(log, &mut install)?;
            if staging {
                let rel = job.prefix.strip_prefix("/").unwrap_or(&job.prefix);
                let staged = txn.stage_dir().join(rel);
                txn.place(&staged).map_err(|e| InstallError::Message(format!("pyenv: cannot move the build into {}: {e}", job.prefix.display())))?;
            }
            Ok(())
        }
        s if s.starts_with("verify_py") => verify(job, plan, s, python, say),
        "ensurepip" | "ensurepip_lt21" => {
            let isolation = if step == "ensurepip" { "-I" } else { "-s" };
            let mut c = command(&python.display().to_string(), job.env);
            c.args([isolation, "-m", "ensurepip"]);
            if plan.altinstall {
                c.arg("--altinstall");
            }
            let ok = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
            if interrupted() {
                return Err(InstallError::Interrupted);
            }
            if !ok {
                say("error: failed to install pip via ensurepip");
                return Err(InstallError::Failed);
            }
            symlink_version_suffix(&job.prefix, plan);
            Ok(())
        }
        "copy_python_gdb" => {
            let gdb = src.join("Tools/gdb/libpython.py");
            let v = pkg.name.split('-').nth(1).and_then(xy_of);
            if let (true, Some(v)) = (gdb.exists(), v) {
                let _ = std::fs::copy(&gdb, job.prefix.join("bin").join(format!("python{v}-gdb.py")));
            }
            Ok(())
        }
        "python" => logged(log, command(&python.display().to_string(), job.env).current_dir(src).args(["setup.py", "install"])),
        other => Err(InstallError::Message(format!("rpyenv cannot run build step `{other}'"))),
    }
}
```

Notes for the implementer:
- **Interface deviation:** `Fetcher::fetch` takes `log: &mut dyn Write`, so `fetch_package` passes `&mut &BuildLog`. If Task 1's signature makes that awkward, change `fetch`'s `log` parameter to `&mut dyn std::io::Write`. Keep it generic over the writer, and record the deviation.
- **`require_gcc`:** the CPython 2.1–2.4 definitions only. rpyenv relies on configure's own compiler check, so the line above only logs. This is part of the plan Decision 5 allowlist row for unsupported old-version details.

- [ ] **Step 6: Run the tests and see them pass**

Run in WSL: `cargo test -p pyenv --test install_build` and `cargo test -p pyenv --lib install::verify`.
Expected: 9 integration tests and 3 unit tests pass. `make` and `patch` must be installed in WSL (`command -v make patch`). If either is missing, say so instead of skipping the tests.

- [ ] **Step 7: Check that the tests can fail**

1. Temporarily remove `array.push("--with-ensurepip=no".into());`. `a_standard_build_installs_with_python_builds_flags_and_messages` must fail. Restore.
2. Temporarily change `if c.fatal {` to `if false {`. `a_missing_optional_module_warns_and_a_missing_ssl_fails_and_rolls_back` must fail. Restore.

- [ ] **Step 8: Lint and commit**

Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` (both OSes) and `python ci/shim_deps.py`.

```bash
git add crates Cargo.lock
git commit -m "Build CPython from python-build definitions into a staged prefix, with upstream's flags, checks and failure report"
```

---

### Task 6: Build pre-flight check

**Files:**
- Create: `crates/pyenv/src/install/preflight.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `#[cfg(unix)] pub mod preflight;`)
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` §13 (add `RPYENV_SKIP_PREFLIGHT`)

**Interfaces:**
- Produces:
  - `preflight::{Dep { name: &'static str, required: bool, missing: Missing }, Missing::{Program(String), Header(String)}, check(env: &dyn Fn(&str) -> Option<String>, needs_patch: bool) -> Vec<Dep>, evaluate(present: &dyn Fn(&Probe) -> bool, needs_patch: bool, display: bool) -> Vec<Dep>, report(missing: &[Dep], os_release: Option<&str>, root: bool) -> Report { refuse: bool, lines: Vec<String> }}`
  - `Probe::{Program(&str), Headers(&[&str])}`
  - The returned `Vec<Dep>` is the structured result spec §15.2 asks for. M8 builds on it.

- [ ] **Step 1: Write the failing unit tests (in the module)**

Create `crates/pyenv/src/install/preflight.rs` with the tests first:

```rust
//! The build pre-flight check (spec §9.2, plan Decision 8): a compiler, `make`, `patch` when
//! needed, and the headers of the modules python-build verifies. Missing required pieces
//! refuse the build before downloading; missing optional ones warn. Results are structured
//! for M8 (spec §15.2).

use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    Program(String),
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// What the user loses: `C compiler`, `make`, `patch`, or a module name such as `ssl`.
    pub name: &'static str,
    pub required: bool,
    pub missing: Missing,
}

pub enum Probe<'a> {
    Program(&'a str),
    /// Any one of these headers is enough.
    Headers(&'a [&'a str]),
}

/// (name, required, probe, apt, dnf, zypper, pacman, apk)
type Row = (&'static str, bool, Probe<'static>, [&'static str; 5]);

fn rows(compiler: &'static str, make: &'static str) -> Vec<Row> {
    vec![
        ("C compiler", true, Probe::Program(compiler), ["build-essential", "gcc", "gcc", "base-devel", "build-base"]),
        ("make", true, Probe::Program(make), ["make", "make", "make", "make", "make"]),
        ("patch", true, Probe::Program("patch"), ["patch", "patch", "patch", "patch", "patch"]),
        ("ssl", true, Probe::Headers(&["openssl/ssl.h"]), ["libssl-dev", "openssl-devel", "libopenssl-devel", "openssl", "openssl-dev"]),
        ("zlib", true, Probe::Headers(&["zlib.h"]), ["zlib1g-dev", "zlib-devel", "zlib-devel", "zlib", "zlib-dev"]),
        ("bz2", false, Probe::Headers(&["bzlib.h"]), ["libbz2-dev", "bzip2-devel", "libbz2-devel", "bzip2", "bzip2-dev"]),
        ("readline", false, Probe::Headers(&["readline/readline.h", "editline/readline.h"]), ["libreadline-dev", "readline-devel", "readline-devel", "readline", "readline-dev"]),
        ("sqlite3", false, Probe::Headers(&["sqlite3.h"]), ["libsqlite3-dev", "sqlite-devel", "sqlite3-devel", "sqlite", "sqlite-dev"]),
        ("ctypes", false, Probe::Headers(&["ffi.h"]), ["libffi-dev", "libffi-devel", "libffi-devel", "libffi", "libffi-dev"]),
        ("curses", false, Probe::Headers(&["ncurses.h", "curses.h"]), ["libncurses-dev", "ncurses-devel", "ncurses-devel", "ncurses", "ncurses-dev"]),
        ("lzma", false, Probe::Headers(&["lzma.h"]), ["liblzma-dev", "xz-devel", "xz-devel", "xz", "xz-dev"]),
        ("tkinter", false, Probe::Headers(&["tk.h"]), ["tk-dev", "tk-devel", "tk-devel", "tk", "tk-dev"]),
    ]
}

/// Which missing items there are, given a way to test each probe. `display`: tkinter is only
/// checked by upstream when `$DISPLAY` is set, so it is only probed then.
pub fn evaluate(present: &dyn Fn(&Probe) -> bool, needs_patch: bool, display: bool) -> Vec<Dep> {
    evaluate_with(present, needs_patch, display, "cc", "make")
}

fn evaluate_with(present: &dyn Fn(&Probe) -> bool, needs_patch: bool, display: bool, cc: &'static str, make: &'static str) -> Vec<Dep> {
    let mut out = Vec::new();
    let mut compiler_ok = true;
    for (name, required, probe, _) in rows(cc, make) {
        if (name == "patch" && !needs_patch) || (name == "tkinter" && !display) {
            continue;
        }
        // Without a compiler the headers can't be probed; report the compiler alone.
        if matches!(probe, Probe::Headers(_)) && !compiler_ok {
            continue;
        }
        if !present(&probe) {
            if name == "C compiler" {
                compiler_ok = false;
            }
            let missing = match probe {
                Probe::Program(p) => Missing::Program(p.to_string()),
                Probe::Headers(h) => Missing::Header(h[0].to_string()),
            };
            out.push(Dep { name, required, missing });
        }
    }
    out
}

pub struct Report {
    pub refuse: bool,
    pub lines: Vec<String>,
}

fn manager(os_release: &str) -> Option<(usize, &'static str)> {
    let ids: Vec<String> = os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| v.trim_matches('"').split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect();
    let has = |names: &[&str]| ids.iter().any(|i| names.contains(&i.as_str()));
    if has(&["debian", "ubuntu"]) {
        Some((0, "apt-get install"))
    } else if has(&["fedora", "rhel", "centos"]) {
        Some((1, "dnf install"))
    } else if has(&["suse", "opensuse", "sles"]) || ids.iter().any(|i| i.starts_with("opensuse")) {
        Some((2, "zypper install"))
    } else if has(&["arch"]) {
        Some((3, "pacman -S"))
    } else if has(&["alpine"]) {
        Some((4, "apk add"))
    } else {
        None
    }
}

fn shown(d: &Dep) -> String {
    let what = match &d.missing {
        Missing::Program(p) => p.clone(),
        Missing::Header(h) => h.clone(),
    };
    format!("{} ({what})", d.name)
}

/// The text for stderr. `root`: no `sudo` in the suggested command.
pub fn report(missing: &[Dep], os_release: Option<&str>, root: bool) -> Report {
    let refuse = missing.iter().any(|d| d.required);
    let mut lines = Vec::new();
    if missing.is_empty() {
        return Report { refuse, lines };
    }
    if refuse {
        lines.push("pyenv: cannot build Python: missing build dependencies:".to_string());
    } else {
        lines.push("pyenv: the build will lack these optional modules:".to_string());
    }
    for d in missing {
        lines.push(format!("  {}", shown(d)));
    }
    if let Some((col, cmd)) = os_release.and_then(manager) {
        let mut pkgs: Vec<&str> = Vec::new();
        for d in missing {
            if let Some(row) = rows("cc", "make").into_iter().find(|r| r.0 == d.name) {
                if !pkgs.contains(&row.3[col]) {
                    pkgs.push(row.3[col]);
                }
            }
        }
        lines.push("Install them with:".to_string());
        lines.push(format!("  {}{cmd} {}", if root { "" } else { "sudo " }, pkgs.join(" ")));
    }
    if refuse {
        lines.push("To build anyway, set RPYENV_SKIP_PREFLIGHT=1.".to_string());
    }
    Report { refuse, lines }
}

/// Probes the real system: `$CC` (else `cc`), `$MAKE` (else `make`), and each header through
/// `<cc> -E` with `CPPFLAGS` and `PYTHON_CPPFLAGS`.
pub fn check(env: &dyn Fn(&str) -> Option<String>, needs_patch: bool) -> Vec<Dep> {
    let cc = env("CC").filter(|v| !v.is_empty()).unwrap_or_else(|| "cc".into());
    let make = env("MAKE").filter(|v| !v.is_empty()).unwrap_or_else(|| "make".into());
    let flags: Vec<String> = [env("CPPFLAGS"), env("PYTHON_CPPFLAGS")].into_iter().flatten().flat_map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>()).collect();
    let path = env("PATH");
    // `$CC` may carry words (`ccache gcc`): the first is the program, the rest lead the args.
    let runs = |program: &str, args: &[&str], input: Option<&str>| -> bool {
        let mut words = program.split_whitespace();
        let Some(first) = words.next() else { return false };
        let mut c = Command::new(first);
        c.args(words);
        if let Some(p) = &path {
            c.env("PATH", p);
        }
        c.args(args).stdout(Stdio::null()).stderr(Stdio::null());
        c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
        let Ok(mut child) = c.spawn() else { return false };
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        child.wait().map(|s| s.success()).unwrap_or(false)
    };
    let present = |p: &Probe| match p {
        Probe::Program(name) => {
            let name = if *name == "cc" { cc.as_str() } else if *name == "make" { make.as_str() } else { name };
            runs(name, &["--version"], None)
        }
        Probe::Headers(hs) => hs.iter().any(|h| {
            let mut args: Vec<&str> = flags.iter().map(String::as_str).collect();
            args.extend(["-E", "-x", "c", "-o", "/dev/null", "-"]);
            runs(&cc, &args, Some(&format!("#include <{h}>\n")))
        }),
    };
    let display = env("DISPLAY").is_some_and(|d| !d.is_empty());
    evaluate(&present, needs_patch, display)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn missing_only(names: &'static [&'static str]) -> impl Fn(&Probe) -> bool {
        move |p: &Probe| match p {
            Probe::Program(n) => !names.contains(n),
            Probe::Headers(hs) => !hs.iter().all(|h| names.contains(h)),
        }
    }

    const DEBIAN: &str = "PRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nID=debian\n";
    const UBUNTU: &str = "ID=ubuntu\nID_LIKE=debian\n";
    const FEDORA: &str = "ID=fedora\n";

    #[test]
    fn nothing_missing_means_no_report() {
        let deps = evaluate(&|_: &Probe| true, true, true);
        assert!(deps.is_empty());
        let r = report(&deps, Some(DEBIAN), false);
        assert!(!r.refuse && r.lines.is_empty());
    }

    #[test]
    fn missing_ssl_refuses_with_the_debian_package() {
        let deps = evaluate(&missing_only(&["openssl/ssl.h"]), false, false);
        assert_eq!(deps, vec![Dep { name: "ssl", required: true, missing: Missing::Header("openssl/ssl.h".into()) }]);
        let r = report(&deps, Some(DEBIAN), false);
        assert!(r.refuse);
        assert_eq!(r.lines, [
            "pyenv: cannot build Python: missing build dependencies:",
            "  ssl (openssl/ssl.h)",
            "Install them with:",
            "  sudo apt-get install libssl-dev",
            "To build anyway, set RPYENV_SKIP_PREFLIGHT=1.",
        ]);
    }

    #[test]
    fn optional_modules_warn_only() {
        let deps = evaluate(&missing_only(&["bzlib.h", "lzma.h"]), false, false);
        let r = report(&deps, Some(FEDORA), true);
        assert!(!r.refuse);
        assert_eq!(r.lines, [
            "pyenv: the build will lack these optional modules:",
            "  bz2 (bzlib.h)",
            "  lzma (lzma.h)",
            "Install them with:",
            "  dnf install bzip2-devel xz-devel",
        ]);
    }

    #[test]
    fn readline_accepts_libedit_and_tk_is_probed_only_with_a_display() {
        assert!(evaluate(&missing_only(&["readline/readline.h"]), false, false).is_empty());
        assert!(evaluate(&missing_only(&["tk.h"]), false, false).is_empty());
        assert_eq!(evaluate(&missing_only(&["tk.h"]), false, true).len(), 1);
    }

    #[test]
    fn a_missing_compiler_hides_the_header_probes() {
        let deps = evaluate(&missing_only(&["cc", "openssl/ssl.h"]), false, false);
        assert_eq!(deps.iter().map(|d| d.name).collect::<Vec<_>>(), ["C compiler"]);
        let r = report(&deps, Some(UBUNTU), false);
        assert!(r.lines.contains(&"  sudo apt-get install build-essential".to_string()));
    }

    #[test]
    fn a_compiler_with_a_wrapper_is_probed_as_program_plus_args() {
        // CI sets CC="ccache gcc"; `ccache` must run with `gcc --version`, not a program
        // named "ccache gcc". `sh -c true` stands in: one program, leading args.
        let env = |k: &str| match k {
            "CC" => Some("sh -c true".to_string()),
            "PATH" => std::env::var("PATH").ok(),
            _ => None,
        };
        let deps = check(&env, false);
        assert!(!deps.iter().any(|d| d.name == "C compiler"), "{deps:?}");
    }

    #[test]
    fn patch_is_needed_only_with_patches_and_unknown_distros_get_no_command() {
        assert!(evaluate(&missing_only(&["patch"]), false, false).is_empty());
        let deps = evaluate(&missing_only(&["patch"]), true, false);
        let r = report(&deps, Some("ID=gentoo\n"), false);
        assert_eq!(r.lines, ["pyenv: cannot build Python: missing build dependencies:", "  patch (patch)", "To build anyway, set RPYENV_SKIP_PREFLIGHT=1."]);
    }
}
```

`evaluate` matches the compiler by the probe name `cc`. In `check`, the closure maps `cc` and `make` to `$CC` and `$MAKE`, so `evaluate` stays testable with fixed names.

- [ ] **Step 2: Run them and see them pass**

Run in WSL: `cargo test -p pyenv --lib install::preflight`.
Expected: 7 passed. The `#[cfg(unix)]` gate compiles the module out on Windows. `sh -c true` also answers the header probes (it ignores stdin and exits 0), so no header is reported missing either.

- [ ] **Step 3: Probe the real system once**

Add this test to the same module:

```rust
    #[test]
    #[ignore = "probes this machine; run by hand"]
    fn probe_this_machine() {
        let env = |k: &str| std::env::var(k).ok();
        let deps = check(&env, true);
        let os = std::fs::read_to_string("/etc/os-release").ok();
        for l in report(&deps, os.as_deref(), false).lines {
            eprintln!("{l}");
        }
        eprintln!("{deps:?}");
    }
```

Run it in WSL with `cargo test -p pyenv --lib preflight::tests::probe_this_machine -- --ignored --nocapture`. Paste its output into the report. The WSL host built CPython 3.12 in the reference probes, so `[]` is expected. Anything else is a finding to report.

- [ ] **Step 4: Spec §13 row**

In the §13 table of `docs/specs/2026-09-27-rpyenv-design.md`, after the `RPYENV_CATALOG_URL` row, add:

```markdown
| `RPYENV_SKIP_PREFLIGHT` | rpyenv | `1` skips the Linux build pre-flight check (§9.2), for headers in places the check doesn't look |
```

- [ ] **Step 5: Lint and commit**

```bash
git add crates docs/specs/2026-09-27-rpyenv-design.md
git commit -m "Check for a compiler, make, patch and module headers before a Linux build, with distro package names"
```

---
### Task 7: `pyenv install` and built-in default packages

**Files:**
- Create: `crates/pyenv/src/commands/install.rs`, `crates/pyenv/src/install/default_packages.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (`#[cfg(unix)] pub mod default_packages;`), `crates/pyenv/src/commands/mod.rs` (`#[cfg(unix)] pub mod install;` plus a `LINUX_ONLY` row), `crates/pyenv/src/help.rs` (the `install` topic)
- Create: `crates/pyenv/tests/cli_install.rs`

**Interfaces:**
- Consumes everything from Tasks 1–6:
  - `defs::{names, find, parse}`;
  - `commands::latest::resolve_known`;
  - `txn::{Txn, is_complete}`;
  - `builder::{run, Job, Options, patch_files}`;
  - `preflight::{check, report}`;
  - `fetch::Fetcher`.
- Produces:
  - `commands::install::install(&Ctx, &[&str]) -> Output`;
  - `default_packages::run(root: &Path, prefix: &Path) -> Option<String>`, which returns the error line, if any.

- [ ] **Step 1: Write the failing CLI tests**

Create `crates/pyenv/tests/cli_install.rs`:

```rust
//! `pyenv install` end to end with a fake CPython over a local server (spec §12.5 tier 1),
//! against docs/parity/pyenv-m2-reference.md "pyenv install".
#![cfg(unix)]

mod common;

use common::fakebuild::tarball;
use common::server::{start, Reply, Server};
use common::Fixture;
use std::path::PathBuf;

/// A plugin definition `3.12.99` (so `pyenv install 3.12` resolves to it) serving the fake
/// tarball; returns the server so its hit counts can be checked.
fn plugin_def(f: &Fixture) -> Server {
    let body = tarball("3.12.99");
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("t");
    std::fs::write(&p, &body).unwrap();
    let sha = pyenv::install::checksum::sha256_file(&p).unwrap();
    let s = start(vec![("/Python-3.12.99.tar.gz", vec![Reply::Body(body)])]);
    f.file(
        &f.root.join("plugins/fake/share/python-build/3.12.99"),
        &format!("install_package \"Python-3.12.99\" \"{}#{sha}\" standard verify_py312 ensurepip\n", s.url("/Python-3.12.99.tar.gz")),
    );
    s
}

fn env<'a>(f: &'a Fixture, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
    let mut v: Vec<(&str, String)> = vec![
        ("PATH", format!("{}:{}", f.syspath.display(), std::env::var("PATH").unwrap())),
        ("TMPDIR", f.base.join("tmp").display().to_string()),
        ("PYTHON_BUILD_SKIP_MIRROR", "1".into()),
        ("RPYENV_SKIP_PREFLIGHT", "1".into()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

fn run(f: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> common::Run {
    let owned = env(f, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_env(args, &pairs)
}

fn staging_left(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.root.join("versions")).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect()
}

#[test]
fn a_prefix_resolves_to_a_plugin_definition_builds_and_rehashes() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    let r = run(&f, &["install", "3.12"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let p = f.root.join("versions/3.12.99");
    assert_eq!(
        r.stderr,
        format!("Downloading Python-3.12.99.tar.gz...\n-> {}\nInstalling Python-3.12.99...\nInstalled Python-3.12.99 to {}\n", s.url("/Python-3.12.99.tar.gz"), p.display())
    );
    assert_eq!(r.stdout, "");
    assert!(f.root.join("shims/python3.12").exists(), "rehashed after the install");
    assert!(staging_left(&f).is_empty());
}

#[test]
fn an_alias_installs_under_its_name() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "3.12.99:mine"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f.root.join("versions/mine/bin/python3.12").is_file());
}

#[test]
fn an_existing_version_prompts_and_eof_stops_the_run() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    f.version("3.12.99/bin");
    let r = run(&f, &["install", "3.12.99", "3.12.99:other"], &[]);
    assert_eq!((r.stderr.as_str(), r.code), (format!("pyenv: {} already exists\n", f.root.join("versions/3.12.99").display()).as_str(), 1));
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 0);
    assert!(!f.root.join("versions/other").exists(), "EOF ends the whole run, as upstream");
}

#[test]
fn skip_existing_is_silent_and_force_rebuilds() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    f.version("3.12.99/bin");
    f.file(&f.root.join("versions/3.12.99/bin/old"), "");
    let r = run(&f, &["install", "-sf", "3.12.99"], &[]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0), "-s wins over -f");
    let r = run(&f, &["install", "-f", "3.12.99"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 1);
    assert!(!f.root.join("versions/3.12.99/bin/old").exists(), "replaced, not built over");
}

#[test]
fn an_unknown_version_prints_upstreams_hint_and_exits_2() {
    let f = Fixture::new();
    let r = run(&f, &["install", "9.9.9"], &[]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("python-build: definition not found: 9.9.9\n\nSee all available versions with `pyenv install --list'.\n\nIf the version you need is missing, try upgrading pyenv.\n", 2)
    );
    let r = run(&f, &["install", "3.15.0"], &[]);
    assert_eq!(
        r.stderr,
        "python-build: definition not found: 3.15.0\n\nThe following versions contain `3.15.0' in the name:\n  3.15.0rc2\n  3.15.0rc2t\n\nSee all available versions with `pyenv install --list'.\n\nIf the version you need is missing, try upgrading pyenv.\n"
    );
}

#[test]
fn list_prints_the_definitions() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "--list"], &[]);
    assert!(r.stdout.starts_with("Available versions:\n  2.1.3\n"), "{}", &r.stdout[..80]);
    assert!(r.stdout.contains("\n  3.12.14\n") && r.stdout.contains("\n  3.12.99\n"));
    let bare = run(&f, &["install", "-l", "--bare"], &[]);
    assert!(bare.stdout.starts_with("2.1.3\n"));
    assert_eq!(bare.stdout.lines().count() + 1, r.stdout.lines().count());
}

#[test]
fn usage_errors_and_version() {
    let f = Fixture::new();
    let r = run(&f, &["install"], &[]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.starts_with("Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n"), "{}", r.stderr);
    let r = run(&f, &["install", "-x", "--help"], &[]);
    assert_eq!(r.code, 1, "options are handled in order");
    let r = run(&f, &["install", "--help", "-x"], &[]);
    assert_eq!(r.code, 0);
    let r = run(&f, &["install", "--version"], &[]);
    assert_eq!(r.stdout, format!("python-build 2.8.6 (rpyenv {})\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn with_no_arguments_the_local_version_file_is_used() {
    let f = Fixture::new();
    plugin_def(&f);
    f.file(&f.work.join(".python-version"), "3.12.99\n");
    let r = run(&f, &["install"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f.root.join("versions/3.12.99/bin").is_dir());
    let global_only = Fixture::new();
    global_only.file(&global_only.root.join("version"), "3.12.99\n");
    assert_eq!(run(&global_only, &["install"], &[]).code, 1, "the global file is not read");
}

#[test]
fn default_packages_run_in_the_new_version_and_a_failure_still_succeeds() {
    let f = Fixture::new();
    plugin_def(&f);
    f.file(&f.root.join("default-packages"), "requests\n");
    let log = f.base.join("pip.log");
    let log_s = log.display().to_string();
    let r = run(&f, &["install", "3.12.99:dp"], &[("FAKE_PIP_LOG", log_s.as_str())]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), format!("-m pip install -r {}\n", f.root.join("default-packages").display()));
    let r = run(&f, &["install", "3.12.99:dp2"], &[("FAKE_PIP_FAIL", "1")]);
    assert_eq!(r.code, 0);
    assert!(r.stderr.ends_with(&format!("pyenv: error installing packages from  `{}'\n", f.root.join("default-packages").display())), "{}", r.stderr);
}

#[test]
fn a_failed_build_exits_1_and_stops_at_the_first_failure() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "3.12.99", "3.12.99:second"], &[("FAKE_CONFIGURE_FAIL", "1")]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("BUILD FAILED"), "{}", r.stderr);
    assert!(!f.root.join("versions/3.12.99").exists() && !f.root.join("versions/second").exists());
    assert!(staging_left(&f).is_empty());
}

/// Review focus 3: Ctrl+C mid-build exits 130 and leaves nothing behind, or restores what
/// was there.
#[test]
fn ctrl_c_rolls_back_and_exits_130() {
    use std::os::unix::process::CommandExt;
    for preexisting in [false, true] {
        let f = Fixture::new();
        plugin_def(&f);
        if preexisting {
            f.file(&f.root.join("versions/3.12.99/bin/old"), "");
        }
        let owned = env(&f, &[("FAKE_MAKE_SLEEP", "30")]);
        let mut cmd = f.command(std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")), &f.work, &[]);
        for (k, v) in &owned {
            cmd.env(k, v);
        }
        let mut args = vec!["install".to_string()];
        if preexisting {
            args.push("-f".into());
        }
        args.push("3.12.99".into());
        let mut child = cmd.args(&args).process_group(0).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let lock: PathBuf = f.root.join("versions/.lock-3.12.99");
        let start = std::time::Instant::now();
        while !lock.exists() && start.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::Command::new("kill").args(["-INT", &format!("-{}", child.id())]).status().unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(130), "preexisting={preexisting}");
        assert!(staging_left(&f).is_empty(), "{:?}", staging_left(&f));
        assert_eq!(f.root.join("versions/3.12.99/bin/old").is_file(), preexisting);
        if !preexisting {
            assert!(!f.root.join("versions/3.12.99").exists());
        }
    }
}
```

`Fixture::command` is already public (`crates/pyenv/tests/common/mod.rs:100`). `Run` is `common::Run`.

Run in WSL: `cargo test -p pyenv --test cli_install`.
Expected: every test fails with `pyenv: no such command `install'`.

- [ ] **Step 2: Implement default packages**

Create `crates/pyenv/src/install/default_packages.rs`:

```rust
//! The pyenv-default-packages plugin's behavior, built in (spec §9.3): after a successful
//! install, `pip install -r $PYENV_ROOT/default-packages` in the new version. It runs pip in the
//! installed directory, so an alias gets the packages too (plan Decision 5).

use std::path::Path;
use std::process::Command;

/// None on success or when there is no file; otherwise the error line for stderr.
pub fn run(root: &Path, prefix: &Path) -> Option<String> {
    let file = root.join("default-packages");
    if !file.is_file() {
        return None;
    }
    let bin = prefix.join("bin");
    let python = ["python", "python3"].iter().map(|n| bin.join(n)).find(|p| p.exists()).or_else(|| {
        std::fs::read_dir(&bin).ok()?.filter_map(Result::ok).map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("python") && n.to_string_lossy().ends_with(|c: char| c.is_ascii_digit())))
            .max()
    })?;
    let ok = Command::new(&python).args(["-m", "pip", "install", "-r"]).arg(&file).status().map(|s| s.success()).unwrap_or(false);
    (!ok).then(|| format!("pyenv: error installing packages from  `{}'", file.display()))
}
```

- [ ] **Step 3: Implement the command**

Create `crates/pyenv/src/commands/install.rs`:

```rust
//! `pyenv install` (docs/parity/pyenv-m2-reference.md "pyenv install"), Linux flavor.

use crate::commands::latest::resolve_known;
use crate::install::builder::{self, patch_files, Job, Options};
use crate::install::defs::{self, Found, Origin};
use crate::install::fetch::Fetcher;
use crate::install::txn::{is_complete, Txn};
use crate::install::{default_packages, preflight, watch_interrupt, InstallError};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::verfile;
use std::io::{BufRead, IsTerminal, Read};
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n       pyenv install [-f] [-kvp] <definition-file>[:<alias>]\n       pyenv install -l|--list [--bare]\n       pyenv install --version\n\n  -l/--list          List all available versions\n  -f/--force         Install even if the version appears to be installed already\n  -s/--skip-existing Skip if the version appears to be installed already\n\n  python-build options:\n\n  -k/--keep          Keep source tree in $PYENV_BUILD_ROOT after installation\n                     (defaults to $PYENV_ROOT/sources)\n  -p/--patch         Apply a patch from stdin before building\n  -v/--verbose       Verbose mode: print compilation status to stdout\n  --version          Show version of python-build\n  -g/--debug         Build a debug version\n\n  Append `:<alias>' to a version to install it under a custom name, so that\n  several builds of the same version can coexist:\n\n      pyenv install 3.12.0:my-3.12\n\n  This installs into $PYENV_ROOT/versions/my-3.12.\n\nFor detailed information on installing Python versions with\npython-build, including a list of environment variables for adjusting\ncompilation, see: https://github.com/pyenv/pyenv#readme\n\n";
pub const USAGE: &str = "Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n       pyenv install [-f] [-kvp] <definition-file>[:<alias>]\n       pyenv install -l|--list [--bare]\n       pyenv install --version";

fn say(line: &str) {
    rpyenv_core::textout::write(true, &format!("{line}\n"));
}

#[derive(Default)]
struct Flags {
    list: bool,
    bare: bool,
    force: bool,
    skip: bool,
    keep: bool,
    verbose: bool,
    patch: bool,
    debug: bool,
}

enum Early {
    Help,
    Usage,
    Version,
}

/// python-build's `parse_options`: every `-…` argument is options (`--name` or `-abc`),
/// handled in order; the rest are positional.
fn parse<'a>(args: &[&'a str]) -> Result<(Flags, Vec<&'a str>), Early> {
    let mut f = Flags::default();
    let mut pos = Vec::new();
    for a in args {
        let names: Vec<String> = if let Some(long) = a.strip_prefix("--") {
            vec![long.to_string()]
        } else if let Some(short) = a.strip_prefix('-') {
            short.chars().map(String::from).collect()
        } else {
            pos.push(*a);
            continue;
        };
        for n in names {
            match n.as_str() {
                "h" | "help" => return Err(Early::Help),
                "bare" => f.bare = true,
                "l" | "list" => f.list = true,
                "f" | "force" => f.force = true,
                "s" | "skip-existing" => f.skip = true,
                "k" | "keep" => f.keep = true,
                "v" | "verbose" => f.verbose = true,
                "p" | "patch" => f.patch = true,
                "g" | "debug" => f.debug = true,
                "version" => return Err(Early::Version),
                _ => return Err(Early::Usage),
            }
        }
    }
    Ok((f, pos))
}

/// `read -p`: the prompt shows only on a terminal; None on EOF.
fn prompt(text: &str) -> Option<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        rpyenv_core::textout::write(true, text);
    }
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
    }
}

fn not_found(definition: &str, names: &[String]) {
    say(&format!("python-build: definition not found: {definition}"));
    let matches: Vec<&String> = names.iter().filter(|n| n.contains(definition)).collect();
    if !matches.is_empty() {
        say("");
        say(&format!("The following versions contain `{definition}' in the name:"));
        for n in matches {
            say(&format!("  {n}"));
        }
    }
    say("");
    say("See all available versions with `pyenv install --list'.");
    say("");
    say("If the version you need is missing, try upgrading pyenv.");
}

fn needs_patch(found: &Found, def: &defs::Definition) -> bool {
    def.packages.iter().any(|p| p.name.starts_with("Python-") && !patch_files(found, &p.name).is_empty())
}

pub fn install(ctx: &Ctx, args: &[&str]) -> Output {
    let env = |k: &str| std::env::var(k).ok();
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        for opt in ["--bare", "--list", "--force", "--skip-existing", "--keep", "--patch", "--verbose", "--version", "--debug"] {
            o.out(opt);
        }
        for n in defs::names(&ctx.root, &env) {
            o.out(n);
        }
        return o;
    }
    let (mut f, positional) = match parse(args) {
        Ok(v) => v,
        Err(Early::Help) => return Output { stdout: HELP.into(), ..Output::new() },
        Err(Early::Usage) => return Output { stderr: HELP.into(), code: 1, ..Output::new() },
        Err(Early::Version) => {
            let mut o = Output::new();
            o.out(format!("python-build {} (rpyenv {})", defs::UPSTREAM_VERSION, env!("CARGO_PKG_VERSION")));
            return o;
        }
    };
    if env("PYENV_DEBUG").is_some_and(|v| !v.is_empty()) {
        f.verbose = true;
    }
    let names = defs::names(&ctx.root, &env);
    if f.list {
        let mut o = Output::new();
        if !f.bare {
            o.out("Available versions:");
        }
        for n in &names {
            o.out(if f.bare { n.clone() } else { format!("  {n}") });
        }
        return o;
    }
    let wanted: Vec<String> = if positional.is_empty() {
        verfile::find_local(&ctx.pwd)
            .map(|file| verfile::read_pyenv(&file, &file.display().to_string(), &ctx.versions_dir()).versions)
            .unwrap_or_default()
    } else {
        positional.iter().map(|s| s.to_string()).collect()
    };
    if wanted.is_empty() {
        return Output { stderr: HELP.into(), code: 1, ..Output::new() };
    }
    watch_interrupt();
    let mut status = 0;
    let mut stdin_patch: Option<Vec<u8>> = None;
    for arg in &wanted {
        // Alias: the text after the last `:`, unless it is `latest`.
        let (mut definition, alias) = match arg.rsplit_once(':') {
            Some((d, a)) if a != "latest" => (d.to_string(), Some(a.to_string())),
            _ => (arg.clone(), None),
        };
        if let Some(prefix) = definition.strip_suffix(":latest") {
            match resolve_known(prefix, &names) {
                Some(v) => definition = v,
                None => {
                    say(&format!("pyenv: no known versions match the prefix `{prefix}'"));
                    status = status.max(1);
                    break;
                }
            }
        } else if let Some(v) = resolve_known(&definition, &names) {
            definition = v;
        }
        let base = Path::new(&definition).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let version_name = if f.debug { format!("{base}-debug") } else { base };
        let name = alias.unwrap_or(version_name);
        let prefix = ctx.versions_dir().join(&name);
        if is_complete(&prefix) {
            if f.skip {
                continue;
            }
            if !f.force {
                say(&format!("pyenv: {} already exists", prefix.display()));
                match prompt("continue with installation? (y/N) ") {
                    None => return Output::new().with_code(1),
                    Some(r) if ["y", "Y", "yes", "YES"].contains(&r.as_str()) => {}
                    Some(_) => {
                        status = status.max(1);
                        continue;
                    }
                }
            }
        }
        let Some(found) = defs::find(&ctx.root, &definition, &env) else {
            not_found(&definition, &names);
            status = status.max(2);
            break;
        };
        let root = ctx.root.clone();
        let here = match &found.origin {
            Origin::Path(p) => p.parent().map(Path::to_path_buf),
            _ => None,
        };
        let sibling = |n: &str| match &here {
            Some(dir) => defs::find(&root, &dir.join(n).display().to_string(), &env),
            None => defs::find(&root, n, &env),
        };
        let parsed = match defs::parse(&found, &env, &sibling) {
            Ok(d) => d,
            Err(e) => {
                say(&e);
                status = status.max(1);
                break;
            }
        };
        if env("RPYENV_SKIP_PREFLIGHT").as_deref() != Some("1") {
            let missing = preflight::check(&env, f.patch || needs_patch(&found, &parsed));
            let os = std::fs::read_to_string("/etc/os-release").ok();
            let r = preflight::report(&missing, os.as_deref(), env("USER").as_deref() == Some("root"));
            for l in &r.lines {
                say(l);
            }
            if r.refuse {
                status = status.max(1);
                break;
            }
        }
        let mut txn = match Txn::begin(&ctx.versions_dir(), &name) {
            Ok(t) => t,
            Err(m) => {
                say(&m);
                status = status.max(1);
                break;
            }
        };
        let build_root = env("PYENV_BUILD_ROOT").filter(|v| !v.is_empty()).or_else(|| f.keep.then(|| ctx.root.join("sources").display().to_string()));
        let build_path = build_root.as_ref().map(|r| Path::new(r).join(&name).display().to_string());
        let job_env = |k: &str| if k == "PYTHON_BUILD_BUILD_PATH" && build_path.is_some() { build_path.clone() } else { env(k) };
        let cache = env("PYTHON_BUILD_CACHE_PATH")
            .filter(|p| Path::new(p).is_dir())
            .map(PathBuf::from)
            .or_else(|| Some(ctx.root.join("cache")).filter(|c| c.is_dir()));
        let fetcher = Fetcher::from_env(&job_env, cache);
        if f.patch && stdin_patch.is_none() {
            let mut buf = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut buf);
            stdin_patch = Some(buf);
        }
        let job = Job {
            found: &found,
            definition: &parsed,
            prefix: prefix.clone(),
            opts: Options { keep: build_root.is_some(), verbose: f.verbose, debug: f.debug, stdin_patch: stdin_patch.take() },
            env: &job_env,
            fetcher: &fetcher,
        };
        match builder::run(&job, &mut txn, &mut |s: &str| say(s)) {
            Ok(()) => {
                if let Err(e) = txn.commit() {
                    say(&format!("pyenv: cannot finish installing {}: {e}", prefix.display()));
                    status = status.max(1);
                    break;
                }
                if let Some(line) = default_packages::run(&ctx.root, &prefix) {
                    say(&line);
                }
                let r = crate::commands::rehash::rehash(ctx, &[]);
                if r.code != 0 {
                    r.emit(ctx.flavor);
                    status = status.max(r.code);
                }
            }
            Err(InstallError::Interrupted) => {
                drop(txn);
                return Output::new().with_code(130);
            }
            Err(_) => {
                drop(txn);
                status = status.max(1);
                break;
            }
        }
    }
    Output::new().with_code(status)
}
```

Wire it up:
1. `crates/pyenv/src/commands/mod.rs`: add `#[cfg(unix)] pub mod install;`. In `LINUX_ONLY`, add `#[cfg(unix)] ("install", install::install),`. An attribute on an array element is allowed; if it isn't, build the table with a `cfg`'d `const`.
2. `crates/pyenv/src/help.rs`: add `topic("install", Some("Install a Python version using python-build"), Some(<USAGE>), <HELP>)` with the same strings as `HELP`/`USAGE` above, in the table's alphabetical position. If `help.rs` can reference `crate::commands::install::HELP` under `cfg(unix)`, do that instead of copying.

`Output { stdout: …, ..Output::new() }` uses struct-update syntax; `Output`'s fields are public (`crates/pyenv/src/output.rs`).

- [ ] **Step 4: Run the tests and see them pass**

Run in WSL: `cargo test -p pyenv --test cli_install`, then `cargo test --workspace` on both OSes.
Expected: 11 passed in WSL. Any `cli_dispatch` listing test that now misses `install` gets its expected string updated, as in Task 4.

- [ ] **Step 5: Check that the tests can fail**

1. Temporarily remove `return Output::new().with_code(130);` and let the arm fall through to the `Err(_)` branch. `ctrl_c_rolls_back_and_exits_130` must fail. Restore.
2. Temporarily change `status = status.max(2)` to `max(1)`. `an_unknown_version_prints_upstreams_hint_and_exits_2` must fail. Restore.

- [ ] **Step 6: Build a real CPython once (execution, not a unit test)**

Write `C:\tmp\m2a_real_build.sh`:

```bash
#!/usr/bin/env bash
# One real `pyenv install` in WSL: the newest 3.12 from python.org, into a scratch root.
set -u
source "$HOME/.cargo/env"
cd /home/jm/rpyenv-linux
cargo build -q --workspace
R=/home/jm/m2a-real-root
rm -rf /home/jm/m2a-real-root
start=$(date +%s)
PYENV_ROOT="$R" ./target/debug/pyenv install 3.12
echo "exit=$? seconds=$(( $(date +%s) - start ))"
V=$(ls "$R/versions")
echo "version=$V"
"$R/versions/$V/bin/python3.12" -c "import ssl, sqlite3, lzma, ctypes, bz2, zlib, readline, curses; print('modules ok')"
head -1 "$R/versions/$V/bin/pip3"
head -1 "$R/versions/$V/bin/idle3"
"$R/versions/$V/bin/python3.12" -m pip --version
ls -a "$R/versions"
ls "$R/shims" | head -5
```

Fetch the branch into `/home/jm/rpyenv-linux` first, as `implementer-instructions.md` says. Then run the script with `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m2a_real_build.sh`.

Expected, all of which must be pasted into the report:
- `exit=0`;
- `modules ok`;
- both shebangs read `#!/home/jm/m2a-real-root/versions/3.12.14/bin/python3.12`, which proves the DESTDIR staging didn't leak the build tree;
- pip's version line;
- `versions` holds only `3.12.14` besides `.` and `..`;
- the shims exist.

Any difference is a finding. Report it; don't work around it. Delete `/home/jm/m2a-real-root` afterwards by literal path.

- [ ] **Step 7: Lint and commit**

Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` on both OSes, and `python ci/shim_deps.py`.

```bash
git add crates
git commit -m "Add pyenv install: resolve, build through the transaction, install default packages, rehash"
```

---

### Task 8: `pyenv uninstall`

**Files:**
- Create: `crates/pyenv/src/commands/uninstall.rs`
- Modify: `crates/pyenv/src/commands/mod.rs` (`LINUX_ONLY` row), `crates/pyenv/src/help.rs` (the `uninstall` topic)
- Create: `crates/pyenv/tests/cli_uninstall.rs`

**Interfaces:**
- Produces: `commands::uninstall::uninstall(&Ctx, &[&str]) -> Output`

- [ ] **Step 1: Write the failing tests**

Create `crates/pyenv/tests/cli_uninstall.rs`:

```rust
//! `pyenv uninstall` (docs/parity/pyenv-m2-reference.md "pyenv uninstall").
#![cfg(unix)]

mod common;
use common::Fixture;

const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> ...\n\n   -f  Attempt to remove the specified version without prompting\n       for confirmation. If the version does not exist, do not\n       display an error message.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";

#[test]
fn force_removes_and_rehashes() {
    let f = Fixture::new();
    f.exe("3.12.0/bin/onlyhere");
    f.version("3.11.0/bin");
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    assert!(f.root.join("shims/onlyhere").exists());
    let r = f.pyenv(&["uninstall", "-f", "3.12.0"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("pyenv: 3.12.0 uninstalled\n", "", 0));
    assert!(!f.root.join("versions/3.12.0").exists());
    assert!(!f.root.join("shims/onlyhere").exists(), "rehashed");
}

#[test]
fn a_missing_version_stops_without_force_and_is_silent_with_it() {
    let f = Fixture::new();
    f.version("3.11.0");
    let r = f.pyenv(&["uninstall", "9.9", "3.11.0"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "pyenv: version `9.9' not installed\n", 1));
    assert!(f.root.join("versions/3.11.0").exists(), "later arguments not processed");
    let r = f.pyenv(&["uninstall", "-f", "9.9", "3.11.0"]);
    assert_eq!((r.stdout.as_str(), r.code), ("pyenv: 3.11.0 uninstalled\n", 0));
}

#[test]
fn without_force_eof_at_the_prompt_stops_and_removes_nothing() {
    let f = Fixture::new();
    f.version("3.11.0");
    let r = f.pyenv(&["uninstall", "3.11.0"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
    assert!(f.root.join("versions/3.11.0").exists());
}

#[test]
fn no_prefix_resolution_and_paths_lose_their_directories() {
    let f = Fixture::new();
    f.version("3.11.15").version("u4");
    assert_eq!(f.pyenv(&["uninstall", "3.11"]).stderr, "pyenv: version `3.11' not installed\n");
    let r = f.pyenv(&["uninstall", "-f", "/some/where/u4"]);
    assert_eq!(r.stdout, "pyenv: u4 uninstalled\n");
}

#[test]
fn a_symlinked_version_loses_only_the_link() {
    let f = Fixture::new();
    let target = f.base.join("elsewhere");
    std::fs::create_dir_all(target.join("bin")).unwrap();
    std::os::unix::fs::symlink(&target, f.root.join("versions/linked")).unwrap();
    assert_eq!(f.pyenv(&["uninstall", "-f", "linked"]).code, 0);
    assert!(target.join("bin").is_dir());
}

#[test]
fn usage_errors_and_help() {
    let f = Fixture::new();
    f.version("u2").version("u3");
    for args in [&["uninstall"][..], &["uninstall", "-f", "u2", "--bogus", "u3"], &["uninstall", "u2", "-f"], &["uninstall", "-f", ""]] {
        let r = f.pyenv(args);
        assert_eq!((r.stderr.as_str(), r.code), (HELP, 1), "{args:?}");
    }
    assert!(f.root.join("versions/u2").exists() && f.root.join("versions/u3").exists(), "checked before any removal");
    let r = f.pyenv(&["uninstall", "--help"]);
    assert_eq!((r.stdout.as_str(), r.code), (HELP, 0));
}
```

Run: `cargo test -p pyenv --test cli_uninstall` in WSL.
Expected: every test fails, with `no such command`.

- [ ] **Step 2: Implement**

Create `crates/pyenv/src/commands/uninstall.rs`:

```rust
//! `pyenv uninstall [-f|--force] <version> ...` (reference "pyenv uninstall"). Options are
//! positional; nothing is resolved by prefix; each argument loses its directories.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use std::io::{BufRead, IsTerminal};
use std::path::Path;

pub const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> ...\n\n   -f  Attempt to remove the specified version without prompting\n       for confirmation. If the version does not exist, do not\n       display an error message.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";
pub const USAGE: &str = "Usage: pyenv uninstall [-f|--force] <version> ...";

fn usage() -> Output {
    Output { stderr: HELP.into(), code: 1, ..Output::new() }
}

pub fn uninstall(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        o.out("--force");
        for n in rpyenv_core::installed::names(&ctx.versions_dir(), ctx.flavor) {
            o.out(n);
        }
        return o;
    }
    if matches!(args.first(), Some(&"-h" | &"--help")) {
        return Output { stdout: HELP.into(), ..Output::new() };
    }
    let (force, versions) = match args.first() {
        Some(&"-f" | &"--force") => (true, &args[1..]),
        _ => (false, args),
    };
    if versions.is_empty() || versions.iter().any(|v| v.is_empty() || v.starts_with('-')) {
        return usage();
    }
    let mut out = Output::new();
    for v in versions {
        let name = Path::new(v).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let prefix = ctx.versions_dir().join(&name);
        if !force {
            if !prefix.is_dir() {
                out.err(format!("pyenv: version `{name}' not installed"));
                return out.with_code(1);
            }
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                rpyenv_core::textout::write(true, &format!("pyenv: remove {}? (y/N) ", prefix.display()));
            }
            let mut line = String::new();
            let reply = match stdin.lock().read_line(&mut line) {
                Ok(0) | Err(_) => String::new(),
                Ok(_) => line.trim_end_matches(['\n', '\r']).to_string(),
            };
            if !["y", "Y", "yes", "YES"].contains(&reply.as_str()) {
                return out.with_code(1);
            }
        }
        if prefix.is_dir() {
            let gone = if prefix.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                std::fs::remove_file(&prefix)
            } else {
                std::fs::remove_dir_all(&prefix)
            };
            if let Err(e) = gone {
                out.err(format!("pyenv: cannot remove {}: {e}", prefix.display()));
                return out.with_code(1);
            }
            let r = crate::commands::rehash::rehash(ctx, &[]);
            out.stderr.push_str(&r.stderr);
            if r.code != 0 {
                return out.with_code(r.code);
            }
            out.out(format!("pyenv: {name} uninstalled"));
        }
    }
    out
}
```

Add `pub mod uninstall;` and the `("uninstall", uninstall::uninstall)` row to `LINUX_ONLY`. Add the `uninstall` help topic: summary `Uninstall Python versions`, usage `USAGE`, text `HELP`.

- [ ] **Step 3: Run the tests, check one can fail, and commit**

Run: `cargo test -p pyenv --test cli_uninstall` in WSL, then `cargo test --workspace` on both OSes.
Expected: 6 passed.

Mutation: temporarily make the symlink branch call `remove_dir_all`. `a_symlinked_version_loses_only_the_link` must fail, because the target's `bin` would be removed. Restore.

Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`.

```bash
git add crates
git commit -m "Add pyenv uninstall with upstream's prompts, messages and exit codes"
```

---

### Task 9: Parity wiring: allowlist rows, milestone tags, bats, differential cases

**Files:**
- Modify: `docs/parity/allowlist.md` (rows D-55 to D-67), `parity/allowlist.py` (tags), `parity/test_allowlist.py`
- Modify: `parity/bats_run.sh` (the `latest` wrapper), `parity/expected/bats.txt`, `parity/expected/pyenv-win.txt`
- Modify: `parity/diff_cases.py`, `parity/golden/linux/**` (new and changed goldens)
- Modify: the Rust tests from Tasks 1–8, to add `allowlist D-NN` citations
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` §12 item 3 (checklist wording, M2a/M2b)

**Interfaces:**
- Consumes all the earlier tasks.
- Produces:
  - `MILESTONES` gains `M2a`, `M2b`;
  - `DELIVERED = ("M1", "M2a")`.

- [ ] **Step 1: Milestone tags, test first**

In `parity/test_allowlist.py`, add a test asserting that:
- `check_reason("M2b Windows installer", "Windows", table)` is accepted;
- `check_reason("M2a ...", "Linux", table)` is rejected with the delivered message;
- `check_reason("M2 ...", ...)` is still accepted.

Run `python -m unittest discover -s parity -p "test_*.py"` and see it fail. Then, in `parity/allowlist.py`, set:

```python
MILESTONES = ("M2a", "M2b", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9")
DELIVERED = ("M1", "M2a")
```

Longer tags come first so a `startswith` check matches `M2a` before `M2`. Read `check_reason` and adjust the match so `M2a` and `M2b` are whole tags. The test must pass.

- [ ] **Step 2: Re-tag and re-measure the expected failures**

1. In `parity/expected/pyenv-win.txt`, change every reason that starts with `M2 ` to start with `M2b `, because those tests wait for the Windows installer.
2. In `parity/bats_run.sh`, add `latest` to the wrapper list, with the comment `# M2a`.
3. Fetch the branch into WSL and build it.
4. Run the bats suite as root, then check it:
   - `wsl -d Debian -u root --exec /usr/bin/bash /mnt/c/tmp/m1cb_ctl_bats_root.sh`
   - `wsl -d Debian --exec /usr/bin/bash /mnt/c/tmp/m1cb_ctl_bats_check.sh`

   If those controller scripts are gone, write your own from the call in `.github/workflows/parity.yml`.
5. Expected: `bats_check` reports the 10 `latest.bats` entries as now-passing or failing differently. For each one:
   - **It passes:** remove it from `bats.txt`.
   - **It still fails:** its tests stub `pyenv-versions` and `python-build` on `PATH`, which rpyenv's in-process resolution never calls. The reason becomes `D-52 rpyenv resolves versions in-process; the test stubs pyenv-versions/python-build on PATH`, and you must confirm that from the failure output.
6. Paste the before and after counts into the report.

- [ ] **Step 3: Allowlist rows**

Add these rows to `docs/parity/allowlist.md`, in the existing column format (ID | OS | Command | upstream | rpyenv | Reason). All are `Linux` and cite the plan Decision named:

| Row | Command | Difference |
|---|---|---|
| D-55 | `install --list` | CPython definitions plus plugin definitions only; other families come in M7 (Decision 2) |
| D-56 | `install` | definitions are interpreted, not sourced; other bash is refused with its line (Decision 3) |
| D-57 | `versions`, `latest`, rehash | the installer's `.tmp-*`/`.old-*` names are hidden (Decision 4) |
| D-58 | `install` | atomic: staged, swapped, rolled back; `-f` never builds over the old tree; an interrupted version carries `.rpyenv-incomplete` (Decision 4, 5) |
| D-59 | `install` | not-found always exits 2, Ctrl+C always 130, no `PREFIX_EXISTS` leak (Decision 5) |
| D-60 | `install` | `patch` output goes to the log; the temporary patch file is removed (Decision 5) |
| D-61 | `install X:latest` | version-boundary match, alpha dropped, `pyenv latest`'s sort; no match prints `pyenv: no known versions match the prefix` (Decision 5, spec §9.3) |
| D-62 | `install` default packages | pip runs in the installed directory, so an alias gets them (Decision 5) |
| D-63 | `install` | downloads in-process: the http-client variables and `-4`/`-6` are ignored, proxies come from the environment, transient errors are retried (Decision 6) |
| D-64 | `install` | SHA-256 required (MD5 or none refused); no `get-pip.py` fallback (Decision 7) |
| D-65 | `install` | a pre-flight check refuses missing compiler/make/patch/OpenSSL/zlib before downloading; `RPYENV_SKIP_PREFLIGHT=1` (Decision 8) |
| D-66 | `install --version`, BUILD FAILED | name rpyenv (Decision 9) |
| D-67 | `install` | configure gets `--with-ensurepip=no`; pip is installed after the move to the final prefix (Task 5) |

For the "upstream" and "rpyenv" cells, copy the exact behavior from `docs/parity/pyenv-m2-reference.md` and this plan. Each cell is one or two sentences. Also extend D-51's rpyenv cell: `install`/`uninstall` hooks (`before_install`, `after_install`, …) are not run either.

- [ ] **Step 4: Cite the rows in tests**

Put `// allowlist D-NN` on the comment line directly above each test that pins a row. If a test has no comment, add one line naming the row. Use these pairs:

| Row | Test |
|---|---|
| D-56 | `other_bash_is_refused_with_its_line` |
| D-57 | `installer_staging_names_are_not_versions`, `installed_mode_ignores_the_installers_staging` |
| D-58 | `dropping_without_commit_restores_the_previous_version`, `skip_existing_is_silent_and_force_rebuilds` |
| D-59 | `an_unknown_version_prints_upstreams_hint_and_exits_2`, `ctrl_c_rolls_back_and_exits_130` |
| D-60 | `built_in_patches_are_applied_and_their_output_stays_in_the_log` |
| D-62 | `default_packages_run_in_the_new_version_and_a_failure_still_succeeds` |
| D-63 | `a_server_error_is_retried` |
| D-64 | `md5_missing_and_malformed_fragments_are_refused`, `a_failed_ensurepip_does_not_fall_back_to_get_pip` |
| D-65 | `missing_ssl_refuses_with_the_debian_package` |
| D-66 | `usage_errors_and_version` |
| D-67 | `a_standard_build_installs_with_python_builds_flags_and_messages` |

D-55 and D-61 are covered by the differential cases below.

- [ ] **Step 5: Differential cases**

Append these Linux cases to `CASES` in `parity/diff_cases.py`. Read how existing cases build fixtures (`files`, `{v0}`/`{v1}`) and follow that style. The upstream copy at the pinned commit includes `plugins/python-build`, which pyenv's dispatcher puts on `PATH`.

```python
    Case("install --help", ("install", "--help"), os="Linux"),
    Case("help install", ("help", "install"), os="Linux"),
    Case("install with a bad option", ("install", "-x"), os="Linux"),
    Case("install an unknown version", ("install", "9.9.9"), os="Linux"),
    Case("install a version that only has prereleases", ("install", "3.15.0"), os="Linux"),
    Case("install --list", ("install", "--list"), os="Linux", allow=("D-55",)),
    Case("install --list --bare", ("install", "--list", "--bare"), os="Linux", allow=("D-55",)),
    Case("install --version", ("install", "--version"), os="Linux", allow=("D-66",)),
    Case("install X:latest with no match", ("install", "3.15:latest"), os="Linux", allow=("D-61",)),
    Case("latest -k 3.12", ("latest", "-k", "3.12"), os="Linux"),
    Case("latest -k 3t", ("latest", "-k", "3t"), os="Linux"),
    Case("latest -k 3.15", ("latest", "-k", "3.15"), os="Linux"),
    Case("latest with installed versions", ("latest", "3"), os="Linux"),
    Case("uninstall --help", ("uninstall", "--help"), os="Linux"),
    Case("uninstall a missing version", ("uninstall", "9.9"), os="Linux"),
    Case("uninstall -f a missing version", ("uninstall", "-f", "9.9"), os="Linux"),
    Case("uninstall -f an installed version", ("uninstall", "-f", "{v0}"), os="Linux"),
```

If `args` don't expand `{v0}`, use the fixture's literal first version name, as `diff.py`'s fixture builder defines it, and say so.

Then, in WSL:
1. Run `python3 parity/diff.py --rpyenv target/debug --upstream <pinned upstream>`.
2. Write the goldens for the allowed cases with `--update-golden`.
3. Review `git diff parity/golden`.
4. Re-run without `--update-golden` and confirm exit 0.

The `commands`/`help` goldens change because `install`, `latest` and `uninstall` now exist (D-49); check that this is the only change in those files. Report the new totals of allowed and same cases. Any case that reads `differs` is a finding: fix the code to match upstream, or, if the difference is intentional, add the row and say why.

- [ ] **Step 6: Coverage and the spec checklist**

1. Run `python parity/coverage.py` on Windows and in WSL. It must exit 0, with every row from D-01 to D-67 covered by execution or by a test citation.
2. In spec §12 item 3, change the shipping checklist's first step to: "add the milestone (or sub-milestone, e.g. `M2a`) to `DELIVERED`".

- [ ] **Step 7: Commit**

Run `python -m unittest discover -s parity -p "test_*.py"`.

```bash
git add docs parity crates
git commit -m "Wire the M2a installer into parity: allowlist rows D-55 to D-67, M2a delivered, latest bats, differential cases"
```

---

### Task 10: CI: the real Linux source build (tier 3) and the weekly definitions sync

**Files:**
- Create: `.github/workflows/install-linux.yml`, `.github/workflows/sync-python-build.yml`, `ci/check_install.sh`

**Interfaces:**
- Consumes: the `pyenv` binary, and `ci/sync_python_build.py` (Task 3).

- [ ] **Step 1: The post-install check script**

Create `ci/check_install.sh`:

```bash
#!/usr/bin/env bash
# Tier 3 (spec §12.5): checks a real `pyenv install` result.
#   ci/check_install.sh <PYENV_ROOT> <version>
set -euo pipefail
root="$1"
v="$2"
p="$root/versions/$v"
xy=$(echo "$v" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')
py="$p/bin/python$xy"
"$py" -c "import ssl, sqlite3, lzma, ctypes, bz2, zlib, readline, curses; print('modules ok')"
want="#!$py"
for s in pip3 idle3; do
  got=$(head -1 "$p/bin/$s")
  [ "$got" = "$want" ] || { echo "::error::$s shebang is '$got', want '$want'"; exit 1; }
done
"$py" -m pip --version
left=$(find "$root/versions" -maxdepth 1 -name '.tmp-*' -o -maxdepth 1 -name '.old-*' -o -maxdepth 1 -name '.lock-*')
[ -z "$left" ] || { echo "::error::staging left behind: $left"; exit 1; }
[ ! -e "$p/.rpyenv-incomplete" ] || { echo "::error::marker left in $p"; exit 1; }
[ -e "$root/shims/python$xy" ] || { echo "::error::no shim for python$xy"; exit 1; }
echo "install check ok: $v"
```

Run it by hand in WSL against the Task 7 Step 6 result before deleting that root. If that root is already gone, reinstall it. Paste the output.

- [ ] **Step 2: The tier-3 workflow**

Create `.github/workflows/install-linux.yml`:

```yaml
name: install-linux
# Tier 3 (spec §12.5): a real CPython source build. PRs that touch the Linux installer build
# one pinned version; the nightly run builds the newest of each supported minor.
on:
  pull_request:
    paths:
      - "crates/pyenv/src/install/**"
      - "crates/pyenv/src/commands/install.rs"
      - "crates/pyenv/src/commands/uninstall.rs"
      - "crates/pyenv/python-build/**"
      - "crates/pyenv/build.rs"
      - "ci/check_install.sh"
      - ".github/workflows/install-linux.yml"
  schedule:
    - cron: "17 3 * * *"
  workflow_dispatch:

permissions:
  contents: read

jobs:
  versions:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    outputs:
      list: ${{ steps.pick.outputs.list }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - run: cargo build --release -p pyenv
      - id: pick
        run: |
          if [ "${{ github.event_name }}" = "pull_request" ]; then
            echo 'list=["3.12.14"]' >> "$GITHUB_OUTPUT"
          else
            list=$(for m in 3.9 3.10 3.11 3.12 3.13 3.14 3.14t; do target/release/pyenv latest -k "$m"; done | python3 -c 'import json,sys; print(json.dumps([l.strip() for l in sys.stdin if l.strip()]))')
            echo "list=$list" >> "$GITHUB_OUTPUT"
          fi

  build:
    needs: versions
    runs-on: ubuntu-latest
    timeout-minutes: 60
    strategy:
      fail-fast: false
      matrix:
        version: ${{ fromJSON(needs.versions.outputs.list) }}
    name: install ${{ matrix.version }}
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master, 2026-10-02
        with:
          toolchain: "1.96"
      - name: Build dependencies (upstream's install-prerequisites list)
        run: |
          sudo apt-get update -q
          sudo apt-get install -yq make build-essential libssl-dev zlib1g-dev libbz2-dev libreadline-dev libsqlite3-dev curl git llvm libncurses5-dev libncursesw5-dev xz-utils tk-dev libxml2-dev libxmlsec1-dev libffi-dev liblzma-dev libzstd-dev ccache
      - uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        with:
          path: ~/.cache/ccache
          key: ccache-${{ matrix.version }}-${{ github.run_id }}
          restore-keys: ccache-${{ matrix.version }}-
      - run: cargo build --release -p pyenv -p pyenv-shim
      - name: pyenv install ${{ matrix.version }}
        env:
          CC: ccache gcc
        run: |
          start=$(date +%s)
          PYENV_ROOT="$RUNNER_TEMP/root" target/release/pyenv install "${{ matrix.version }}"
          echo "| ${{ matrix.version }} | $(( $(date +%s) - start )) s |" >> "$GITHUB_STEP_SUMMARY"
      - run: bash ci/check_install.sh "$RUNNER_TEMP/root" "${{ matrix.version }}"
```

`CC` is `ccache gcc`, two words. Task 6's probe already splits it (`a_compiler_with_a_wrapper_is_probed_as_program_plus_args`), and configure reads `CC` from the environment as autoconf does.

- [ ] **Step 3: The weekly sync workflow**

Create `.github/workflows/sync-python-build.yml`:

```yaml
name: sync-python-build
# Keeps crates/pyenv/python-build in step with pyenv's latest release (plan M2a, Decision 1):
# opens a pull request when the vendored copy differs.
on:
  schedule:
    - cron: "41 4 * * 1"
  workflow_dispatch:

permissions:
  contents: write
  pull-requests: write

jobs:
  sync:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09 # v5
      - name: Fetch pyenv's latest release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          tag=$(gh api repos/pyenv/pyenv/releases/latest --jq .tag_name)
          sha=$(gh api "repos/pyenv/pyenv/commits/$tag" --jq .sha)
          curl -fsSL "https://github.com/pyenv/pyenv/archive/$sha.tar.gz" | tar -xz -C "$RUNNER_TEMP"
          echo "TAG=$tag" >> "$GITHUB_ENV"
          echo "SHA=$sha" >> "$GITHUB_ENV"
      - name: Update the vendored copy
        run: python3 ci/sync_python_build.py --upstream "$RUNNER_TEMP/pyenv-$SHA" --commit "$SHA" --write
      - name: Open a pull request when it changed
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          if git diff --quiet; then echo "up to date with $TAG"; exit 0; fi
          branch="sync-python-build-$TAG"
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          git checkout -b "$branch"
          git add crates/pyenv/python-build
          git commit -m "Sync python-build definitions with pyenv $TAG"
          git push -f origin "$branch"
          gh pr create --base main --head "$branch" --title "Sync python-build definitions with pyenv $TAG" --body "Automated: crates/pyenv/python-build updated from pyenv $TAG ($SHA) by ci/sync_python_build.py." || echo "a pull request for $branch already exists"
```

Two notes for the PR description:
- Pull requests created with `GITHUB_TOKEN` don't trigger other workflows. A maintainer closes and reopens the PR, or pushes a commit, so CI runs.
- The repository setting "Allow GitHub Actions to create and approve pull requests" must be on.

- [ ] **Step 4: Validate the workflows locally**

1. Parse both YAML files with Python (`python -c "import yaml,sys; [yaml.safe_load(open(f, encoding='utf-8')) for f in sys.argv[1:]]" .github/workflows/install-linux.yml .github/workflows/sync-python-build.yml`; `pip install pyyaml` into a venv if needed).
2. Run the `pick` step's nightly branch by hand in WSL, from `target/release/pyenv`, and paste the JSON it prints.
3. Run `ci/sync_python_build.py` in check mode against the pinned tarball. It must report `0 difference(s)`.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/install-linux.yml .github/workflows/sync-python-build.yml ci/check_install.sh crates
git commit -m "Build CPython for real on CI (tier 3) and sync python-build definitions weekly"
```

---

## Self-review (done while writing; recorded for reviewers)

1. **Spec coverage.** Each M2 requirement for Linux and the shared core maps to a task:
   - §9.2 catalog/build/variables/pre-flight/failure: Tasks 3, 5, 6.
   - §9.3 atomic: Task 2 and Task 5, with Ctrl+C in Task 7.
   - §9.3 verified: Task 1.
   - §9.3 `:latest`: Task 7.
   - §9.3 default packages: Task 7.
   - §12.5 tiers 1 and 3: Tasks 1, 5, 7, 10.
   - §14 item 2 Linux commands: Tasks 4, 7, 8.
   - §13 new variable: Task 6.

   Deferred to M2b: §9.1 Windows packages, the catalog workflow, `pyenv update`, and §12.5 tier 2.
2. **Placeholder scan.** No TBD or TODO. The places where the implementer must read existing code (diff fixture placeholders, `check_reason`, the `installed::names` order, the `help.rs` table position) name the exact file, and say what to report.
3. **Type consistency.** These names are used identically across tasks:
   - `Found`/`Origin`/`Definition`/`Package`/`Fetch` (Tasks 3, 5, 7);
   - `Txn::{begin, stage_dir, place, placed, commit, target}` (Tasks 2, 5, 7);
   - `Fetcher::{from_env, fetch}` and `FetchRequest` (Tasks 1, 5, 7);
   - `builder::{run, Job, Options, patch_files, xy_of}` (Tasks 5, 7);
   - `preflight::{check, report}` (Tasks 6, 7);
   - `defs::{names, known, find, parse, patches_for, sort_versions, UPSTREAM_VERSION}` (Tasks 3, 4, 7).
4. **Review Focus.**
   - Line 1: Task 2 tests.
   - Line 2: Task 2 `staging_lives_inside_versions…`.
   - Line 3: Task 7 `ctrl_c_rolls_back_and_exits_130`.
   - Line 4: every `Fixture` path has `py env ñ`, and Task 5 builds under it.
   - Line 5: Task 2 `a_second_install_of_the_same_name_is_refused…`.
