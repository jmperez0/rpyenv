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
/// exits 5 at once (its sleep, up to 30 s, only bounds how long the break may take to
/// arrive under load). A shim that didn't ignore the event would die at once with
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
                ("ARGV_ECHO_SLEEP_MS", v("30000")),
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

/// Review focus 5: a GUI program's shim is the GUI shim, and it passes arguments and the
/// exit code on like the console shim.
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
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
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
            return 0; // stop: found it
        }
        1
    }

    let mut search = Search {
        pid: pi.dwProcessId,
        found: false,
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
/// `RPYENV_FORWARD_CP`; a 65001 host would otherwise mask a broken encoding).
fn pinned_850(cmdline: &str) -> String {
    format!("\"\"%SystemRoot%\\System32\\chcp.com\" 850 >nul & {cmdline}\"")
}

/// A forwarded batch tool changes the caller's environment, and the helper variable
/// doesn't stay behind.
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
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
}

/// `pyenv.exe` behind a non-ASCII shared root (the fixture's own base) gets the
/// `%~dp0`-relative reference and the `RPYENV_FORWARD_DIR` `call :d` subroutine; a quoted
/// invocation without the extension (`"setvar"`, the case plain `%~dp0` misresolves) still
/// finds and runs it.
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
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
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
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("FROM_BAT=hello"), "{text}");
    assert!(!text.contains("RPYENV_FORWARD_TARGET="), "{text}");
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
