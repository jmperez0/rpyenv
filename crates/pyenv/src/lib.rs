//! The `pyenv` command-line interface.

mod commands;
mod help;
mod output;

pub use output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

/// rpyenv's own version.
pub const RPYENV_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `pyenv --version`: the upstream version rpyenv matches, then rpyenv's own (allowlist D-01).
pub fn version_line(flavor: Flavor) -> String {
    let upstream = match flavor {
        Flavor::Pyenv => "2.8.6",
        Flavor::PyenvWin => "3.1.1",
    };
    format!("pyenv {upstream} (rpyenv {RPYENV_VERSION})")
}

/// Runs one invocation. `args` excludes the program name.
pub fn run(args: &[String], ctx: &Ctx) -> Output {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match ctx.flavor {
        Flavor::Pyenv => run_pyenv(&args, ctx),
        Flavor::PyenvWin => run_pyenv_win(&args, ctx),
    }
}

/// Upstream `libexec/pyenv`.
fn run_pyenv(args: &[&str], ctx: &Ctx) -> Output {
    // `--debug` is accepted and ignored (allowlist D-19).
    let args = match args.first() {
        Some(&"--debug") => &args[1..],
        _ => args,
    };
    let Some((&cmd, rest)) = args.split_first() else {
        let mut o = Output::new();
        o.err(version_line(Flavor::Pyenv));
        o.stderr.push_str(&help::listing(Flavor::Pyenv));
        return o.with_code(1);
    };
    match cmd {
        "-v" | "--version" => return commands::misc::version_cmd(ctx, rest),
        "-h" | "--help" => return help::help_command(Flavor::Pyenv, &[]),
        "shell" => {
            return Output::error(
                "pyenv: shell integration not enabled. Run `pyenv init' for instructions.",
            )
        }
        _ => {}
    }
    match commands::lookup(Flavor::Pyenv, cmd) {
        Some(_) if rest.first() == Some(&"--help") => help::help_command(Flavor::Pyenv, &[cmd]),
        Some(command) => command(ctx, rest),
        None => Output::error(format!("pyenv: no such command `{cmd}'")),
    }
}

/// pyenv-win's `pyenv.bat`, with command names matched case-insensitively (allowlist D-10).
fn run_pyenv_win(args: &[&str], ctx: &Ctx) -> Output {
    let Some((&typed, rest)) = args.split_first() else {
        return help::show_help_win();
    };
    let cmd = typed.to_ascii_lowercase();
    // `pyenv --help [<cmd>]` prints help instead of "no such command" (allowlist D-09).
    if cmd == "--help" || cmd == "help" {
        return help::help_command(Flavor::PyenvWin, rest);
    }
    match commands::lookup(Flavor::PyenvWin, &cmd) {
        Some(_) if rest.first() == Some(&"--help") => {
            help::help_command(Flavor::PyenvWin, &[cmd.as_str()])
        }
        Some(command) => command(ctx, rest),
        None => {
            let mut o = Output::new();
            o.out(format!("pyenv: no such command '{typed}'"));
            o.with_code(1)
        }
    }
}
