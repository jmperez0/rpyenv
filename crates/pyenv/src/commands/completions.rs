//! `pyenv completions` and the `--complete` answers (libexec/pyenv-completions, and each
//! upstream command's "Provide pyenv completions" block; M3L, M1L). Decision 5: one table
//! per flavor instead of a block per command.

use crate::commands;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use std::ffi::OsString;

/// What follows a command's fixed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tail {
    Nothing,
    /// `pyenv-versions --bare`
    VersionsBare,
    /// `pyenv-shims --short`
    ShimsShort,
    /// `pyenv-commands`
    Commands,
    /// The installed version names.
    InstalledNames,
    /// `pyenv-rehash --complete`, which rehashes (Decision 11).
    Rehash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    Words(&'static [&'static str], Tail),
    /// The command handles `--complete` itself (Linux `install` and `uninstall`).
    Itself,
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub name: &'static str,
    /// Upstream's `# Provide pyenv completions` marker: whether `pyenv completions <cmd>`
    /// forwards to `<cmd> --complete`.
    pub marker: bool,
    pub answer: Answer,
}

const fn e(name: &'static str, marker: bool, answer: Answer) -> Entry {
    Entry {
        name,
        marker,
        answer,
    }
}

use Answer::{Itself, Words};
use Tail::{Commands, InstalledNames, Nothing, Rehash, ShimsShort, VersionsBare};

/// Linux, from upstream's scripts at 2.8.8. A command not listed has no `--complete`.
pub(crate) const TABLE_PYENV: &[Entry] = &[
    e("commands", true, Words(&["--sh", "--no-sh"], Nothing)),
    e("completions", true, Words(&[], Commands)),
    e("exec", true, Words(&["--environment"], ShimsShort)),
    e("global", true, Words(&["system"], VersionsBare)),
    e("help", true, Words(&["--usage"], Commands)),
    e(
        "init",
        true,
        Words(
            &[
                "-",
                "--path",
                "--install",
                "--no-push-path",
                "--no-rehash",
                "--detect-shell",
                "bash",
                "fish",
                "ksh",
                "pwsh",
                "zsh",
            ],
            Nothing,
        ),
    ),
    e("install", true, Itself),
    e("local", true, Words(&["--unset", "system"], VersionsBare)),
    e("prefix", true, Words(&["system"], VersionsBare)),
    e("sh-rehash", true, Words(&[], Rehash)),
    e(
        "sh-shell",
        true,
        Words(&["--unset", "system"], VersionsBare),
    ),
    e("shims", true, Words(&["--short"], Nothing)),
    e("uninstall", true, Itself),
    // Handles `--complete` but has no marker, so `pyenv completions version` is `--help`.
    e("version", false, Words(&["--bare"], Nothing)),
    e(
        "versions",
        true,
        Words(&["--bare", "--skip-aliases", "--skip-envs"], Nothing),
    ),
    e("whence", true, Words(&["--path"], ShimsShort)),
    e("which", true, Words(&[], ShimsShort)),
];

/// Windows: rpyenv's own (pyenv-win has none). The options are each pyenv-win command's.
pub(crate) const TABLE_WIN: &[Entry] = &[
    e("commands", true, Words(&[], Nothing)),
    e("completions", true, Words(&[], Commands)),
    e("exec", true, Words(&[], ShimsShort)),
    e("global", true, Words(&["--unset"], VersionsBare)),
    e("help", true, Words(&[], Commands)),
    e(
        "init",
        true,
        Words(
            &[
                "-",
                "--path",
                "--no-push-path",
                "--no-rehash",
                "--detect-shell",
                "bash",
                "cmd",
                "fish",
                "powershell",
                "pwsh",
                "zsh",
            ],
            Nothing,
        ),
    ),
    e(
        "install",
        true,
        Words(
            &[
                "--list",
                "--force",
                "--skip-existing",
                "--register",
                "--all",
                "--clear",
                "--32only",
                "--64only",
                "--quiet",
                "--dev",
            ],
            Nothing,
        ),
    ),
    e("local", true, Words(&["--unset"], VersionsBare)),
    e("prefix", true, Words(&[], VersionsBare)),
    e("sh-rehash", true, Words(&[], Nothing)),
    e("sh-shell", true, Words(&["--unset"], VersionsBare)),
    e("shell", true, Words(&["--unset"], VersionsBare)),
    e("shims", true, Words(&["--short"], Nothing)),
    e(
        "uninstall",
        true,
        Words(&["--force", "--all"], InstalledNames),
    ),
    e("update", true, Words(&["--ignore"], Nothing)),
    e(
        "versions",
        true,
        Words(&["--bare", "--skip-aliases"], Nothing),
    ),
    e("whence", true, Words(&["--path"], ShimsShort)),
    e("which", true, Words(&[], ShimsShort)),
];

