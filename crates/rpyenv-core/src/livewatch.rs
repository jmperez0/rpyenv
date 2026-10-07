//! The live rehash watcher (spec §8 point 3, `RPYENV_LIVE_REHASH=1`): while a long-running
//! program runs (Jupyter's `%pip install`), a new script gets its shim without waiting for
//! the program to exit.

use crate::ctx::Ctx;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long the watcher waits before it starts: most commands finish sooner and don't need
/// it (spec §8).
pub const START_DELAY: Duration = Duration::from_secs(1);
/// Changes must stop for this long before one rehash runs (spec §8).
pub const QUIET: Duration = Duration::from_millis(500);

/// Whether this shim starts a watcher: `RPYENV_LIVE_REHASH` is exactly `1`, and this
/// process isn't PID 1. Orphans go back to PID 1, which would be the program itself
/// (spec §8).
pub fn should_start(own_pid: u32, setting: Option<&str>) -> bool {
    setting == Some("1") && own_pid != 1
}

/// The one rehash that follows a burst of changes: each change moves the deadline to
/// `QUIET` after it, and the rehash is due once the deadline passes.
#[derive(Debug, Default, Clone, Copy)]
pub struct Debounce {
    due: Option<Instant>,
}

impl Debounce {
    pub fn changed(&mut self, now: Instant) {
        self.due = Some(now + QUIET);
    }

    /// How long to wait for more changes; `None` when no rehash is pending.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.due.map(|d| d.saturating_duration_since(now))
    }

    /// True, once, when the pending rehash is due.
    pub fn fire(&mut self, now: Instant) -> bool {
        match self.due {
            Some(d) if now >= d => {
                self.due = None;
                true
            }
            _ => false,
        }
    }
}

/// Keeps a Windows watcher thread running until it's dropped (after the program and the
/// exit check). Nothing on Linux, where the watcher is a detached process.
#[derive(Default)]
pub struct Guard {
    #[cfg(windows)]
    _running: Option<crate::livewatch_win::Running>,
}

/// Starts the watcher for this shim when `RPYENV_LIVE_REHASH=1` (spec §8 point 3). Any
/// failure skips it silently; the exit check still runs.
pub fn start(ctx: &Ctx, shim_exe: Option<&Path>) -> Guard {
    let setting = std::env::var("RPYENV_LIVE_REHASH").ok();
    let Some(_exe) = shim_exe else {
        return Guard::default();
    };
    if !should_start(std::process::id(), setting.as_deref()) {
        if setting.as_deref() == Some("1") {
            crate::debuglog::append("live=skipped pid1");
        }
        return Guard::default();
    }
    #[cfg(target_os = "linux")]
    crate::livewatch_linux::spawn(ctx, _exe);
    #[cfg(windows)]
    return Guard {
        _running: crate::livewatch_win::spawn(ctx, _exe),
    };
    #[cfg(not(windows))]
    {
        let _ = ctx;
        Guard::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn it_starts_only_when_asked_and_never_as_pid_1() {
        assert!(should_start(4321, Some("1")));
        assert!(!should_start(4321, None));
        assert!(!should_start(4321, Some("0")));
        assert!(!should_start(4321, Some("yes")));
        assert!(
            !should_start(1, Some("1")),
            "PID 1 (a container without init)"
        );
    }

    #[test]
    fn one_rehash_once_changes_stop_for_the_quiet_period() {
        let t0 = Instant::now();
        let mut d = Debounce::default();
        assert_eq!(d.wait(t0), None);
        assert!(!d.fire(t0));
        d.changed(t0);
        assert_eq!(d.wait(t0), Some(QUIET));
        assert!(!d.fire(t0 + QUIET - Duration::from_millis(1)));
        // A later change moves the deadline.
        d.changed(t0 + Duration::from_millis(300));
        assert!(!d.fire(t0 + QUIET));
        assert!(d.fire(t0 + Duration::from_millis(300) + QUIET));
        assert!(!d.fire(t0 + Duration::from_secs(5)), "fires once");
        assert_eq!(d.wait(t0 + Duration::from_secs(5)), None);
    }
}
