//! `pyenv install` of a python-build-standalone build on Linux (M8 design §7) against a fake
//! tarball on a local server.
#![cfg(unix)]

mod common;

use common::fakepbs::tarball_gz;
use common::fakepypy::sha256;
use common::server::{start, Reply, Server};
use common::Fixture;

const PKG: &str = "cpython-3.13.99-20261020-x86_64";

/// The environment the CLI tests run with: the fixture's `sys/bin` first, then the real PATH,
/// a scratch TMPDIR, and no mirror. The pre-flight check is not skipped.
fn env<'a>(f: &'a Fixture, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
    let mut v: Vec<(&str, String)> = vec![
        (
            "PATH",
            format!("{}:{}", f.syspath.display(), std::env::var("PATH").unwrap()),
        ),
        ("TMPDIR", f.base.join("tmp").display().to_string()),
        ("PYTHON_BUILD_SKIP_MIRROR", "1".into()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

fn run(f: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> common::Run {
    let owned = env(f, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_env(args, &pairs)
}

fn left_in_versions(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn serve(body: Vec<u8>) -> (Server, String) {
    let sha = sha256(&body);
    (start(vec![("/pbs.tar.gz", vec![Reply::Body(body)])]), sha)
}

/// `<base>/defs/3.13.99`: the one-line definition the route makes.
fn def_file(f: &Fixture, s: &Server, sha: &str) -> String {
    let p = f.base.join("defs").join("3.13.99");
    f.file(
        &p,
        &format!(
            "install_package \"{PKG}\" \"{}#{sha}\" prebuilt verify_py313 ensurepip\n",
            s.url("/pbs.tar.gz")
        ),
    );
    p.display().to_string()
}

#[test]
fn the_prebuilt_step_places_the_tree_and_rewrites_its_compiler_settings() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&[], true));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = f.root.join("versions/3.13.99");
    assert!(v.join("bin/python3.13").is_file());
    let sc =
        std::fs::read_to_string(v.join("lib/python3.13/_sysconfigdata__linux_x86_64-linux-gnu.py"))
            .unwrap();
    assert!(
        sc.contains("'CC': 'cc -pthread'") && sc.contains("'CXX': 'c++ -pthread'"),
        "{sc}"
    );
    assert!(sc.contains(&format!("'prefix': '{}'", v.display())), "{sc}");
    for p in [
        "lib/python3.13/config-3.13-x86_64-linux-gnu/Makefile",
        "bin/python3.13-config",
    ] {
        let t = std::fs::read_to_string(v.join(p)).unwrap();
        assert!(!t.contains("/install") && !t.contains("clang"), "{p}: {t}");
    }
    assert!(!v.join(".rpyenv-incomplete").exists());
}

/// D17: a prebuilt build has no dbm.gnu or nis; §4.4 doesn't ask for them.
#[test]
fn the_check_after_install_skips_dbm_gnu_and_nis_for_a_prebuilt_build() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&["dbm.gnu", "nis"], true));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
}

#[test]
fn a_prebuilt_build_without_sqlite3_fails_the_check_and_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&["sqlite3"], true));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains("doesn't work here without: sqlite3"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("--deps=build"), "{}", r.stderr);
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
}

/// Review Focus 2: a changed layout fails clearly and leaves nothing.
#[test]
fn a_prebuilt_tarball_without_sysconfig_data_fails_and_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(tarball_gz(&[], false));
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_ne!(r.code, 0);
    assert!(r.stderr.contains("_sysconfigdata"), "{}", r.stderr);
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
}

// --- The route: `--deps=prebuilt`, RPYENV_BUILD_DEPS=prebuilt and `[s]` (M8 design §7) ---

/// A built-in definition in the vendored share; its own check step is `verify_py313`, and the
/// fake interpreter is `bin/python3.13`.
const DEF: &str = "3.13.16";
const RELEASE: &str = "20261020";
const SUMMARY: &str = "python-build-standalone 2026-10-20 (third-party build; OpenSSL 3.5.9, Tk 9.0; no dbm.gnu or nis)";
const TK9: &str =
    "pyenv: this build has Tk 9, under which some older Tk programs behave differently";
