use std::io::{Read, Write};

#[cfg(windows)]
static CAUGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the Ctrl+Break handler has run (never, off Windows).
fn caught() -> bool {
    #[cfg(windows)]
    return CAUGHT.load(std::sync::atomic::Ordering::SeqCst);
    #[cfg(not(windows))]
    false
}

#[cfg(windows)]
unsafe extern "system" fn catch(event: u32) -> windows_sys::core::BOOL {
    if event == windows_sys::Win32::System::Console::CTRL_CLOSE_EVENT {
        // Cleanup that takes a while: Windows ends this process when the handler returns.
        if let Some(p) = std::env::var_os("ARGV_ECHO_ON_CLOSE") {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let _ = std::fs::write(p, b"");
        }
        return 1;
    }
    CAUGHT.store(true, std::sync::atomic::Ordering::SeqCst);
    1
}

/// Exits 0 when process `pid`'s console window is minimized, 1 when it isn't, and 2 when
/// that console can't be reached.
#[cfg(windows)]
fn window_minimized(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::IsIconic;
    // SAFETY: console attachment of this helper only; IsIconic reads the window's state.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        let hwnd = GetConsoleWindow();
        FreeConsole();
        if IsIconic(hwnd) != 0 {
            0
        } else {
            1
        }
    }
}

/// Posts `WM_CLOSE` to process `pid`'s console window, as clicking its close button does.
/// Exits 4 when the window isn't conhost's (Windows Terminal hosts it elsewhere).
#[cfg(windows)]
fn close_window(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, PostMessageW, WM_CLOSE};
    // SAFETY: console attachment of this helper only; reads a class name into a local.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        let hwnd = GetConsoleWindow();
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), 64);
        if String::from_utf16_lossy(&class[..n.max(0) as usize]) != "ConsoleWindowClass" {
            return 4;
        }
        // Leave that console before closing it: the close sends CTRL_CLOSE_EVENT to every
        // process still on it, and this helper would die with 0xC000013A (seen on CI,
        // windows-2022). The window handle stays valid after leaving.
        FreeConsole();
        if PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0 {
            return 3;
        }
    }
    0
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

/// Sends Ctrl+C to every process on the console of process `pid`: this helper leaves its
/// own console, attaches to that one, and ignores Ctrl+C itself first. Only processes on
/// that console get it.
#[cfg(windows)]
fn send_ctrl_c(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_C_EVENT,
    };
    // SAFETY: console calls with no pointer arguments; they change only this helper's own
    // console attachment and Ctrl+C flag, and send the event to the console it attached to.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return 2;
        }
        SetConsoleCtrlHandler(None, 1);
        if GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) == 0 {
            return 3;
        }
    }
    0
}

/// Starts `exe` as Explorer does: from this console-less helper (the test starts it with
/// `DETACHED_PROCESS`), with null standard handles and no creation flags, passing this
/// helper's own arguments on. Writes the exit code to `ARGV_ECHO_SPAWN_EXIT`.
#[cfg(windows)]
fn spawn_like_explorer(exe: &std::ffi::OsStr) -> i32 {
    // Read first: the hidden start clears the variables before starting the program.
    let exit_file = std::env::var_os("ARGV_ECHO_SPAWN_EXIT");
    use windows_sys::Win32::System::Console::{
        SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: clears this helper's own standard handles; std then passes null handles on.
    unsafe {
        for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetStdHandle(h, std::ptr::null_mut());
        }
    }
    // Some callers give a file for stdout only.
    let stdout =
        std::env::var_os("ARGV_ECHO_SPAWN_STDOUT").and_then(|p| std::fs::File::create(p).ok());
    let code = if std::env::var_os("ARGV_ECHO_SPAWN_HIDDEN").is_some() {
        spawn_hidden(exe)
    } else {
        spawn_minimized(exe, stdout.as_ref())
    };
    if let Some(p) = exit_file {
        let _ = std::fs::write(p, code.to_string());
    }
    0
}

