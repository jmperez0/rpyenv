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

/// Review focus 3: killing the shim ends the child, through the Job Object (allowlist D-45).
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

/// Review focus 5: a program that can't start is reported on stderr and in the debug log,
/// named, with exit code 126 (allowlist D-43), and without a literal `%1`.
#[test]
fn win_a_start_failure_reaches_the_debug_log() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let bad = f.root.join("versions").join("3.9.1").join("bad.exe");
    std::fs::copy(built("argv-echo"), &bad).unwrap();
    f.rehash();
    std::fs::write(&bad, b"not a PE file").unwrap();
    let log = f.base.join("debug.log");
    let out = f
        .shim_command(
            "bad",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ],
        )
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(126));
    // stderr is in the console's code page, or the OEM one when this process has no console
    // (the shim then gets a console of its own).
    let cp = match rpyenv_core::wincp::output_cp() {
        0 => rpyenv_core::wincp::oem_cp(),
        cp => cp,
    };
    let stderr = rpyenv_core::wincp::decode(&out.stderr, cp);
    assert!(stderr.contains("bad.exe: "), "{stderr}");
    assert!(!stderr.contains("%1"), "{stderr}");
    let text = std::fs::read_to_string(&log).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("rpyenv-shim: pyenv: "))
        .unwrap_or_else(|| panic!("no start-failure line in the log:\n{text}"));
    assert!(line.contains("bad.exe: "), "{line}");
    assert!(!line.contains("%1"), "{line}");
}

/// Ends the child when a test leaves early (a failed assert or a panic), so a failure
/// never leaves a shim and its 30-second child running.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Review focus 2. The shim gets a windowless console of its own and a process group, the
/// child shares both, and a helper sends Ctrl+Break to the group. The child catches it and
/// exits 5 at once (its sleep, up to 30 s, only bounds how long the break may take to
/// arrive under load). A shim that didn't ignore the event would die at once with
/// 0xC000013A.
/// Spec §8 Safety (M5b): `pyenv rehash` ended by Ctrl+Break (or Ctrl+C, or closing its
/// window) removes its lock instead of leaving it for two minutes.
#[test]
fn win_an_interrupted_rehash_removes_its_lock() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let lock = f.root.join("shims").join(".rehash.lock");
    let mut child = KillOnDrop(
        f.command(
            &built("pyenv"),
            &[("RPYENV_TEST_HOLD_REHASH_MS", v("20000"))],
        )
        .arg("rehash")
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for(&lock);
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_BREAK_PID", child.0.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+Break");
    let _ = child.0.wait();
    assert!(!lock.exists(), "Ctrl+Break left the lock behind");
}

#[test]
fn win_ctrl_break_reaches_the_child_and_the_shim_waits() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let ready = f.base.join("ready");
    let mut shim = KillOnDrop(
        f.shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_CATCH_BREAK", v("1")),
                ("ARGV_ECHO_SLEEP_MS", v("30000")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
            ],
        )
        .creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for(&ready);
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_BREAK_PID", shim.0.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+Break");
    assert_eq!(shim.0.wait().unwrap().code(), Some(5));
}

/// Spec §5.3 bans `SetConsoleCtrlHandler(NULL, TRUE)` in the shim: children inherit it and
/// would ignore Ctrl+C. The shim gets a windowless console of its own (no new process
/// group, which would also turn Ctrl+C off), the child shares it, and a helper attached to
/// that console sends Ctrl+C to every process on it. Only that console's processes get it,
/// so this test process is not hit. The child catches it and exits 5 at once; its sleep,
/// up to 30 s, only bounds how long the event may take to arrive under load.
#[test]
fn win_ctrl_c_reaches_the_child_through_the_shim() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    // Whether Ctrl+C is ignored is inherited: clear it here, so the shim starts as it would
    // from an ordinary console, whatever started this test.
    // SAFETY: no handler pointer; it only changes this process's Ctrl+C flag.
    unsafe { SetConsoleCtrlHandler(None, 0) };
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let ready = f.base.join("ready");
    let mut shim = KillOnDrop(
        f.shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("ARGV_ECHO_CATCH_BREAK", v("1")),
                ("ARGV_ECHO_SLEEP_MS", v("30000")),
                ("ARGV_ECHO_READY", ready.as_os_str()),
            ],
        )
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for(&ready);
    let sent = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_CTRLC_PID", shim.0.id().to_string())
        .status()
        .unwrap();
    assert_eq!(sent.code(), Some(0), "could not send Ctrl+C");
    assert_eq!(shim.0.wait().unwrap().code(), Some(5));
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

const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Whether this Windows honors the shim's manifest (build 26100 or later).
fn api() -> bool {
    rpyenv_core::winproc::alloc_console_available()
}

/// Tests that open console windows run one at a time: run together, they starve each
/// other's process starts (a helper was seen to start 14 s late) and fight over focus.
static WINDOWS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn one_window_test_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner())
}

