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
unsafe extern "system" fn catch(_event: u32) -> windows_sys::core::BOOL {
    CAUGHT.store(true, std::sync::atomic::Ordering::SeqCst);
    1
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
    use windows_sys::Win32::System::Console::{
        SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: clears this helper's own standard handles; std then passes null handles on.
    unsafe {
        for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetStdHandle(h, std::ptr::null_mut());
        }
    }
    let status = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .env_remove("ARGV_ECHO_SPAWN")
        .env_remove("ARGV_ECHO_SPAWN_EXIT")
        .status();
    let code = match status {
        Ok(s) => i64::from(s.code().unwrap_or(-1) as u32),
        Err(_) => -2,
    };
    if let Some(p) = std::env::var_os("ARGV_ECHO_SPAWN_EXIT") {
        let _ = std::fs::write(p, code.to_string());
    }
    0
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
    if let Some(exe) = std::env::var_os("ARGV_ECHO_SPAWN") {
        std::process::exit(spawn_like_explorer(&exe));
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
    if std::env::var("ARGV_ECHO_CATCH_BREAK").as_deref() == Ok("1") {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(catch), 1);
        }
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
