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

    pub fn name(self) -> String {
        match self {
            Hold::Off => "off".to_string(),
            Hold::Key => "key".to_string(),
            Hold::Seconds(n) => format!("{n}s"),
        }
    }
}

/// `STATUS_CONTROL_C_EXIT`: how a program ended by Ctrl+C exits.
pub const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

/// Whether an exit code is a failure worth keeping the window open for: non-zero, and
/// not Ctrl+C's.
pub fn should_hold(code: u32) -> bool {
    code != 0 && code != STATUS_CONTROL_C_EXIT
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

    /// Review focus 4: an exit from Ctrl+C isn't a failure to hold a window for.
    #[test]
    fn only_a_real_failure_holds_the_window() {
        assert!(!should_hold(0));
        assert!(should_hold(1));
        assert!(should_hold(0xC000_0005));
        assert!(!should_hold(STATUS_CONTROL_C_EXIT));
    }
}
