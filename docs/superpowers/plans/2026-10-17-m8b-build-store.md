# M8b: build missing libraries from source without root, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Linux, `pyenv install --deps=build <CPython 3.7+>` (or `[b]` at the prompt) compiles
every missing standard-library dependency from source, without root, into a shared store of
static libraries, and builds a CPython in which every module imports.

**Architecture:** A checked-in catalog (`crates/pyenv/deps/catalog.tsv`: version, URL, SHA-256)
plus build recipes in `install/catalog.rs` say how each library is built. `install/store.rs`
builds the missing ones, static and `-fPIC`, into `<root>/cache/deps/<lib>-<version>-<hash>/`
(staged with `DESTDIR`, renamed into place, one lock per entry) and turns the entries into
CPython's flags. `install/record.rs` writes `versions/<v>/.rpyenv-deps` and reads it back to
warn when the catalog moves on. `commands/install.rs` offers the route; `install/builder.rs`
links CPython against the store and copies Tcl/Tk's script folders into the version.

**Tech Stack:** Rust (crate `pyenv`, Linux code paths only), POSIX `sh` recipes run by the
store, GitHub Actions with Ubuntu and Fedora containers, Python 3 for the weekly catalog check.

**Spec:** `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md` (M8b = §3 row 2,
§6.1–6.3, §6.5, §9 row 3). Parent: `docs/specs/2026-09-27-rpyenv-design.md` §15.2.

## Global Constraints

- Linux flavor only. No Windows code path changes; Windows tests must stay green.
- The build route is for CPython **3.7 and later** built from source (ruling P2). Python 2.x,
  3.0–3.6, PyPy and conda keep today's behavior exactly.
- Tcl/Tk **8.6** for every source build (spec D11).
- Static libraries only (`--disable-shared`, `-fPIC`); CPython links them statically, so the
  store is a cache and deleting `cache/deps` breaks no installed version (spec §6.2, D7).
- **Only missing libraries are built** (§6.2): what the system provides is used as it is.
- A library entry is reused when library, version and build options match (spec D6).
- Scripts that set neither `--deps` nor `RPYENV_BUILD_DEPS` stay **lenient** in M8b; the
  strict default moves to M8c (ruling P1, user decision 2026-10-17).
- Never run `sudo`, `apt-get`, `dnf`, `zypper` or `pacman` on a development machine, and never
  build the real libraries there: unit and CLI tests use fakes (`RPYENV_TEST_PREFLIGHT`,
  `RPYENV_TEST_BUILD_TOOLS`, `RPYENV_TEST_CATALOG` with `file://` URLs). Real builds run only in
  CI containers (Task 6).
- Test hooks are honored in debug builds only (`cfg!(debug_assertions)`), as the repo's other
  `RPYENV_TEST_*` hooks.
- Every user-visible difference from upstream pyenv gets an allowlist entry (spec D12), cited
  by a test (`// allowlist D-NNN` above a `#[test]`). The next free ID is **D-123**.
- Commit messages: one line, no attribution lines of any kind.

## Plan rulings (made while planning; the user confirms them at plan review)

- **P1 (user, 2026-10-17):** spec D13's strict default for scripts moves from M8b to M8c. M8b
  can't build X11, so a script without root on a machine without X11 headers would have no way
  to pass. The lenient summary's notice now names `--deps=build` too.
- **P2:** the build route needs CPython 3.7 or later. Older definitions pin OpenSSL 1.0.2 or
  1.1.0, which the recipe doesn't build, and are end-of-life. `[b]` says so when left out.
- **P3:** until M8c, Tk needs the system's X11 headers (`libx11-dev`, `libX11-devel`). Without
  them `tkinter` can't be built: at a terminal `[b]` builds the rest and `tkinter` comes back to
  the prompt (where `[c]`, `[p]` and `[i]` remain); `--deps=build` without a terminal refuses
  before building anything, naming `tkinter` and the package.
- **P4:** OpenSSL and libxcrypt are built by Perl scripts that need Perl's core modules. Without
  them `ssl`, `crypt` and the CA fallback can't be built; the reason names `perl-core` on dnf and
  `perl` elsewhere.
- **P5:** the build route needs a `PYENV_ROOT` path without whitespace or shell characters:
  recipe commands and CPython's flags carry store paths unquoted. `[b]` says so when left out;
  `--deps=build` refuses before any download.
- **P6:** after the store is built, detection counts every library the store supplied as
  present. The system check can't link a static store library without its private
  dependencies; spec §4.4's check after the build is what proves the store's libraries work.
- **P7:** library builds get `CFLAGS=-O2 -fPIC` and the user's `CC`. The user's `CPPFLAGS`,
  `LDFLAGS`, `LIBS`, `CFLAGS`, `CXXFLAGS`, `PKG_CONFIG_PATH` and `PKG_CONFIG_LIBDIR` are for
  CPython and are removed from the libraries' environment.
- **P8:** the store's `CPPFLAGS`/`LDFLAGS` come after the user's own, and its `configure`
  arguments before the user's `CONFIGURE_OPTS`/`PYTHON_CONFIGURE_OPTS`, so the user's choices win.
- **P9:** the pinned CA bundle (spec §6.5) lives at `$PYENV_ROOT/.rpyenv/ca/cert.pem`, outside
  the cache (user decision 2026-10-17: deleting `cache/deps` must not break HTTPS for versions
  whose OpenSSL reads the bundle). It is fetched only when OpenSSL is built with the bundle's
  folder as `OPENSSLDIR`; `pyenv install` refreshes it when the file exists and its
  hash differs from the catalog's.
- **P10:** a missing `pkg-config` is supplied by building pkgconf into the store; its `bin/`
  goes first on `PATH` for the later library builds and for CPython's build.
- **P11:** libraries that need each other are built together and their archives merged with
  `ar -M`: readline with ncurses, Tk with Tcl, libnsl with libtirpc, and `libgdbm_compat.a`
  with `libgdbm.a`. CPython's extension modules don't get `LIBS` on their link line
  (measured), so each `-l` must stand alone.
- **P12:** the stale-version warning (spec §6.3) compares versions for inequality ("now X"),
  not order. A row pinned by a definition is compared with that definition's pin today.

## Measured (2026-10-17, throwaway podman containers on the dev host; method in each row)

Library builds, CPython builds against them, and the §4.4 check, in `ubuntu:24.04` (packages:
`build-essential libx11-dev`, no `-dev` package of any library, no `pkg-config`, no
`ca-certificates`) and `fedora:44` (`gcc make patch perl-core libX11-devel`). Scripts:
`C:\tmp\m8b_spike\r3.sh`, `r4.sh` (each library staged with `DESTDIR` and renamed, as Task 2
does).

| Fact | Evidence |
|---|---|
| Every catalog download matches its row's SHA-256 | `sha256sum` of each download. OpenSSL 3.6.5/4.0.3/3.0.16/1.1.1u, readline 8.3 and Python tarballs also match python-build's pins; `cacert-2026-09-25.pem` matches curl's `.sha256` |
| The 17 recipes build on Ubuntu 24.04 and Fedora 44 | Store directories present after the run, and the CPython rows below importing every module each library serves |
| CPython 3.14.8 (with upstream's `0001-…-Add-OpenSSL-4-support.patch`, OpenSSL 4.0.3) against the store: 15/15 modules import, TLS to pypi.org works, `info library` is inside the version | `r4.sh`, Ubuntu (TLS through the pinned bundle) and Fedora (`/etc/pki/tls`) |
| CPython 3.12.15 (OpenSSL 3.6.5) against the store: 16/16 modules import (`crypt` and `nis` included), TLS works | `r4.sh`, both distributions; `r3.sh` had 15/16 before the `.pc` fold |
| OpenSSL 1.1.1u, 3.0.16, 3.6.5 and 4.0.3 build with `./config … && make build_libs && make install_dev`; `make build_sw` doesn't exist in 1.1.1, and `make -j build_libs build_programs` races and fails on all four | `r5.sh`, `r6.sh` (Ubuntu; each build in its own `bash -ec`) |
| CPython 3.8.20 against OpenSSL 1.1.1u imports `ssl` and `_hashlib`; its `_uuid` needs `-I<libuuid>/include/uuid` (3.8's `setup.py` looks for `uuid.h` itself) | `r5.sh`, `r6.sh` |
| `_sqlite3` is "not found" until `Libs.private` (`-lm`) is folded into `Libs` in the store's `sqlite3.pc` | `r3.sh` vs `r4.sh` |
| Without the `ar -M` merges, `_dbm` (`-lgdbm_compat` alone), `readline` (`undefined symbol: UP`) and `nis` (libtirpc) are dropped as "could not be imported"; with them all three import | Round 2 vs round 3 |
| `_curses` needs `-I<ncurses>/include/ncursesw` (`term.h`'s `setupterm`) | Round 1 compile error |
| `_crypt` doesn't link against libxcrypt built with symbol versions (`version node not found for symbol crypt_checksalt@@XCRYPT_4.3`); `--disable-symvers` fixes it | Round 1 vs 2 |
| Tcl's compiled-in library path wins over the copy in the version while the store exists; building with `TCL_LIBRARY=/nonexistent/rpyenv/tcl8.6` (and `TK_LIBRARY` likewise) makes Tcl find `versions/<v>/lib/tcl8.6` | Round 1 (`/store/tcl/lib/tcl8.6`) vs round 2 (`/py/3.10.22/lib/tcl8.6`) |
| With the store moved aside, the built CPython still imports `tkinter`, `readline`, `sqlite3`, `dbm.ndbm` and `ssl` | Round 2 and 3, `mv cache/deps` then import |
| `_ssl`, `readline`, `_curses`, `_dbm`, `nis` link no shared library outside libc; `_tkinter` links the system's `libX11` | `ldd` |
| Fedora 44 `perl-interpreter` alone lacks `FindBin`, `lib`, `IPC::Cmd`, `File::Compare`, `File::Copy`, `Digest::SHA` (used by OpenSSL's `Configure` and libxcrypt's scripts) | `perl -M<module> -e1` for every `use` in OpenSSL 1.1.1u/3.6.5/4.0.3 and libxcrypt 4.5.2 build scripts |
| CPython 3.10.22 builds against the store with `--with-tcltk-includes`/`--with-tcltk-libs`; 3.11+ use `TCLTK_CFLAGS`/`TCLTK_LIBS` | Rounds 2–3 |

**Not measured by the spike** (Task 6 covers them): the full 3.8 module set against the whole
store (only `ssl`, `_hashlib` and `_uuid` were checked; the weekly leg builds 3.8), and Debian,
Rocky, openSUSE and Arch (weekly legs). The spike's rounds 1–4 counted a library as built from
a subshell whose `set -e` bash ignores; the rows above rest on store directories and imports,
not on those counts.

## Review Focus

1. **Two `pyenv install` runs build the same library at once** (two terminals, parallel CI jobs
   on one root): one builds it, the other waits and reuses the entry. Pinned in Task 2
   (`two_builders_of_one_entry_build_it_once`).
2. **Ctrl+C during a library build** must exit 130 and leave neither a store entry nor a
   `.tmp-*` folder; the next run builds the library again. Pinned in Task 4
   (`an_interrupt_during_a_library_build_leaves_no_store_entry`).
3. **The store deleted after an install** (`rm -rf cache/deps`): installed versions keep working
   and the next `--deps=build` builds again. Pinned in Task 6 (the build leg moves `cache/deps`
   aside and imports again) and Task 2 (`a_missing_entry_is_built_again`).
4. **A `PYENV_ROOT` with a space** must be refused before any download, naming the path. Pinned
   in Task 4 (`a_root_with_a_space_refuses_the_build_route_before_downloading`).
5. **A definition with no pin** (a user's own definition file) gets the catalog's OpenSSL and
   readline, recorded as `catalog`. Pinned in Task 1
   (`a_definition_without_a_pin_gets_the_catalog_row`).

---

### Task 1: The catalog (`deps/catalog.tsv`, `install/catalog.rs`)

**Files:**
- Create: `crates/pyenv/deps/catalog.tsv`
- Create: `crates/pyenv/src/install/catalog.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `#[cfg(unix)] pub mod catalog;` after `builder`)

**Interfaces:**
- Consumes: `install::defs::{Definition, Package, Fetch}` (existing), `install::stdlib::{modules, CA_KEY}` (existing).
- Produces:
  - `pub struct Row { pub key: String, pub version: String, pub url: String, pub sha256: String }` (derives `Debug, Clone, PartialEq, Eq`)
  - `pub enum Origin { Catalog, Definition }` (derives `Debug, Clone, Copy, PartialEq, Eq`) with `pub fn word(self) -> &'static str` (`"catalog"`, `"definition"`) and `pub fn from_word(w: &str) -> Option<Origin>`
  - `pub struct Recipe { pub key: &'static str, pub dir: &'static str, pub needs: &'static [&'static str], pub perl: bool, pub x11: bool, pub includes: &'static [&'static str], pub steps: &'static [&'static str] }`
  - `pub const ORDER: [&str; 17]`
  - `pub fn parse(text: &str) -> Result<Vec<Row>, String>`
  - `pub fn table(env: &dyn Fn(&str) -> Option<String>) -> Result<Vec<Row>, String>` (debug: `RPYENV_TEST_CATALOG`)
  - `pub fn recipe(key: &str) -> Option<&'static Recipe>`
  - `pub fn keys_for(lib: &str) -> &'static [&'static str]`
  - `pub fn plan(libs: &[&str]) -> Vec<&'static str>`
  - `pub fn pinned(key: &str, def: &Definition) -> Option<Row>`
  - `pub fn resolve(rows: &[Row], key: &str, def: &Definition) -> Option<(Row, Origin)>`

- [ ] **Step 1: Write the catalog file** `crates/pyenv/deps/catalog.tsv` (tabs between fields;
  copy the values exactly):

```text
# M8 design §6.1: the libraries the build route compiles. One row per library: key, version,
# source URL and SHA-256, separated by tabs. ci/deps_catalog.py moves rows to new upstream
# releases weekly; a definition's own pin wins for openssl and readline (catalog::resolve).
# Hashes: computed from a download on 2026-10-17; openssl and readline also match
# python-build's pins, cacert matches curl's published .sha256.
pkgconf	3.0.9	https://distfiles.ariadne.space/pkgconf/pkgconf-3.0.9.tar.xz	6ab2d3b8087dcf500e34003a6a8934dca55d0a3d6066aeafb03c352db3e68bfd
zlib	1.3.2	https://github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz	bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16
bzip2	1.0.8	https://sourceware.org/pub/bzip2/bzip2-1.0.8.tar.gz	ab5a03176ee106d3f0fa90e381da478ddae405918153cca248e682cd0c4a2269
xz	5.8.4	https://github.com/tukaani-project/xz/releases/download/v5.8.4/xz-5.8.4.tar.gz	0014c7886930454fe8bd4228665b51af55eeae560ea135c9c4cd33f55b2591d9
ncurses	6.6	https://ftp.gnu.org/gnu/ncurses/ncurses-6.6.tar.gz	355b4cbbed880b0381a04c46617b7656e362585d52e9cf84a67e2009b749ff11
readline	8.3	https://ftp.gnu.org/gnu/readline/readline-8.3.tar.gz	fe5383204467828cd495ee8d1d3c037a7eba1389c22bc6a041f627976f9061cc
gdbm	1.26	https://ftp.gnu.org/gnu/gdbm/gdbm-1.26.tar.gz	6a24504a14de4a744103dcb936be976df6fbe88ccff26065e54c1c47946f4a5e
sqlite	3.54.1	https://sqlite.org/2026/sqlite-autoconf-3540100.tar.gz	926ef8d3eab93fbf6e06011901273eec076b25594a4e1cdadd50c987f4f6d64e
libffi	3.8.0	https://github.com/libffi/libffi/releases/download/v3.8.0/libffi-3.8.0.tar.gz	7da3e2d9a171eb0a038f592ecad3ff2bb2550f3496d87b3b29ad0cf4430c0db4
tcl	8.6.18	https://downloads.sourceforge.net/project/tcl/Tcl/8.6.18/tcl8.6.18-src.tar.gz	14f9af32b1767ff718477a8f974ad03c34341097e6b43f4ce54644ee974e268e
tk	8.6.18	https://downloads.sourceforge.net/project/tcl/Tcl/8.6.18/tk8.6.18-src.tar.gz	95cd528a80f5e4bdb557af9b14a7197d6860793a3894e25e7c9fad2ed05d4c3c
libuuid	2.42	https://mirrors.edge.kernel.org/pub/linux/utils/util-linux/v2.42/util-linux-2.42.tar.xz	3452b260bbaa775d6e749ac3bb22111785003fc1f444970025c8da26dfa758e9
libxcrypt	4.5.2	https://github.com/besser82/libxcrypt/releases/download/v4.5.2/libxcrypt-4.5.2.tar.xz	71513a31c01a428bccd5367a32fd95f115d6dac50fb5b60c779d5c7942aec071
libtirpc	1.3.8	https://downloads.sourceforge.net/project/libtirpc/libtirpc/1.3.8/libtirpc-1.3.8.tar.bz2	8839959bfcc7a0f4c609d8e4f53f1c67ae33de23775ec35beb39ff15adf11920
libnsl	2.0.1	https://github.com/thkukuk/libnsl/releases/download/v2.0.1/libnsl-2.0.1.tar.xz	5c9e470b232a7acd3433491ac5221b4832f0c71318618dc6aa04dd05ffcd8fd9
zstd	1.5.7	https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.gz	eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3
openssl	3.6.5	https://github.com/openssl/openssl/releases/download/openssl-3.6.5/openssl-3.6.5.tar.gz	a2157c2830efdec3788939b00c9b0638306d3f0bbb76dc4832ee503bb397df98
cacert	2026-09-25	https://curl.se/ca/cacert-2026-09-25.pem	a41b5d356aea97a529fe27e0f7316d2f9d946d75927476cf9cf1b90637d00505
```

- [ ] **Step 2: Write the failing tests** at the bottom of the new `catalog.rs` (with only the
  module doc and `use` lines above them for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::defs::{self, Package};
    use std::path::Path;

    #[test]
    fn the_shipped_catalog_has_a_row_and_a_recipe_for_each_library() {
        let rows = parse(EMBEDDED).unwrap();
        for k in ORDER {
            assert!(rows.iter().any(|r| r.key == k), "{k} has no row");
            assert!(recipe(k).is_some(), "{k} has no recipe");
        }
        assert!(rows.iter().any(|r| r.key == "cacert"));
        assert_eq!(rows.len(), ORDER.len() + 1, "one row per library, plus cacert");
        for r in &rows {
            assert!(r.url.starts_with("https://"), "{}", r.url);
        }
        let mut keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), rows.len(), "a key appears twice");
    }

    #[test]
    fn malformed_rows_are_refused_with_their_line() {
        let sha = "a".repeat(64);
        let ok = format!("# comment\n\nzlib\t1.3.2\thttps://x/z.tar.gz\t{sha}\n");
        assert_eq!(parse(&ok).unwrap().len(), 1);
        assert!(parse("zlib\t1.3.2\thttps://x/z.tar.gz\n")
            .unwrap_err()
            .contains("line 1"));
        let upper = format!("zlib\t1.3.2\thttps://x/z.tar.gz\t{}\n", "A".repeat(64));
        assert!(parse(&upper).is_err(), "SHA-256 must be lowercase hex");
        let short = "zlib\t1.3.2\thttps://x/z.tar.gz\tabc\n";
        assert!(parse(short).is_err());
    }

    #[test]
    fn every_recipe_names_only_dependencies_built_before_it() {
        for r in RECIPES {
            let at = ORDER.iter().position(|k| *k == r.key).unwrap();
            for n in r.needs {
                let dep = ORDER.iter().position(|k| k == n).expect(n);
                assert!(dep < at, "{} needs {n}, built later", r.key);
            }
            for s in r.steps {
                let mut rest = *s;
                while let Some(i) = rest.find("{dep:") {
                    let tail = &rest[i + 5..];
                    let k = &tail[..tail.find('}').unwrap()];
                    assert!(r.needs.contains(&k), "{} uses {{dep:{k}}} without needing it", r.key);
                    rest = &tail[k.len()..];
                }
            }
        }
    }

    #[test]
    fn every_library_the_check_names_has_catalog_keys() {
        for minor in [7, 12, 13, 14] {
            for m in stdlib::modules(minor) {
                assert!(!keys_for(m.lib).is_empty(), "{} has no catalog key", m.lib);
            }
        }
        assert_eq!(keys_for(stdlib::CA_KEY), ["openssl"]);
        assert_eq!(keys_for("pkg-config"), ["pkgconf"]);
        for system_only in ["compiler", "make", "patch"] {
            assert!(keys_for(system_only).is_empty(), "{system_only}");
        }
    }

    #[test]
    fn the_plan_keeps_dependencies_together_and_in_order() {
        assert_eq!(
            plan(&["tk", "readline", "libnsl"]),
            ["ncurses", "readline", "tcl", "tk", "libtirpc", "libnsl"]
        );
        assert_eq!(plan(&["openssl", stdlib::CA_KEY]), ["openssl"]);
        assert_eq!(plan(&["zstd", "pkg-config"]), ["pkgconf", "zstd"]);
        assert!(plan(&["compiler"]).is_empty());
    }

    fn def_with(pkgs: &[(&str, &str, &str)]) -> Definition {
        let mut d = Definition::default();
        for (name, url, cond) in pkgs {
            d.packages.push(Package {
                name: name.to_string(),
                fetch: Fetch::Tarball {
                    url: url.to_string(),
                    fragment: Some("b".repeat(64)),
                },
                steps: vec![],
                condition: (!cond.is_empty()).then(|| cond.to_string()),
            });
        }
        d
    }

    #[test]
    fn a_definitions_pin_wins_for_openssl_and_readline() {
        let rows = parse(EMBEDDED).unwrap();
        let d = def_with(&[
            ("openssl-1.1.1u", "https://www.openssl.org/source/openssl-1.1.1u.tar.gz", "has_broken_mac_openssl"),
            ("readline-8.1", "https://ftpmirror.gnu.org/readline/readline-8.1.tar.gz", "has_broken_mac_readline"),
            ("Python-3.8.20", "https://www.python.org/ftp/python/3.8.20/Python-3.8.20.tar.xz", ""),
        ]);
        let (r, o) = resolve(&rows, "openssl", &d).unwrap();
        assert_eq!((r.version.as_str(), o), ("1.1.1u", Origin::Definition));
        assert_eq!(r.url, "https://www.openssl.org/source/openssl-1.1.1u.tar.gz");
        assert_eq!(r.sha256, "b".repeat(64));
        assert_eq!(resolve(&rows, "readline", &d).unwrap().0.version, "8.1");
        // Every other library comes from the catalog.
        let (z, o) = resolve(&rows, "zlib", &d).unwrap();
        assert_eq!((z.version.as_str(), o), ("1.3.2", Origin::Catalog));
    }

    // Review Focus 5
    #[test]
    fn a_definition_without_a_pin_gets_the_catalog_row() {
        let rows = parse(EMBEDDED).unwrap();
        // An unconditional `openssl-*` package is no pin: only the macOS-only ones are.
        let d = def_with(&[("openssl-9.9.9", "https://example.org/o.tar.gz", "")]);
        let (r, o) = resolve(&rows, "openssl", &d).unwrap();
        assert_eq!((r.version.as_str(), o), ("3.6.5", Origin::Catalog));
        assert_eq!(Origin::from_word(o.word()), Some(o));
    }

    /// Every vendored CPython 3.7+ definition that pins OpenSSL pins a series the recipe
    /// builds (1.1.1, 3.x or 4.x), with a SHA-256.
    #[test]
    fn every_vendored_cpython_3_7_and_later_pins_an_openssl_the_recipe_builds() {
        let env = |_: &str| None;
        let root = Path::new("/nonexistent-rpyenv-root");
        let mut checked = 0;
        for name in defs::known(&env) {
            let Some(minor) = name
                .strip_prefix("3.")
                .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|m| m.parse::<u32>().ok())
            else {
                continue;
            };
            if minor < 7 {
                continue;
            }
            let Some(found) = defs::find(root, &name, &env) else { continue };
            let Ok(d) = defs::parse(&found, &env, &|n| defs::find(root, n, &env)) else {
                continue;
            };
            if let Some(r) = pinned("openssl", &d) {
                assert!(
                    ["1.1.1", "3.", "4."].iter().any(|p| r.version.starts_with(p)),
                    "{name}: openssl {}",
                    r.version
                );
                assert_eq!(r.sha256.len(), 64, "{name}");
                checked += 1;
            }
        }
        assert!(checked > 100, "only {checked} definitions checked");
    }
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run (WSL, see "Running the tests" below): `cargo test -q -p pyenv --lib -- catalog::`
Expected: compile errors (`parse`, `ORDER`, `RECIPES`, … not found).

- [ ] **Step 4: Write the implementation** above the tests in `catalog.rs`:

```rust
//! The libraries the build route compiles without root (M8 design §6.1): each library's
//! version, source and SHA-256 come from `deps/catalog.tsv`; how to build it is here. The
//! recipes were measured in Ubuntu 24.04 and Fedora 44 containers on 2026-10-17 (plan M8b,
//! "Measured").

use super::defs::{Definition, Fetch};
use super::stdlib;

/// One row of `deps/catalog.tsv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// Where a library's version came from: the catalog, or the definition's own pin, which
/// wins (M8 design §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Catalog,
    Definition,
}

impl Origin {
    pub fn word(self) -> &'static str {
        match self {
            Origin::Catalog => "catalog",
            Origin::Definition => "definition",
        }
    }

    pub fn from_word(w: &str) -> Option<Origin> {
        match w {
            "catalog" => Some(Origin::Catalog),
            "definition" => Some(Origin::Definition),
            _ => None,
        }
    }
}

