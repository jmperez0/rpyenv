//! How a Windows shim starts its child, depending on the console it has
//! (docs/windows-lazy-console.md, rule 2). The decision and the settings are pure, so
//! their tests run on every OS.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleMode {
    /// Attached to a console: the child shares it, and the caller's arrangements stand.
    Inherit,
    /// No console, output redirected: `CREATE_NO_WINDOW`, so no window ever appears.
    NoWindow,
    /// No console and no way to ask for one later (before Windows 11 24H2, `pyenv exec`,
    /// the GUI shim): `DETACHED_PROCESS`, as the caller gave the shim.
    Mirror,
    /// Asks Windows at once for the console the caller wanted
    /// (`ALLOC_CONSOLE_MODE_DEFAULT`), then starts the child on it, or with
    /// `DETACHED_PROCESS` when the caller wanted none.
    Eager,
    /// Runs the child on a pseudo-console and asks for the caller's console only when the
    /// child first prints something.
    Lazy,
}

impl ConsoleMode {
    pub fn name(self) -> &'static str {
        match self {
            ConsoleMode::Inherit => "INHERIT",
            ConsoleMode::NoWindow => "NO-WINDOW",
            ConsoleMode::Mirror => "MIRROR",
            ConsoleMode::Eager => "EAGER",
            ConsoleMode::Lazy => "LAZY",
        }
    }
}

/// `RPYENV_CONSOLE`: what the console shim does when it starts without a console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Lazy,
    Eager,
}

impl Setting {
    /// `eager`, in any case, selects EAGER. Anything else, or unset, is the default, LAZY
    /// (user decision 2026-10-06).
    pub fn parse(value: Option<&str>) -> Setting {
        match value {
            Some(v) if v.trim().eq_ignore_ascii_case("eager") => Setting::Eager,
            _ => Setting::Lazy,
        }
    }
}

/// What the decision needs to know about the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Situation {
    pub attached: bool,
    pub stdout_redirected: bool,
    pub stderr_redirected: bool,
    /// This is the console shim, whose manifest asks for no console at startup, on a
    /// Windows that honors it (`AllocConsoleWithOptions` exists: build 26100 or later).
    pub detached_policy: bool,
    /// The shim's parent has a console. A shim it started without one was given
    /// `DETACHED_PROCESS` or `CREATE_NEW_CONSOLE`; only EAGER's allocation tells which.
    pub parent_has_console: bool,
    /// The caller gave the shim at least one standard handle (a file, a pipe, NUL). LAZY
    /// would replace them with the pseudo-console's, so the shim takes EAGER and the child
    /// keeps them. Explorer gives none.
    pub handles_given: bool,
    pub setting: Setting,
}

/// The decision tree (docs/windows-lazy-console.md, rule 2, as built in M5a).
pub fn choose(s: Situation) -> ConsoleMode {
    if s.attached {
        ConsoleMode::Inherit
    } else if s.stdout_redirected && s.stderr_redirected {
        ConsoleMode::NoWindow
    } else if !s.detached_policy {
        ConsoleMode::Mirror
    } else if s.parent_has_console
        || s.handles_given
        || s.setting == Setting::Eager
        || s.stdout_redirected
        || s.stderr_redirected
    {
        ConsoleMode::Eager
    } else {
        ConsoleMode::Lazy
    }
}

/// `RPYENV_CONSOLE_HOLD`: whether a window the shim opened stays on screen after the
/// child fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Off,
    Key,
    Seconds(u32),
}

impl Hold {
    /// `0` turns it off. A positive whole number closes the window after that many
    /// seconds, or sooner on a key. Anything else, or unset, waits for a key (user decision
    /// 2026-10-06).
    pub fn parse(value: Option<&str>) -> Hold {
        match value.map(str::trim).and_then(|v| v.parse::<u32>().ok()) {
            Some(0) => Hold::Off,
            Some(n) => Hold::Seconds(n),
            None => Hold::Key,
        }
    }

