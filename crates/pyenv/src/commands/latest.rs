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

pub fn latest(ctx: &Ctx, args: &[&str]) -> Output {
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