/// How one library is built: shell command lines run with `sh -c`, in order, in the source
/// tree's `dir`. `store::expand` fills in `{prefix}` (the entry), `{destdir}` (where `make
/// install` stages it), `{stage}` (`{destdir}{prefix}`), `{jobs}`, `{triple}` (`cc
/// -dumpmachine`), `{openssldir}` and `{dep:<key>}` (the entry of a library in `needs`).
pub struct Recipe {
    pub key: &'static str,
    /// The subdirectory of the source tree the commands run in (`unix` for Tcl and Tk).
    pub dir: &'static str,
    /// Built first, always together with this one (plan ruling P11).
    pub needs: &'static [&'static str],
    /// Its build runs Perl scripts that need Perl's core modules (ruling P4).
    pub perl: bool,
    /// It needs the system's X11 headers and libX11 (Tk, until M8c; ruling P3).
    pub x11: bool,
    /// Include directories under the entry besides `include`, for CPython's `CPPFLAGS`.
    pub includes: &'static [&'static str],
    pub steps: &'static [&'static str],
}

const EMBEDDED: &str = include_str!("../../deps/catalog.tsv");

/// The order libraries are built in: each after the ones it needs.
pub const ORDER: [&str; 17] = [
    "pkgconf", "zlib", "bzip2", "xz", "ncurses", "readline", "gdbm", "sqlite", "libffi", "tcl",
    "tk", "libuuid", "libxcrypt", "libtirpc", "libnsl", "zstd", "openssl",
];

/// `make install` with `DESTDIR` (three steps every autotools recipe ends with).
macro_rules! autotools {
    ($configure:expr) => {
        &[$configure, "make -j{jobs}", "make install DESTDIR={destdir}"]
    };
}

static RECIPES: &[Recipe] = &[
    Recipe {
        key: "pkgconf",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: &[
            "./configure --prefix={prefix} --disable-shared --with-system-libdir=/lib:/usr/lib:/lib64:/usr/lib64 --with-system-includedir=/usr/include --with-pkg-config-dir=/usr/local/lib/pkgconfig:/usr/local/share/pkgconfig:/usr/lib/{triple}/pkgconfig:/usr/lib64/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig",
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            "ln -s pkgconf {stage}/bin/pkg-config",
        ],
    },
    Recipe {
        key: "zlib",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: autotools!("./configure --static --prefix={prefix}"),
    },
    Recipe {
        key: "bzip2",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: &[
            r#"make -j{jobs} libbz2.a CC="${CC:-cc}" "CFLAGS=-O2 -fPIC -D_FILE_OFFSET_BITS=64""#,
            "install -D -m644 bzlib.h {stage}/include/bzlib.h",
            "install -D -m644 libbz2.a {stage}/lib/libbz2.a",
        ],
    },
    Recipe {
        key: "xz",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-xz --disable-xzdec --disable-lzmadec --disable-lzmainfo --disable-scripts --disable-doc --disable-nls"),
    },
    Recipe {
        key: "ncurses",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        // `term.h` (`setupterm`, for `_curses`) is only there (measured).
        includes: &["include/ncursesw"],
        steps: autotools!("./configure --prefix={prefix} --without-shared --with-normal --enable-widec --without-debug --without-ada --without-cxx-binding --without-manpages --without-progs --without-tests --enable-pc-files --with-pkg-config-libdir={prefix}/lib/pkgconfig --with-default-terminfo-dir=/usr/share/terminfo --with-terminfo-dirs=/etc/terminfo:/lib/terminfo:/usr/share/terminfo"),
    },
    Recipe {
        key: "readline",
        dir: "",
        needs: &["ncurses"],
        perl: false,
        x11: false,
        includes: &[],
        steps: &[
            "CPPFLAGS='-I{dep:ncurses}/include -I{dep:ncurses}/include/ncursesw' ./configure --prefix={prefix} --disable-shared --enable-static --with-curses",
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            // ncurses joins the archive: `-lreadline` then stands alone (ruling P11).
            r"cd {stage}/lib && mv libreadline.a in-libreadline.a && printf 'create libreadline.a\naddlib in-libreadline.a\naddlib {dep:ncurses}/lib/libncursesw.a\nsave\nend\n' | ar -M && rm in-libreadline.a",
        ],
    },
    Recipe {
        key: "gdbm",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: &[
            "./configure --prefix={prefix} --disable-shared --enable-static --with-pic --enable-libgdbm-compat --without-readline --disable-nls",
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            // `_dbm` links `-lgdbm_compat` alone (ruling P11).
            r"cd {stage}/lib && mv libgdbm_compat.a in-libgdbm_compat.a && printf 'create libgdbm_compat.a\naddlib in-libgdbm_compat.a\naddlib libgdbm.a\nsave\nend\n' | ar -M && rm in-libgdbm_compat.a",
        ],
    },
    Recipe {
        key: "sqlite",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static"),
    },
    Recipe {
        key: "libffi",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-docs --disable-multi-os-directory"),
    },
    Recipe {
        key: "tcl",
        dir: "unix",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        // The compiled-in library path names a folder that never exists, so Tcl finds the copy
        // in the version (`<prefix>/lib/tcl8.6`) and never the store's (measured).
        steps: &[
            "./configure --prefix={prefix} --disable-shared --enable-threads",
            "make -j{jobs} TCL_LIBRARY=/nonexistent/rpyenv/tcl8.6 binaries libraries",
            "make install-binaries install-libraries install-headers INSTALL_ROOT={destdir}",
        ],
    },
    Recipe {
        key: "tk",
        dir: "unix",
        needs: &["tcl"],
        perl: false,
        x11: true,
        includes: &[],
        steps: &[
            "./configure --prefix={prefix} --disable-shared --with-tcl={dep:tcl}/lib",
            "make -j{jobs} TK_LIBRARY=/nonexistent/rpyenv/tk8.6 binaries libraries",
            "make install-binaries install-libraries install-headers INSTALL_ROOT={destdir}",
        ],
    },
    Recipe {
        key: "libuuid",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        // CPython 3.8's setup.py looks for `uuid.h` itself, not `uuid/uuid.h` (measured).
        includes: &["include/uuid"],
        steps: autotools!("./configure --prefix={prefix} --disable-all-programs --enable-libuuid --disable-shared --enable-static --with-pic --without-python --disable-nls --disable-bash-completion --without-systemd --without-udev --disable-liblastlog2"),
    },
    Recipe {
        key: "libxcrypt",
        dir: "",
        needs: &[],
        perl: true,
        x11: false,
        includes: &[],
        // Without `--disable-symvers`, `_crypt` doesn't link (measured).
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-symvers --disable-failure-tokens"),
    },
    Recipe {
        key: "libtirpc",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &["include/tirpc"],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-gssapi"),
    },
    Recipe {
        key: "libnsl",
        dir: "",
        needs: &["libtirpc"],
        perl: false,
        x11: false,
        includes: &[],
        steps: &[
            r#"TIRPC_CFLAGS=-I{dep:libtirpc}/include/tirpc TIRPC_LIBS="-L{dep:libtirpc}/lib -ltirpc" ./configure --prefix={prefix} --disable-shared --enable-static --with-pic"#,
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            r"cd {stage}/lib && mv libnsl.a in-libnsl.a && printf 'create libnsl.a\naddlib in-libnsl.a\naddlib {dep:libtirpc}/lib/libtirpc.a\nsave\nend\n' | ar -M && rm in-libnsl.a",
        ],
    },
    Recipe {
        key: "zstd",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        includes: &[],
        steps: &[
            "make -C lib -j{jobs} libzstd.a",
            "make -C lib install-static install-includes install-pc PREFIX={prefix} DESTDIR={destdir}",
        ],
    },
    Recipe {
        key: "openssl",
        dir: "",
        needs: &[],
        perl: true,
        x11: false,
        includes: &[],
        // `./config` and `build_libs`/`install_dev` exist in 1.1.1, 3.x and 4.x alike (1.1.1 has
        // no `build_sw`); CPython needs the libraries and headers, not the `openssl` program
        // (measured on 1.1.1u, 3.0.16, 3.6.5 and 4.0.3).
        steps: &[
            "./config --prefix={prefix} --libdir=lib --openssldir={openssldir} no-shared no-tests -fPIC",
            "make -j{jobs} build_libs",
            "make install_dev DESTDIR={destdir}",
        ],
    },
];

/// The rows of a catalog file: key, version, URL and lowercase SHA-256, separated by tabs;
/// `#` lines and blank lines are skipped. A malformed row is an error naming its line.
pub fn parse(text: &str) -> Result<Vec<Row>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let hex = |s: &str| s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if f.len() != 4 || f[0].is_empty() || f[1].is_empty() || f[2].is_empty() || !hex(f[3]) {
            return Err(format!("the build catalog, line {}: {line}", i + 1));
        }
        out.push(Row {
            key: f[0].into(),
            version: f[1].into(),
            url: f[2].into(),
            sha256: f[3].into(),
        });
    }
    Ok(out)
}

/// The catalog rpyenv ships. Debug builds read the file `RPYENV_TEST_CATALOG` names instead.
pub fn table(env: &dyn Fn(&str) -> Option<String>) -> Result<Vec<Row>, String> {
    if cfg!(debug_assertions) {
        if let Some(p) = env("RPYENV_TEST_CATALOG").filter(|p| !p.is_empty()) {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
            return parse(&text);
        }
    }
    parse(EMBEDDED)
}

pub fn recipe(key: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.key == key)
}

/// The catalog keys that supply a `Dep::lib` (a `stdlib` key, `ca-certificates` or
/// `pkg-config`). Empty for what only the system provides: the compiler, `make`, `patch`.
pub fn keys_for(lib: &str) -> &'static [&'static str] {
    match lib {
        // A self-built OpenSSL reads the pinned bundle when the system has no certificates.
        "openssl" | stdlib::CA_KEY => &["openssl"],
        "zlib" => &["zlib"],
        "bzip2" => &["bzip2"],
        "xz" => &["xz"],
        "readline" => &["ncurses", "readline"],
        "sqlite" => &["sqlite"],
        "libffi" => &["libffi"],
        "ncurses" => &["ncurses"],
        "tk" => &["tcl", "tk"],
        "gdbm" => &["gdbm"],
        "libuuid" => &["libuuid"],
        "libcrypt" => &["libxcrypt"],
        "libnsl" => &["libtirpc", "libnsl"],
        "zstd" => &["zstd"],
        "pkg-config" => &["pkgconf"],
        _ => &[],
    }
}

/// Every catalog key `libs` (`Dep::lib` values) need, each once, in `ORDER`.
pub fn plan(libs: &[&str]) -> Vec<&'static str> {
    ORDER
        .iter()
        .copied()
        .filter(|k| libs.iter().any(|l| keys_for(l).contains(k)))
        .collect()
}

/// A definition's own pin of `key`: python-build's macOS-only `openssl-*` and `readline-*`
/// packages (`--if has_broken_mac_openssl`, `--if has_broken_mac_readline`), which Linux
/// otherwise skips.
pub fn pinned(key: &str, def: &Definition) -> Option<Row> {
    let (prefix, condition) = match key {
        "openssl" => ("openssl-", "has_broken_mac_openssl"),
        "readline" => ("readline-", "has_broken_mac_readline"),
        _ => return None,
    };
    def.packages.iter().rev().find_map(|p| {
        let version = p.name.strip_prefix(prefix)?;
        if p.condition.as_deref() != Some(condition) {
            return None;
        }
        match &p.fetch {
            Fetch::Tarball {
                url,
                fragment: Some(sha),
            } => Some(Row {
                key: key.into(),
                version: version.into(),
                url: url.clone(),
                sha256: sha.to_ascii_lowercase(),
            }),
            _ => None,
        }
    })
}

/// `key`'s row for this definition: its pin when it has one, else the catalog's.
pub fn resolve(rows: &[Row], key: &str, def: &Definition) -> Option<(Row, Origin)> {
    pinned(key, def)
        .map(|r| (r, Origin::Definition))
        .or_else(|| {
            rows.iter()
                .find(|r| r.key == key)
                .cloned()
                .map(|r| (r, Origin::Catalog))
        })
}
```

- [ ] **Step 5: Register the module** in `crates/pyenv/src/install/mod.rs`, after
  `pub mod builder;`:

```rust
#[cfg(unix)]
pub mod catalog;
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -q -p pyenv --lib -- catalog::`
Expected: 8 passed.

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv/deps/catalog.tsv crates/pyenv/src/install/catalog.rs crates/pyenv/src/install/mod.rs
git commit -m "M8b: the build catalog: pinned library sources and their static recipes"
```

---

### Task 2: The store and its record (`install/store.rs`, `install/record.rs`)

**Files:**
- Create: `crates/pyenv/src/install/store.rs`
- Create: `crates/pyenv/src/install/record.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `#[cfg(unix)] pub mod record;` and `#[cfg(unix)] pub mod store;`)
- Modify: `crates/pyenv/src/install/builder.rs:325` (`fn logged` becomes `pub(crate) fn logged`)

**Interfaces:**
- Consumes: Task 1's `catalog::{Row, Origin, Recipe, recipe, resolve}`; existing
  `builder::{get, logged}`, `archive::{extract, Kind}`, `fetch::{Check, FetchRequest, Fetcher}`,
  `log::BuildLog`, `checksum::sha256_file`, `install::{interrupted, InstallError}`.
- Produces (`store.rs`):
  - `pub struct Entry { pub key: &'static str, pub version: String, pub origin: Origin, pub dir: PathBuf }` (derives `Debug, Clone, PartialEq, Eq`)
  - `pub fn deps_dir(root: &Path) -> PathBuf` (`<root>/cache/deps`)
  - `pub fn ca_dir(root: &Path) -> PathBuf` (`<root>/.rpyenv/ca`, outside the cache; ruling P9)
  - `pub fn plain_path(p: &Path) -> bool`
  - `pub fn system_openssldir(exists: &dyn Fn(&str) -> bool, has_files: &dyn Fn(&str) -> bool) -> Option<&'static str>`
  - `pub fn entry_name(key: &str, version: &str, fingerprint: &str) -> String`
  - `pub fn fingerprint(r: &Recipe, row: &Row, cc: &str, triple: &str, openssldir: &str, deps: &[String]) -> String`
  - `pub fn expand(step: &str, vars: &[(String, String)]) -> String`
  - `pub fn fold_private(pc: &str) -> String`
  - `pub struct Build<'a> { pub root: &'a Path, pub rows: &'a [Row], pub definition: &'a Definition, pub env: &'a [(OsString, OsString)], pub fetcher: &'a Fetcher, pub openssldir: PathBuf, pub verbose: bool }`
  - `pub fn ensure(b: &Build, keys: &[&'static str], say: &mut dyn FnMut(&str)) -> Result<Vec<Entry>, InstallError>`
  - `pub fn place_ca(root: &Path, rows: &[Row], fetcher: &Fetcher, say: &mut dyn FnMut(&str)) -> Result<bool, InstallError>`
  - `pub struct Linked { pub entries: Vec<Entry>, pub cppflags: Vec<String>, pub ldflags: Vec<String>, pub pkg_config_path: Vec<PathBuf>, pub path: Option<PathBuf>, pub configure: Vec<String>, pub configure_env: Vec<(String, String)>, pub scripts: Vec<PathBuf> }`
  - `pub fn link(entries: Vec<Entry>, minor: u32, read: &dyn Fn(&Path) -> Option<String>) -> Linked`
