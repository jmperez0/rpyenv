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
    GetConsoleProcessList, GetStdHandle, SetConsoleCtrlHandler, STD_ERROR_HANDLE, STD_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

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

/// A disk file or a pipe (the doc's "redirected"). These are this process's own handles,
/// with no I/O pending, so `GetFileType` can't block.
fn redirected(which: STD_HANDLE) -> bool {
    // SAFETY: reads this process's own standard handle and asks for its type.
    unsafe {
        let h = GetStdHandle(which);
        usable(h) && matches!(GetFileType(h), FILE_TYPE_DISK | FILE_TYPE_PIPE)
    }
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

/// Ctrl+C, Ctrl+Break and closing the console reach the child, which shares the console.
/// The shim ignores them and waits for the child. It uses a handler, never
/// `SetConsoleCtrlHandler(NULL, TRUE)`, which children would inherit (spec §5.3).
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
    if let Some(job) = &job {
        job.assign(&child);
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
    // SAFETY: reads this process's own standard handle and asks for its type.
    unsafe {
        let h = GetStdHandle(which);
        usable(h) && GetFileType(h) != FILE_TYPE_UNKNOWN
    }
}

/// A modal error box titled "rpyenv", for the GUI shim when there is nowhere to print.
pub fn message_box(text: &str) {
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
