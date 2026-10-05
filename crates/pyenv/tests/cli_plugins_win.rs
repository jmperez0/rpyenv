//! Windows plugin dispatch (plan M4a Decision 5; allowlist D-93).
#![cfg(windows)]

mod common;
use common::Fixture;

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv_env(args, &[("PATHEXT", ".COM;.EXE;.BAT;.CMD")]);
    (r.stdout, r.stderr, r.code)
}

/// allowlist D-93
#[test]
fn a_batch_plugin_runs_with_the_dispatcher_s_environment() {
    let f = Fixture::new();
    f.file(
        &f.root.join("plugins/x/bin/pyenv-hello.bat"),
        "@echo off\r\necho [%1][%2][%PYENV_ROOT%]\r\nexit /b 3\r\n",
    );
    let (out, err, code) = run(&f, &["hello", "a", "b"]);
    assert_eq!(
        (out, code),
        (format!("[a][b][{}]\r\n", f.root.display()), 3),
        "{err}"
    );
    assert!(run(&f, &["commands"]).0.lines().any(|l| l == "hello"));
    // No marker: `--help` only.
    assert_eq!(
        run(&f, &["completions", "hello"]),
        ("--help\r\n".to_string(), String::new(), 0)
    );
    assert_eq!(
        run(&f, &["help", "hello"]),
        (
            String::new(),
            "Sorry, this command isn't documented yet.\r\n".to_string(),
            1
        )
    );
    assert_eq!(
        run(&f, &["nosuch"]).0,
        "pyenv: no such command 'nosuch'\r\n"
    );
}

/// Decision 5: a pyenv-win root's own `libexec\pyenv-<cmd>.bat` scripts aren't plugins
/// (`<prefix>\libexec` is upstream pyenv's slot, not pyenv-win's).
#[test]
fn pyenv_win_s_libexec_scripts_are_not_plugins() {
    let f = Fixture::new();
    let inst = f.base.join("inst");
    std::fs::create_dir_all(inst.join("bin")).unwrap();
    let exe = inst.join("bin").join("pyenv.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_pyenv"), &exe).unwrap();
    f.file(
        &inst.join("libexec").join("pyenv-duplicate.bat"),
        "@echo ran\r\n",
    );
    let o = f
        .command(&exe, &f.work, &[("PATHEXT", ".COM;.EXE;.BAT;.CMD")])
        .arg("duplicate")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "pyenv: no such command 'duplicate'\r\n"
    );
}

/// Final review #4: a name with a path separator can't climb out of the plugin folders.
#[test]
fn a_plugin_name_cannot_leave_the_plugin_folders() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("plugins/p/bin")).unwrap();
    f.file(&f.root.join("evil.bat"), "@echo OUTSIDE\r\n");
    let name = r"x\..\..\..\..\evil";
    let (out, _, code) = run(&f, &[name]);
    assert_eq!(
        (out, code),
        (format!("pyenv: no such command '{name}'\r\n"), 1)
    );
}

/// Re-review M-5: with the marker, `completions` prints `--help`, then the plugin's own
/// output and exit code; `\n` after `--help` when an integrated POSIX shell reads it.
#[test]
fn a_batch_plugin_s_completions_pass_through() {
    let f = Fixture::new();
    f.file(
        &f.root.join("plugins/x/bin/pyenv-ask.cmd"),
        "@echo off\r\necho args=%*\r\necho err>&2\r\nexit /b 7\r\n# Provide pyenv completions\r\n",
    );
    let pathext = ("PATHEXT", ".COM;.EXE;.BAT;.CMD");
    let r = f.pyenv_env(&["completions", "ask", "a", "b"], &[pathext]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("--help\r\nargs=--complete a b\r\n", 7),
        "{}",
        r.stderr
    );
    assert_eq!(r.stderr, "err\r\n");
    let r = f.pyenv_env(
        &["completions", "ask", "a", "b"],
        &[pathext, ("PYENV_SHELL", "bash")],
    );
    assert_eq!(r.stdout, "--help\nargs=--complete a b\r\n");
}