static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A path under `base` that no other call in this test process returns.
fn unique(base: &std::path::Path, stem: &str) -> std::path::PathBuf {
    base.join(format!(
        "{stem}-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ))
}

/// Starts shim `name` as Explorer does (docs/windows-lazy-console.md, "Testing"): a helper
/// started with `DETACHED_PROCESS` has no console, clears its standard handles and starts
/// the shim with no flags and `args`. Returns the helper and the file the shim's exit code
/// goes to.
fn launch_like_explorer(
    f: &Fixture,
    name: &str,
    env: &[(&str, &OsStr)],
    args: &[&str],
) -> (std::process::Child, std::path::PathBuf) {
    let exit = unique(&f.base, "exit");
    let helper = f
        .command(&built("argv-echo"), env)
        .env("ARGV_ECHO_SPAWN", f.shim(name))
        .env("ARGV_ECHO_SPAWN_EXIT", &exit)
        .args(args)
        .creation_flags(DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    (helper, exit)
}

/// Waits for the helper; the shim's exit code, as a DWORD.
fn explorer_exit(mut helper: std::process::Child, exit: &std::path::Path) -> i64 {
    helper.wait().unwrap();
    std::fs::read_to_string(exit)
        .unwrap_or_else(|_| panic!("{} was never written", exit.display()))
        .trim()
        .parse()
        .unwrap()
}

/// Polls the debug log until it contains `needle` (20 s at most); returns the log.
fn wait_log(log: &std::path::Path, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no {needle:?} in:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The shim's process ID: the first `pid=` in its log.
fn shim_pid(log_text: &str) -> u32 {
    log_text
        .split_whitespace()
        .find_map(|w| w.strip_prefix("pid="))
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| panic!("no pid= in:\n{log_text}"))
}

/// The text on process `pid`'s console.
fn screen(pid: u32) -> String {
    let out = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_SCREEN_PID", pid.to_string())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "cannot read the console of {pid}"
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The text on process `pid`'s console once it contains `needle` (10 s at most). Output
/// passes through the pseudo-console's relay on its way to the window, so it can show a
/// moment after the program wrote it.
fn screen_until(pid: u32, needle: &str) -> String {
    let start = std::time::Instant::now();
    loop {
        let shown = screen(pid);
        if shown.contains(needle) || start.elapsed() > Duration::from_secs(10) {
            return shown;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Types `text` into process `pid`'s console.
fn type_keys(pid: u32, text: &str) {
    let status = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_TYPE_PID", pid.to_string())
        .env("ARGV_ECHO_TYPE", text)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(0),
        "cannot type into the console of {pid}"
    );
}

/// Explorer-like start with `RPYENV_CONSOLE=eager`: the window is there before the
/// program prints anything.
#[test]
fn win_eager_makes_the_window_at_startup() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let first = f.base.join("first");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
            ("ARGV_ECHO_FIRST", first.as_os_str()),
            ("ARGV_ECHO_DELAY_MS", v("1500")),
        ],
        &[],
    );
    wait_for(&first);
    if !api() {
        assert_eq!(explorer_exit(helper, &exit), 0);
        assert_eq!(mode_in(&log), "INHERIT");
        return;
    }
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(mode_in(&log), "EAGER");
    assert!(text.contains("console=new"), "{text}");
    assert_eq!(explorer_exit(helper, &exit), 0);
}

/// Review focus 1: `start python` from a terminal (`CREATE_NEW_CONSOLE`) gets its new
/// window at once, as `python.exe` does.
#[test]
fn win_a_new_console_request_gets_a_window() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let status = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ],
        )
        .creation_flags(CREATE_NEW_CONSOLE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    if api() {
        assert_eq!(mode_in(&log), "EAGER");
        assert!(std::fs::read_to_string(&log)
            .unwrap()
            .contains("console=new"));
    } else {
        assert_eq!(mode_in(&log), "INHERIT");
    }
}

/// Final review I2: `start /wait python x.py` in a script (`CREATE_NEW_CONSOLE` from a
/// console parent): a failing program's window closes at once and the exit code comes
/// back, as with python.exe, instead of waiting for a key nobody may press.
#[test]
fn win_a_new_console_request_is_not_held_after_a_failure() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let mut shim = f
        .shim_command(
            "python",
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("ARGV_ECHO_EXIT", v("3")),
            ],
        )
        .creation_flags(CREATE_NEW_CONSOLE)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = shim.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = shim.kill();
            panic!(
                "the shim held the window:\n{}",
                std::fs::read_to_string(&log).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(3));
}

/// A failed program in a window EAGER opened: the window stays for the seconds
/// `RPYENV_CONSOLE_HOLD` sets, and the exit code comes back.
#[test]
fn win_eager_holds_a_failed_programs_window_for_the_set_seconds() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let start = std::time::Instant::now();
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
            ("RPYENV_CONSOLE_HOLD", v("1")),
            ("ARGV_ECHO_EXIT", v("3")),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 3);
    if api() {
        assert!(start.elapsed() >= Duration::from_secs(1));
        assert!(std::fs::read_to_string(&log).unwrap().contains("hold=1s"));
    }
}

/// A double-clicked script whose version isn't installed: the shim asks for the window
/// the caller wanted, shows why, and waits for a key.
#[test]
fn win_an_error_with_nowhere_to_print_gets_a_window_that_waits() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("9.9.9")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
        ],
        &[],
    );
    if !api() {
        assert_ne!(explorer_exit(helper, &exit), 0);
        return;
    }
    let text = wait_log(&log, "hold=key");
    let pid = shim_pid(&text);
    let shown = screen(pid);
    assert!(shown.contains("9.9.9"), "{shown}");
    type_keys(pid, "x");
    assert_ne!(explorer_exit(helper, &exit), 0);
}

