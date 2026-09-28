mod common;
use common::Fixture;

#[cfg(unix)]
#[test]
fn which_prints_the_path() {
    let f = Fixture::new();
    let tool = f.exe("3.12.1/bin/tool");
    let r = f.pyenv_env(&["which", "tool"], &[("PYENV_VERSION", "3.12.1")]);
    assert_eq!(
        (r.stdout, r.stderr, r.code),
        (format!("{}\n", tool.display()), String::new(), 0)
    );
}

#[cfg(unix)]
#[test]
fn which_not_found_matches_upstream() {
    let f = Fixture::new();
    f.exe("3.11.9/bin/tool");
    f.exe("3.12.1/envs/venv1/bin/tool");
    let r = f.pyenv_env(&["which", "tool"], &[("PYENV_VERSION", "9.9:8.8")]);
    assert_eq!(r.stdout, "");
    assert_eq!(
        r.stderr,
        "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n\
         pyenv: version `8.8' is not installed (set by PYENV_VERSION environment variable)\n\
         pyenv: tool: command not found\n\
         \n\
         The `tool' command exists in these Python versions:\n  \
         3.11.9\n  \
         3.12.1/envs/venv1\n\
         \n\
         Note: See 'pyenv help global' for tips on allowing multiple\n      \
         Python versions to be found at the same time.\n"
    );
    assert_eq!(r.code, 127);
    let r = f.pyenv_env(
        &["which", "tool", "--skip-advice"],
        &[("PYENV_VERSION", "9.9")],
    );
    assert_eq!(
        r.stderr,
        "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n\
         pyenv: tool: command not found\n"
    );
}

#[cfg(unix)]
#[test]
fn which_usage_and_a_flag_in_command_position() {
    let f = Fixture::new();
    let r = f.pyenv(&["which"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "Usage: pyenv which <command> [--nosystem] [--skip-advice]\n",
            1
        )
    );
    // Reference probe: the command is always the first argument.
    let r = f.pyenv(&["which", "--nosystem", "ls"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: --nosystem: command not found\n", 127)
    );
}

#[cfg(unix)]
#[test]
fn whence_names_paths_and_no_match() {
    let f = Fixture::new();
    let a = f.exe("3.11.9/bin/python");
    let b = f.exe("3.12.1/bin/python");
    assert_eq!(f.pyenv(&["whence", "python"]).stdout, "3.11.9\n3.12.1\n");
    assert_eq!(
        f.pyenv(&["whence", "--path", "python"]).stdout,
        format!("{}\n{}\n", a.display(), b.display())
    );
    let r = f.pyenv(&["whence", "nothing"]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 1));
    let r = f.pyenv(&["whence", "--path"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Usage: pyenv whence [--path] <command>\n", 1)
    );
}

#[cfg(unix)]
#[test]
fn help_for_which_and_whence() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["help", "which"]).stdout,
        "Usage: pyenv which <command> [--nosystem] [--skip-advice]\n\n\
         Displays the full path to the executable that pyenv will invoke when\n\
         you run the given command.\n\
         Use --nosystem argument in case when you don't need to search command in the \n\
         system environment.\n\
         Internal switch --skip-advice used to skip printing an error message on a\n\
         failed search.\n\n"
    );
    assert_eq!(
        f.pyenv(&["help", "--usage", "whence"]).stdout,
        "Usage: pyenv whence [--path] <command>\n"
    );
    let listing = f.pyenv(&["help"]).stdout;
    assert!(listing.contains("   which       Display the full path to an executable\n"));
    assert!(listing
        .contains("   whence      List all Python versions that contain the given executable\n"));
}

#[cfg(windows)]
const WIN_ENV: [(&str, &str); 2] = [
    ("PYENV_VERSION", "3.9.1"),
    ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
];

#[cfg(windows)]
#[test]
fn win_which_and_not_found() {
    let f = Fixture::new();
    let py = f.exe("3.9.1/python.exe");
    f.exe("3.8.2/python38.exe");
    let r = f.pyenv_env(&["which", "python"], &WIN_ENV);
    assert_eq!((r.stdout, r.code), (format!("{}\r\n", py.display()), 0));
    let r = f.pyenv_env(&["which", "python38"], &WIN_ENV);
    assert_eq!(
        r.stdout,
        "pyenv: python38: command not found\r\n\r\n\
         The 'python38' command exists in these Python versions:\r\n  \
         3.8.2\r\n  \r\n"
    );
    assert_eq!(r.code, 127);
}

#[cfg(windows)]
#[test]
fn win_which_and_whence_without_a_program_print_help() {
    let f = Fixture::new();
    let r = f.pyenv(&["which"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "Usage: pyenv which <command>\r\n\r\nShows the full path of the executable\r\n\
             selected. To obtain the full path, use `pyenv which pip'.\r\n\r\n",
            1
        )
    );
    let r = f.pyenv(&["whence", "--path"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "Usage: pyenv whence [--path] <command>\r\n\r\nShows the currently given executable contains path\r\n\
             selected. To obtain python version of executable, use `pyenv whence pip'.\r\n\r\n",
            1
        )
    );
}

#[cfg(windows)]
#[test]
fn win_whence() {
    let f = Fixture::new();
    for v in ["3.8.2", "3.8.7", "3.9.1"] {
        f.exe(&format!("{v}/python.exe"));
    }
    f.exe("3.8.2/python38.exe");
    f.exe("3.8.7/python38.exe");
    assert_eq!(
        f.pyenv_env(&["whence", "python38"], &WIN_ENV).stdout,
        "3.8.2\r\n3.8.7\r\n"
    );
    let r = f.pyenv_env(&["whence", "unknown3.8"], &WIN_ENV);
    assert_eq!((r.stdout.as_str(), r.code), ("", 1));
}
