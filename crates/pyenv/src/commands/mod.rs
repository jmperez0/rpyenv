//! The command table.

pub mod misc;

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

pub type Command = fn(&Ctx, &[&str]) -> Output;

/// Commands on both OSes. Later tasks add rows.
const COMMANDS: &[(&str, Command)] = &[
    ("--version", misc::version_cmd),
    ("commands", misc::commands),
    ("help", misc::help),
    ("root", misc::root),
];

/// pyenv-win only.
const WIN_ONLY: &[(&str, Command)] = &[];

fn table(flavor: Flavor) -> impl Iterator<Item = &'static (&'static str, Command)> {
    let extra: &'static [(&'static str, Command)] = if flavor == Flavor::PyenvWin {
        WIN_ONLY
    } else {
        &[]
    };
    COMMANDS.iter().chain(extra)
}

pub fn lookup(flavor: Flavor, name: &str) -> Option<Command> {
    table(flavor).find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// The names `pyenv commands` prints, in the flavor's order.
pub fn names(flavor: Flavor) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = table(flavor).map(|(n, _)| *n).collect();
    match flavor {
        // `sort -u` in the C locale.
        Flavor::Pyenv => names.sort_unstable(),
        // pyenv-win lists `libexec\pyenv-<name>.<ext>` in NTFS order (allowlist D-16).
        Flavor::PyenvWin => names.sort_by_key(|n| format!("{}.", n.to_ascii_uppercase())),
    }
    names
}
