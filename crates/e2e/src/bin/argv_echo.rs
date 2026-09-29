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
//! failure. `ARGV_ECHO_READY=<path>` creates that file just before the sleep, so a test can
//! wait until the program is running and set up. `ARGV_ECHO_CONSOLE=1` prints `console=<n>`,
//! the number of processes on its console (0 without one), and `window=0` or `window=1`,
//! whether a console window is visible.

#[path = "../echo.rs"]
mod echo;

fn main() {
    echo::main()
}
