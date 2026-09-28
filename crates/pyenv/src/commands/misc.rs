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

/// `pyenv commands [--sh|--no-sh]`. rpyenv has no `sh-` commands yet, so `--sh` lists nothing.
pub fn commands(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    if ctx.flavor == Flavor::Pyenv && args.first() == Some(&"--sh") {
        return o;
    }
    for name in super::names(ctx.flavor) {
        o.out(name);
    }
    o
}

pub fn help(ctx: &Ctx, args: &[&str]) -> Output {
    crate::help::help_command(ctx.flavor, args)
}
