//! Windows process plumbing for the shims and `pyenv exec` (spec §5.3): a Job Object that
//! ends the child with the shim, and a console handler that keeps the shim alive through
//! the child's Ctrl+C.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, TRUE};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
};

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
    let _ = program;
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
