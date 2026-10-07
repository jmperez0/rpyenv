//! A test program (spec §12.2). It prints how it was started, one item per line, each
//! value with `{:?}` so tests compare exact bytes:
//! `argv0=`, one `arg=` per argument, `cwd=`, `env NAME=` (or `env NAME unset`) for each
//! name in `ARGV_ECHO_ENV` (comma-separated), and `stdin=` when `ARGV_ECHO_STDIN=1`.
//! `sigign=` (the `SigIgn` mask from `/proc/self/status`, hex) when `ARGV_ECHO_SIGIGN=1`, on Linux.
//! `children=<n>` (how many child processes it has) when `ARGV_ECHO_CHILDREN=1`, on Linux.
//! Then `ARGV_ECHO_TOUCH=<path>` creates that file (runnable), as `pip install` creates a
//! script; `ARGV_ECHO_SLEEP_MS` waits, after which `ARGV_ECHO_AFTER=<path>` creates that
//! file, so a test can tell whether the process was still running past the sleep;
//! `ARGV_ECHO_EXIT` is the exit code (default 0).
//!
//! On Windows, `ARGV_ECHO_CATCH_BREAK=1` catches console events (Ctrl+C and Ctrl+Break
//! alike) and makes the exit code 5 if one arrived by the end of the sleep, and
//! `ARGV_ECHO_BREAK_PID=<pid>` only sends Ctrl+Break to that process group (attaching to its
//! console) and exits 0, or 2 or 3 on failure. `ARGV_ECHO_CTRLC_PID=<pid>` likewise sends
//! Ctrl+C to every process on that process's console, ignoring it itself.
//! `ARGV_ECHO_READY=<path>` creates that file just before the sleep, so a test can wait
//! until the program is running and set up. `ARGV_ECHO_CONSOLE=1` prints `console=<n>`,
//! the number of processes on its console (0 without one), and `window=0` or `window=1`,
//! whether a console window is visible.
//! `ARGV_ECHO_OUT=<path>` also writes what it prints to that file, and `ARGV_ECHO_QUIET=1`
//! prints nothing to stdout.
//!
//! `ARGV_ECHO_LEAVE=<path>` starts a quiet copy of this program that it doesn't wait
//! for, which sleeps `ARGV_ECHO_LEAVE_MS` (default 1500) and then creates that file; with
//! `ARGV_ECHO_LEAVE_DETACHED=1` the copy has no console (`DETACHED_PROCESS`).
//!
//! Before anything else, `ARGV_ECHO_FIRST=<path>` creates that file and
//! `ARGV_ECHO_DELAY_MS` waits before any output, after which
//! `ARGV_ECHO_PROMPT` is printed, before stdin is read. Helper modes, on Windows, each exiting
//! when done: `ARGV_ECHO_SPAWN=<exe>` starts that program as Explorer does (no console,
//! null standard handles, no creation flags, this helper's own arguments) and writes its
//! exit code to `ARGV_ECHO_SPAWN_EXIT=<path>` (with `ARGV_ECHO_SPAWN_STDOUT=<path>`, stdout
//! goes to that file; with `ARGV_ECHO_SPAWN_HIDDEN=1`, it starts it hidden, as
//! `WshShell.Run cmd, 0, True` does); `ARGV_ECHO_SCREEN_PID=<pid>` prints the
//! text on that process's console; `ARGV_ECHO_TYPE_PID=<pid>` types `ARGV_ECHO_TYPE` into
//! that process's console (`\x03` is Ctrl+C, `\x1a` Ctrl+Z, `\r` Enter).
//! `ARGV_ECHO_CLOSE_PID=<pid>` posts `WM_CLOSE` to that process's console window (exit 4
//! when it isn't conhost's). `ARGV_ECHO_ON_CLOSE=<path>`, on `CTRL_CLOSE_EVENT`, waits
//! 1 s and then creates that file, as slow cleanup would. `ARGV_ECHO_PTY_RUN=<exe>` runs
//! that program on a pseudo-console this helper owns, as a terminal tab would, and closes
//! it once the file `ARGV_ECHO_PTY_CLOSE_AFTER=<path>` exists.

#[path = "../echo.rs"]
mod echo;

fn main() {
    echo::main()
}
