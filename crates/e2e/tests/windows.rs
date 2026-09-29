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

/// Waits (up to 20 s) until `path` exists: argv-echo creates it once it is running and set up.
fn wait_for(path: &std::path::Path) {
    let start = std::time::Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

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
    let ready = f.base.join("ready");
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
                ("ARGV_ECHO_AFTER", after.as_os_str()),
                ("ARGV_ECHO_READY", ready.as_os_str()),
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&ready);
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
    let ready = f.base.join("ready");
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_CATCH_BREAK", v("1")),
                ("ARGV_ECHO_SLEEP_MS", v("3000")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
            ],
        )
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&ready);
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_BREAK_PID", shim.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+Break");
    assert_eq!(shim.wait().unwrap().code(), Some(5));
}

const DETACHED_PROCESS: u32 = 0x0000_0008;

/// The console mode the shim logged.
fn mode_in(log: &std::path::Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .find_map(|l| l.split(' ').find_map(|w| w.strip_prefix("mode=")))
        .unwrap_or("")
        .to_string()
}

/// The `console=` count argv-echo printed.
fn console_count(out: &std::process::Output) -> u32 {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("console="))
        .and_then(|n| n.parse().ok())
        .expect("argv-echo printed no console= line")
}

/// Whether argv-echo's `window=` line said a console window is visible.
fn has_window(out: &std::process::Output) -> bool {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("window="))
        .expect("argv-echo printed no window= line")
        == "1"
}

fn console_env(log: &std::path::Path) -> [(&'static str, &OsStr); 3] {
    [
        ("PYENV_VERSION", v("3.9.1")),
        ("RPYENV_DEBUG_LOG", log.as_os_str()),
        ("ARGV_ECHO_CONSOLE", v("1")),
    ]
}

/// No console, output redirected → a windowless console for the child.
#[test]
fn win_no_console_with_redirected_output_gets_no_window() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command("python", &console_env(&log))
        .creation_flags(DETACHED_PROCESS)
        .output()
        .unwrap();
    assert_eq!(mode_in(&log), "NO-WINDOW");
    assert!(
        console_count(&out) >= 1,
        "the child should have a windowless console"
    );
    assert!(!has_window(&out), "the child's console should be hidden");
}

/// No console and stderr on NUL (not redirected) → the child gets no console either.
#[test]
fn win_no_console_without_both_outputs_redirected_mirrors() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command("python", &console_env(&log))
        .creation_flags(DETACHED_PROCESS)
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(mode_in(&log), "MIRROR");
    assert_eq!(console_count(&out), 0);
    assert!(!has_window(&out), "the child should have no console at all");
}

/// A shim with a console shares it with the child: the console then holds at least the
/// shim and the child.
#[test]
fn win_a_shim_with_a_console_shares_it() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command("python", &console_env(&log))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert_eq!(mode_in(&log), "INHERIT");
    assert!(console_count(&out) >= 2);
}
