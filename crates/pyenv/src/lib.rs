//! The `pyenv` command-line interface.

mod commands;
mod help;
pub mod install;
mod output;
mod plugin;

pub use output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use std::ffi::OsString;
use std::path::PathBuf;

/// rpyenv's own version.
pub const RPYENV_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The folder above the one holding this `pyenv` binary: upstream's
/// `_PYENV_INSTALL_PREFIX`, which its dispatcher derives the same way and overwrites any
/// inherited value with (libexec/pyenv:79-82; Decision 3).
pub(crate) fn install_prefix() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.to_path_buf())
}

/// The shim binary installed next to this `pyenv` binary. It may not exist.
pub(crate) fn shim_exe() -> Option<PathBuf> {
    let name = format!("pyenv-shim{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe().ok().map(|e| e.with_file_name(name))
}

/// `pyenv --version`: the upstream version rpyenv matches, then rpyenv's own (allowlist D-01).
pub fn version_line(flavor: Flavor) -> String {
    let upstream = match flavor {
        // The pyenv release the vendored python-build comes from (python-build/UPSTREAM),
        // so a definitions sync moves both together.
        Flavor::Pyenv => install::defs::UPSTREAM_VERSION,
        Flavor::PyenvWin => "3.1.1",
    };
    format!("pyenv {upstream} (rpyenv {RPYENV_VERSION})")
}

/// True when `name` is a built-in command of `flavor` (multicall, Decision 3).
pub fn is_builtin(flavor: Flavor, name: &str) -> bool {
    commands::lookup(flavor, name).is_some()
}

/// Runs one invocation. `args` excludes the program name. They stay `OsString`s so that
/// `exec` passes them on byte for byte (review M-5); other commands see them as text.
pub fn run(args: &[OsString], ctx: &Ctx) -> Output {
    let text: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let strs: Vec<&str> = text.iter().map(String::as_str).collect();
    match ctx.flavor {
        Flavor::Pyenv => run_pyenv(&strs, args, ctx),
        Flavor::PyenvWin => run_pyenv_win(&strs, args, ctx),
    }
}

/// Upstream `libexec/pyenv`.
fn run_pyenv(args: &[&str], raw: &[OsString], ctx: &Ctx) -> Output {
    // `--debug` is accepted and ignored (allowlist D-19).
    let skip = usize::from(args.first() == Some(&"--debug"));
    let (args, raw) = (&args[skip..], &raw[skip..]);
    let Some((&cmd, rest)) = args.split_first() else {
        let mut o = Output::new();
        o.err(version_line(Flavor::Pyenv));
        o.stderr.push_str(&help::listing(Flavor::Pyenv, Some(ctx)));
        return o.with_code(1);
    };
    // Each upstream command answers `--complete` as its first argument (Decision 5).
    if rest.first() == Some(&"--complete") {
        if let Some(o) = commands::completions::answer(ctx, cmd) {
            return o;
        }
    }
    match cmd {
        "-v" | "--version" => return commands::misc::version_cmd(ctx, rest),
        "-h" | "--help" => return help::help_ctx(ctx, &[]),
        // Only when no `pyenv-shell` is on the dispatch PATH (libexec/pyenv:127-128).
        "shell" if plugin::find(ctx, "shell").is_none() => {
            return Output::error(
                "pyenv: shell integration not enabled. Run `pyenv init' for instructions.",
            )
        }
        "exec" if rest.first() != Some(&"--help") => return commands::exec::exec(ctx, &raw[1..]),
        _ => {}
    }
    match commands::lookup(Flavor::Pyenv, cmd) {
        // `pyenv sh-<cmd> --help` prints the help command for the shell function to
        // evaluate (libexec/pyenv:133-136).
        Some(_) if rest.first() == Some(&"--help") && cmd.starts_with("sh-") => {
            let mut o = Output::new();
            o.out(format!("pyenv help \"{cmd}\""));
            o
        }
        Some(_) if rest.first() == Some(&"--help") => help::help_ctx(ctx, &[cmd]),
        Some(command) => command(ctx, rest),
        None => plugin::dispatch(ctx, cmd, rest, &raw[1..])
            .unwrap_or_else(|| Output::error(format!("pyenv: no such command `{cmd}'"))),
    }
}

/// pyenv-win's `pyenv.bat`, with command names matched case-insensitively (allowlist D-10).
fn run_pyenv_win(args: &[&str], raw: &[OsString], ctx: &Ctx) -> Output {
    // Spec §9.4: an all-users install sets the user up before their first command runs.
    #[cfg(windows)]
    let first = commands::setup_win::first_run(
        ctx,
        &args
            .first()
            .map(|c| c.to_ascii_lowercase())
            .unwrap_or_default(),
    );
    #[allow(unused_mut)]
    let mut out = dispatch_win(args, raw, ctx);
    #[cfg(windows)]
    if let Some(f) = first {
        out.stderr.insert_str(0, &f.stderr);
    }
    out
}

fn dispatch_win(args: &[&str], raw: &[OsString], ctx: &Ctx) -> Output {
    let Some((&typed, rest)) = args.split_first() else {
        return help::show_help_win();
    };
    let cmd = typed.to_ascii_lowercase();
    // `pyenv --help [<cmd>]` prints help instead of "no such command" (allowlist D-09).
    if cmd == "--help" || cmd == "help" {
        return help::help_ctx(ctx, rest);
    }
    if cmd == "exec" && rest.first() != Some(&"--help") {
        return commands::exec::exec(ctx, &raw[1..]);
    }
    // As upstream's dispatcher does for the POSIX function to evaluate (libexec/pyenv:133-136).
    if cmd.starts_with("sh-")
        && rest.first() == Some(&"--help")
        && commands::lookup(Flavor::PyenvWin, &cmd).is_some()
    {
        let mut o = Output::new();
        o.out(format!("pyenv help \"{cmd}\""));
        // bash evaluates it: `\n`, which `eval "$(…)"` doesn't keep as `\r`.
        return commands::shell_win::lf(o);
    }
    match commands::lookup(Flavor::PyenvWin, &cmd) {
        Some(_) if rest.first() == Some(&"--help") => help::help_ctx(ctx, &[cmd.as_str()]),
        // The completion scripts read `commands` and `completions` with `$(…)`; in an
        // integrated bash, zsh or fish (PYENV_SHELL, set by `pyenv init`), they get `\n`
        // line ends, which `$(…)` doesn't strip inside the text (final review #4).
        Some(command)
            if matches!(cmd.as_str(), "commands" | "completions") && evaluating_shell() =>
        {
            commands::shell_win::lf(command(ctx, rest))
        }
        Some(command) => command(ctx, rest),
        // A plugin (allowlist D-93), else pyenv-win's message.
        None => plugin::dispatch(ctx, &cmd, rest, &raw[1..]).unwrap_or_else(|| {
            let mut o = Output::new();
            o.out(format!("pyenv: no such command '{typed}'"));
            o.with_code(1)
        }),
    }
}

/// True when `PYENV_SHELL` names a bash, zsh or fish on Windows.
pub(crate) fn evaluating_shell() -> bool {
    std::env::var("PYENV_SHELL")
        .ok()
        .and_then(|s| rpyenv_core::shellname::windows_name(&s))
        .is_some_and(|n| {
            matches!(
                rpyenv_core::shellname::family(&n, Flavor::PyenvWin),
                rpyenv_core::shellname::Family::Posix
                    | rpyenv_core::shellname::Family::Ksh
                    | rpyenv_core::shellname::Family::Fish
            )
        })
}
