//! `version`, `version-name`, `version-origin`, `version-file`, `version-file-read`, `version-file-write`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::paths::lexical_normalize;
use rpyenv_core::{pathsearch, prefix, select, verfile};
use std::path::Path;

pub use rpyenv_core::select::WIN_NO_VERSION;

pub fn win_no_version() -> Output {
    let mut o = Output::new();
    for line in WIN_NO_VERSION {
        o.out(line);
    }
    o.with_code(1)
}

pub fn version(ctx: &Ctx, args: &[&str]) -> Output {
    match ctx.flavor {
        Flavor::Pyenv => {
            // Upstream resolves first, so its messages come before any usage error.
            let r = select::version_name(ctx, false);
            let mut o = Output::new();
            for line in &r.stderr {
                o.err(line);
            }
            let mut bare = false;
            for a in args {
                if *a == "--bare" {
                    bare = true;
                } else {
                    o.err("Usage: pyenv version [--bare]");
                    return o.with_code(1);
                }
            }
            let origin = select::version_origin(ctx);
            for name in &r.names {
                if bare {
                    o.out(name);
                } else {
                    o.out(format!("{name} (set by {origin})"));
                }
            }
            o.with_code(i32::from(r.failed))
        }
        Flavor::PyenvWin => {
            let mut o = Output::new();
            win_path_check(ctx, &mut o);
            let selected = select::win_select(ctx);
            if selected.is_empty() {
                for line in WIN_NO_VERSION {
                    o.out(line);
                }
                return o.with_code(1);
            }
            for s in &selected {
                o.out(format!("{} (set by {})", s.name, s.origin));
            }
            o
        }
    }
}

/// pyenv-win's `:check_path`: warn when the `python` shim is not found on PATH at all.
/// rpyenv's shim is `python.exe`, not `python.bat` (allowlist D-15).
fn win_path_check(ctx: &Ctx, o: &mut Output) {
    let shim = ctx.shims_dir().join("python.exe");
    if !shim.is_file() {
        return;
    }
    let found = pathsearch::find_all(
        "python",
        ctx.path.as_deref(),
        None,
        Flavor::PyenvWin,
        ctx.pathext.as_deref(),
    );
    if found.iter().any(|p| path_key(p) == path_key(&shim)) {
        return;
    }
    let first = found
        .first()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let dir = found
        .first()
        .and_then(|p| p.parent())
        .map(|d| format!("{}\\", d.display()))
        .unwrap_or_default();
    o.out(format!(
        "\x1b[91mFATAL: Found \x1b[95m{first}\x1b[91m version before pyenv in PATH.\x1b[0m"
    ));
    o.out(format!(
        "\x1b[91mPlease remove \x1b[95m{dir}\x1b[91m from PATH for pyenv to work properly.\x1b[0m"
    ));
}

/// A comparison key for a Windows path: lexically normalized (so `..` is resolved),
/// `/` unified to `\`, a trailing `\` trimmed, and ASCII-lowercased.
fn path_key(p: &Path) -> String {
    let normalized = lexical_normalize(p).to_string_lossy().replace('/', "\\");
    normalized.trim_end_matches('\\').to_ascii_lowercase()
}

pub fn version_name(ctx: &Ctx, args: &[&str]) -> Output {
    match ctx.flavor {
        Flavor::Pyenv => {
            let force = args.iter().any(|a| matches!(*a, "-f" | "--force"));
            let r = select::version_name(ctx, force);
            let mut o = Output::new();
            for line in &r.stderr {
                o.err(line);
            }
            o.out(r.names.join(":"));
            o.with_code(i32::from(r.failed))
        }
        Flavor::PyenvWin => {
            let selected = select::win_select(ctx);
            if selected.is_empty() {
                return win_no_version();
            }
            let mut o = Output::new();
            for s in &selected {
                o.out(&s.name);
            }
            o
        }
    }
}

pub fn version_origin(ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    o.out(match ctx.flavor {
        Flavor::Pyenv => select::version_origin(ctx),
        Flavor::PyenvWin => select::win_origin(ctx),
    });
    o
}

/// Upstream `pyenv version-file [<dir>]`, on Windows too (allowlist D-12).
pub fn version_file(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    match args.first() {
        Some(dir) => {
            let dir = ctx.pwd.join(dir);
            // A missing directory exits 1 without bash's `cd` message (allowlist D-18).
            let found = if dir.is_dir() {
                verfile::find_local(&lexical_normalize(&dir))
            } else {
                None
            };
            match found {
                Some(f) => o.out(f.display().to_string()),
                None => return o.with_code(1),
            }
        }
        None => {
            let file = match ctx.flavor {
                Flavor::Pyenv => select::version_file(ctx),
                Flavor::PyenvWin => select::win_version_file(ctx),
            };
            o.out(file.display().to_string());
        }
    }
    o
}

pub fn version_file_read(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    let Some(&given) = args.first() else {
        return o.with_code(1);
    };
    let file = ctx.pwd.join(given);
    let versions = match ctx.flavor {
        Flavor::Pyenv => {
            let r = verfile::read_pyenv(&file, given, &ctx.versions_dir());
            for w in &r.warnings {
                o.err(w);
            }
            r.versions
        }
        Flavor::PyenvWin => verfile::read_pyenv_win(&file),
    };
    if versions.is_empty() {
        return o.with_code(1);
    }
    o.out(versions.join(":"));
    o
}

pub fn version_file_write(ctx: &Ctx, args: &[&str]) -> Output {
    let forced = args
        .iter()
        .take_while(|a| matches!(**a, "-f" | "--force"))
        .count();
    let rest = &args[forced..];
    match rest {
        [file, first, ..] if !file.is_empty() && !first.is_empty() => {
            write_checked(ctx, &ctx.pwd.join(file), &rest[1..], forced > 0)
        }
        _ => Output::error("Usage: pyenv version-file-write [-f|--force] <file> <version> [...]"),
    }
}

/// Validates each version with `pyenv-prefix` (unless forced), then writes them.
/// Used by `local` and `global` on Linux and by `version-file-write` on both OSes.
pub fn write_checked(ctx: &Ctx, file: &Path, versions: &[&str], force: bool) -> Output {
    if !force {
        for v in versions {
            if let Err(e) = prefix::prefix_of(ctx, v) {
                return Output::error(e.message());
            }
        }
    }
    match verfile::write_versions(file, versions, ctx.flavor) {
        Ok(()) => Output::new(),
        Err(e) => Output::error(format!("pyenv: cannot write `{}': {e}", file.display())),
    }
}
