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
