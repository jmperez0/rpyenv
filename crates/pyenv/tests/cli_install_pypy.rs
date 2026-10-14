//! `pyenv install` of PyPy on Linux against a fake PyPy tarball on a local server (M7a design
//! §4.3, §8; spec §12.5 tier 1).
#![cfg(unix)]

mod common;

use common::fakepypy::{sha256, tarball_bz2};
use common::server::{start, Reply, Server};
use common::Fixture;
use std::path::Path;

const TOP: &str = "pypy3.11-v7.3.99-linux64";

/// A local server with `/<TOP>.tar.bz2`; returns it and the file's SHA-256.
fn serve_pypy(real_pypy: bool) -> (Server, String) {
    let body = tarball_bz2(TOP, real_pypy);
    let sha = sha256(&body);
    (
        start(vec![(&format!("/{TOP}.tar.bz2"), vec![Reply::Body(body)])]),
        sha,
    )
}

/// The environment the CLI tests run with: the fixture's `sys/bin` first, then the real PATH
/// (for `sh`, `bash`, `dirname`), a scratch TMPDIR, and no mirror. The pre-flight check is not
/// skipped: a PyPy install must not need a compiler (spec §4.3).
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

/// A one-line definition file `<base>/defs/pypy3.11-7.3.99` for the served tarball.
fn def_file(f: &Fixture, s: &Server, sha: &str) -> String {
    let p = f.base.join("defs").join("pypy3.11-7.3.99");
    f.file(
        &p,
        &format!(
            "install_package \"{TOP}\" \"{}#{sha}\" pypy verify_py311 ensurepip\n",
            s.url(&format!("/{TOP}.tar.bz2"))
        ),
    );
    p.display().to_string()
}

fn link_of(p: &Path) -> String {
    std::fs::read_link(p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .display()
        .to_string()
}

fn left_in_versions(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_pypy_definition_installs_with_upstreams_links_and_pip() {
    let f = Fixture::new();
    let (s, sha) = serve_pypy(true);
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = f.root.join("versions/pypy3.11-7.3.99");
    assert_eq!(link_of(&v.join("bin/python")), "pypy3.11");
    assert_eq!(
        link_of(&v.join("lib/libpypy3.11-c.so")),
        "../bin/libpypy3.11-c.so"
    );
    assert_eq!(
        link_of(&v.join("bin/pypy3")),
        "pypy3.11",
        "links are copied as links"
    );
    assert!(v.join("bin/pip3.11").is_file(), "ensurepip ran");
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert_eq!(left_in_versions(&f), ["pypy3.11-7.3.99"]);
    assert!(
        r.stderr
            .contains(&format!("Installed {TOP} to {}", v.display())),
        "{}",
        r.stderr
    );
}

/// Review focus 2.
#[test]
fn a_tarball_without_a_pypy_executable_fails_and_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve_pypy(false);
    let def = def_file(&f, &s, &sha);
    let r = run(&f, &["install", &def], &[]);
    assert_ne!(r.code, 0);
    assert!(
        r.stderr.contains("invalid Python executable") && r.stderr.contains("/bin/python"),
        "{}",
        r.stderr
    );
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
}

/// Review focus 1: pip's packages live in `lib/pypy3.11/site-packages`, which a reinstall keeps
/// as upstream's copy over the old tree does.
#[test]
fn force_reinstall_keeps_packages_in_pypys_site_packages() {
    let f = Fixture::new();
    let (s, sha) = serve_pypy(true);
    let def = def_file(&f, &s, &sha);
    assert_eq!(run(&f, &["install", &def], &[]).code, 0);
    let sp = f
        .root
        .join("versions/pypy3.11-7.3.99/lib/pypy3.11/site-packages");
    std::fs::create_dir_all(sp.join("requests")).unwrap();
    let r = run(&f, &["install", "-f", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(sp.join("requests").is_dir(), "the package was kept");
    assert!(
        sp.join("README").is_file(),
        "the new tree's own files are there"
    );
}
