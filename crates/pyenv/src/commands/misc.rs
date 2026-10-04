//! `--version`, `root`, `commands`, `help`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;

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