- Produces (`record.rs`):
  - `pub const FILE: &str = ".rpyenv-deps";`
  - `pub struct Record { pub definition: String, pub libs: Vec<(String, String, Origin)> }`
  - `pub fn text(definition: &str, entries: &[Entry]) -> String`
  - `pub fn parse(text: &str) -> Option<Record>`

- [ ] **Step 1: Make the builder's runner reachable.** In `builder.rs`, change
  `fn logged(log: &BuildLog, cmd: &mut Command) -> R<()> {` to
  `pub(crate) fn logged(log: &BuildLog, cmd: &mut Command) -> R<()> {`. Nothing else changes.

- [ ] **Step 2: Write the failing tests** at the bottom of the new `store.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::defs::Definition;
    use std::io::Write;

    /// The fake's `configure`: counts its runs in COUNTER, records its environment, stops at
    /// FAIL when asked to, remembers `--prefix`, and writes a Makefile whose `install` runs
    /// `install.sh` with DESTDIR.
    const CONFIGURE: &str = r#"#!/bin/sh
echo run >> 'COUNTER'
env > configure-env.txt
FAIL
for a in "$@"; do case "$a" in --prefix=*) echo "${a#--prefix=}" > prefix.txt;; esac; done
printf 'all:\n\ttrue\ninstall:\n\tsh ./install.sh "$(DESTDIR)"\n' > Makefile
"#;

    /// The fake's `make install`: a header, the recorded environment and a `.pc` file with a
    /// `Libs.private` line, under `$DESTDIR<prefix>`.
    const INSTALL: &str = r#"p="$1$(cat prefix.txt)"
mkdir -p "$p/include" "$p/lib/pkgconfig"
echo zlib > "$p/include/zlib.h"
cp configure-env.txt "$p/lib/env.txt"
printf 'Libs: -lz\nLibs.private: -lm\n' > "$p/lib/pkgconfig/zlib.pc"
"#;

    /// A `zlib`-named fake source tarball. `fail`: its `configure` exits 1.
    fn fake_tarball(dir: &Path, counter: &Path, fail: bool) -> (PathBuf, String) {
        let configure = CONFIGURE
            .replace("COUNTER", &counter.display().to_string())
            .replace("FAIL", if fail { "exit 1" } else { "" });
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (name, body, mode) in [("configure", configure.as_str(), 0o755), ("install.sh", INSTALL, 0o644)] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(mode);
            b.append_data(&mut h, format!("zlib-0.1/{name}"), body.as_bytes()).unwrap();
        }
        let mut gz = b.into_inner().unwrap().finish().unwrap();
        gz.flush().unwrap();
        let path = dir.join("zlib-0.1.tar.gz");
        std::fs::write(&path, &gz).unwrap();
        let sha = crate::install::checksum::sha256_file(&path).unwrap();
        (path, sha)
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        counter: PathBuf,
        rows: Vec<Row>,
        env: Vec<(OsString, OsString)>,
    }

    fn fixture(fail: bool) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let counter = tmp.path().join("configure-runs");
        let (tarball, sha) = fake_tarball(tmp.path(), &counter, fail);
        let rows = vec![Row {
            key: "zlib".into(),
            version: "0.1".into(),
            url: format!("file://{}", tarball.display()),
            sha256: sha,
        }];
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        env.push(("TMPDIR".into(), tmp.path().join("tmp").into()));
        env.push(("CPPFLAGS".into(), "-I/user/include".into()));
        std::fs::create_dir_all(tmp.path().join("tmp")).unwrap();
        Fixture { _tmp: tmp, root, counter, rows, env }
    }

    fn run(f: &Fixture, said: &mut Vec<String>) -> Result<Vec<Entry>, InstallError> {
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &f.rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: PathBuf::from("/etc/ssl"),
            verbose: false,
        };
        ensure(&b, &["zlib"], &mut |s: &str| said.push(s.to_string()))
    }

    fn runs(f: &Fixture) -> usize {
        std::fs::read_to_string(&f.counter).map(|s| s.lines().count()).unwrap_or(0)
    }

    #[test]
    fn a_library_is_built_once_and_reused() {
        let f = fixture(false);
        let mut said = Vec::new();
        let e = run(&f, &mut said).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].key, e[0].version.as_str(), e[0].origin), ("zlib", "0.1", Origin::Catalog));
        assert!(e[0].dir.starts_with(deps_dir(&f.root)), "{}", e[0].dir.display());
        assert!(e[0].dir.join("include/zlib.h").is_file());
        assert!(said.iter().any(|l| l == "Building zlib 0.1..."), "{said:?}");
        // Nothing staged is left beside the entry.
        let left: Vec<String> = std::fs::read_dir(deps_dir(&f.root)).unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".tmp-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let mut again = Vec::new();
        let e2 = run(&f, &mut again).unwrap();
        assert_eq!(e2, e);
        assert_eq!(runs(&f), 1, "configure ran again");
        assert!(again.iter().any(|l| l.starts_with("Using zlib 0.1 from ")), "{again:?}");
    }

    /// Ruling P7: the library build sees `CFLAGS=-O2 -fPIC`, not the user's `CPPFLAGS`.
    #[test]
    fn the_library_build_gets_pic_flags_and_not_the_users_cppflags() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        let env = std::fs::read_to_string(e[0].dir.join("lib/env.txt")).unwrap();
        assert!(env.lines().any(|l| l == "CFLAGS=-O2 -fPIC"), "{env}");
        assert!(!env.contains("/user/include"), "{env}");
    }

    #[test]
    fn libs_private_is_folded_into_libs_in_the_entrys_pc_files() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        let pc = std::fs::read_to_string(e[0].dir.join("lib/pkgconfig/zlib.pc")).unwrap();
        assert_eq!(pc, "Libs: -lz -lm\n");
    }

    #[test]
    fn a_failed_build_leaves_no_entry_and_names_its_log() {
        let f = fixture(true);
        let mut said = Vec::new();
        assert_eq!(run(&f, &mut said), Err(InstallError::Failed));
        let names: Vec<String> = std::fs::read_dir(deps_dir(&f.root)).unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != ".locks")
            .collect();
        assert!(names.is_empty(), "{names:?}");
        let line = said.iter().find(|l| l.starts_with("pyenv: building zlib 0.1 failed; the log is ")).expect("no failure line");
        let log = line.rsplit(' ').next().unwrap();
        assert!(Path::new(log).is_file(), "{log}");
    }

    // Review Focus 3
    #[test]
    fn a_missing_entry_is_built_again() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        std::fs::remove_dir_all(deps_dir(&f.root)).unwrap();
        let e2 = run(&f, &mut Vec::new()).unwrap();
        assert_eq!(e2, e);
        assert_eq!(runs(&f), 2);
    }

    // Review Focus 1
    #[test]
    fn two_builders_of_one_entry_build_it_once() {
        let f = std::sync::Arc::new(fixture(false));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let f = f.clone();
                std::thread::spawn(move || run(&f, &mut Vec::new()).unwrap())
            })
            .collect();
        let got: Vec<Vec<Entry>> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(got[0], got[1]);
        assert_eq!(runs(&f), 1, "both threads built it");
    }

    #[test]
    fn the_entry_name_changes_with_every_input_of_the_fingerprint() {
        let r = catalog::recipe("openssl").unwrap();
        let row = Row { key: "openssl".into(), version: "3.6.5".into(), url: "u".into(), sha256: "a".repeat(64) };
        let base = fingerprint(r, &row, "cc", "x86_64-linux-gnu", "/etc/ssl", &[]);
        let name = entry_name("openssl", "3.6.5", &base);
        assert!(name.starts_with("openssl-3.6.5-") && name.len() == "openssl-3.6.5-".len() + 12, "{name}");
        assert_eq!(name, entry_name("openssl", "3.6.5", &base), "stable");
        for other in [
            fingerprint(r, &row, "gcc-14", "x86_64-linux-gnu", "/etc/ssl", &[]),
            fingerprint(r, &row, "cc", "aarch64-linux-gnu", "/etc/ssl", &[]),
            fingerprint(r, &row, "cc", "x86_64-linux-gnu", "/etc/pki/tls", &[]),
            fingerprint(r, &row, "cc", "x86_64-linux-gnu", "/etc/ssl", &["dep-1".into()]),
            fingerprint(r, &Row { sha256: "b".repeat(64), ..row.clone() }, "cc", "x86_64-linux-gnu", "/etc/ssl", &[]),
        ] {
            assert_ne!(entry_name("openssl", "3.6.5", &other), name);
        }
        // OPENSSLDIR only matters for OpenSSL.
        let z = catalog::recipe("zlib").unwrap();
        let zrow = Row { key: "zlib".into(), ..row.clone() };
        assert_eq!(
            fingerprint(z, &zrow, "cc", "t", "/etc/ssl", &[]),
            fingerprint(z, &zrow, "cc", "t", "/etc/pki/tls", &[])
        );
    }

    #[test]
    fn expand_fills_every_placeholder() {
        let vars = vec![("prefix".to_string(), "/r/e".to_string()), ("dep:tcl".to_string(), "/r/t".to_string())];
        assert_eq!(expand("--prefix={prefix} --with-tcl={dep:tcl}/lib", &vars), "--prefix=/r/e --with-tcl=/r/t/lib");
    }

    #[test]
    fn fold_private_moves_libs_private_into_libs() {
        assert_eq!(
            fold_private("prefix=/x\nLibs: -L${libdir} -lsqlite3\nLibs.private: -lm -lz\nCflags: -I/x\n"),
            "prefix=/x\nLibs: -L${libdir} -lsqlite3 -lm -lz\nCflags: -I/x\n"
        );
        assert_eq!(fold_private("Libs: -lz\n"), "Libs: -lz\n");
    }

    #[test]
    fn plain_paths_have_no_spaces_or_shell_characters() {
        assert!(plain_path(Path::new("/home/u/.pyenv")));
        assert!(plain_path(Path::new("/opt/py-env_2.0/root")));
        for bad in ["/home/u/my pyenv", "/a\tb", "/a'b", "/a\"b", "/a$b", "/a;b", "/a`b", "/a\\b", "/a*b", "/a(b", ""] {
            assert!(!plain_path(Path::new(bad)), "{bad:?}");
        }
    }

    #[test]
    fn openssldir_prefers_pki_then_ssl_then_the_bundle() {
        let fedora = |p: &str| p == "/etc/pki/tls/cert.pem";
        assert_eq!(system_openssldir(&fedora, &|_| false), Some("/etc/pki/tls"));
        let debian = |d: &str| d == "/etc/ssl/certs";
        assert_eq!(system_openssldir(&|_| false, &debian), Some("/etc/ssl"));
        assert_eq!(system_openssldir(&|_| false, &|_| false), None);
    }

    /// Ruling P9: deleting the cache must not break HTTPS for versions that read the bundle.
    #[test]
    fn the_ca_bundle_lives_outside_the_cache() {
        let root = Path::new("/r");
        assert_eq!(ca_dir(root), Path::new("/r/.rpyenv/ca"));
        assert!(!ca_dir(root).starts_with(deps_dir(root)));
    }

    fn entry(key: &'static str, dir: &str) -> Entry {
        Entry { key, version: "1".into(), origin: Origin::Catalog, dir: PathBuf::from(dir) }
    }

    #[test]
    fn link_composes_cpythons_flags_from_the_entries() {
        let entries = vec![
            entry("pkgconf", "/s/pkgconf-1-a"),
            entry("ncurses", "/s/ncurses-1-b"),
            entry("tcl", "/s/tcl-1-c"),
            entry("tk", "/s/tk-1-d"),
            entry("openssl", "/s/openssl-1-e"),
        ];
        let read = |p: &Path| (p == Path::new("/s/tk-1-d/lib/tkConfig.sh")).then(|| "TK_VERSION='8.6'\nTK_LIBS='-lX11   -lpthread -lm'\n".to_string());
        let l = link(entries.clone(), 12, &read);
        assert_eq!(l.path, Some(PathBuf::from("/s/pkgconf-1-a/bin")));
        assert!(l.cppflags.contains(&"-I/s/ncurses-1-b/include".to_string()));
        assert!(l.cppflags.contains(&"-I/s/ncurses-1-b/include/ncursesw".to_string()));
        assert!(l.ldflags.contains(&"-L/s/ncurses-1-b/lib".to_string()));
        assert!(!l.cppflags.iter().any(|f| f.contains("openssl") || f.contains("pkgconf")), "{:?}", l.cppflags);
        assert!(l.configure.contains(&"--with-openssl=/s/openssl-1-e".to_string()));
        assert!(l.pkg_config_path.contains(&PathBuf::from("/s/ncurses-1-b/lib/pkgconfig")));
        assert_eq!(
            l.configure_env,
            [
                ("TCLTK_CFLAGS".to_string(), "-I/s/tcl-1-c/include -I/s/tk-1-d/include".to_string()),
                ("TCLTK_LIBS".to_string(), "-L/s/tk-1-d/lib -ltk8.6 -L/s/tcl-1-c/lib -ltcl8.6 -lX11 -lpthread -lm".to_string()),
            ]
        );
        assert_eq!(l.scripts, [PathBuf::from("/s/tcl-1-c/lib/tcl8.6"), PathBuf::from("/s/tk-1-d/lib/tk8.6")]);
        assert_eq!(l.entries, entries);
        // 3.10 and older take Tcl/Tk through configure options instead.
        let old = link(entries, 10, &read);
        assert!(old.configure_env.is_empty());
        assert!(old.configure.contains(&"--with-tcltk-includes=-I/s/tcl-1-c/include -I/s/tk-1-d/include".to_string()));
        assert!(old.configure.contains(&"--with-tcltk-libs=-L/s/tk-1-d/lib -ltk8.6 -L/s/tcl-1-c/lib -ltcl8.6 -lX11 -lpthread -lm".to_string()));
    }
}
```

And at the bottom of the new `record.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_record_round_trips_and_leaves_out_pkgconf() {
        let e = |key: &'static str, v: &str, origin| Entry { key, version: v.into(), origin, dir: PathBuf::from("/s") };
        let t = text("3.12.15", &[
            e("pkgconf", "3.0.9", Origin::Catalog),
            e("openssl", "3.6.5", Origin::Definition),
            e("sqlite", "3.54.1", Origin::Catalog),
        ]);
        assert!(t.starts_with('#'), "{t}");
        assert!(t.contains("\ndefinition 3.12.15\nopenssl 3.6.5 definition\nsqlite 3.54.1 catalog\n"), "{t}");
        assert!(!t.contains("pkgconf"), "{t}");
        let r = parse(&t).unwrap();
        assert_eq!(r.definition, "3.12.15");
        assert_eq!(r.libs, [
            ("openssl".to_string(), "3.6.5".to_string(), Origin::Definition),
            ("sqlite".to_string(), "3.54.1".to_string(), Origin::Catalog),
        ]);
        assert!(parse("openssl 3.6.5 catalog\n").is_none(), "no definition line");
    }
}
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `cargo test -q -p pyenv --lib -- store:: record::`
Expected: compile errors (`ensure`, `Build`, `text`, … not found).

- [ ] **Step 4: Write `store.rs`** above its tests:

```rust
//! The build store (M8 design §6.2): `<root>/cache/deps/<lib>-<version>-<hash>/`, static
//! libraries compiled from the catalog without root and reused by every CPython version that
//! needs the same library, version and options (D6, D7). It is a cache: CPython links these
//! libraries statically, so deleting `cache/deps` breaks no installed version.

use super::archive::{extract, Kind};
use super::builder::{get, logged};
use super::catalog::{self, Origin, Recipe, Row};
use super::checksum::sha256_file;
use super::defs::Definition;
use super::fetch::{Check, FetchRequest, Fetcher};
use super::log::BuildLog;
use super::{interrupted, InstallError};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

type R<T> = Result<T, InstallError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: &'static str,
    pub version: String,
    pub origin: Origin,
    pub dir: PathBuf,
}

pub fn deps_dir(root: &Path) -> PathBuf {
    root.join("cache").join("deps")
}

/// Where the pinned CA bundle lives when the system has no certificates (M8 design §6.5).
/// Outside `cache/deps` (ruling P9): a self-built OpenSSL has this path compiled in, so the
/// file must survive the cache being deleted.
pub fn ca_dir(root: &Path) -> PathBuf {
    root.join(".rpyenv").join("ca")
}

/// A path the recipes and CPython's flags can carry unquoted (plan ruling P5): not empty, no
/// whitespace or control character, and none of the characters a shell gives a meaning.
pub fn plain_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    !s.is_empty()
        && !s.chars().any(|c| {
            c.is_whitespace() || c.is_control() || "\"'`$\\;&|<>*?()[]{}!#~".contains(c)
        })
}

/// Where the system keeps CA certificates, for a self-built OpenSSL's `OPENSSLDIR` (§6.5):
/// `/etc/pki/tls` (Fedora, RHEL) or `/etc/ssl` (Debian, Arch, SUSE), whichever has a
/// `cert.pem` or a non-empty `certs/`. `None`: rpyenv's pinned bundle instead.
pub fn system_openssldir(
    exists: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> Option<&'static str> {
    ["/etc/pki/tls", "/etc/ssl"]
        .into_iter()
        .find(|d| exists(&format!("{d}/cert.pem")) || has_files(&format!("{d}/certs")))
}

/// `<key>-<version>-<the first 12 hex digits of the fingerprint's SHA-256>`.
pub fn entry_name(key: &str, version: &str, fingerprint: &str) -> String {
    let h = Sha256::digest(fingerprint.as_bytes());
    let hex: String = h.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{key}-{version}-{hex}")
}

/// Everything that makes two builds of a library differ (spec D6): its source, its recipe,
/// the compiler and target, `OPENSSLDIR` for OpenSSL, and the entries it was built against.
pub fn fingerprint(
    r: &Recipe,
    row: &Row,
    cc: &str,
    triple: &str,
    openssldir: &str,
    deps: &[String],
) -> String {
    let mut s = format!(
        "{}\n{}\n{}\n{}\n{cc}\n{triple}\n",
        r.key,
        row.version,
        row.sha256,
        r.steps.join("\n")
    );
    if r.key == "openssl" {
        s.push_str(openssldir);
        s.push('\n');
    }
    for d in deps {
        s.push_str(d);
        s.push('\n');
    }
    s
}

/// `step` with each `{name}` of `vars` replaced by its value.
pub fn expand(step: &str, vars: &[(String, String)]) -> String {
    let mut s = step.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// A store `.pc` file with `Libs.private` joined to `Libs`: the store holds static libraries
/// only, so a consumer always needs them (measured: CPython 3.12's `_sqlite3` is not found
/// without sqlite3.pc's `-lm`).
pub fn fold_private(pc: &str) -> String {
    let mut private = String::new();
    let mut kept: Vec<&str> = Vec::new();
    for l in pc.lines() {
        match l.strip_prefix("Libs.private:") {
            Some(rest) if !rest.trim().is_empty() => {
                private.push(' ');
                private.push_str(rest.trim());
            }
            Some(_) => {}
            None => kept.push(l),
        }
    }
    let mut out = String::new();
    for l in kept {
        out.push_str(l.trim_end());
        if l.starts_with("Libs:") {
            out.push_str(&private);
        }
        out.push('\n');
    }
    out
}

pub struct Build<'a> {
    /// `PYENV_ROOT`, absolute.
    pub root: &'a Path,
    pub rows: &'a [Row],
    pub definition: &'a Definition,
    /// The job's environment: `PATH`, `CC`, `TMPDIR`, `HOME` and the rest.
    pub env: &'a [(OsString, OsString)],
    pub fetcher: &'a Fetcher,
    /// A system folder, or `ca_dir(root)` for the pinned bundle (§6.5).
    pub openssldir: PathBuf,
    pub verbose: bool,
}

