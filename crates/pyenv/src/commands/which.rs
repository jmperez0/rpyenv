//! `which` and `whence`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::lookup::{self, Found, NotFound, Skip};

pub fn which(ctx: &Ctx, args: &[&str]) -> Output {
    let command = args.first().copied().filter(|c| !c.is_empty());
    match (ctx.flavor, command) {
        (Flavor::Pyenv, None) => {
            Output::error("Usage: pyenv which <command> [--nosystem] [--skip-advice]")
        }
        (Flavor::PyenvWin, None) => print_help("which"),
        // Upstream takes the command from the first argument even when it is a flag, and
        // recognizes the flags anywhere (libexec/pyenv-which:24-41).
        (Flavor::Pyenv, Some(c)) => {
            let nosystem = args.contains(&"--nosystem");
            let advice = !args.contains(&"--skip-advice");
            let skip = Skip::from_env(c, crate::shim_exe());
            found_or_report(ctx, c, lookup::which_pyenv(ctx, c, nosystem, &skip), advice)
        }
        (Flavor::PyenvWin, Some(c)) => found_or_report(ctx, c, lookup::which_win(ctx, c), true),
    }
}

fn found_or_report(
    ctx: &Ctx,
    command: &str,
    result: Result<Found, NotFound>,
    advice: bool,
) -> Output {
    match result {
        Ok(found) => {
            let mut o = Output::new();
            for w in &found.warnings {
                o.err(w);
            }
            o.out(found.path.display().to_string());
            o
        }
        Err(nf) => lookup::not_found_report(ctx, command, &nf, advice).into(),
    }
}

pub fn whence(ctx: &Ctx, args: &[&str]) -> Output {
    let (with_path, rest) = match args.split_first() {
        Some((&"--path", rest)) => (true, rest),
        _ => (false, args),
    };
    let Some(command) = rest.first().copied().filter(|c| !c.is_empty()) else {
        return match ctx.flavor {
            Flavor::Pyenv => Output::error("Usage: pyenv whence [--path] <command>"),
            Flavor::PyenvWin => print_help("whence"),
        };
    };
    let lines: Vec<String> = match ctx.flavor {
        Flavor::Pyenv => lookup::whence_pyenv(ctx, command)
            .into_iter()
            .map(|(name, path)| {
                if with_path {
                    path.display().to_string()
                } else {
                    name
                }
            })
            .collect(),
        Flavor::PyenvWin => {
            let program = command.strip_suffix('.').unwrap_or(command);
            lookup::whence_win(ctx, program, with_path)
        }
    };
    let mut o = Output::new();
    for line in &lines {
        o.out(line);
    }
    // Both upstreams exit 1 when nothing was printed.
    if lines.is_empty() {
        o.with_code(1)
    } else {
        o
    }
}

/// pyenv-win `PrintHelp`: the command's help text, one more line ending, exit 1.
pub fn print_help(name: &str) -> Output {
    let mut o = crate::help::help_command(Flavor::PyenvWin, &[name]);
    o.stdout.push('\n');
    o.with_code(1)
}
