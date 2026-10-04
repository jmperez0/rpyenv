//! The command table.

pub mod completions;
pub mod exec;
pub mod init;
pub mod init_win;
#[cfg(unix)]
pub mod install;
pub mod install_win;
pub mod latest;
pub mod local_global;
pub mod misc;
pub mod prefix;
pub mod rehash;
pub mod shell;
pub mod shell_win;
#[cfg(unix)]
pub mod uninstall;
pub mod uninstall_win;
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
    ("completions", completions::completions),
    ("exec", exec::exec_listed),
    ("global", local_global::global),
    ("help", misc::help),
    ("latest", latest::latest),
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
    ("init", init_win::init),
    ("install", install_win::install),
    ("uninstall", uninstall_win::uninstall),
    ("update", update::update),
    ("vname", version::version_name),
    ("sh-rehash", shell_win::sh_rehash),
    ("sh-shell", shell_win::sh_shell),
    ("shell", shell_win::shell),
];

/// pyenv (Linux) only, until M2b brings pyenv-win's installer commands.
const LINUX_ONLY: &[(&str, Command)] = &[
    #[cfg(unix)]
    ("install", install::install),
    #[cfg(unix)]
    ("uninstall", uninstall::uninstall),
    ("init", init::init),
    ("sh-rehash", shell::sh_rehash),
    ("sh-shell", shell::sh_shell),
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

/// Which commands `pyenv commands` lists (libexec/pyenv-commands:15-47).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// Every command, `sh-` stripped, so `sh-shell` lists as `shell`.
    All,
    /// `--sh`: only the `sh-` commands, stripped.
    ShOnly,
    /// `--no-sh`: every command but the `sh-` ones.
    NoSh,
}

/// The command table's names as they are (`sh-*` included), for the built-in links.
#[cfg(unix)]
pub fn builtin_names(flavor: Flavor) -> Vec<&'static str> {
    table(flavor).map(|&(n, _)| n).collect()
}

/// The names `pyenv commands` prints, each once, in the flavor's order.
pub fn names(flavor: Flavor, listing: Listing) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = table(flavor)
        .filter_map(|&(n, _)| {
            let short = n.strip_prefix("sh-");
            match listing {
                Listing::All => Some(short.unwrap_or(n)),
                Listing::ShOnly => short,
                Listing::NoSh => short.is_none().then_some(n),
            }
        })
        .collect();
    match flavor {
        // `sort -u` in the C locale.
        Flavor::Pyenv => names.sort_unstable(),
        // pyenv-win lists `libexec\pyenv-<name>.<ext>` in NTFS order (allowlist D-16).
        Flavor::PyenvWin => names.sort_by_key(|n| format!("{}.", n.to_ascii_uppercase())),
    }
    names.dedup();
    names
}
