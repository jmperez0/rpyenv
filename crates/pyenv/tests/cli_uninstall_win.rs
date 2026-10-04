//! `pyenv uninstall` (pyenv-win flavor; reference "uninstall"; review focus 4).
#![cfg(windows)]

mod common;
use common::Fixture;
use std::io::Write;
use std::process::Stdio;

const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> [<version> ...]\r\n       pyenv uninstall [-f|--force] [-a|--all]\r\n\r\n   -f/--force  Attempt to remove the specified version without prompting\r\n               for confirmation. If the version does not exist, do not\r\n               display an error message.\r\n\r\n   -a/--all    *Caution* Attempt to remove all installed versions.\r\n\r\nSee `pyenv versions` for a complete list of installed versions.\r\n\r\n";

fn with(versions: &[&str]) -> Fixture {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("versions")).unwrap();
    for v in versions {
        f.version(v);
        std::fs::write(f.root.join("versions").join(v).join("python.exe"), "").unwrap();
    }
    f
}

fn with_stdin(f: &Fixture, args: &[&str], input: &[u8]) -> (i32, String) {
    let mut child = f
        .command(
            std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
            &f.work,
            &[],
        )
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), common::decode(&out.stdout))
}

#[test]
fn help_and_no_arguments_print_help_and_exit_0() {
    let f = with(&["3.12.1"]);
    for args in [&["uninstall", "--help"][..], &["uninstall"][..]] {
        let r = f.pyenv(args);
        assert_eq!((r.code, r.stdout), (0, HELP.to_string()), "{args:?}");
    }
}

#[test]
fn messages_and_exit_codes() {
    let f = with(&[]);
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!(
        (r.code, r.stdout),
        (
            1,
            "pyenv: No valid versions of python installed.\r\n".to_string()
        )
    );
    let f = with(&["3.12.1"]);
    let r = f.pyenv(&["uninstall", "bad!name"]);
    assert_eq!(
        (r.code, r.stdout),
        (
            1,
            "pyenv: Unrecognized python version: bad!name\r\n".to_string()
        )
    );
    let r = f.pyenv(&["uninstall", "9.9"]);
    assert_eq!(
        (r.code, r.stdout),
        (0, "pyenv: version '9.9' not installed\r\n".to_string())
    );
    let r = f.pyenv(&["uninstall", "9.9", "9.8"]);
    assert_eq!(
        (r.code, r.stdout),
        (0, String::new()),
        "several missing: silent"
    );
}

