# M8a: detect and install missing build dependencies, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Linux, `pyenv install <CPython 3.x>` finds every library the standard library's
extension modules need, offers to install the missing ones through the package manager, and
refuses to call a build installed until every such module imports and `ssl` has CA
certificates.

**Architecture:** A data table (`install/stdlib.rs`) says, per CPython 3 minor version, which
extension modules exist, which library each needs, how to probe it, its minimum version and
its package per distribution. The pre-flight check (`install/preflight.rs`) probes that table
(compile **and link**, plus version macros and CA certificates). A new `install/deps.rs` picks
a route (print, or install with the package manager) from a terminal prompt, a flag or an
environment variable, and runs the install. After the build, `install/builder.rs` imports every
module of the table.

**Tech Stack:** Rust (crate `pyenv`, Linux code paths only), shell stubs in tests, GitHub Actions
with distribution containers.

**Spec:** `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md` (M8a = §3 row 1,
§4, §5). Parent: `docs/specs/2026-09-27-rpyenv-design.md` §15.2.

## Global Constraints

- Linux flavor only. No Windows code path changes; Windows tests must stay green.
- CPython **3.x** definitions only (plan ruling P2). Python 2.x, PyPy and conda keep today's
  behavior exactly.
- Distributions: apt (Debian, Ubuntu and derivatives), dnf (Fedora, RHEL, Rocky, Alma), zypper
  (openSUSE, SLES), pacman (Arch). The existing apk column stays for the printed command only.
- Routes in M8a: `system` (print and fail) and `install` (package manager). `build` and
  `prebuilt` are accepted words that print `pyenv: the <route> route comes in a later rpyenv
  release` and exit 1 (plan ruling P3).
- At a terminal: ask; Enter = `p`; every module required (spec D4). Without a terminal, `--deps`
  or `RPYENV_BUILD_DEPS` set: that route, every module required (D5). Without a terminal and
  neither set: **lenient mode** (D13): only the compiler, `make`, OpenSSL and zlib stop the
  build; every other missing item warns, the build goes on, and the run ends with §5.2's
  summary (three lines, verbatim from the spec).
- rpyenv never adds `-y` (or `--noconfirm`, `--non-interactive`) to a command it ran at a
  terminal. Without a terminal, `--deps=install` adds them and prints the full command first.
- `RPYENV_SKIP_PREFLIGHT=1` skips the pre-flight check (as today) **and** turns §4.4's
  post-build failures into warnings (plan ruling P1).
- Test hooks, honored in debug builds only (`cfg!(debug_assertions)`), as the repo's other
  `RPYENV_TEST_*` hooks: `RPYENV_TEST_PREFLIGHT` (comma list of library keys to report missing;
  empty = nothing missing; skips the real probes), `RPYENV_TEST_OS_RELEASE` (path read instead
  of `/etc/os-release`), `RPYENV_TEST_TTY=1` (treat stdin as a terminal).
- Never run `sudo`, `apt-get`, `dnf`, `zypper` or `pacman` on a development machine: unit and
  CLI tests use shell stubs on `PATH`; real installs only in CI containers (Task 6).
- Every user-visible difference from upstream pyenv gets an allowlist entry (Task 7, spec D12).
- Commit messages: one line, no attribution lines.

## Plan rulings (made while planning; the user confirms them at plan review)

- **P1:** `RPYENV_SKIP_PREFLIGHT=1` (an existing escape hatch) also turns the post-build check
  into warnings, as lenient mode does. Why: it already means "build anyway". Cost if wrong:
  one `if`.
- **P4 (spec D13):** lenient mode is decided once, before the build, in `install.rs`:
  `strict = tty || f.deps.is_some() || env("RPYENV_BUILD_DEPS").is_some()`. When not strict,
  `install.rs` pushes `("RPYENV_DEPS_MODE", "lenient")` into `job_env` before `job_get` is
  defined, and the builder (Task 5) reads it.
- **P2:** CPython 2.x keeps today's check and warnings. Why: EOL since 2020, different module
  names; the spec's examples are 3.x. Cost if wrong: a later table extension.
- **P3:** `--deps=build|prebuilt` exist as words in M8a and exit 1 with "comes in a later
  rpyenv release", so scripts written now keep working later. Cost if wrong: none.

## Review Focus

1. **A derivative distribution** (`ID=linuxmint`, `ID_LIKE="ubuntu debian"`; `ID=rocky`,
   `ID_LIKE="rhel centos fedora"`) must map to apt and dnf. Pinned in Task 3
   (`derivatives_map_to_their_parents_manager`).
2. **The install command fails or installs only part** (wrong password, package not found):
   no build may start while something is still missing; detection runs again and the prompt
   returns without `i`. Pinned in Task 4 (`a_failed_install_reprompts_without_i_and_builds_nothing`).
3. **Ctrl+C at the prompt** must exit 130 with nothing installed or built. Pinned in Task 4
   (`eof_or_interrupt_at_the_prompt_builds_nothing`; EOF stands in for the interrupt path the
   shared `prompt` already handles).
4. **A compiler wrapper and custom flags** (`CC="ccache gcc"`, `LDFLAGS=-L/opt/x/lib`,
   `CPPFLAGS=-I/opt/x/include`) must reach the link probe. Pinned in Task 2
   (`the_link_probe_passes_cppflags_and_ldflags`).
5. **A non-CPython or 2.x definition** must see no new check. Pinned in Task 4
   (`a_python2_definition_keeps_todays_behavior`).

---

### Task 1: The standard-library module table (`install/stdlib.rs`)

**Files:**
- Create: `crates/pyenv/src/install/stdlib.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod stdlib;`)

**Interfaces:**
- Produces:
  - `pub struct Lib { pub key: &'static str, pub headers: &'static [&'static str], pub links: &'static [&'static [&'static str]], pub pkg_config: Option<&'static str>, pub packages: [&'static str; 5] }`
    (`links`: alternatives, each a list of `-l` flags; `packages`: apt, dnf, zypper, pacman, apk)
  - `pub struct Module { pub name: &'static str, pub lib: &'static str, pub imports: &'static [&'static str], pub since: u32, pub until: Option<u32> }`
    (`since`/`until`: CPython 3 minor versions; `until` is the last minor that has it)
  - `pub struct MinVersion { pub header: &'static str, pub expr: &'static str, pub need: u64, pub shown: &'static str }`
  - `pub fn lib(key: &str) -> Option<&'static Lib>`
  - `pub fn modules(minor: u32) -> Vec<&'static Module>`
  - `pub fn min_version(key: &str, minor: u32) -> Option<MinVersion>`
  - `pub fn minor_of_step(step: &str) -> Option<u32>` (`verify_py312` → 12, `verify_py3_latest` → `u32::MAX`; 2.x → `None`)
  - `pub const CA_KEY: &str = "ca-certificates";`

