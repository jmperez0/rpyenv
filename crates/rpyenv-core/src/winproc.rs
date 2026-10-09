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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::core::{BOOL, HRESULT};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, TRUE};
use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
use windows_sys::Win32::Foundation::{GetLastError, GENERIC_READ, GENERIC_WRITE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileType, FILE_TYPE_DISK, FILE_TYPE_PIPE, FILE_TYPE_UNKNOWN,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, FlushConsoleInputBuffer, FreeConsole, ReadConsoleInputW, SetConsoleMode,
    SetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS, INPUT_RECORD, KEY_EVENT,
};
use windows_sys::Win32::System::Console::{ClosePseudoConsole, ResizePseudoConsole, COORD, HPCON};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleProcessList, GetStdHandle, SetConsoleCtrlHandler, STD_ERROR_HANDLE,
    STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Console::{
    CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::StationsAndDesktops::{
    GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, DETACHED_PROCESS};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, MessageBoxW, MB_ICONERROR, MB_OK, WSF_VISIBLE,
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
        // SAFETY: `child` owns its process handle for the duration of the call.
        unsafe { self.assign_handle(child.as_raw_handle()) }
    }

    /// `assign`, for a process this crate started itself.
    ///
    /// # Safety
    ///
    /// `process` must be a valid process handle for the duration of the call.
    pub unsafe fn assign_handle(&self, process: HANDLE) -> bool {
        // SAFETY: the job handle is ours; the caller vouches for `process`.
        unsafe { AssignProcessToJobObject(self.0, process) != 0 }
    }
}

/// The running pseudo-console, if any: LAZY's main thread, its input relay and the
/// console handler all reach it through this lock, so it's never used after closing.
static PTY: Mutex<HPCON> = Mutex::new(0);

pub fn set_pty(hpc: HPCON) {
    *PTY.lock().unwrap_or_else(|e| e.into_inner()) = hpc;
}

/// Closes the pseudo-console once, whichever thread gets here first.
pub fn close_pty() {
    let mut pty = PTY.lock().unwrap_or_else(|e| e.into_inner());
    if *pty != 0 {
        // SAFETY: a pseudo-console from CreatePseudoConsole, closed once under the lock.
        unsafe { ClosePseudoConsole(*pty) };
        *pty = 0;
    }
}

/// Runs `f` with the pseudo-console while holding its lock, so it can't close meanwhile;
/// `None` when it's already closed.
pub fn with_open_pty<T>(f: impl FnOnce(HPCON) -> T) -> Option<T> {
    let pty = PTY.lock().unwrap_or_else(|e| e.into_inner());
    (*pty != 0).then(|| f(*pty))
}