// allowlist D-80
#[test]
fn every_version_is_removed_with_no_false_errors_and_shims_follow() {
    let f = with(&["9.9.4", "9.9.5", "9.9.6", "3.12.1"]);
    assert_eq!(f.pyenv(&["rehash"]).code, 0);
    let r = f.pyenv(&["uninstall", "9.9.4", "9.9.5", "9.9.6"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(r.stdout, "pyenv: Successfully uninstalled 9.9.4\r\npyenv: Successfully uninstalled 9.9.5\r\npyenv: Successfully uninstalled 9.9.6\r\n");
    for v in ["9.9.4", "9.9.5", "9.9.6"] {
        assert!(!f.root.join("versions").join(v).exists());
    }
    assert!(f.root.join("versions").join("3.12.1").is_dir());
}

#[test]
fn on_x86_names_get_the_win32_suffix() {
    let f = with(&["9.9.7-win32"]);
    let r = f.pyenv_env(&["uninstall", "9.9.7"], &[("PYENV_FORCE_ARCH", "X86")]);
    assert_eq!(
        (r.code, r.stdout),
        (
            0,
            "pyenv: Successfully uninstalled 9.9.7-win32\r\n".to_string()
        )
    );
}

/// Review focus 4: the root sits three levels inside a tempdir with a canary at every level;
/// no argument may remove anything but a version folder.
// allowlist D-80
#[test]
fn escaping_names_remove_nothing() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("a").join("b").join("root");
    let mut canaries = vec![t.path().join("canary")];
    for dir in [
        t.path().join("a"),
        t.path().join("a").join("b"),
        root.clone(),
        root.join("versions"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        canaries.push(dir.join("canary"));
    }
    for c in &canaries {
        std::fs::write(c, "").unwrap();
    }
    std::fs::create_dir_all(root.join("versions").join("3.12.1")).unwrap();
    std::fs::create_dir_all(root.join("versions").join(".tmp-3.12.2")).unwrap();
    std::fs::create_dir_all(root.join("versions").join(".old-3.12.1")).unwrap();
    std::fs::create_dir_all(root.join("versions").join(".del-3.12.1-7")).unwrap();
    let pyenv = std::path::Path::new(env!("CARGO_BIN_EXE_pyenv"));
    for arg in [
        "..",
        ".",
        "a\\b",
        "..\\..",
        "C:\\x",
        ".tmp-3.12.2",
        ".old-3.12.1",
        "versions",
        "..\\versions",
        "...",
        "....",
        "3.12.1.",
        "3.12.1 ",
        "con",
        "CON.txt",
        ".TMP-3.12.2",
        ".Old-3.12.1",
        ".del-3.12.1-7",
        ".DEL-3.12.1-7",
    ] {
        let out = std::process::Command::new(pyenv)
            .args(["uninstall", "-f", arg])
            .env_clear()
            .env("PYENV_ROOT", &root)
            .env("PYENV", &root)
            .env("PYENV_HOME", &root)
            .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
            .current_dir(t.path())
            .output()
            .unwrap();
        let text = common::decode(&out.stdout);
        assert!(!text.contains("Successfully"), "{arg}: {text}");
        assert!(
            text.contains("not installed") || text.contains("Unrecognized python version"),
            "{arg}: {text}"
        );
        for c in &canaries {
            assert!(c.is_file(), "{arg} removed {}", c.display());
        }
        assert!(root.join("versions").join("3.12.1").is_dir(), "{arg}");
        assert!(root.join("versions").join(".tmp-3.12.2").is_dir(), "{arg}");
        assert!(root.join("versions").join(".old-3.12.1").is_dir(), "{arg}");
        assert!(
            root.join("versions").join(".del-3.12.1-7").is_dir(),
            "{arg}"
        );
    }
}

// allowlist D-81
#[test]
fn all_asks_first_and_eof_or_no_keeps_everything() {
    for (input, gone) in [
        (&b"n\r\n"[..], false),
        (b"\r\n", false),
        (b"", false),
        (b"x\r\nY\r\n", true),
    ] {
        let f = with(&["9.9.3", "9.9.8"]);
        let (code, out) = with_stdin(&f, &["uninstall", "-a"], input);
        assert_eq!(code, 0, "{input:?}: {out}");
        let prompts = out.matches("pyenv: Confirm uninstall all? (Y/N): ").count();
        assert_eq!(
            prompts,
            if input.starts_with(b"x") { 2 } else { 1 },
            "{input:?}: {out}"
        );
        assert_eq!(
            !f.root.join("versions").join("9.9.3").exists(),
            gone,
            "{input:?}"
        );
    }
    let f = with(&["9.9.3"]);
    let r = f.pyenv(&["uninstall", "-a", "-f"]);
    assert_eq!(
        (r.code, r.stdout),
        (0, "pyenv: Successfully uninstalled 9.9.3\r\n".to_string())
    );
}

#[test]
fn an_install_in_progress_keeps_its_version() {
    let f = with(&["3.12.1"]);
    let _held = pyenv::install::txn::Txn::begin_for(
        &f.root.join("versions"),
        "3.12.1",
        rpyenv_core::flavor::Flavor::PyenvWin,
    )
    .unwrap();
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!(r.code, 1);
    assert!(
        r.stdout
            .starts_with("pyenv: an install of 3.12.1 is in progress"),
        "{}",
        r.stdout
    );
    assert!(f.root.join("versions").join("3.12.1").is_dir());
}

// allowlist D-80
#[test]
fn a_read_only_file_does_not_stop_the_removal() {
    let f = with(&["3.12.1"]);
    let p = f.root.join("versions").join("3.12.1").join("python.exe");
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&p, perm).unwrap();
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!(
        (r.code, r.stdout),
        (0, "pyenv: Successfully uninstalled 3.12.1\r\n".to_string())
    );
}

/// `.del-*` names left in `versions\` by uninstall's rename-then-delete.
fn del_dirs(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".del-"))
        .collect()
}