/// `arg` quoted for a Windows command line as the C runtime reads it back (and as std's
/// `Command` writes it): in quotes when it's empty or has a space or tab, with the
/// backslashes before a quote, and before the closing quote, doubled.
#[cfg(windows)]
fn push_arg(line: &mut Vec<u16>, arg: &std::ffi::OsStr) {
    use std::os::windows::ffi::OsStrExt;
    let units: Vec<u16> = arg.encode_wide().collect();
    let quote = units.is_empty() || units.iter().any(|&u| u == 0x20 || u == 0x09);
    line.push(0x20);
    if quote {
        line.push(0x22);
    }
    let mut backslashes = 0;
    for &u in &units {
        if u == 0x5C {
            backslashes += 1;
        } else {
            if u == 0x22 {
                line.extend(std::iter::repeat_n(0x5C, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(u);
    }
    if quote {
        line.extend(std::iter::repeat_n(0x5C, backslashes));
        line.push(0x22);
    }
}

/// Starts `exe` with this helper's arguments as std's `Command` does (the program in
/// quotes, `push_arg` quoting, handles inherited, standard handles given only when
/// `stdout` is), but minimized and not activated (`SW_SHOWMINNOACTIVE`): the window the
/// shim opens never takes the keyboard, so a key typed in another window during a test
/// can't end its hold. Waits and returns the exit code (-2 when it can't start).
#[cfg(windows)]
fn spawn_minimized(exe: &std::ffi::OsStr, stdout: Option<&std::fs::File>) -> i64 {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, WaitForSingleObject, INFINITE, PROCESS_INFORMATION,
        STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE};
    // `ARGV_ECHO_SPAWN_SHOW=noactivate`: shown, still never activated (a test that must
    // close the window: a minimized LAZY window's close is under investigation).
    let show = match std::env::var("ARGV_ECHO_SPAWN_SHOW").as_deref() {
        Ok("noactivate") => SW_SHOWNOACTIVATE,
        _ => SW_SHOWMINNOACTIVE,
    };
    std::env::remove_var("ARGV_ECHO_SPAWN");
    std::env::remove_var("ARGV_ECHO_SPAWN_EXIT");
    std::env::remove_var("ARGV_ECHO_SPAWN_STDOUT");
    std::env::remove_var("ARGV_ECHO_SPAWN_SHOW");
    let mut line: Vec<u16> = vec![0x22];
    line.extend(exe.encode_wide());
    line.push(0x22);
    for a in std::env::args_os().skip(1) {
        push_arg(&mut line, &a);
    }
    line.push(0);
    // SAFETY: a zeroed STARTUPINFOW with its size, flags and handles set; the stdout file
    // stays open for the call; locals for every out-pointer; the command line is
    // NUL-terminated and mutable.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = show as u16;
        if let Some(f) = stdout {
            let h = f.as_raw_handle();
            SetHandleInformation(h, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
            si.dwFlags |= STARTF_USESTDHANDLES;
            si.hStdOutput = h;
        }
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        if CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        ) == 0
        {
            return -2;
        }
        WaitForSingleObject(pi.hProcess, INFINITE);
        let mut code = 0u32;
        GetExitCodeProcess(pi.hProcess, &mut code);
        i64::from(code)
    }
}

/// Starts `exe` with this helper's arguments as `WshShell.Run cmd, 0, True` does: no
/// creation flags, `SW_HIDE`, no handles. Waits and returns the exit code (-2 when it
/// can't start).
#[cfg(windows)]
fn spawn_hidden(exe: &std::ffi::OsStr) -> i64 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, WaitForSingleObject, INFINITE, PROCESS_INFORMATION,
        STARTF_USESHOWWINDOW, STARTUPINFOW,
    };
    std::env::remove_var("ARGV_ECHO_SPAWN");
    std::env::remove_var("ARGV_ECHO_SPAWN_EXIT");
    std::env::remove_var("ARGV_ECHO_SPAWN_HIDDEN");
    let mut line: Vec<u16> = vec![u16::from(b'"')];
    line.extend(exe.encode_wide());
    line.push(u16::from(b'"'));
    for a in std::env::args_os().skip(1) {
        line.extend(" \"".encode_utf16());
        line.extend(a.encode_wide());
        line.push(u16::from(b'"'));
    }
    line.push(0);
    // SAFETY: a zeroed STARTUPINFOW with its size and show-window fields set; locals for
    // every out-pointer; the command line is NUL-terminated and mutable.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = 0;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        if CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            0,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        ) == 0
        {
            return -2;
        }
        WaitForSingleObject(pi.hProcess, INFINITE);
        let mut code = 0u32;
        GetExitCodeProcess(pi.hProcess, &mut code);
        i64::from(code)
    }
}