/// The variables a library build must not inherit (plan ruling P7).
const DROPPED: [&str; 7] = [
    "CPPFLAGS",
    "LDFLAGS",
    "LIBS",
    "CFLAGS",
    "CXXFLAGS",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
];

/// The environment of a library build: the job's, without `DROPPED`, with `CFLAGS=-O2 -fPIC`,
/// and with the store's pkgconf first on `PATH` once it has been built (ruling P10).
fn step_env(env: &[(OsString, OsString)], pkgconf: Option<&Path>) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = env
        .iter()
        .filter(|(k, _)| !DROPPED.iter().any(|d| k == d))
        .cloned()
        .collect();
    out.push(("CFLAGS".into(), "-O2 -fPIC".into()));
    if let Some(bin) = pkgconf {
        let path = get(env, "PATH").unwrap_or_default();
        out.push(("PATH".into(), format!("{}:{path}", bin.display()).into()));
    }
    out
}

fn message(m: String) -> InstallError {
    InstallError::Message(m)
}

/// `cc -dumpmachine`, or `unknown`.
fn triple_of(cc: &str, env: &[(OsString, OsString)]) -> String {
    let mut words = cc.split_whitespace();
    let Some(program) = words.next() else {
        return "unknown".into();
    };
    Command::new(program)
        .args(words)
        .arg("-dumpmachine")
        .env("PATH", get(env, "PATH").unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// Builds every library of `keys` (catalog keys, in `catalog::ORDER`) that the store doesn't
/// have yet, and returns the entries of all of them. One lock per entry: a second `pyenv`
/// building the same entry waits, then reuses it (Review Focus 1).
pub fn ensure(b: &Build, keys: &[&'static str], say: &mut dyn FnMut(&str)) -> R<Vec<Entry>> {
    let dir = deps_dir(b.root);
    let locks = dir.join(".locks");
    std::fs::create_dir_all(&locks)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", locks.display())))?;
    let cc = get(b.env, "CC")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "cc".into());
    let triple = triple_of(&cc, b.env);
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .to_string();
    let openssldir = b.openssldir.display().to_string();
    let mut done: Vec<Entry> = Vec::new();
    for &key in keys {
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        let recipe = catalog::recipe(key)
            .ok_or_else(|| message(format!("pyenv: the build catalog has no recipe for {key}")))?;
        let (row, origin) = catalog::resolve(b.rows, key, b.definition)
            .ok_or_else(|| message(format!("pyenv: the build catalog has no {key}")))?;
        let mut vars: Vec<(String, String)> = Vec::new();
        let mut deps: Vec<String> = Vec::new();
        for n in recipe.needs {
            let e = done
                .iter()
                .find(|e| e.key == *n)
                .ok_or_else(|| message(format!("pyenv: {key} needs {n}, built first")))?;
            vars.push((format!("dep:{n}"), e.dir.display().to_string()));
            deps.push(e.dir.file_name().unwrap_or_default().to_string_lossy().into_owned());
        }
        let name = entry_name(
            key,
            &row.version,
            &fingerprint(recipe, &row, &cc, &triple, &openssldir, &deps),
        );
        let prefix = dir.join(&name);
        let lock_path = locks.join(format!("{name}.lock"));
        let lock = std::fs::File::create(&lock_path)
            .map_err(|e| message(format!("pyenv: cannot create {}: {e}", lock_path.display())))?;
        lock.lock()
            .map_err(|e| message(format!("pyenv: cannot lock {}: {e}", lock_path.display())))?;
        if prefix.is_dir() {
            say(&format!("Using {key} {} from {}", row.version, prefix.display()));
        } else {
            vars.extend([
                ("prefix".to_string(), prefix.display().to_string()),
                ("jobs".to_string(), jobs.clone()),
                ("triple".to_string(), triple.clone()),
                ("openssldir".to_string(), openssldir.clone()),
            ]);
            let pkgconf = done
                .iter()
                .find(|e| e.key == "pkgconf")
                .map(|e| e.dir.join("bin"));
            build_one(b, recipe, &row, &name, &prefix, vars, pkgconf.as_deref(), say)?;
        }
        drop(lock);
        done.push(Entry {
            key,
            version: row.version.clone(),
            origin,
            dir: prefix,
        });
    }
    if b.openssldir == ca_dir(b.root) && done.iter().any(|e| e.key == "openssl") {
        place_ca(b.root, b.rows, b.fetcher, say)?;
    }
    Ok(done)
}

/// Downloads, checks, extracts and builds one library, staged in `.tmp-<name>-<pid>` and
/// renamed to `prefix` only when every step succeeded. A failure or Ctrl+C leaves no entry
/// (Review Focus 2); a failure keeps the log and names it.
#[allow(clippy::too_many_arguments)]
fn build_one(
    b: &Build,
    recipe: &Recipe,
    row: &Row,
    name: &str,
    prefix: &Path,
    mut vars: Vec<(String, String)>,
    pkgconf: Option<&Path>,
    say: &mut dyn FnMut(&str),
) -> R<()> {
    say(&format!("Building {} {}...", recipe.key, row.version));
    let tmp = get(b.env, "TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let pid = std::process::id();
    let work = tmp.join(format!("rpyenv-deps.{name}.{pid}"));
    let log_path = tmp.join(format!("rpyenv-deps.{name}.{pid}.log"));
    let destdir = prefix.with_file_name(format!(".tmp-{name}-{pid}"));
    let stage = destdir.join(prefix.strip_prefix("/").unwrap_or(prefix));
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&destdir);
        let _ = std::fs::remove_dir_all(&work);
    };
    cleanup();
    std::fs::create_dir_all(&work)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", work.display())))?;
    let log = BuildLog::open(&log_path, b.verbose)
        .map_err(|e| message(format!("pyenv: cannot write {}: {e}", log_path.display())))?;
    let result = (|| -> R<()> {
        let kind = Kind::of_url(&row.url);
        let req = FetchRequest {
            file_name: format!("{}-{}{}", recipe.key, row.version, kind.ext()),
            url: row.url.clone(),
            check: Check::Sha256(row.sha256.clone()),
            dest_dir: work.clone(),
        };
        let mut w: &BuildLog = &log;
        let archive = b.fetcher.fetch(&req, &mut w, say)?;
        let src = extract(&archive, kind, &work, recipe.key).map_err(|e| {
            log.line(&e);
            InstallError::Failed
        })?;
        let cwd = if recipe.dir.is_empty() { src } else { src.join(recipe.dir) };
        vars.push(("destdir".to_string(), destdir.display().to_string()));
        vars.push(("stage".to_string(), stage.display().to_string()));
        let env = step_env(b.env, pkgconf);
        for step in recipe.steps {
            let line = expand(step, &vars);
            log.line(&format!("+ {line}"));
            let mut c = Command::new("sh");
            c.arg("-c").arg(&line).current_dir(&cwd).env_clear().envs(env.iter().map(|(k, v)| (k, v)));
            logged(&log, &mut c)?;
        }
        let pc = stage.join("lib").join("pkgconfig");
        if let Ok(rd) = std::fs::read_dir(&pc) {
            for e in rd.filter_map(Result::ok) {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "pc") {
                    if let Ok(t) = std::fs::read_to_string(&p) {
                        let _ = std::fs::write(&p, fold_private(&t));
                    }
                }
            }
        }
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        std::fs::rename(&stage, prefix).map_err(|e| {
            message(format!("pyenv: cannot move {} into {}: {e}", stage.display(), prefix.display()))
        })
    })();
    cleanup();
    match result {
        Ok(()) => {
            drop(log);
            let _ = std::fs::remove_file(&log_path);
            Ok(())
        }
        Err(InstallError::Interrupted) => Err(InstallError::Interrupted),
        Err(e) => {
            if let InstallError::Message(m) = &e {
                say(m);
            }
            say(&format!(
                "pyenv: building {} {} failed; the log is {}",
                recipe.key,
                row.version,
                log_path.display()
            ));
            Err(InstallError::Failed)
        }
    }
}

/// Puts the catalog's CA bundle at `<root>/.rpyenv/ca/cert.pem` (M8 design §6.5), unless
/// the file there already has the catalog's SHA-256. `Ok(true)` when it wrote the file.
pub fn place_ca(root: &Path, rows: &[Row], fetcher: &Fetcher, say: &mut dyn FnMut(&str)) -> R<bool> {
    let row = rows
        .iter()
        .find(|r| r.key == "cacert")
        .ok_or_else(|| message("pyenv: the build catalog has no cacert row".into()))?;
    let dir = ca_dir(root);
    let target = dir.join("cert.pem");
    if sha256_file(&target).is_ok_and(|h| h == row.sha256) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", dir.display())))?;
    let req = FetchRequest {
        file_name: format!("cacert-{}.pem", row.version),
        url: row.url.clone(),
        check: Check::Sha256(row.sha256.clone()),
        dest_dir: dir.clone(),
    };
    let got = fetcher.fetch(&req, &mut std::io::sink(), say)?;
    std::fs::rename(&got, &target)
        .map_err(|e| message(format!("pyenv: cannot write {}: {e}", target.display())))?;
    Ok(true)
}

/// What CPython's build needs from the store (M8 design §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub entries: Vec<Entry>,
    pub cppflags: Vec<String>,
    pub ldflags: Vec<String>,
    pub pkg_config_path: Vec<PathBuf>,
    /// pkgconf's `bin/`, first on `PATH` (ruling P10).
    pub path: Option<PathBuf>,
    pub configure: Vec<String>,
    pub configure_env: Vec<(String, String)>,
    /// Tcl's and Tk's script folders, copied into `<prefix>/lib/` (§6.3).
    pub scripts: Vec<PathBuf>,
}

/// `name='value'` from a `tclConfig.sh`/`tkConfig.sh`, with its runs of spaces collapsed.
fn config_value(text: &str, name: &str) -> Option<String> {
    let v = text.lines().find_map(|l| l.strip_prefix(&format!("{name}=")))?;
    let v = v.trim().trim_matches('\'');
    Some(v.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// CPython's flags for `entries` and CPython 3.`minor` (measured, plan M8b): `-I`/`-L` and
/// `pkg-config` folders for every library but OpenSSL (`--with-openssl`) and pkgconf (`PATH`);
/// Tcl/Tk through `TCLTK_CFLAGS`/`TCLTK_LIBS` from 3.11 on and through
/// `--with-tcltk-includes`/`--with-tcltk-libs` before. `read` reads Tk's `tkConfig.sh`.
pub fn link(entries: Vec<Entry>, minor: u32, read: &dyn Fn(&Path) -> Option<String>) -> Linked {
    let mut l = Linked {
        entries: Vec::new(),
        cppflags: Vec::new(),
        ldflags: Vec::new(),
        pkg_config_path: Vec::new(),
        path: None,
        configure: Vec::new(),
        configure_env: Vec::new(),
        scripts: Vec::new(),
    };
    let dir_of = |k: &str| entries.iter().find(|e| e.key == k).map(|e| e.dir.clone());
    for e in &entries {
        match e.key {
            "pkgconf" => l.path = Some(e.dir.join("bin")),
            "openssl" => l.configure.push(format!("--with-openssl={}", e.dir.display())),
            k => {
                l.cppflags.push(format!("-I{}/include", e.dir.display()));
                for inc in catalog::recipe(k).map(|r| r.includes).unwrap_or(&[]) {
                    l.cppflags.push(format!("-I{}/{inc}", e.dir.display()));
                }
                l.ldflags.push(format!("-L{}/lib", e.dir.display()));
                l.pkg_config_path.push(e.dir.join("lib").join("pkgconfig"));
            }
        }
    }
    if let (Some(tcl), Some(tk)) = (dir_of("tcl"), dir_of("tk")) {
        let tk_libs = read(&tk.join("lib").join("tkConfig.sh"))
            .and_then(|t| config_value(&t, "TK_LIBS"))
            .unwrap_or_default();
        let cflags = format!("-I{}/include -I{}/include", tcl.display(), tk.display());
        let libs = format!(
            "-L{}/lib -ltk8.6 -L{}/lib -ltcl8.6 {tk_libs}",
            tk.display(),
            tcl.display()
        )
        .trim_end()
        .to_string();
        if minor <= 10 {
            l.configure.push(format!("--with-tcltk-includes={cflags}"));
            l.configure.push(format!("--with-tcltk-libs={libs}"));
        } else {
            l.configure_env.push(("TCLTK_CFLAGS".into(), cflags));
            l.configure_env.push(("TCLTK_LIBS".into(), libs));
        }
        l.scripts = vec![tcl.join("lib").join("tcl8.6"), tk.join("lib").join("tk8.6")];
    }
    l.entries = entries;
    l
}
```

- [ ] **Step 5: Write `record.rs`** above its tests:

```rust
//! `versions/<v>/.rpyenv-deps` (M8 design §6.3): the libraries the build route compiled into a
//! version, and where each one's version came from, so `pyenv install` can warn when rpyenv's
//! catalog moves past one of them.

use super::catalog::Origin;
use super::store::Entry;

pub const FILE: &str = ".rpyenv-deps";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The definition the version was built from (its pins are compared, ruling P12).
    pub definition: String,
    /// (library, version, origin)
    pub libs: Vec<(String, String, Origin)>,
}

/// The file's text: a comment, `definition <name>`, then `<library> <version> <origin>` per
/// library linked in (pkgconf is a build tool, not a library, and is left out).
pub fn text(definition: &str, entries: &[Entry]) -> String {
    let mut s = String::from(
        "# Libraries rpyenv's build route compiled into this version: name, version, origin.\n",
    );
    s.push_str(&format!("definition {definition}\n"));
    for e in entries.iter().filter(|e| e.key != "pkgconf") {
        s.push_str(&format!("{} {} {}\n", e.key, e.version, e.origin.word()));
    }
    s
}

/// The record in `text`, or `None` when it has no `definition` line.
pub fn parse(text: &str) -> Option<Record> {
    let mut definition = None;
    let mut libs = Vec::new();
    for l in text.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        match w.as_slice() {
            ["definition", d] => definition = Some(d.to_string()),
            [k, v, o] if !k.starts_with('#') => {
                if let Some(o) = Origin::from_word(o) {
                    libs.push((k.to_string(), v.to_string(), o));
                }
            }
            _ => {}
        }
    }
    Some(Record {
        definition: definition?,
        libs,
    })
}
```

- [ ] **Step 6: Register the modules** in `install/mod.rs`, after `pub mod preflight;`'s
  `#[cfg(unix)]` pair, keeping the file's alphabetical order:

```rust
#[cfg(unix)]
pub mod record;
```

and after `pub mod stdlib;`:

```rust
#[cfg(unix)]
pub mod store;
```

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo test -q -p pyenv --lib -- store:: record:: catalog::`
Expected: all pass (13 store, 1 record, 8 catalog).

- [ ] **Step 8: Commit**

```bash
git add crates/pyenv/src/install/store.rs crates/pyenv/src/install/record.rs crates/pyenv/src/install/mod.rs crates/pyenv/src/install/builder.rs
git commit -m "M8b: the build store: static entries built once per library, version and options, then reused"
```

---

### Task 3: What can be built here, and `[b]` in the prompt (`preflight.rs`, `deps.rs`)

**Files:**
- Modify: `crates/pyenv/src/install/preflight.rs` (add `Tools`, `PERL_MODULES`, `build_tools`)
- Modify: `crates/pyenv/src/install/deps.rs` (add `BuildPlan`, `build_plan`, `answer_hint`; extend `prompt_lines`, `choice`)
- Modify: `crates/pyenv/src/commands/install.rs` (only the two call sites that no longer compile: pass `None` and `false` for now; Task 4 wires them)

**Interfaces:**
- Consumes: Task 1's `catalog::{plan, keys_for, recipe}`.
- Produces:
  - `preflight`: `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Tools { pub perl: bool, pub x11: bool }`;
    `pub const PERL_MODULES: [&str; 6]`; `pub fn build_tools(env: &dyn Fn(&str) -> Option<String>) -> Tools`
  - `deps`: `#[derive(Debug, Clone, PartialEq, Eq)] pub struct BuildPlan { pub libs: Vec<&'static str>, pub keys: Vec<&'static str>, pub not: Vec<(&'static str, String)>, pub blocked: Option<String> }`;
    `pub fn build_plan(missing: &[Dep], minor: u32, tools: &Tools, root_ok: Result<(), String>, m: Option<Manager>) -> BuildPlan`;
    `pub fn prompt_lines(version, deps, m, install, build: Option<&BuildPlan>, crb, continue_offered) -> Vec<String>` (new 5th parameter);
    `pub fn choice(line: &str, install_offered: bool, build_offered: bool, continue_offered: bool) -> Option<Route>`;
    `pub fn answer_hint(install_offered: bool, build_offered: bool, continue_offered: bool) -> String`

- [ ] **Step 1: Write the failing tests** in `deps.rs`'s `mod tests` (the module already has a
  `dep(name, lib)` helper building a `Dep` with `Missing::Header`):