#[test]
fn a_normal_uninstall_leaves_no_renamed_folder() {
    let f = with(&["3.12.1", "3.12.2"]);
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    assert_eq!(
        (r.code, r.stdout),
        (0, "pyenv: Successfully uninstalled 3.12.1\r\n".to_string())
    );
    assert!(!f.root.join("versions").join("3.12.1").exists());
    assert_eq!(del_dirs(&f), Vec::<String>::new());
}

/// Final review M1: a running `python.exe` maps its image, which blocks deleting the file but
/// not renaming its folder (measured: an open file handle, in any share mode, blocks the folder
/// rename too, so a running program is what reproduces the reviewer's case). The version is
/// renamed away first, so it is gone from pyenv even though a file stays behind.
// allowlist D-80
#[test]
fn a_running_python_blocks_the_delete_but_not_the_uninstall() {
    let f = with(&["3.12.1", "3.12.2"]);
    let v = f.root.join("versions").join("3.12.1");
    let exe = v.join("python.exe");
    let cmd = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe");
    std::fs::copy(cmd, &exe).unwrap();
    std::fs::write(v.join("other.txt"), "x").unwrap();
    // cmd reads commands from the piped stdin, so it runs until the pipe closes. Its working
    // directory is outside the version: a working directory is an open handle.
    let mut child = std::process::Command::new(&exe)
        .current_dir(&f.work)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    let left = del_dirs(&f);
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].starts_with(".del-3.12.1-"), "{left:?}");
    let leftover = f.root.join("versions").join(&left[0]);
    assert_eq!(
        r.stdout,
        format!(
            "pyenv: Successfully uninstalled 3.12.1\r\npyenv: could not remove every file of 3.12.1; leftover at {}\r\n",
            leftover.display()
        )
    );
    assert!(!v.exists());
    let listed = f.pyenv(&["versions"]);
    assert!(!listed.stdout.contains("3.12.1"), "{}", listed.stdout);
    assert!(listed.stdout.contains("3.12.2"), "{}", listed.stdout);
    // The program has exited: the leftover can go now.
    let mut gone = false;
    for _ in 0..50 {
        if std::fs::remove_dir_all(&leftover).is_ok() {
            gone = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(gone, "{}", leftover.display());
}

/// A file held open blocks the rename: nothing is deleted and the version stays installed
/// (before the rename-first fix, part of the tree was deleted).
// allowlist D-80
#[test]
fn a_held_file_stops_the_uninstall_before_anything_is_deleted() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = with(&["3.12.1"]);
    let v = f.root.join("versions").join("3.12.1");
    std::fs::write(v.join("a.txt"), "a").unwrap();
    std::fs::write(v.join("z.txt"), "z").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(v.join("z.txt"))
        .unwrap();
    let r = f.pyenv(&["uninstall", "3.12.1"]);
    drop(held);
    assert_eq!(r.code, 1, "{}", r.stdout);
    assert!(
        r.stdout
            .starts_with("pyenv: Error uninstalling version 3.12.1: "),
        "{}",
        r.stdout
    );
    for n in ["python.exe", "a.txt", "z.txt"] {
        assert!(v.join(n).is_file(), "{n}");
    }
    assert_eq!(del_dirs(&f), Vec::<String>::new());
}
