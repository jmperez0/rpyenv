//! `pyenv exec`.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::launch::{self, ExecEnv, Mode};
use rpyenv_core::select;
use std::ffi::OsString;

const USAGE: &str = "Usage: pyenv exec <command> [arg1 arg2...]";

/// `pyenv exec <command> [args...]`. On Linux this process becomes the command, except
/// for pip commands; otherwise the command's exit code comes back in the `Output`.
pub fn exec(ctx: &Ctx, args: &[OsString]) -> Output {
    let command = args
        .first()
        .map(|a| a.to_string_lossy().into_owned())
        .filter(|c| !c.is_empty());
    let Some(command) = command else {
        return usage(ctx);
    };
    let env = ExecEnv::from_process(&command, crate::shim_exe());
    match launch::plan(ctx, Mode::Exec, &command, args[1..].to_vec(), &env) {
        Err(report) => report.into(),
        Ok(plan) => {
            #[cfg(windows)]
            let plan = launch::LaunchPlan {
                raw_tail: rpyenv_core::wincmd::own_tail(3),
                ..plan
            };
            let mut before = Output::new();
            for w in &plan.warnings {
                before.err(w);
            }
            before.emit(ctx.flavor);
            let rehash_with = crate::shim_exe().filter(|e| e.is_file());
            Output::new().with_code(launch::run(&plan, ctx, rehash_with.as_deref()))
        }
    }
}

/// No command. Upstream resolves the version first, so its warnings come before the
/// usage. pyenv-win checks the version first too, then fails on the empty command line;
/// rpyenv prints the usage instead (allowlist D-41).
fn usage(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    match ctx.flavor {
        Flavor::Pyenv => {
            for w in select::version_name(ctx, true).stderr {
                o.err(w);
            }
            o.err(USAGE);
        }
        Flavor::PyenvWin => {
            let names: Vec<String> = select::win_select(ctx)
                .into_iter()
                .map(|s| s.name)
                .collect();
            if let Err(report) = launch::win_exec_version_check(ctx, &names) {
                return report.into();
            }
            o.out(USAGE);
        }
    }
    o.with_code(1)
}

/// The command-table entry, so that `commands` and `help` know `exec`. The dispatcher
/// calls [`exec`] directly with the raw arguments.
pub fn exec_listed(ctx: &Ctx, args: &[&str]) -> Output {
    let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
    exec(ctx, &raw)
}