const DEBIAN: &str =
    "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\nVERSION_ID=\"12\"\n";

/// The table's arch for this machine (`std::env::consts::ARCH` is the table's word).
fn arch() -> &'static str {
    std::env::consts::ARCH
}

/// Where the route downloads DEF's build from, below the base: `+` is written `%2B`.
fn asset_path() -> String {
    format!(
        "/{RELEASE}/cpython-{DEF}%2B{RELEASE}-{}-unknown-linux-gnu-install_only.tar.gz",
        arch()
    )
}

/// A local server with DEF's build; returns it and the build's SHA-256.
fn serve_build(reply: impl FnOnce(Vec<u8>) -> Reply, body: Vec<u8>) -> (Server, String) {
    let sha = sha256(&body);
    (start(vec![(&asset_path(), vec![reply(body)])]), sha)
}

/// `<base>/prebuilt.tsv` with one row for DEF on this machine (none when `sha` is `None`).
fn table(f: &Fixture, sha: Option<&str>, tk: &str) -> String {
    let p = f.base.join("prebuilt.tsv");
    let row = sha
        .map(|sha| format!("{DEF}\t{}\t{RELEASE}\t{sha}\t3.5.9\t{tk}\n", arch()))
        .unwrap_or_default();
    f.file(
        &p,
        &format!("# name\tarch\trelease\tsha256\topenssl\ttk\n{row}"),
    );
    p.display().to_string()
}

/// `<base>/tools`: links to the two programs the fake interpreter runs (`mkdir`, `cp`). With
/// `sys/bin`, the whole PATH of a route test: no compiler, no `sudo`, no `id`.
fn tools(f: &Fixture) -> std::path::PathBuf {
    let dir = f.base.join("tools");
    std::fs::create_dir_all(&dir).unwrap();
    for prog in ["mkdir", "cp"] {
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|d| d.join(prog))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("no {prog} on PATH"));
        let link = dir.join(prog);
        if !link.exists() {
            std::os::unix::fs::symlink(real, link).unwrap();
        }
    }
    dir
}

