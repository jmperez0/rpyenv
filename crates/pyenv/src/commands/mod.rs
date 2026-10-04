//! The command table.

pub mod exec;
#[cfg(unix)]
pub mod install;
pub mod install_win;
pub mod latest;
pub mod local_global;
pub mod misc;
pub mod prefix;
pub mod rehash;
#[cfg(unix)]
pub mod uninstall;
pub mod update;
pub mod version;
pub mod versions;
pub mod which;

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

pub type Command = fn(&Ctx, &[&str]) -> Output;

/// Commands on both OSes. Later tasks add rows.
const COMMANDS: &[(&str, Command)] = &[
    ("--version", misc::version_cmd),
    ("commands", misc::commands),
    ("exec", exec::exec_listed),
    ("global", local_global::global),
    ("help", misc::help),
    ("local", local_global::local),
    ("prefix", prefix::prefix),
    ("rehash", rehash::rehash),
    ("root", misc::root),
    ("shims", rehash::shims),
    ("version", version::version),
    ("version-file", version::version_file),
    ("version-file-read", version::version_file_read),
    ("version-file-write", version::version_file_write),
    ("version-name", version::version_name),
    ("version-origin", version::version_origin),
    ("versions", versions::versions),
    ("whence", which::whence),
    ("which", which::which),
];

/// pyenv-win only: its installer commands (`install`, `update`) and the `vname` alias.
const WIN_ONLY: &[(&str, Command)] = &[
    ("install", install_win::install),
    ("update", update::update),
    ("vname", version::version_name),
];

/// pyenv (Linux) only, until M2b brings pyenv-win's installer commands.
const LINUX_ONLY: &[(&str, Command)] = &[
    #[cfg(unix)]
    ("install", install::install),
    ("latest", latest::latest),
    #[cfg(unix)]
    ("uninstall", uninstall::uninstall),
];

fn table(flavor: Flavor) -> impl Iterator<Item = &'static (&'static str, Command)> {
    let extra: &'static [(&'static str, Command)] = if flavor == Flavor::PyenvWin {
        WIN_ONLY
    } else {
        LINUX_ONLY
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
