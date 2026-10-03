//! The installer (spec §9): downloads, extraction, the install transaction, python-build
//! definitions and the Linux source build.

pub mod archive;
#[cfg(unix)]
pub mod builder;
pub mod checksum;
#[cfg(unix)]
pub mod default_packages;
pub mod defs;
pub mod fetch;
pub mod log;
#[cfg(unix)]
pub mod preflight;
#[cfg(unix)]
mod reply;
pub mod txn;
#[cfg(unix)]
pub mod verify;

#[cfg(unix)]
pub use reply::{prompt, Reply};

use std::sync::atomic::{AtomicBool, Ordering};

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