```rust
    const ALL_TOOLS: Tools = Tools { perl: true, x11: true };

    #[test]
    fn the_build_plan_turns_missing_libraries_into_catalog_keys() {
        let missing = [dep("ssl", "openssl"), dep("tkinter", "tk"), dep("uuid", "libuuid")];
        let p = build_plan(&missing, 12, &ALL_TOOLS, Ok(()), Some(Manager::Apt));
        assert_eq!(p.blocked, None);
        assert!(p.not.is_empty(), "{:?}", p.not);
        assert_eq!(p.keys, ["tcl", "tk", "libuuid", "openssl"]);
        assert_eq!(p.libs, ["openssl", "tk", "libuuid"]);
    }

    /// Plan rulings P3 and P4: Tk without X11 headers, OpenSSL without Perl's modules.
    #[test]
    fn libraries_the_tools_cant_build_are_named_with_their_package() {
        let missing = [dep("ssl", "openssl"), dep("tkinter", "tk"), dep("uuid", "libuuid")];
        let none = Tools { perl: false, x11: false };
        let p = build_plan(&missing, 12, &none, Ok(()), Some(Manager::Dnf));
        assert_eq!(p.keys, ["libuuid"]);
        assert_eq!(
            p.not,
            [
                ("ssl", "needs Perl 5 with its core modules (perl-core)".to_string()),
                ("tkinter", "needs the X11 headers (libX11-devel)".to_string()),
            ]
        );
        let apt = build_plan(&missing, 12, &none, Ok(()), Some(Manager::Apt));
        assert!(apt.not.contains(&("tkinter", "needs the X11 headers (libx11-dev)".to_string())), "{:?}", apt.not);
        assert!(apt.not.contains(&("ssl", "needs Perl 5 with its core modules (perl)".to_string())), "{:?}", apt.not);
        let unknown = build_plan(&missing, 12, &none, Ok(()), None);
        assert!(unknown.not.contains(&("tkinter", "needs the X11 headers".to_string())), "{:?}", unknown.not);
        let only_tk = build_plan(&[dep("tkinter", "tk")], 12, &none, Ok(()), Some(Manager::Apt));
        assert_eq!(only_tk.blocked.as_deref(), Some("nothing missing can be built here"));
    }

    #[test]
    fn the_build_route_is_blocked_without_a_compiler_for_old_pythons_and_odd_roots() {
        let compiler = Dep { name: "C compiler", required: true, missing: Missing::Program("cc".into()), lib: "compiler" };
        let p = build_plan(&[compiler, dep("uuid", "libuuid")], 12, &ALL_TOOLS, Ok(()), None);
        assert_eq!(p.blocked.as_deref(), Some("it needs the C compiler, make and patch from the system first"));
        let old = build_plan(&[dep("uuid", "libuuid")], 6, &ALL_TOOLS, Ok(()), None);
        assert_eq!(old.blocked.as_deref(), Some("the build route needs CPython 3.7 or later"));
        let root = build_plan(&[dep("uuid", "libuuid")], 12, &ALL_TOOLS, Err("PYENV_ROOT has a space or a shell character in its path (/a b)".into()), None);
        assert_eq!(root.blocked.as_deref(), Some("PYENV_ROOT has a space or a shell character in its path (/a b)"));
    }

    #[test]
    fn pkg_config_and_ca_certificates_are_buildable() {
        let pc = Dep { name: "pkg-config", required: true, missing: Missing::Program("pkg-config".into()), lib: "pkg-config" };
        let ca = Dep { name: "ssl certificates", required: true, missing: Missing::Certificates, lib: stdlib::CA_KEY };
        let p = build_plan(&[pc, ca], 13, &ALL_TOOLS, Ok(()), None);
        assert_eq!(p.keys, ["pkgconf", "openssl"]);
        assert_eq!(p.libs, ["pkg-config", stdlib::CA_KEY]);
    }

    #[test]
    fn the_prompt_offers_b_and_says_what_it_leaves_out() {
        let missing = [dep("ssl", "openssl"), dep("tkinter", "tk")];
        let partial = build_plan(&missing, 12, &Tools { perl: true, x11: false }, Ok(()), Some(Manager::Apt));
        let lines = prompt_lines("3.12.15", &missing, Some(Manager::Apt), Err("sudo was not found"), Some(&partial), None, false);
        assert!(lines.contains(&"  [b] build them from source for rpyenv, no root (not tkinter: needs the X11 headers, libx11-dev)".to_string()), "{lines:#?}");
        let full = build_plan(&missing, 12, &ALL_TOOLS, Ok(()), Some(Manager::Apt));
        let lines = prompt_lines("3.12.15", &missing, Some(Manager::Apt), Err("sudo was not found"), Some(&full), None, false);
        let at = lines.iter().position(|l| l == "  [b] build them from source for rpyenv, no root").expect("no [b]");
        let i_at = lines.iter().position(|l| l.starts_with("  [i]")).unwrap();
        assert!(i_at < at, "[b] comes after [i]");
        let blocked = build_plan(&[dep("tkinter", "tk")], 12, &Tools { perl: true, x11: false }, Ok(()), Some(Manager::Apt));
        let lines = prompt_lines("3.12.15", &missing, Some(Manager::Apt), Err("x"), Some(&blocked), None, false);
        assert!(lines.contains(&"  [b] not offered: nothing missing can be built here".to_string()), "{lines:#?}");
        let none = prompt_lines("3.12.15", &missing, Some(Manager::Apt), Err("x"), None, None, false);
        assert!(!none.iter().any(|l| l.starts_with("  [b]")), "{none:#?}");
    }

    #[test]
    fn b_is_a_choice_only_when_offered_and_the_hint_lists_the_letters() {
        assert_eq!(choice("b", false, true, false), Some(Route::Build));
        assert_eq!(choice("B", true, true, true), Some(Route::Build));
        assert_eq!(choice("b", true, false, true), None);
        assert_eq!(answer_hint(true, true, true), "pyenv: answer p, i, b or c");
        assert_eq!(answer_hint(false, true, false), "pyenv: answer p or b");
        assert_eq!(answer_hint(true, false, true), "pyenv: answer p, i or c");
        assert_eq!(answer_hint(false, false, false), "pyenv: answer p");
    }
```

  Add `use crate::install::preflight::Tools;` to the test module's `use` lines. Update the
  existing calls in `deps.rs`'s tests: every `prompt_lines(a, b, c, d, e, f)` becomes
  `prompt_lines(a, b, c, d, None, e, f)`, and every `choice(l, i, c)` becomes
  `choice(l, i, false, c)`.

  In `preflight.rs`'s `mod tests`:

```rust
    #[test]
    fn the_build_tools_hook_names_the_tools_present() {
        let env = |k: &str| (k == "RPYENV_TEST_BUILD_TOOLS").then(|| "x11".to_string());
        assert_eq!(build_tools(&env), Tools { perl: false, x11: true });
        let none = |k: &str| (k == "RPYENV_TEST_BUILD_TOOLS").then(String::new);
        assert_eq!(build_tools(&none), Tools { perl: false, x11: false });
        assert!(PERL_MODULES.contains(&"IPC::Cmd") && PERL_MODULES.contains(&"FindBin"));
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -q -p pyenv --lib -- deps:: preflight::`
Expected: compile errors (`build_plan`, `Tools`, `answer_hint` not found; arity of `prompt_lines` and `choice`).

- [ ] **Step 3: Add the tool probes** to `preflight.rs` (after `ca_present`):

```rust
/// What the build route needs besides a compiler and `make` (plan rulings P3, P4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tools {
    /// Perl with the core modules OpenSSL's `Configure` and libxcrypt's scripts load.
    pub perl: bool,
    /// The X11 headers and libX11, which Tk needs until M8c.
    pub x11: bool,
}

/// The Perl modules OpenSSL's and libxcrypt's build scripts load that Fedora 44's
/// `perl-interpreter` alone lacks (measured 2026-10-17; `perl-core` has them).
pub const PERL_MODULES: [&str; 6] = [
    "FindBin",
    "lib",
    "IPC::Cmd",
    "File::Compare",
    "File::Copy",
    "Digest::SHA",
];

/// Probes for `Tools`. Debug builds: `RPYENV_TEST_BUILD_TOOLS` (a comma list of `perl`, `x11`)
/// names the tools present instead.
pub fn build_tools(env: &dyn Fn(&str) -> Option<String>) -> Tools {
    if cfg!(debug_assertions) {
        if let Some(list) = env("RPYENV_TEST_BUILD_TOOLS") {
            let has = |t: &str| list.split(',').map(str::trim).any(|w| w == t);
            return Tools {
                perl: has("perl"),
                x11: has("x11"),
            };
        }
    }
    let path = env("PATH");
    let run = |program: &str, args: &[&str], input: &str, capture: bool| {
        run_with_input(path.as_deref(), program, args, input, capture)
    };
    let mut perl_args: Vec<String> = PERL_MODULES.iter().map(|m| format!("-M{m}")).collect();
    perl_args.push("-e1".into());
    let perl_args: Vec<&str> = perl_args.iter().map(String::as_str).collect();
    let perl = run("perl", &perl_args, "", false).is_some_and(|(ok, _)| ok);
    let cc = probe_compiler(env, &run);
    let tmp = probe_path();
    let x11 = link_probe(
        env,
        &cc,
        &|p, a| {
            run(p, a, "#include <X11/Xlib.h>\nint main(void) { return XOpenDisplay(0) != 0; }\n", false)
                .is_some_and(|(ok, _)| ok)
        },
        &["-lX11"],
        &tmp.display().to_string(),
    );
    let _ = std::fs::remove_file(&tmp);
    Tools { perl, x11 }
}
```

  (`link_probe`, `probe_compiler`, `probe_path` and `run_with_input` are the existing private
  helpers; `link_probe(env, cc, run: &dyn Fn(&str, &[&str]) -> bool, libs, out)` is at
  `preflight.rs:513`.)

- [ ] **Step 4: Add the plan and the prompt's `[b]`** to `deps.rs`. Add
  `use super::catalog;` and `use super::preflight::Tools;` to the `use` lines, then:

```rust
/// What `[b]` and `--deps=build` would do with `missing` (M8 design §6, plan rulings P2–P5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    /// The `Dep::lib` values the build supplies.
    pub libs: Vec<&'static str>,
    /// The catalog keys to build, in `catalog::ORDER`.
    pub keys: Vec<&'static str>,
    /// What can't be built here: (`Dep::name`, why).
    pub not: Vec<(&'static str, String)>,
    /// Why the route can't run at all, when it can't.
    pub blocked: Option<String>,
}

/// The package that brings what a build needs, per manager (apt, dnf, zypper, pacman, apk).
fn tool_package(m: Option<Manager>, perl: bool) -> Option<&'static str> {
    let col = m?.column();
    Some(if perl {
        ["perl", "perl-core", "perl", "perl", "perl"][col]
    } else {
        ["libx11-dev", "libX11-devel", "libX11-devel", "libx11", "libx11-dev"][col]
    })
}

pub fn build_plan(
    missing: &[Dep],
    minor: u32,
    tools: &Tools,
    root_ok: Result<(), String>,
    m: Option<Manager>,
) -> BuildPlan {
    let blocked = |why: String| BuildPlan {
        libs: Vec::new(),
        keys: Vec::new(),
        not: Vec::new(),
        blocked: Some(why),
    };
    if minor < 7 {
        return blocked("the build route needs CPython 3.7 or later".into());
    }
    if missing
        .iter()
        .any(|d| matches!(d.lib, "compiler" | "make" | "patch"))
    {
        return blocked("it needs the C compiler, make and patch from the system first".into());
    }
    if let Err(why) = root_ok {
        return blocked(why);
    }
    let mut libs: Vec<&'static str> = Vec::new();
    let mut not: Vec<(&'static str, String)> = Vec::new();
    for d in missing {
        let keys = catalog::keys_for(d.lib);
        if keys.is_empty() || libs.contains(&d.lib) || not.iter().any(|(n, _)| *n == d.name) {
            continue;
        }
        let recipes = keys.iter().filter_map(|k| catalog::recipe(k));
        let (needs_perl, needs_x11) = recipes.fold((false, false), |(p, x), r| (p || r.perl, x || r.x11));
        let with_package = |what: &str, perl: bool| match tool_package(m, perl) {
            Some(p) => format!("{what} ({p})"),
            None => what.to_string(),
        };
        if needs_perl && !tools.perl {
            not.push((d.name, with_package("needs Perl 5 with its core modules", true)));
        } else if needs_x11 && !tools.x11 {
            not.push((d.name, with_package("needs the X11 headers", false)));
        } else {
            libs.push(d.lib);
        }
    }
    let keys = catalog::plan(&libs);
    if keys.is_empty() {
        return BuildPlan {
            blocked: Some("nothing missing can be built here".into()),
            libs,
            keys,
            not,
        };
    }
    BuildPlan {
        libs,
        keys,
        not,
        blocked: None,
    }
}

/// The line `prompt` prints after an answer it can't use, listing the letters on offer.
pub fn answer_hint(install_offered: bool, build_offered: bool, continue_offered: bool) -> String {
    let mut letters = vec!["p"];
    for (on, l) in [(install_offered, "i"), (build_offered, "b"), (continue_offered, "c")] {
        if on {
            letters.push(l);
        }
    }
    let shown = match letters.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => "p".to_string(),
    };
    format!("pyenv: answer {shown}")
}
```

  In the tests above the expected strings write the package after a comma inside the prompt
  line (`needs the X11 headers, libx11-dev`) but in parentheses inside `BuildPlan::not`
  (`needs the X11 headers (libx11-dev)`). The prompt line turns `" ("` into `", "` and drops the
  closing `)`, so the parenthesis doesn't nest inside the `[b]` line's own. Implement it in
  `prompt_lines`: change its signature to

```rust
pub fn prompt_lines(
    version: &str,
    deps: &[Dep],
    m: Option<Manager>,
    install: Result<Vec<String>, &'static str>,
    build: Option<&BuildPlan>,
    crb: Option<&str>,
    continue_offered: bool,
) -> Vec<String> {
```

  and insert, between the `match install { … }` block and the `if continue_offered` line:

```rust
    if let Some(p) = build {
        match &p.blocked {
            Some(why) => out.push(format!("  [b] not offered: {why}")),
            None if p.not.is_empty() => {
                out.push("  [b] build them from source for rpyenv, no root".into())
            }
            None => {
                let not: Vec<String> = p
                    .not
                    .iter()
                    .map(|(n, why)| {
                        let why = why.replacen(" (", ", ", 1);
                        format!("not {n}: {}", why.strip_suffix(')').unwrap_or(&why))
                    })
                    .collect();
                out.push(format!(
                    "  [b] build them from source for rpyenv, no root ({})",
                    not.join("; ")
                ));
            }
        }
    }
```

  Replace `choice`:

```rust
pub fn choice(
    line: &str,
    install_offered: bool,
    build_offered: bool,
    continue_offered: bool,
) -> Option<Route> {
    match line.trim().to_ascii_lowercase().as_str() {
        "" | "p" => Some(Route::System),
        "i" if install_offered => Some(Route::Install),
        "b" if build_offered => Some(Route::Build),
        "c" if continue_offered => Some(Route::Continue),
        _ => None,
    }
}
```

  `Manager::column` is private today (`fn column(self) -> usize` at `deps.rs:93`); it is used
  from the same module, so it needs no change.

- [ ] **Step 5: Keep `install.rs` compiling.** In `commands/install.rs`, the `prompt_lines`
  call gets `None,` after `shown,`; the `choice` call becomes
  `deps::choice(&l, offer.is_ok(), false, continue_offered)`. Task 4 wires both for real.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -q -p pyenv --lib -- deps:: preflight::` and
`cargo test -q -p pyenv --test cli_install_deps`
Expected: all pass (the CLI tests are unchanged in behavior).

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv/src/install/preflight.rs crates/pyenv/src/install/deps.rs crates/pyenv/src/commands/install.rs
git commit -m "M8b: what the build route can build here, and [b] in the prompt"
```

---

### Task 4: `pyenv install` builds the store and links CPython against it

**Files:**
- Modify: `crates/pyenv/src/commands/install.rs` (`resolve_deps`, the per-version loop)
- Modify: `crates/pyenv/src/install/builder.rs` (`Job.store`, `Plan::new`, the `standard`
  step, `verify`/`stdlib_check`/`stdlib_failures`, `run`, `lenient_summary`)
- Modify: `crates/pyenv/src/install/preflight.rs` (`report_deps_with`: one hint line)
- Modify: `crates/pyenv/src/commands/install_win.rs:289`, `crates/pyenv/tests/common/buildharness.rs:96`,
  `crates/pyenv/tests/install_winpkg.rs:105` only if they build a `builder::Job` (add `store: None`)
- Modify: `crates/pyenv/tests/common/fakebuild.rs` (the fake interpreter prints Tcl's library)
- Test: `crates/pyenv/tests/cli_install_deps.rs`, `crates/pyenv/src/install/builder.rs` tests

**Interfaces:**
- Consumes: Task 1 `catalog::table`; Task 2 `store::{ensure, link, Build, Entry, Linked, plain_path, system_openssldir, ca_dir}`, `record::{text, FILE}`; Task 3 `deps::{build_plan, answer_hint, BuildPlan}`, `preflight::build_tools`.
- Produces:
  - `builder::Job` gains `pub store: Option<&'a store::Linked>`.
  - `builder::stdlib_failures(run, minor, tcl_home: Option<&Path>) -> Vec<String>`.

- [ ] **Step 1: Update the fake interpreter.** In `tests/common/fakebuild.rs`'s `PYTHON`
  script, replace the `*tkinter.Tcl*)` line with:

```sh
        *tkinter.Tcl*) [ -n "$FAKE_PY_NO_TCL" ] && exit 1; [ -n "$FAKE_PY_TCL_LIBRARY" ] && echo "$FAKE_PY_TCL_LIBRARY"; exit 0;;
```

- [ ] **Step 2: Write the failing unit tests** in `builder.rs`'s `mod tests`:

```rust
    /// M8 design §4.4's last clause, deferred from M8a: when the build route supplied Tcl/Tk,
    /// Tcl's `info library` must lie inside the version.
    #[test]
    fn tcl_must_find_its_library_inside_the_version_when_the_store_supplied_it() {
        let home = Path::new("/r/versions/3.12.15");
        let says = |lib: &'static str| move |args: &[&str]| -> (bool, String) {
            if args[1].contains("tkinter.Tcl") {
                (true, format!("{lib}\n"))
            } else {
                (true, String::new())
            }
        };
        assert!(stdlib_failures(&says("/r/versions/3.12.15/lib/tcl8.6"), 12, Some(home)).is_empty());
        assert_eq!(
            stdlib_failures(&says("/r/cache/deps/tcl-8.6.18-abc/lib/tcl8.6"), 12, Some(home)),
            ["tkinter (Tcl's library /r/cache/deps/tcl-8.6.18-abc/lib/tcl8.6 is outside /r/versions/3.12.15; needs tk)"]
        );
        // Without the store, where Tcl's library is doesn't matter.
        assert!(stdlib_failures(&says("/usr/share/tcltk/tcl8.6"), 12, None).is_empty());
    }
```

  In the same module, change every existing `stdlib_failures(x, n)` call to
  `stdlib_failures(x, n, None)`, and change the expected text in
  `the_lenient_summary_is_the_three_lines_of_the_spec` to:

```rust
            "pyenv: 3.12.14 was built without: tkinter, uuid
  Add them: sudo apt-get install tk-dev uuid-dev, then pyenv install -f 3.12.14
  In a later rpyenv release this stops the build; choose now with --deps=install,
  --deps=build or --deps=system (or RPYENV_BUILD_DEPS)."
```

- [ ] **Step 3: Write the failing CLI tests** in `tests/cli_install_deps.rs`. Add these helpers
  after `installed`:

```rust
/// A fake `libuuid` source tarball and a catalog naming it by `file://` URL. Its `configure`
/// counts its runs in `<base>/uuid-configure-runs`, sleeps `FAKE_LIB_SLEEP` seconds and fails
/// when `FAKE_LIB_FAIL` is set; `make install` honors DESTDIR.
const UUID_CONFIGURE: &str = r#"#!/bin/sh
echo run >> 'RUNS'
sleep "${FAKE_LIB_SLEEP:-0}"
[ -n "$FAKE_LIB_FAIL" ] && { echo 'configure: error: fake failure'; exit 1; }
for a in "$@"; do case "$a" in --prefix=*) echo "${a#--prefix=}" > prefix.txt;; esac; done
printf 'all:\n\ttrue\ninstall:\n\tsh ./install.sh "$(DESTDIR)"\n' > Makefile
"#;

const UUID_INSTALL: &str = r#"p="$1$(cat prefix.txt)"
mkdir -p "$p/include/uuid" "$p/lib"
echo uuid > "$p/include/uuid/uuid.h"
echo a > "$p/lib/libuuid.a"
"#;

fn fake_catalog(f: &Fixture) -> std::path::PathBuf {
    let runs = f.base.join("uuid-configure-runs");
    let configure = UUID_CONFIGURE.replace("RUNS", &runs.display().to_string());
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (name, body, mode) in [("configure", configure.as_str(), 0o755), ("install.sh", UUID_INSTALL, 0o644)] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(mode);
        b.append_data(&mut h, format!("util-linux-9.9/{name}"), body.as_bytes()).unwrap();
    }
    let gz = b.into_inner().unwrap().finish().unwrap();
    let tarball = f.base.join("util-linux-9.9.tar.gz");
    std::fs::write(&tarball, &gz).unwrap();
    let sha = pyenv::install::checksum::sha256_file(&tarball).unwrap();
    let cat = f.base.join("catalog.tsv");
    f.file(&cat, &format!("libuuid\t9.9\tfile://{}\t{sha}\n", tarball.display()));
    cat
}

fn uuid_runs(f: &Fixture) -> usize {
    std::fs::read_to_string(f.base.join("uuid-configure-runs")).map(|s| s.lines().count()).unwrap_or(0)
}

/// The store's entries (directory names under `cache/deps`, without `.locks`).
fn store_entries(f: &Fixture) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(f.root.join("cache/deps"))
        .map(|rd| rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.retain(|n| n != ".locks");
    v.sort();
    v
}
```

  Delete the `--deps=build` half of `build_and_prebuilt_routes_say_they_come_later` (keep its
  `prebuilt` and `bogus` checks; rename it `the_prebuilt_route_says_it_comes_later`). Then add:

```rust
// allowlist D-123
#[test]
fn deps_build_compiles_the_missing_library_into_the_store_and_links_it() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f);
    let cat = cat.display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("Building libuuid 9.9...\n"), "{}", r.stderr);
    let entries = store_entries(&f);
    assert_eq!(entries.len(), 1, "only the missing library: {entries:?}");
    assert!(entries[0].starts_with("libuuid-9.9-"), "{entries:?}");
    let entry = f.root.join("cache/deps").join(&entries[0]);
    let config = std::fs::read_to_string(f.root.join("versions/3.12.99/lib/rpyenv-config.txt")).unwrap();
    assert!(config.contains(&format!("-I{}/include", entry.display())), "{config}");
    assert!(config.contains(&format!("-L{}/lib", entry.display())), "{config}");
    let record = std::fs::read_to_string(f.root.join("versions/3.12.99/.rpyenv-deps")).unwrap();
    assert!(record.contains("\ndefinition 3.12.99\nlibuuid 9.9 catalog\n"), "{record}");
    assert_eq!(log(&f), "", "no package manager ran");
}

