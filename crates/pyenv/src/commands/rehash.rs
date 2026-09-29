//! `rehash` and `shims`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::installed;
use rpyenv_core::rehash::{self, RehashError, Wait};
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

pub fn rehash(ctx: &Ctx, _args: &[&str]) -> Output {
    if ctx.flavor == Flavor::PyenvWin
        && installed::top_level(&ctx.versions_dir(), Flavor::PyenvWin).is_empty()
    {
        let mut o = Output::new();
        o.out("No version installed. Please install one with 'pyenv install <version>'.");
        return o;
    }
    let Some(exe) = crate::shim_exe().filter(|e| e.is_file()) else {
        return fail(
            ctx,
            &["pyenv: cannot rehash: pyenv-shim is missing next to pyenv".to_string()],
        );
    };
    let timeout = std::env::var("PYENV_REHASH_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(60)
        .max(0);
    let lock = ctx.shims_dir().join(rehash::LOCK_NAME);
    match rehash::rehash(ctx, &exe, Wait::Upto(Duration::from_secs(timeout as u64))) {
        Ok(_) => Output::new(),
        Err(RehashError::NotWritable(dir)) => fail(
            ctx,
            &[format!("pyenv: cannot rehash: {} isn't writable", dir.display())],
        ),
        // The second line stands in for bash's noclobber error (allowlist D-35).
        Err(RehashError::Timeout(_) | RehashError::Busy) => fail(
            ctx,
            &[
                format!(
                    "pyenv: cannot rehash: couldn't acquire lock {} for {timeout} seconds. Last error message:",
                    lock.display()
                ),
                format!("{}: cannot overwrite existing file", lock.display()),
            ],
        ),
        Err(RehashError::Io(e)) => fail(ctx, &[format!("pyenv: cannot rehash: {e}")]),
    }
}

/// Upstream prints rehash errors on stderr; on Windows they follow pyenv-win's stdout
/// convention (allowlist D-36).
fn fail(ctx: &Ctx, lines: &[String]) -> Output {
    let mut o = Output::new();
    for line in lines {
        match ctx.flavor {
            Flavor::Pyenv => o.err(line),
            Flavor::PyenvWin => o.out(line),
        }
    }
    o.with_code(1)
}

pub fn shims(ctx: &Ctx, args: &[&str]) -> Output {
    let dir = ctx.shims_dir();
    let mut o = Output::new();
    match ctx.flavor {
        Flavor::Pyenv => {
            let short = args.first() == Some(&"--short");
            let mut names = visible_entries(&dir);
            // Byte order rather than the locale's collation (allowlist D-37).
            names.sort();
            for n in names {
                if short {
                    o.out(n.to_string_lossy());
                } else {
                    o.out(dir.join(&n).display().to_string());
                }
            }
        }
        // pyenv-win relays `dir` output and adds one more line ending (pyenv.vbs:69-82).
        Flavor::PyenvWin => match args.first() {
            None => {
                list_recursive(&dir, &mut o);
                o.out("");
            }
            Some(&"--short") => {
                for n in visible_entries(&dir) {
                    o.out(n.to_string_lossy());
                }
                o.out("");
            }
            Some(_) => return super::which::print_help("shims").with_code(0),
        },
    }
    o
}

/// `dir /s /b`: a folder's entries, then each subfolder's, depth first.
fn list_recursive(dir: &Path, o: &mut Output) {
    let names = visible_entries(dir);
    for n in &names {
        o.out(dir.join(n).display().to_string());
    }
    for n in &names {
        let p = dir.join(n);
        if p.is_dir() {
            list_recursive(&p, o);
        }
    }
}

/// Entries in directory order, without names starting with `.` (rpyenv's own files) and,
/// on Windows, without hidden or system files, which `dir` leaves out.
fn visible_entries(dir: &Path) -> Vec<OsString> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| !hidden(e))
        .map(|e| e.file_name())
        .collect()
}

#[cfg(windows)]
fn hidden(e: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
    e.metadata()
        .map(|m| m.file_attributes() & HIDDEN_OR_SYSTEM != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn hidden(_: &std::fs::DirEntry) -> bool {
    false
}