- [ ] **Step 1: Write the failing tests** at the bottom of the new file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn names(minor: u32) -> Vec<&'static str> {
        modules(minor).iter().map(|m| m.name).collect()
    }

    #[test]
    fn modules_follow_cpythons_releases() {
        let m312 = names(12);
        for n in ["ssl", "zlib", "bz2", "lzma", "readline", "sqlite3", "ctypes", "curses",
                  "tkinter", "dbm", "uuid", "crypt", "nis"] {
            assert!(m312.contains(&n), "3.12 lacks {n}");
        }
        let m313 = names(13);
        assert!(!m313.contains(&"crypt") && !m313.contains(&"nis"), "{m313:?}");
        assert!(!names(6).contains(&"uuid"), "_uuid is 3.7+");
        assert!(!names(6).contains(&"ctypes"), "3.6 bundles libffi");
        assert!(names(u32::MAX).contains(&"tkinter"));
    }

    #[test]
    fn every_module_names_a_known_library_with_five_packages() {
        for minor in [7, 12, 13] {
            for m in modules(minor) {
                let l = lib(m.lib).unwrap_or_else(|| panic!("{}: no lib {}", m.name, m.lib));
                assert!(l.packages.iter().all(|p| !p.is_empty()), "{}", l.key);
                assert!(!m.imports.is_empty(), "{}", m.name);
            }
        }
        assert!(lib(CA_KEY).is_some());
    }

    #[test]
    fn minimum_versions_come_from_cpythons_configure() {
        let ssl310 = min_version("openssl", 10).unwrap();
        assert_eq!((ssl310.need, ssl310.shown), (0x1010_100f, "1.1.1"));
        assert_eq!(min_version("openssl", 9).unwrap().shown, "1.0.2");
        assert!(min_version("openssl", 6).is_none());
        assert_eq!(min_version("sqlite", 13).unwrap().need, 3_015_002);
        assert_eq!(min_version("sqlite", 11).unwrap().shown, "3.7.15");
        assert!(min_version("zlib", 13).is_none());
    }

    #[test]
    fn the_minor_comes_from_the_verify_step() {
        assert_eq!(minor_of_step("verify_py312"), Some(12));
        assert_eq!(minor_of_step("verify_py39"), Some(9));
        assert_eq!(minor_of_step("verify_py3_latest"), Some(u32::MAX));
        assert_eq!(minor_of_step("verify_py27"), None);
        assert_eq!(minor_of_step("standard"), None);
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p pyenv --lib stdlib` → FAIL to compile (module and items don't exist).

- [ ] **Step 3: Implement** `crates/pyenv/src/install/stdlib.rs`:

```rust
//! Which standard-library extension modules each CPython 3 minor version builds, the
//! library each needs, how to probe it, and its package per distribution (M8 design §4.2).
//! Package columns: apt, dnf, zypper, pacman, apk.

pub struct Lib {
    pub key: &'static str,
    /// Any one of these headers is enough.
    pub headers: &'static [&'static str],
    /// Alternatives for the link probe, each a list of `-l` flags.
    pub links: &'static [&'static [&'static str]],
    /// When set, `pkg-config --cflags --libs <it>` is tried too (Tk).
    pub pkg_config: Option<&'static str>,
    pub packages: [&'static str; 5],
}

pub struct Module {
    /// What the user loses, as in the prompt: `ssl`, `tkinter`.
    pub name: &'static str,
    pub lib: &'static str,
    /// What the post-build check imports (M8 design §4.4).
    pub imports: &'static [&'static str],
    pub since: u32,
    /// The last 3.x minor that has the module.
    pub until: Option<u32>,
}

pub struct MinVersion {
    pub header: &'static str,
    /// A macro whose preprocessed value is a number (`0x30000020L`, `3045001`).
    pub expr: &'static str,
    pub need: u64,
    pub shown: &'static str,
}

pub const CA_KEY: &str = "ca-certificates";

static LIBS: &[Lib] = &[
    Lib { key: "openssl", headers: &["openssl/ssl.h"], links: &[&["-lssl", "-lcrypto"]], pkg_config: None,
          packages: ["libssl-dev", "openssl-devel", "libopenssl-devel", "openssl", "openssl-dev"] },
    Lib { key: "zlib", headers: &["zlib.h"], links: &[&["-lz"]], pkg_config: None,
          packages: ["zlib1g-dev", "zlib-devel", "zlib-devel", "zlib", "zlib-dev"] },
    Lib { key: "bzip2", headers: &["bzlib.h"], links: &[&["-lbz2"]], pkg_config: None,
          packages: ["libbz2-dev", "bzip2-devel", "libbz2-devel", "bzip2", "bzip2-dev"] },
    Lib { key: "xz", headers: &["lzma.h"], links: &[&["-llzma"]], pkg_config: None,
          packages: ["liblzma-dev", "xz-devel", "xz-devel", "xz", "xz-dev"] },
    Lib { key: "readline", headers: &["readline/readline.h", "editline/readline.h"],
          links: &[&["-lreadline"], &["-ledit"]], pkg_config: None,
          packages: ["libreadline-dev", "readline-devel", "readline-devel", "readline", "readline-dev"] },
    Lib { key: "sqlite", headers: &["sqlite3.h"], links: &[&["-lsqlite3"]], pkg_config: None,
          packages: ["libsqlite3-dev", "sqlite-devel", "sqlite3-devel", "sqlite", "sqlite-dev"] },
    Lib { key: "libffi", headers: &["ffi.h"], links: &[&["-lffi"]], pkg_config: None,
          packages: ["libffi-dev", "libffi-devel", "libffi-devel", "libffi", "libffi-dev"] },
    Lib { key: "ncurses", headers: &["ncursesw/ncurses.h", "ncurses.h", "curses.h"],
          links: &[&["-lncursesw", "-lpanelw"], &["-lncurses", "-lpanel"]], pkg_config: None,
          packages: ["libncurses-dev", "ncurses-devel", "ncurses-devel", "ncurses", "ncurses-dev"] },
    Lib { key: "tk", headers: &["tk.h"], links: &[&["-ltk8.6", "-ltcl8.6"], &["-ltk", "-ltcl"]],
          pkg_config: Some("tk"),
          packages: ["tk-dev", "tk-devel", "tk-devel", "tk", "tk-dev"] },
    Lib { key: "gdbm", headers: &["gdbm.h"], links: &[&["-lgdbm", "-lgdbm_compat"]], pkg_config: None,
          packages: ["libgdbm-dev libgdbm-compat-dev", "gdbm-devel", "gdbm-devel", "gdbm", "gdbm-dev"] },
    Lib { key: "libuuid", headers: &["uuid/uuid.h"], links: &[&["-luuid"]], pkg_config: None,
          packages: ["uuid-dev", "libuuid-devel", "libuuid-devel", "util-linux-libs", "util-linux-dev"] },
    Lib { key: "libcrypt", headers: &["crypt.h"], links: &[&["-lcrypt"]], pkg_config: None,
          packages: ["libcrypt-dev", "libxcrypt-devel", "libxcrypt-devel", "libxcrypt", "libxcrypt-dev"] },
    Lib { key: "libnsl", headers: &["rpcsvc/yp_prot.h"], links: &[&["-lnsl", "-ltirpc"]], pkg_config: None,
          packages: ["libnsl-dev libtirpc-dev", "libnsl2-devel libtirpc-devel", "libnsl-devel libtirpc-devel",
                     "libnsl libtirpc", "libnsl-dev libtirpc-dev"] },
    Lib { key: CA_KEY, headers: &[], links: &[], pkg_config: None,
          packages: ["ca-certificates", "ca-certificates", "ca-certificates", "ca-certificates", "ca-certificates"] },
];

static MODULES: &[Module] = &[
    Module { name: "ssl", lib: "openssl", imports: &["ssl", "_hashlib"], since: 0, until: None },
    Module { name: "zlib", lib: "zlib", imports: &["zlib"], since: 0, until: None },
    Module { name: "bz2", lib: "bzip2", imports: &["bz2"], since: 0, until: None },
    Module { name: "lzma", lib: "xz", imports: &["lzma"], since: 3, until: None },
    Module { name: "readline", lib: "readline", imports: &["readline"], since: 0, until: None },
    Module { name: "sqlite3", lib: "sqlite", imports: &["sqlite3"], since: 0, until: None },
    Module { name: "ctypes", lib: "libffi", imports: &["ctypes"], since: 7, until: None },
    Module { name: "curses", lib: "ncurses", imports: &["curses", "curses.panel"], since: 0, until: None },
    Module { name: "tkinter", lib: "tk", imports: &["tkinter"], since: 0, until: None },
    Module { name: "dbm", lib: "gdbm", imports: &["dbm.gnu", "dbm.ndbm"], since: 0, until: None },
    Module { name: "uuid", lib: "libuuid", imports: &["_uuid"], since: 7, until: None },
    Module { name: "crypt", lib: "libcrypt", imports: &["crypt"], since: 0, until: Some(12) },
    Module { name: "nis", lib: "libnsl", imports: &["nis"], since: 0, until: Some(12) },
];

pub fn lib(key: &str) -> Option<&'static Lib> {
    LIBS.iter().find(|l| l.key == key)
}

/// The modules CPython 3.`minor` builds that need an outside library.
pub fn modules(minor: u32) -> Vec<&'static Module> {
    MODULES
        .iter()
        .filter(|m| minor >= m.since && m.until.is_none_or(|u| minor <= u))
        .collect()
}

/// CPython's `configure` minimums (PEP 644 for OpenSSL; `configure.ac` for SQLite).
pub fn min_version(key: &str, minor: u32) -> Option<MinVersion> {
    match key {
        "openssl" if minor >= 10 => Some(MinVersion { header: "openssl/opensslv.h",
            expr: "OPENSSL_VERSION_NUMBER", need: 0x1010_100f, shown: "1.1.1" }),
        "openssl" if minor >= 7 => Some(MinVersion { header: "openssl/opensslv.h",
            expr: "OPENSSL_VERSION_NUMBER", need: 0x1000_2000, shown: "1.0.2" }),
        "sqlite" if minor >= 13 => Some(MinVersion { header: "sqlite3.h",
            expr: "SQLITE_VERSION_NUMBER", need: 3_015_002, shown: "3.15.2" }),
        "sqlite" if minor >= 11 => Some(MinVersion { header: "sqlite3.h",
            expr: "SQLITE_VERSION_NUMBER", need: 3_007_015, shown: "3.7.15" }),
        _ => None,
    }
}