/// The environment of a route test: PATH is `sys/bin` and `tools` only, the test table, the
/// server as the release folder base, a Debian os-release with apt lists present.
fn route_env<'a>(
    f: &'a Fixture,
    base: &str,
    table: &str,
    extra: &[(&'a str, &'a str)],
) -> Vec<(&'a str, String)> {
    let os = f.base.join("os-release");
    f.file(&os, DEBIAN);
    let lists = f.base.join("lists");
    f.file(
        &lists.join("deb.debian.org_debian_dists_bookworm_main_binary-amd64_Packages"),
        "",
    );
    let mut v: Vec<(&str, String)> = vec![
        (
            "PATH",
            format!("{}:{}", f.syspath.display(), tools(f).display()),
        ),
        ("TMPDIR", f.base.join("tmp").display().to_string()),
        ("PYTHON_BUILD_SKIP_MIRROR", "1".into()),
        ("RPYENV_TEST_PYTHON_ORG", "1".into()),
        ("RPYENV_TEST_PREBUILT_TABLE", table.to_string()),
        ("RPYENV_TEST_PREBUILT_BASE", base.to_string()),
        ("RPYENV_TEST_OS_RELEASE", os.display().to_string()),
        ("RPYENV_TEST_APT_LISTS", lists.display().to_string()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

fn run_route(
    f: &Fixture,
    base: &str,
    table: &str,
    args: &[&str],
    extra: &[(&str, &str)],
) -> common::Run {
    let owned = route_env(f, base, table, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_env(args, &pairs)
}

fn run_route_input(
    f: &Fixture,
    base: &str,
    table: &str,
    args: &[&str],
    extra: &[(&str, &str)],
    input: &str,
) -> common::Run {
    let owned = route_env(f, base, table, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_input(args, &pairs, input)
}

/// `versions/<name>/.rpyenv-deps`, or "" when there is none.
fn record(f: &Fixture, name: &str) -> String {
    std::fs::read_to_string(f.root.join("versions").join(name).join(".rpyenv-deps"))
        .unwrap_or_default()
}

fn prebuilt_record() -> String {
    format!("definition {DEF}\nprebuilt {RELEASE} {}\n", arch())
}

// allowlist D-131
#[test]
fn deps_prebuilt_installs_the_tables_build_without_a_compiler() {
    let f = Fixture::plain();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "--deps=prebuilt", DEF],
        &[],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains(&format!("pyenv: installing the {SUMMARY} of {DEF}\n")),
        "{}",
        r.stderr
    );
    // No pre-flight check ran: on this PATH it would have found no compiler.
    assert!(!r.stderr.contains("can't build"), "{}", r.stderr);
    for dir in [f.syspath.clone(), f.base.join("tools")] {
        for cc in ["cc", "gcc", "clang", "c99"] {
            assert!(!dir.join(cc).exists(), "{} on PATH", dir.join(cc).display());
        }
    }
    let v = f.root.join("versions").join(DEF);
    assert!(v.join("bin/python3.13").is_file());
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert!(
        record(&f, DEF).ends_with(&prebuilt_record()),
        "{}",
        record(&f, DEF)
    );
    assert_eq!(s.hits(&asset_path()), 1);
}

#[test]
fn rpyenv_build_deps_prebuilt_works_without_a_terminal() {
    let f = Fixture::plain();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", DEF],
        &[("RPYENV_BUILD_DEPS", "prebuilt")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains(&format!("pyenv: installing the {SUMMARY} of {DEF}\n")),
        "{}",
        r.stderr
    );
    assert!(
        record(&f, DEF).ends_with(&prebuilt_record()),
        "{}",
        record(&f, DEF)
    );
}

#[test]
fn a_prebuilt_download_with_the_wrong_hash_leaves_nothing() {
    let f = Fixture::plain();
    let (s, _) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha256(b"other bytes")), "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "--deps=prebuilt", DEF],
        &[],
    );
    assert_ne!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("checksum"), "{}", r.stderr);
    assert!(s.hits(&asset_path()) >= 1);
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
}

// allowlist D-131
#[test]
fn deps_prebuilt_without_a_row_refuses_before_downloading() {
    let f = Fixture::plain();
    let (s, _) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, None, "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "--deps=prebuilt", DEF],
        &[],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "pyenv: this rpyenv has no prebuilt build of {DEF} for {} (python-build-standalone releases from 2025-03-11 on); install it from source with --deps=system, install or build\n",
            arch()
        )),
        "{}",
        r.stderr
    );
    assert_eq!(s.hits(&asset_path()), 0);
    assert!(left_in_versions(&f).is_empty());
}

// allowlist D-131
#[test]
fn deps_prebuilt_refuses_debug_and_patch() {
    let f = Fixture::plain();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    for (flag, message) in [
        (
            "-g",
            "pyenv: --deps=prebuilt can't make a debug build (-g)\n",
        ),
        ("-p", "pyenv: --deps=prebuilt can't apply a patch (-p)\n"),
    ] {
        let r = run_route(
            &f,
            &s.url(""),
            &t,
            &["install", "--deps=prebuilt", flag, DEF],
            &[],
        );
        assert_eq!(r.code, 1, "{flag}: {}", r.stderr);
        assert!(r.stderr.contains(message), "{flag}: {}", r.stderr);
    }
    assert_eq!(s.hits(&asset_path()), 0);
    assert!(left_in_versions(&f).is_empty());
}

