//! Running a plugin command (spec §6, decision D4; plan M4a Decisions 2-5).

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{launch, plugins};
use std::ffi::OsString;
use std::path::PathBuf;

/// The `PATH` a plugin runs with, and the one plugins are looked up on.
pub(crate) fn dispatch_path(ctx: &Ctx) -> OsString {
    let prefix = crate::install_prefix();
    let mut front = plugins::front_dirs(&ctx.root, prefix.as_deref());
    if let Some(links) = builtin_links(ctx) {
        front.push(links);
    }
    plugins::dispatch_path(&front, ctx.path.as_deref(), ctx.flavor)
}

/// `pyenv-<name>` on the dispatch `PATH`.
pub(crate) fn find(ctx: &Ctx, name: &str) -> Option<PathBuf> {
    plugins::find(
        name,
        &dispatch_path(ctx),
        ctx.flavor,
        ctx.pathext.as_deref(),
    )
}

/// `$PYENV_ROOT/.rpyenv/libexec`, with a `pyenv-<cmd>` symlink to this binary per built-in
/// (Decision 2): made or repaired here, `None` when that fails (a read-only root).
#[cfg(unix)]
fn builtin_links(ctx: &Ctx) -> Option<PathBuf> {
    if ctx.flavor != Flavor::Pyenv {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let dir = ctx.root.join(".rpyenv").join("libexec");
    std::fs::create_dir_all(&dir).ok()?;
    for name in crate::commands::builtin_names(Flavor::Pyenv) {
        let link = dir.join(format!("pyenv-{name}"));
        if std::fs::read_link(&link).ok().as_deref() != Some(exe.as_path()) {
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&exe, &link).ok()?;
        }
    }
    Some(dir)
}

#[cfg(not(unix))]
fn builtin_links(_ctx: &Ctx) -> Option<PathBuf> {
    None
}

/// The environment upstream's dispatcher exports (Decision 4).
fn env(ctx: &Ctx, path: OsString) -> Vec<(OsString, Option<OsString>)> {
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
    let path = dispatch_path(ctx);
    let program = plugins::find(cmd, &path, ctx.flavor, ctx.pathext.as_deref())?;
    if args.first() == Some(&"--help") {
        if cmd.starts_with("sh-") {
            let mut o = Output::new();
            o.out(format!("pyenv help \"{cmd}\""));
            return Some(o);
        }
        return Some(crate::help::help_command(ctx.flavor, &[cmd]));
    }
    let plan = launch::LaunchPlan {
        program,
        args: raw.to_vec(),
        raw_tail: None,
        env: env(ctx, path),
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
