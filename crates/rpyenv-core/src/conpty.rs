//! LAZY console mode (docs/windows-lazy-console.md, "LAZY mode: pseudo-console relay"):
//! the child runs on a pseudo-console, and the shim asks Windows for the console the
//! caller wanted only when the child first prints something.

use crate::debuglog;
use crate::vtscan::Scanner;
use crate::winproc::{self, Alloc, Job};
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Console::{CreatePseudoConsole, COORD, HPCON};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// The pseudo-console's size until a window shows (plan M5a, R5).
const START_SIZE: COORD = COORD { X: 120, Y: 30 };

/// Why LAZY didn't run the child.
#[derive(Debug)]
pub enum LazyError {
    /// The pseudo-console couldn't be set up: the caller falls back to EAGER.
    Setup(io::Error),
    /// The program itself couldn't start, as `Command::spawn` would report.
    Start(io::Error),
}

/// How the child ended, and whether the shim opened a window for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub code: u32,
    pub new_window: bool,
}

/// The child's variables: `base` with `changes` applied (`None` removes). Names compare
/// without regard to case, as Windows does, and the result is sorted the same way, as
/// `CreateProcessW` expects.
pub fn merge_env(
    base: Vec<(OsString, OsString)>,
    changes: &[(OsString, Option<OsString>)],
) -> Vec<(OsString, OsString)> {
    let key = |k: &OsStr| k.to_string_lossy().to_uppercase();
    let mut out: Vec<(OsString, OsString)> = base
        .into_iter()
        .filter(|(k, _)| !changes.iter().any(|(c, _)| key(c) == key(k)))
        .collect();
    out.extend(
        changes
            .iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v))),
    );
    out.sort_by_key(|(k, _)| key(k));
    out
}

/// A `CREATE_UNICODE_ENVIRONMENT` block: `NAME=value` entries, each NUL-terminated, then
/// one more NUL.
pub fn env_block(vars: &[(OsString, OsString)]) -> Vec<u16> {
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(k.encode_wide());
        block.push(u16::from(b'='));
        block.extend(v.encode_wide());
        block.push(0);
    }
    if vars.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// The child's command line, NUL-terminated: the program, quoted, then the shim's own
/// arguments exactly as it got them (spec §5.3).
pub fn command_line(program: &Path, tail: Option<&OsStr>) -> Vec<u16> {
    let mut line = vec![u16::from(b'"')];
    line.extend(program.as_os_str().encode_wide());
    line.push(u16::from(b'"'));
    if let Some(t) = tail {
        line.push(u16::from(b' '));
        line.extend(t.encode_wide());
    }
    line.push(0);
    line
}

/// A handle closed on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is owned here and closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn pipe() -> io::Result<(Owned, Owned)> {
    let (mut r, mut w) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: two out-pointers to locals; no security attributes, default size.
    if unsafe { CreatePipe(&mut r, &mut w, std::ptr::null(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((Owned(r), Owned(w)))
}

/// Runs `cmd`'s program on a pseudo-console and waits for it. `cmd` supplies the variable
/// changes; the arguments are the shim's own raw tail.
pub fn run(cmd: &Command, program: &Path, job: Option<&Job>) -> Result<Outcome, LazyError> {
    let (in_read, in_write) = pipe().map_err(LazyError::Setup)?;
    let (out_read, out_write) = pipe().map_err(LazyError::Setup)?;
    let mut hpc: HPCON = 0;
    // SAFETY: two valid pipe ends and an out-pointer; the pseudo-console keeps its own
    // copies of the handles, so ours close below.
    let hr = unsafe { CreatePseudoConsole(START_SIZE, in_read.0, out_write.0, 0, &mut hpc) };
    if hr != 0 {
        return Err(LazyError::Setup(io::Error::from_raw_os_error(hr)));
    }
    drop(in_read);
    drop(out_write);
    winproc::set_pty(hpc);
    let changes: Vec<(OsString, Option<OsString>)> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_owned(), v.map(OsStr::to_owned)))
        .collect();
    let env = env_block(&merge_env(std::env::vars_os().collect(), &changes));
    let tail = crate::wincmd::own_tail(1);
    let mut line = command_line(program, tail.as_deref());
    let process = match start(&mut line, &env, hpc, job) {
        Ok(p) => p,
        Err(e) => {
            winproc::close_pty();
            return Err(LazyError::Start(e));
        }
    };
    let win32 = Arc::new(AtomicBool::new(false));
    let reader = {
        let (out, input, win32) = (out_read.0 as usize, in_write.0 as usize, win32.clone());
        std::thread::spawn(move || relay_output(out as HANDLE, input as HANDLE, win32))
    };
    let mut code = 1u32;
    // SAFETY: waits on and reads the child's process handle, which `process` owns.
    unsafe {
        WaitForSingleObject(process.0, INFINITE);
        GetExitCodeProcess(process.0, &mut code);
    }
    // Closing the pseudo-console ends its output once drained; the reader then returns.
    winproc::close_pty();
    let shown = reader.join().unwrap_or_default();
    let new_window = shown.alloc == Some(Alloc::New);
    shown.finish();
    drop(in_write);
    drop(out_read);
    Ok(Outcome { code, new_window })
}

