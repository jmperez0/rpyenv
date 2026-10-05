//! `pyenv versions`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{installed, prefix, select, shimset};
use std::path::Path;

pub fn versions(ctx: &Ctx, args: &[&str]) -> Output {
    match ctx.flavor {
        Flavor::Pyenv => versions_pyenv(ctx, args),
        Flavor::PyenvWin => versions_win(ctx, args),
    }
}

fn versions_pyenv(ctx: &Ctx, args: &[&str]) -> Output {
    let (mut bare, mut skip_aliases, mut skip_envs) = (false, false, false);
    for a in args {
        match *a {
            "--bare" => bare = true,
            "--skip-aliases" => skip_aliases = true,
            "--skip-envs" => skip_envs = true,
            // Rehash mode: argument parsing stops here (libexec/pyenv-versions).
            "--executables" => {
                let mut o = Output::new();
                for name in shimset::executables_pyenv(&ctx.versions_dir()) {
                    o.out(name.to_string_lossy());
                }
                return o;
            }
            _ => {
                return Output::error(
                    "Usage: pyenv versions [--bare] [--skip-aliases] [--skip-envs] [--executables]",
                )
            }
        }
    }
    let mut o = Output::new();
    let (current, origin) = if bare {
        (Vec::new(), String::new())
    } else {
        let r = select::version_name(ctx, false);
        for line in &r.stderr {
            o.err(line);
        }
        (r.names, select::version_origin(ctx))
    };
    let line = |name: &str, link: Option<&Path>| -> String {
        if bare {
            return name.to_string();
        }
        let text = match link {
            Some(l) => format!("{name} --> {}", l.display()),
            None => name.to_string(),
        };
        if current.iter().any(|c| c == name) {
            format!("* {text} (set by {origin})")
        } else {
            format!("  {text}")
        }
    };
    let mut printed = false;
    if !bare && prefix::system_python(ctx).is_some() {
        o.out(line("system", None));
        printed = true;
    }
    for entry in installed::top_level(&ctx.versions_dir(), Flavor::Pyenv) {
        if skip_aliases && entry.alias {
            continue;
        }
        o.out(line(&entry.name, entry.link.as_deref()));
        printed = true;
        if !skip_envs {
            for env in installed::envs_of(&entry) {
                o.out(line(&env.name, env.link.as_deref()));
            }
        }
    }
    if !bare && !printed {
        o.err("Warning: no Python detected on the system");
        return o.with_code(1);
    }
    o
}

fn versions_win(ctx: &Ctx, args: &[&str]) -> Output {
    let bare = args.first() == Some(&"--bare");
    let selected = if bare {
        Vec::new()
    } else {
        select::win_select(ctx)
    };
    let mut o = Output::new();
    for entry in installed::top_level(&ctx.versions_dir(), Flavor::PyenvWin) {
        // A version, then its envs as `<base>\envs\<name>`; a junction as
        // `<name> --> <target>` (allowlist D-101).
        let mut lines = vec![(entry.name.clone(), entry.link.clone())];
        lines.extend(
            installed::envs_of(&entry)
                .into_iter()
                .map(|e| (e.name.replace('/', "\\"), None)),
        );
        for (name, link) in lines {
            let text = match &link {
                Some(l) => format!("{name} --> {}", l.display()),
                None => name.clone(),
            };
            match selected.iter().find(|s| s.name.replace('/', "\\") == name) {
                _ if bare => o.out(&name),
                Some(s) => o.out(format!("* {text} (set by {})", s.origin)),
                None => o.out(format!("  {text}")),
            }
        }
    }
    o
}
