//! `pyenv uninstall` (docs/parity/pyenv-m2-reference.md "pyenv uninstall").
#![cfg(unix)]

mod common;
use common::Fixture;

const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> ...\n\n   -f  Attempt to remove the specified version without prompting\n       for confirmation. If the version does not exist, do not\n       display an error message.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";

#[test]
fn force_removes_and_rehashes() {
    let f = Fixture::new();
    f.exe("3.12.0/bin/onlyhere");
    f.version("3.11.0/bin");
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    assert!(f.root.join("shims/onlyhere").exists());
    let r = f.pyenv(&["uninstall", "-f", "3.12.0"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("pyenv: 3.12.0 uninstalled\n", "", 0)
    );
    assert!(!f.root.join("versions/3.12.0").exists());
    assert!(!f.root.join("shims/onlyhere").exists(), "rehashed");
}

#[test]
fn a_missing_version_stops_without_force_and_is_silent_with_it() {
    let f = Fixture::new();
    f.version("3.11.0");
    let r = f.pyenv(&["uninstall", "9.9", "3.11.0"]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("", "pyenv: version `9.9' not installed\n", 1)
    );
    assert!(
        f.root.join("versions/3.11.0").exists(),
        "later arguments not processed"
    );
    let r = f.pyenv(&["uninstall", "-f", "9.9", "3.11.0"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("pyenv: 3.11.0 uninstalled\n", 0)
    );
}

#[test]
fn without_force_eof_at_the_prompt_stops_and_removes_nothing() {
    let f = Fixture::new();
    f.version("3.11.0");
    let r = f.pyenv(&["uninstall", "3.11.0"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
    assert!(f.root.join("versions/3.11.0").exists());
}

#[test]
fn no_prefix_resolution_and_paths_lose_their_directories() {
    let f = Fixture::new();
    f.version("3.11.15").version("u4");
    assert_eq!(
        f.pyenv(&["uninstall", "3.11"]).stderr,
        "pyenv: version `3.11' not installed\n"
    );
    let r = f.pyenv(&["uninstall", "-f", "/some/where/u4"]);
    assert_eq!(r.stdout, "pyenv: u4 uninstalled\n");
}

#[test]
fn a_symlinked_version_loses_only_the_link() {
    let f = Fixture::new();
    let target = f.base.join("elsewhere");
    std::fs::create_dir_all(target.join("bin")).unwrap();
    std::os::unix::fs::symlink(&target, f.root.join("versions/linked")).unwrap();
    assert_eq!(f.pyenv(&["uninstall", "-f", "linked"]).code, 0);
    assert!(target.join("bin").is_dir());
}

#[test]
fn usage_errors_and_help() {
    let f = Fixture::new();
    f.version("u2").version("u3");
    for args in [
        &["uninstall"][..],
        &["uninstall", "-f", "u2", "--bogus", "u3"],
        &["uninstall", "u2", "-f"],
        &["uninstall", "-f", ""],
    ] {
        let r = f.pyenv(args);
        assert_eq!((r.stderr.as_str(), r.code), (HELP, 1), "{args:?}");
    }
    assert!(
        f.root.join("versions/u2").exists() && f.root.join("versions/u3").exists(),
        "checked before any removal"
    );
    let r = f.pyenv(&["uninstall", "--help"]);
    assert_eq!((r.stdout.as_str(), r.code), (HELP, 0));
}

/// The installer's `.tmp-*` and `.old-*` directories are hidden from `pyenv versions`; they
/// may belong to a running install, so uninstall treats them as absent. `.` and `..` would
/// name `versions/` or the root itself.
#[test]
fn staging_names_and_dot_names_are_not_installed() {
    let f = Fixture::new();
    f.version(".tmp-3.12.0")
        .version(".old-3.12.0")
        .version("3.11.0");
    for name in [".tmp-3.12.0", ".old-3.12.0", "..", "."] {
        let r = f.pyenv(&["uninstall", name]);
        let want = format!("pyenv: version `{name}' not installed\n");
        assert_eq!(
            (r.stdout.as_str(), r.stderr.as_str(), r.code),
            ("", want.as_str(), 1),
            "{name}"
        );
        let r = f.pyenv(&["uninstall", "-f", name]);
        assert_eq!(
            (r.stdout.as_str(), r.stderr.as_str(), r.code),
            ("", "", 0),
            "-f {name}"
        );
    }
    assert!(f.root.join("versions/.tmp-3.12.0").is_dir());
    assert!(f.root.join("versions/.old-3.12.0").is_dir());
    assert!(f.root.join("versions/3.11.0").is_dir());
}

/// Ctrl+C while `remove <prefix>? (y/N)` waits for a reply ends the run at once with 130
/// and removes nothing. stdin is a pipe that is never written to. The prompt text shows
/// only on a tty, so the test waits for the reply-reading helper thread to exist: the
/// process then has main, the Ctrl+C handler thread and that helper (3 entries in
/// /proc/<pid>/task), which happens after the handler is installed.
#[test]
fn ctrl_c_at_the_remove_prompt_exits_130() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    f.version("3.11.0");
    let mut child = f
        .command(
            std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
            &f.work,
            &[],
        )
        .args(["uninstall", "3.11.0", "3.12.0"])
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _held_open = child.stdin.take().unwrap();
    let tasks = format!("/proc/{}/task", child.id());
    let start = Instant::now();
    while std::fs::read_dir(&tasks).map(|d| d.count()).unwrap_or(0) < 3 {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the prompt was never reached"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::process::Command::new("kill")
        .args(["-INT", &format!("-{}", child.id())])
        .status()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if start.elapsed() > Duration::from_secs(3) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert_eq!(
        status.and_then(|st| st.code()),
        Some(130),
        "exit within 3 s of SIGINT"
    );
    assert!(f.root.join("versions/3.11.0").is_dir());
}
