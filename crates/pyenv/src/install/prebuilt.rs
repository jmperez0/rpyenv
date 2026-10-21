//! The prebuilt route (M8 design §7, D15–D19): the table of python-build-standalone builds that
//! ci/prebuilt_table.py writes (`crates/pyenv/prebuilt/linux.tsv`), the one-line definition a row
//! becomes, the text that names a build, and the rewrite of its compiler settings (D18).

use std::path::Path;

static EMBEDDED: &str = include_str!("../../prebuilt/linux.tsv");
pub const BASE: &str = "https://github.com/astral-sh/python-build-standalone/releases/download";
/// Releases before this one have no GitHub attestation (D16); the table never holds them.
const FIRST_ATTESTED: &str = "20250311";
/// §4.4's imports a prebuilt build never has (D17): gdbm is GPL, nis is not built.
pub const SKIPPED_IMPORTS: [&str; 2] = ["dbm.gnu", "nis"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub arch: String,
    pub release: String,
    pub sha256: String,
    pub openssl: String,
    pub tk: String,
}

fn is_version_name(s: &str) -> bool {
    let core = s.strip_suffix('t').unwrap_or(s);
    let mut parts = core.splitn(3, '.');
    let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let patch_end = c.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(c.len());
    let (num, pre) = c.split_at(patch_end);
    let pre_ok = pre.is_empty()
        || ["a", "b", "rc"]
            .iter()
            .any(|p| pre.strip_prefix(p).is_some_and(digits));
    digits(a) && digits(b) && digits(num) && pre_ok
}

/// A release tag: 8 ASCII digits (`YYYYMMDD`).
pub fn is_release(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())
}

/// One of the two arches the table holds.
pub fn is_arch(s: &str) -> bool {
    matches!(s, "x86_64" | "aarch64")
}

/// An OpenSSL version word: letters, digits and dots.
pub fn is_openssl(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
}

pub fn parse(text: &str) -> Result<Vec<Row>, String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let bad = || format!("unreadable prebuilt table line: {l}");
            let [name, arch, release, sha, openssl, tk] = f.as_slice() else {
                return Err(bad());
            };
            let ok = is_version_name(name)
                && is_arch(arch)
                && is_release(release)
                && *release >= FIRST_ATTESTED
                && sha.len() == 64
                && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                && is_openssl(openssl)
                && matches!(*tk, "8.6" | "9.0");
            if !ok {
                return Err(bad());
            }
            Ok(Row {
                name: name.to_string(),
                arch: arch.to_string(),
                release: release.to_string(),
                sha256: sha.to_string(),
                openssl: openssl.to_string(),
                tk: tk.to_string(),
            })
        })
        .collect()
}

/// Debug builds read `RPYENV_TEST_PREBUILT_TABLE` (a file) when it is set; release builds always
/// use the embedded table.
pub fn table() -> Result<Vec<Row>, String> {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("RPYENV_TEST_PREBUILT_TABLE").filter(|v| !v.is_empty()) {
        let text =
            std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", Path::new(&p).display()))?;
        return parse(&text);
    }
    parse(EMBEDDED)
}

fn arch_of(rust_arch: &str) -> Option<&'static str> {
    match rust_arch {
        "x86_64" => Some("x86_64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

/// The table's arch for the machine rpyenv was built for.
pub fn host_arch() -> Option<&'static str> {
    arch_of(std::env::consts::ARCH)
}

pub fn find<'a>(rows: &'a [Row], name: &str, arch: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.name == name && r.arch == arch)
}

/// The release folder base. Debug builds honor `RPYENV_TEST_PREBUILT_BASE`; with
/// `RPYENV_TEST_PYTHON_ORG` set and it unset, a closed local port, so a test never reaches the
/// network (as `pypy::urls`). Release builds ignore both.
pub fn base() -> String {
    #[cfg(debug_assertions)]
    {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(b) = var("RPYENV_TEST_PREBUILT_BASE") {
            return b.trim_end_matches('/').to_string();
        }
        if var("RPYENV_TEST_PYTHON_ORG").is_some() {
            return "http://127.0.0.1:9/pbs".to_string();
        }
    }
    BASE.to_string()
}

fn asset(row: &Row) -> String {
    let (ver, ft) = match row.name.strip_suffix('t') {
        Some(v) => (v, "freethreaded-"),
        None => (row.name.as_str(), ""),
    };
    format!(
        "cpython-{ver}%2B{}-{}-unknown-linux-gnu-{ft}install_only.tar.gz",
        row.release, row.arch
    )
}

