//! Which upstream tool rpyenv imitates.

/// Linux follows pyenv; Windows follows pyenv-win (spec §4). Kept as a value,
/// not only a `cfg`, so both behaviors can be unit-tested on either OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Pyenv,
    PyenvWin,
}

impl Flavor {
    pub fn current() -> Flavor {
        if cfg!(windows) {
            Flavor::PyenvWin
        } else {
            Flavor::Pyenv
        }
    }

    /// The line ending of everything the CLI prints.
    pub fn eol(self) -> &'static str {
        match self {
            Flavor::Pyenv => "\n",
            Flavor::PyenvWin => "\r\n",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_flavor_matches_the_os() {
        let expected = if cfg!(windows) {
            Flavor::PyenvWin
        } else {
            Flavor::Pyenv
        };
        assert_eq!(Flavor::current(), expected);
    }

    #[test]
    fn line_endings() {
        assert_eq!(Flavor::Pyenv.eol(), "\n");
        assert_eq!(Flavor::PyenvWin.eol(), "\r\n");
    }
}