/// Starts the child suspended on the pseudo-console, puts it in the job, then lets it run.
fn start(line: &mut [u16], env: &[u16], hpc: HPCON, job: Option<&Job>) -> io::Result<Owned> {
    // SAFETY: the attribute list lives in `attrs` for the whole call and is deleted on
    // every path; all other pointers are to locals or to the caller's NUL-terminated
    // buffers; the thread handle is closed here and the process handle returned owned.
    unsafe {
        let mut size = 0usize;
        InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size);
        let mut attrs = vec![0u8; size];
        let list = attrs.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(list, 1, 0, &mut size) == 0 {
            return Err(io::Error::last_os_error());
        }
        let ok = UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            hpc as *const core::ffi::c_void,
            std::mem::size_of::<HPCON>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        if ok == 0 {
            let e = io::Error::last_os_error();
            DeleteProcThreadAttributeList(list);
            return Err(e);
        }
        let mut si: STARTUPINFOEXW = std::mem::zeroed();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        // Null standard handles: the child takes the pseudo-console's. Without the flag it
        // inherits the shim's own, and its output bypasses the pseudo-console (probe 4).
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.lpAttributeList = list;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            env.as_ptr().cast(),
            std::ptr::null(),
            &si.StartupInfo,
            &mut pi,
        );
        let err = io::Error::last_os_error();
        DeleteProcThreadAttributeList(list);
        if ok == 0 {
            return Err(err);
        }
        if !job.is_some_and(|j| j.assign_handle(pi.hProcess)) {
            debuglog::append("job=none");
        }
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);
        Ok(Owned(pi.hProcess))
    }
}

/// What the output relay did: whether it asked for a console.
#[derive(Default)]
struct Shown {
    alloc: Option<Alloc>,
}

impl Shown {
    /// Stops what the relay started.
    fn finish(self) {}
}

/// Reads the pseudo-console's output until it closes. Until the first printable
/// character the bytes are held; then the console the caller wanted is asked for.
fn relay_output(out: HANDLE, _input: HANDLE, win32: Arc<AtomicBool>) -> Shown {
    let mut scanner = Scanner::new();
    let mut shown = Shown::default();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut n = 0u32;
        // SAFETY: reads into a local buffer of the length given.
        let ok = unsafe {
            ReadFile(
                out,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut n,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || n == 0 {
            break;
        }
        let trigger = scanner.feed(&buf[..n as usize]);
        win32.store(scanner.win32_input(), Ordering::SeqCst);
        if shown.alloc.is_none() && trigger.is_some() {
            // The next task shows the window here; until then the output is dropped.
            shown.alloc = Some(Alloc::None);
        }
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn changes_apply_without_regard_to_case_and_the_result_is_sorted() {
        let base = vec![
            (os("Path"), os("a")),
            (os("ZED"), os("z")),
            (os("keep"), os("k")),
        ];
        let changes = vec![
            (os("PATH"), Some(os("b"))),
            (os("zed"), None),
            (os("NEW"), Some(os("n"))),
        ];
        assert_eq!(
            merge_env(base, &changes),
            vec![
                (os("keep"), os("k")),
                (os("NEW"), os("n")),
                (os("PATH"), os("b"))
            ]
        );
    }

    #[test]
    fn the_block_is_nul_separated_and_double_terminated() {
        let block = env_block(&[(os("A"), os("1")), (os("B"), os("ñ"))]);
        let want: Vec<u16> = "A=1\0B=ñ\0\0".encode_utf16().collect();
        assert_eq!(block, want);
        assert_eq!(env_block(&[]), vec![0, 0]);
    }

    #[test]
    fn the_command_line_is_the_quoted_program_and_the_raw_tail() {
        let line = command_line(
            Path::new(r"C:\a b\python.exe"),
            Some(OsStr::new(r#" "x y" z"#)),
        );
        assert_eq!(
            String::from_utf16(&line).unwrap(),
            "\"C:\\a b\\python.exe\"  \"x y\" z\0"
        );
        let line = command_line(Path::new(r"C:\p.exe"), None);
        assert_eq!(String::from_utf16(&line).unwrap(), "\"C:\\p.exe\"\0");
    }
}