/// LAZY, review focus 2: a program that never prints never gets a window. Its arguments
/// arrive unchanged, the variables too, and its exit code comes back.
#[test]
fn win_lazy_a_silent_program_never_shows_a_window() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f.base.join("out.txt");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            // A regression that shows a window must fail here, not wait for a key.
            ("RPYENV_CONSOLE_HOLD", v("0")),
            ("ARGV_ECHO_QUIET", v("1")),
            ("ARGV_ECHO_OUT", out.as_os_str()),
            ("ARGV_ECHO_ENV", v("MARKER")),
            ("MARKER", v("ñ x")),
            ("ARGV_ECHO_EXIT", v("7")),
        ],
        &["a b", "ñ"],
    );
    assert_eq!(explorer_exit(helper, &exit), 7);
    if !api() {
        assert_eq!(mode_in(&log), "INHERIT");
        return;
    }
    let text = std::fs::read_to_string(&log).unwrap();
    assert_eq!(mode_in(&log), "LAZY");
    assert!(!text.contains("console="), "{text}");
    let got = std::fs::read_to_string(&out).unwrap();
    assert!(got.contains(&line("arg", "a b")), "{got}");
    assert!(got.contains(&line("arg", "ñ")), "{got}");
    assert!(got.contains("env MARKER=\"ñ x\""), "{got}");
}

/// Final review minor 10: a program EAGER can't start, after EAGER opened a window: the
/// message stays on screen until a key, as for a version that isn't installed.
#[test]
fn win_eager_a_start_failure_keeps_its_window() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    let python = f.install("3.9.1/python.exe");
    std::fs::write(&python, b"not a program").unwrap();
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
        ],
        &[],
    );
    if !api() {
        assert_ne!(explorer_exit(helper, &exit), 0);
        return;
    }
    let pid = shim_pid(&wait_log(&log, "hold=key"));
    let shown = screen(pid);
    assert!(shown.contains("python"), "{shown}");
    type_keys(pid, "x");
    assert_ne!(explorer_exit(helper, &exit), 0);
}

/// Final review minor 17: a caller that gives only stdout (a file) gets EAGER. The window
/// is made at once, and the program's output still goes to the file.
#[test]
fn win_a_given_stdout_takes_eager_and_keeps_the_file() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let out = f.base.join("stdout.txt");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_SPAWN_STDOUT", out.as_os_str()),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 0);
    let got = std::fs::read_to_string(&out).unwrap_or_default();
    assert!(got.contains("argv0="), "{got:?}");
    if api() {
        let text = std::fs::read_to_string(&log).unwrap();
        assert_eq!(mode_in(&log), "EAGER");
        assert!(text.contains("console=new"), "{text}");
    }
}

/// Re-review I-c: a program started hidden (`WshShell.Run "python x.py", 0, True`) that
/// fails: nobody can see its window, so the shim doesn't wait for a key, and the caller
/// gets the exit code, as with python.exe.
#[test]
fn win_a_hidden_window_is_not_held() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let (mut helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_SPAWN_HIDDEN", v("1")),
            ("ARGV_ECHO_EXIT", v("3")),
        ],
        &[],
    );
    let start = std::time::Instant::now();
    while helper.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(15) {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            terminate(shim_pid(&text));
            let _ = helper.wait();
            panic!("the shim held a hidden window:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(explorer_exit(helper, &exit), 3);
    if api() {
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("console=new"), "{text}");
        assert!(text.contains("hold=invisible"), "{text}");
    }
}

/// Final review I4 (user decision 2026-10-06): a program that leaves a console process
/// running on the pseudo-console when it exits doesn't take it down. The process runs to
/// the end, as it would in python.exe's console, and the shim stays until it's done.
#[test]
fn win_lazy_a_process_left_running_finishes() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let left = f.base.join("left");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_QUIET", v("1")),
            ("ARGV_ECHO_LEAVE", left.as_os_str()),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 0);
    if !api() {
        return;
    }
    assert!(
        left.exists(),
        "the process left running was ended with the program:\n{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
}

/// The leftover wait counts only processes on the pseudo-console: a process the program
/// left running with no console (`DETACHED_PROCESS`, as daemons start; a GUI program is
/// the same case) neither delays the shim's exit, which a waiting caller such as Task
/// Scheduler would see as a stuck task, nor dies with it.
#[test]
fn win_lazy_a_detached_process_left_running_does_not_delay_the_exit() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let left = f.base.join("left");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_QUIET", v("1")),
            ("ARGV_ECHO_LEAVE", left.as_os_str()),
            ("ARGV_ECHO_LEAVE_DETACHED", v("1")),
            ("ARGV_ECHO_LEAVE_MS", v("4000")),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 0);
    assert!(
        !left.exists(),
        "the shim waited for a process that isn't on its pseudo-console:\n{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
    // ...and that process wasn't ended with the shim.
    wait_for(&left);
}

