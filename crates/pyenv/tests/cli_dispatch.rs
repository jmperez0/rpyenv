mod common;
use common::{nl, Fixture};

#[test]
fn version_flag() {
    let expected = if cfg!(windows) {
        "pyenv 3.1.1 (rpyenv 0.1.0)\n"
    } else {
        "pyenv 2.8.6 (rpyenv 0.1.0)\n"
    };
    let r = Fixture::new().pyenv(&["--version"]);
    assert_eq!((r.stdout, r.code), (nl(expected), 0));
}

#[test]
fn root_prints_pyenv_root() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["root"]).stdout,
        nl(&format!("{}\n", f.root.display()))
    );
}

#[test]
fn commands_lists_root() {
    assert!(Fixture::new()
        .pyenv(&["commands"])
        .stdout
        .contains(&nl("root\n")));
}

#[cfg(unix)]
#[test]
fn no_arguments_prints_version_and_help_to_stderr() {
    let r = Fixture::new().pyenv(&[]);
    assert_eq!(r.stdout, "");
    assert!(r
        .stderr
        .starts_with("pyenv 2.8.6 (rpyenv 0.1.0)\nUsage: pyenv <command> [<args>]\n\nSome useful pyenv commands are:\n"));
    assert!(r
        .stderr
        .ends_with("For full documentation, see: https://github.com/pyenv/pyenv#readme\n"));
    assert_eq!(r.code, 1);
}

#[cfg(unix)]
#[test]
fn unknown_command() {
    let r = Fixture::new().pyenv(&["nosuch"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: no such command `nosuch'\n", 1)
    );
}

#[cfg(unix)]
#[test]
fn help_texts() {
    let f = Fixture::new();
    let root_help =
        "Usage: pyenv root\n\nDisplay the root directory where versions and shims are kept\n\n";
    assert_eq!(f.pyenv(&["help", "root"]).stdout, root_help);
    assert_eq!(f.pyenv(&["root", "--help"]).stdout, root_help);
    let r = f.pyenv(&["help", "--usage"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("Usage: pyenv <command> [<args>]\n", 1)
    );
    assert_eq!(f.pyenv(&["help", "--usage", "root"]).stdout, "");
    assert_eq!(
        f.pyenv(&["help", "--usage", "commands"]).stdout,
        "Usage: pyenv commands [--sh|--no-sh]\n"
    );
    let r = f.pyenv(&["help", "nosuch"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: no such command `nosuch'\n", 1)
    );
}

#[cfg(unix)]
#[test]
fn shell_needs_integration() {
    let r = Fixture::new().pyenv(&["shell"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "pyenv: shell integration not enabled. Run `pyenv init' for instructions.\n",
            1
        )
    );
}

#[cfg(unix)]
#[test]
fn bad_pyenv_dir() {
    let r = Fixture::new().pyenv_env(&["root"], &[("PYENV_DIR", "/nope")]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: cannot change working directory to `/nope'\n", 1)
    );
}

/// Review M-4: a terminal can sit in a directory that was deleted. Upstream keeps
/// working from bash's `$PWD`.
#[cfg(unix)]
#[test]
fn works_in_a_deleted_directory() {
    let f = Fixture::new();
    let doomed = f.base.join("doomed");
    std::fs::create_dir_all(&doomed).unwrap();
    let out = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            r#"cd "$1" && export PWD && /bin/rmdir "$1" && exec "$2" root"#,
            "sh",
        ])
        .arg(&doomed)
        .arg(env!("CARGO_BIN_EXE_pyenv"))
        .env_clear()
        .env("PYENV_ROOT", &f.root)
        .output()
        .unwrap();
    assert_eq!(
        (String::from_utf8(out.stdout).unwrap(), out.status.code()),
        (format!("{}\n", f.root.display()), Some(0))
    );
}

#[cfg(windows)]
#[test]
fn win_no_arguments_prints_show_help() {
    let r = Fixture::new().pyenv(&[]);
    assert!(r
        .stdout
        .starts_with("pyenv 3.1.1 (rpyenv 0.1.0)\r\n\r\nUsage: pyenv <command> [<args>]\r\n"));
    assert_eq!(r.code, 0);
}

#[cfg(windows)]
#[test]
fn win_unknown_command_goes_to_stdout() {
    let r = Fixture::new().pyenv(&["nosuch"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("pyenv: no such command 'nosuch'\r\n", "", 1)
    );
}

#[cfg(windows)]
#[test]
fn win_help() {
    let f = Fixture::new();
    let listing = f.pyenv(&["help"]);
    assert!(listing
        .stdout
        .contains("   rehash      Rehash pyenv shims (run this after installing executables)\r\n"));
    let bare = f.pyenv(&["--help"]);
    assert_eq!((bare.stdout, bare.code), (listing.stdout, 0));
    let root_help = "Usage: pyenv root\r\n\r\nDisplay the root directory where versions and shims are kept\r\n\r\n";
    assert_eq!(f.pyenv(&["help", "root"]).stdout, root_help);
    assert_eq!(f.pyenv(&["ROOT", "--help"]).stdout, root_help);
    assert_eq!(
        f.pyenv(&["commands", "--help"]).stdout,
        "Usage: pyenv commands\r\n\r\nList all available pyenv commands\r\n\r\n"
    );
}
