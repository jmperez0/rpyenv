//! A test program (spec §12.2). It prints how it was started, one item per line, each
//! value with `{:?}` so tests compare exact bytes:
//! `argv0=`, one `arg=` per argument, `cwd=`, `env NAME=` (or `env NAME unset`) for each
//! name in `ARGV_ECHO_ENV` (comma-separated), and `stdin=` when `ARGV_ECHO_STDIN=1`.
//! `sigign=` (the `SigIgn` mask from `/proc/self/status`, hex) when `ARGV_ECHO_SIGIGN=1`, on Linux.
//! Then `ARGV_ECHO_TOUCH=<path>` creates that file (runnable), as `pip install` creates a
//! script; `ARGV_ECHO_SLEEP_MS` waits, after which `ARGV_ECHO_AFTER=<path>` creates that
//! file, so a test can tell whether the process was still running past the sleep;
//! `ARGV_ECHO_EXIT` is the exit code (default 0).
//!
//! On Windows, `ARGV_ECHO_CATCH_BREAK=1` catches console events and makes the exit code 5
//! if one arrived by the end of the sleep, and `ARGV_ECHO_BREAK_PID=<pid>` only sends
//! Ctrl+Break to that process group (attaching to its console) and exits 0, or 2 or 3 on
//! failure.

use std::io::{Read, Write};

#[cfg(windows)]
static CAUGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
unsafe extern "system" fn catch(_event: u32) -> windows_sys::core::BOOL {
    CAUGHT.store(true, std::sync::atomic::Ordering::SeqCst);
    1
}

/// Sends Ctrl+Break to process group `pid`, which must own a console: this helper leaves
/// its own console and attaches to that one first.
#[cfg(windows)]
fn send_break(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT,
    };
    // SAFETY: console calls that affect only this helper process.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        if GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) == 0 {
            return 3;
        }
    }
    0
}

fn main() {
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_BREAK_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(send_break(pid));
    }
    #[cfg(windows)]
    if std::env::var("ARGV_ECHO_CATCH_BREAK").as_deref() == Ok("1") {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(catch), 1);
        }
    }
    let mut out = String::new();
    let mut args = std::env::args_os();
    out.push_str(&format!("argv0={:?}\n", args.next().unwrap_or_default()));
    for a in args {
        out.push_str(&format!("arg={a:?}\n"));
    }
    out.push_str(&format!(
        "cwd={:?}\n",
        std::env::current_dir().unwrap_or_default()
    ));
    let var = |k: &str| std::env::var(k).ok();
    for name in var("ARGV_ECHO_ENV")
        .unwrap_or_default()
        .split(',')
        .filter(|n| !n.is_empty())
    {
        match std::env::var_os(name) {
            Some(v) => out.push_str(&format!("env {name}={v:?}\n")),
            None => out.push_str(&format!("env {name} unset\n")),
        }
    }
    if var("ARGV_ECHO_STDIN").as_deref() == Some("1") {
        let mut input = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut input);
        out.push_str(&format!("stdin={:?}\n", String::from_utf8_lossy(&input)));
    }
    #[cfg(target_os = "linux")]
    if var("ARGV_ECHO_SIGIGN").as_deref() == Some("1") {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(mask) = status.lines().find_map(|l| l.strip_prefix("SigIgn:")) {
                out.push_str(&format!("sigign={}\n", mask.trim()));
            }
        }
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_TOUCH") {
        let _ = std::fs::write(&p, b"");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
    }
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    drop(stdout);
    if let Some(ms) = var("ARGV_ECHO_SLEEP_MS").and_then(|v| v.parse().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_AFTER") {
        let _ = std::fs::write(&p, b"");
    }
    #[cfg(windows)]
    if CAUGHT.load(std::sync::atomic::Ordering::SeqCst) {
        std::process::exit(5);
    }
    std::process::exit(
        var("ARGV_ECHO_EXIT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
}
