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

#[cfg(unix)]
#[test]
fn exec_usage_and_not_found() {
    let f = Fixture::new();
    let r = f.pyenv(&["exec"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("Usage: pyenv exec <command> [arg1 arg2...]\n", 1)
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
        "Usage: pyenv exec <command> [arg1 arg2...]\n\n\
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