/// Resizes the pseudo-console, if it's still open.
pub fn resize_pty(size: COORD) {
    let pty = PTY.lock().unwrap_or_else(|e| e.into_inner());
    if *pty != 0 {
        // SAFETY: an open pseudo-console, held open by the lock.
        unsafe { ResizePseudoConsole(*pty, size) };
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
    /// At least one of the three standard handles is a usable handle (a file, a pipe, a
    /// console, NUL). Explorer gives none.
    pub handles_given: bool,
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
    // SAFETY: reads this process's own standard input handle.
    let stdin = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    // SAFETY: reads this process's own standard output and error handles.
    let (stdout, stderr) = unsafe {
        (
            GetStdHandle(STD_OUTPUT_HANDLE),
            GetStdHandle(STD_ERROR_HANDLE),
        )
    };
    Probe {
        attached: attached(),
        stdout_redirected: redirected(STD_OUTPUT_HANDLE),
        stderr_redirected: redirected(STD_ERROR_HANDLE),
        stdin_provided: usable(stdin),
        handles_given: usable(stdin) || usable(stdout) || usable(stderr),
    }
}

/// A copy of the child's process handle, never closed, for the console handler to wait on.
static CHILD: AtomicUsize = AtomicUsize::new(0);

/// Remembers the child for the console handler (plan M5a, R8).
///
/// # Safety
///
/// `process` must be a valid process handle for the duration of the call.
pub unsafe fn watch_child(process: HANDLE) {
    let mut copy: HANDLE = std::ptr::null_mut();
    // SAFETY: duplicates the caller's live process handle within this process; the copy
    // is kept for the life of the process.
    unsafe {
        let me = GetCurrentProcess();
        if DuplicateHandle(me, process, me, &mut copy, 0, 0, DUPLICATE_SAME_ACCESS) != 0 {
            CHILD.store(copy as usize, Ordering::SeqCst);
        }
    }
}

/// Ctrl+C and Ctrl+Break: TRUE, so the shim ignores them and waits for the child, which
/// gets them too. CLOSE, LOGOFF and SHUTDOWN (plan M5a, R8): Windows ends the shim when this
/// returns, and the job would then kill the child mid-cleanup. So the handler first closes
/// a LAZY pseudo-console (which sends the child its own CTRL_CLOSE_EVENT), then waits for
/// the child, until Windows' own timeout ends both. On a shared console the child is
/// usually done already: the console host closes the most recently attached first.
pub(crate) unsafe extern "system" fn on_console_event(event: u32) -> BOOL {
    debuglog::append(&format!("event={event}"));
    if matches!(
        event,
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
    ) {
        close_pty();
        let child = CHILD.load(Ordering::SeqCst);
        if child != 0 {
            WaitForSingleObject(child as HANDLE, INFINITE);
        }
    }
    TRUE
}

/// Console events reach the child, which shares the console (`on_console_event` says what
/// the shim does with each). It uses a handler, never `SetConsoleCtrlHandler(NULL, TRUE)`,
/// which children would inherit (spec §5.3).
pub fn ignore_console_events() {
    // SAFETY: registers a handler that touches only this module's atomics and lock.
    unsafe {
        SetConsoleCtrlHandler(Some(on_console_event), TRUE);
    }
}

static CONSOLE_SHIM: AtomicBool = AtomicBool::new(false);

/// Marks this process as the console shim, whose manifest asks Windows for no console at
/// startup (spec §5.3). Only it starts children in EAGER or LAZY mode, and only it asks for
/// a console to show an error in.
pub fn set_console_shim() {
    CONSOLE_SHIM.store(true, Ordering::SeqCst);
}

fn console_shim() -> bool {
    CONSOLE_SHIM.load(Ordering::SeqCst)
}

/// `ALLOC_CONSOLE_OPTIONS` (ConsoleApi.h, build 26100).
#[repr(C)]
struct AllocConsoleOptions {
    mode: i32,
    use_show_window: BOOL,
    show_window: u16,
}

type AllocConsoleWithOptionsFn =
    unsafe extern "system" fn(*mut AllocConsoleOptions, *mut i32) -> HRESULT;

/// `AllocConsoleWithOptions`, looked up once: windows-sys 0.61 doesn't declare it, and
/// Windows before build 26100 doesn't have it.
fn alloc_fn() -> Option<AllocConsoleWithOptionsFn> {
    static ADDR: OnceLock<Option<usize>> = OnceLock::new();
    let addr = (*ADDR.get_or_init(|| {
        let name: Vec<u16> = "kernel32.dll".encode_utf16().chain(Some(0)).collect();
        // SAFETY: kernel32 is loaded in every process; both names are NUL-terminated.
        unsafe {
            let k = GetModuleHandleW(name.as_ptr());
            if k.is_null() {
                return None;
            }
            GetProcAddress(k, c"AllocConsoleWithOptions".as_ptr().cast()).map(|f| f as usize)
        }
    }))?;
    // SAFETY: the address is kernel32's AllocConsoleWithOptions, whose signature this type
    // spells out.
    Some(unsafe { std::mem::transmute::<usize, AllocConsoleWithOptionsFn>(addr) })
}

/// Whether this Windows has `AllocConsoleWithOptions` (Windows 11 24H2, Server 2025),
/// and so honors the shim's `consoleAllocationPolicy`.
pub fn alloc_console_available() -> bool {
    alloc_fn().is_some()
}

/// What asking for the caller's console gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alloc {
    /// The caller wanted none (`DETACHED_PROCESS`), or the call isn't available.
    None,
    /// A new console: a window, or a windowless one if the caller asked for that.
    New,
    /// The console this process already had.
    Existing,
}