/// Leaves this helper's console and attaches to process `pid`'s, keeping stdout (the
/// test's pipe) as it was. Returns that console's handle named `name` (`CONOUT$` or
/// `CONIN$`), or an exit code.
#[cfg(windows)]
fn attach_to(pid: u32, name: &str) -> Result<windows_sys::Win32::Foundation::HANDLE, i32> {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GetStdHandle, SetStdHandle, STD_OUTPUT_HANDLE,
    };
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: console attachment and standard-handle calls on this helper only; the name
    // is NUL-terminated.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        FreeConsole();
        if AttachConsole(pid) == 0 {
            let error = windows_sys::Win32::Foundation::GetLastError();
            eprintln!("AttachConsole({pid}) failed: error {error}");
            return Err(2);
        }
        SetStdHandle(STD_OUTPUT_HANDLE, out);
        let h = CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(3);
        }
        Ok(h)
    }
}

/// Prints the text on process `pid`'s console, row by row up to the cursor, each row's
/// trailing blanks removed.
#[cfg(windows)]
fn read_screen(pid: u32) -> i32 {
    use windows_sys::Win32::System::Console::{
        GetConsoleScreenBufferInfo, ReadConsoleOutputCharacterW, CONSOLE_SCREEN_BUFFER_INFO, COORD,
    };
    let out = match attach_to(pid, "CONOUT$") {
        Ok(h) => h,
        Err(code) => return code,
    };
    let mut text = String::new();
    // SAFETY: reads the attached console's buffer into a local of the width it reports.
    unsafe {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        if GetConsoleScreenBufferInfo(out, &mut info) == 0 {
            return 4;
        }
        let width = info.dwSize.X as usize;
        let mut row = vec![0u16; width];
        for y in 0..=info.dwCursorPosition.Y {
            let mut n = 0u32;
            ReadConsoleOutputCharacterW(
                out,
                row.as_mut_ptr(),
                width as u32,
                COORD { X: 0, Y: y },
                &mut n,
            );
            text.push_str(String::from_utf16_lossy(&row[..n as usize]).trim_end());
            text.push('\n');
        }
    }
    print!("{text}");
    0
}

/// Writes `text` as key presses into process `pid`'s console: `\x03` is Ctrl+C, `\x1a`
/// Ctrl+Z, `\r` Enter, anything else that character.
#[cfg(windows)]
fn type_keys(pid: u32, text: &str) -> i32 {
    use windows_sys::Win32::System::Console::{
        WriteConsoleInputW, INPUT_RECORD, KEY_EVENT, LEFT_CTRL_PRESSED,
    };
    let conin = match attach_to(pid, "CONIN$") {
        Ok(h) => h,
        Err(code) => return code,
    };
    let mut records = Vec::new();
    for c in text.encode_utf16() {
        let (vk, sc, ctrl) = match c {
            0x03 => (0x43, 0x2E, LEFT_CTRL_PRESSED),
            0x1A => (0x5A, 0x2C, LEFT_CTRL_PRESSED),
            0x0D => (0x0D, 0x1C, 0),
            _ => (0, 0, 0),
        };
        for down in [1, 0] {
            // SAFETY: a zeroed INPUT_RECORD is valid; only the key-event member is set.
            let mut r: INPUT_RECORD = unsafe { std::mem::zeroed() };
            r.EventType = KEY_EVENT as u16;
            r.Event.KeyEvent.bKeyDown = down;
            r.Event.KeyEvent.wRepeatCount = 1;
            r.Event.KeyEvent.wVirtualKeyCode = vk;
            r.Event.KeyEvent.wVirtualScanCode = sc;
            r.Event.KeyEvent.uChar.UnicodeChar = c;
            r.Event.KeyEvent.dwControlKeyState = ctrl;
            records.push(r);
        }
    }
    let mut written = 0u32;
    // SAFETY: writes `records.len()` initialized records to the attached console's input.
    let ok =
        unsafe { WriteConsoleInputW(conin, records.as_ptr(), records.len() as u32, &mut written) };
    if ok == 0 {
        4
    } else {
        0
    }
}

