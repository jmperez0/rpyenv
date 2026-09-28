mod common;
use common::{nl, Fixture};

#[cfg(unix)]
#[test]
fn nothing_selected() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["version"]).stdout,
        format!("system (set by {}/version)\n", f.root.display())
    );
    assert_eq!(f.pyenv(&["version-name"]).stdout, "system\n");
    assert_eq!(
        f.pyenv(&["version-origin"]).stdout,
        format!("{}/version\n", f.root.display())
    );
    assert_eq!(
        f.pyenv(&["version-file"]).stdout,
        format!("{}/version\n", f.root.display())
    );
}

#[cfg(unix)]
#[test]
fn env_and_local_file() {
    let f = Fixture::new();
    f.version("3.12.10").version("3.11.9");
    let r = f.pyenv_env(&["version"], &[("PYENV_VERSION", "3.12")]);
    assert_eq!(
        r.stdout,
        "3.12.10 (set by PYENV_VERSION environment variable)\n"
    );
    f.file(&f.work.join(".python-version"), "3.11.9\n");
    assert_eq!(
        f.pyenv(&["version"]).stdout,
        format!("3.11.9 (set by {}/.python-version)\n", f.work.display())
    );
    assert_eq!(f.pyenv(&["version", "--bare"]).stdout, "3.11.9\n");
    assert_eq!(
        f.pyenv(&["version-file"]).stdout,
        format!("{}/.python-version\n", f.work.display())
    );
}

#[cfg(unix)]
#[test]
fn missing_versions() {
    let f = Fixture::new();
    f.version("3.12.10");
    let r = f.pyenv_env(&["version"], &[("PYENV_VERSION", "9.9")]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 1));
    assert_eq!(
        r.stderr,
        "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n"
    );
    let r = f.pyenv_env(&["version-name"], &[("PYENV_VERSION", "3.12.10:9.9")]);
    assert_eq!((r.stdout.as_str(), r.code), ("3.12.10\n", 1));
    assert_eq!(
        f.pyenv_env(&["version-name"], &[("PYENV_VERSION", "8.8:9.9")])
            .stdout,
        "\n"
    );
}

