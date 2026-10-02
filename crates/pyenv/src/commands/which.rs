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
        (Flavor::PyenvWin, Some(c)) => which_win_output(ctx, c, lookup::which_win(ctx, c)),
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

/// `pyenv which`'s Windows output: the normal line (redirected output is in the console's
/// code page, with a UTF-8 fallback), unless `RPYENV_FORWARD_CP` asks for the strict form
/// instead: the path in the console's code page or a failure (a `.cmd` forwarder's `for /f`, spec
/// §5.3) — `found_or_report_cp` on Windows, `found_or_report` everywhere else (the
/// environment variable never applies off Windows).
#[cfg(windows)]
fn which_win_output(ctx: &Ctx, command: &str, result: Result<Found, NotFound>) -> Output {
    if std::env::var_os("RPYENV_FORWARD_CP").is_some() {
        return found_or_report_cp(ctx, command, result);
    }
    found_or_report(ctx, command, result, true)
}

#[cfg(not(windows))]
fn which_win_output(ctx: &Ctx, command: &str, result: Result<Found, NotFound>) -> Output {
    found_or_report(ctx, command, result, true)
}

/// Like `found_or_report`, but a resolved path is written encoded in the console's output
/// code page, strictly: no UTF-8 fallback, since a `.cmd` forwarder's `for /f` decodes the
/// piped stdout that way and would garble it. A path that code page can't represent fails the
/// same way an unresolved command does: a `pyenv:` message on stderr and exit 127.
#[cfg(windows)]
fn found_or_report_cp(ctx: &Ctx, command: &str, result: Result<Found, NotFound>) -> Output {
    match result {
        Ok(found) => {
            let mut o = Output::new();
            for w in &found.warnings {
                o.err(w);
            }
            let path = found.path.display().to_string();
            match rpyenv_core::wincp::encode_for_console(&path) {
                Ok(bytes) => {
                    let mut raw = bytes;
                    raw.extend_from_slice(b"\r\n");
                    o.raw_stdout = Some(raw);
                }
                Err(cp) => {
                    o.err(format!(
                        "pyenv: {command}: the path can't be written in code page {cp}"
                    ));
                    o.code = 127;
                }
            }
            o
        }
        Err(nf) => lookup::not_found_report(ctx, command, &nf, true).into(),
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
