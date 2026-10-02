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
fn win_version_file_read_and_write() {
    let f = Fixture::new();
    f.version("3.12.1").version("3.11.9");
    f.file(&f.work.join("vf"), "3.12.1\r\n3.11.9\r\n");
    assert_eq!(
        f.pyenv(&["version-file-read", "vf"]).stdout,
        "3.12.1:3.11.9\r\n"
    );
    f.file(&f.work.join("empty"), "");
    let r = f.pyenv(&["version-file-read", "empty"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
    let r = f.pyenv(&["version-file-read", "missing"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
    assert_eq!(
        f.pyenv(&["version-file-write", "out", "3.12.1", "3.11.9"])
            .code,
        0
    );
    assert_eq!(
        std::fs::read(f.work.join("out")).unwrap(),
        b"3.12.1\r\n3.11.9\r\n"
    );
    let r = f.pyenv(&["version-file-write", "out", "9.9"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("", "pyenv: version `9.9' not installed\r\n", 1)
    );
    assert_eq!(
        std::fs::read(f.work.join("out")).unwrap(),
        b"3.12.1\r\n3.11.9\r\n"
    );
    let r = f.pyenv(&["version-file-write", "out"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        (
            "",
            "Usage: pyenv version-file-write [-f|--force] <file> <version> [...]\r\n",
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
fn win_version_file_and_origin_agree_with_version_when_local_is_empty() {
    // Fix 1 (I-1): an empty local .python-version must fall through to the global
    // file for version-file and version-origin, the same as it already does for
    // version and version-name.
    let f = Fixture::new();
    f.version("3.9.1");
    f.file(&f.root.join("version"), "3.9.1\r\n");
    f.file(&f.work.join(".python-version"), "");
    let expected = format!("{}\\version\r\n", f.root.display());
    assert_eq!(f.pyenv(&["version-file"]).stdout, expected);
    assert_eq!(f.pyenv(&["version-origin"]).stdout, expected);
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

#[cfg(windows)]
#[test]
fn win_path_check_ignores_a_dot_dot_path_entry() {
    // Fix 2 (I-2): a PATH entry that reaches the shims directory through `..`
    // must not read as "a different python", i.e. no FATAL warning.
    let f = Fixture::new();
    f.version("3.9.1");
    f.file(&f.root.join("version"), "3.9.1\r\n");
    f.file(&f.root.join("shims").join("python.exe"), "");
    let root_name = f.root.file_name().unwrap().to_string_lossy();
    let path = format!("{}\\..\\{}\\shims", f.root.display(), root_name);
    let r = f.pyenv_env(&["version"], &[("PATH", &path)]);
    assert!(!r.stdout.contains("FATAL"), "stdout: {}", r.stdout);
    assert_eq!(
        r.stdout,
        format!("3.9.1 (set by {}\\version)\r\n", f.root.display())
    );
}

#[cfg(windows)]
#[test]
fn win_path_check_ignores_forward_slashes_in_pyenv_root() {
    // Fix 2 (I-2): a forward-slash PYENV_ROOT must still match a backslash PATH
    // entry, and `pyenv root` must print backslashes.
    let f = Fixture::new();
    f.version("3.9.1");
    f.file(&f.root.join("version"), "3.9.1\r\n");
    f.file(&f.root.join("shims").join("python.exe"), "");
    let root_fwd = f.root.display().to_string().replace('\\', "/");
    let shims = f.root.join("shims");
    let r = f.pyenv_env(
        &["version"],
        &[
            ("PYENV_ROOT", root_fwd.as_str()),
            ("PATH", &shims.display().to_string()),
        ],
    );
    assert!(!r.stdout.contains("FATAL"), "stdout: {}", r.stdout);
    let root_out = f.pyenv_env(&["root"], &[("PYENV_ROOT", root_fwd.as_str())]);
    assert_eq!(root_out.stdout, format!("{}\r\n", f.root.display()));
}

/// Review focus 3. The fixture's root holds `ñ`. With the console's code page pinned in a
/// console of the test's own, redirected output must be in that code page, as cmd.exe and
/// pyenv-win write it: `ñ` is 0xA4 in code page 850 and 0xC3 0xB1 in UTF-8 (65001).
#[cfg(windows)]
#[test]
fn win_redirected_output_uses_the_console_code_page() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let f = Fixture::new();
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let pyenv = env!("CARGO_BIN_EXE_pyenv");
    let run = |cp: u32| {
        f.command(&system.join("cmd.exe"), &f.work, &[])
            .raw_arg(format!(
                r#"/d /s /c ""{}" {cp} >nul & "{pyenv}" root""#,
                system.join("chcp.com").display()
            ))
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap()
            .stdout
    };
    let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    let oem = run(850);
    assert!(has(&oem, b"py env \xa4\\root\r\n"), "{oem:?}");
    let utf8 = run(65001);
    assert!(has(&utf8, "py env ñ\\root\r\n".as_bytes()), "{utf8:?}");
}
