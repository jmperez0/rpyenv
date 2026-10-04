mod common;
use common::Fixture;

#[cfg(unix)]
#[test]
fn exec_runs_the_file_with_the_upstream_environment() {
    let f = Fixture::new();
    let tool = f.exe("3.12.10/bin/tool");
    std::fs::write(
        &tool,
        "#!/bin/sh\nprintf '%s|' \"$0\" \"$@\" \"$PYENV_VERSION\"\nprintf '%s\\n' \"${PATH%%:*}\"\n",
    )
    .unwrap();
    let r = f.pyenv_env(&["exec", "tool", "a b", ""], &[("PYENV_VERSION", "3.12")]);
    assert_eq!(
        (r.stdout, r.code),
        (
            format!(
                "{t}|a b||3.12.10|{b}\n",
                t = tool.display(),
                b = tool.parent().unwrap().display()
            ),
            0
        )
    );
}

/// A file that exists but can't be started gets `pyenv: <path>: <reason>` on stderr
/// instead of bash's errors, and exit 126 as in bash: here its `#!` interpreter is missing
/// (bash's `bad interpreter`) (allowlist D-43). A CRLF `#!` line names `/bin/sh\r`, which
/// bash shows as `/bin/sh^M`. A file without `#!` is no trigger: `execvp` hands it to
/// `/bin/sh`. Exit 127, for a file that is itself gone, is pinned in `launch.rs`.
#[cfg(unix)]
#[test]
fn exec_start_failure_exits_126() {
    let f = Fixture::new();
    for (name, script, interp) in [
        (
            "orphan",
            "#!/nonexistent/interpreter\n",
            "/nonexistent/interpreter",
        ),
        ("crlf", "#!/bin/sh\r\necho hi\r\n", "/bin/sh^M"),
    ] {
        let path = f.exe(&format!("3.12.10/bin/{name}"));
        std::fs::write(&path, script).unwrap();
        let r = f.pyenv_env(&["exec", name], &[("PYENV_VERSION", "3.12.10")]);
        assert_eq!(
            (r.stderr, r.code),
            (
                format!(
                    "pyenv: {}: {interp}: bad interpreter: No such file or directory\n",
                    path.display()
                ),
                126
            ),
            "{name}"
        );
    }
}

/// A file only group and others may run doesn't count for its owner: upstream's `[ -x ]`
/// checks the caller's access, so the lookup gives `command not found`, exit 127, before
/// anything is started (allowlist D-29). Root may run any file with an x bit, as `-x`
/// says too, so the check is skipped there.
#[cfg(unix)]
#[test]
fn a_file_the_caller_may_not_run_is_not_found() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let locked = f.exe("3.12.10/bin/locked");
    std::fs::write(&locked, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o011)).unwrap();
    if std::fs::read(&locked).is_ok() {
        eprintln!("running as root: skipping a_file_the_caller_may_not_run_is_not_found");
        return;
    }
    for cmd in ["exec", "which"] {
        let r = f.pyenv_env(&[cmd, "locked"], &[("PYENV_VERSION", "3.12.10")]);
        assert_eq!(
            (r.stdout.as_str(), r.stderr.as_str(), r.code),
            ("", "pyenv: locked: command not found\n", 127),
            "{cmd}"
        );
    }
}

#[cfg(unix)]
#[test]
fn exec_usage_and_not_found() {
    let f = Fixture::new();
    let r = f.pyenv(&["exec"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "Usage: pyenv exec [-N|--environment] <command> [arg1 arg2...]\n",
            1
        )
    );
    let r = f.pyenv(&["exec", "-N"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "Usage: pyenv exec [-N|--environment] <command> [arg1 arg2...]\n",
            1
        )
    );
    let r = f.pyenv_env(&["exec", "tool"], &[("PYENV_VERSION", "9.9")]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)\n\
             pyenv: tool: command not found\n",
            127
        )
    );
}