/// Review focus 5: killing a LAZY shim ends its program, through the Job Object.
#[test]
fn win_lazy_killing_the_shim_kills_the_child() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let after = f.base.join("after");
    let (mut helper, _exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_QUIET", v("1")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            // Long enough that the kill lands while it sleeps, even under a loaded suite.
            ("ARGV_ECHO_SLEEP_MS", v("8000")),
            ("ARGV_ECHO_AFTER", after.as_os_str()),
        ],
        &[],
    );
    wait_for(&ready);
    let pid = shim_pid(&std::fs::read_to_string(&log).unwrap());
    let hosts = children_named(pid, "conhost.exe");
    // Directly, not through taskkill: starting a process from a busy test harness was
    // seen to take 14 s, long enough for the program to finish on its own.
    terminate(pid);
    let _ = helper.wait();
    std::thread::sleep(Duration::from_millis(8500));
    assert!(
        !after.exists(),
        "the child kept running after the shim was killed"
    );
    // Final review minor 15: the pseudo-console's own conhost is gone too.
    if api() {
        assert!(!hosts.is_empty(), "no conhost found under the LAZY shim");
        for h in hosts {
            assert!(gone(h), "conhost {h} was left behind");
        }
    }
}

/// Process IDs of `parent`'s children named `name` (case-insensitive).
fn children_named(parent: u32, name: &str) -> Vec<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut found = Vec::new();
    // SAFETY: a process snapshot walked with a correctly sized entry and closed after.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        assert!(snap != INVALID_HANDLE_VALUE);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut e) != 0;
        while more {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let exe = String::from_utf16_lossy(&e.szExeFile[..len]);
            if e.th32ParentProcessID == parent && exe.eq_ignore_ascii_case(name) {
                found.push(e.th32ProcessID);
            }
            more = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    found
}

/// Whether process `pid` has ended (within 5 s).
fn gone(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    // SAFETY: opens a process by ID only to wait on it, and closes the handle.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return true;
        }
        let ended = WaitForSingleObject(h, 5000) == WAIT_OBJECT_0;
        CloseHandle(h);
        ended
    }
}

/// Ends process `pid` at once, as `taskkill /F` does.
fn terminate(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: opens a process by id for termination and closes the handle after.
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        assert!(!h.is_null(), "process {pid} is already gone");
        assert!(TerminateProcess(h, 1) != 0, "cannot end process {pid}");
        CloseHandle(h);
    }
}

/// LAZY: no window while the program is quiet; it appears when the program prints, and
/// shows the output intact (review focus 3: non-ASCII).
#[test]
fn win_lazy_shows_the_window_on_first_output() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let first = f.base.join("first");
    let ready = f.base.join("ready");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_FIRST", first.as_os_str()),
            ("ARGV_ECHO_DELAY_MS", v("1500")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            // Long enough to read the screen from a busy test harness; ended below.
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &["ñ 漢"],
    );
    wait_for(&first);
    if !api() {
        assert_eq!(explorer_exit(helper, &exit), 0);
        return;
    }
    assert!(
        !std::fs::read_to_string(&log).unwrap().contains("console="),
        "a window before any output"
    );
    wait_for(&ready);
    let text = wait_log(&log, "console=new");
    let pid = shim_pid(&text);
    let shown = screen_until(pid, "ñ 漢");
    terminate(pid);
    let _ = explorer_exit(helper, &exit);
    assert!(shown.contains("argv0="), "{shown}");
    assert!(shown.contains("ñ 漢"), "{shown}");
}

/// LAZY: what's typed in the window reaches the program's stdin.
#[test]
fn win_lazy_relays_typed_input() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    if !api() {
        return;
    }
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_PROMPT", v("name? ")),
            ("ARGV_ECHO_STDIN", v("1")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            // Long enough to read the screen from a busy test harness; ended below.
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    let pid = shim_pid(&wait_log(&log, "console=new"));
    type_keys(pid, "abc\r\x1a\r");
    wait_for(&ready);
    let shown = screen_until(pid, r#"stdin="abc\r\n""#);
    terminate(pid);
    let _ = explorer_exit(helper, &exit);
    assert!(shown.contains(r#"stdin="abc\r\n""#), "{shown}");
}

/// LAZY: Ctrl+C typed in the window reaches the program as Ctrl+C.
#[test]
fn win_lazy_ctrl_c_reaches_the_program() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    if !api() {
        return;
    }
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE_HOLD", v("0")),
            ("ARGV_ECHO_CATCH_BREAK", v("1")),
            ("ARGV_ECHO_PROMPT", v("go")),
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    let pid = shim_pid(&wait_log(&log, "console=new"));
    type_keys(pid, "\x03");
    assert_eq!(explorer_exit(helper, &exit), 5);
}

/// LAZY: a failed program's window waits for a key (the default hold).
#[test]
fn win_lazy_holds_a_failed_programs_window_until_a_key() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_PROMPT", v("x")),
            ("ARGV_ECHO_EXIT", v("3")),
        ],
        &[],
    );
    if !api() {
        assert_eq!(explorer_exit(helper, &exit), 3);
        return;
    }
    let pid = shim_pid(&wait_log(&log, "hold=key"));
    let shown = screen(pid);
    assert!(shown.contains("exited with code 3"), "{shown}");
    type_keys(pid, "x");
    assert_eq!(explorer_exit(helper, &exit), 3);
}

/// LAZY can't start a batch file with the raw command line safely, so it takes EAGER.
#[test]
fn win_lazy_a_batch_target_takes_eager() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("tool.bat"), "@exit /b 4\r\n").unwrap();
    f.rehash();
    let log = f.base.join("debug.log");
    let (helper, exit) = launch_like_explorer(
        &f,
        "tool",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE_HOLD", v("0")),
        ],
        &[],
    );
    assert_eq!(explorer_exit(helper, &exit), 4);
    if api() {
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("lazy=batch"), "{text}");
        assert_eq!(mode_in(&log), "EAGER");
    }
}

