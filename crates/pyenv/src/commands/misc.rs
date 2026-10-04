//! `--version`, `root`, `commands`, `help`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use std::path::{Component, Path, PathBuf};

pub fn version_cmd(ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    o.out(crate::version_line(ctx.flavor));
    o
}

pub fn root(ctx: &Ctx, _args: &[&str]) -> Output {
    let mut o = Output::new();
    o.out(ctx.root.display().to_string());
    o
}

/// `pyenv hooks <command>` (Linux): the `<command>/*.bash` files on the hook path, each
/// resolved as upstream's `realpath` does (libexec/pyenv-hooks). rpyenv runs no hooks
/// itself (allowlist D-51); plugins such as pyenv-virtualenv source what this lists.
pub fn hooks(ctx: &Ctx, args: &[&str]) -> Output {
    let Some(cmd) = args.first().filter(|c| !c.is_empty()) else {
        return Output::error("Usage: pyenv hooks <command>");
    };
    let inherited = std::env::var("PYENV_HOOK_PATH")
        .ok()
        .filter(|s| !s.is_empty());
    let prefix = crate::install_prefix();
    let path = rpyenv_core::plugins::hook_path(inherited.as_deref(), &ctx.root, prefix.as_deref());
    let mut o = Output::new();
    // Each folder once: a plugin calling `pyenv-hooks` inherits the full path, which
    // rpyenv's single entry point extends again (upstream's libexec script doesn't).
    let mut seen = Vec::new();
    for dir in path.split(':') {
        if seen.contains(&dir) {
            continue;
        }
        seen.push(dir);
        // `"$path/$PYENV_COMMAND"/*.bash` under nullglob: no dot files, sorted.
        let Ok(rd) = std::fs::read_dir(format!("{dir}/{cmd}")) else {
            continue;
        };
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".bash") && !n.starts_with('.'))
            })
            .collect();
        files.sort();
        for f in files {
            o.out(hook_realpath(&f).display().to_string());
        }
    }
    o
}

/// pyenv-hooks' `realpath`: the folder taken logically (`cd`), the file's own link chain
/// followed.
fn hook_realpath(file: &Path) -> PathBuf {
    let mut dir = logical(file.parent().unwrap_or(Path::new("/")));
    let Some(mut name) = file.file_name().map(|n| n.to_os_string()) else {
        return file.to_path_buf();
    };
    for _ in 0..40 {
        let Ok(target) = std::fs::read_link(dir.join(&name)) else {
            break;
        };
        if let Some(p) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            dir = logical(&dir.join(p));
        }
        match target.file_name() {
            Some(n) => name = n.to_os_string(),
            None => break,
        }
    }
    dir.join(name)
}

/// `p` made absolute with `..` and `.` removed by text, as bash's `cd` does.
fn logical(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// `pyenv commands [--sh|--no-sh]`. pyenv-win's has no options.
pub fn commands(ctx: &Ctx, args: &[&str]) -> Output {
    let listing = match (ctx.flavor, args.first()) {
        (Flavor::Pyenv, Some(&"--sh")) => super::Listing::ShOnly,
        (Flavor::Pyenv, Some(&"--no-sh")) => super::Listing::NoSh,
        _ => super::Listing::All,
    };
    let mut o = Output::new();
    for name in super::command_names(ctx, listing) {
        o.out(name);
    }
    o
}

pub fn help(ctx: &Ctx, args: &[&str]) -> Output {
    crate::help::help_ctx(ctx, args)
}