/// `verify_py312` → 12; `verify_py3_latest` → the newest (`u32::MAX`); 2.x and others → `None`.
pub fn minor_of_step(step: &str) -> Option<u32> {
    let tag = step.strip_prefix("verify_py3")?;
    if tag == "_latest" {
        return Some(u32::MAX);
    }
    tag.parse().ok()
}
```

Note on `packages`: an entry may hold two package names separated by a space
(`"libgdbm-dev libgdbm-compat-dev"`); Task 3 splits on whitespace. Task 6's container test
proves every name by running the printed command.

- [ ] **Step 4: Run them.** `cargo test -p pyenv --lib stdlib` → PASS (4 tests). `cargo fmt --all`.

- [ ] **Step 5: Commit.**
```bash
git add crates/pyenv/src/install/stdlib.rs crates/pyenv/src/install/mod.rs
git commit -m "M8a task 1: the standard-library module table (modules per CPython 3 minor, libraries, packages, minimum versions)"
```

---

### Task 2: Probes that link, check versions and find CA certificates (`install/preflight.rs`)

**Files:**
- Modify: `crates/pyenv/src/install/preflight.rs`

**Interfaces:**
- Consumes: Task 1's `stdlib::{lib, modules, min_version, Lib, CA_KEY}`.
- Produces (the old `Row`/`rows` table and `report` stay for Python 2.x, P2):
  - `pub enum Missing { Program(String), Header(String), Link(String), TooOld { found: String, need: &'static str }, Certificates }`
  - `pub struct Dep { pub name: &'static str, pub required: bool, pub missing: Missing, pub lib: &'static str }`
    (`lib`: a `stdlib` key, or `"compiler"`, `"make"`, `"patch"`, `"pkg-config"` for programs)
  - `pub fn check3(env: &dyn Fn(&str) -> Option<String>, needs_patch: bool, minor: u32) -> Vec<Dep>`
    (CPython 3; every item `required: true`; `RPYENV_TEST_PREFLIGHT` in debug builds)
  - `pub(crate) fn parse_version(preprocessed: &str) -> Option<u64>`
  - `pub(crate) fn ca_present(openssldir: Option<&str>, exists: &dyn Fn(&str) -> bool, has_files: &dyn Fn(&str) -> bool) -> bool`

`Dep` gains the `lib` field: set it to `"compiler"`, `"make"`, `"patch"` or the module's lib key
in the existing `evaluate_with` too (the old table's module names map: `ssl`→`openssl`,
`zlib`→`zlib`, `bz2`→`bzip2`, `readline`→`readline`, `sqlite3`→`sqlite`, `ctypes`→`libffi`,
`curses`→`ncurses`, `lzma`→`xz`, `tkinter`→`tk`). Update the existing tests' `Dep { … }`
literals with `lib: "openssl"`.

- [ ] **Step 1: Write the failing tests** (add to `mod tests`):

```rust
    #[test]
    fn version_macros_parse_in_hex_and_decimal() {
        assert_eq!(parse_version("# 1 \"x\"\n\n0x30000020L\n"), Some(0x3000_0020));
        assert_eq!(parse_version("3045001\n"), Some(3_045_001));
        assert_eq!(parse_version("OPENSSL_VERSION_NUMBER\n"), None);
    }

    #[test]
    fn ca_certificates_are_found_as_a_file_or_a_directory() {
        let none = |_: &str| false;
        assert!(ca_present(Some("/usr/lib/ssl"), &|p| p == "/usr/lib/ssl/cert.pem", &none));
        assert!(ca_present(Some("/usr/lib/ssl"), &none, &|d| d == "/usr/lib/ssl/certs"));
        // No openssl binary: the distributions' bundle paths.
        assert!(ca_present(None, &|p| p == "/etc/pki/tls/certs/ca-bundle.crt", &none));
        assert!(!ca_present(Some("/usr/lib/ssl"), &none, &none));
    }

    /// Review focus 4: a compiler wrapper and custom flags reach the link probe.
    #[test]
    fn the_link_probe_passes_cppflags_and_ldflags() {
        let seen = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |program: &str, args: &[&str]| -> bool {
            seen.borrow_mut().push(
                std::iter::once(program.to_string()).chain(args.iter().map(|a| a.to_string())).collect(),
            );
            true
        };
        let env = |k: &str| match k {
            "CC" => Some("ccache gcc".to_string()),
            "CPPFLAGS" => Some("-I/opt/x/include".to_string()),
            "LDFLAGS" => Some("-L/opt/x/lib".to_string()),
            _ => None,
        };
        assert!(link_probe(&env, &run, "zlib.h", &["-lz"], "/tmp/out"));
        let call = &seen.borrow()[0];
        assert_eq!(call[0], "ccache gcc");
        for want in ["-I/opt/x/include", "-L/opt/x/lib", "-lz", "-o", "/tmp/out"] {
            assert!(call.iter().any(|a| a == want), "{want} missing from {call:?}");
        }
        // Libraries after the source, as linkers need.
        let src = call.iter().position(|a| a == "-").unwrap();
        assert!(call.iter().position(|a| a == "-lz").unwrap() > src);
    }

    #[test]
    fn the_test_hook_reports_exactly_the_listed_libraries() {
        let env = |k: &str| match k {
            "RPYENV_TEST_PREFLIGHT" => Some("openssl,libuuid".to_string()),
            _ => None,
        };
        let deps = check3(&env, false, 12);
        let libs: Vec<&str> = deps.iter().map(|d| d.lib).collect();
        assert_eq!(libs, ["openssl", "libuuid"]);
        assert!(deps.iter().all(|d| d.required));
        assert_eq!(deps[0].name, "ssl");
        let none = |k: &str| (k == "RPYENV_TEST_PREFLIGHT").then(String::new);
        assert!(check3(&none, false, 12).is_empty());
    }

    #[test]
    fn a_too_old_library_is_reported_with_both_versions() {
        let d = too_old("ssl", "openssl", 0x1000_2000, &crate::install::stdlib::min_version("openssl", 13).unwrap());
        assert_eq!(d.unwrap().missing, Missing::TooOld { found: "0x10002000".into(), need: "1.1.1" });
        assert!(too_old("ssl", "openssl", 0x3000_0000, &crate::install::stdlib::min_version("openssl", 13).unwrap()).is_none());
    }
```

- [ ] **Step 2: Run them.** `cargo test -p pyenv --lib preflight` → FAIL to compile.

- [ ] **Step 3: Implement** (in `preflight.rs`, below `check`):

```rust
/// `<cc> CPPFLAGS... -x c - -o <out> LDFLAGS... <libs>`: compiles and links a program that
/// includes `header` (M8 design §4.2 item 3). `run(program, args)` runs it with the source on
/// stdin; the caller deletes `out`.
fn link_probe(
    env: &dyn Fn(&str) -> Option<String>,
    run: &dyn Fn(&str, &[&str]) -> bool,
    header: &str,
    libs: &[&str],
    out: &str,
) -> bool {
    let cc = env("CC").filter(|v| !v.is_empty()).unwrap_or_else(|| "cc".into());
    let words = |k: &str| env(k).map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let mut args: Vec<String> = Vec::new();
    args.extend(words("CPPFLAGS"));
    args.extend(words("PYTHON_CPPFLAGS"));
    args.extend(["-x".into(), "c".into(), "-".into(), "-o".into(), out.into()]);
    args.extend(words("LDFLAGS"));
    args.extend(words("PYTHON_LDFLAGS"));
    args.extend(libs.iter().map(|l| l.to_string()));
    let _ = header; // the source text is passed by `run`
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&cc, &refs)
}

/// The number a version macro expands to: the last non-empty, non-`#` line, hex or decimal,
/// with C suffixes (`L`, `UL`) dropped.
pub(crate) fn parse_version(preprocessed: &str) -> Option<u64> {
    let last = preprocessed.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).last()?;
    let digits = last.trim_end_matches(['L', 'l', 'U', 'u']);
    match digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => digits.parse().ok(),
    }
}

fn too_old(name: &'static str, lib: &'static str, found: u64, need: &stdlib::MinVersion) -> Option<Dep> {
    (found < need.need).then(|| Dep {
        name,
        required: true,
        missing: Missing::TooOld { found: format!("{found:#x}"), need: need.shown },
        lib,
    })
}

