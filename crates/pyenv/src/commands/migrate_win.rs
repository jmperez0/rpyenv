//! `pyenv migrate` / `--restore` (rpyenv-only, spec §9.4): takes over a pyenv-win install,
//! links pyenv-win-venv envs in (user decision 2026-10-08), and undoes it all from a
//! manifest of what it did (plan M6a, R4, R5).

use crate::commands::pwsh_profile::{self, Added};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{pathlist, winenv};
use std::io::{self, Write};
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
    // Final review I5: a Path that can't be read stops migrate before anything changes.
    let user = match winenv::get(winenv::Scope::User, "Path") {
        Ok(v) => v,
        Err(e) => {
            return Output::error(format!(
                "pyenv: cannot read your user PATH ({e}); nothing was changed"
            ))
            .with_code(1)
        }
    };
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
    // Final review M1: a step that fails fails the command.
    let mut failed = false;
    for name in ["pyenv.ps1", "pyenv.bat", "pyenv"] {
        let from = bin.join(name);
        if !from.is_file() {
            continue;
        }
        // A launcher kept by an earlier restore is still in the backup: never replace it.
        let to = backup.join(name);
        let moved = if to.exists() {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is in the way", to.display()),
            ))
        } else {
            std::fs::rename(&from, &to)
        };
        match moved {
            Ok(()) => record(ctx, &format!("bin\t{name}")),
            Err(e) => {
                o.err(format!("pyenv: cannot move {}: {e}", from.display()));
                failed = true;
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
                Err(e) => {
                    o.err(format!("pyenv: cannot change your user PATH: {e}"));
                    failed = true;
                }
            }
        }
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    o.stderr.push_str(&r.stderr);
    failed |= r.code != 0;
    if let Some(docs) = winenv::documents() {
        for p in pwsh_profile::paths(&docs) {
            match pwsh_profile::add(&p) {
                Ok(Added::Added) => {
                    record(ctx, &format!("profile\t{}", p.display()));
                    o.out(format!(
                        "pyenv: added the PowerShell line to {}",
                        p.display()
                    ));
                }
                // Final review I2: the line `pyenv setup` added fails under pyenv-win too,
                // so `--restore` takes it out whoever added it.
                Ok(Added::Mentions) => {
                    if pwsh_profile::has_line(&p).unwrap_or(false) {
                        record(ctx, &format!("profile\t{}", p.display()));
                    }
                }
                Err(e) => {
                    o.err(format!("pyenv: {}: {e}", p.display()));
                    failed = true;
                }
            }
        }
    }
    if let Some(envs) = envs_dir {
        link_venvs(ctx, &envs, &mut o);
    }
    if failed {
        o.err("pyenv: migrate didn't finish; `pyenv migrate --restore` undoes what it did");
        o.code = 1;
        return o;
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

/// Undoes what the manifest lists. A step that fails is reported and stays in the manifest
/// for the next `--restore`; a step left alone on purpose (a link that isn't migrate's, a
/// launcher that exists again) is final (final review I3, I4).
fn restore(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    let manifest = dir(ctx).join("manifest.txt");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        return Output::error("pyenv: nothing to restore: no migration recorded").with_code(1);
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut left = vec![false; lines.len()];
    // Junctions first, newest first: `versions\<name>` before the one it points to.
    for (i, l) in lines.iter().enumerate().rev() {
        let f: Vec<&str> = l.split('\t').collect();
        if let ["junction", link, target] = f[..] {
            let link = Path::new(link);
            let ours = std::fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink())
                && std::fs::read_link(link)
                    .is_ok_and(|t| pathlist::same(&t.display().to_string(), target));
            if ours {
                if let Err(e) = std::fs::remove_dir(link) {
                    o.err(format!("pyenv: cannot remove {}: {e}", link.display()));
                    left[i] = true;
                }
            } else if link.exists() {
                o.err(format!(
                    "pyenv: left {} alone: it isn't the link migrate made",
                    link.display()
                ));
            }
        }
    }
    let bin = ctx.root.join("bin");
    let backup = dir(ctx).join("bin");
    for (i, l) in lines.iter().enumerate() {
        let f: Vec<&str> = l.split('\t').collect();
        match f[..] {
            ["bin", name] => {
                let to = bin.join(name);
                if to.exists() {
                    o.err(format!(
                        "pyenv: kept {}, which exists again; the old one stays in {}",
                        to.display(),
                        backup.display()
                    ));
                    continue;
                }
                let _ = std::fs::create_dir_all(&bin);
                if let Err(e) = std::fs::rename(backup.join(name), &to) {
                    o.err(format!("pyenv: cannot put back {}: {e}", to.display()));
                    left[i] = true;
                }
            }
            ["profile", p] => {
                if let Err(e) = pwsh_profile::remove(Path::new(p)) {
                    o.err(format!(
                        "pyenv: cannot take the PowerShell line out of {p}: {e}"
                    ));
                    left[i] = true;
                }
            }
            _ => {}
        }
    }
    let gone: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with("path\t"))
        .collect();
    if !gone.is_empty() {
        let put_back = winenv::get(winenv::Scope::User, "Path").and_then(|v| {
            let v = v.unwrap_or(winenv::Value {
                text: String::new(),
                expand: true,
            });
            let mut text = v.text.clone();
            for &i in gone.iter().rev() {
                let entry = &lines[i]["path\t".len()..];
                text = pathlist::put_first(&text, entry, &winenv::expand).unwrap_or(text);
            }
            winenv::set_user(
                "Path",
                &winenv::Value {
                    text,
                    expand: v.expand,
                },
            )
        });
        match put_back {
            Ok(()) => winenv::broadcast(),
            Err(e) => {
                o.err(format!(
                    "pyenv: cannot put pyenv-win back on your user PATH: {e}"
                ));
                for &i in &gone {
                    left[i] = true;
                }
            }
        }
    }
    if left.contains(&true) {
        let rest: String = lines
            .iter()
            .zip(&left)
            .filter(|(_, &l)| l)
            .map(|(l, _)| format!("{l}\n"))
            .collect();
        if let Err(e) = std::fs::write(&manifest, rest) {
            o.err(format!("pyenv: cannot update {}: {e}", manifest.display()));
        }
        o.err(
            "pyenv: restore didn't finish; fix the above and run `pyenv migrate --restore` again",
        );
        o.code = 1;
        return o;
    }
    // Only empty folders go: a launcher kept in the backup stays there.
    if let Err(e) = std::fs::remove_file(&manifest) {
        o.err(format!("pyenv: cannot remove {}: {e}", manifest.display()));
    }
    let _ = std::fs::remove_dir(&backup);
    let _ = std::fs::remove_dir(dir(ctx));
    o.out("pyenv: restored pyenv-win; run its `pyenv rehash` in a new terminal to bring back its shims");
    o
}