impl Alloc {
    pub fn name(self) -> &'static str {
        match self {
            Alloc::None => "none",
            Alloc::New => "new",
            Alloc::Existing => "existing",
        }
    }
}

/// `AllocConsoleWithOptions(ALLOC_CONSOLE_MODE_DEFAULT)`: the console the caller asked
/// for when it started this process, created now. Logs `console=<what> pid=<id>`.
pub fn alloc_default() -> Alloc {
    let alloc = match alloc_fn() {
        None => Alloc::None,
        Some(f) => {
            let mut options = AllocConsoleOptions {
                mode: 0,
                use_show_window: 0,
                show_window: 0,
            };
            let mut result = 0i32;
            // SAFETY: both pointers are to locals of the documented layouts.
            let hr = unsafe { f(&mut options, &mut result) };
            match (hr, result) {
                (0, 1) => Alloc::New,
                (0, 2) => Alloc::Existing,
                _ => Alloc::None,
            }
        }
    };
    debuglog::append(&format!(
        "console={} pid={}",
        alloc.name(),
        std::process::id()
    ));
    alloc
}

/// Whether this process is attached to a console.
fn attached() -> bool {
    let mut one = 0u32;
    // SAFETY: asks for at most one process id into a one-element buffer; the count it
    // returns is 0 exactly when this process has no console.
    unsafe { GetConsoleProcessList(&mut one, 1) != 0 }
}

/// Whether the parent process has a console (R1): attaches to it and leaves at once,
/// keeping this process's standard handles as they were.
fn parent_has_console() -> bool {
    // SAFETY: reads and restores this process's own standard handles; the console
    // attachment it changes is none before and after.
    unsafe {
        let saved =
            [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].map(|w| (w, GetStdHandle(w)));
        let found = AttachConsole(ATTACH_PARENT_PROCESS) != 0;
        if found {
            FreeConsole();
        }
        for (w, h) in saved {
            SetStdHandle(w, h);
        }
        found
    }
}

/// Whether this process's console window is visible (re-review I-c: probed on build
/// 26200, a launch with `SW_HIDE` gets a console whose window isn't).
fn console_window_visible() -> bool {
    // SAFETY: no arguments; IsWindowVisible accepts any window handle, null included.
    unsafe { IsWindowVisible(windows_sys::Win32::System::Console::GetConsoleWindow()) != 0 }
}

/// Opens this process's console input (`CONIN$`) or screen (`CONOUT$`).
pub fn open_console(name: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a NUL-terminated name; the handle is the caller's to close.
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    (h != INVALID_HANDLE_VALUE).then_some(h)
}