/// Review Focus 1.
// allowlist D-131
#[test]
fn deps_prebuilt_refuses_a_root_with_a_space() {
    // `Fixture::new` puts a space (and a non-ASCII letter) in every path.
    let f = Fixture::new();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "--deps=prebuilt", DEF],
        &[],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "pyenv: --deps=prebuilt needs a version folder without spaces, colons or shell characters ({})\n",
            f.root.join("versions").join(DEF).display()
        )),
        "{}",
        r.stderr
    );
    assert_eq!(s.hits(&asset_path()), 0);
    assert!(left_in_versions(&f).is_empty());
}

/// Review Focus 3.
#[test]
fn an_interrupt_during_the_prebuilt_download_leaves_nothing() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::plain();
    // Half the build, then a pause long enough for Ctrl+C, then the rest.
    let (s, sha) = serve_build(
        |b| Reply::StallThenFinish(b, Duration::from_secs(3)),
        tarball_gz(&[], true),
    );
    let t = table(&f, Some(&sha), "9.0");
    let owned = route_env(&f, &s.url(""), &t, &[]);
    let mut cmd = f.command(
        std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
        &f.work,
        &[],
    );
    for (k, v) in &owned {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["install", "--deps=prebuilt", DEF])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while s.hits(&asset_path()) == 0 {
        assert!(
            child.try_wait().unwrap().is_none(),
            "pyenv ended before the download started"
        );
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the download never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    common::sigint_group(child.id());
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("pyenv did not stop after SIGINT");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert_eq!(status.code(), Some(130), "{err}");
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
    assert_eq!(record(&f, DEF), "");
}

/// Review Focus 4.
#[test]
fn an_alias_installs_under_its_name_and_records_the_definition() {
    let f = Fixture::plain();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    let alias = format!("{DEF}:mine");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "--deps=prebuilt", &alias],
        &[],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains(&format!("pyenv: installing the {SUMMARY} of {DEF}\n")),
        "{}",
        r.stderr
    );
    assert_eq!(left_in_versions(&f), ["mine"]);
    assert!(f.root.join("versions/mine/bin/python3.13").is_file());
    assert!(
        record(&f, "mine").ends_with(&prebuilt_record()),
        "{}",
        record(&f, "mine")
    );
}

// allowlist D-132
#[test]
fn the_prompt_offers_s_and_s_installs_the_prebuilt_build() {
    let f = Fixture::plain();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    let r = run_route_input(
        &f,
        &s.url(""),
        &t,
        &["install", DEF],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "sqlite"),
            ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ],
        "s\n",
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let offer = format!("  [s] use a prebuilt CPython instead: {SUMMARY}\n");
    let at_s = r
        .stderr
        .find(&offer)
        .unwrap_or_else(|| panic!("{}", r.stderr));
    let at_c = r
        .stderr
        .find("  [c] ")
        .unwrap_or_else(|| panic!("{}", r.stderr));
    assert!(at_s < at_c, "[s] comes before [c]: {}", r.stderr);
    assert!(
        r.stderr
            .contains(&format!("pyenv: installing the {SUMMARY} of {DEF}\n")),
        "{}",
        r.stderr
    );
    assert!(
        record(&f, DEF).contains("\nprebuilt "),
        "{}",
        record(&f, DEF)
    );
    assert_eq!(s.hits(&asset_path()), 1);
}

#[test]
fn the_prompt_leaves_s_out_without_a_row() {
    let f = Fixture::plain();
    let t = table(&f, None, "9.0");
    let r = run_route_input(
        &f,
        "http://127.0.0.1:9",
        &t,
        &["install", DEF],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "sqlite"),
            ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ],
        "s\n",
    );
    // `s` is refused; then the input ends.
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains("How should rpyenv get them?"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("[s]"), "{}", r.stderr);
    let hint = r
        .stderr
        .lines()
        .find(|l| l.starts_with("pyenv: answer p"))
        .unwrap_or_else(|| panic!("no hint: {}", r.stderr));
    let letters: Vec<&str> = hint["pyenv: answer ".len()..]
        .split([',', ' '])
        .filter(|w| !w.is_empty() && *w != "or")
        .collect();
    assert!(!letters.contains(&"s"), "{hint}");
    assert!(left_in_versions(&f).is_empty());
}