/// Closes process `pid`'s console window. False (after ending the shim) when the window
/// isn't conhost's, which this test can't close.
fn close_window_of(pid: u32) -> bool {
    let code = Command::new(built("argv-echo"))
        .env("ARGV_ECHO_CLOSE_PID", pid.to_string())
        .stdin(Stdio::null())
        .status()
        .unwrap()
        .code();
    if code == Some(4) {
        eprintln!(
            "skipped: the console window isn't conhost's (Windows Terminal is the default terminal)"
        );
        terminate(pid);
        return false;
    }
    assert_eq!(code, Some(0), "cannot close the window of {pid}");
    true
}

/// R8 in a terminal: closing the terminal (a pseudo-console the test owns, as Windows
/// Terminal holds one per tab) sends `CTRL_CLOSE_EVENT` to the shim and the program. The
/// shim waits for the program's cleanup, instead of ending at once and taking the program
/// down with the job.
#[test]
fn win_closing_the_terminal_lets_the_program_finish_its_cleanup() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let cleaned = f.base.join("cleaned");
    let status = f
        .command(
            &built("argv-echo"),
            &[
                ("PYENV_VERSION", v("3.9.1")),
                ("RPYENV_DEBUG_LOG", log.as_os_str()),
                ("ARGV_ECHO_PTY_RUN", f.shim("python").as_os_str()),
                ("ARGV_ECHO_PTY_CLOSE_AFTER", ready.as_os_str()),
                ("ARGV_ECHO_ON_CLOSE", cleaned.as_os_str()),
                ("ARGV_ECHO_READY", ready.as_os_str()),
                ("ARGV_ECHO_SLEEP_MS", v("20000")),
            ],
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0), "the pseudo-console helper failed");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(mode_in(&log), "INHERIT");
    assert!(
        cleaned.exists(),
        "the program was ended before its cleanup finished"
    );
}

/// R8: closing a window the program shares with the shim lets the program's own close
/// handler finish, instead of the job killing it as soon as the shim ends.
#[test]
fn win_closing_the_window_lets_the_program_finish_its_cleanup() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let cleaned = f.base.join("cleaned");
    let (mut helper, _exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("RPYENV_CONSOLE", v("eager")),
            ("ARGV_ECHO_ON_CLOSE", cleaned.as_os_str()),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    wait_for(&ready);
    let pid = shim_pid(&std::fs::read_to_string(&log).unwrap());
    if !close_window_of(pid) {
        return;
    }
    helper.wait().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        cleaned.exists(),
        "the program was ended before its cleanup finished"
    );
}

/// R8 in LAZY: closing the shim's window closes the pseudo-console, and the program's
/// close handler runs to the end.
#[test]
fn win_lazy_closing_the_window_reaches_the_program() {
    let _windows = one_window_test_at_a_time();
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.rehash();
    if !api() {
        return;
    }
    let log = f.base.join("debug.log");
    let ready = f.base.join("ready");
    let cleaned = f.base.join("cleaned");
    let (mut helper, _exit) = launch_like_explorer(
        &f,
        "python",
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("RPYENV_DEBUG_LOG", log.as_os_str()),
            ("ARGV_ECHO_ON_CLOSE", cleaned.as_os_str()),
            ("ARGV_ECHO_PROMPT", v("x")),
            ("ARGV_ECHO_READY", ready.as_os_str()),
            ("ARGV_ECHO_SLEEP_MS", v("20000")),
        ],
        &[],
    );
    let pid = shim_pid(&wait_log(&log, "console=new"));
    wait_for(&ready);
    if !close_window_of(pid) {
        return;
    }
    helper.wait().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        cleaned.exists(),
        "the program never got to finish its close handler"
    );
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
    // R1: a console parent detached the shim; EAGER asks, gets no console, and starts the
    // child as MIRROR would.
    assert_eq!(mode_in(&log), if api() { "EAGER" } else { "MIRROR" });
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

/// Review focus 5: a GUI program's shim is the GUI shim, and it passes arguments and the
/// exit code on like the console shim (allowlist D-46).
#[test]
fn win_gui_programs_get_the_gui_shim() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let pythonw = f.root.join("versions").join("3.9.1").join("pythonw.exe");
    std::fs::copy(built("argv-echow"), &pythonw).unwrap();
    f.rehash();
    let read = |p: std::path::PathBuf| std::fs::read(p).unwrap();
    assert_eq!(read(f.shim("pythonw")), read(built("pyenv-shimw")));
    assert_eq!(read(f.shim("python")), read(built("pyenv-shim")));
    let out = f.run_shim(
        "pythonw",
        &["a b".into()],
        &[("PYENV_VERSION", v("3.9.1")), ("ARGV_ECHO_EXIT", v("7"))],
    );
    assert_eq!(arg_lines(&out), [line("arg", "a b")]);
    assert_eq!(out.status.code(), Some(7));
}