    /// How long to wait for a key, in milliseconds: `u32::MAX` (Windows' `INFINITE`) for
    /// `Key`, never reached by a number of seconds, however large.
    pub fn wait_ms(self) -> u32 {
        match self {
            Hold::Off => 0,
            Hold::Key => u32::MAX,
            Hold::Seconds(n) => n.saturating_mul(1000).min(u32::MAX - 1),
        }
    }

    pub fn name(self) -> String {
        match self {
            Hold::Off => "off".to_string(),
            Hold::Key => "key".to_string(),
            Hold::Seconds(n) => format!("{n}s"),
        }
    }
}

/// Whether a key event ends a hold: a key pressed down, unless it's a modifier or lock key
/// on its own (Shift, Ctrl, Alt, the Windows keys, Caps/Num/Scroll Lock), so that Ctrl, as
/// the start of Ctrl+C to copy the traceback, doesn't close the window.
pub fn ends_hold(vk: u16, down: bool) -> bool {
    const MODIFIERS: [u16; 14] = [
        0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x90, 0x91, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5,
    ];
    down && !MODIFIERS.contains(&vk)
}

/// How a window the shim opened behaves after the child ended with `code`: `Some` keeps
/// it as `hold` says, `None` closes it at once. It closes at once when `code` isn't a
/// failure (`should_hold`), when nobody can see the window (`visible` false: a scheduled
/// task or a service, where waiting for a key would never end), and when a console parent
/// asked for the window (`start /wait` in a script must get its errorlevel, as with
/// python.exe).
pub fn hold_wait(
    code: u32,
    hold: Hold,
    visible: bool,
    asked_by_console_parent: bool,
) -> Option<Hold> {
    (should_hold(code) && visible && !asked_by_console_parent && hold != Hold::Off).then_some(hold)
}

