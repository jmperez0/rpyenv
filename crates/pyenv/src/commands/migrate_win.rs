//! `pyenv migrate` / `--restore` (rpyenv-only, spec §9.4): takes over a pyenv-win install,
//! links pyenv-win-venv envs in (user decision 2026-10-08), and undoes it all from a
//! manifest of what it did (plan M6a, R4, R5).

use crate::commands::pwsh_profile::{self, Added};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{pathlist, winenv};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const HELP: &str = "Usage: pyenv migrate [--restore]

Takes over a pyenv-win install in PYENV_ROOT: moves pyenv-win's launchers out of `bin`
into a backup, takes `bin` off your user PATH, rehashes, links pyenv-win-venv envs in,
and adds the PowerShell profile line. `--restore` undoes exactly what it did.
";

fn dir(ctx: &Ctx) -> PathBuf {
    ctx.root.join(".rpyenv-migrate")
}

fn record(ctx: &Ctx, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir(ctx).join("manifest.txt"))
    {
        let _ = writeln!(f, "{line}");
    }
}

pub fn migrate(ctx: &Ctx, args: &[&str]) -> Output {
    match args {
        [] => forward(ctx),
        ["--restore"] => restore(ctx),
        ["--help"] => {
            let mut o = Output::new();
            o.stdout.push_str(HELP);
            o
        }
        _ => Output::error(HELP).with_code(1),
    }
}