#[test]
fn a_second_build_reuses_the_store_entry() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    assert_eq!(run(&f, &["install", "--deps=build", "3.12.99"], &p).code, 0);
    let r = run(&f, &["install", "-f", "--deps=build", "3.12.99:second"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("Using libuuid 9.9 from "), "{}", r.stderr);
    assert!(!r.stderr.contains("Building libuuid"), "{}", r.stderr);
    assert_eq!(uuid_runs(&f), 1);
    assert_eq!(store_entries(&f).len(), 1);
}

// allowlist D-124
#[test]
fn at_a_terminal_b_builds_the_library_then_python() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("RPYENV_TEST_TTY", "1"),
    ];
    let r = run_input(&f, &["install", "3.12.99"], &p, "b\n");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("  [b] build them from source for rpyenv, no root\n"), "{}", r.stderr);
    assert_eq!(store_entries(&f).len(), 1);
    assert!(installed(&f));
}

#[test]
fn without_x11_headers_deps_build_refuses_tkinter_before_building_anything() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid,tk"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains("pyenv: --deps=build can't build tkinter here: needs the X11 headers (libx11-dev)\n"),
        "{}",
        r.stderr
    );
    assert_eq!(uuid_runs(&f), 0, "nothing was built");
    assert!(store_entries(&f).is_empty());
    assert!(!installed(&f));
}

#[test]
fn at_a_terminal_b_builds_what_it_can_and_the_rest_comes_back_to_the_prompt() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid,tk"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("RPYENV_TEST_TTY", "1"),
    ];
    // `b` builds libuuid; tkinter is left, `[b]` is no longer offered, `c` builds Python.
    let r = run_input(&f, &["install", "3.12.99"], &p, "b\nc\n");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("(not tkinter: needs the X11 headers, libx11-dev)"), "{}", r.stderr);
    assert!(r.stderr.contains("  [b] not offered: nothing missing can be built here\n"), "{}", r.stderr);
    assert_eq!(uuid_runs(&f), 1);
    assert!(installed(&f));
    let record = std::fs::read_to_string(f.root.join("versions/3.12.99/.rpyenv-deps")).unwrap();
    assert!(record.contains("libuuid 9.9 catalog"), "{record}");
}

#[test]
fn a_library_that_fails_to_build_names_its_log_and_installs_nothing() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("FAKE_LIB_FAIL", "1"),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("pyenv: building libuuid 9.9 failed; the log is "), "{}", r.stderr);
    assert!(store_entries(&f).is_empty(), "{:?}", store_entries(&f));
    assert!(!installed(&f));
}

// Review Focus 4
#[test]
fn a_root_with_a_space_refuses_the_build_route_before_downloading() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let spaced = f.base.join("my root");
    // The definition must be found under this root too, or the run stops before detection.
    let def = "plugins/fake/share/python-build/3.12.99";
    std::fs::create_dir_all(spaced.join(def).parent().unwrap()).unwrap();
    std::fs::copy(f.root.join(def), spaced.join(def)).unwrap();
    let spaced_s = spaced.display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("PYENV_ROOT", spaced_s.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "pyenv: --deps=build can't run: PYENV_ROOT has a space or a shell character in its path ({spaced_s})\n"
        )),
        "{}",
        r.stderr
    );
    assert_eq!(uuid_runs(&f), 0);
}

