//! `pyenv latest [-k|--known] [-b|--bypass] [-f|--force] <prefix>` (M1 reference).

use crate::install::defs;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::installed;
use rpyenv_core::latest::{best, latest as latest_installed};

/// The known-mode winner for `prefix` among `names` (definitions), for `pyenv install`.
pub fn resolve_known(prefix: &str, names: &[String]) -> Option<String> {
    best(prefix, names)
}

/// The prefix as upstream's messages show it: a trailing `t` after a digit is stripped.
fn shown(prefix: &str) -> &str {
    match prefix.strip_suffix('t') {
        Some(b) if b.ends_with(|c: char| c.is_ascii_digit()) => b,
        _ => prefix,
    }
}

/// pyenv-win's `pyenv-latest.vbs` help (M2 reference, "latest").
pub const WIN_HELP: &str = "Usage: pyenv latest [-k|--known] [-q|--quiet] <prefix>\n\n  -k/--known      Select from all known versions instead of installed\n  -q/--quiet      Do not print an error message on resolution failure\n\n";

/// pyenv-win's `pyenv latest` (M1 reference, "latest"): the last non-option argument is the
/// prefix; `--help` wins wherever it is; no arguments at all is the help with exit 1.
fn latest_win(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    let (mut known, mut quiet, mut prefix) = (false, false, "");
    for a in args {
        match *a {
            "--help" => {
                o.stdout.push_str(WIN_HELP);
                return o;
            }
            "-k" | "--known" => known = true,
            "-q" | "--quiet" => quiet = true,
            p => prefix = p,
        }
    }
    if args.is_empty() {
        o.stdout.push_str(WIN_HELP);
        return o.with_code(1);
    }
    if prefix.is_empty() {
        if !quiet {
            o.out("pyenv-latest: missing <prefix> argument");
        }
        return o.with_code(1);
    }
    let candidates: Vec<String> = if known {
        crate::install::wincatalog::read_db(&ctx.root)
            .map(|rows| rows.into_iter().map(|r| r.code).collect())
            .unwrap_or_default()
    } else {
        installed::names(&ctx.versions_dir(), ctx.flavor)
    };
    match rpyenv_core::winresolve::find_latest(prefix, &candidates, ctx.arch_suffix) {
        Some(v) => o.out(v),
        None => {
            if !quiet {
                let kind = if known { "known" } else { "installed" };
                o.out(format!(
                    "pyenv-latest: no {kind} versions match the prefix '{prefix}'."
                ));
            }
            o.code = 1;
        }
    }
    o
}

pub fn latest(ctx: &Ctx, args: &[&str]) -> Output {
    if ctx.flavor == rpyenv_core::flavor::Flavor::PyenvWin {
        return latest_win(ctx, args);
    }
    let (mut known, mut bypass, mut force) = (false, false, false);
    let mut i = 0;
    while let Some(a) = args.get(i) {
        match *a {
            "-k" | "--known" => known = true,
            "-b" | "--bypass" => bypass = true,
            "-f" | "--force" => force = true,
            _ => break,
        }
        i += 1;
    }
    let prefix = args.get(i).copied().unwrap_or("");
    let found = if known {
        let env = |k: &str| std::env::var(k).ok();
        resolve_known(prefix, &defs::known(&env))
    } else {
        // The same list prefix.rs and select.rs pass to `latest::latest`; it already
        // omits the installer's staging names.
        let names = installed::names(&ctx.versions_dir(), ctx.flavor);
        latest_installed(prefix, &names, &ctx.versions_dir())
    };
    let mut o = Output::new();
    match found {
        Some(v) => o.out(v),
        None if bypass || force => {
            o.out(shown(prefix));
            o.code = if force { 0 } else { 1 };
        }
        None => {
            let kind = if known { "known" } else { "installed" };
            o.err(format!(
                "pyenv: no {kind} versions match the prefix `{}'",
                shown(prefix)
            ));
            o.code = 1;
        }
    }
    o
}