/// A Windows root always has a template. If one goes missing, a shim's exit check must not
/// seed it from the running shim: a GUI shim would turn every console shim into a GUI shim.
#[test]
fn win_a_missing_template_is_not_seeded_by_the_running_shim() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let version = f.root.join("versions").join("3.9.1");
    std::fs::copy(built("argv-echow"), version.join("pythonw.exe")).unwrap();
    f.rehash();
    std::fs::remove_file(f.root.join("shims/.template/pyenv-shim.exe")).unwrap();
    let scripts = version.join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::copy(built("argv-echo"), scripts.join("new.exe")).unwrap();
    let out = f.run_shim("pythonw", &[], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(out.status.code(), Some(0));
    let read = |p: std::path::PathBuf| std::fs::read(p).unwrap();
    assert_eq!(read(f.shim("python")), read(built("pyenv-shim")));
    assert_eq!(read(f.shim("pythonw")), read(built("pyenv-shimw")));
}

/// With an output to write to, the GUI shim reports there, not in a message box (which
/// would block this test).
#[test]
fn win_gui_shim_reports_on_a_given_output() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let tool = f.root.join("versions").join("3.8.2").join("tool.exe");
    std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
    std::fs::copy(built("argv-echow"), &tool).unwrap();
    f.rehash();
    let read = |p: std::path::PathBuf| std::fs::read(p).unwrap();
    assert_eq!(read(f.shim("tool")), read(built("pyenv-shimw")));
    let out = f.run_shim("tool", &[], &[("PYENV_VERSION", v("3.9.1"))]);
    assert_eq!(out.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("pyenv: tool: command not found\r\n"));
}

/// Fix round 1, Important: a GUI shim whose target can't be started must still say so, even
/// when it was started the way Explorer starts a GUI program: no console, no inherited
/// handles, nothing on `STARTF_USESTDHANDLES`. This drives `CreateProcessW` by hand to
/// reproduce exactly that (a `Command`/`Stdio` launch always gives the child *some* usable
/// handle), then polls for the "rpyenv" message box by window title and owning pid. It's
/// `#[ignore]`d because a real message box blocks until dismissed and needs a desktop to
/// show on; run it with `cargo test -p rpyenv-e2e --test windows -- --ignored`.
#[test]
#[ignore = "shows a message box; needs an interactive desktop"]
fn win_gui_shim_shows_a_message_box_when_it_has_nowhere_to_print() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, TerminateProcess, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
        STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        SendMessageW, WM_GETTEXT,
    };

    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    let bad = f.root.join("versions").join("3.9.1").join("bad.exe");
    std::fs::copy(built("argv-echow"), &bad).unwrap();
    f.rehash();
    std::fs::write(&bad, b"not a PE file").unwrap();

    // Build the launch the same way the other tests do, then take the program, environment
    // and working directory back out of it: `CreateProcessW` needs them as raw wide
    // buffers, since it (unlike `std::process::Command`) is what lets this test control
    // inherited handles and `STARTF_USESTDHANDLES` directly.
    let cmd = f.shim_command("bad", &[("PYENV_VERSION", v("3.9.1"))]);
    let wide = |s: &OsStr| -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() };
    let program = wide(cmd.get_program());
    let dir = wide(cmd.get_current_dir().unwrap().as_os_str());
    let mut env_block: Vec<u16> = Vec::new();
    for (k, val) in cmd.get_envs() {
        let Some(val) = val else { continue };
        env_block.extend(k.encode_wide());
        env_block.push(u16::from(b'='));
        env_block.extend(val.encode_wide());
        env_block.push(0);
    }
    env_block.push(0);

    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: `program`, `env_block` and `dir` are NUL-terminated (env double-NUL-terminated)
    // wide buffers kept alive until the call returns; `startup` and `pi` are valid, correctly
    // sized in/out parameters. `bInheritHandles` is FALSE and `startup.dwFlags` has no
    // `STARTF_USESTDHANDLES`, so the child gets no std handles at all, as a GUI program
    // started from Explorer does.
    let ok = unsafe {
        CreateProcessW(
            program.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_UNICODE_ENVIRONMENT,
            env_block.as_ptr().cast(),
            dir.as_ptr(),
            &startup,
            &mut pi,
        )
    };
    assert_ne!(
        ok,
        0,
        "CreateProcessW failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: `pi.hThread` came from the call above; it isn't needed past this point.
    unsafe { CloseHandle(pi.hThread) };

    /// Ends and closes the child no matter how this test leaves: a passing assert, a
    /// failing one, or a panic. Without this, a failure here would leave a live process
    /// and a message box open on the desktop.
    struct Killer(HANDLE);
    impl Drop for Killer {
        fn drop(&mut self) {
            // SAFETY: `self.0` came from `CreateProcessW` above, and this guard is its
            // only owner.
            unsafe {
                TerminateProcess(self.0, 1);
                CloseHandle(self.0);
            }
        }
    }
    let _killer = Killer(pi.hProcess);

    struct Search {
        pid: u32,
        found: bool,
        text: String,
    }
    unsafe extern "system" fn each_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is `&mut Search`, valid for the whole `EnumWindows` call below.
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: `hwnd` is a window handle `EnumWindows` just supplied.
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        if owner != search.pid || unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1; // keep enumerating
        }
        let mut buf = [0u16; 256];
        // SAFETY: `hwnd` is valid for the call; `buf`'s length is passed alongside it.
        let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        let title = String::from_utf16_lossy(&buf[..usize::try_from(len.max(0)).unwrap_or(0)]);
        if title == "rpyenv" {
            search.found = true;
            unsafe extern "system" fn each_child(child: HWND, lparam: LPARAM) -> BOOL {
                // SAFETY: `lparam` is `&mut String`, valid for the whole
                // `EnumChildWindows` call below.
                let text = unsafe { &mut *(lparam as *mut String) };
                let mut buf = [0u16; 1024];
                // SAFETY: `WM_GETTEXT` copies at most `buf.len()` UTF-16 units into `buf`;
                // the system marshals it across processes, which `GetWindowTextW` would not
                // do for another process's control.
                let len = unsafe {
                    SendMessageW(child, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as isize)
                };
                text.push_str(&String::from_utf16_lossy(
                    &buf[..usize::try_from(len.max(0)).unwrap_or(0)],
                ));
                text.push('\n');
                1
            }
            // SAFETY: `each_child` only dereferences the pointer during this call, and
            // `search.text` outlives it.
            unsafe {
                EnumChildWindows(
                    hwnd,
                    Some(each_child),
                    std::ptr::addr_of_mut!(search.text) as isize,
                );
            }
            return 0; // stop: found it
        }
        1
    }

    let mut search = Search {
        pid: pi.dwProcessId,
        found: false,
        text: String::new(),
    };
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(10) && !search.found {
        // SAFETY: `each_window` only dereferences the pointer for the duration of this
        // call, and `search` outlives it.
        unsafe {
            EnumWindows(Some(each_window), std::ptr::addr_of_mut!(search) as isize);
        }
        if !search.found {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(
        search.found,
        "no visible \"rpyenv\" window appeared for pid {} within 10s",
        search.pid
    );
    assert!(
        search.text.contains("bad.exe: "),
        "box text: {}",
        search.text
    );
}

fn cmd_exe() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("cmd.exe")
}

