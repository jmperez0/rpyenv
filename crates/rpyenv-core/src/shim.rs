//! The shim program: a shim named `python` resolves and runs the selected version's
//! `python`, as `pyenv exec python` would (spec §5).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::launch::{self, ExecEnv, Mode};
use crate::rehash;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The shim binary's own name, which is never a command.
pub const SHIM_NAME: &str = "pyenv-shim";

/// Runs the shim and returns the exit code, unless the command replaced this process.
pub fn main() -> i32 {
    let flavor = Flavor::current();
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_default();
    let args: Vec<OsString> = argv.collect();
    let own = std::env::current_exe().ok();
    let Some(program) = command_name(flavor, &argv0, own.as_deref()) else {
        eprint!(
            "pyenv-shim: run this through a shim (such as `python`), not directly{}",
            flavor.eol()
        );
        return 1;
    };
    let ctx = match Ctx::from_process() {
        Ok(ctx) => ctx,
        Err(e) => {
            eprint!("{}{}", e.message(), flavor.eol());
            return 1;
        }
    };
    // Linux shims link to this binary; Windows shims are hardlinks to the template, so
    // the exit check passes the template itself and nothing is copied.
    let rehash_with = match flavor {
        Flavor::Pyenv => own.clone(),
        Flavor::PyenvWin => Some(
            ctx.shims_dir()
                .join(rehash::TEMPLATE_DIR)
                .join(rehash::TEMPLATE_EXE),
        ),
    };
    let env = ExecEnv::from_process(&program, own);
    match launch::plan(&ctx, Mode::Shim, &program, args, &env) {
        Err(report) => {
            report.emit(flavor);
            report.code
        }
        Ok(plan) => {
            for w in &plan.warnings {
                eprint!("{w}{}", flavor.eol());
            }
            launch::run(&plan, &ctx, rehash_with.as_deref())
        }
    }
}

/// The command a shim stands for: `argv[0]`'s last component on Linux, where every shim
/// is a symlink to one binary; the file's own name without `.exe` on Windows, where each
/// shim is a hardlink named after its command. None for the shim binary's own name.
pub fn command_name(flavor: Flavor, argv0: &OsStr, own: Option<&Path>) -> Option<String> {
    let name = match flavor {
        Flavor::Pyenv => Path::new(argv0).file_name()?.to_string_lossy().into_owned(),
        Flavor::PyenvWin => own?.file_stem()?.to_string_lossy().into_owned(),
    };
    (!name.is_empty() && !name.eq_ignore_ascii_case(SHIM_NAME)).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names() {
        let linux = |argv0: &str| command_name(Flavor::Pyenv, OsStr::new(argv0), None);
        assert_eq!(linux("/r/shims/python3.12"), Some("python3.12".to_string()));
        assert_eq!(linux("pip"), Some("pip".to_string()));
        assert_eq!(linux("/usr/lib/pyenv-shim"), None);
        let win = |own: &str| command_name(Flavor::PyenvWin, OsStr::new("x"), Some(Path::new(own)));
        assert_eq!(win("python.exe"), Some("python".to_string()));
        assert_eq!(win("PYENV-SHIM.EXE"), None);
        assert_eq!(command_name(Flavor::PyenvWin, OsStr::new("x"), None), None);
    }
}
