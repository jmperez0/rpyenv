//! A test program (spec §12.2). It prints how it was started, one item per line, each
//! value with `{:?}` so tests compare exact bytes:
//! `argv0=`, one `arg=` per argument, `cwd=`, `env NAME=` (or `env NAME unset`) for each
//! name in `ARGV_ECHO_ENV` (comma-separated), and `stdin=` when `ARGV_ECHO_STDIN=1`.
//! `sigign=` (the `SigIgn` mask from `/proc/self/status`, hex) when `ARGV_ECHO_SIGIGN=1`, on Linux.
//! Then `ARGV_ECHO_TOUCH=<path>` creates that file (runnable), as `pip install` creates a
//! script; `ARGV_ECHO_SLEEP_MS` waits, after which `ARGV_ECHO_AFTER=<path>` creates that
//! file, so a test can tell whether the process was still running past the sleep;
//! `ARGV_ECHO_EXIT` is the exit code (default 0).

use std::io::{Read, Write};

fn main() {
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
    std::process::exit(
        var("ARGV_ECHO_EXIT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
}
