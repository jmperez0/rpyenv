//! The Windows live watcher (spec §8 point 3): a thread in the shim. After `START_DELAY`, if
//! the program still runs, it watches the whole `versions` tree through one handle
//! (`ReadDirectoryChangesW`, subtree, share-delete: plan M5b, R1). Once changes stop for
//! `QUIET`, it runs the stored-state check.

use crate::ctx::Ctx;
use crate::debuglog;
use crate::livewatch::{Debounce, START_DELAY};
use crate::rehash;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Instant;
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects, WaitForSingleObject, INFINITE,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// A running watcher: dropping it stops the thread and waits for it.
pub struct Running {
    stop: usize,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // SAFETY: the stop event this module created; set, then closed once the thread
        // that waits on it has ended.
        unsafe { SetEvent(self.stop as HANDLE) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: as above; closed once, after the thread ended.
        unsafe { CloseHandle(self.stop as HANDLE) };
    }
}

/// Starts the watcher thread, or `None` when it can't (silently, spec §8).
pub fn spawn(ctx: &Ctx, shim_exe: &Path) -> Option<Running> {
    // SAFETY: a manual-reset event, initially unset, unnamed.
    let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if stop.is_null() {
        return None;
    }
    let (ctx, exe, stop_id) = (ctx.clone(), shim_exe.to_path_buf(), stop as usize);
    let thread = std::thread::spawn(move || watch(&ctx, &exe, stop_id as HANDLE));
    Some(Running {
        stop: stop as usize,
        thread: Some(thread),
    })
}

fn watch(ctx: &Ctx, exe: &Path, stop: HANDLE) {
    // SAFETY: waits on the stop event, owned by `Running`, which outlives this thread.
    if unsafe { WaitForSingleObject(stop, START_DELAY.as_millis() as u32) } == WAIT_OBJECT_0 {
        return;
    }
    let versions: PathBuf = ctx.versions_dir();
    let wide: Vec<u16> = versions.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated path; the handle is closed below on every path.
    let dir = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
            std::ptr::null_mut(),
        )
    };
    if dir == INVALID_HANDLE_VALUE {
        return;
    }
    // SAFETY: a manual-reset event, initially unset, unnamed; closed below.
    let changed = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if changed.is_null() {
        // SAFETY: the directory handle opened above, closed once.
        unsafe { CloseHandle(dir) };
        return;
    }
    debuglog::append(&format!("live=watching pid={}", std::process::id()));
    // `u32` elements: the notification buffer must be DWORD-aligned.
    let mut buf = vec![0u32; 16 * 1024];
    let mut debounce = Debounce::default();
    'outer: loop {
        // SAFETY: a zeroed OVERLAPPED is valid; its event is the one created above.
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = changed;
        // SAFETY: the buffer and OVERLAPPED live until the operation completes or is
        // cancelled and waited for below.
        let armed = unsafe {
            ResetEvent(changed);
            ReadDirectoryChangesW(
                dir,
                buf.as_mut_ptr().cast(),
                (buf.len() * 4) as u32,
                1,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES,
                std::ptr::null_mut(),
                &mut ov,
                None,
            ) != 0
        };
        if !armed {
            break;
        }
        loop {
            let timeout = debounce
                .wait(Instant::now())
                .map(|d| d.as_millis() as u32 + 1)
                .unwrap_or(INFINITE);
            let handles = [stop, changed];
            // SAFETY: waits on two live events.
            let w = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout) };
            if w == WAIT_OBJECT_0 + 1 {
                let mut n = 0u32;
                // SAFETY: the operation has completed; reads its result.
                unsafe { GetOverlappedResult(dir, &ov, &mut n, 0) };
                debounce.changed(Instant::now());
                continue 'outer;
            }
            if w == WAIT_TIMEOUT {
                if debounce.fire(Instant::now()) {
                    rehash::check(ctx, exe);
                    debuglog::append("live=check");
                }
                continue;
            }
            // Stopped (or the wait failed): cancel the read and wait for it to end before
            // the buffer goes away.
            let mut n = 0u32;
            // SAFETY: cancels this thread's own operation on `dir` and waits for it.
            unsafe {
                CancelIoEx(dir, &ov);
                GetOverlappedResult(dir, &ov, &mut n, 1);
            }
            break 'outer;
        }
    }
    // SAFETY: handles created above, closed once.
    unsafe {
        CloseHandle(changed);
        CloseHandle(dir);
    }
}