fn install_setvar(f: &Fixture) {
    let scripts = f.root.join("versions").join("3.9.1").join("Scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("setvar.bat"), "@set FROM_BAT=%1\r\n").unwrap();
}

/// Copies `pyenv`, `pyenv-shim` and `pyenv-shimw` into `dir` (created if needed) and
/// returns the copy's `pyenv` path, so a forwarder can be built (and rehashed) from a
/// `pyenv.exe` that doesn't live in the usual `target/debug` build output — the point
/// being to control what characters are in its path.
fn install_tools(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    for name in ["pyenv", "pyenv-shim", "pyenv-shimw"] {
        std::fs::copy(built(name), dir.join(format!("{name}{EXE}"))).unwrap();
    }
    dir.join(format!("pyenv{EXE}"))
}

/// The tail after `/d /c`: pins the console's output code page to 850 with the external
/// `chcp.com` (not the `chcp` builtin, so it needs its full path — `System32` isn't on the
/// fixture's `PATH`) before `cmdline`, so the test's outcome doesn't depend on whatever
/// code page the host that runs `cargo test` happens to use (spec §5.3,
/// `RPYENV_FORWARD_CP`; a 65001 host would otherwise mask a broken encoding). Callers start
/// cmd.exe with `CREATE_NO_WINDOW`, so the `chcp` changes a console of its own, not the one
/// this test shares with parallel tests.
fn pinned_850(cmdline: &str) -> String {
    format!("\"\"%SystemRoot%\\System32\\chcp.com\" 850 >nul & {cmdline}\"")
}

/// A forwarded batch tool changes the caller's environment, and the helper variable
/// doesn't stay behind (allowlist D-47).
#[test]
fn win_forwarder_changes_the_callers_environment() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    install_setvar(&f);
    let env = [
        ("PYENV_VERSION", v("3.9.1")),
        ("RPYENV_BATCH_FORWARD", v("setvar")),
    ];
    assert!(f.pyenv(&["rehash"], &env).status.success());
    assert!(f.root.join("shims").join("setvar.cmd").is_file());
    assert!(!f.shim("setvar").exists());
    let out = f
        .command(&cmd_exe(), &env)
        .args(["/d", "/c"])
        .raw_arg(pinned_850(
            "setvar hello & set FROM_BAT & set RPYENV_FORWARD_TARGET",
        ))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
}

/// `pyenv.exe` behind a non-ASCII shared root (the fixture's own base) gets the
/// `%~dp0`-relative reference and the `RPYENV_FORWARD_DIR` `call :d` subroutine. Plain
/// `%~dp0` misresolves for any quoted invocation, with or without the extension; this test
/// runs the quoted form without it (`"setvar"`), from a tools folder whose name holds `(`,
/// `)` and `&`, and checks that the batch tool set `FROM_BAT` in the caller and that
/// `RPYENV_FORWARD_TARGET` didn't stay behind.
#[test]
fn win_forwarder_relative_branch_with_a_quoted_invocation() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    install_setvar(&f);
    // Ascii except for what the fixture's own base already contributes ("py env ñ"), so
    // the relative tail `pyenv_reference` computes stays writable.
    let tools = install_tools(&f.base.join("tools (x86) & co"));
    let env = [
        ("PYENV_VERSION", v("3.9.1")),
        ("RPYENV_BATCH_FORWARD", v("setvar")),
    ];
    assert!(f
        .command(&tools, &env)
        .arg("rehash")
        .status()
        .unwrap()
        .success());
    let fwd = std::fs::read_to_string(f.root.join("shims").join("setvar.cmd")).unwrap();
    assert!(fwd.contains("RPYENV_FORWARD_DIR"), "{fwd}");
    assert!(!f.shim("setvar").exists());
    let out = f
        .command(&cmd_exe(), &env)
        .args(["/d", "/c"])
        .raw_arg(pinned_850(
            "\"setvar\" hello & set FROM_BAT & set RPYENV_FORWARD_TARGET",
        ))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
}

