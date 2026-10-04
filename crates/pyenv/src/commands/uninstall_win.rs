//! `pyenv uninstall` for the pyenv-win flavor (docs/parity/pyenv-win-m2-reference.md
//! "uninstall"). No registry keys are touched (plan M2b Decision 11), so pyenv-win's
//! alternating false error can't happen; names never reach outside `versions\` (review
//! focus 4). Lines stream to stdout in CRLF as each version goes.

use crate::install::{child_of, is_plain_name};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::installed;
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> [<version> ...]\n       pyenv uninstall [-f|--force] [-a|--all]\n\n   -f/--force  Attempt to remove the specified version without prompting\n               for confirmation. If the version does not exist, do not\n               display an error message.\n\n   -a/--all    *Caution* Attempt to remove all installed versions.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";

fn say(line: &str) {
    rpyenv_core::textout::write(false, &format!("{line}\r\n"));
}

/// pyenv-win's `IsVersion`: `^[a-zA-Z_0-9-.]+$`.
fn is_version(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// `versions\<name>` for a name that can only be a version folder: one plain component, not
/// `.`/`..`, not an installer staging name.
fn target(versions: &Path, name: &str) -> Option<PathBuf> {
    if !is_plain_name(name) || installed::is_staging_name(name) {
        return None;
    }
    child_of(versions, name)
}

/// pyenv-win's `-a` prompt: `y` yes; `n`, an empty line or EOF no; anything else asks again.
fn confirm() -> bool {
    let stdin = std::io::stdin();
    loop {
        rpyenv_core::textout::write(false, "pyenv: Confirm uninstall all? (Y/N): ");
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        match line.trim().chars().next().map(|c| c.to_ascii_lowercase()) {
            Some('y') => return true,
            Some('n') | None => return false,
            Some(_) => {}
        }
    }
}

/// The install lock M2a's transaction holds (`<root>\.locks\install-<name>`): a version being
/// installed is not pulled away.
fn locked(root: &Path, name: &str) -> bool {
    let p = root.join(".locks").join(format!("install-{name}"));
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p)
        .is_ok_and(|l| matches!(l.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
}

pub fn uninstall(ctx: &Ctx, args: &[&str]) -> Output {
    let (mut force, mut all) = (false, false);
    let mut names: Vec<String> = Vec::new();
    for a in args {
        match *a {
            "--help" => {
                let mut o = Output::new();
                o.stdout.push_str(HELP);
                return o;
            }
            "-f" | "--force" => force = true,
            "-a" | "--all" => all = true,
            v if is_version(v) => {
                // pyenv-win's `Check32Bit`.
                let lower = v.to_ascii_lowercase();
                names.push(
                    if ctx.arch_suffix == "-win32" && !lower.ends_with("-win32") {
                        format!("{v}-win32")
                    } else {
                        v.to_string()
                    },
                );
            }
            v => {
                let mut o = Output::new();
                o.out(format!("pyenv: Unrecognized python version: {v}"));
                return o.with_code(1);
            }
        }
    }
    if names.is_empty() && !all {
        let mut o = Output::new();
        o.stdout.push_str(HELP);
        return o;
    }
    let versions = ctx.versions_dir();
    let installed_names = installed::names(&versions, Flavor::PyenvWin);
    if installed_names.is_empty() {
        let mut o = Output::new();
        o.out("pyenv: No valid versions of python installed.");
        return o.with_code(1);
    }
    if all {
        if !force && !confirm() {
            return Output::new();
        }
        names = installed_names
            .into_iter()
            .filter(|n| is_version(n))
            .collect();
    }
    let single = names.len() == 1;
    let mut status = 0;
    for n in &names {
        let Some(p) = target(&versions, n).filter(|p| p.is_dir()) else {
            if single {
                say(&format!("pyenv: version '{n}' not installed"));
            }
            continue;
        };
        if locked(&ctx.root, n) {
            say(&format!(
                "pyenv: an install of {n} is in progress ({})",
                ctx.root
                    .join(".locks")
                    .join(format!("install-{n}"))
                    .display()
            ));
            status = 1;
            continue;
        }
        let is_link = p
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        let gone = if is_link {
            std::fs::remove_dir(&p)
        } else {
            std::fs::remove_dir_all(&p)
        };
        match gone {
            Ok(()) => say(&format!("pyenv: Successfully uninstalled {n}")),
            Err(e) => {
                say(&format!("pyenv: Error uninstalling version {n}: {e}"));
                status = 1;
            }
        }
    }
    if status == 0 {
        let r = crate::commands::rehash::rehash(ctx, &[]);
        if r.code != 0 {
            r.emit(ctx.flavor);
            status = r.code;
        }
    }
    Output::new().with_code(status)
}