/// CA certificates where the system's OpenSSL looks (`openssl version -d`), else the
/// distributions' bundle files (M8 design §4.3).
pub(crate) fn ca_present(
    openssldir: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> bool {
    if let Some(d) = openssldir {
        if exists(&format!("{d}/cert.pem")) || has_files(&format!("{d}/certs")) {
            return true;
        }
    }
    [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/etc/ssl/cert.pem",
    ]
    .iter()
    .any(|p| exists(p))
}
```

and `check3`:

```rust
/// CPython 3.`minor` (M8 design §4): programs as `check`, then every module of the table:
/// header, link, minimum version; then CA certificates. Every item is required (spec D1).
/// Debug builds: `RPYENV_TEST_PREFLIGHT` lists the library keys to report missing instead.
pub fn check3(env: &dyn Fn(&str) -> Option<String>, needs_patch: bool, minor: u32) -> Vec<Dep> {
    if cfg!(debug_assertions) {
        if let Some(list) = env("RPYENV_TEST_PREFLIGHT") {
            let keys: Vec<&str> = list.split(',').map(str::trim).filter(|k| !k.is_empty()).collect();
            let mut out = Vec::new();
            for k in keys {
                let name = stdlib::modules(minor).iter().find(|m| m.lib == k).map(|m| m.name).unwrap_or(match k {
                    stdlib::CA_KEY => "ssl certificates",
                    _ => "build tools",
                });
                let lib = stdlib::lib(k).map(|l| l.key).unwrap_or("compiler");
                out.push(Dep { name, required: true, missing: Missing::Header(k.to_string()), lib });
            }
            return out;
        }
    }
    // Programs first, exactly as `check` (a missing compiler hides the rest).
    let mut out: Vec<Dep> = check(env, needs_patch)
        .into_iter()
        .filter(|d| matches!(d.missing, Missing::Program(_)))
        .map(|d| Dep { required: true, ..d })
        .collect();
    if out.iter().any(|d| d.lib == "compiler") {
        return out;
    }
    let path = env("PATH");
    let runs_with_input = |program: &str, args: &[&str], input: &str, capture: bool| -> Option<(bool, String)> {
        let mut words = program.split_whitespace();
        let mut c = Command::new(words.next()?);
        c.args(words).args(args);
        if let Some(p) = &path { c.env("PATH", p); }
        c.stdin(Stdio::piped()).stdout(if capture { Stdio::piped() } else { Stdio::null() }).stderr(Stdio::null());
        let mut child = c.spawn().ok()?;
        if let Some(mut s) = child.stdin.take() { use std::io::Write; let _ = s.write_all(input.as_bytes()); }
        let o = child.wait_with_output().ok()?;
        Some((o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned()))
    };
    if runs_with_input("pkg-config", &["--version"], "", false).is_none_or(|(ok, _)| !ok) {
        out.push(Dep { name: "pkg-config", required: true, missing: Missing::Program("pkg-config".into()), lib: "pkg-config" });
    }
    let tmp = std::env::temp_dir().join(format!("rpyenv-probe-{}", std::process::id()));
    let out_path = tmp.display().to_string();
    for m in stdlib::modules(minor) {
        let Some(l) = stdlib::lib(m.lib) else { continue };
        if l.key == stdlib::CA_KEY || out.iter().any(|d| d.lib == l.key) {
            continue;
        }
        let pc = l.pkg_config.and_then(|p| runs_with_input("pkg-config", &["--cflags", "--libs", p], "", true))
            .filter(|(ok, _)| *ok).map(|(_, s)| s);
        let mut found = None;
        'h: for h in l.headers {
            let src = format!("#include <{h}>\nint main(void) {{ return 0; }}\n");
            let mut alts: Vec<Vec<String>> = l.links.iter().map(|a| a.iter().map(|s| s.to_string()).collect()).collect();
            if let Some(flags) = &pc { alts.insert(0, flags.split_whitespace().map(str::to_string).collect()); }
            for alt in &alts {
                let libs: Vec<&str> = alt.iter().map(String::as_str).collect();
                if link_probe(env, &|p, a| runs_with_input(p, a, &src, false).is_some_and(|(ok, _)| ok), h, &libs, &out_path) {
                    found = Some(*h);
                    break 'h;
                }
            }
        }
        let _ = std::fs::remove_file(&tmp);
        let Some(_) = found else {
            out.push(Dep { name: m.name, required: true, missing: Missing::Link(l.headers[0].to_string()), lib: l.key });
            continue;
        };
        if let Some(need) = stdlib::min_version(l.key, minor) {
            let src = format!("#include <{}>\n{}\n", need.header, need.expr);
            let cc = env("CC").filter(|v| !v.is_empty()).unwrap_or_else(|| "cc".into());
            let v = runs_with_input(&cc, &["-E", "-P", "-x", "c", "-"], &src, true)
                .and_then(|(ok, s)| ok.then_some(s)).and_then(|s| parse_version(&s));
            if let Some(d) = v.and_then(|v| too_old(m.name, l.key, v, &need)) {
                out.push(d);
            }
        }
    }
    let dir = runs_with_input("openssl", &["version", "-d"], "", true)
        .and_then(|(ok, s)| ok.then_some(s))
        .and_then(|s| s.split('"').nth(1).map(str::to_string));
    let has_files = |d: &str| std::fs::read_dir(d).is_ok_and(|mut r| r.next().is_some());
    if !ca_present(dir.as_deref(), &|p| std::path::Path::new(p).is_file(), &has_files) {
        out.push(Dep { name: "ssl certificates", required: true, missing: Missing::Certificates, lib: stdlib::CA_KEY });
    }
    out
}
```

Keep `use super::stdlib;` at the top. `check` and `report` stay as they are for Python 2.x
(P2); add the `lib` field where they build `Dep`.

- [ ] **Step 4: Run them.** `cargo test -p pyenv --lib preflight` → PASS (old and new).
  Then probe this machine by hand in WSL (not on Windows): add to the `#[ignore]` test a call
  `check3(&env, true, 13)` and run
  `cargo test -p pyenv --lib preflight::tests::probe_this_machine -- --ignored --nocapture`
  in a WSL script (`CARGO_TARGET_DIR=/home/jm/rpyenv-m8-target`, `wsl -d Debian --exec
  /usr/bin/bash -l /mnt/c/tmp/<x>.sh`). Paste the output in the report: it shows real probes on
  Debian (a probe that wrongly says "missing" for an installed library is a bug to fix here).

- [ ] **Step 5: Commit.**
```bash
git add crates/pyenv/src/install/preflight.rs
git commit -m "M8a task 2: probes link and check versions; CA certificates; every CPython 3 module required"
```

---

### Task 3: Routes, package managers and the prompt text (`install/deps.rs`)

