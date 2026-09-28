//! `local` and `global`.

use crate::commands::version::write_checked;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{prefix, verfile};
use std::path::Path;

pub fn local(ctx: &Ctx, args: &[&str]) -> Output {
    let file = ctx.pwd.join(verfile::LOCAL_NAME);
    match ctx.flavor {
        Flavor::Pyenv => {
            let forced = args
                .iter()
                .take_while(|a| matches!(**a, "-f" | "--force"))
                .count();
            let rest = &args[forced..];
            if rest.first() == Some(&"--unset") {
                return remove_checked(ctx, &file);
            }
            if !rest.is_empty() {
                return write_checked(ctx, &file, rest, forced > 0);
            }
            let Some(found) = verfile::find_local(&ctx.pwd) else {
                return Output::error("pyenv: no local version configured for this directory");
            };
            let r = verfile::read_pyenv(&found, &found.display().to_string(), &ctx.versions_dir());
            let mut o = Output::new();
            for w in &r.warnings {
                o.err(w);
            }
            // An unusable file exits 1 with no message, as upstream does.
            if r.versions.is_empty() {
                return o.with_code(1);
            }
            for v in &r.versions {
                o.out(v);
            }
            o
        }
        Flavor::PyenvWin => {
            if args.first() == Some(&"--unset") {
                // A missing file is not an error (allowlist D-11).
                return remove_checked(ctx, &file);
            }
            if !args.is_empty() {
                return win_write(ctx, &file, args);
            }
            let lines = verfile::find_local(&ctx.pwd)
                .map(|f| verfile::read_pyenv_win(&f))
                .unwrap_or_default();
            win_show(&lines, "no local version configured for this directory")
        }
    }
}

pub fn global(ctx: &Ctx, args: &[&str]) -> Output {
    let file = ctx.global_version_file();
    match ctx.flavor {
        Flavor::Pyenv => {
            // Upstream has no flags and writes `-f` into the file; rpyenv treats it as a flag (allowlist D-14).
            let forced = args
                .iter()
                .take_while(|a| matches!(**a, "-f" | "--force"))
                .count();
            let rest = &args[forced..];
            if !rest.is_empty() {
                return write_checked(ctx, &file, rest, forced > 0);
            }
            let mut o = Output::new();
            for name in ["version", "global", "default"] {
                let f = ctx.root.join(name);
                let r = verfile::read_pyenv(&f, &f.display().to_string(), &ctx.versions_dir());
                for w in &r.warnings {
                    o.err(w);
                }
                if !r.versions.is_empty() {
                    for v in &r.versions {
                        o.out(v);
                    }
                    return o;
                }
            }
            o.out("system");
            o
        }
        Flavor::PyenvWin => {
            if args.first() == Some(&"--unset") {
                return remove_checked(ctx, &file);
            }
            if !args.is_empty() {
                return win_write(ctx, &file, args);
            }
            win_show(
                &verfile::read_pyenv_win(&file),
                "no global version configured",
            )
        }
    }
}

/// Removes `file`. A missing file is not an error (allowlist D-11). Any other
/// failure is reported instead of being swallowed (M-1): stderr and exit 1 on
/// Linux, matching `write_checked`'s reason formatting; stdout and exit 1 on
/// Windows, matching `win_write`'s "cannot write" message (D-23).
fn remove_checked(ctx: &Ctx, file: &Path) -> Output {
    match std::fs::remove_file(file) {
        Ok(()) => Output::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Output::new(),
        Err(e) => match ctx.flavor {
            Flavor::Pyenv => {
                Output::error(format!("pyenv: cannot remove `{}': {e}", file.display()))
            }
            Flavor::PyenvWin => {
                let mut o = Output::new();
                o.out(format!("pyenv: cannot remove '{}': {e}", file.display()));
                o.with_code(1)
            }
        },
    }
}

fn win_show(lines: &[String], none: &str) -> Output {
    let mut o = Output::new();
    if lines.is_empty() {
        o.out(none);
    }
    for l in lines {
        o.out(l);
    }
    o
}

/// pyenv-win: every name must resolve to an installed version; the raw names are written.
fn win_write(ctx: &Ctx, file: &Path, versions: &[&str]) -> Output {
    for v in versions {
        if let Err(resolved) = prefix::win_bin_dir(ctx, v) {
            let mut o = Output::new();
            o.out("pyenv specific python requisite didn't meet. Project is using different version of python.");
            o.out(format!(
                "Install python '{resolved}' by typing: 'pyenv install {resolved}'"
            ));
            return o.with_code(1);
        }
    }
    match verfile::write_versions(file, versions, Flavor::PyenvWin) {
        Ok(()) => Output::new(),
        Err(e) => {
            let mut o = Output::new();
            o.out(format!("pyenv: cannot write '{}': {e}", file.display()));
            o.with_code(1)
        }
    }
}
