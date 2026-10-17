//! `pyenv install` of PyPy on Linux against a fake PyPy tarball on a local server (M7a design
//! §4.3, §8; spec §12.5 tier 1).
#![cfg(unix)]

mod common;

use common::fakepypy::{sha256, tarball_bz2, tarball_bz2_lacking};
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

/// Ruling R14: M8 design §4.4's module check is for CPython built from source. A PyPy that
/// lacks `_uuid` (the real PyPy 3.11 does) installs as before M8a, at a terminal or not: only
/// upstream's checks, no "was built without" and no lenient summary.
#[test]
fn a_pypy_without_uuid_installs_with_upstreams_checks_only() {
    let mut bad: Vec<String> = Vec::new();
    for tty in ["1", ""] {
        let f = Fixture::new();
        let body = tarball_bz2_lacking(TOP, "_uuid");
        let sha = sha256(&body);
        let s = start(vec![(&format!("/{TOP}.tar.bz2"), vec![Reply::Body(body)])]);
        let def = def_file(&f, &s, &sha);
        let r = run(&f, &["install", &def], &[("RPYENV_TEST_TTY", tty)]);
        // Both runs are judged before any assertion, so a failure shows both.
        let ok = r.code == 0
            && !r.stderr.contains("was built without")
            && !r.stderr.contains("In a later rpyenv release")
            && left_in_versions(&f) == ["pypy3.11-7.3.99"];
        if !ok {
            bad.push(format!("tty={tty:?}: exit {}\n{}", r.code, r.stderr));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n---\n"));
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

/// A table folder for `RPYENV_TEST_PYPY_TABLES`: `name` gets `verdict` for this machine's
/// architecture; `windows.tsv` is empty.
fn tables(f: &Fixture, name: &str, verdict: &str) -> String {
    let arch = pyenv::install::pypy::host_arch().expect("tests run on x64 or aarch64");
    let d = f.base.join("tables");
    f.file(
        &d.join("linux.tsv"),
        &format!("{name}\t{arch}\t{verdict}\n"),
    );
    f.file(&d.join("windows.tsv"), "");
    d.display().to_string()
}

/// The newest vendored name with this suffix rule, so the test follows the weekly sync.
fn vendored(pred: impl Fn(&str) -> bool) -> String {
    pyenv::install::defs::DEFINITIONS
        .iter()
        .map(|(n, _)| n.to_string())
        .filter(|n| n.starts_with("pypy") && pred(n))
        .last()
        .expect("a vendored PyPy definition")
}

#[test]
fn a_vendored_name_installs_from_its_table_row_without_a_compiler() {
    let f = Fixture::new();
    let (s, sha) = serve_pypy(true);
    let name = vendored(|n| n.starts_with("pypy3.11-") && !n.ends_with("-src"));
    let verdict = format!(
        "ok {TOP} {}#{sha} pypy verify_py311 ensurepip",
        s.url(&format!("/{TOP}.tar.bz2"))
    );
    let t = tables(&f, &name, &verdict);
    // PATH has no compiler beyond the real one: drop it to prove preflight doesn't run.
    let r = run(
        &f,
        &["install", &name],
        &[("RPYENV_TEST_PYPY_TABLES", &t), ("CC", "/nonexistent/cc")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        link_of(&f.root.join(format!("versions/{name}/bin/python"))),
        "pypy3.11"
    );
}

// allowlist D-106 (ruling R7)
#[test]
fn a_third_party_build_row_is_refused_by_name() {
    let f = Fixture::new();
    let name = vendored(|n| !n.ends_with("-src"));
    let t = tables(&f, &name, "unsupported third-party build");
    let r = run(&f, &["install", &name], &[("RPYENV_TEST_PYPY_TABLES", &t)]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "python-build: {name}: PyPy source, nightly, distribution-specific and third-party builds are not supported"
        )),
        "{}",
        r.stderr
    );
    assert!(left_in_versions(&f).is_empty());
}

// allowlist D-106
#[test]
fn source_builds_and_missing_binaries_are_refused_by_name() {
    let f = Fixture::new();
    let src = vendored(|n| n.ends_with("-src"));
    let r = run(&f, &["install", &src], &[]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "{src}: PyPy source, nightly, distribution-specific and third-party builds are not supported"
        )),
        "{}",
        r.stderr
    );
    let name = vendored(|n| !n.ends_with("-src"));
    let t = tables(&f, &name, "no-binary");
    let r = run(&f, &["install", &name], &[("RPYENV_TEST_PYPY_TABLES", &t)]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    let arch = pyenv::install::pypy::host_arch().unwrap();
    assert!(
        r.stderr.contains(&format!(
            "{name}: The binary distribution of PyPy is not available for {arch}."
        )),
        "{}",
        r.stderr
    );
    assert!(left_in_versions(&f).is_empty());
}

#[test]
fn a_users_own_template_definition_is_read_with_bash() {
    let f = Fixture::new();
    let (s, sha) = serve_pypy(true);
    let arch = pyenv::install::pypy::host_arch().unwrap();
    let url = s.url(&format!("/{TOP}.tar.bz2"));
    // The template's shape: variables, then functions that pick the row (spec §3).
    let text = format!(
        "VERSION='7.3.99'\nPYVER='3.11'\nfunction pick {{\n  case \"$1\" in\n  {arch}) echo install_package {TOP} \"{url}#{sha}\" ;;\n  *) exit 1 ;;\n  esac\n}}\ndeclare -a pd=\"$(pick \"$(pypy_architecture)\")\"\n${{pd[0]}} \"${{pd[1]}}\" \"${{pd[2]}}\" pypy verify_py311 ensurepip\n"
    );
    let defs = f.base.join("defs");
    f.file(&defs.join("pypy3.11-7.3.99"), &text);
    let d = defs.display().to_string();
    let r = run(
        &f,
        &["install", "pypy3.11-7.3.99"],
        &[("PYTHON_BUILD_DEFINITIONS", &d)],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f
        .root
        .join("versions/pypy3.11-7.3.99/bin/pip3.11")
        .is_file());
    // Without bash on PATH, the error names the file.
    // -f: the first run left the version installed, which would otherwise stop at the prompt.
    let r = f.pyenv_env(
        &["install", "-f", "pypy3.11-7.3.99"],
        &[
            ("PYTHON_BUILD_DEFINITIONS", &d),
            ("PATH", &f.syspath.display().to_string()),
        ],
    );
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("pypy3.11-7.3.99 needs bash to be read"),
        "{}",
        r.stderr
    );
}