**Files:**
- Create: `crates/pyenv/src/install/deps.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `pub mod deps;`)

**Interfaces:**
- Consumes: `preflight::{Dep, Missing}`, `stdlib::lib`.
- Produces:
  - `pub enum Route { System, Install, Build, Prebuilt }` with `pub fn parse(s: &str) -> Option<Route>`
  - `pub enum Manager { Apt, Dnf, Zypper, Pacman, Apk }` with `pub fn from_os_release(text: &str) -> Option<Manager>` and `fn column(self) -> usize`
  - `pub fn packages(m: Manager, deps: &[Dep]) -> Vec<String>` (deduplicated, in order; splits two-name entries)
  - `pub fn install_argv(m: Manager, pkgs: &[String], sudo: bool, assume_yes: bool) -> Vec<String>`
  - `pub enum SudoUse { Root, Allowed, Not(&'static str) }` and `pub fn classify_sudo(is_root: bool, has_sudo: bool, n_status: Option<(bool, String)>) -> SudoUse`
  - `pub fn rhel_crb_hint(os_release: &str) -> Option<String>`
  - `pub fn prompt_lines(version: &str, deps: &[Dep], m: Option<Manager>, install: Result<Vec<String>, &'static str>) -> Vec<String>`
  - `pub fn choice(line: &str, install_offered: bool) -> Option<Route>` (Enter → `System`)

- [ ] **Step 1: Write the failing tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::preflight::{Dep, Missing};

    fn dep(name: &'static str, lib: &'static str) -> Dep {
        Dep { name, required: true, missing: Missing::Header("x.h".into()), lib }
    }

    /// Review focus 1.
    #[test]
    fn derivatives_map_to_their_parents_manager() {
        assert!(matches!(Manager::from_os_release("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), Some(Manager::Apt)));
        assert!(matches!(Manager::from_os_release("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"), Some(Manager::Dnf)));
        assert!(matches!(Manager::from_os_release("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n"), Some(Manager::Zypper)));
        assert!(matches!(Manager::from_os_release("ID=endeavouros\nID_LIKE=arch\n"), Some(Manager::Pacman)));
        assert!(Manager::from_os_release("ID=gentoo\n").is_none());
    }

    #[test]
    fn packages_are_split_and_deduplicated() {
        let d = [dep("dbm", "gdbm"), dep("ssl", "openssl"), dep("ssl", "openssl")];
        assert_eq!(packages(Manager::Apt, &d), ["libgdbm-dev", "libgdbm-compat-dev", "libssl-dev"]);
    }

    #[test]
    fn the_install_command_adds_yes_only_when_asked() {
        let p = vec!["libssl-dev".to_string()];
        assert_eq!(install_argv(Manager::Apt, &p, true, false), ["sudo", "apt-get", "install", "libssl-dev"]);
        assert_eq!(install_argv(Manager::Apt, &p, false, true), ["apt-get", "install", "-y", "libssl-dev"]);
        assert_eq!(install_argv(Manager::Pacman, &p, true, true), ["sudo", "pacman", "-S", "--needed", "--noconfirm", "libssl-dev"]);
        assert_eq!(install_argv(Manager::Zypper, &p, false, true), ["zypper", "--non-interactive", "install", "libssl-dev"]);
        assert_eq!(install_argv(Manager::Dnf, &p, true, false), ["sudo", "dnf", "install", "libssl-dev"]);
    }

    #[test]
    fn sudo_is_offered_unless_it_cannot_work() {
        assert!(matches!(classify_sudo(true, false, None), SudoUse::Root));
        assert!(matches!(classify_sudo(false, false, None), SudoUse::Not(_)));
        assert!(matches!(classify_sudo(false, true, Some((true, String::new()))), SudoUse::Allowed));
        assert!(matches!(classify_sudo(false, true, Some((false, "sudo: a password is required\n".into()))), SudoUse::Allowed));
        assert!(matches!(classify_sudo(false, true, Some((false, "Sorry, user jm may not run sudo on box.\n".into()))), SudoUse::Not(_)));
    }

    #[test]
    fn rhel_family_gets_the_crb_hint_and_fedora_does_not() {
        assert_eq!(rhel_crb_hint("ID=rocky\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n").unwrap(),
                   "sudo dnf config-manager --set-enabled crb");
        assert_eq!(rhel_crb_hint("ID=almalinux\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"8.10\"\n").unwrap(),
                   "sudo dnf config-manager --set-enabled powertools");
        assert!(rhel_crb_hint("ID=fedora\nVERSION_ID=41\n").is_none());
    }

    #[test]
    fn the_prompt_lists_the_modules_and_the_offer() {
        let d = [Dep { name: "ssl", required: true, missing: Missing::TooOld { found: "0x10002000".into(), need: "1.1.1" }, lib: "openssl" },
                 Dep { name: "uuid", required: true, missing: Missing::Link("uuid/uuid.h".into()), lib: "libuuid" }];
        let argv = install_argv(Manager::Apt, &packages(Manager::Apt, &d), true, false);
        let lines = prompt_lines("3.13.1", &d, Some(Manager::Apt), Ok(argv));
        assert_eq!(lines, [
            "pyenv: 3.13.1 can't build these modules: ssl, uuid",
            "  ssl       OpenSSL 0x10002000 found, needs 1.1.1+   (libssl-dev)",
            "  uuid      no usable uuid/uuid.h                   (uuid-dev)",
            "How should rpyenv get them?",
            "  [p] print the commands and stop (default)",
            "  [i] install with sudo: sudo apt-get install libssl-dev uuid-dev",
        ]);
        let lines = prompt_lines("3.13.1", &d, None, Err("no sudo on PATH"));
        assert!(lines.contains(&"  [i] not offered: no sudo on PATH".to_string()), "{lines:?}");
    }

    #[test]
    fn choices() {
        assert!(matches!(choice("", true), Some(Route::System)));
        assert!(matches!(choice("p", true), Some(Route::System)));
        assert!(matches!(choice("I", true), Some(Route::Install)));
        assert!(choice("i", false).is_none());
        assert!(choice("x", true).is_none());
        assert!(matches!(Route::parse("install"), Some(Route::Install)));
        assert!(matches!(Route::parse("prebuilt"), Some(Route::Prebuilt)));
        assert!(Route::parse("yes").is_none());
    }
}
```

- [ ] **Step 2: Run them.** `cargo test -p pyenv --lib deps::` → FAIL to compile.

- [ ] **Step 3: Implement** `crates/pyenv/src/install/deps.rs`:

```rust
//! How `pyenv install` gets missing build dependencies (M8 design §5): the routes, the
//! package manager and its command, whether `sudo` can work, and the prompt's text.

use super::preflight::{Dep, Missing};
use super::stdlib;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route { System, Install, Build, Prebuilt }

impl Route {
    pub fn parse(s: &str) -> Option<Route> {
        match s {
            "system" => Some(Route::System),
            "install" => Some(Route::Install),
            "build" => Some(Route::Build),
            "prebuilt" => Some(Route::Prebuilt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager { Apt, Dnf, Zypper, Pacman, Apk }

fn ids(os_release: &str) -> Vec<String> {
    os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| v.trim_matches('"').split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

impl Manager {
    pub fn from_os_release(text: &str) -> Option<Manager> {
        let ids = ids(text);
        let has = |n: &[&str]| ids.iter().any(|i| n.contains(&i.as_str()));
        if has(&["debian", "ubuntu"]) { Some(Manager::Apt) }
        else if has(&["fedora", "rhel", "centos"]) { Some(Manager::Dnf) }
        else if has(&["suse", "opensuse", "sles"]) || ids.iter().any(|i| i.starts_with("opensuse")) { Some(Manager::Zypper) }
        else if has(&["arch"]) { Some(Manager::Pacman) }
        else if has(&["alpine"]) { Some(Manager::Apk) }
        else { None }
    }

    fn column(self) -> usize {
        match self { Manager::Apt => 0, Manager::Dnf => 1, Manager::Zypper => 2, Manager::Pacman => 3, Manager::Apk => 4 }
    }
}

/// Programs (compiler, make, patch, pkg-config) by manager.
fn program_packages(lib: &str) -> Option<[&'static str; 5]> {
    match lib {
        "compiler" => Some(["build-essential", "gcc", "gcc", "base-devel", "build-base"]),
        "make" => Some(["make"; 5]),
        "patch" => Some(["patch"; 5]),
        "pkg-config" => Some(["pkg-config", "pkgconf-pkg-config", "pkg-config", "pkgconf", "pkgconf"]),
        _ => None,
    }
}

pub fn packages(m: Manager, deps: &[Dep]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in deps {
        let row = stdlib::lib(d.lib).map(|l| l.packages).or_else(|| program_packages(d.lib));
        if let Some(row) = row {
            for p in row[m.column()].split_whitespace() {
                if !out.iter().any(|o| o == p) {
                    out.push(p.to_string());
                }
            }
        }
    }
    out
}

pub fn install_argv(m: Manager, pkgs: &[String], sudo: bool, assume_yes: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if sudo { v.push("sudo".into()); }
    let (head, yes): (&[&str], &[&str]) = match m {
        Manager::Apt => (&["apt-get", "install"], &["-y"]),
        Manager::Dnf => (&["dnf", "install"], &["-y"]),
        Manager::Zypper => (&["zypper"], &["--non-interactive"]),
        Manager::Pacman => (&["pacman", "-S", "--needed"], &["--noconfirm"]),
        Manager::Apk => (&["apk", "add"], &[]),
    };
    v.extend(head.iter().map(|s| s.to_string()));
    if assume_yes { v.extend(yes.iter().map(|s| s.to_string())); }
    if m == Manager::Zypper { v.push("install".into()); }
    v.extend(pkgs.iter().cloned());
    v
}

pub enum SudoUse { Root, Allowed, Not(&'static str) }

/// `n_status`: `sudo -n true`'s success and stderr. A password prompt is fine at a terminal.
pub fn classify_sudo(is_root: bool, has_sudo: bool, n_status: Option<(bool, String)>) -> SudoUse {
    if is_root { return SudoUse::Root; }
    if !has_sudo { return SudoUse::Not("no sudo on PATH"); }
    match n_status {
        Some((true, _)) => SudoUse::Allowed,
        Some((false, err)) if err.contains("password is required") => SudoUse::Allowed,
        _ => SudoUse::Not("sudo says this user may not use it"),
    }
}

/// RHEL, Rocky and Alma keep some `-devel` packages in CRB (9) or PowerTools (8).
pub fn rhel_crb_hint(os_release: &str) -> Option<String> {
    let ids = ids(os_release);
    if !ids.iter().any(|i| i == "rhel") || ids.iter().any(|i| i == "fedora" && !os_release.contains("ID_LIKE")) {
        return None;
    }
    let major: u32 = os_release.lines().find_map(|l| l.strip_prefix("VERSION_ID="))?
        .trim_matches('"').split('.').next()?.parse().ok()?;
    let repo = if major >= 9 { "crb" } else { "powertools" };
    Some(format!("sudo dnf config-manager --set-enabled {repo}"))
}

fn why(d: &Dep) -> String {
    match &d.missing {
        Missing::Program(p) => format!("no {p} program"),
        Missing::Header(h) | Missing::Link(h) => format!("no usable {h}"),
        Missing::TooOld { found, need } => format!("{} {found} found, needs {need}+", if d.lib == "openssl" { "OpenSSL" } else { "version" }),
        Missing::Certificates => "no CA certificates".to_string(),
    }
}

pub fn prompt_lines(version: &str, deps: &[Dep], m: Option<Manager>, install: Result<Vec<String>, &'static str>) -> Vec<String> {
    let names: Vec<&str> = deps.iter().map(|d| d.name).fold(Vec::new(), |mut v, n| { if !v.contains(&n) { v.push(n); } v });
    let mut out = vec![format!("pyenv: {version} can't build these modules: {}", names.join(", "))];
    for d in deps {
        let pkg = m.map(|m| packages(m, std::slice::from_ref(d)).join(" ")).unwrap_or_default();
        out.push(format!("  {:<9} {:<38}  ({pkg})", d.name, why(d)).trim_end_matches(" ()").to_string());
    }
    out.push("How should rpyenv get them?".into());
    out.push("  [p] print the commands and stop (default)".into());
    match install {
        Ok(argv) => out.push(format!("  [i] install with sudo: {}", argv.join(" "))),
        Err(reason) => out.push(format!("  [i] not offered: {reason}")),
    }
    out
}

pub fn choice(line: &str, install_offered: bool) -> Option<Route> {
    match line.trim().to_ascii_lowercase().as_str() {
        "" | "p" => Some(Route::System),
        "i" if install_offered => Some(Route::Install),
        _ => None,
    }
}
```

The exact column widths in `prompt_lines` must make the test's expected lines match; adjust
the format widths, not the test, if they differ by spacing (the test is the contract for the
words; widths `{:<9}` and `{:<38}` are a starting point).

- [ ] **Step 4: Run them.** `cargo test -p pyenv --lib deps::` → PASS (7 tests). `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 5: Commit.**
```bash
git add crates/pyenv/src/install/deps.rs crates/pyenv/src/install/mod.rs
git commit -m "M8a task 3: routes, package managers, sudo use, CRB hint and the prompt text"
```

---

### Task 4: Wire detection and the routes into `pyenv install`

**Files:**
- Modify: `crates/pyenv/src/commands/install.rs` (flag `--deps=`, completion, help, the pre-flight block at lines ~427-447)
- Modify: `crates/pyenv/tests/common/mod.rs` (a helper that runs `pyenv` with stdin text)
- Test: `crates/pyenv/tests/cli_install_deps.rs` (new, `#![cfg(unix)]`)

**Interfaces:**
- Consumes: `preflight::{check3, report}`, `stdlib::minor_of_step`, `deps::*`, `reply::prompt`.
- Produces: `pyenv install --deps=system|install|build|prebuilt`; `RPYENV_BUILD_DEPS`.

- [ ] **Step 1: Test helper.** In `tests/common/mod.rs`, add to `impl Fixture`:

```rust
    /// `pyenv args` with `input` on stdin (the prompt tests set RPYENV_TEST_TTY=1).
    pub fn pyenv_input(&self, args: &[&str], env: &[(&str, &str)], input: &str) -> Run {
        use std::io::Write;
        let mut c = self.command(Path::new(env!("CARGO_BIN_EXE_pyenv")), &self.work, env);
        c.args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        Run { stdout: decode(&out.stdout), stderr: decode(&out.stderr), code: out.status.code().unwrap_or(-1) }
    }
```

- [ ] **Step 2: Write the failing CLI tests** in `crates/pyenv/tests/cli_install_deps.rs`. Read
  `tests/cli_install.rs` first and copy its `plugin_def`, `env` and `run` helpers (they serve
  the fake CPython `3.12.99` through `common::fakebuild`), but **without**
  `RPYENV_SKIP_PREFLIGHT`. Stubs on the fixture's `syspath`:

```rust
#![cfg(unix)]
mod common;
use common::Fixture;
// (copy plugin_def / env / run from cli_install.rs; env() omits RPYENV_SKIP_PREFLIGHT and adds
//  ("RPYENV_TEST_OS_RELEASE", <path to a written os-release>))

fn stub(f: &Fixture, name: &str, body: &str) {
    let p = f.syspath.join(name);
    f.file(&p, &format!("#!/bin/sh\n{body}\n"));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `sudo` and `apt-get` that only log; `id -u` answers 1000.
fn stubs(f: &Fixture, apt_exit: i32) {
    let log = f.base.join("cmd.log").display().to_string();
    stub(f, "sudo", &format!("[ \"$1\" = -n ] && [ \"$2\" = true ] && exit 0\necho \"sudo $*\" >> '{log}'\nexec \"$@\""));
    stub(f, "apt-get", &format!("echo \"apt-get $*\" >> '{log}'\nexit {apt_exit}"));
    stub(f, "id", "echo 1000");
}

#[test]
fn without_a_terminal_or_flag_openssl_still_stops_the_build() {
    let f = Fixture::new();
    // lenient mode (D13); missing: openssl, libuuid (RPYENV_TEST_PREFLIGHT); Debian os-release
    // run `pyenv install 3.12.99` → exit 1 (OpenSSL is in the hard set); stderr has
    //   "pyenv: cannot build Python: missing build dependencies:" and
    //   "  sudo apt-get install libssl-dev uuid-dev";
    // nothing in cmd.log; no versions/3.12.99.
}

#[test]
fn without_a_terminal_or_flag_other_libraries_only_warn_and_the_summary_ends_the_run() {
    // lenient mode; RPYENV_TEST_PREFLIGHT=tk,libuuid; FAKE_PY_MISSING="tkinter _uuid" →
    // exit 0; versions/3.12.99 exists; stderr ends with the three summary lines of spec §5.2:
    //   "pyenv: 3.12.99 was built without: tkinter, uuid",
    //   "  Add them: sudo apt-get install tk-dev uuid-dev, then pyenv install -f 3.12.99",
    //   and the line starting "  In a later rpyenv release this stops the build".
}

#[test]
fn deps_system_without_a_terminal_makes_every_module_required() {
    // --deps=system; RPYENV_TEST_PREFLIGHT=libuuid → exit 1; stderr lists uuid and
    // "  sudo apt-get install uuid-dev"; no versions/3.12.99.
}

#[test]
fn deps_install_without_a_terminal_runs_the_command_with_yes() {
    // stubs(apt_exit 0); RPYENV_TEST_PREFLIGHT=openssl first, then "" after the install:
    // the test hook can't change between calls, so use RPYENV_TEST_PREFLIGHT_AFTER="" (read by
    // check3 only after an install ran; see Step 3) — run with --deps=install → exit 0;
    // cmd.log has "sudo -n apt-get install -y libssl-dev"; versions/3.12.99 exists.
}

/// Review focus 2.
#[test]
fn a_failed_install_reprompts_without_i_and_builds_nothing() {
    // RPYENV_TEST_TTY=1, stubs(apt_exit 100); input "i\n\n" (install, then Enter at the
    // second prompt) → exit 1; stderr has the prompt twice, the second has "[i] not offered";
    // cmd.log has "sudo apt-get install libssl-dev" (no -y at a terminal); no versions/3.12.99.
}

#[test]
fn at_a_terminal_enter_prints_and_stops() {
    // RPYENV_TEST_TTY=1; input "\n" → exit 1; the prompt lines (Task 3) on stderr; cmd.log empty.
}

/// Review focus 3.
#[test]
fn eof_or_interrupt_at_the_prompt_builds_nothing() {
    // RPYENV_TEST_TTY=1; input "" (EOF) → exit 1; no versions/3.12.99; cmd.log empty.
}

#[test]
fn build_and_prebuilt_routes_say_they_come_later() {
    // --deps=build → exit 1, stderr "pyenv: the build route comes in a later rpyenv release";
    // RPYENV_BUILD_DEPS=prebuilt → same with "prebuilt"; --deps=bogus → exit 1 with
    // "pyenv: --deps takes system, install, build or prebuilt".
}

#[test]
fn not_root_and_no_sudo_leaves_i_out() {
    // no sudo stub; RPYENV_TEST_TTY=1; input "i\n\n" → stderr has "[i] not offered: no sudo on PATH",
    // the "i" answer is rejected ("pyenv: answer p" then the prompt again), exit 1.
}

/// Review focus 5.
#[test]
fn a_python2_definition_keeps_todays_behavior() {
    // A plugin definition 2.7.99 (verify_py27 step) with RPYENV_TEST_PREFLIGHT=openssl:
    // check3 is not used; the old check runs (stubs give a working `cc`? no: set
    // RPYENV_SKIP_PREFLIGHT unset and assert stderr does NOT contain "can't build these modules").
}
```

Write each body fully in `cli_install.rs`'s style (exact stderr lines asserted with
`contains`, file existence with `is_dir`). The two hook variables used by the tests:
`RPYENV_TEST_PREFLIGHT` (before an install) and `RPYENV_TEST_PREFLIGHT_AFTER` (after one).

- [ ] **Step 3: Run them.** `cargo test -p pyenv --test cli_install_deps` → FAIL.

- [ ] **Step 4: Implement.**

(a) `Flags` gains `deps: Option<String>`; in `parse`, a long name `deps=<v>` sets it
(`n.strip_prefix("deps=")`). Add `"--deps="` to the `--complete` list and this line to `HELP`
after `-g/--debug`:
`  --deps=<route>     How to get missing build libraries: system, install, build, prebuilt`.

(b) Replace the pre-flight block (`if compiles && …` through its closing brace) with a call
to a new function in `install.rs`:

```rust
        if compiles && job_get("RPYENV_SKIP_PREFLIGHT").as_deref() != Some("1") {
            let patch = f.patch || needs_patch(&found, &parsed);
            let minor = parsed.packages.iter().flat_map(|p| p.steps.iter())
                .find_map(|s| crate::install::stdlib::minor_of_step(s));
            let ok = match minor {
                Some(m) => resolve_deps(&job_get, &name, m, patch, f.deps.as_deref()),
                None => old_preflight(&job_get, patch), // 2.x: today's code, moved unchanged
            };
            if let Err(code) = ok {
                return Output::new().with_code(code);
            }
        }
```

`old_preflight` is today's block, unchanged, returning `Err(1)` when `r.refuse`.

`resolve_deps` (in `install.rs`):

```rust
/// M8 design §5: detect, then print (system) or install (install) until nothing is missing.
/// Ok(()) when the build may start; Err(exit code) otherwise.
fn resolve_deps(get: &dyn Fn(&str) -> Option<String>, version: &str, minor: u32, patch: bool, flag: Option<&str>) -> Result<(), i32> {
    use crate::install::{deps::{self, Route}, preflight};
    let chosen = match flag.map(str::to_string).or_else(|| get("RPYENV_BUILD_DEPS")) {
        Some(v) => match Route::parse(&v) {
            Some(r) => Some(r),
            None => { say("pyenv: --deps takes system, install, build or prebuilt"); return Err(1); }
        },
        None => None,
    };
    if let Some(r @ (Route::Build | Route::Prebuilt)) = chosen {
        let w = if r == Route::Build { "build" } else { "prebuilt" };
        say(&format!("pyenv: the {w} route comes in a later rpyenv release"));
        return Err(1);
    }
    let os_path = if cfg!(debug_assertions) { get("RPYENV_TEST_OS_RELEASE") } else { None }
        .unwrap_or_else(|| "/etc/os-release".into());
    let os = std::fs::read_to_string(&os_path).ok();
    let manager = os.as_deref().and_then(deps::Manager::from_os_release);
    let tty = std::io::stdin().is_terminal() || (cfg!(debug_assertions) && get("RPYENV_TEST_TTY").as_deref() == Some("1"));
    let is_root = run_out(get, "id", &["-u"]).is_some_and(|(_, s)| s.trim() == "0");
    let has_sudo = run_out(get, "sudo", &["--version"]).is_some();
    let sudo_n = has_sudo.then(|| run_out_err(get, "sudo", &["-n", "true"])).flatten();
    let mut installed = false;
    loop {
        let after = if installed && cfg!(debug_assertions) { get("RPYENV_TEST_PREFLIGHT_AFTER") } else { None };
        let probe_env = |k: &str| if k == "RPYENV_TEST_PREFLIGHT" { after.clone().or_else(|| get(k)) } else { get(k) };
        let missing = preflight::check3(&probe_env, patch, minor);
        if missing.is_empty() {
            return Ok(());
        }
        let sudo = deps::classify_sudo(is_root, has_sudo, sudo_n.clone());
        let offer: Result<Vec<String>, &'static str> = match (manager, &sudo) {
            (None, _) => Err("this distribution isn't recognized"),
            (_, deps::SudoUse::Not(r)) => Err(r),
            (_, _) if installed => Err("the last install left these missing"),
            (Some(m), s) => Ok(deps::install_argv(m, &deps::packages(m, &missing), !matches!(s, deps::SudoUse::Root), !tty)),
        };
        let route = if tty {
            for l in deps::prompt_lines(version, &missing, manager, offer.clone()) { say(&l); }
            loop {
                match crate::install::reply::prompt("> ") {
                    crate::install::reply::Reply::Interrupted => return Err(130),
                    crate::install::reply::Reply::Eof => return Err(1),
                    crate::install::reply::Reply::Line(l) => match deps::choice(&l, offer.is_ok()) {
                        Some(r) => break r,
                        None => say(if offer.is_ok() { "pyenv: answer p or i" } else { "pyenv: answer p" }),
                    },
                }
            }
        } else if let Some(r) = chosen {
            r
        } else {
            // D13, lenient mode: only the hard set stops the build; the rest warns and the
            // build goes on (the builder prints the end summary, Task 5).
            const HARD: [&str; 4] = ["compiler", "make", "openssl", "zlib"];
            let hard: Vec<preflight::Dep> = missing.iter().filter(|d| HARD.contains(&d.lib) || d.lib == "patch").cloned().collect();
            let r = preflight::report_deps(&missing, os.as_deref(), is_root);
            for l in &r { say(l); }
            return if hard.is_empty() { Ok(()) } else { Err(1) };
        };
        match (route, offer) {
            (Route::Install, Ok(argv)) => {
                // Without a terminal, sudo must not ask (`-n`), and the full command is shown.
                let argv: Vec<String> = if !tty && argv.first().is_some_and(|a| a == "sudo") {
                    std::iter::once("sudo".to_string()).chain(std::iter::once("-n".to_string())).chain(argv.into_iter().skip(1)).collect()
                } else { argv };
                say(&format!("pyenv: running: {}", argv.join(" ")));
                let ok = std::process::Command::new(&argv[0]).args(&argv[1..])
                    .env("PATH", get("PATH").unwrap_or_default())
                    .status().is_ok_and(|s| s.success());
                if !ok {
                    say("pyenv: the install command failed");
                    if let Some(h) = os.as_deref().and_then(deps::rhel_crb_hint) {
                        say(&format!("pyenv: some packages live in the CRB repository: {h}"));
                    }
                    if manager == Some(deps::Manager::Apt) {
                        say("pyenv: if apt couldn't find a package, run sudo apt-get update first");
                    }
                    if !tty { return Err(1); }
                }
                installed = true;
            }
            (Route::Install, Err(reason)) => { say(&format!("pyenv: can't install the packages: {reason}")); return Err(1); }
            _ => {
                // system: today's text, now listing every missing module.
                let r = preflight::report_deps(&missing, os.as_deref(), is_root);
                for l in &r { say(l); }
                return Err(1);
            }
        }
    }
}
```

Add to `preflight.rs` a `pub fn report_deps(missing: &[Dep], os_release: Option<&str>, root: bool) -> Vec<String>`
that produces today's `report` lines (`pyenv: cannot build Python: missing build dependencies:`,
one `  <name> (<what>)` line per item, `Install them with:`, the `sudo`-or-not command from
`deps::install_argv(m, &deps::packages(m, missing), !root, false)`, and
`To build anyway, set RPYENV_SKIP_PREFLIGHT=1.`), so the non-terminal default prints the same
shape as today. `run_out(get, program, args) -> Option<(bool, String)>` (stdout) and
`run_out_err` (stderr) are small helpers in `install.rs` that spawn with `PATH` from `get` and
`stdin(Stdio::null())`, returning `None` when the program can't start.

Spec §5.3 (apt): when the manager is apt and `/var/lib/apt/lists` holds no `*_Packages` file, the offer and the printed command start with `sudo apt-get update &&` and `resolve_deps` runs the two commands in order; add a unit test in `deps.rs` for the argv pair (`install_argv` stays one command; a new `pub fn apt_update_argv(sudo: bool) -> Vec<String>`). Read `install.rs` around the old block before editing; keep `Txn::begin` and everything after
it unchanged. `use std::io::IsTerminal;` at the top.

- [ ] **Step 5: Run.** `cargo test -p pyenv --test cli_install_deps` → PASS (8 tests);
  `cargo test -p pyenv --test cli_install` → PASS (unchanged; it sets
  `RPYENV_SKIP_PREFLIGHT`). WSL: `cargo test -p pyenv` → PASS (script as Task 2, run
  `cargo build -p pyenv-shim` first).

- [ ] **Step 6: Commit.**
```bash
git add crates/pyenv/src/commands/install.rs crates/pyenv/src/install/preflight.rs crates/pyenv/tests/common/mod.rs crates/pyenv/tests/cli_install_deps.rs
git commit -m "M8a task 4: pyenv install asks how to get missing libraries (print or install), with --deps and RPYENV_BUILD_DEPS"
```

---

### Task 5: The check after the build (spec §4.4)

**Files:**
- Modify: `crates/pyenv/src/install/builder.rs` (`verify`)
- Modify: `crates/pyenv/tests/common/fakebuild.rs` (the fake `python3.12` answers two more scripts)
- Test: `crates/pyenv/tests/cli_install_deps.rs` (add)

**Interfaces:**
- Consumes: `stdlib::{modules, minor_of_step}`.
- Produces: `pub(crate) fn stdlib_failures(run: &dyn Fn(&[&str]) -> bool, minor: u32) -> Vec<String>` in `builder.rs`.

- [ ] **Step 1: Fake python.** In `fakebuild.rs`'s `PYTHON`, before the `-c` import case, add:

```sh
  -c) case "$2" in
        *get_default_verify_paths*) [ -n "$FAKE_PY_NO_CA" ] && exit 1; exit 0;;
        *tkinter.Tcl*) [ -n "$FAKE_PY_NO_TCL" ] && exit 1; exit 0;;
      esac
      mod="${2#import }"
      …(existing lines)
```

(merge into the existing `-c)` arm; the import loop stays as it is).

- [ ] **Step 2: Unit test** in `builder.rs`'s tests:

```rust
    #[test]
    fn stdlib_failures_name_each_missing_import_and_the_certificates() {
        let fails = |args: &[&str]| -> bool {
            !(args[1] == "import _uuid" || args[1].contains("get_default_verify_paths"))
        };
        let f = stdlib_failures(&fails, 12);
        assert!(f.contains(&"uuid (import _uuid failed)".to_string()), "{f:?}");
        assert!(f.contains(&"ssl certificates (no CA file or directory)".to_string()), "{f:?}");
        assert!(stdlib_failures(&|_| true, 12).is_empty());
    }
```

and CLI tests in `cli_install_deps.rs`:

```rust
#[test]
fn a_build_missing_a_module_is_not_installed() {
    // RPYENV_TEST_PREFLIGHT="" (nothing missing before), FAKE_PY_MISSING="_uuid" → exit 1;
    // stderr has "pyenv: 3.12.99 was built without: uuid (import _uuid failed)"; no versions/3.12.99.
}

#[test]
fn skip_preflight_turns_the_failure_into_a_warning() {
    // RPYENV_SKIP_PREFLIGHT=1, FAKE_PY_NO_CA=1 → exit 0; stderr has
    // "pyenv: warning: 3.12.99 was built without: ssl certificates (no CA file or directory)".
}
```

- [ ] **Step 3: Run.** `cargo test -p pyenv --lib builder` and `--test cli_install_deps` → FAIL.

- [ ] **Step 4: Implement** in `builder.rs`:

```rust
/// M8 design §4.4: every module of the table imports, CA certificates exist, Tcl runs.
pub(crate) fn stdlib_failures(run: &dyn Fn(&[&str]) -> bool, minor: u32) -> Vec<String> {
    let mut out = Vec::new();
    for m in super::stdlib::modules(minor) {
        for i in m.imports {
            let code = format!("import {i}");
            if !run(&["-c", &code]) {
                out.push(format!("{} (import {i} failed)", m.name));
                break;
            }
        }
    }
    let ca = "import ssl, os; p = ssl.get_default_verify_paths(); \
              ok = (p.cafile and os.path.isfile(p.cafile)) or (p.capath and os.path.isdir(p.capath) and os.listdir(p.capath)); \
              raise SystemExit(0 if ok else 1)";
    if !run(&["-c", ca]) {
        out.push("ssl certificates (no CA file or directory)".into());
    }
    if !run(&["-c", "import tkinter; tkinter.Tcl().eval('info library')"]) {
        out.push("tkinter (Tcl doesn't start)".into());
    }
    out
}
```

At the end of `verify` (after upstream's loop, before `Ok(())`):

```rust
    if let Some(minor) = super::stdlib::minor_of_step(step) {
        let run = |args: &[&str]| -> bool {
            let s = command(python, &plan.env).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
            s.is_ok_and(|s| s.success())
        };
        let fails = stdlib_failures(&run, minor);
        if !fails.is_empty() {
            let lenient = get(job.env, "RPYENV_DEPS_MODE").as_deref() == Some("lenient");
            if lenient {
                // Spec §5.2's summary, verbatim; the build counts as installed (D13).
                say(&lenient_summary(&plan.prefix, &fails, job.env));
                return Ok(());
            }
            let skip = get(job.env, "RPYENV_SKIP_PREFLIGHT").as_deref() == Some("1");
            let name = plan.prefix.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let lead = if skip { "pyenv: warning: " } else { "pyenv: " };
            say(&format!("{lead}{name} was built without: {}", fails.join(", ")));
            if !skip {
                say("Install the missing libraries (pyenv install asks how), or set RPYENV_SKIP_PREFLIGHT=1 to keep such a build.");
                return Err(InstallError::Failed);
            }
        }
    }
```

(Interruption: if `interrupted()` is true after a `run`, return `Err(InstallError::Interrupted)`
as upstream's loop does.)

`lenient_summary(prefix, fails, env) -> String` builds spec §5.2's three lines: the name list
(the part of each failure before ` (`), `  Add them: <deps::install_argv(...) for the
matching libraries, sudo unless root>, then pyenv install -f <name>`, and the fixed notice
`  In a later rpyenv release this stops the build; choose now with --deps=install or`
`  --deps=system (or RPYENV_BUILD_DEPS).` (read `/etc/os-release`, or
`RPYENV_TEST_OS_RELEASE` in debug builds, for the manager; without one, the `Add them`
line names the libraries instead). Unit-test it with a fixed os-release text.

- [ ] **Step 5: Run.** The tests from Step 2 PASS; `cargo test -p pyenv` (whole crate, Linux:
  in WSL) PASS; `cargo test --workspace` on Windows PASS.

- [ ] **Step 6: Commit.**
```bash
git add crates/pyenv/src/install/builder.rs crates/pyenv/tests/common/fakebuild.rs crates/pyenv/tests/cli_install_deps.rs
git commit -m "M8a task 5: a CPython 3 build counts as installed only when every standard-library module imports and CA certificates exist"
```

---

### Task 6: CI: real distributions in containers

**Files:**
- Create: `ci/check_deps.sh`
- Modify: `.github/workflows/install-linux.yml` (the `build` job's package list; a new `deps` job)

- [ ] **Step 1: The `build` job's package list.** Add `libgdbm-dev libgdbm-compat-dev uuid-dev
  libcrypt-dev libnsl-dev libtirpc-dev pkg-config ca-certificates` to the
  `sudo apt-get install -yq …` line, so the existing CPython build passes M8a's stricter check
  (it would otherwise refuse: the runner lacks gdbm, uuid and nis headers).

- [ ] **Step 2: `ci/check_deps.sh <distro>`** runs inside a container as root:

```bash
#!/usr/bin/env bash
# M8a CI (M8 design §9): in a container with only a compiler and make, `pyenv install`
# without a terminal prints a command; running it makes detection come back empty; then
# --deps=install builds a real CPython whose every module imports.
set -euo pipefail
distro="$1"; v="${2:-3.12.14}"
root="$(mktemp -d)/root"; mkdir -p "$root"
export PYENV_ROOT="$root"
out=$(./pyenv install "$v" 2>&1) && { echo "::error::$distro: install succeeded with nothing installed"; exit 1; }
echo "$out"
cmd=$(printf '%s\n' "$out" | sed -n 's/^  \(.*\(apt-get\|dnf\|zypper\|pacman\) .*\)$/\1/p' | head -1)
[ -n "$cmd" ] || { echo "::error::$distro: no install command printed"; exit 1; }
case "$cmd" in *apt-get*) apt-get update -q ;; esac
yes | eval "${cmd/install/install -y}" || eval "$cmd --noconfirm" || true
# Detection must now be empty: a second run gets past the check (it then builds).
./pyenv install --deps=install "$v"
"$root/versions/$v/bin/python3" -c 'import ssl, tkinter, sqlite3, ctypes, lzma, bz2, zlib, readline, curses, dbm.gnu, uuid; print("ok")'
```

Adjust the `eval` lines to the exact command shape `report_deps` prints (no `sudo` as root);
the point is: the printed command, run as printed (plus the manager's yes flag), must satisfy
detection.

- [ ] **Step 3: The `deps` job.** In `install-linux.yml`, a job with a matrix of containers
  (`debian:stable-slim`, `ubuntu:24.04`, `fedora:41`, `rockylinux:9`,
  `opensuse/tumbleweed`, `archlinux:latest`). Steps: checkout; build `pyenv` on the host
  runner (`cargo build --release -p pyenv`); then
  `docker run --rm -v "$PWD:/w" -w /w <image> bash -c '<install a compiler and make with the
  manager>; cp target/release/pyenv ./pyenv; bash ci/check_deps.sh <distro>'`.
  Per PR: Ubuntu and Fedora run the full script (with the CPython build); the other four run
  only up to "Detection must now be empty" (pass `DETECT_ONLY=1` and stop the script there).
  Weekly (`schedule`): all six run the full script. Rocky's step enables CRB first
  (`dnf config-manager --set-enabled crb`) only after the script has shown the hint once.

- [ ] **Step 4: The discriminating run.** In the Ubuntu leg, before the final build, run once
  with `uuid-dev` removed (`apt-get remove -y uuid-dev`) and `--deps=system`: it must fail
  naming `uuid`; then reinstall it and continue. This proves the check can fail.

- [ ] **Step 5: Test the workflow edit locally with stubs** as M7c Task 4 did
  (`C:\tmp\m7c_t4_*.sh` is the pattern): extract `ci/check_deps.sh`'s parsing of the printed
  command and run it in WSL against canned outputs for apt, dnf, zypper and pacman. A real run
  happens on the PR.

- [ ] **Step 6: Commit.**
```bash
git add ci/check_deps.sh .github/workflows/install-linux.yml
git commit -m "M8a task 6: CI runs pyenv install's dependency check in six distribution containers, and a real build through --deps=install"
```

---

### Task 7: Allowlist, docs, parent spec

**Files:**
- Modify: `docs/parity/allowlist.md` (new rows D-117…)
- Modify: `docs/distributions.md` (CPython on Linux)
- Modify: `README.md` (one line in Goals; `--deps` in Environment variables table: `RPYENV_BUILD_DEPS`)
- Modify: `docs/specs/2026-09-27-rpyenv-design.md` (§15.2: "Designed in M8's spec"; §13's `RPYENV_BUILD_DEPS` row: values now `system`, `install`, `build`, `prebuilt`)

- [ ] **Step 1: Allowlist rows** (read D-110 to D-116 for the shape; cite tests with
  `// allowlist D-NNN` comments, as Task 11 of M7c did):
  - D-117 `install` (Linux, CPython 3): every standard-library extension module with an outside
    library is required, and `tkinter` is probed without `$DISPLAY`; upstream warns for most.
  - D-118 `install`: probes compile and link and read version macros; a too-old library is missing.
  - D-119 `install`: CA certificates are a build requirement.
  - D-120 `install`: at a terminal, a prompt offers to install the packages; `--deps=` and
    `RPYENV_BUILD_DEPS` choose without one.
  - D-121 `install --help` and completion list `--deps=`.
  - D-122 `install`: a build is rolled back when §4.4's check fails (unless
    `RPYENV_SKIP_PREFLIGHT=1`).
- [ ] **Step 2: Docs.** `docs/distributions.md` → under "CPython", a "Missing libraries on
  Linux" subsection (what is checked, the prompt, `--deps`, `RPYENV_SKIP_PREFLIGHT`), in the
  page's plain-language style. README: Goals' CPython line gains "missing libraries are found
  before the build and can be installed for you (Linux)"; the environment-variable table gains
  `RPYENV_BUILD_DEPS`.
- [ ] **Step 3: Checks.** `python parity/coverage.py` → every row covered; the control-character
  grep from the house rules; `cargo test --workspace` (Windows) and the WSL suite.
- [ ] **Step 4: Commit.**
```bash
git add docs/parity/allowlist.md docs/distributions.md README.md docs/specs/2026-09-27-rpyenv-design.md crates
git commit -m "M8a task 7: allowlist D-117 to D-122, docs for missing Linux libraries, parent spec pointers"
```
