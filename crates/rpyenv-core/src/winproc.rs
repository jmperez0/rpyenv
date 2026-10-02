//! Windows process plumbing for the shims and `pyenv exec` (spec §5.3): a Job Object that
//! ends the child with the shim, and a console handler that keeps the shim alive through
//! the child's Ctrl+C.

use crate::console::{self, ConsoleMode};
use crate::debuglog;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, TRUE};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileType, FILE_TYPE_DISK, FILE_TYPE_PIPE, FILE_TYPE_UNKNOWN,
};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleProcessList, GetStdHandle, SetConsoleCtrlHandler, STD_ERROR_HANDLE,
    STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
};
use windows_sys::Win32::System::StationsAndDesktops::{
    GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, WSF_VISIBLE};

/// A Job Object whose processes end when its last handle closes, which happens when this
/// process ends, however it ends. Processes they start break away silently and live on,
/// as if Python were run directly.
pub struct Job(HANDLE);

impl Job {
    pub fn new() -> Option<Job> {
        // SAFETY: creates an unnamed job and sets its limits from a zero-initialized
        // plain-data struct, as the API documents; the handle is closed on failure and on
        // drop.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Job(job))
        }
    }

    /// Puts `child` in the job. False when Windows refuses, for example when the shim runs
    /// in a job that forbids it; the child then runs without one.
    pub fn assign(&self, child: &Child) -> bool {
        // SAFETY: both handles are valid for the duration of the call.
        unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) != 0 }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// What the console decision needs to know about this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub attached: bool,
    pub stdout_redirected: bool,
    pub stderr_redirected: bool,
    pub stdin_provided: bool,
}

fn usable(h: HANDLE) -> bool {
    !h.is_null() && h != INVALID_HANDLE_VALUE
}

/// Whether this process's stdout (or, with `stderr`, stderr) is a console. std writes text
/// to a console as Unicode; anything else (a pipe, a file, NUL) gets bytes.
pub fn std_is_console(stderr: bool) -> bool {
    let which = if stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    let mut mode = 0u32;
    // SAFETY: reads this process's own standard handle; `GetConsoleMode` writes one `u32`
    // into `mode` and fails harmlessly on a handle that isn't a console.
    unsafe {
        let h = GetStdHandle(which);
        usable(h) && GetConsoleMode(h, &mut mode) != 0
    }
}

/// This process's own standard handle's file type, or `None` when the handle itself isn't
/// usable. These are this process's own handles, with no I/O pending, so `GetFileType`
/// can't block.
fn std_file_type(which: STD_HANDLE) -> Option<u32> {
    // SAFETY: reads this process's own standard handle and asks for its type.
    unsafe {
        let h = GetStdHandle(which);
        usable(h).then(|| GetFileType(h))
    }
}

/// A disk file or a pipe (the doc's "redirected").
fn redirected(which: STD_HANDLE) -> bool {
    matches!(
        std_file_type(which),
        Some(FILE_TYPE_DISK) | Some(FILE_TYPE_PIPE)
    )
}

pub fn probe() -> Probe {
    let mut one = 0u32;
    // SAFETY: asks for at most one process id into a one-element buffer; the count it
    // returns is 0 exactly when this process has no console.
    let attached = unsafe { GetConsoleProcessList(&mut one, 1) } != 0;
    // SAFETY: reads this process's own standard input handle.
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    Probe {
        attached,
        stdout_redirected: redirected(STD_OUTPUT_HANDLE),
        stderr_redirected: redirected(STD_ERROR_HANDLE),
        stdin_provided: usable(stdin),
    }
}

unsafe extern "system" fn keep_running(_event: u32) -> BOOL {
    TRUE
}

/// Console events reach the child, which shares the console. For Ctrl+C and Ctrl+Break the
/// handler's TRUE means the shim ignores them and waits for the child. For
/// `CTRL_CLOSE_EVENT`, `CTRL_LOGOFF_EVENT` and `CTRL_SHUTDOWN_EVENT` it doesn't: Windows
/// ends the shim as soon as the handler returns, without waiting for the child, which gets
/// the event too (under conhost, the child's own cleanup was observed to finish).
/// It uses a handler, never `SetConsoleCtrlHandler(NULL, TRUE)`, which children would
/// inherit (spec §5.3).
pub fn ignore_console_events() {
    // SAFETY: registers a handler that only returns TRUE and touches no state.
    unsafe {
        SetConsoleCtrlHandler(Some(keep_running), TRUE);
    }
}

/// Starts `cmd` inside a job, with console events ignored, and waits for it. The child
/// joins the job right after it starts (plan decision 2).
pub fn spawn_and_wait(cmd: &mut Command, program: &Path) -> io::Result<ExitStatus> {
    let p = probe();
    let mode = console::choose(p.attached, p.stdout_redirected, p.stderr_redirected);
    debuglog::append(&format!(
        "mode={} program={}",
        mode.name(),
        program.display()
    ));
    match mode {
        ConsoleMode::Inherit => {}
        ConsoleMode::NoWindow => {
            cmd.creation_flags(CREATE_NO_WINDOW);
            if !p.stdin_provided {
                cmd.stdin(Stdio::null());
            }
        }
        ConsoleMode::Mirror => {
            cmd.creation_flags(DETACHED_PROCESS);
        }
    }
    ignore_console_events();
    let job = Job::new();
    let mut child = cmd.spawn()?;
    // Without the job, killing the shim leaves the child running (D-45); say so in the log.
    if !job.as_ref().is_some_and(|j| j.assign(&child)) {
        debuglog::append("job=none");
    }
    let status = child.wait();
    drop(job);
    status
}

/// Whether this process has a usable stdout (or, with `stderr`, stderr) to write to. A
/// GUI program started from Explorer has neither.
pub fn std_handle_usable(stderr: bool) -> bool {
    let which = if stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    std_file_type(which).is_some_and(|t| t != FILE_TYPE_UNKNOWN)
}

/// Whether this process's window station is visible on a physical display. A service or a
/// scheduled task with no interactive session has an invisible one: a modal box there would
/// have nothing to show it and no user to click it, and would block forever.
fn window_station_visible() -> bool {
    // SAFETY: `GetProcessWindowStation` returns this process's own window station handle,
    // owned by the system, so it needs no closing; `GetUserObjectInformationW` reads into
    // a local, zero-initialized buffer of exactly the size it's told.
    unsafe {
        let station = GetProcessWindowStation();
        if station.is_null() {
            return false;
        }
        let mut flags: USEROBJECTFLAGS = std::mem::zeroed();
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            station,
            UOI_FLAGS,
            (&mut flags as *mut USEROBJECTFLAGS).cast(),
            std::mem::size_of::<USEROBJECTFLAGS>() as u32,
            &mut needed,
        );
        ok != 0 && (flags.dwFlags & WSF_VISIBLE as u32) != 0
    }
}

/// A modal error box titled "rpyenv", for the GUI shim when there is nowhere to print. Shows
/// nothing when the window station isn't visible (plan decision 4 doesn't cover a session
/// nobody can see).
pub fn message_box(text: &str) {
    if !window_station_visible() {
        return;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("rpyenv"));
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call; no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