/// `STATUS_CONTROL_C_EXIT`: how a program ended by Ctrl+C exits.
pub const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// Whether an exit code is a failure worth keeping the window open for: non-zero, and not
/// an interruption (Ctrl+C's `STATUS_CONTROL_C_EXIT`, or 130, which programs that catch
/// Ctrl+C commonly exit with).
pub fn should_hold(code: u32) -> bool {
    code != 0 && code != STATUS_CONTROL_C_EXIT && code != 130
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(
        attached: bool,
        out: bool,
        err: bool,
        policy: bool,
        parent: bool,
        setting: Setting,
    ) -> Situation {
        Situation {
            attached,
            stdout_redirected: out,
            stderr_redirected: err,
            detached_policy: policy,
            parent_has_console: parent,
            handles_given: false,
            setting,
        }
    }

    #[test]
    fn the_decision_tree() {
        use ConsoleMode::*;
        use Setting::{Eager as E, Lazy as L};
        let cases = [
            // attached: whatever else holds
            (s(true, false, false, true, false, L), Inherit),
            (s(true, true, true, true, true, E), Inherit),
            // both outputs redirected
            (s(false, true, true, true, false, L), NoWindow),
            (s(false, true, true, false, false, L), NoWindow),
            // no policy (pre-24H2, exec, GUI shim): M1's tree
            (s(false, true, false, false, false, L), Mirror),
            (s(false, false, false, false, true, L), Mirror),
            // policy: a console parent detached us or asked for a new window
            (s(false, false, false, true, true, L), Eager),
            // policy: the setting, then partial redirection
            (s(false, false, false, true, false, E), Eager),
            (s(false, true, false, true, false, L), Eager),
            (s(false, false, true, true, false, L), Eager),
            (s(false, false, false, true, false, L), Lazy),
        ];
        for (situation, mode) in cases {
            assert_eq!(choose(situation), mode, "{situation:?}");
        }
        assert_eq!(NoWindow.name(), "NO-WINDOW");
        assert_eq!(Eager.name(), "EAGER");
        assert_eq!(Lazy.name(), "LAZY");
    }

    #[test]
    fn rpyenv_console_is_lazy_unless_eager() {
        assert_eq!(Setting::parse(None), Setting::Lazy);
        assert_eq!(Setting::parse(Some("")), Setting::Lazy);
        assert_eq!(Setting::parse(Some("lazy")), Setting::Lazy);
        assert_eq!(Setting::parse(Some("bogus")), Setting::Lazy);
        assert_eq!(Setting::parse(Some("eager")), Setting::Eager);
        assert_eq!(Setting::parse(Some(" EAGER ")), Setting::Eager);
    }

    #[test]
    fn rpyenv_console_hold_values() {
        assert_eq!(Hold::parse(None), Hold::Key);
        assert_eq!(Hold::parse(Some("")), Hold::Key);
        assert_eq!(Hold::parse(Some("yes")), Hold::Key);
        assert_eq!(Hold::parse(Some("-1")), Hold::Key);
        assert_eq!(Hold::parse(Some("0")), Hold::Off);
        assert_eq!(Hold::parse(Some(" 0 ")), Hold::Off);
        assert_eq!(Hold::parse(Some("1")), Hold::Seconds(1));
        assert_eq!(Hold::parse(Some("30")), Hold::Seconds(30));
        assert_eq!(Hold::Key.name(), "key");
        assert_eq!(Hold::Off.name(), "off");
        assert_eq!(Hold::Seconds(3).name(), "3s");
    }

    /// A held window closes on a real key, not on a modifier pressed alone: Ctrl, as the
    /// start of Ctrl+C to copy the traceback, must not close it.
    #[test]
    fn modifier_keys_alone_do_not_end_a_hold() {
        for vk in [0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0x90, 0x91, 0xA0, 0xA5] {
            assert!(!ends_hold(vk, true), "{vk:#x}");
        }
        assert!(ends_hold(0x0D, true));
        assert!(ends_hold(0x41, true));
        assert!(ends_hold(0x20, true));
        assert!(!ends_hold(0x41, false));
    }

    /// Final review C1 and I2: no hold where nobody can see the window (a scheduled task
    /// or service would wait forever), nor when a console parent asked for the window
    /// (`start /wait` in a script must get its errorlevel, as with python.exe).
    #[test]
    fn when_a_window_is_held() {
        assert_eq!(hold_wait(3, Hold::Key, true, false), Some(Hold::Key));
        assert_eq!(
            hold_wait(3, Hold::Seconds(2), true, false),
            Some(Hold::Seconds(2))
        );
        assert_eq!(hold_wait(3, Hold::Off, true, false), None);
        assert_eq!(hold_wait(0, Hold::Key, true, false), None);
        assert_eq!(
            hold_wait(STATUS_CONTROL_C_EXIT, Hold::Key, true, false),
            None
        );
        assert_eq!(
            hold_wait(3, Hold::Key, false, false),
            None,
            "invisible window station"
        );
        assert_eq!(
            hold_wait(3, Hold::Key, true, true),
            None,
            "a console parent asked"
        );
    }

    /// Final review I3: a caller that gave the shim any standard handle (stdin from a file,
    /// output to NUL) gets EAGER, so the child keeps those handles; only a caller that gave
    /// none (Explorer) gets LAZY.
    #[test]
    fn any_given_handle_means_eager() {
        let mut lazy = s(false, false, false, true, false, Setting::Lazy);
        assert_eq!(choose(lazy), ConsoleMode::Lazy);
        lazy.handles_given = true;
        assert_eq!(choose(lazy), ConsoleMode::Eager);
    }

    /// Final review minor 12: a huge N waits long, never forever (INFINITE is u32::MAX).
    #[test]
    fn hold_wait_times() {
        assert_eq!(Hold::Key.wait_ms(), u32::MAX);
        assert_eq!(Hold::Seconds(3).wait_ms(), 3000);
        assert!(Hold::Seconds(u32::MAX).wait_ms() < u32::MAX);
        assert_eq!(Hold::Off.wait_ms(), 0);
    }

    /// Review focus 4: an exit from Ctrl+C isn't a failure to hold a window for.
    #[test]
    fn only_a_real_failure_holds_the_window() {
        assert!(!should_hold(0));
        assert!(should_hold(1));
        assert!(should_hold(0xC000_0005));
        assert!(!should_hold(STATUS_CONTROL_C_EXIT));
        // Final review minor 14: 130, the usual exit after an interruption, isn't either.
        assert!(!should_hold(130));
    }
}
