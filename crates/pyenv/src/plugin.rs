//! Running a plugin command (spec §6, decision D4; plan M4a Decisions 2-5).

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{launch, plugins};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The `PATH` plugins are looked up and listed on. It doesn't make the built-in links, so
/// commands that only list or look up write nothing into the root.
pub(crate) fn search_path(ctx: &Ctx) -> OsString {
    plugins::dispatch_path(&front(ctx), ctx.path.as_deref(), ctx.flavor)
}

/// The `PATH` a plugin runs with: the search path's folders, then the built-in links,
/// made or repaired here.
pub(crate) fn run_path(ctx: &Ctx) -> OsString {
    let mut front = front(ctx);
    if let Some(links) = builtin_links(ctx) {
        front.push(links);
    }
    plugins::dispatch_path(&front, ctx.path.as_deref(), ctx.flavor)
}

fn front(ctx: &Ctx) -> Vec<PathBuf> {
    let prefix = crate::install_prefix();
    let mut front = plugins::front_dirs(&ctx.root, prefix.as_deref());
    // `<prefix>\libexec` is upstream pyenv's slot; in a pyenv-win root it holds pyenv-win's
    // own scripts, which aren't plugins (Decision 5).
    if ctx.flavor == Flavor::PyenvWin {
        if let Some(libexec) = prefix.as_ref().map(|p| p.join("libexec")) {
            front.retain(|d| *d != libexec);
        }
    }
    front
}

/// `pyenv-<name>` on the search `PATH`.
pub(crate) fn find(ctx: &Ctx, name: &str) -> Option<PathBuf> {
    find_on(ctx, name, &search_path(ctx))
}

fn find_on(ctx: &Ctx, name: &str, path: &OsStr) -> Option<PathBuf> {
    plugins::find(name, path, ctx.flavor, ctx.pathext.as_deref()).filter(|p| !is_own(p))
}

/// The plugin names on `path` (`plugins::listed_names`), without rpyenv's own binaries.
pub(crate) fn listed(ctx: &Ctx, path: &OsStr) -> Vec<String> {
    plugins::listed_names(path, ctx.flavor, ctx.pathext.as_deref())
        .into_iter()
        .filter(|n| {
            !plugins::find(n, path, ctx.flavor, ctx.pathext.as_deref()).is_some_and(|p| is_own(&p))
        })
        .collect()
}

/// True for the `pyenv-shim` and `pyenv-shimw` installed next to this binary, which sit on
/// `PATH` in an install but aren't plugins (upstream's `bin` has no `pyenv-*` files).
fn is_own(file: &Path) -> bool {
    let Some(shim) = crate::shim_exe() else {
        return false;
    };
    let shimw = shim.with_file_name(format!("pyenv-shimw{}", std::env::consts::EXE_SUFFIX));
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    let f = canon(file);
    f.is_some() && (f == canon(&shim) || f == canon(&shimw))
}

/// `$PYENV_ROOT/.rpyenv/libexec`, with a `pyenv-<cmd>` symlink to this binary per built-in
/// (Decision 2): made or repaired here, `None` when the folder can't be made (a read-only
/// root). Each link is made under a unique name and renamed over the old one, so a run in
/// parallel never finds one missing, and a link that can't be made leaves the rest.
#[cfg(unix)]
fn builtin_links(ctx: &Ctx) -> Option<PathBuf> {
    if ctx.flavor != Flavor::Pyenv {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = ctx.root.join(".rpyenv").join("libexec");
    std::fs::create_dir_all(&dir).ok()?;
    let names = crate::commands::builtin_names(Flavor::Pyenv);
    // Links for built-ins this binary doesn't have (an older rpyenv's): only links go.
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let file = e.file_name();
        let stale = file
            .to_str()
            .and_then(|n| n.strip_prefix("pyenv-"))
            .is_some_and(|c| !names.contains(&c));
        if stale && e.file_type().is_ok_and(|t| t.is_symlink()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    for name in names {
        let link = dir.join(format!("pyenv-{name}"));
        if std::fs::read_link(&link).ok().as_deref() != Some(exe.as_path()) {
            // A folder in the link's place is moved aside, never deleted.
            if std::fs::symlink_metadata(&link).is_ok_and(|m| m.is_dir()) {
                let aside = dir.join(format!(".pyenv-{name}.{}.aside", std::process::id()));
                let _ = std::fs::rename(&link, aside);
            }
            let tmp = dir.join(format!(".pyenv-{name}.{}.tmp", std::process::id()));
            let _ = std::fs::remove_file(&tmp);
            if std::os::unix::fs::symlink(&exe, &tmp).is_err()
                || std::fs::rename(&tmp, &link).is_err()
            {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    Some(dir)
}

#[cfg(not(unix))]
fn builtin_links(_ctx: &Ctx) -> Option<PathBuf> {
    None
}

/// The environment upstream's dispatcher exports (Decision 4).
pub(crate) fn env(ctx: &Ctx, path: OsString) -> Vec<(OsString, Option<OsString>)> {
    let prefix = crate::install_prefix();
    let mut v = vec![
        ("PYENV_ROOT".into(), Some(ctx.root.clone().into_os_string())),
        ("PYENV_DIR".into(), Some(ctx.dir.clone().into_os_string())),
        ("PATH".into(), Some(path)),
    ];
    if let Some(p) = &prefix {
        v.push((
            "_PYENV_INSTALL_PREFIX".into(),
            Some(p.clone().into_os_string()),
        ));
    }
    if ctx.flavor == Flavor::Pyenv {
        let inherited = std::env::var("PYENV_HOOK_PATH")
            .ok()
            .filter(|s| !s.is_empty());
        let hooks = plugins::hook_path(inherited.as_deref(), &ctx.root, prefix.as_deref());
        v.push(("PYENV_HOOK_PATH".into(), Some(hooks.into())));
    }
    v
}

/// `pyenv <cmd> <args>` for a command rpyenv doesn't have. `None` when no plugin has that
/// name. `sh-*` plugins answer `--help` with the help command, as the dispatcher does.
pub(crate) fn dispatch(ctx: &Ctx, cmd: &str, args: &[&str], raw: &[OsString]) -> Option<Output> {
    let program = find_on(ctx, cmd, &search_path(ctx))?;
    if args.first() == Some(&"--help") {
        if cmd.starts_with("sh-") {
            let mut o = Output::new();
            o.out(format!("pyenv help \"{cmd}\""));
            return Some(o);
        }
        return Some(crate::help::help_ctx(ctx, &[cmd]));
    }
    let plan = launch::LaunchPlan {
        program,
        args: raw.to_vec(),
        raw_tail: None,
        env: env(ctx, run_path(ctx)),
        warnings: Vec::new(),
        wait: false,
    };
    // Windows: the caller's command line after `pyenv <cmd>`, unchanged (spec §5.3).
    #[cfg(windows)]
    let plan = launch::LaunchPlan {
        raw_tail: rpyenv_core::wincmd::own_tail(2),
        ..plan
    };
    Some(match launch::run(&plan, ctx, None) {
        Ok(code) => Output::new().with_code(code),
        Err(r) => r.into(),
    })
}
