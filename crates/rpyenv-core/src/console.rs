//! How a Windows shim starts its child, depending on the console it has
//! (docs/windows-lazy-console.md, rule 2). M1 has three modes; EAGER and LAZY are M5.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleMode {
    /// Attached to a console: the child shares it, and the caller's arrangements stand.
    Inherit,
    /// No console, output redirected: `CREATE_NO_WINDOW`, so no window ever appears.
    NoWindow,
    /// No console otherwise: `DETACHED_PROCESS`, as the caller gave the shim.
    Mirror,
}

impl ConsoleMode {
    pub fn name(self) -> &'static str {
        match self {
            ConsoleMode::Inherit => "INHERIT",
            ConsoleMode::NoWindow => "NO-WINDOW",
            ConsoleMode::Mirror => "MIRROR",
        }
    }
}

/// The decision tree, without the 24H2 branches.
pub fn choose(attached: bool, stdout_redirected: bool, stderr_redirected: bool) -> ConsoleMode {
    if attached {
        ConsoleMode::Inherit
    } else if stdout_redirected && stderr_redirected {
        ConsoleMode::NoWindow
    } else {
        ConsoleMode::Mirror
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decision_tree() {
        use ConsoleMode::*;
        let cases = [
            ((true, false, false), Inherit),
            ((true, true, true), Inherit),
            ((false, true, true), NoWindow),
            ((false, true, false), Mirror),
            ((false, false, true), Mirror),
            ((false, false, false), Mirror),
        ];
        for ((attached, out, err), mode) in cases {
            assert_eq!(choose(attached, out, err), mode, "{attached} {out} {err}");
        }
        assert_eq!(NoWindow.name(), "NO-WINDOW");
    }
}
