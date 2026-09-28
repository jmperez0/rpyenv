mod common;
use common::Fixture;

#[cfg(unix)]
#[test]
fn prefix_of_versions_and_system() {
    let f = Fixture::new();
    f.version("3.12.10").version("3.12.1").version("3.11.9");
    let v = f.root.join("versions");
    assert_eq!(
        f.pyenv(&["prefix", "3.12"]).stdout,
        format!("{}\n", v.join("3.12.10").display())
    );
    assert_eq!(
        f.pyenv(&["prefix", "3.12.1", "3.11.9"]).stdout,
        format!(
            "{}:{}\n",
            v.join("3.12.1").display(),
            v.join("3.11.9").display()
        )
    );
    let r = f.pyenv(&["prefix", "9.9"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: version `9.9' not installed\n", 1)
    );
    let r = f.pyenv(&["prefix"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: system version not found in PATH\n", 1)
    );
    f.python_on_path();
    assert_eq!(
        f.pyenv(&["prefix", "system"]).stdout,
        format!("{}\n", f.syspath.parent().unwrap().display())
    );
}

#[cfg(unix)]
#[test]
fn versions_listing() {
    let f = Fixture::new();
    let r = f.pyenv(&["versions"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Warning: no Python detected on the system\n", 1)
    );
    assert_eq!(f.pyenv(&["versions", "--bare"]).code, 0);
    f.version("3.12.10").version("3.11.9");
    f.python_on_path();
    assert_eq!(
        f.pyenv(&["versions"]).stdout,
        format!(
            "* system (set by {}/version)\n  3.11.9\n  3.12.10\n",
            f.root.display()
        )
    );
    let r = f.pyenv_env(&["versions"], &[("PYENV_VERSION", "3.12")]);
    assert_eq!(
        r.stdout,
        "  system\n  3.11.9\n* 3.12.10 (set by PYENV_VERSION environment variable)\n"
    );
    assert_eq!(f.pyenv(&["versions", "--bare"]).stdout, "3.11.9\n3.12.10\n");
    let r = f.pyenv(&["versions", "--bogus"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]\n",
            1
        )
    );
}

#[cfg(unix)]
#[test]
fn versions_aliases_and_envs() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.version("3.12.10").version("3.12.9/envs/alpha");
    symlink("3.12.10", f.root.join("versions").join("3.12")).unwrap();
    assert_eq!(
        f.pyenv(&["versions", "--bare"]).stdout,
        "3.12\n3.12.9\n3.12.9/envs/alpha\n3.12.10\n"
    );
    assert_eq!(
        f.pyenv(&["versions", "--bare", "--skip-aliases", "--skip-envs"])
            .stdout,
        "3.12.9\n3.12.10\n"
    );
    let r = f.pyenv_env(&["versions"], &[("PYENV_VERSION", "3.12")]);
    assert!(r
        .stdout
        .contains("* 3.12 --> 3.12.10 (set by PYENV_VERSION environment variable)\n"));
}

#[cfg(unix)]
#[test]
fn full_help_listing_and_commands() {
    let f = Fixture::new();
    let expected = "Usage: pyenv <command> [<args>]\n\nSome useful pyenv commands are:\n   --version   Display the version of pyenv\n   commands    List all available pyenv commands\n   global      Set or show the global Python version(s)\n   help        Display help for a command\n   local       Set or show the local application-specific Python version(s)\n   prefix      Display prefixes for Python versions\n   root        Display the root directory where versions and shims are kept\n   version     Show the current Python version(s) and its origin\n   version-file   Detect the file that sets the current pyenv version\n   version-name   Show the current Python version\n   version-origin   Explain how the current Python version is set\n   versions    List all Python versions available to pyenv\n\nSee `pyenv help <command>' for information on a specific command.\nFor full documentation, see: https://github.com/pyenv/pyenv#readme\n";
    assert_eq!(f.pyenv(&["help"]).stdout, expected);
    assert_eq!(
        f.pyenv(&["commands"]).stdout,
        "--version\ncommands\nglobal\nhelp\nlocal\nprefix\nroot\nversion\nversion-file\nversion-file-read\nversion-file-write\nversion-name\nversion-origin\nversions\n"
    );
    assert_eq!(f.pyenv(&["commands", "--sh"]).stdout, "");
}

#[cfg(windows)]
#[test]
fn win_versions_and_prefix() {
    let f = Fixture::new();
    assert_eq!(f.pyenv(&["versions"]).stdout, "");
    f.version("3.9.1").version("3.10.1");
    f.file(&f.root.join("version"), "3.9\r\n");
    assert_eq!(
        f.pyenv(&["versions"]).stdout,
        format!(
            "  3.10.1\r\n* 3.9.1 (set by {}\\version)\r\n",
            f.root.display()
        )
    );
    assert_eq!(
        f.pyenv(&["versions", "--bare"]).stdout,
        "3.10.1\r\n3.9.1\r\n"
    );
    let v = f.root.join("versions");
    assert_eq!(
        f.pyenv(&["prefix"]).stdout,
        format!("{}\r\n", v.join("3.9.1").display())
    );
    assert_eq!(
        f.pyenv(&["prefix", "3.9", "3.10.1"]).stdout,
        format!(
            "{};{}\r\n",
            v.join("3.9.1").display(),
            v.join("3.10.1").display()
        )
    );
    let r = f.pyenv(&["prefix", "9.9"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: version `9.9' not installed\r\n", 1)
    );
}

#[cfg(windows)]
#[test]
fn win_commands_order() {
    let r = Fixture::new().pyenv(&["commands"]);
    assert_eq!(
        r.stdout,
        "--version\r\ncommands\r\nglobal\r\nhelp\r\nlocal\r\nprefix\r\nroot\r\nversion-file-read\r\nversion-file-write\r\nversion-file\r\nversion-name\r\nversion-origin\r\nversion\r\nversions\r\nvname\r\n"
    );
}
