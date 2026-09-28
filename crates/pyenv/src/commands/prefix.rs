//! `pyenv prefix`.

use crate::commands::version::win_no_version;
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{prefix, select};

pub fn prefix(ctx: &Ctx, args: &[&str]) -> Output {
    let mut o = Output::new();
    let names: Vec<String> = if !args.is_empty() {
        select::split_colon(&args.join(":"))
    } else {
        match ctx.flavor {
            Flavor::Pyenv => match &ctx.pyenv_version {
                Some(v) => select::split_colon(v),
                None => {
                    let r = select::version_name(ctx, false);
                    for line in &r.stderr {
                        o.err(line);
                    }
                    if r.failed {
                        return o.with_code(1);
                    }
                    r.names
                }
            },
            Flavor::PyenvWin => {
                let selected = select::win_select(ctx);
                if selected.is_empty() {
                    return win_no_version();
                }
                selected.into_iter().map(|s| s.name).collect()
            }
        }
    };
    let mut dirs = Vec::new();
    for name in &names {
        match prefix::prefix_of(ctx, name) {
            Ok(d) => dirs.push(d.display().to_string()),
            Err(message) => {
                o.err(message);
                return o.with_code(1);
            }
        }
    }
    // Windows paths contain `:`, so rpyenv joins them with `;` there (allowlist D-13).
    let sep = match ctx.flavor {
        Flavor::Pyenv => ":",
        Flavor::PyenvWin => ";",
    };
    o.out(dirs.join(sep));
    o
}
