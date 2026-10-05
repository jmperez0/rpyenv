//! Built-in virtualenvs on Windows (allowlist D-101; spec §10).
#![cfg(windows)]

mod common;
use common::winshell::{self, host, output};
use common::Fixture;
use std::path::{Path, PathBuf};

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

/// A real CPython install to copy into the scratch root: `RPYENV_TEST_PYTHON` (a folder with
/// `python.exe`), else the first `python.exe` on `PATH` that sits in a full install (it has
/// `Lib\venv`) and isn't a conda install (copying one takes minutes, and its venvs aren't this
/// test's case). Never a pyenv-win shim or an install under `.pyenv`: the test copies from it
/// and never runs it. CI must have one; a machine without one skips with a note.
fn real_python_home() -> Option<PathBuf> {
    let full = |d: &Path| {
        d.join("python.exe").is_file()
            && d.join("Lib").join("venv").is_dir()
            && !d.join("conda-meta").exists()
    };
    if let Some(d) = std::env::var_os("RPYENV_TEST_PYTHON").map(PathBuf::from) {
        assert!(
            full(&d),
            "RPYENV_TEST_PYTHON is not a CPython folder: {}",
            d.display()
        );
        return Some(d);
    }
    let found = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .filter(|d| {
                let s = d.to_string_lossy().to_ascii_lowercase();
                !s.contains(".pyenv") && !s.contains("shims")
            })
            .find(|d| full(d))
    });
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI needs a CPython install on PATH"
        );
        eprintln!("skipped: no CPython install on PATH (set RPYENV_TEST_PYTHON)");
    }
    found
}

/// Copies a CPython install into `to`, leaving out what venv doesn't need: the top-level
/// `Scripts`, `Doc` and `tcl`, and `Lib\site-packages`. `Lib\venv\scripts` holds the launcher
/// venv copies, so names are only skipped at those two places.
fn copy_install(from: &Path, to: &Path) {
    fn copy(from: &Path, to: &Path, skip: &[&str]) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let name = e.file_name();
            let n = name.to_string_lossy().to_ascii_lowercase();
            if skip.contains(&n.as_str()) {
                continue;
            }
            let dest = to.join(&name);
            if e.file_type().unwrap().is_dir() {
                let inner: &[&str] = if n == "lib" { &["site-packages"] } else { &[] };
                copy(&e.path(), &dest, inner);
            } else {
                std::fs::copy(e.path(), &dest).unwrap();
            }
        }
    }
    copy(from, to, &["scripts", "doc", "tcl"]);
}

/// A real `python -m venv` (side-agent note, 2026-10-05: the other tests use a fake Python):
/// a copy of a CPython install as `versions\3.99.0`, an env behind a junction, `exec` through
/// it, `virtualenv-prefix`, and uninstall taking the env and the junction.
#[test]
fn a_real_python_makes_a_working_env() {
    let Some(home) = real_python_home() else {
        return;
    };
    let f = Fixture::new();
    let base = f.root.join("versions").join("3.99.0");
    copy_install(&home, &base);
    assert_eq!(
        run(&f, &["virtualenv", "--without-pip", "3.99.0", "realenv"]).2,
        0
    );
    let env = base.join("envs").join("realenv");
    assert_eq!(
        std::fs::read_link(f.root.join("versions").join("realenv")).unwrap(),
        env
    );
    let r = f.pyenv_env(
        &[
            "exec",
            "python",
            "-c",
            "import sys; print(sys.prefix.isascii(), sys.base_prefix == sys.prefix)",
        ],
        &[
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
            ("PYENV_VERSION", "realenv"),
        ],
    );
    // The fixture root isn't ASCII, so the prefix itself isn't compared through the console.
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("False False\r\n", 0),
        "{}",
        r.stderr
    );
    let (out, err, code) = run(&f, &["virtualenv-prefix", "realenv"]);
    assert_eq!((out, code), (format!("{}\r\n", base.display()), 0), "{err}");
    assert_eq!(run(&f, &["uninstall", "-f", "realenv"]).2, 0);
    assert!(!env.exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("realenv")).is_err());
}

/// Final review C1: uninstalling a user's junction into `versions` never follows it.
#[test]
fn uninstall_never_follows_a_users_junction() {
    let f = Fixture::new();
    let outside = f.base.join("outside");
    f.file(
        &outside
            .join("conda")
            .join("envs")
            .join("userenv")
            .join("data.txt"),
        "mine",
    );
    rpyenv_core::junction::create(
        &f.root.join("versions").join("conda"),
        &outside.join("conda"),
    )
    .unwrap();
    assert_eq!(run(&f, &["uninstall", "-f", "conda"]).2, 0);
    assert!(outside
        .join("conda")
        .join("envs")
        .join("userenv")
        .join("data.txt")
        .is_file());
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("conda")).is_err());
}

/// Final review I3: a junction to an env that is gone is removed, and nothing removed is never
/// reported as uninstalled.
#[test]
fn uninstall_removes_a_dangling_junction_and_reports_only_what_it_removed() {
    let f = Fixture::new();
    let b = base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "foo"]).2, 0);
    std::fs::remove_dir_all(b.join("envs").join("foo")).unwrap();
    let (out, _, code) = run(&f, &["uninstall", "-f", "foo"]);
    assert_eq!(
        (out.as_str(), code),
        ("pyenv: Successfully uninstalled foo\r\n", 0)
    );
    assert!(std::fs::symlink_metadata(f.root.join("versions").join("foo")).is_err());
    let (out, _, _) = run(&f, &["uninstall", "-f", "3.13.1/envs/nosuch"]);
    assert!(!out.contains("Successfully"), "{out}");
}

/// Final review M6: re-activating in cmd replaces the prompt tag instead of stacking it.
#[test]
fn cmd_reactivation_does_not_stack_prompts() {
    let f = Fixture::new();
    base(&f, "3.13.1");
    assert_eq!(run(&f, &["virtualenv", "3.13.1", "foo"]).2, 0);
    let r = f.pyenv_env(
        &["activate", "foo"],
        &[
            ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
            ("PYENV_SHELL", "cmd"),
            ("PROMPT", "(bar) $P$G"),
            ("_OLD_VIRTUAL_PROMPT", "$P$G"),
        ],
    );
    assert!(
        r.stdout.contains("set \"PROMPT=(foo) $P$G\""),
        "{}",
        r.stdout
    );
}