fn forward(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    if dir(ctx).join("manifest.txt").exists() {
        o.out("pyenv: already migrated; `pyenv migrate --restore` undoes it");
        return o;
    }
    let bin = ctx.root.join("bin");
    let user = winenv::get(winenv::Scope::User, "Path");
    let on_path = user.as_ref().is_some_and(|v| {
        pathlist::split(&v.text)
            .iter()
            .any(|e| pathlist::same(&winenv::expand(e), &bin.display().to_string()))
    });
    let envs_dir = std::env::var_os("USERPROFILE")
        .map(|h| PathBuf::from(h).join(".pyenv-win-venv").join("envs"));
    if crate::commands::setup_win::pyenv_win_bin(ctx).is_none() && !on_path {
        return Output::error(format!(
            "pyenv: no pyenv-win installation found in {}",
            ctx.root.display()
        ))
        .with_code(1);
    }
    let backup = dir(ctx).join("bin");
    if let Err(e) = std::fs::create_dir_all(&backup) {
        return Output::error(format!("pyenv: cannot create {}: {e}", backup.display()));
    }
    for name in ["pyenv.ps1", "pyenv.bat", "pyenv"] {
        let from = bin.join(name);
        if from.is_file() {
            match std::fs::rename(&from, backup.join(name)) {
                Ok(()) => record(ctx, &format!("bin\t{name}")),
                Err(e) => o.err(format!("pyenv: cannot move {}: {e}", from.display())),
            }
        }
    }
    if let Some(v) = user {
        let target = bin.display().to_string();
        let (text, gone) =
            pathlist::without(&v.text, &|e| pathlist::same(&winenv::expand(e), &target));
        if !gone.is_empty() {
            match winenv::set_user(
                "Path",
                &winenv::Value {
                    text,
                    expand: v.expand,
                },
            ) {
                Ok(()) => {
                    for g in &gone {
                        record(ctx, &format!("path\t{g}"));
                    }
                    o.out(format!("pyenv: took {target} off your user PATH"));
                    winenv::broadcast();
                }
                Err(e) => o.err(format!("pyenv: cannot change your user PATH: {e}")),
            }
        }
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    o.stderr.push_str(&r.stderr);
    if let Some(docs) = winenv::documents() {
        for p in pwsh_profile::paths(&docs) {
            if let Ok(Added::Added) = pwsh_profile::add(&p) {
                record(ctx, &format!("profile\t{}", p.display()));
                o.out(format!(
                    "pyenv: added the PowerShell line to {}",
                    p.display()
                ));
            }
        }
    }
    if let Some(envs) = envs_dir {
        link_venvs(ctx, &envs, &mut o);
    }
    o.out("pyenv: migrated; open a new terminal. `pyenv migrate --restore` undoes it");
    o
}

/// Links each pyenv-win-venv env whose base is installed here: `versions\<base>\envs\<name>`
/// and `versions\<name>` (spec §10 layout), recording both.
fn link_venvs(ctx: &Ctx, envs: &Path, o: &mut Output) {
    let Ok(entries) = std::fs::read_dir(envs) else {
        return;
    };
    let versions = ctx.versions_dir();
    for e in entries.flatten() {
        let env = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !env.is_dir() || !crate::install::is_safe_win_segment(&name) {
            continue;
        }
        let Some(home) = rpyenv_core::venv::read_cfg(&env, ctx.flavor).and_then(|c| c.home) else {
            continue;
        };
        let base_ok = home.parent().is_some_and(|p| {
            pathlist::same(&p.display().to_string(), &versions.display().to_string())
        }) && home.is_dir();
        if !base_ok {
            o.err(format!(
                "pyenv: {} not linked: its Python ({}) isn't installed here",
                env.display(),
                home.display()
            ));
            continue;
        }
        let in_base = home.join("envs").join(&name);
        let top = versions.join(&name);
        if top.exists() || in_base.exists() {
            o.err(format!(
                "pyenv: {} not linked: {} is taken",
                env.display(),
                name
            ));
            continue;
        }
        let _ = std::fs::create_dir_all(home.join("envs"));
        if rpyenv_core::junction::create(&in_base, &env).is_ok() {
            record(
                ctx,
                &format!("junction\t{}\t{}", in_base.display(), env.display()),
            );
            if rpyenv_core::junction::create(&top, &in_base).is_ok() {
                record(
                    ctx,
                    &format!("junction\t{}\t{}", top.display(), in_base.display()),
                );
                o.out(format!("pyenv: linked the pyenv-win-venv env {name}"));
            }
        }
    }
}

fn restore(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    let Ok(text) = std::fs::read_to_string(dir(ctx).join("manifest.txt")) else {
        return Output::error("pyenv: nothing to restore: no migration recorded").with_code(1);
    };
    let lines: Vec<&str> = text.lines().collect();
    // Junctions first, newest first: `versions\<name>` before the one it points to.
    for l in lines.iter().rev() {
        let f: Vec<&str> = l.split('\t').collect();
        if let ["junction", link, target] = f[..] {
            let link = Path::new(link);
            let ours = std::fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink())
                && std::fs::read_link(link)
                    .is_ok_and(|t| pathlist::same(&t.display().to_string(), target));
            if ours {
                let _ = std::fs::remove_dir(link);
            } else if link.exists() {
                o.err(format!(
                    "pyenv: left {} alone: it isn't the link migrate made",
                    link.display()
                ));
            }
        }
    }
    let bin = ctx.root.join("bin");
    for l in &lines {
        let f: Vec<&str> = l.split('\t').collect();
        match f[..] {
            ["bin", name] => {
                let to = bin.join(name);
                if to.exists() {
                    o.err(format!(
                        "pyenv: kept {}, which exists again; the old one stays in {}",
                        to.display(),
                        dir(ctx).join("bin").display()
                    ));
                    continue;
                }
                let _ = std::fs::create_dir_all(&bin);
                if let Err(e) = std::fs::rename(dir(ctx).join("bin").join(name), &to) {
                    o.err(format!("pyenv: cannot put back {}: {e}", to.display()));
                }
            }
            ["profile", p] => {
                let _ = pwsh_profile::remove(Path::new(p));
            }
            _ => {}
        }
    }
    let gone: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("path\t"))
        .collect();
    if !gone.is_empty() {
        let v = winenv::get(winenv::Scope::User, "Path").unwrap_or(winenv::Value {
            text: String::new(),
            expand: true,
        });
        let mut text = v.text.clone();
        for g in gone.iter().rev() {
            text = pathlist::put_first(&text, g, &winenv::expand).unwrap_or(text);
        }
        if winenv::set_user(
            "Path",
            &winenv::Value {
                text,
                expand: v.expand,
            },
        )
        .is_ok()
        {
            winenv::broadcast();
        }
    }
    if o.stderr.is_empty() {
        let _ = std::fs::remove_dir_all(dir(ctx));
    }
    o.out("pyenv: restored pyenv-win; run its `pyenv rehash` in a new terminal to bring back its shims");
    o
}