pub(crate) fn table(flavor: Flavor) -> &'static [Entry] {
    match flavor {
        Flavor::Pyenv => TABLE_PYENV,
        Flavor::PyenvWin => TABLE_WIN,
    }
}

fn tail(ctx: &Ctx, t: Tail) -> Output {
    match t {
        Tail::Nothing => Output::new(),
        Tail::VersionsBare => commands::versions::versions(ctx, &["--bare"]),
        Tail::ShimsShort => commands::rehash::shims(ctx, &["--short"]),
        Tail::Commands => commands::misc::commands(ctx, &[]),
        Tail::InstalledNames => {
            let mut o = Output::new();
            for n in rpyenv_core::installed::names(&ctx.versions_dir(), ctx.flavor) {
                o.out(n);
            }
            o
        }
        Tail::Rehash => commands::rehash::rehash(ctx, &[]),
    }
}

/// `<cmd> --complete` from the table: the words, then the tail and its exit code. `None`
/// when the command answers it itself, or has no answer.
pub fn answer(ctx: &Ctx, cmd: &str) -> Option<Output> {
    let entry = table(ctx.flavor).iter().find(|e| e.name == cmd)?;
    let Answer::Words(words, t) = entry.answer else {
        return None;
    };
    let mut o = Output::new();
    for w in words {
        o.out(w);
    }
    let rest = tail(ctx, t);
    o.stdout.push_str(&rest.stdout);
    o.stderr.push_str(&rest.stderr);
    Some(o.with_code(rest.code))
}

/// `pyenv completions <command> [arg1 arg2...]` (libexec/pyenv-completions).
pub fn completions(ctx: &Ctx, args: &[&str]) -> Output {
    let Some(&cmd) = args.first().filter(|c| !c.is_empty()) else {
        return Output::error("Usage: pyenv completions <command> [arg1 arg2...]");
    };
    if cmd == "--complete" {
        return commands::misc::commands(ctx, &[]);
    }
    // `command -v pyenv-<cmd> || command -v pyenv-sh-<cmd>`: the plain command wins, so
    // `rehash` finds `pyenv-rehash`, which has no marker.
    let sh = format!("sh-{cmd}");
    let target = if commands::lookup(ctx.flavor, cmd).is_some() {
        cmd
    } else if commands::lookup(ctx.flavor, &sh).is_some() {
        sh.as_str()
    } else {
        // `set -e` ends the script when neither exists: no output at all.
        return Output::new().with_code(1);
    };
    let mut o = Output::new();
    o.out("--help");
    let Some(entry) = table(ctx.flavor)
        .iter()
        .find(|e| e.name == target && e.marker)
    else {
        return o;
    };
    let rest = match entry.answer {
        Answer::Words(..) => answer(ctx, target).unwrap_or_default(),
        Answer::Itself => {
            let mut a: Vec<OsString> = vec![target.into(), "--complete".into()];
            a.extend(args[1..].iter().map(OsString::from));
            crate::run(&a, ctx)
        }
    };
    o.stdout.push_str(&rest.stdout);
    o.stderr.push_str(&rest.stderr);
    o.with_code(rest.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Linux entry names a Linux command. Unix only: Linux `install` and
    /// `uninstall` exist only in Unix builds.
    #[cfg(unix)]
    #[test]
    fn every_linux_entry_names_a_command() {
        for e in TABLE_PYENV {
            assert!(
                commands::lookup(Flavor::Pyenv, e.name).is_some(),
                "{}",
                e.name
            );
        }
    }

    #[test]
    fn every_windows_entry_names_a_command() {
        for e in TABLE_WIN {
            assert!(
                commands::lookup(Flavor::PyenvWin, e.name).is_some(),
                "{}",
                e.name
            );
        }
    }
}
