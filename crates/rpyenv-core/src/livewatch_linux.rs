//! The Linux live watcher (spec §8 point 3). The shim forks; the child opens a pidfd on the
//! shim (alive, waiting for it: plan M5b, R5), forks the watcher and exits; the shim reaps
//! it and then runs `execv`, so the watcher is never the program's child. The watcher moves
//! to a new session, puts stdin, stdout and stderr on /dev/null, keeps only the pidfd, and
//! after `START_DELAY` watches the stored-state folders with inotify until the program
//! exits; then it runs the final stored-state check.

use crate::ctx::Ctx;
use crate::debuglog;
use crate::livewatch::{Debounce, START_DELAY};
use crate::rehash;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

/// Forks the watcher. Every failure skips it silently (spec §8).
pub fn spawn(ctx: &Ctx, shim_exe: &Path) {
    // SAFETY: getpid and fork have no preconditions. The shim runs no other threads here,
    // so the forked processes may run ordinary Rust code.
    let shim = unsafe { libc::getpid() };
    let first = unsafe { libc::fork() };
    if first < 0 {
        return;
    }
    if first == 0 {
        // SAFETY: the pidfd syscall on a live process (the shim waits for this one).
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, shim, 0) } as libc::c_int;
        // SAFETY: forks the watcher from this single-threaded child.
        if pidfd >= 0 && unsafe { libc::fork() } == 0 {
            watcher(ctx, shim_exe, pidfd);
        }
        // SAFETY: ends the first child at once, without running exit handlers.
        unsafe { libc::_exit(0) };
    }
    let mut status = 0;
    // SAFETY: reaps the first child, which exits at once.
    unsafe { libc::waitpid(first, &mut status, 0) };
}

fn watcher(ctx: &Ctx, shim_exe: &Path, pidfd: libc::c_int) -> ! {
    // SAFETY: plain syscalls on this process's own session and descriptors.
    unsafe {
        libc::setsid();
        if pidfd != 3 {
            libc::dup2(pidfd, 3);
            libc::close(pidfd);
        }
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null >= 0 {
            for fd in 0..3 {
                libc::dup2(null, fd);
            }
        }
        close_from(4);
    }
    run(ctx, shim_exe, 3);
    // SAFETY: ends the watcher without running the shim's exit handlers.
    unsafe { libc::_exit(0) }
}

/// Closes every descriptor from `low` up.
///
/// # Safety
///
/// Nothing in this process may use a descriptor at or above `low` afterwards.
unsafe fn close_from(low: libc::c_int) {
    if libc::syscall(
        libc::SYS_close_range,
        low as libc::c_uint,
        libc::c_uint::MAX,
        0,
    ) == 0
    {
        return;
    }
    // Kernels before 5.9: close what /proc lists.
    let fds: Vec<libc::c_int> = std::fs::read_dir("/proc/self/fd")
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok()))
                .collect()
        })
        .unwrap_or_default();
    for fd in fds.into_iter().filter(|&fd| fd >= low) {
        libc::close(fd);
    }
}

fn run(ctx: &Ctx, shim_exe: &Path, pidfd: libc::c_int) {
    let mut pfd = libc::pollfd {
        fd: pidfd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: polls one descriptor this process owns.
    if unsafe { libc::poll(&mut pfd, 1, START_DELAY.as_millis() as libc::c_int) } != 0 {
        return; // ended (or failed) within the start delay
    }
    // SAFETY: creates an inotify instance; it closes when the process exits.
    let ino = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
    if ino < 0 {
        return;
    }
    watch_all(ino, ctx);
    debuglog::append(&format!("live=watching pid={}", std::process::id()));
    let mut debounce = Debounce::default();
    let mut buf = [0u8; 4096];
    loop {
        if debounce.fire(Instant::now()) {
            rehash::check(ctx, shim_exe);
            debuglog::append("live=check");
            watch_all(ino, ctx);
        }
        let timeout = debounce
            .wait(Instant::now())
            .map(|d| d.as_millis() as libc::c_int + 1)
            .unwrap_or(-1);
        let mut fds = [
            libc::pollfd {
                fd: pidfd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: ino,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: polls two descriptors this process owns.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if fds[0].revents != 0 {
            break; // the program ended
        }
        if fds[1].revents != 0 {
            // SAFETY: drains the non-blocking inotify descriptor into a local buffer.
            while unsafe { libc::read(ino, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
            debounce.changed(Instant::now());
        }
    }
    // The program ended: the final stored-state check (spec §8).
    rehash::check(ctx, shim_exe);
}

/// Watches every stored-state folder (re-adding a watched one only refreshes it). Folders
/// that can't be watched (gone, or the inotify limit) are skipped.
fn watch_all(ino: libc::c_int, ctx: &Ctx) {
    let mask = libc::IN_CREATE
        | libc::IN_DELETE
        | libc::IN_MOVED_FROM
        | libc::IN_MOVED_TO
        | libc::IN_ATTRIB
        | libc::IN_ONLYDIR;
    for d in rehash::state_dirs(ctx) {
        if let Ok(c) = CString::new(d.as_os_str().as_bytes()) {
            // SAFETY: a NUL-terminated path; failures are ignored.
            unsafe { libc::inotify_add_watch(ino, c.as_ptr(), mask) };
        }
    }
}