/// Runs `exe` on a pseudo-console this helper owns, as a terminal tab would, draining its
/// output. Once the file `ARGV_ECHO_PTY_CLOSE_AFTER` exists, closes the pseudo-console, as
/// closing the tab does: everything on it gets `CTRL_CLOSE_EVENT`. Then waits for `exe`.
#[cfg(windows)]
fn run_in_pty(exe: &std::ffi::OsStr) -> i32 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Console::{
        ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
        WaitForSingleObject, EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES,
        STARTUPINFOEXW,
    };
    let close_after = std::env::var_os("ARGV_ECHO_PTY_CLOSE_AFTER");
    // The program on the pseudo-console must not run this mode itself.
    std::env::remove_var("ARGV_ECHO_PTY_RUN");
    std::env::remove_var("ARGV_ECHO_PTY_CLOSE_AFTER");
    let mut line: Vec<u16> = vec![u16::from(b'"')];
    line.extend(exe.encode_wide());
    line.extend([u16::from(b'"'), 0]);
    // SAFETY: pipe, pseudo-console and process calls with locals for every out-pointer;
    // the attribute list lives in `attrs` while used. Handles leak until this helper exits.
    unsafe {
        let (mut in_r, mut in_w, mut out_r, mut out_w) = (
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if CreatePipe(&mut in_r, &mut in_w, std::ptr::null(), 0) == 0
            || CreatePipe(&mut out_r, &mut out_w, std::ptr::null(), 0) == 0
        {
            return 2;
        }
        let mut hpc: HPCON = 0;
        if CreatePseudoConsole(COORD { X: 120, Y: 30 }, in_r, out_w, 0, &mut hpc) != 0 {
            return 2;
        }
        CloseHandle(in_r);
        CloseHandle(out_w);
        let out = out_r as usize;
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                let mut n = 0u32;
                let ok = ReadFile(
                    out as HANDLE,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                );
                if ok == 0 || n == 0 {
                    break;
                }
            }
        });
        let mut size = 0usize;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
        let mut attrs = vec![0u8; size];
        let list = attrs.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0
            || UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                hpc as *const core::ffi::c_void,
                std::mem::size_of::<HPCON>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            ) == 0
        {
            return 3;
        }
        let mut si: STARTUPINFOEXW = std::mem::zeroed();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.lpAttributeList = list;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        if CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT,
            std::ptr::null(),
            std::ptr::null(),
            &si.StartupInfo,
            &mut pi,
        ) == 0
        {
            return 3;
        }
        if let Some(p) = close_after {
            while !std::path::Path::new(&p).exists() {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        ClosePseudoConsole(hpc);
        WaitForSingleObject(pi.hProcess, 30_000);
    }
    0
}