/// After a child ended with `code` in a window this shim opened: keeps the window on
/// screen as `RPYENV_CONSOLE_HOLD` says (console doc, LAZY step 6), unless
/// `console::hold_wait` says not to: no failure, a window nobody can see, or a window a
/// console parent asked for (`asked_by_console_parent`).
pub fn hold_after(code: u32, asked_by_console_parent: bool) {
    if !console::should_hold(code) {
        return;
    }
    let setting = console::Hold::parse(std::env::var("RPYENV_CONSOLE_HOLD").ok().as_deref());
    // Visible means a window someone can see: on a visible window station, and not
    // hidden (a caller's `SW_HIDE`, such as `WshShell.Run cmd, 0, True`).
    let visible = window_station_visible() && console_window_visible();
    let Some(hold) = console::hold_wait(code, setting, visible, asked_by_console_parent) else {
        debuglog::append(match (setting, visible) {
            (console::Hold::Off, _) => "hold=off",
            (_, false) => "hold=invisible",
            _ => "hold=parent",
        });
        return;
    };
    let wait_ms = hold.wait_ms();
    let shown = if code > 0xFFFF {
        format!("0x{code:08X}")
    } else {
        code.to_string()
    };
    let text = match hold {
        console::Hold::Seconds(n) => {
            format!("\r\n[exited with code {shown}] This window closes in {n} s, or press any key.")
        }
        _ => format!("\r\n[exited with code {shown}] Press any key to close this window."),
    };
    let (Some(conin), Some(conout)) = (open_console("CONIN$"), open_console("CONOUT$")) else {
        return;
    };
    let wide: Vec<u16> = text.encode_utf16().collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(u64::from(wait_ms));
    // SAFETY: console calls on handles opened above, with local buffers of the sizes given.
    unsafe {
        let mut written = 0u32;
        WriteConsoleW(
            conout,
            wide.as_ptr().cast(),
            wide.len() as u32,
            &mut written,
            std::ptr::null(),
        );
        // Ctrl+C is a key here, not a signal.
        SetConsoleMode(conin, 0);
        FlushConsoleInputBuffer(conin);
        debuglog::append(&format!("hold={}", hold.name()));
        let ended = loop {
            let left = if wait_ms == INFINITE {
                INFINITE
            } else {
                deadline
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis() as u32
            };
            let waited = WaitForSingleObject(conin, left);
            if waited != WAIT_OBJECT_0 {
                break format!("wait={waited:#x} error={}", GetLastError());
            }
            let mut records: [INPUT_RECORD; 16] = std::mem::zeroed();
            let mut n = 0u32;
            if ReadConsoleInputW(conin, records.as_mut_ptr(), 16, &mut n) == 0 {
                break format!("read error={}", GetLastError());
            }
            if let Some(r) = records[..n as usize].iter().find(|r| {
                u32::from(r.EventType) == KEY_EVENT
                    && console::ends_hold(
                        r.Event.KeyEvent.wVirtualKeyCode,
                        r.Event.KeyEvent.bKeyDown != 0,
                    )
            }) {
                break format!("key vk={:#x}", r.Event.KeyEvent.wVirtualKeyCode);
            }
        };
        debuglog::append(&format!("hold-end={ended}"));
        CloseHandle(conin);
        CloseHandle(conout);
    }
}

/// For the console shim with a message and no usable handle to print it on (R7): asks
/// for the console the caller wanted, so a double-clicked script shows why it failed.
/// True when that made a new window the caller may hold open: not when a console parent
/// asked for it (`hold_wait`).
pub fn console_for_message(stderr: bool) -> bool {
    // A window EAGER already opened for the program: hold it for this message too.
    if OPENED_WINDOW.load(Ordering::SeqCst) {
        return true;
    }
    if !console_shim() || std_handle_usable(stderr) || attached() || !alloc_console_available() {
        return false;
    }
    let parent = parent_has_console();
    alloc_default() == Alloc::New && !parent
}

/// Set when EAGER opened a new window that a failure may hold (no console parent asked
/// for it), so a later message, such as "cannot run", is held there too.
static OPENED_WINDOW: AtomicBool = AtomicBool::new(false);