/// `exec -N` (pyenv 2.8.8, test/exec.bats "--environment sets envvars in Linux" and "in
/// macOS"): PYTHONHOME is the version's prefix, and `<prefix>/lib` goes first on the
/// library path, which `uname -s` picks.
#[cfg(unix)]
#[test]
fn exec_environment_sets_python_home_and_the_library_path() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let tool = f.exe("3.12.10/bin/print_env");
    std::fs::write(
        &tool,
        "#!/bin/sh\necho PYTHONHOME=\"$PYTHONHOME\"\necho LD=\"$LD_LIBRARY_PATH\"\necho DYLD=\"$DYLD_LIBRARY_PATH\"\n",
    )
    .unwrap();
    let home = f.root.join("versions").join("3.12.10");
    let home = home.display();
    // A sibling test's fork can briefly hold a just-written script open, so exec fails
    // with ETXTBSY (exit 126, ubuntu-latest CI run 37242317356). Retry while that lasts.
    let run = |lib: &str| {
        for _ in 0..100 {
            let r = f.pyenv_env(
                &["exec", "-N", "print_env"],
                &[
                    ("PYENV_VERSION", "3.12.10"),
                    ("LD_LIBRARY_PATH", lib),
                    ("DYLD_LIBRARY_PATH", lib),
                ],
            );
            if !(r.code == 126 && r.stderr.contains("Text file busy")) {
                assert_eq!(r.code, 0, "{}", r.stderr);
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("print_env stayed busy");
    };
    let r = run("");
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            format!("PYTHONHOME={home}\nLD={home}/lib\nDYLD=\n").as_str(),
            0
        )
    );
    let r = run("/foo/bar");
    assert_eq!(
        r.stdout,
        format!("PYTHONHOME={home}\nLD={home}/lib:/foo/bar\nDYLD=/foo/bar\n")
    );
    // A `uname` on PATH that says Darwin switches to macOS's variable, as upstream's does.
    let uname = f.syspath.join("uname");
    std::fs::create_dir_all(&f.syspath).unwrap();
    std::fs::write(&uname, "#!/bin/sh\necho Darwin\n").unwrap();
    std::fs::set_permissions(&uname, std::fs::Permissions::from_mode(0o755)).unwrap();
    let r = run("/foo/bar");
    assert_eq!(
        r.stdout,
        format!("PYTHONHOME={home}\nLD=/foo/bar\nDYLD={home}/lib:/foo/bar\n")
    );
    // Without -N nothing is set.
    let r = f.pyenv_env(&["exec", "print_env"], &[("PYENV_VERSION", "3.12.10")]);
    assert_eq!(r.stdout, "PYTHONHOME=\nLD=\nDYLD=\n");
}

/// Several versions: the first one's prefix, with upstream's warning. A selected version
/// that isn't installed fails as `pyenv-prefix` does, after the command is found.
#[cfg(unix)]
#[test]
fn exec_environment_with_several_versions() {
    let f = Fixture::new();
    let tool = f.exe("3.12.10/bin/print_home");
    std::fs::write(&tool, "#!/bin/sh\necho \"$PYTHONHOME\"\n").unwrap();
    std::fs::create_dir_all(f.root.join("versions").join("3.11.9")).unwrap();
    let r = f.pyenv_env(
        &["exec", "--environment", "print_home"],
        &[("PYENV_VERSION", "3.12.10:3.11.9")],
    );
    let home = f.root.join("versions").join("3.12.10");
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        (
            format!("{}\n", home.display()).as_str(),
            "pyenv: Warning: multiple Python versions are selected. Setting environment variables for the first one, (/versions/3.12.10)\n",
            0
        )
    );
    let r = f.pyenv_env(
        &["exec", "-N", "print_home"],
        &[("PYENV_VERSION", "3.12.10:9.9")],
    );
    assert_eq!((r.stdout.as_str(), r.code), ("", 1), "stderr: {}", r.stderr);
    assert!(
        r.stderr.ends_with("pyenv: version `9.9' not installed\n"),
        "{}",
        r.stderr
    );
}

/// Review M-5: arguments that aren't UTF-8 reach the program unchanged.
#[cfg(unix)]
#[test]
fn exec_passes_non_utf8_arguments() {
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new();
    let tool = f.exe("3.12.10/bin/tool");
    std::fs::write(&tool, "#!/bin/sh\nprintf '%s' \"$1\"\n").unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pyenv"))
        .arg("exec")
        .arg("tool")
        .arg(std::ffi::OsStr::from_bytes(b"a\xffb"))
        .current_dir(&f.work)
        .env_clear()
        .env("PYENV_ROOT", &f.root)
        .env("PYENV_VERSION", "3.12.10")
        .output()
        .unwrap();
    assert_eq!(out.stdout, b"a\xffb");
}

#[cfg(unix)]
#[test]
fn help_for_exec() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["exec", "--help"]).stdout,
        "Usage: pyenv exec [-N|--environment] <command> [arg1 arg2...]\n\n\
         \x20  -N/--environment   Set PYTHONHOME and LD_LIBRARY_PATH (DYLD_LIBRARY_PATH in macOS)\n\
         \x20                 envvars for the running command.\n\
         \x20                 This allows to run programs that embed Python without using rpath or\n\
         \x20                 exact library path to libpython.\n\
         \x20                 WARNING: For the running command and its child processes, this will\n\
         \x20                 break linkage for programs that expect to be linked to a different\n\
         \x20                 libpython instance with the same name!\n\
         \x20                 WARNING: In macOS, DYLD_LIBRARY_PATH will be unset by the system for\n\
         \x20                 processes covered by System Integrity Protection.\n\n\
         Runs an executable by first preparing PATH so that the selected Python\n\
         version's `bin' directory is at the front.\n\n\
         For example, if the currently selected Python version is 2.7.6:\n  \
         pyenv exec pip install -r requirements.txt\n\n\
         is equivalent to:\n  \
         PATH=\"$PYENV_ROOT/versions/2.7.6/bin:$PATH\" pip install -r requirements.txt\n\n"
    );
}