pub fn url(row: &Row) -> String {
    format!("{}/{}/{}", base(), row.release, asset(row))
}

/// The package name: the download's file name in the cache and the extracted folder's name.
pub fn package(row: &Row) -> String {
    format!("cpython-{}-{}-{}", row.name, row.release, row.arch)
}

/// The one-line definition the interpreter runs; `verify_step` is the built-in definition's own.
pub fn definition_text(row: &Row, verify_step: &str) -> String {
    format!(
        "install_package \"{}\" \"{}#{}\" prebuilt {verify_step} ensurepip\n",
        package(row),
        url(row),
        row.sha256
    )
}

/// `YYYY-MM-DD` of a release tag; anything that is not 8 ASCII digits comes back unchanged.
pub fn date(release: &str) -> String {
    if is_release(release) {
        format!("{}-{}-{}", &release[..4], &release[4..6], &release[6..])
    } else {
        release.to_string()
    }
}

/// The order of a version name, and whether it is free-threaded: `[X, Y, Z, stage, N]` with
/// stage 0, 1, 2 for `aN`, `bN`, `rcN` and 3 (N 0) for a final release.
fn version_key(name: &str) -> Option<([u64; 5], bool)> {
    if !is_version_name(name) {
        return None;
    }
    let (core, ft) = match name.strip_suffix('t') {
        Some(c) => (c, true),
        None => (name, false),
    };
    let mut parts = core.splitn(3, '.');
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let c = parts.next()?;
    let end = c.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(c.len());
    let (z, pre) = c.split_at(end);
    let (stage, n) = if pre.is_empty() {
        (3, 0)
    } else {
        let (stage, rest) = [(0, "a"), (1, "b"), (2, "rc")]
            .iter()
            .find_map(|(st, p)| pre.strip_prefix(p).map(|r| (*st, r)))?;
        (stage, rest.parse().ok()?)
    };
    Some(([x, y, z.parse().ok()?, stage, n], ft))
}

/// Final review B1: python-build-standalone rebuilds only the newest patch of each minor, so an
/// older patch never gets a newer build. The table's highest version above `definition` with
/// the same `X.Y` and variant (`t` or not) for `arch`, compared numerically; final releases
/// only, unless `definition` is itself a prerelease (then later prereleases count too).
pub fn newer_patch<'a>(rows: &'a [Row], definition: &str, arch: &str) -> Option<&'a Row> {
    let (have, ft) = version_key(definition)?;
    let pre_ok = have[3] < 3;
    rows.iter()
        .filter(|r| r.arch == arch)
        .filter_map(|r| version_key(&r.name).map(|(k, t)| (k, t, r)))
        .filter(|(k, t, _)| *t == ft && k[..2] == have[..2] && *k > have && (pre_ok || k[3] == 3))
        .max_by_key(|(k, _, _)| *k)
        .map(|(_, _, r)| r)
}

pub fn summary(row: &Row) -> String {
    format!(
        "python-build-standalone {} (third-party build; OpenSSL {}, Tk {}; no dbm.gnu or nis)",
        date(&row.release),
        row.openssl,
        row.tk
    )
}

pub fn tk9_line(row: &Row) -> Option<&'static str> {
    row.tk.starts_with('9').then_some(
        "pyenv: this build has Tk 9, under which some older Tk programs behave differently",
    )
}

/// `/install` at the start of a path: after a quote, a blank, `=`, `:`, `,`, `(`, `;`, or
/// `-I`/`-L`, and followed by `/`, a quote, a blank or the end.
fn install_at(b: &[u8], i: usize) -> bool {
    const P: &[u8] = b"/install";
    if !b[i..].starts_with(P) {
        return false;
    }
    let before_ok = i == 0
        || matches!(
            b[i - 1],
            b'\'' | b'"' | b' ' | b'\t' | b'\n' | b'=' | b':' | b',' | b'(' | b';'
        )
        || (i >= 2 && b[i - 2] == b'-' && matches!(b[i - 1], b'I' | b'L'));
    let after = b.get(i + P.len()).copied();
    let after_ok = matches!(
        after,
        None | Some(b'/' | b'\'' | b'"' | b' ' | b'\t' | b'\n' | b':')
    );
    before_ok && after_ok
}

/// The whole word `w` at `i`: not preceded or followed by a letter, digit, `_`, `-`, `.` or `/`.
fn word_at(b: &[u8], i: usize, w: &[u8]) -> bool {
    let part = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'/' | b'+');
    b[i..].starts_with(w)
        && (i == 0 || !part(b[i - 1]))
        && b.get(i + w.len()).is_none_or(|c| !part(*c))
}