/// Starts `cmd` inside a job, with console events ignored, and waits for it. The child
/// joins the job right after it starts (plan decision 2). LAZY starts the child itself,
/// from `raw_tail` (the caller's arguments, unchanged) and `env` (the plan's variable
/// changes); `cmd` carries the same for every other mode.
pub fn spawn_and_wait(
    cmd: &mut Command,
    program: &Path,
    raw_tail: Option<&std::ffi::OsStr>,
    env: &[(std::ffi::OsString, Option<std::ffi::OsString>)],
) -> io::Result<ExitStatus> {
    // Before the parent-console check, which attaches to the parent's console for a moment:
    // a Ctrl+C typed there then can't end the shim.
    ignore_console_events();
    let p = probe();
    let policy = console_shim() && alloc_console_available();
    let parent = policy
        && !p.attached
        && !(p.stdout_redirected && p.stderr_redirected)
        && parent_has_console();
    let mut mode = console::choose(console::Situation {
        attached: p.attached,
        stdout_redirected: p.stdout_redirected,
        stderr_redirected: p.stderr_redirected,
        detached_policy: policy,
        parent_has_console: parent,
        handles_given: p.handles_given,
        setting: console::Setting::parse(std::env::var("RPYENV_CONSOLE").ok().as_deref()),
    });
    // LAZY starts the child itself, with the raw command line; std's batch escaping is the
    // only safe way to start a batch file, so a batch target takes EAGER.
    if mode == ConsoleMode::Lazy && crate::launch::is_batch(program) {
        debuglog::append("lazy=batch");
        mode = ConsoleMode::Eager;
    }
    debuglog::append(&format!(
        "mode={} pid={} program={}",
        mode.name(),
        std::process::id(),
        program.display()
    ));
    let job = Job::new();
    if mode == ConsoleMode::Lazy {
        match crate::conpty::run(env, raw_tail, program, job.as_ref()) {
            Ok(o) => {
                drop(job);
                if o.new_window {
                    // LAZY only runs when no console parent asked (`choose`).
                    hold_after(o.code, false);
                }
                return Ok(std::os::windows::process::ExitStatusExt::from_raw(o.code));
            }
            Err(crate::conpty::LazyError::Start(e)) => return Err(e),
            Err(crate::conpty::LazyError::Setup(e)) => {
                debuglog::append(&format!("lazy=failed {e}"));
                mode = ConsoleMode::Eager;
            }
        }
    }
    let mut new_window = false;
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
        ConsoleMode::Eager => match alloc_default() {
            Alloc::New => {
                new_window = true;
                OPENED_WINDOW.store(!parent, Ordering::SeqCst);
            }
            Alloc::Existing => {}
            Alloc::None => {
                cmd.creation_flags(DETACHED_PROCESS);
            }
        },
        ConsoleMode::Lazy => unreachable!("LAZY returned or fell back above"),
    }
    let mut child = cmd.spawn()?;
    // SAFETY: `child` owns its process handle for the duration of the call.
    unsafe { watch_child(child.as_raw_handle() as HANDLE) };
    // Without the job, killing the shim leaves the child running (D-45); say so in the log.
    if !job.as_ref().is_some_and(|j| j.assign(&child)) {
        debuglog::append("job=none");
    }
    let status = child.wait();
    drop(job);
    if let (true, Ok(s)) = (new_window, &status) {
        hold_after(s.code().unwrap_or(1) as u32, parent);
    }
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

/// The parent process's executable file name (`cmd.exe`, `pwsh.exe`, …), when the parent
/// was created before this process. A parent that exited may have had its process ID
/// reused by a later process; the creation-time check rejects that (spec §7).
pub fn parent_image_name() -> Option<String> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn created(process: HANDLE) -> Option<u64> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
        // SAFETY: four valid FILETIME out-pointers and a process handle.
        let ok = unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) };
        (ok != 0).then(|| (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
    }

    // SAFETY: plain Win32 calls; the snapshot handle is closed on every path below.
    unsafe {
        let me = GetCurrentProcessId();
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ppid = None;
        let mut names: Vec<(u32, String)> = Vec::new();
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push((
                entry.th32ProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..len]),
            ));
            if entry.th32ProcessID == me {
                ppid = Some(entry.th32ParentProcessID);
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        let ppid = ppid?;
        let name = names.into_iter().find(|(p, _)| *p == ppid)?.1;
        let mine = created(GetCurrentProcess())?;
        let parent = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, ppid);
        if parent.is_null() {
            return None;
        }
        let theirs = created(parent);
        CloseHandle(parent);
        (theirs? < mine).then_some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Under `cargo test` the parent (cargo, or the shell that ran the test binary) is
    /// alive and older than this process. Tasks 6 and 7 check the name under cmd and
    /// PowerShell.
    #[test]
    fn a_test_process_has_a_trusted_parent() {
        let name = parent_image_name().expect("a trusted parent");
        assert!(name.to_ascii_lowercase().ends_with(".exe"), "{name}");
    }
}
