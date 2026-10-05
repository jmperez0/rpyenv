//! Built-in virtualenvs on Windows (allowlist D-101; spec §10).
#![cfg(windows)]

mod common;
use common::winshell::{self, host, output};
use common::Fixture;
use std::path::PathBuf;

/// A stand-in base `python.bat`: `-m venv --help`, and `-m venv DIR` (no options).
const FAKE_PY: &str = "@echo off\r\nif \"%~1\"==\"-m\" if \"%~2\"==\"venv\" goto venv\r\nexit /b 3\r\n:venv\r\nif \"%~3\"==\"--help\" exit /b 0\r\nif defined FAKE_VENV_FAIL (echo venv failed& exit /b 5)\r\nmkdir \"%~3\\Scripts\"\r\ncopy /y \"%~dp0pyvenv.template\" \"%~3\\pyvenv.cfg\" >nul\r\ncopy /y \"%~f0\" \"%~3\\Scripts\\python.bat\" >nul\r\ntype nul > \"%~3\\Scripts\\activate\"\r\ntype nul > \"%~3\\Scripts\\pip.exe\"\r\ntype nul > \"%~3\\Scripts\\python.exe\"\r\nexit /b 0\r\n";

fn base(f: &Fixture, v: &str) -> PathBuf {
    let b = f.root.join("versions").join(v);
    f.file(&b.join("python.bat"), FAKE_PY);
    // cmd would write a non-ASCII `home` in the OEM code page; venv writes UTF-8. The fake
    // copies this UTF-8 file byte for byte.
    f.file(
        &b.join("pyvenv.template"),
        &format!(
            "home = {}\r\ninclude-system-site-packages = false\r\n",
            b.display()
        ),
    );
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

/// Windows PowerShell 5.1 and, when installed, pwsh 7: `pyenv activate` through the function
/// sets the variables, `pyenv deactivate` removes them (Decision 6; allowlist D-101).
#[test]
fn powershell_activate_and_deactivate_round_trip() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "foo"]).2, 0);
    winshell::install_pyenv(&f);
    // The fixture root has a non-ASCII name, and PowerShell 5.1's console output can't carry
    // it, so the value is checked inside PowerShell: the env folder's name, and that it exists.
    let script = "iex ((pyenv init - --no-rehash powershell) -join \"`n\"); pyenv activate foo; Write-Output \"[$(Split-Path -Leaf $Env:VIRTUAL_ENV)][$(Test-Path -LiteralPath $Env:VIRTUAL_ENV)][$(Split-Path -Leaf (Split-Path -Parent $Env:VIRTUAL_ENV))][$Env:PYENV_VERSION]\"; pyenv deactivate; Write-Output \"[$Env:VIRTUAL_ENV][$Env:PYENV_VERSION]\"";
    let mut shells = vec![winshell::powershell()];
    shells.extend(winshell::pwsh());
    for sh in shells {
        let mut c = host(&f, &sh, &[("PYENV_SHELL", "pwsh")], &[]);
        c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        let (out, err, code) = output(c);
        assert_eq!(
            (out, code),
            ("[foo][True][envs][foo]\r\n[][]\r\n".to_string(), 0),
            "{}: {err}",
            sh.display()
        );
    }
}

#[test]
fn cmd_and_no_integration_print_the_set_lines() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "foo"]).2, 0);
    let r = f.pyenv_env(
        &["activate", "foo"],
        &[("PATHEXT", ".COM;.EXE;.BAT;.CMD"), ("PYENV_SHELL", "cmd")],
    );
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("set \"VIRTUAL_ENV="), "{}", r.stdout);
    assert!(
        r.stderr.contains("cmd has no shell integration"),
        "{}",
        r.stderr
    );
}

#[test]
fn git_bash_gets_an_msys_virtual_env() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "foo"]).2, 0);
    let r = f.pyenv_env(
        &["sh-activate", "foo"],
        &[("PATHEXT", ".COM;.EXE;.BAT;.CMD"), ("PYENV_SHELL", "bash")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("export VIRTUAL_ENV=\"/"), "{}", r.stdout);
    assert!(!r.stdout.contains('\r'), "LF only");
}
