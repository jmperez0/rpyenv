//! Windows launch fidelity (spec §5.3), with real shims.
#![cfg(windows)]

mod common;
use common::*;
use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

fn v(s: &str) -> &OsStr {
    OsStr::new(s)
}

/// Raw command-line tails that cmd.exe or a split-and-requote round trip would change.
const RAW: [&str; 13] = [
    r#"a b"#,
    r#""a b""#,
    r#""""#,
    r#"x^y 100% %USERNAME% a&b !x!"#,
    r#""say \"hi\"""#,
    r#""C:\path\\""#,
    r#""abc"#,
    r#"a""b"#,
    "ñ ü 漢",
    r#"\\server\share"#,
    "  lead",
    r#"a\\\"b"#,
    r#"a|b <c >d"#,
];

/// Review focus 1: through the shim, the child parses exactly what it parses run directly.
#[test]
fn win_shim_is_transparent_to_raw_command_lines() {
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    f.rehash();
    let env = [("PYENV_VERSION", v("3.9.1"))];
    for raw in RAW {
        let direct = f.command(&python, &env).raw_arg(raw).output().unwrap();
        let shim = f
            .shim_command("python", &env)
            .raw_arg(raw)
            .output()
            .unwrap();
        assert_eq!(arg_lines(&shim), arg_lines(&direct), "raw tail {raw:?}");
    }
}

#[test]
fn win_exec_is_transparent_to_raw_command_lines() {
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    let env = [("PYENV_VERSION", v("3.9.1"))];
    for raw in RAW {
        let direct = f.command(&python, &env).raw_arg(raw).output().unwrap();
        let exec = f
            .command(&built("pyenv"), &env)
            .args(["exec", "python"])
            .raw_arg(raw)
            .output()
            .unwrap();
        assert_eq!(arg_lines(&exec), arg_lines(&direct), "raw tail {raw:?}");
    }
}

const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Exit codes are DWORDs; STATUS_CONTROL_C_EXIT must come back unchanged.
#[test]
fn win_exit_codes_pass_through_unchanged() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let code = 0xC000_013Au32 as i32;
    let text = code.to_string();
    let out = f.run_shim(
        "python",
        &[],
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v(&text))],
    );
    assert_eq!(out.status.code(), Some(code));
}

/// Review focus 3.
#[test]
fn win_killing_the_shim_kills_the_child() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let after = f.base.join("after");
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
                ("ARGV_ECHO_AFTER", after.as_os_str()),
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    shim.kill().unwrap();
    let _ = shim.wait();
    std::thread::sleep(Duration::from_millis(3500));
    assert!(
        !after.exists(),
        "the child kept running after the shim was killed"
    );
}

/// Review focus 2. The shim gets a windowless console of its own and a process group, the
/// child shares both, and a helper sends Ctrl+Break to the group. The child catches it and
/// exits 5 when its sleep ends. A shim that didn't ignore the event would die at once with
/// 0xC000013A.
#[test]
fn win_ctrl_break_reaches_the_child_and_the_shim_waits() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_CATCH_BREAK", v("1")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
            ],
        )
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_BREAK_PID", shim.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+Break");
    assert_eq!(shim.wait().unwrap().code(), Some(5));
}