#[cfg(unix)]
#[test]
fn version_rejects_unknown_arguments() {
    let r = Fixture::new().pyenv(&["version", "--foo"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Usage: pyenv version [--bare]\n", 1)
    );
}

#[cfg(unix)]
#[test]
fn trailing_slash_in_pyenv_root_is_not_doubled() {
    // Review focus 1.
    let f = Fixture::new();
    let r = f.pyenv_env(
        &["version"],
        &[("PYENV_ROOT", &format!("{}/", f.root.display()))],
    );
    assert_eq!(
        r.stdout,
        format!("system (set by {}/version)\n", f.root.display())
    );
}

#[test]
fn bom_and_crlf_in_a_local_file() {
    // Review focus 3.
    let f = Fixture::new();
    f.version("3.12.10");
    f.file(&f.work.join(".python-version"), "\u{feff}3.12.10\r\n");
    assert_eq!(f.pyenv(&["version-name"]).stdout, nl("3.12.10\n"));
}

#[cfg(unix)]
#[test]
fn version_file_with_a_directory_argument() {
    let f = Fixture::new();
    let sub = f.work.join("sub");
    f.file(&sub.join(".python-version"), "3.12\n");
    assert_eq!(
        f.pyenv(&["version-file", "sub"]).stdout,
        format!("{}/.python-version\n", sub.display())
    );
    assert_eq!(f.pyenv(&["version-file", "missing-dir"]).code, 1);
}

#[cfg(unix)]
#[test]
fn version_file_read_and_write() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    f.file(&f.work.join("vf"), "3.12.1\r\n3.11.9\r\n");
    assert_eq!(
        f.pyenv(&["version-file-read", "vf"]).stdout,
        "3.12.1:3.11.9\n"
    );
    f.file(&f.work.join("empty"), "");
    assert_eq!(f.pyenv(&["version-file-read", "empty"]).code, 1);
    assert_eq!(
        f.pyenv(&["version-file-write", "out", "3.12.1", "3.11.9"])
            .code,
        0
    );
    assert_eq!(
        std::fs::read_to_string(f.work.join("out")).unwrap(),
        "3.12.1\n3.11.9\n"
    );
    let r = f.pyenv(&["version-file-write", "out", "9.9"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: version `9.9' not installed\n", 1)
    );
    assert_eq!(
        std::fs::read_to_string(f.work.join("out")).unwrap(),
        "3.12.1\n3.11.9\n"
    );
    let r = f.pyenv(&["version-file-write", "out"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "Usage: pyenv version-file-write [-f|--force] <file> <version> [...]\n",
            1
        )
    );
}

#[cfg(windows)]
#[test]
fn win_nothing_selected() {
    let r = Fixture::new().pyenv(&["version"]);
    assert_eq!(r.code, 1);
    assert_eq!(
        r.stdout,
        "No global/local python version has been set yet. Please set the global/local version by typing:\r\npyenv global <python-version>\r\npyenv global 3.7.4\r\npyenv local <python-version>\r\npyenv local 3.7.4\r\n"
    );
}

#[cfg(windows)]
#[test]
fn win_global_local_and_env() {
    let f = Fixture::new();
    f.version("3.9.1").version("3.8.2");
    f.file(&f.root.join("version"), "3.9\r\n");
    assert_eq!(
        f.pyenv(&["version"]).stdout,
        format!("3.9.1 (set by {}\\version)\r\n", f.root.display())
    );
    let local = f.work.join(".python-version");
    f.file(&local, "3.8\r\n3.9.1\r\n");
    assert_eq!(
        f.pyenv(&["version"]).stdout,
        format!(
            "3.8.2 (set by {0})\r\n3.9.1 (set by {0})\r\n",
            local.display()
        )
    );
    assert_eq!(f.pyenv(&["vname"]).stdout, "3.8.2\r\n3.9.1\r\n");
    let r = f.pyenv_env(&["version"], &[("PYENV_VERSION", "3.9.2 3.8.1")]);
    assert_eq!(
        r.stdout,
        "3.9.2 (set by %PYENV_VERSION%)\r\n3.8.1 (set by %PYENV_VERSION%)\r\n"
    );
}

#[cfg(windows)]
#[test]
fn win_trailing_backslash_in_pyenv_root_is_not_doubled() {
    // Review focus 1: pyenv-win's installer sets PYENV_ROOT with a trailing backslash.
    let f = Fixture::new();
    f.version("3.9.1");
    f.file(&f.root.join("version"), "3.9.1\r\n");
    let r = f.pyenv_env(
        &["version"],
        &[("PYENV_ROOT", &format!("{}\\", f.root.display()))],
    );
    assert_eq!(
        r.stdout,
        format!("3.9.1 (set by {}\\version)\r\n", f.root.display())
    );
}

#[cfg(windows)]
#[test]
fn win_path_check_warns_when_the_shim_is_not_on_path() {
    let f = Fixture::new();
    f.version("3.9.1");
    f.file(&f.root.join("version"), "3.9.1\r\n");
    f.file(&f.root.join("shims").join("python.exe"), "");
    let other = f.python_on_path();
    let expected = format!(
        "\x1b[91mFATAL: Found \x1b[95m{}\x1b[91m version before pyenv in PATH.\x1b[0m\r\n\x1b[91mPlease remove \x1b[95m{}\\\x1b[91m from PATH for pyenv to work properly.\x1b[0m\r\n3.9.1 (set by {}\\version)\r\n",
        other.display(),
        f.syspath.display(),
        f.root.display()
    );
    assert_eq!(f.pyenv(&["version"]).stdout, expected);
}