// Review Focus 2
#[test]
fn an_interrupt_during_a_library_build_leaves_no_store_entry() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let owned = env(
        &f,
        &[
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
            ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
            ("RPYENV_TEST_CATALOG", cat.as_str()),
            ("FAKE_LIB_SLEEP", "30"),
        ],
    );
    let mut cmd = f.command(std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")), &f.work, &[]);
    for (k, v) in &owned {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["install", "--deps=build", "3.12.99"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while uuid_runs(&f) == 0 {
        assert!(child.try_wait().unwrap().is_none(), "pyenv ended before configure ran");
        assert!(start.elapsed() < Duration::from_secs(20), "configure never ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    common::sigint_group(child.id());
    let status = child.wait().unwrap();
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert_eq!(status.code(), Some(130), "{err}");
    assert!(store_entries(&f).is_empty(), "{:?}", store_entries(&f));
    assert!(!installed(&f));
}

// allowlist D-125
#[test]
fn a_refusal_names_the_build_route_for_users_without_root() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(&f, &["install", "--deps=system", "3.12.99"], &[("RPYENV_TEST_PREFLIGHT", "libuuid")]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains("Without root, pyenv install --deps=build compiles them from source.\n"),
        "{}",
        r.stderr
    );
}
```

  `common::sigint_group(pgid)` exists (`tests/common/mod.rs:253`). `store_entries` lists
  `.tmp-*` folders too, so the interrupt test also proves none is left. `Fixture::command`
  sets `PYENV_ROOT` first and the extra pairs after it, so the space test's `PYENV_ROOT` wins.

- [ ] **Step 4: Run them to see them fail**

Run: `cargo test -q -p pyenv --test cli_install_deps` and `cargo test -q -p pyenv --lib -- builder::`
Expected: the new tests fail (`--deps=build` still prints "comes in a later rpyenv release");
`stdlib_failures` arity errors.

- [ ] **Step 5: The builder.** In `builder.rs`:

  1. `Job` gains a field after `fetcher`:

```rust
    /// The build store's libraries for this version (M8 design §6.3), when the build route
    /// supplied any.
    pub store: Option<&'a super::store::Linked>,
```

  2. In `Plan::new`, replace the `ldflags` and `cppflags` lines with:

```rust
        // The store's flags come after the user's own (plan ruling P8).
        let store = job.store;
        let store_ld = store.map(|s| s.ldflags.join(" ")).unwrap_or_default();
        let store_cpp = store.map(|s| s.cppflags.join(" ")).unwrap_or_default();
        let ldflags = join(&[&format!("-L{p}/lib"), &rpath, &user_ld, &store_ld]);
        let libs = join(&[&format!("-L{p}/lib"), &rpath, &var("LIBS")]);
        let cppflags = join(&[&format!("-I{p}/include"), &var("CPPFLAGS"), &store_cpp]);
```

     After the `for (k, v) in [("CPPFLAGS", cppflags), …]` loop that fills `child_env`, add:

```rust
        if let Some(s) = store {
            if !s.pkg_config_path.is_empty() {
                let mut v: Vec<String> = s.pkg_config_path.iter().map(|p| p.display().to_string()).collect();
                v.extend(get(env, "PKG_CONFIG_PATH").filter(|x| !x.is_empty()));
                child_env.push(("PKG_CONFIG_PATH".into(), v.join(":").into()));
            }
            if let Some(bin) = &s.path {
                let path = get(env, "PATH").unwrap_or_default();
                child_env.push(("PATH".into(), format!("{}:{path}", bin.display()).into()));
            }
            for (k, v) in &s.configure_env {
                if var(k).is_empty() {
                    configure_env.push((k.clone(), v.clone()));
                }
            }
        }
```

     and replace `configure.extend(array);` with:

```rust
        configure.extend(array);
        // Before the user's options, which win (ruling P8).
        if let Some(s) = store {
            configure.extend(s.configure.iter().cloned());
        }
```

  3. In `run_step`'s `"standard"` arm, after the `if staging { … txn.place … }` block and
     before `Ok(())`:

```rust
            // Tcl and Tk find their script folders beside the executable (M8 design §6.3).
            if staging {
                if let Some(s) = job.store {
                    for d in &s.scripts {
                        let to = plan.prefix.join("lib").join(d.file_name().unwrap_or_default());
                        copy_tree(d, &to).map_err(|e| {
                            InstallError::Message(format!(
                                "pyenv: cannot copy {} into {}: {e}",
                                d.display(),
                                to.display()
                            ))
                        })?;
                    }
                }
            }
```

  4. `stdlib_failures` gains `tcl_home: Option<&Path>`; its Tcl check becomes:

```rust
    let (tcl_ok, tcl_out) = run(&["-c", "import tkinter; print(tkinter.Tcl().eval('info library'))"]);
    if !tcl_ok {
        out.push("tkinter (Tcl doesn't start; needs tk)".into());
    } else if let Some(home) = tcl_home {
        // M8 design §4.4: a store-built Tcl must use the version's copy of its library.
        let lib = tcl_out.trim();
        if !Path::new(lib).starts_with(home) {
            out.push(format!(
                "tkinter (Tcl's library {lib} is outside {}; needs tk)",
                home.display()
            ));
        }
    }
```

     In `stdlib_check`, the call becomes
     `stdlib_failures(&run, minor, job.store.filter(|s| !s.scripts.is_empty()).map(|_| plan.prefix.as_path()))`.

  5. In `lenient_summary`, the last two lines of the format string become:

```rust
  In a later rpyenv release this stops the build; choose now with --deps=install,
  --deps=build or --deps=system (or RPYENV_BUILD_DEPS)."
```

  6. In `run`, in the `Ok(())` arm of `match &result`, before the `if !job.opts.keep` line:

```rust
            if let Some(s) = job.store {
                let file = plan.prefix.join(super::record::FILE);
                if let Err(e) = std::fs::write(&file, super::record::text(&job.found.name, &s.entries)) {
                    say(&format!("pyenv: warning: cannot write {}: {e}", file.display()));
                }
            }
```

  7. Every other `builder::Job { … }` in the workspace gets `store: None,` (`grep -rn
     "Job {" crates/`: `commands/install.rs` gets the real value in Step 6; the others, if they
     build this `Job`, get `None`).

- [ ] **Step 6: The command.** In `commands/install.rs`:

  1. `use` additions: `crate::install::{catalog, store}` (both `#[cfg(unix)]`-only modules;
     this file is Linux-only already).
  2. Above `resolve_deps`, add:

```rust
/// What `resolve_deps` settled. `cont`: the prompt's `[c]`, so the builder warns at the end
/// instead of failing. `store`: the build store's entries to link (`[b]`, `--deps=build`).
struct Resolution {
    cont: bool,
    store: Vec<store::Entry>,
}

/// Builds catalog keys into the store (`store::ensure` with the job's setup).
type StoreBuild<'a> =
    dyn FnMut(&[&'static str], &mut dyn FnMut(&str)) -> Result<Vec<store::Entry>, InstallError> + 'a;
```

  3. `resolve_deps` gets two more parameters, `root: &Path` and `build: &mut StoreBuild`, and
     returns `Result<Resolution, i32>`. Inside it:
     - The early rejection keeps only `prebuilt`:

```rust
    if chosen == Some(Route::Prebuilt) {
        say("pyenv: the prebuilt route comes in a later rpyenv release");
        return Err(1);
    }
```

     - Before `loop {`, add:

```rust
    // The build route (M8 design §6): what the store supplied counts as present (ruling P6).
    let root_ok: Result<(), String> = if store::plain_path(root) {
        Ok(())
    } else {
        Err(format!(
            "PYENV_ROOT has a space or a shell character in its path ({})",
            root.display()
        ))
    };
    let mut tools: Option<preflight::Tools> = None;
    let mut supplied: Vec<&'static str> = Vec::new();
    let mut store_entries: Vec<store::Entry> = Vec::new();
```

     - `let missing = preflight::check3(&probe_env, patch, minor);` becomes:

```rust
        let mut missing = preflight::check3(&probe_env, patch, minor);
        missing.retain(|d| !supplied.contains(&d.lib));
```

     - `if missing.is_empty() { return Ok(false); }` becomes
       `if missing.is_empty() { return Ok(Resolution { cont: false, store: store_entries }); }`
     - After `continue_offered` is computed, add:

```rust
        let plan = (interactive || chosen == Some(Route::Build)).then(|| {
            let t = *tools.get_or_insert_with(|| preflight::build_tools(get));
            deps::build_plan(&missing, minor, &t, root_ok.clone(), manager)
        });
        let build_offered = plan.as_ref().is_some_and(|p| p.blocked.is_none());
```

     - In the interactive branch: `prompt_lines(…, shown, plan.as_ref(), crb.as_deref(), continue_offered)`,
       `deps::choice(&l, offer.is_ok(), build_offered, continue_offered)`, and the bad-answer
       line `None => say(&deps::answer_hint(offer.is_ok(), build_offered, continue_offered))`
       (replacing the four-way `match`).
     - The lenient branch's `return if hard_missing { Err(1) } else { Ok(false) };` becomes
       `return if hard_missing { Err(1) } else { Ok(Resolution { cont: false, store: Vec::new() }) };`
     - The `match (route, offer)` gets, first:

```rust
            (Route::Build, _) => {
                let Some(p) = plan else {
                    return Err(1);
                };
                // Without the prompt, every module is required (D5): refuse before building
                // anything when one can't be built here (rulings P3-P5).
                if !interactive {
                    if let Some(why) = &p.blocked {
                        say(&format!("pyenv: --deps=build can't run: {why}"));
                        return Err(1);
                    }
                    if !p.not.is_empty() {
                        for (n, why) in &p.not {
                            say(&format!("pyenv: --deps=build can't build {n} here: {why}"));
                        }
                        return Err(1);
                    }
                }
                match build(&p.keys, &mut |s: &str| say(s)) {
                    Ok(e) => {
                        store_entries.extend(e);
                        supplied.extend(p.libs.iter().copied());
                    }
                    Err(InstallError::Interrupted) => return Err(130),
                    Err(InstallError::Message(m)) => {
                        say(&m);
                        return Err(1);
                    }
                    Err(InstallError::Failed) => return Err(1),
                }
            }
            (Route::Continue, _) => return Ok(Resolution { cont: true, store: store_entries }),
```

       (replacing the old `(Route::Continue, _) => return Ok(true),` arm).
  4. In `install()`, in the per-version loop:
     - Move the `cache` and `fetcher` lines (`let cache = …; let fetcher = Fetcher::from_env(&job_get, cache);`)
       up, to just after `let job_get = |k: &str| builder::get(&job_env, k);` (the first one),
       so the store can download with the same mirror and cache settings.
     - Hoist `minor` out of the `if compiles && …` block: compute it right after `compiles`.
     - Replace the `resolve_deps` call and its `match` with:

```rust
        let mut store_entries: Vec<store::Entry> = Vec::new();
        if compiles && job_get("RPYENV_SKIP_PREFLIGHT").as_deref() != Some("1") {
            let patch = f.patch || needs_patch(&found, &parsed);
            let abs_root = std::path::absolute(&ctx.root).unwrap_or_else(|_| ctx.root.clone());
            let snapshot = job_env.clone();
            let mut build = |keys: &[&'static str], say: &mut dyn FnMut(&str)| {
                let rows = catalog::table(&|k: &str| builder::get(&snapshot, k))
                    .map_err(InstallError::Message)?;
                let has_files = |d: &str| std::fs::read_dir(d).is_ok_and(|mut r| r.next().is_some());
                let openssldir = store::system_openssldir(&|p| Path::new(p).is_file(), &has_files)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| store::ca_dir(&abs_root));
                store::ensure(
                    &store::Build {
                        root: &abs_root,
                        rows: &rows,
                        definition: &parsed,
                        env: &snapshot,
                        fetcher: &fetcher,
                        openssldir,
                        verbose: f.verbose,
                    },
                    keys,
                    say,
                )
            };
            let ok = match minor {
                Some(m) => resolve_deps(&job_get, &name, m, patch, f.deps.as_deref(), &abs_root, &mut build),
                // CPython 2.x keeps today's check (plan ruling P2 of M8a).
                None => old_preflight(&job_get, patch).map(|()| Resolution { cont: false, store: Vec::new() }),
            };
            match ok {
                Err(code) => {
                    if code == 130 {
                        return Output::new().with_code(130);
                    }
                    status = status.max(code);
                    break;
                }
                Ok(r) => {
                    // `[c]`: build without the optional libraries; the builder warns at the end.
                    if r.cont {
                        job_env.retain(|(k, _)| k != "RPYENV_DEPS_MODE");
                        job_env.push(("RPYENV_DEPS_MODE".into(), "continue".into()));
                    }
                    store_entries = r.store;
                }
            }
        }
        let linked = (!store_entries.is_empty()).then(|| {
            store::link(store_entries, minor.unwrap_or(u32::MAX), &|p| std::fs::read_to_string(p).ok())
        });
```

       (`job_get` borrows `job_env` immutably while `build` holds `snapshot`, a copy, so the
       later `job_env.push` compiles; if the borrow checker still complains about `job_get`,
       re-create `job_get` after the block as the existing code does.)
     - The `Job { … }` gets `store: linked.as_ref(),`.

- [ ] **Step 7: The hint for users without root.** In `preflight::report_deps_with`, before
  `if refuse { lines.push("To build anyway, set RPYENV_SKIP_PREFLIGHT=1.".to_string()); }`:

```rust
    // M8b: the way out without root (not when the compiler or make itself is missing).
    if refuse
        && !missing
            .iter()
            .any(|d| matches!(d.lib, "compiler" | "make" | "patch"))
    {
        lines.push("Without root, pyenv install --deps=build compiles them from source.".to_string());
    }
```

  Then run `cargo test -q -p pyenv --lib -- preflight::` and update every test that compares
  `report_deps`/`report_deps_with` output line by line: the new line comes right before
  `To build anyway, set RPYENV_SKIP_PREFLIGHT=1.` whenever the build is refused and no program
  is missing. (`report`, the pre-M8 2.x report, is unchanged.)

- [ ] **Step 8: Run all the tests**

Run: `cargo test -q -p pyenv --test cli_install_deps`, `cargo test -q -p pyenv --lib`, then
`cargo test -q --workspace`
Expected: all pass.

- [ ] **Step 9: Commit**

```bash
git add -A crates/pyenv
git commit -m "M8b: --deps=build and [b] build the missing libraries into the store and link CPython against them"
```

---

### Task 5: Warn when the catalog moves past an installed version's libraries; refresh the CA bundle

**Files:**
- Modify: `crates/pyenv/src/install/record.rs` (add `stale_lines`)
- Modify: `crates/pyenv/src/commands/install.rs` (call it, and `store::place_ca`, once per run)
- Test: `crates/pyenv/src/install/record.rs` tests, `crates/pyenv/tests/cli_install_deps.rs`

**Interfaces:**
- Consumes: Task 2 `record::{parse, FILE, Record}`, `store::{place_ca, ca_dir}`; Task 1 `catalog::{table, pinned, Origin}`.
- Produces: `pub fn stale_lines(versions: &Path, now: &dyn Fn(&str, &str, Origin) -> Option<String>) -> Vec<String>`
  (`now(library, definition, origin)` is the version rpyenv would build today).

- [ ] **Step 1: Write the failing unit test** in `record.rs`'s tests:

```rust
    #[test]
    fn stale_lines_name_each_changed_library_and_how_to_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path();
        let put = |name: &str, body: &str| {
            std::fs::create_dir_all(v.join(name)).unwrap();
            std::fs::write(v.join(name).join(FILE), body).unwrap();
        };
        put("3.12.15", "definition 3.12.15\nopenssl 3.6.4 definition\nsqlite 3.54.0 catalog\nzlib 1.3.2 catalog\n");
        put("fresh", "definition 3.13.16\nzlib 1.3.2 catalog\n");
        put("mine", "definition 3.14.8\nsqlite 3.54.0 catalog\n");
        std::fs::create_dir_all(v.join("no-record")).unwrap();
        let now = |lib: &str, def: &str, o: Origin| -> Option<String> {
            match (lib, o) {
                ("openssl", Origin::Definition) => (def == "3.12.15").then(|| "3.6.5".to_string()),
                ("sqlite", _) => Some("3.54.1".into()),
                ("zlib", _) => Some("1.3.2".into()),
                _ => None,
            }
        };
        assert_eq!(
            stale_lines(v, &now),
            [
                "pyenv: 3.12.15 was built with openssl 3.6.4, sqlite 3.54.0; rpyenv now builds openssl 3.6.5, sqlite 3.54.1. To rebuild it: pyenv install -f 3.12.15",
                "pyenv: mine was built with sqlite 3.54.0; rpyenv now builds sqlite 3.54.1. To rebuild it: pyenv install -f 3.14.8:mine",
            ]
        );
    }
```

- [ ] **Step 2: Write the failing CLI tests** in `cli_install_deps.rs`:

```rust
// allowlist D-126
#[test]
fn pyenv_install_warns_once_per_version_built_with_an_older_library() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    f.file(&f.root.join("versions/3.12.98/.rpyenv-deps"), "definition 3.12.98\nlibuuid 9.8 catalog\n");
    f.file(&f.root.join("versions/3.12.97/.rpyenv-deps"), "definition 3.12.97\nlibuuid 9.9 catalog\n");
    let p = [("RPYENV_TEST_PREFLIGHT", ""), ("RPYENV_TEST_CATALOG", cat.as_str())];
    let r = run(&f, &["install", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let want = "pyenv: 3.12.98 was built with libuuid 9.8; rpyenv now builds libuuid 9.9. To rebuild it: pyenv install -f 3.12.98\n";
    assert_eq!(r.stderr.matches(want).count(), 1, "{}", r.stderr);
    assert!(!r.stderr.contains("3.12.97 was built"), "{}", r.stderr);
    // Listing versions is not installing: no warning.
    let l = run(&f, &["install", "--list"], &p);
    assert!(!l.stderr.contains("was built with"), "{}", l.stderr);
}

// allowlist D-127
#[test]
fn pyenv_install_replaces_an_outdated_ca_bundle() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let pem = f.base.join("cacert-2099-01-01.pem");
    f.file(&pem, "-----BEGIN CERTIFICATE-----\nnew\n-----END CERTIFICATE-----\n");
    let sha = pyenv::install::checksum::sha256_file(&pem).unwrap();
    let cat = f.base.join("catalog.tsv");
    f.file(&cat, &format!("cacert\t2099-01-01\tfile://{}\t{sha}\n", pem.display()));
    let bundle = f.root.join(".rpyenv/ca/cert.pem");
    f.file(&bundle, "old\n");
    let cat_s = cat.display().to_string();
    let p = [("RPYENV_TEST_PREFLIGHT", ""), ("RPYENV_TEST_CATALOG", cat_s.as_str())];
    let r = run(&f, &["install", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(std::fs::read_to_string(&bundle).unwrap(), std::fs::read_to_string(&pem).unwrap());
    assert!(
        r.stderr.contains(&format!("pyenv: updated the CA bundle in {} (2099-01-01)\n", bundle.display())),
        "{}",
        r.stderr
    );
    // Up to date: nothing is fetched or said.
    let again = run(&f, &["install", "-f", "3.12.99"], &p);
    assert!(!again.stderr.contains("CA bundle"), "{}", again.stderr);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -q -p pyenv --lib -- record::` and `cargo test -q -p pyenv --test cli_install_deps pyenv_install_`
Expected: `stale_lines` not found; the CLI tests see no warning line.

- [ ] **Step 4: Implement `stale_lines`** in `record.rs`:

```rust
/// One line per installed version whose recorded libraries differ from what rpyenv would build
/// today (M8 design §6.3, ruling P12), in name order. `now(library, definition, origin)`: that
/// version, or `None` when unknown (the line then leaves the library out).
pub fn stale_lines(
    versions: &std::path::Path,
    now: &dyn Fn(&str, &str, Origin) -> Option<String>,
) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(versions) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let Ok(t) = std::fs::read_to_string(versions.join(&name).join(FILE)) else {
            continue;
        };
        let Some(r) = parse(&t) else { continue };
        let (mut was, mut is) = (Vec::new(), Vec::new());
        for (lib, v, origin) in &r.libs {
            if let Some(cur) = now(lib, &r.definition, *origin) {
                if &cur != v {
                    was.push(format!("{lib} {v}"));
                    is.push(format!("{lib} {cur}"));
                }
            }
        }
        if was.is_empty() {
            continue;
        }
        let rebuild = if name == r.definition {
            name.clone()
        } else {
            format!("{}:{name}", r.definition)
        };
        out.push(format!(
            "pyenv: {name} was built with {}; rpyenv now builds {}. To rebuild it: pyenv install -f {rebuild}",
            was.join(", "),
            is.join(", ")
        ));
    }
    out
}
```

- [ ] **Step 5: Call both from `pyenv install`.** In `commands/install.rs`'s `install()`,
  right after `watch_interrupt();` (so `--list`, `--help`, `--version` and `--complete`,
  which return earlier, never do this):

```rust
    // M8 design §6.3 and §6.5: once per run, before any version is installed.
    if let Ok(rows) = catalog::table(&env) {
        let now = |lib: &str, def: &str, origin: catalog::Origin| -> Option<String> {
            if origin == catalog::Origin::Definition {
                let pinned = defs::find(&ctx.root, def, &env)
                    .and_then(|found| defs::parse(&found, &env, &|n| defs::find(&ctx.root, n, &env)).ok())
                    .and_then(|d| catalog::pinned(lib, &d));
                if let Some(r) = pinned {
                    return Some(r.version);
                }
            }
            rows.iter().find(|r| r.key == lib).map(|r| r.version.clone())
        };
        for l in crate::install::record::stale_lines(&ctx.versions_dir(), &now) {
            say(&l);
        }
        let bundle = store::ca_dir(&ctx.root).join("cert.pem");
        if bundle.is_file() {
            let version = rows.iter().find(|r| r.key == "cacert").map(|r| r.version.clone()).unwrap_or_default();
            match store::place_ca(&ctx.root, &rows, &probe, &mut |s: &str| say(s)) {
                Ok(true) => say(&format!("pyenv: updated the CA bundle in {} ({version})", bundle.display())),
                Ok(false) => {}
                Err(InstallError::Message(m)) => say(&format!("pyenv: warning: cannot update the CA bundle: {m}")),
                Err(_) => say(&format!("pyenv: warning: cannot update the CA bundle in {}", bundle.display())),
            }
        }
    }
```

  `probe` (the `Fetcher` without cache) is created a few lines below `watch_interrupt()`; move
  its `let probe = Fetcher::from_env(&env, None);` line above this block.

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -q -p pyenv --lib -- record::` and `cargo test -q -p pyenv --test cli_install_deps`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv/src/install/record.rs crates/pyenv/src/commands/install.rs crates/pyenv/tests/cli_install_deps.rs
git commit -m "M8b: pyenv install warns when a version's built-in libraries are behind the catalog, and refreshes the CA bundle"
```

---

### Task 6: CI: the build route on real distributions

**Files:**
- Modify: `ci/check_deps.sh` (a `BUILD=1` mode)
- Modify: `.github/workflows/install-linux.yml` (two build legs; `crates/pyenv/deps/**` in the path filter)

**Interfaces:**
- Consumes: the static musl `pyenv` the `deps` job already builds; Tasks 1–5 behavior.
- Produces: CI evidence for spec §9 row 3 ("`build` (M8b): only a compiler, no `-dev`
  packages; §4.4 check"), the "check must be able to fail" rule, and Review Focus 3.

- [ ] **Step 1: Add the build mode to `ci/check_deps.sh`.** Extend the header comment with:

```bash
# BUILD=1 (M8b, M8 design §9 row 3): the container gets a compiler, make, patch, Perl, the X11
# headers and CA certificates, and no -dev package of any library and no pkg-config.
# `pyenv install --deps=build` compiles every missing library into the store, builds a CPython
# whose every module imports, and records them; a second build reuses the store; the version
# keeps working without the store; and (Ubuntu) a store entry with its library removed makes
# the check after the build fail naming the module. WEEKLY=1 adds the newest 3.14 (OpenSSL 4
# and upstream's patch) and 3.8 (OpenSSL 1.1.1).
```

  Change stage 0's `case` so build mode installs its own set:

```bash
if [ -n "${BUILD:-}" ]; then
  case "$distro" in
    debian|ubuntu)
      apt-get update -q
      apt-get install -yq gcc libc6-dev make patch perl libx11-dev ca-certificates
      rm -rf /var/lib/apt/lists/* ;;
    fedora|rocky) dnf install -y gcc make patch perl-core libX11-devel ;;
    tumbleweed) zypper --non-interactive install gcc make patch perl libX11-devel ca-certificates ;;
    arch) pacman -Syu --noconfirm gcc make patch perl libx11 ;;
    *) echo "::error::unknown distro $distro"; exit 1 ;;
  esac
else
  case "$distro" in
    # … the existing case, unchanged …
  esac
fi
```

  After the `imports()` function, add the build stage (it ends the script in build mode):

```bash
if [ -n "${BUILD:-}" ]; then
  # Stage B1: the build route, without a terminal.
  "$bin" install --deps=build "$v" </dev/null 2>&1 | tee /tmp/build1.log
  [ "${PIPESTATUS[0]}" -eq 0 ] || { echo "::error::$distro: --deps=build failed"; exit 1; }
  imports "$v"
  rec="$root/versions/$v/.rpyenv-deps"
  [ -f "$rec" ] || { echo "::error::$distro: no $rec"; exit 1; }
  cat "$rec"
  # No libssl-dev was installed: OpenSSL must have come from the store.
  grep -q '^openssl ' "$rec" || { echo "::error::$distro: OpenSSL is not in the record"; exit 1; }
  # Tcl finds its library inside the version, not in the store (§4.4).
  lib=$("$root/versions/$v/bin/python3" -c "import tkinter; print(tkinter.Tcl().eval('info library'))")
  case "$lib" in "$root/versions/$v/"*) ;; *) echo "::error::$distro: Tcl's library is $lib"; exit 1 ;; esac
  # The system itself is unchanged: strict detection still finds what is missing.
  if detect_empty; then echo "::error::$distro: detection is empty without any -dev package"; exit 1; fi
  # Stage B2: reuse (D6). A second build compiles no library.
  "$bin" install -f --deps=build "$v" </dev/null > /tmp/build2.log 2>&1 ||
    { tail -30 /tmp/build2.log; echo "::error::$distro: the second --deps=build failed"; exit 1; }
  if grep -q '^Building ' /tmp/build2.log; then
    grep '^Building ' /tmp/build2.log; echo "::error::$distro: a library was built again"; exit 1
  fi
  grep -q '^Using openssl ' /tmp/build2.log || { echo "::error::$distro: OpenSSL was not reused"; exit 1; }
  # Stage B3 (Review Focus 3): the store is only a cache.
  mv "$root/cache/deps" "$root/cache/deps.off"
  imports "$v"
  mv "$root/cache/deps.off" "$root/cache/deps"
  # Stage B4 (Ubuntu): the check can fail. A store entry whose library is gone makes the check
  # after the build fail naming uuid; the rollback keeps the version from stage B2.
  if [ "$distro" = ubuntu ]; then
    rm "$root"/cache/deps/libuuid-*/lib/libuuid.a
    rc=0
    out=$("$bin" install -f --deps=build "$v" 2>&1 </dev/null) || rc=$?
    tail -5 <<<"$out"
    { [ "$rc" -ne 0 ] && grep -q 'uuid (import _uuid failed' <<<"$out"; } ||
      { echo "::error::$distro: a store without libuuid.a did not fail the check naming uuid"; exit 1; }
    imports "$v"
    echo "== the check after the build failed naming uuid, as it must"
  fi
  if [ -n "${WEEKLY:-}" ]; then
    for w in "$(newest 3.14)" "$(newest 3.8)"; do
      echo "== weekly: Python $w through the build route"
      "$bin" install --deps=build "$w" </dev/null
      imports "$w"
    done
  fi
  echo "== the build route works on $distro"
  exit 0
fi
```

  (`imports` checks `crypt` for 3.12 and older and `compression.zstd` for 3.14; for 3.8 it
  checks the same modules as for 3.12.)

- [ ] **Step 2: Add the legs** to the `deps` job in `.github/workflows/install-linux.yml`.
  In `matrix.include`, add:

```yaml
          - { distro: ubuntu, image: "ubuntu:24.04", full: true, version: "3.12.14", build: true }
          - { distro: fedora, image: "fedora:44", full: true, version: "3.12.14", build: true }
          # Spec §9: the build route on all six, weekly (and on a manual run).
          - { distro: debian, image: "debian:stable-slim", full: true, version: "3.12.14", build: true, weekly_only: true }
          - { distro: rocky, image: "rockylinux:9", full: true, version: "3.12.14", build: true, weekly_only: true }
          - { distro: tumbleweed, image: "opensuse/tumbleweed", full: true, version: "3.12.14", build: true, weekly_only: true }
          - { distro: arch, image: "archlinux:latest", full: true, version: "3.12.14", build: true, weekly_only: true }
```

  Give the container step an `if:` so the weekly-only legs skip their work on pull requests and
  nightly runs (the job still appears, green, with the step skipped):

```yaml
        if: ${{ !matrix.weekly_only || github.event.schedule == '41 4 * * 0' || github.event_name == 'workflow_dispatch' }}
```

  Change the job's `name:` to
  `deps (${{ matrix.distro }}${{ matrix.build && ', build' || '' }})`, add
  `BUILD: ${{ matrix.build || false }}` to the step's `env:`, and in its `run:` add, after the
  `DETECT_ONLY` line:

```bash
          if [ "$BUILD" = true ]; then args+=(-e BUILD=1); fi
```

  and guard the `DISCRIMINATE` line so build legs skip it:
  `if [ "$DISTRO" = ubuntu ] && [ "$BUILD" != true ]; then args+=(-e DISCRIMINATE=1); fi`.
  Raise `timeout-minutes` from 75 to 90. Add `- "crates/pyenv/deps/**"` to the
  `pull_request.paths` list. Extend the comment above the job with one sentence: "Two more
  legs (`build: true`) run M8b's build route on Ubuntu and Fedora with no `-dev` package at all
  (`ci/check_deps.sh` BUILD=1)."

- [ ] **Step 3: Check the script parses and the workflow is valid YAML**

Run: `bash -n ci/check_deps.sh` and
`python -c "import yaml,sys; yaml.safe_load(open(r'.github/workflows/install-linux.yml', encoding='utf-8'))"`
Expected: no output, exit 0 (install PyYAML into a throwaway venv if it is missing; never into
the system Python).

- [ ] **Step 4: Run the Ubuntu build leg locally in a throwaway container** (the dev host's
  WSL podman; never on the host itself). Build the static musl `pyenv` in WSL
  (`cargo build --release --target x86_64-unknown-linux-musl -p pyenv -p pyenv-shim`), then:

```bash
podman --cgroup-manager=cgroupfs run --rm -e BUILD=1 \
  -v "$PWD:/w:ro" -v "$PWD/target/x86_64-unknown-linux-musl/release:/in:ro" -w /root ubuntu:24.04 \
  bash -c "cp /in/pyenv /in/pyenv-shim . && bash /w/ci/check_deps.sh ubuntu 3.12.14"
```

Expected: ends with `== the build route works on ubuntu`. Record the wall-clock time in the
task report. If the run shows a recipe fault the spike missed (see the plan's "Open items"),
fix the recipe in `catalog.rs`, rerun, and say what changed in the report.

- [ ] **Step 5: Commit**

```bash
git add ci/check_deps.sh .github/workflows/install-linux.yml
git commit -m "M8b: CI builds CPython through the build route on Ubuntu and Fedora with no -dev packages"
```

---

### Task 7: The weekly catalog check (`ci/deps_catalog.py`)

**Files:**
- Create: `ci/deps_catalog.py`
- Create: `ci/test_deps_catalog.py`
- Modify: `.github/workflows/sync-python-build.yml`

**Interfaces:**
- Consumes: `crates/pyenv/deps/catalog.tsv` (Task 1's format).
- Produces: `python3 ci/deps_catalog.py --catalog <tsv> [--write] [--report <md>]`; exit 0
  (up to date, or written), 1 (a source couldn't be read; nothing written), 2 (bad usage).

- [ ] **Step 1: Write the failing tests** `ci/test_deps_catalog.py` (no network; the
  `parity` coverage job runs `python3 -m unittest discover -s ci -p "test_*.py"`):

```python
"""ci/deps_catalog.py without the network: version parsing, the per-library filters, and
rewriting a row."""
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import deps_catalog as dc  # noqa: E402

SHA = "a" * 64


class Versions(unittest.TestCase):
    def test_versions_sort_numerically(self):
        self.assertEqual(dc.newest(["1.3.1", "1.3.10", "1.3.2"]), "1.3.10")

    def test_listing_matches_take_the_newest(self):
        page = '<a href="ncurses-6.5.tar.gz">x</a> <a href="ncurses-6.6.tar.gz">y</a> ncurses-6.4.tar.gz'
        self.assertEqual(dc.newest(dc.matches(page, r"ncurses-(\d+\.\d+)\.tar\.gz")), "6.6")

    def test_tcl_and_tk_stay_on_8_6_and_openssl_on_3(self):
        self.assertEqual(dc.allowed("tcl", ["8.6.18", "9.0.3", "8.6.19"]), ["8.6.18", "8.6.19"])
        self.assertEqual(dc.allowed("openssl", ["3.6.5", "4.0.3", "3.5.9"]), ["3.6.5", "3.5.9"])
        self.assertEqual(dc.allowed("zlib", ["1.3.2"]), ["1.3.2"])

    def test_github_tags_become_versions(self):
        tags = ["v5.8.4", "v5.8.3", "openssl-3.6.5", "v6.0.0beta1"]
        self.assertEqual(dc.tag_versions(tags), ["5.8.4", "5.8.3", "3.6.5"])

    def test_sqlite_product_line(self):
        page = "PRODUCT,3.54.1,2026/sqlite-autoconf-3540100.tar.gz,3332984,b6ced585\n"
        self.assertEqual(dc.sqlite_latest(page), ("3.54.1", "https://sqlite.org/2026/sqlite-autoconf-3540100.tar.gz"))

    def test_cacert_names_sort_by_date(self):
        page = "cacert-2026-07-16.pem cacert-2026-09-25.pem cacert-2026-08-13.pem"
        self.assertEqual(dc.cacert_latest(page), "2026-09-25")


class Rows(unittest.TestCase):
    TSV = "# comment\nzlib\t1.3.1\thttps://x/zlib-1.3.1.tar.gz\t" + SHA + "\nxz\t5.8.4\thttps://x/xz-5.8.4.tar.gz\t" + SHA + "\n"

    def test_rows_are_read_and_comments_kept_on_write(self):
        rows = dc.read_rows(self.TSV)
        self.assertEqual([r[0] for r in rows], ["zlib", "xz"])
        out = dc.replace_row(self.TSV, "zlib", "1.3.2", "https://x/zlib-1.3.2.tar.gz", "b" * 64)
        self.assertIn("# comment\n", out)
        self.assertIn("zlib\t1.3.2\thttps://x/zlib-1.3.2.tar.gz\t" + "b" * 64 + "\n", out)
        self.assertIn("xz\t5.8.4\t", out)

    def test_every_catalog_key_has_a_source(self):
        here = os.path.dirname(os.path.abspath(__file__))
        path = os.path.join(here, "..", "crates", "pyenv", "deps", "catalog.tsv")
        with open(path, encoding="utf-8") as f:
            keys = [r[0] for r in dc.read_rows(f.read())]
        self.assertEqual(sorted(keys), sorted(dc.SOURCES))


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run them to see them fail**

Run: `python -m unittest discover -s ci -p "test_deps_catalog.py" -v`
Expected: `ModuleNotFoundError: No module named 'deps_catalog'`.

- [ ] **Step 3: Write `ci/deps_catalog.py`:**

```python
"""The build catalog's weekly check (M8 design §6.1): for every library in
crates/pyenv/deps/catalog.tsv, the newest upstream release. With --write, each row that is
behind moves to that release: the new tarball is downloaded here and its SHA-256 computed, so
the pull request the sync workflow opens carries a hash a person reviews before it ships.
Tcl and Tk stay on 8.6 (spec D11) and OpenSSL on 3.x (definitions pin their own).

  python3 ci/deps_catalog.py --catalog crates/pyenv/deps/catalog.tsv [--write] [--report out.md]

Exit 0: up to date, or rows written. 1: a source couldn't be read (nothing is written).
2: bad usage.
"""
import argparse
import hashlib
import json
import os
import re
import sys
import tempfile
import urllib.request

GH = "https://github.com/{repo}/releases/download/{tag}/{name}"

# key: (kind, details). Kinds: listing (url, regex, url template), github (repo, tag template,
# file template), sqlite, kernel (util-linux's two-level listing), cacert.
SOURCES = {
    "pkgconf": ("listing", "https://distfiles.ariadne.space/pkgconf/", r"pkgconf-(\d+\.\d+\.\d+)\.tar\.xz", "https://distfiles.ariadne.space/pkgconf/pkgconf-{v}.tar.xz"),
    "zlib": ("github", "madler/zlib", "v{v}", "zlib-{v}.tar.gz"),
    "bzip2": ("listing", "https://sourceware.org/pub/bzip2/", r"bzip2-(\d+\.\d+\.\d+)\.tar\.gz", "https://sourceware.org/pub/bzip2/bzip2-{v}.tar.gz"),
    "xz": ("github", "tukaani-project/xz", "v{v}", "xz-{v}.tar.gz"),
    "ncurses": ("listing", "https://ftp.gnu.org/gnu/ncurses/", r"ncurses-(\d+\.\d+)\.tar\.gz", "https://ftp.gnu.org/gnu/ncurses/ncurses-{v}.tar.gz"),
    "readline": ("listing", "https://ftp.gnu.org/gnu/readline/", r"readline-(\d+\.\d+)\.tar\.gz", "https://ftp.gnu.org/gnu/readline/readline-{v}.tar.gz"),
    "gdbm": ("listing", "https://ftp.gnu.org/gnu/gdbm/", r"gdbm-(\d+\.\d+(?:\.\d+)?)\.tar\.gz", "https://ftp.gnu.org/gnu/gdbm/gdbm-{v}.tar.gz"),
    "sqlite": ("sqlite",),
    "libffi": ("github", "libffi/libffi", "v{v}", "libffi-{v}.tar.gz"),
    "tcl": ("listing", "https://www.tcl-lang.org/software/tcltk/download.html", r"tcl(8\.6\.\d+)-src\.tar\.gz", "https://downloads.sourceforge.net/project/tcl/Tcl/{v}/tcl{v}-src.tar.gz"),
    "tk": ("listing", "https://www.tcl-lang.org/software/tcltk/download.html", r"tk(8\.6\.\d+)-src\.tar\.gz", "https://downloads.sourceforge.net/project/tcl/Tcl/{v}/tk{v}-src.tar.gz"),
    "libuuid": ("kernel",),
    "libxcrypt": ("github", "besser82/libxcrypt", "v{v}", "libxcrypt-{v}.tar.xz"),
    "libtirpc": ("listing", "https://sourceforge.net/projects/libtirpc/files/libtirpc/", r"libtirpc/(\d+\.\d+\.\d+)/", "https://downloads.sourceforge.net/project/libtirpc/libtirpc/{v}/libtirpc-{v}.tar.bz2"),
    "libnsl": ("github", "thkukuk/libnsl", "v{v}", "libnsl-{v}.tar.xz"),
    "zstd": ("github", "facebook/zstd", "v{v}", "zstd-{v}.tar.gz"),
    "openssl": ("github", "openssl/openssl", "openssl-{v}", "openssl-{v}.tar.gz"),
    "cacert": ("cacert",),
}


def key(v):
    return [int(p) if p.isdigit() else -1 for p in re.split(r"[.\-]", v)]


def newest(versions):
    return max(versions, key=key) if versions else None


def matches(text, pattern):
    return sorted(set(re.findall(pattern, text)), key=key)


def allowed(lib, versions):
    """Spec D11 (Tcl/Tk 8.6) and OpenSSL 3.x for the catalog's default."""
    if lib in ("tcl", "tk"):
        return [v for v in versions if v.startswith("8.6.")]
    if lib == "openssl":
        return [v for v in versions if v.startswith("3.")]
    return list(versions)


def tag_versions(tags):
    out = []
    for t in tags:
        m = re.fullmatch(r"(?:v|openssl-)?(\d+(?:\.\d+)+)", t)
        if m:
            out.append(m.group(1))
    return out


def sqlite_latest(page):
    m = re.search(r"PRODUCT,(\d+\.\d+\.\d+),(\d{4}/sqlite-autoconf-\d+\.tar\.gz)", page)
    return (m.group(1), "https://sqlite.org/" + m.group(2)) if m else None


def cacert_latest(page):
    found = re.findall(r"cacert-(\d{4}-\d{2}-\d{2})\.pem", page)
    return max(found) if found else None


def get(url, binary=False):
    req = urllib.request.Request(url, headers={"User-Agent": "rpyenv-deps-catalog"})
    token = os.environ.get("GH_TOKEN")
    if token and url.startswith("https://api.github.com/"):
        req.add_header("Authorization", "Bearer " + token)
    with urllib.request.urlopen(req, timeout=60) as r:
        data = r.read()
    return data if binary else data.decode("utf-8", "replace")


def latest(lib):
    """(version, url) of the newest allowed upstream release of `lib`."""
    s = SOURCES[lib]
    kind = s[0]
    if kind == "listing":
        _, url, pattern, template = s
        v = newest(allowed(lib, matches(get(url), pattern)))
        return v, template.format(v=v)
    if kind == "github":
        _, repo, tag, name = s
        rel = json.loads(get(f"https://api.github.com/repos/{repo}/releases?per_page=50"))
        tags = [r["tag_name"] for r in rel if not r["prerelease"] and not r["draft"]]
        v = newest(allowed(lib, tag_versions(tags)))
        return v, GH.format(repo=repo, tag=tag.format(v=v), name=name.format(v=v))
    if kind == "sqlite":
        return sqlite_latest(get("https://sqlite.org/download.html"))
    if kind == "kernel":
        base = "https://mirrors.edge.kernel.org/pub/linux/utils/util-linux/"
        series = newest(matches(get(base), r"v(\d+\.\d+)/"))
        v = newest(matches(get(f"{base}v{series}/"), r"util-linux-(\d+\.\d+(?:\.\d+)?)\.tar\.xz"))
        return v, f"{base}v{series}/util-linux-{v}.tar.xz"
    if kind == "cacert":
        d = cacert_latest(get("https://curl.se/docs/caextract.html"))
        return d, f"https://curl.se/ca/cacert-{d}.pem"
    raise ValueError(kind)


def read_rows(text):
    return [l.split("\t") for l in text.splitlines() if l.strip() and not l.startswith("#")]


def replace_row(text, lib, version, url, sha):
    out = []
    for l in text.splitlines(keepends=True):
        if l.split("\t")[0] == lib and not l.startswith("#"):
            l = f"{lib}\t{version}\t{url}\t{sha}\n"
        out.append(l)
    return "".join(out)


def sha256_of(url):
    data = get(url, binary=True)
    return hashlib.sha256(data).hexdigest(), data


def main(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--catalog", required=True)
    p.add_argument("--write", action="store_true")
    p.add_argument("--report")
    a = p.parse_args(argv)
    with open(a.catalog, encoding="utf-8") as f:
        text = f.read()
    changes, failed = [], []
    for lib, version, _url, _sha in read_rows(text):
        try:
            v, url = latest(lib)
        except Exception as e:  # noqa: BLE001 - every source failure is reported the same way
            failed.append(f"{lib}: {e}")
            continue
        if v is None:
            failed.append(f"{lib}: no release found")
        elif key(v) > key(version):
            changes.append((lib, version, v, url))
    for line in failed:
        print("cannot read", line, file=sys.stderr)
    if failed:
        return 1
    lines = []
    for lib, old, new, url in changes:
        sha, data = sha256_of(url)
        if lib == "cacert":
            published = get(url + ".sha256").split()[0]
            if published != sha:
                print(f"cacert {new}: download {sha} != published {published}", file=sys.stderr)
                return 1
        text = replace_row(text, lib, new, url, sha)
        lines.append(f"- `{lib}` {old} → {new} (`{sha}`)")
        print(lines[-1])
    if a.report:
        with open(a.report, "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + ("\n" if lines else ""))
    if not changes:
        print("the build catalog is up to date")
    elif a.write:
        with open(a.catalog, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `python -m unittest discover -s ci -p "test_deps_catalog.py" -v`
Expected: 8 tests OK.

- [ ] **Step 5: Run it once against the real sources** (read-only; this downloads nothing
  unless a row is behind):

Run: `python ci/deps_catalog.py --catalog crates/pyenv/deps/catalog.tsv`
Expected: `the build catalog is up to date`, or `- \`<lib>\` <old> → <new> (…)` lines. If a
source can't be read from the dev host (a certificate problem on Windows Python, for example),
rerun it in WSL and note which in the report. Do not commit a changed catalog from this run.

- [ ] **Step 6: Wire it into the weekly sync.** In `.github/workflows/sync-python-build.yml`:
  1. After the "Rebuild the conda table" step, add:

```yaml
      # M8 design §6.1: the build catalog follows upstream library releases. Like the tables
      # above, a failure leaves crates/pyenv/deps as it was and fails the run at the end.
      - name: Check the build catalog
        id: catalog
        continue-on-error: true
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          set -o pipefail
          if ! python3 ci/deps_catalog.py --catalog crates/pyenv/deps/catalog.tsv --write --report "$RUNNER_TEMP/catalog.md" 2>&1 | tee "$RUNNER_TEMP/catalog.log"; then
            git checkout -- crates/pyenv/deps
            exit 1
          fi
```

  2. "Anything to propose?": add `crates/pyenv/deps` to the `git status --porcelain` path list,
     and change its "up to date" echo to `"up to date with $TAG, pypy.org, the conda definitions and the build catalog's sources"`.
  3. "Build, regenerate the goldens…": add `CATALOG: ${{ steps.catalog.outcome }}` to `env:`
     and, after the conda `table` line:
     `table "build catalog (ci/deps_catalog.py)" "$CATALOG" crates/pyenv/deps/ "$RUNNER_TEMP/catalog.log"`.
  4. "Open or update the pull request": add `CATALOG: ${{ steps.catalog.outcome }}` to `env:`;
     add `crates/pyenv/deps` to the `git add` line; the non-python-build title becomes
     `"Update the PyPy, conda and build-catalog tables"`; and before `echo "Checks run by the workflow:"` add:

```bash
            if [ -s "$RUNNER_TEMP/catalog.md" ]; then
              echo
              echo "**Build catalog updates** (crates/pyenv/deps/catalog.tsv; each hash was computed from a fresh download, review it before merging):"
              echo
              cat "$RUNNER_TEMP/catalog.md"
            fi
```

  5. "Fail when a table couldn't be rebuilt": add `|| steps.catalog.outcome == 'failure'` to
     its `if:`, `CATALOG: ${{ steps.catalog.outcome }}` to `env:`, and
     `[ "$CATALOG" = "success" ] || echo "::error::ci/deps_catalog.py failed; crates/pyenv/deps was left unchanged. The step 'Check the build catalog' has the log."`
  6. Add one line to the workflow's header comment list:
     `#   - moves the build catalog (crates/pyenv/deps/catalog.tsv) to new upstream library releases (ci/deps_catalog.py);`

- [ ] **Step 7: Validate the YAML**

Run: `python -c "import yaml; yaml.safe_load(open(r'.github/workflows/sync-python-build.yml', encoding='utf-8'))"`
Expected: exit 0.

- [ ] **Step 8: Commit**

```bash
git add ci/deps_catalog.py ci/test_deps_catalog.py .github/workflows/sync-python-build.yml
git commit -m "M8b: the weekly sync moves the build catalog to new upstream library releases"
```

---

### Task 8: Docs, allowlist, spec

**Files:**
- Modify: `docs/distributions.md`
- Modify: `docs/parity/allowlist.md` (D-123 to D-127)
- Modify: `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md`
- Modify: `README.md` (roadmap item 8)

**Interfaces:**
- Consumes: the behavior of Tasks 1–7; the test names that cite D-123 to D-127.
- Produces: user documentation; allowlist rows the coverage check (`python parity/coverage.py`) finds cited.

- [ ] **Step 1: `docs/distributions.md`.** In "The prompt" table, after the `i` row:

```markdown
| `b` | Builds the missing libraries from source, without root, and links them into Python. Offered for CPython 3.7 and later when the compiler, `make` and `patch` are there; it says what it can't build here and why |
```

  In the `--deps` table, the `build` row becomes:

```markdown
| `build` | Builds the missing libraries from source, then Python. Stops before building anything when one of them can't be built here |
```

  In "Scripts that set neither", the last sentence becomes: "A later rpyenv release will make
  that summary stop the build. To choose now, use `--deps=install`, `--deps=build` or
  `--deps=system`." Then add a new subsection after "After the build":

```markdown
### Building libraries without root

`pyenv install --deps=build`, or `b` at the prompt, compiles each missing library from source
and links it into Python statically. Nothing is installed on the system.

- **Where:** `$PYENV_ROOT/cache/deps/<library>-<version>-<hash>/`. Another Python version that
  needs the same library, version and options reuses it.
- **Only what is missing** is built. Libraries the system has keep getting its updates.
- **It is a cache.** Deleting `cache/deps` breaks no installed Python; the next build compiles
  again. (The CA bundle below lives elsewhere, in `$PYENV_ROOT/.rpyenv/ca/`; keep that one.)
- **Versions:** rpyenv pins each library's version and SHA-256 and checks every download. A
  Python version whose definition pins OpenSSL or readline gets that version.
- **What the system still needs:** a C compiler, `make` and `patch`. OpenSSL and libxcrypt also
  need Perl with its core modules (`perl-core` on Fedora); Tk needs the X11 headers
  (`libx11-dev`, `libX11-devel`) until a later release builds those too.
- **`PYENV_ROOT`** must have no spaces or shell characters in its path.
- **CA certificates:** a self-built OpenSSL reads the system's (`/etc/pki/tls` or `/etc/ssl`).
  When the system has none, it reads `$PYENV_ROOT/.rpyenv/ca/cert.pem`, a bundle rpyenv pins (curl's
  copy of Mozilla's list). `pyenv install` replaces that file when an rpyenv upgrade pins a
  newer one.
- **Updates:** each version built this way lists its libraries in `versions/<v>/.rpyenv-deps`.
  When an rpyenv upgrade pins a newer version of one of them, `pyenv install` prints one line
  per affected Python version, with the command that rebuilds it.
```

- [ ] **Step 2: `docs/parity/allowlist.md`.** Append five rows after D-122 (one line each,
  the file's column order: ID, OS, Command, Upstream, rpyenv, Reason):

```markdown
| D-123 | Linux | `install --deps=build`, `RPYENV_BUILD_DEPS=build` (CPython 3.7+) | No such option; missing libraries are installed by hand with root | Compiles each missing library from source, without root, into `$PYENV_ROOT/cache/deps`, links it into Python statically, and records it in `versions/<v>/.rpyenv-deps`. Refuses before building anything when a library can't be built here (no X11 headers for Tk, no Perl modules for OpenSSL) or when `PYENV_ROOT` has a space | Users without root can get every standard-library module (M8 design §6). |
| D-124 | Linux | `install` at a terminal (CPython 3.7+) | No prompt | The prompt offers `[b]` to build the missing libraries without root, naming what it can't build here and why; after it, whatever is left returns to the prompt | Same as D-123, chosen interactively (M8 design §5.1). |
| D-125 | Linux | `install` (CPython 3), a refused build | No such line | The refusal adds `Without root, pyenv install --deps=build compiles them from source.` unless the compiler, `make` or `patch` is missing | Tells users without root the way out. |
| D-126 | Linux | `install` | No such warning | Prints one line per installed version that the build route built with a library rpyenv now pins at another version, with the `pyenv install -f` command that rebuilds it | Statically linked libraries need a rebuild for a security fix (M8 design §6.3). |
| D-127 | Linux | `install` | No such file | Replaces `$PYENV_ROOT/.rpyenv/ca/cert.pem` when it exists and rpyenv pins a newer bundle, and says so | Versions whose OpenSSL reads the bundle get the new certificates at once (M8 design §6.5). |
```

  Run `python parity/coverage.py` and expect it to report every row covered.

- [ ] **Step 3: The spec.** In `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md`:
  - D13's row: replace "**M8b** makes the script default strict (`system`), because from then
    on a script without root can answer with `--deps=build`." with "**M8c** makes the script
    default strict (`system`), because from then on a script without root can always answer
    with `--deps=build` (moved from M8b by the user on 2026-10-17: M8b can't build X11, so a
    script without root on a machine without X11 headers could not pass)."
  - §3's table: the M8b row's "Delivers" gains "; scripts stay lenient (D13)"; the M8c row's
    gains "; scripts become strict (D13)".
  - §5.2: the summary's last two lines become the text Task 4 prints
    (`choose now with --deps=install,` / `--deps=build or --deps=system (or RPYENV_BUILD_DEPS).`),
    and "until M8b" becomes "until M8c" in the paragraph above it.
  - §6.2, after "Libraries build in dependency order (Tcl before Tk, ncurses before readline).",
    add: "Libraries that need each other are built together and their archives merged, so each
    `-l` stands alone: readline with ncurses, Tk with Tcl, libnsl with libtirpc, and
    `libgdbm_compat` with `libgdbm` (measured in M8b: CPython's extension modules don't get
    `LIBS` on their link line). `Libs.private` is folded into `Libs` in the store's `.pc` files."
  - §6.3, after "§4.4's `info library` check confirms it.", add: "Tcl and Tk are built with a
    compiled-in library path that never exists, so they find the copy in the version and never
    the store's (measured in M8b)."
  - §6.5: replace "in `<root>/cache/deps/ca/`" with "in `<root>/.rpyenv/ca/`, outside the
    cache, so deleting `cache/deps` never breaks HTTPS (user decision 2026-10-17)".
  - §6.5, add a last sentence: "The build route needs CPython 3.7 or later, a `PYENV_ROOT`
    without spaces, Perl's core modules for OpenSSL and libxcrypt, and (until M8c) the system's
    X11 headers for Tk."

- [ ] **Step 4: README.** Roadmap item 8 becomes:

```markdown
8. **M8 Missing Linux build dependencies.** Install them through the
   package manager (M8a, done), or compile them without root access (M8b, next release).
```

- [ ] **Step 5: Control-character check** (this repo has had stray `\b` backspaces from
  escaped strings three times):

Run: `grep -rnP "[\x00-\x08\x0b\x0c\x0e-\x1f]" docs/distributions.md docs/parity/allowlist.md docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md README.md crates/pyenv/src/install/catalog.rs crates/pyenv/src/install/store.rs crates/pyenv/src/install/record.rs`
Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add docs/distributions.md docs/parity/allowlist.md docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md README.md
git commit -m "M8b: docs, allowlist D-123 to D-127, and the spec's D13 moved to M8c"
```

---

## Running the tests

The `pyenv` crate's Linux code only builds on Linux. From the Windows dev host, run tests in
WSL as a login shell with a script file (never `$VARS` inside `wsl … bash -c "…"`):

```bash
#!/usr/bin/bash
# C:\tmp\m8b_test.sh, run with: wsl -d Debian --exec /usr/bin/bash -l /mnt/c/tmp/m8b_test.sh
cd /mnt/c/Users/JM/repos/OWN/rpyenv || exit 1
export CARGO_TARGET_DIR=/home/jm/rpyenv-m8-target
cargo fmt --all -- --check; echo "fmt-exit=$?"
cargo clippy -q --workspace --all-targets -- -D warnings; echo "clippy-exit=$?"
cargo test -q -p pyenv --lib -- catalog:: store:: record:: deps:: preflight:: builder:: 2>&1 | grep -E "test result|FAILED|panicked"
cargo test -q -p pyenv --test cli_install_deps 2>&1 | grep -E "test result|FAILED|panicked"
cargo test -q --workspace > /tmp/m8b_test.log 2>&1; echo "test-exit=$?"
grep -E "^test .*FAILED|panicked" /tmp/m8b_test.log | head -5
```

Windows must stay green too: `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` on the Windows host (the new modules are `#[cfg(unix)]`).

Safety, for every task: tests run with `PYENV_ROOT`, `PYENV`, `PYENV_HOME` and `HOME` pointing
at scratch folders (the fixtures do this); nothing installs packages, runs `sudo`, or builds a
real library on the dev host. Real builds happen only in throwaway containers (Task 6).