/// In the relative branch, an `.exe` target's exit code comes back through `cmd /c`: the
/// forwarder's last line runs the target, and no later line resets errorlevel.
#[test]
fn win_forwarder_relative_branch_keeps_an_exe_targets_exit_code() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    install_setvar(&f);
    f.install("3.8.2/setvar.exe");
    // Under the fixture's non-ASCII base, so the reference is relative.
    let tools = install_tools(&f.base.join("tools"));
    let forward = [("RPYENV_BATCH_FORWARD", v("setvar"))];
    assert!(f
        .command(&tools, &forward)
        .arg("rehash")
        .status()
        .unwrap()
        .success());
    let fwd = std::fs::read_to_string(f.root.join("shims").join("setvar.cmd")).unwrap();
    assert!(fwd.contains("RPYENV_FORWARD_DIR"), "{fwd}");
    let out = f
        .command(
            &cmd_exe(),
            &[("PYENV_VERSION", v("3.8.2")), ("ARGV_ECHO_EXIT", v("7"))],
        )
        .args(["/d", "/c"])
        .raw_arg(pinned_850("setvar a"))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(7),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(arg_lines(&out), [line("arg", "a")]);
}

/// An absolute `pyenv.exe` reference with cmd metacharacters in its path (`&`, `(`, `)`,
/// `^`, `%`) still resolves: the path goes into the `RPYENV_FORWARD_PYENV` helper
/// variable, never spelled out directly on a command line where those would break it.
#[test]
fn win_forwarder_absolute_branch_with_special_characters() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    install_setvar(&f);
    // The fixture's own base is non-ASCII; the absolute branch needs a pure-ASCII path,
    // so the tools folder goes under its own, separate ASCII temp directory.
    let ascii_tmp = tempfile::tempdir().unwrap();
    let tools = install_tools(&ascii_tmp.path().join(r"a(b) & ^ % dir"));
    let env = [
        ("PYENV_VERSION", v("3.9.1")),
        ("RPYENV_BATCH_FORWARD", v("setvar")),
    ];
    assert!(f
        .command(&tools, &env)
        .arg("rehash")
        .status()
        .unwrap()
        .success());
    let fwd = std::fs::read_to_string(f.root.join("shims").join("setvar.cmd")).unwrap();
    assert!(!fwd.contains("RPYENV_FORWARD_DIR"), "{fwd}");
    // A literal `%` in the path is doubled when written into the file.
    assert!(fwd.contains(r"a(b) & ^ %% dir"), "{fwd}");
    let out = f
        .command(&cmd_exe(), &env)
        .args(["/d", "/c"])
        .raw_arg(pinned_850(
            "setvar hello & set FROM_BAT & set RPYENV_FORWARD_TARGET",
        ))
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
}

/// A shim's exit check, run without `RPYENV_BATCH_FORWARD` (as `pip install` through a
/// shim is), keeps the forwarders the last `pyenv rehash` made.
#[test]
fn win_the_exit_check_keeps_forwarders_without_the_variable() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    install_setvar(&f);
    let forward = [("RPYENV_BATCH_FORWARD", v("setvar"))];
    assert!(f.pyenv(&["rehash"], &forward).status.success());
    assert!(f.root.join("shims").join("setvar.cmd").is_file());
    let new_tool = f.root.join("versions/3.9.1/Scripts/newtool.exe");
    let out = f.run_shim(
        "python",
        &[],
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("ARGV_ECHO_TOUCH", new_tool.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(f.shim("newtool").is_file(), "the exit check didn't rehash");
    assert!(f.root.join("shims").join("setvar.cmd").is_file());
    assert!(!f.shim("setvar").exists());
    // `pyenv exec`'s exit check, also without the variable, keeps them too.
    let exec_tool = f.root.join("versions/3.9.1/Scripts/exectool.exe");
    let out = f.pyenv(
        &["exec", "python"],
        &[
            ("PYENV_VERSION", v("3.9.1")),
            ("ARGV_ECHO_TOUCH", exec_tool.as_os_str()),
        ],
    );
    assert!(out.status.success());
    assert!(
        f.shim("exectool").is_file(),
        "pyenv exec's check didn't rehash"
    );
    assert!(f.root.join("shims").join("setvar.cmd").is_file());
    assert!(!f.shim("setvar").exists());
}

/// A forwarded tool the selected version lacks: pyenv's message, errorlevel 127.
#[test]
fn win_forwarder_reports_a_missing_command_with_127() {
    let f = Fixture::new();
    f.install("3.9.1/python.exe");
    f.install("3.8.2/python.exe");
    install_setvar(&f);
    let env = [("RPYENV_BATCH_FORWARD", v("setvar"))];
    assert!(f.pyenv(&["rehash"], &env).status.success());
    let out = f
        .command(&cmd_exe(), &[("PYENV_VERSION", v("3.8.2"))])
        .args(["/d", "/c", "setvar"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&out.stdout).contains("pyenv: setvar: command not found"));
}