#[test]
fn a_tk_9_build_says_so() {
    for (tk, says) in [("9.0", true), ("8.6", false)] {
        let f = Fixture::plain();
        let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
        let t = table(&f, Some(&sha), tk);
        let r = run_route(
            &f,
            &s.url(""),
            &t,
            &["install", "--deps=prebuilt", DEF],
            &[],
        );
        assert_eq!(r.code, 0, "Tk {tk}: {}", r.stderr);
        assert!(
            r.stderr.contains(&format!("OpenSSL 3.5.9, Tk {tk};")),
            "Tk {tk}: {}",
            r.stderr
        );
        assert_eq!(r.stderr.contains(TK9), says, "Tk {tk}: {}", r.stderr);
    }
}

/// Ruling R1: the route is for CPython 3 built from source. A PyPy definition ignores
/// RPYENV_BUILD_DEPS=prebuilt, as it ignores the other routes.
// allowlist D-131
#[test]
fn rpyenv_build_deps_prebuilt_leaves_a_pypy_install_alone() {
    use common::fakepypy::tarball_bz2;
    let top = "pypy3.11-v7.3.99-linux64";
    let f = Fixture::new();
    let body = tarball_bz2(top, true);
    let sha = sha256(&body);
    let s = start(vec![(&format!("/{top}.tar.bz2"), vec![Reply::Body(body)])]);
    let def = f.base.join("defs").join("pypy3.11-7.3.99");
    f.file(
        &def,
        &format!(
            "install_package \"{top}\" \"{}#{sha}\" pypy verify_py311 ensurepip\n",
            s.url(&format!("/{top}.tar.bz2"))
        ),
    );
    let def = def.display().to_string();
    let r = run(&f, &["install", &def], &[("RPYENV_BUILD_DEPS", "prebuilt")]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains("prebuilt"), "{}", r.stderr);
    assert_eq!(left_in_versions(&f), ["pypy3.11-7.3.99"]);
}

/// A prebuilt build never gets `[c]`'s summary: a user's RPYENV_DEPS_MODE=continue is dropped,
/// and a build that fails the check is not installed.
#[test]
fn a_users_deps_mode_does_not_keep_a_failing_prebuilt_build() {
    let f = Fixture::plain();
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&["sqlite3"], true));
    let t = table(&f, Some(&sha), "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "--deps=prebuilt", DEF],
        &[("RPYENV_DEPS_MODE", "continue")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "pyenv: {DEF} (a prebuilt build) doesn't work here without: sqlite3"
        )),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("was built without"), "{}", r.stderr);
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
}

/// An installed prebuilt version whose recorded release is older than the table's is named by
/// any `pyenv install`, even one that then fails.
// allowlist D-133
#[test]
fn install_says_when_the_table_has_a_newer_build_of_an_installed_prebuilt_version() {
    let f = Fixture::plain();
    f.file(
        &f.root.join("versions").join("mine").join(".rpyenv-deps"),
        &format!("definition {DEF}\nprebuilt 20260101 {}\n", arch()),
    );
    let (s, sha) = serve_build(Reply::Body, tarball_gz(&[], true));
    let t = table(&f, Some(&sha), "9.0");
    let r = run_route(
        &f,
        &s.url(""),
        &t,
        &["install", "no-such-definition-9.9.9"],
        &[],
    );
    assert_ne!(r.code, 0, "{}", r.stderr);
    let line = format!(
        "pyenv: mine is the python-build-standalone build of 2026-01-01; rpyenv now offers the one of 2026-10-20 (OpenSSL 3.5.9). To reinstall it: pyenv install -f --deps=prebuilt {DEF}:mine\n"
    );
    assert!(
        r.stderr.contains(&line) || r.stdout.contains(&line),
        "{}\n{}",
        r.stdout,
        r.stderr
    );
}
