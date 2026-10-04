//! The installer (spec §9): downloads, extraction, the install transaction, python-build
//! definitions and the Linux source build.

pub mod archive;
#[cfg(unix)]
pub mod builder;
pub mod checksum;
pub mod default_packages;
pub mod defs;
pub mod fetch;
pub mod log;
pub mod msi;
pub mod openpgp;
#[cfg(unix)]
pub mod preflight;
#[cfg(unix)]
mod reply;
pub mod txn;
#[cfg(unix)]
pub mod verify;
pub mod wincatalog;
pub mod winpkg;
pub mod winsource;
pub mod zipx;

#[cfg(unix)]
pub use reply::{prompt, Reply};

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether `name` is exactly one plain path component: not empty, `.` or `..`, no root or
/// drive, and no `/` or `\` (review I2). Package, file and version names are joined to
/// directories whose results are written and deleted.
pub fn is_plain_name(name: &str) -> bool {
    let mut c = Path::new(name).components();
    matches!((c.next(), c.next()), (Some(Component::Normal(_)), None))
        && !name.contains(['/', '\\'])
}

/// `dir/name`, but only when that is a direct child of `dir` (the second guard before a
/// path built from a name is written or deleted).
pub fn child_of(dir: &Path, name: &str) -> Option<PathBuf> {
    let p = dir.join(name);
    (p.parent() == Some(dir) && p.file_name() == Some(OsStr::new(name))).then_some(p)
}

/// One path segment that Windows writes as a plain file or directory name: not empty, `.` or
/// `..`; no separator, drive colon, wildcard or control character; not a DOS device name
/// (`CON`, `NUL`, `COM1`…, with or without an extension); and no trailing dot or space, which
/// Windows strips silently. Used for every name that comes from an archive or MSI table.
pub fn is_safe_win_segment(seg: &str) -> bool {
    const DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if seg.is_empty() || seg == "." || seg == ".." || seg.ends_with(['.', ' ']) {
        return false;
    }
    if seg.chars().any(|c| {
        matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*') || (c as u32) < 0x20
    }) {
        return false;
    }
    let stem = seg.split('.').next().unwrap_or("").trim_end();
    !DEVICES.iter().any(|d| stem.eq_ignore_ascii_case(d))
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Records Ctrl+C instead of dying, so each step can stop and roll back (spec §9.3).
/// Child processes in the same process group get the signal themselves.
pub fn watch_interrupt() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = ctrlc::set_handler(|| INTERRUPTED.store(true, Ordering::SeqCst));
    });
}

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// Ctrl+C: roll back, then exit 130.
    Interrupted,
    /// The reason is already on stderr or in the log; exit 1.
    Failed,
    /// One line for stderr; exit 1.
    Message(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_segments() {
        for ok in [
            "python.exe",
            "Lib",
            "a b",
            "x.tar.gz",
            "COM10",
            "console.py",
            "nul_x",
        ] {
            assert!(super::is_safe_win_segment(ok), "{ok}");
        }
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "C:",
            "a:b",
            "x*",
            "x?",
            "a|b",
            "a\u{1}b",
            "CON",
            "con",
            "NUL.txt",
            "com1.py",
            "LPT9",
            "trailing.",
            "trailing ",
        ] {
            assert!(!super::is_safe_win_segment(bad), "{bad:?}");
        }
    }
}