/// D18: `clang++` → `c++`, `clang` → `cc` (whole words) and the path `/install` → `prefix`.
pub fn rewrite_text(text: &str, prefix: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut start = 0;
    while i < b.len() {
        let (len, with): (usize, &str) = if install_at(b, i) {
            (8, prefix)
        } else if word_at(b, i, b"clang++") {
            (7, "c++")
        } else if word_at(b, i, b"clang") {
            (5, "cc")
        } else {
            i += 1;
            continue;
        };
        out.push_str(&text[start..i]);
        out.push_str(with);
        i += len;
        start = i;
    }
    out.push_str(&text[start..]);
    out
}

/// D18 over `tree` (the build before it is placed at `prefix`): every
/// `lib/python3.*/_sysconfigdata_*.py`, `lib/python3.*/config-3.*/Makefile` and
/// `bin/python3.*-config`. Returns how many files changed; a tree without a sysconfig data file
/// is an error (the table promises python-build-standalone's layout).
pub fn rewrite_build_settings(tree: &Path, prefix: &Path) -> Result<usize, String> {
    let prefix = prefix.to_string_lossy();
    let mut files = Vec::new();
    let mut sysconfig = 0;
    let lib = tree.join("lib");
    for e in std::fs::read_dir(&lib).map_err(|e| format!("{}: {e}", lib.display()))? {
        let d = e.map_err(|e| e.to_string())?.path();
        let n = d
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !n.starts_with("python3.") || !d.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = f.map_err(|e| e.to_string())?.path();
            let fname = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if fname.starts_with("_sysconfigdata_") && fname.ends_with(".py") {
                sysconfig += 1;
                files.push(p);
            } else if fname.starts_with("config-3.") && p.is_dir() && p.join("Makefile").is_file() {
                files.push(p.join("Makefile"));
            }
        }
    }
    if sysconfig == 0 {
        return Err(format!(
            "no _sysconfigdata_*.py under {}/python3.*: not a python-build-standalone layout",
            lib.display()
        ));
    }
    if let Ok(rd) = std::fs::read_dir(tree.join("bin")) {
        for e in rd.filter_map(Result::ok) {
            let n = e.file_name().to_string_lossy().into_owned();
            let is_file = e.file_type().is_ok_and(|t| t.is_file());
            if is_file && n.starts_with("python3.") && n.ends_with("-config") {
                files.push(e.path());
            }
        }
    }
    let mut changed = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let new = rewrite_text(&text, &prefix);
        if new != text {
            std::fs::write(&f, new).map_err(|e| format!("{}: {e}", f.display()))?;
            changed += 1;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0ad293ecda8ea93a28e4a6b5e9cc20ae74be15aafed75be04418b8f82bd0d0da";

    fn row(name: &str, arch: &str, tk: &str) -> Row {
        Row {
            name: name.into(),
            arch: arch.into(),
            release: "20261020".into(),
            sha256: SHA.into(),
            openssl: "3.5.9".into(),
            tk: tk.into(),
        }
    }

    #[test]
    fn the_table_parses_and_rejects_bad_rows() {
        let good = format!("# c\n3.13.16\tx86_64\t20261020\t{SHA}\t3.5.9\t9.0\n");
        assert_eq!(parse(&good).unwrap(), vec![row("3.13.16", "x86_64", "9.0")]);
        for bad in [
            format!("3.13.16\tx86_64\t20261020\t{SHA}\t3.5.9\n"),
            format!("3.13.16\tarm\t20261020\t{SHA}\t3.5.9\t9.0\n"),
            format!("3.13.16\tx86_64\t2026102\t{SHA}\t3.5.9\t9.0\n"),
            format!("3.13.16\tx86_64\t20261020\t{}\t3.5.9\t9.0\n", &SHA[1..]),
            format!("3.13.16\tx86_64\t20250205\t{SHA}\t3.5.9\t9.0\n"),
            format!("../x\tx86_64\t20261020\t{SHA}\t3.5.9\t9.0\n"),
            format!("3.13.16\tx86_64\t20261020\t{SHA}\t3.5.9\t9\n"),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_embedded_table_parses_and_has_rows_for_both_arches() {
        let rows = parse(EMBEDDED).unwrap();
        assert!(rows.iter().any(|r| r.arch == "x86_64"));
        assert!(rows.iter().any(|r| r.arch == "aarch64"));
    }

    #[test]
    fn urls_packages_and_definitions() {
        let r = row("3.13.16t", "aarch64", "9.0");
        assert_eq!(
            url(&r),
            "https://github.com/astral-sh/python-build-standalone/releases/download/20261020/cpython-3.13.16%2B20261020-aarch64-unknown-linux-gnu-freethreaded-install_only.tar.gz"
        );
        assert_eq!(package(&r), "cpython-3.13.16t-20261020-aarch64");
        assert!(crate::install::is_plain_name(&package(&r)));
        assert_eq!(
            definition_text(&r, "verify_py313"),
            format!("install_package \"cpython-3.13.16t-20261020-aarch64\" \"{}#{SHA}\" prebuilt verify_py313 ensurepip\n", url(&r))
        );
    }

    #[test]
    fn find_matches_name_and_arch() {
        let rows = vec![
            row("3.13.16", "x86_64", "9.0"),
            row("3.13.16", "aarch64", "9.0"),
        ];
        assert_eq!(find(&rows, "3.13.16", "aarch64").unwrap().arch, "aarch64");
        assert!(find(&rows, "3.13.15", "x86_64").is_none());
        assert!(find(&rows, "3.13.16t", "x86_64").is_none());
    }

    #[test]
    fn host_arch_maps_only_the_two_built_arches() {
        assert_eq!(arch_of("x86_64"), Some("x86_64"));
        assert_eq!(arch_of("aarch64"), Some("aarch64"));
        assert_eq!(arch_of("riscv64"), None);
        assert_eq!(arch_of("x86"), None);
    }

    #[test]
    fn the_summary_names_date_openssl_tk_and_what_is_missing() {
        let r = row("3.13.16", "x86_64", "9.0");
        assert_eq!(
            summary(&r),
            "python-build-standalone 2026-10-20 (third-party build; OpenSSL 3.5.9, Tk 9.0; no dbm.gnu or nis)"
        );
        assert!(tk9_line(&r).is_some());
        assert!(tk9_line(&row("3.9.25", "x86_64", "8.6")).is_none());
    }

    #[test]
    fn rewrite_replaces_clang_and_the_install_prefix_only_as_whole_words() {
        let t = "build_time_vars = {'CC': 'clang -pthread',\n 'CXX': 'clang++ -pthread',\n 'LDSHARED': 'clang -pthread -shared -L/install/lib',\n 'INCLUDEPY': '/install/include/python3.13',\n 'prefix': '/install',\n 'OTHER': '/usr/install/x clangd -DX=clang_x /installer',\n}\n";
        let out = rewrite_text(t, "/r/versions/3.13.16");
        assert!(out.contains("'CC': 'cc -pthread'"), "{out}");
        assert!(out.contains("'CXX': 'c++ -pthread'"), "{out}");
        assert!(out.contains("-L/r/versions/3.13.16/lib"), "{out}");
        assert!(
            out.contains("'INCLUDEPY': '/r/versions/3.13.16/include/python3.13'"),
            "{out}"
        );
        assert!(out.contains("'prefix': '/r/versions/3.13.16'"), "{out}");
        assert!(
            out.contains("'/usr/install/x clangd -DX=clang_x /installer'"),
            "{out}"
        );
        let mk = "CC=\t\tclang -pthread\nprefix=\t\t/install\nLIBPL=$(prefix)/lib\n";
        let out = rewrite_text(mk, "/r/v");
        assert_eq!(
            out,
            "CC=\t\tcc -pthread\nprefix=\t\t/r/v\nLIBPL=$(prefix)/lib\n"
        );
    }

    /// Final review A1: a date that is not 8 ASCII digits comes back unchanged (no panic on a
    /// multi-byte letter).
    #[test]
    fn date_formats_only_eight_ascii_digits() {
        assert_eq!(date("20261020"), "2026-10-20");
        for odd in ["123é567", "2026102", "202610201", "2026102x", ""] {
            assert_eq!(date(odd), odd);
        }
    }

    /// Final review C3: `/install` after `,`, `(` or `;` is the build prefix too.
    #[test]
    fn rewrite_replaces_install_after_a_comma_a_parenthesis_or_a_semicolon() {
        for (t, want) in [
            ("-Wl,-rpath,/install/lib", "-Wl,-rpath,/r/v/lib"),
            ("$(/install/bin/x)", "$(/r/v/bin/x)"),
            ("a;/install/lib", "a;/r/v/lib"),
            ("'X': '-Wl,-rpath,/install'", "'X': '-Wl,-rpath,/r/v'"),
        ] {
            assert_eq!(rewrite_text(t, "/r/v"), want, "{t}");
        }
        for same in [
            "INSTALL=/usr/bin/install -c",
            "./install",
            "$(srcdir)/install-sh",
            "/opt/install",
        ] {
            assert_eq!(rewrite_text(same, "/r/v"), same);
        }
    }

    /// Final review B1: the newest later patch of the same minor and variant, for one arch.
    #[test]
    fn newer_patch_finds_a_later_patch_of_the_same_minor_and_variant() {
        let mut rows = vec![
            row("3.12.10", "x86_64", "8.6"),
            row("3.12.12", "x86_64", "9.0"),
            row("3.12.13", "x86_64", "9.0"),
            row("3.12.14", "aarch64", "9.0"),
            row("3.13.0", "x86_64", "9.0"),
            row("3.12.13t", "x86_64", "9.0"),
            row("3.12.14rc1", "x86_64", "9.0"),
            row("3.16.0a1", "x86_64", "9.0"),
            row("3.16.0a2", "x86_64", "9.0"),
            row("3.16.0b1t", "x86_64", "9.0"),
        ];
        let name = |r: Option<&Row>| r.map(|r| r.name.clone());
        // Numeric order (3.12.13 > 3.12.9), not text order; the other arch's 3.12.14 and
        // the prerelease 3.12.14rc1 don't count; 3.13.0 is another minor.
        assert_eq!(
            name(newer_patch(&rows, "3.12.10", "x86_64")),
            Some("3.12.13".into())
        );
        assert_eq!(
            name(newer_patch(&rows, "3.12.9", "x86_64")),
            Some("3.12.13".into())
        );
        // The newest patch itself has none.
        assert_eq!(name(newer_patch(&rows, "3.12.13", "x86_64")), None);
        // `t` stays `t`.
        assert_eq!(
            name(newer_patch(&rows, "3.12.10t", "x86_64")),
            Some("3.12.13t".into())
        );
        assert_eq!(name(newer_patch(&rows, "3.12.13t", "x86_64")), None);
        // Another arch: only its own rows.
        assert_eq!(
            name(newer_patch(&rows, "3.12.10", "aarch64")),
            Some("3.12.14".into())
        );
        // An installed prerelease: later prereleases and finals count.
        assert_eq!(
            name(newer_patch(&rows, "3.16.0a1", "x86_64")),
            Some("3.16.0a2".into())
        );
        assert_eq!(
            name(newer_patch(&rows, "3.16.0a1t", "x86_64")),
            Some("3.16.0b1t".into())
        );
        rows.push(row("3.16.0", "x86_64", "9.0"));
        assert_eq!(
            name(newer_patch(&rows, "3.16.0a2", "x86_64")),
            Some("3.16.0".into())
        );
        // A name that is not a version has none.
        assert_eq!(name(newer_patch(&rows, "mine", "x86_64")), None);
    }

    #[test]
    fn rewrite_build_settings_edits_the_three_files_and_needs_sysconfigdata() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path();
        std::fs::create_dir_all(t.join("lib/python3.13t/config-3.13t-x86_64-linux-gnu")).unwrap();
        std::fs::create_dir_all(t.join("bin")).unwrap();
        std::fs::write(
            t.join("lib/python3.13t/_sysconfigdata_t_linux_x86_64-linux-gnu.py"),
            "{'CC': 'clang', 'p': '/install'}\n",
        )
        .unwrap();
        std::fs::write(
            t.join("lib/python3.13t/config-3.13t-x86_64-linux-gnu/Makefile"),
            "CC= clang\nprefix= /install\n",
        )
        .unwrap();
        std::fs::write(
            t.join("bin/python3.13t-config"),
            "prefix_build=\"/install\"\n",
        )
        .unwrap();
        std::fs::write(t.join("bin/python3.13t"), "ELF").unwrap();
        assert_eq!(rewrite_build_settings(t, Path::new("/r/v")).unwrap(), 3);
        for f in [
            "lib/python3.13t/_sysconfigdata_t_linux_x86_64-linux-gnu.py",
            "lib/python3.13t/config-3.13t-x86_64-linux-gnu/Makefile",
            "bin/python3.13t-config",
        ] {
            let s = std::fs::read_to_string(t.join(f)).unwrap();
            assert!(!s.contains("/install") && !s.contains("clang"), "{f}: {s}");
        }
        let e = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(e.path().join("lib/python3.13")).unwrap();
        let err = rewrite_build_settings(e.path(), Path::new("/r/v")).unwrap_err();
        assert!(err.contains("_sysconfigdata"), "{err}");
    }
}