pub fn main() {
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_BREAK_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(send_break(pid));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_CTRLC_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(send_ctrl_c(pid));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_CLOSE_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(close_window(pid));
    }
    #[cfg(windows)]
    if let Some(exe) = std::env::var_os("ARGV_ECHO_PTY_RUN") {
        std::process::exit(run_in_pty(&exe));
    }
    #[cfg(windows)]
    if let Some(exe) = std::env::var_os("ARGV_ECHO_SPAWN") {
        std::process::exit(spawn_like_explorer(&exe));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_MINIMIZED_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(window_minimized(pid));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_SCREEN_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(read_screen(pid));
    }
    #[cfg(windows)]
    if let Some(pid) = std::env::var("ARGV_ECHO_TYPE_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    {
        std::process::exit(type_keys(
            pid,
            &std::env::var("ARGV_ECHO_TYPE").unwrap_or_default(),
        ));
    }
    #[cfg(windows)]
    if std::env::var("ARGV_ECHO_CATCH_BREAK").as_deref() == Ok("1")
        || std::env::var_os("ARGV_ECHO_ON_CLOSE").is_some()
    {
        // SAFETY: the handler only stores to an atomic. The first call clears an inherited
        // "ignore Ctrl+C" (a parent's `SetConsoleCtrlHandler(NULL, TRUE)`), as a program
        // that handles Ctrl+C itself would.
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0);
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(catch), 1);
        }
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_LEAVE") {
        // A process left running on this console: a quiet copy of this program that sleeps,
        // then creates the file. Not waited for.
        let mut copy = std::process::Command::new(std::env::current_exe().unwrap());
        let sleep = std::env::var("ARGV_ECHO_LEAVE_MS").unwrap_or_else(|_| "1500".into());
        #[cfg(windows)]
        if std::env::var_os("ARGV_ECHO_LEAVE_DETACHED").is_some() {
            // No console at all, as daemons are usually started.
            use std::os::windows::process::CommandExt;
            copy.creation_flags(0x0000_0008); // DETACHED_PROCESS
        }
        for k in [
            "ARGV_ECHO_LEAVE",
            "ARGV_ECHO_LEAVE_MS",
            "ARGV_ECHO_LEAVE_DETACHED",
            "ARGV_ECHO_EXIT",
            "ARGV_ECHO_READY",
            "ARGV_ECHO_FIRST",
            "ARGV_ECHO_DELAY_MS",
            "ARGV_ECHO_PROMPT",
            "ARGV_ECHO_OUT",
        ] {
            copy.env_remove(k);
        }
        let _ = copy
            .env("ARGV_ECHO_QUIET", "1")
            .env("ARGV_ECHO_SLEEP_MS", sleep)
            .env("ARGV_ECHO_AFTER", &p)
            .spawn();
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_FIRST") {
        let _ = std::fs::write(&p, b"");
    }
    if let Some(ms) = std::env::var("ARGV_ECHO_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    if let Some(prompt) = std::env::var_os("ARGV_ECHO_PROMPT") {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(prompt.to_string_lossy().as_bytes());
        let _ = stdout.flush();
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
    #[cfg(windows)]
    if var("ARGV_ECHO_CONSOLE").as_deref() == Some("1") {
        let mut one = 0u32;
        // SAFETY: asks for at most one process id into a one-element buffer.
        let n = unsafe { windows_sys::Win32::System::Console::GetConsoleProcessList(&mut one, 1) };
        out.push_str(&format!("console={n}\n"));
        // SAFETY: no arguments; returns this process's console window handle or null.
        let window = unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() };
        out.push_str(&format!("window={}\n", u32::from(!window.is_null())));
    }
    #[cfg(target_os = "linux")]
    if var("ARGV_ECHO_SIGIGN").as_deref() == Some("1") {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(mask) = status.lines().find_map(|l| l.strip_prefix("SigIgn:")) {
                out.push_str(&format!("sigign={}\n", mask.trim()));
            }
        }
    }
    #[cfg(target_os = "linux")]
    if var("ARGV_ECHO_CHILDREN").as_deref() == Some("1") {
        let mut n = 0;
        if let Ok(tasks) = std::fs::read_dir("/proc/self/task") {
            for t in tasks.flatten() {
                n += std::fs::read_to_string(t.path().join("children"))
                    .map(|s| s.split_whitespace().count())
                    .unwrap_or(0);
            }
        }
        out.push_str(&format!("children={n}\n"));
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_TOUCH") {
        let _ = std::fs::write(&p, b"");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_OUT") {
        let _ = std::fs::write(&p, out.as_bytes());
    }
    if var("ARGV_ECHO_QUIET").as_deref() != Some("1") {
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_READY") {
        let _ = std::fs::write(&p, b"");
    }
    if let Some(ms) = var("ARGV_ECHO_SLEEP_MS").and_then(|v| v.parse().ok()) {
        // In slices, so a caught Ctrl+Break ends the sleep at once: tests can then give
        // the break a long window without waiting it out.
        let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < end && !caught() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    if let Some(p) = std::env::var_os("ARGV_ECHO_AFTER") {
        let _ = std::fs::write(&p, b"");
    }
    if caught() {
        std::process::exit(5);
    }
    std::process::exit(
        var("ARGV_ECHO_EXIT")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
}
