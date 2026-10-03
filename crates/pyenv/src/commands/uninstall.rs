//! `pyenv uninstall [-f|--force] <version> ...` (reference "pyenv uninstall"). Options are
//! positional; nothing is resolved by prefix; each argument loses its directories.

use crate::install::{interrupted, prompt, watch_interrupt, Reply};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::installed::is_staging_name;
use std::path::Path;

pub const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> ...\n\n   -f  Attempt to remove the specified version without prompting\n       for confirmation. If the version does not exist, do not\n       display an error message.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";
pub const USAGE: &str = "Usage: pyenv uninstall [-f|--force] <version> ...";

fn usage() -> Output {
    Output {
        stderr: HELP.into(),
        code: 1,
        ..Output::new()
    }
}

/// Names that are not one real version directory: rpyenv's own staging directories (they
/// may belong to a running install) and `.`/`..` (upstream's `rm -rf` would take
/// `versions/` or the root). They count as not installed.
fn not_a_version(name: &str) -> bool {
    name.is_empty() || name == "." || name == ".." || is_staging_name(name)
}

pub fn uninstall(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        let mut o = Output::new();
        o.out("--force");
        for n in rpyenv_core::installed::names(&ctx.versions_dir(), ctx.flavor) {
            o.out(n);
        }
        return o;
    }
    if matches!(args.first(), Some(&"-h" | &"--help")) {
        return Output {
            stdout: HELP.into(),
            ..Output::new()
        };
    }
    let (force, versions) = match args.first() {
        Some(&"-f" | &"--force") => (true, &args[1..]),
        _ => (false, args),
    };
    if versions.is_empty() || versions.iter().any(|v| v.is_empty() || v.starts_with('-')) {
        return usage();
    }
    watch_interrupt();
    let mut out = Output::new();
    for v in versions {
        // A Ctrl+C during the previous version's removal or rehash.
        if interrupted() {
            return out.with_code(130);
        }
        let name = Path::new(v)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| (*v).to_string());
        let prefix = ctx.versions_dir().join(&name);
        let present = !not_a_version(&name) && prefix.is_dir();
        if !force {
            if !present {
                out.err(format!("pyenv: version `{name}' not installed"));
                return out.with_code(1);
            }
            match prompt(&format!("pyenv: remove {}? (y/N) ", prefix.display())) {
                Reply::Interrupted => return out.with_code(130),
                Reply::Eof => return out.with_code(1),
                Reply::Line(r) if ["y", "Y", "yes", "YES"].contains(&r.as_str()) => {}
                Reply::Line(_) => return out.with_code(1),
            }
        }
        if present {
            let is_link = prefix
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let gone = if is_link {
                std::fs::remove_file(&prefix)
            } else {
                std::fs::remove_dir_all(&prefix)
            };
            if let Err(e) = gone {
                out.err(format!("pyenv: cannot remove {}: {e}", prefix.display()));
                return out.with_code(1);
            }
            let r = crate::commands::rehash::rehash(ctx, &[]);
            out.stderr.push_str(&r.stderr);
            if r.code != 0 {
                return out.with_code(r.code);
            }
            out.out(format!("pyenv: {name} uninstalled"));
        }
    }
    out
}
