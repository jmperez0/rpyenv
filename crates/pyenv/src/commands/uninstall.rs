//! `pyenv uninstall [-f|--force] <version> ...` (reference "pyenv uninstall"). Options are
//! positional; nothing is resolved by prefix; each argument loses its directories.

use crate::install::{interrupted, prompt, watch_interrupt, Reply};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::installed::is_staging_name;

pub const HELP: &str = "Usage: pyenv uninstall [-f|--force] <version> ...\n\n   -f  Attempt to remove the specified version without prompting\n       for confirmation. If the version does not exist, do not\n       display an error message.\n\nSee `pyenv versions` for a complete list of installed versions.\n\n";
pub const USAGE: &str = "Usage: pyenv uninstall [-f|--force] <version> ...";

fn usage() -> Output {
    Output {
        stderr: HELP.into(),
        code: 1,
        ..Output::new()
    }
}

/// The version name for an argument, as upstream's `${arg##*/}`: the text after the last
/// `/`. Pure, never touches the filesystem and never falls back to the raw argument.
/// `None` (treated as not installed) for an empty name (`u1/`, `/`), `.`, `..`, a NUL byte,
/// and rpyenv's own staging directories, which may belong to a running install.
fn version_name(arg: &str) -> Option<&str> {
    let name = arg.rsplit('/').next().unwrap_or("");
    let bad = name.is_empty() || name == "." || name == ".." || name.contains('\0');
    (!bad && !is_staging_name(name)).then_some(name)
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
        let name = version_name(v);
        let versions_dir = ctx.versions_dir();
        // Second guard: the target must be a direct child of `versions/`.
        let prefix = name
            .map(|n| versions_dir.join(n))
            .filter(|p| p.parent() == Some(versions_dir.as_path()));
        let present = prefix.as_ref().is_some_and(|p| p.is_dir());
        let shown = name.unwrap_or(v);
        let prefix = prefix.unwrap_or_default();
        let name = name.unwrap_or_default();
        if !force {
            if !present {
                out.err(format!("pyenv: version `{shown}' not installed"));
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
            // An install of this name holds the same lock; do not pull its version away.
            let lock_path = ctx.root.join(".locks").join(format!("install-{name}"));
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path);
            if let Ok(l) = &lock {
                if matches!(l.try_lock(), Err(std::fs::TryLockError::WouldBlock)) {
                    out.err(format!(
                        "pyenv: an install of {name} is in progress ({})",
                        lock_path.display()
                    ));
                    return out.with_code(1);
                }
            }
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
            drop(lock);
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

#[cfg(test)]
mod tests {
    use super::version_name;

    // allowlist D-68
    #[test]
    fn names_come_from_the_text_after_the_last_slash_and_never_from_the_argument() {
        for refused in [
            "/",
            "/tmp/..",
            "u2/..",
            "../../..",
            "u1/",
            "u1/.",
            ".",
            "..",
            "",
            ".tmp-3.12.0",
            "a/.old-3.12.0",
            "u\0x",
        ] {
            assert_eq!(version_name(refused), None, "{refused:?}");
        }
        assert_eq!(version_name("../x"), Some("x"));
        assert_eq!(version_name("/some/where/u4"), Some("u4"));
        assert_eq!(version_name("3.11.0"), Some("3.11.0"));
    }
}
