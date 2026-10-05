//! Built-in virtualenvs on Windows (allowlist D-101; spec §10).
#![cfg(windows)]

mod common;
use common::Fixture;
use std::path::PathBuf;

/// A stand-in base `python.bat`: `-m venv --help`, and `-m venv DIR` (no options).
const FAKE_PY: &str = "@echo off\r\nif \"%~1\"==\"-m\" if \"%~2\"==\"venv\" goto venv\r\nexit /b 3\r\n:venv\r\nif \"%~3\"==\"--help\" exit /b 0\r\nif defined FAKE_VENV_FAIL (echo venv failed& exit /b 5)\r\nmkdir \"%~3\\Scripts\"\r\n(echo home = %~dp0& echo include-system-site-packages = false)> \"%~3\\pyvenv.cfg\"\r\ncopy /y \"%~f0\" \"%~3\\Scripts\\python.bat\" >nul\r\ntype nul > \"%~3\\Scripts\\activate\"\r\ntype nul > \"%~3\\Scripts\\pip.exe\"\r\nexit /b 0\r\n";

fn base(f: &Fixture, v: &str) -> PathBuf {
    let b = f.root.join("versions").join(v);
    f.file(&b.join("python.bat"), FAKE_PY);
    b
}

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv_env(args, &[("PATHEXT", ".COM;.EXE;.BAT;.CMD")]);
    (r.stdout, r.stderr, r.code)
}

#[test]
fn creates_an_env_behind_a_junction_and_lists_it() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    let (_, err, code) = run(&f, &["virtualenv", "3.13.1", "foo"]);
    assert_eq!(code, 0, "{err}");
    let env = b.join("envs").join("foo");
    assert_eq!(
        std::fs::read_link(f.root.join("versions").join("foo")).unwrap(),
        env
    );
    let listed = run(&f, &["versions"]).0;
    assert!(listed.contains("  3.13.1\\envs\\foo\r\n"), "{listed}");
    assert!(
        listed.contains(&format!("  foo --> {}\r\n", env.display())),
        "{listed}"
    );
    assert_eq!(
        run(&f, &["prefix", "3.13.1/envs/foo"]).0,
        format!("{}\r\n", env.display())
    );
}

/// Spec §10: uninstalling by link name removes the env and the junction; uninstalling the base
/// removes its envs and the junctions into them (no dangling junction).
#[test]
fn uninstall_takes_the_env_and_its_junction() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "foo"]).2, 0);
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "bar"]).2, 0);
    assert!(b.join("envs").join("foo").is_dir() && b.join("envs").join("bar").is_dir());
    assert_eq!(run(&f, &["uninstall", "-f", "foo"]).2, 0);
    assert!(!b.join("envs").join("foo").exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("foo")).is_err());
    assert_eq!(run(&f, &["uninstall", "-f", "3.13.1"]).2, 0);
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("bar")).is_err());
    assert!(!b.exists());
}

/// Review focus 1 and 3: a name that leaves `versions`, and a user's junction pointing outside.
#[test]
fn names_and_foreign_junctions_are_safe() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    assert_eq!(
        run(&f, &["virtualenv", "3.13.1", "x\\..\\..\\y"]).1,
        "pyenv-virtualenv: no slash allowed in virtualenv name.\r\n"
    );
    let outside = f.base.join("outside");
    std::fs::create_dir_all(outside.join("Scripts")).unwrap();
    rpyenv_core::junction::create(&f.root.join("versions").join("mine"), &outside).unwrap();
    let (_, err, code) = run(&f, &["virtualenv-delete", "-f", "mine"]);
    assert_eq!(code, 1, "{err}");
    assert!(outside.join("Scripts").is_dir());
}

/// Review focus 4: a failed venv leaves no env and no junction.
#[test]
fn a_failed_venv_cleans_up() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    let r = f.pyenv_env(
        &["virtualenv", "3.13.1", "bad"],
        &[("PATHEXT", ".COM;.EXE;.BAT;.CMD"), ("FAKE_VENV_FAIL", "1")],
    );
    assert_eq!(r.code, 5);
    assert!(!b.join("envs").join("bad").exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("bad")).is_err());
}

#[test]
fn virtualenv_init_prints_nothing() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["virtualenv-init", "-"]),
        (String::new(), String::new(), 0)
    );
}