#[cfg(windows)]
const WIN_ENV: [(&str, &str); 2] = [
    ("PYENV_VERSION", "3.9.1"),
    ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
];

#[cfg(windows)]
#[test]
fn win_exec_messages() {
    let f = Fixture::new();
    let r = f.pyenv(&["exec", "python"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "No global/local python version has been set yet. Please set the global/local version by typing:\r\n\
             pyenv global 3.7.4\r\npyenv local 3.7.4\r\n",
            1
        )
    );
    f.version("3.9.1");
    let r = f.pyenv_env(&["exec", "nothing"], &WIN_ENV);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            "'nothing' is not recognized as an internal or external command,\r\n\
             operable program or batch file.\r\n",
            1
        )
    );
    let r = f.pyenv_env(&["exec"], &WIN_ENV);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("Usage: pyenv exec <command> [arg1 arg2...]\r\n", 1)
    );
}

#[cfg(windows)]
#[test]
fn win_exec_runs_a_batch_file() {
    let f = Fixture::new();
    let bat = f.exe("3.9.1/Scripts/hello.bat");
    std::fs::write(&bat, "@echo hello %1\r\n").unwrap();
    let r = f.pyenv_env(&["exec", "hello", "world"], &WIN_ENV);
    assert_eq!((r.stdout.as_str(), r.code), ("hello world\r\n", 0));
}

/// The debug log names `pyenv exec` as the source of what that command writes to it.
#[test]
fn exec_debug_log_lines_name_pyenv_exec() {
    let f = Fixture::new();
    f.version("3.12.1");
    let log = f.base.join("debug.log");
    let r = f.pyenv_env(
        &["exec", "nosuchcmd"],
        &[
            ("PYENV_VERSION", "3.12.1"),
            ("RPYENV_DEBUG_LOG", log.to_str().unwrap()),
        ],
    );
    assert_eq!(r.code, if cfg!(windows) { 1 } else { 127 });
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|l| l.starts_with("pyenv exec: ")),
        "{text}"
    );
    assert!(lines.iter().any(|l| l.contains("nosuchcmd")), "{text}");
}

/// The folder of a real `python.exe` on this process's PATH (not a WindowsApps alias).
#[cfg(windows)]
fn real_python_dir() -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|d| !d.to_string_lossy().contains("WindowsApps"))
        .find(|d| d.join("python.exe").is_file())
}

/// pyenv-win's `test_exec_arg` arguments (tests/test_pyenv_feature_exec.py), passed to
/// rpyenv's `pyenv exec python` from cmd, as a user types it, and directly. Through cmd,
/// `%World%` expands and `!World!` doesn't (no delayed expansion); directly, nothing does.
/// The suite's own variant calls `bin\pyenv.bat`, which rpyenv doesn't ship (D-92).
#[cfg(windows)]
#[test]
fn exec_passes_pyenv_win_s_argument_cases_unchanged() {
    use common::winshell::{self, host, output};
    let Some(python_dir) = real_python_dir() else {
        eprintln!("skipped: no python.exe on PATH");
        return;
    };
    let f = Fixture::new();
    // A copy of `python.exe` and the DLLs beside it, never a link: nothing that later cleans
    // up the fixture, even after a killed run, can reach the real install, which this test
    // only reads (its standard library, through PYTHONHOME).
    let version = f.root.join("versions").join("3.99");
    std::fs::create_dir_all(&version).unwrap();
    for entry in std::fs::read_dir(&python_dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        if path.is_file() && (name == "python.exe" || name.ends_with(".dll")) {
            std::fs::copy(&path, version.join(path.file_name().unwrap())).unwrap();
        }
    }
    winshell::install_pyenv(&f);
    let home = python_dir.display().to_string();
    let env = [
        ("PYENV_VERSION", "3.99"),
        ("World", "Earth"),
        ("PYTHONHOME", home.as_str()),
    ];
    let script = "import sys; print(sys.argv[1])";
    for arg in [
        "Hello",
        "Hello World",
        "Hello 'World'",
        "Hello \"World\"",
        "Hello %World%",
        "Hello !World!",
        "Hello #World#",
        "Hello World'",
        "Hello World\"",
        "Hello ''World'",
        "Hello \"\"World\"",
    ] {
        let mut c = host(&f, &winshell::cmd_exe(), &env, &[]);
        c.args([
            "/d", "/c", "call", "pyenv", "exec", "python", "-c", script, arg,
        ]);
        let (out, err, _) = output(c);
        assert_eq!(
            out.trim_end(),
            arg.replace("%World%", "Earth"),
            "cmd: {arg} {err}"
        );
        let mut d = host(&f, &f.syspath.join("pyenv.exe"), &env, &[]);
        d.args(["exec", "python", "-c", script, arg]);
        let (out, err, _) = output(d);
        assert_eq!(out.trim_end(), arg, "direct: {arg} {err}");
    }
}
