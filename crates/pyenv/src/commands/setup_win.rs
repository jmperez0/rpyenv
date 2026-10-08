//! `pyenv setup` (rpyenv-only, spec §9.4): prepares one user's environment.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{pathlist, winenv};
use std::path::PathBuf;

pub const MARKER: &str = ".rpyenv-setup";

pub const HELP: &str = "Usage: pyenv setup

Prepares your Windows user for rpyenv: creates PYENV_ROOT, puts its shims folder first
on your user PATH, rehashes, and adds the PowerShell line to your profiles. It's safe to
run again.
";

pub fn setup(ctx: &Ctx, args: &[&str]) -> Output {
    match args {
        [] => run(ctx),
        ["--help"] => {
            let mut o = Output::new();
            o.stdout.push_str(HELP);
            o
        }
        _ => Output::error(HELP).with_code(1),
    }
}

/// pyenv-win's `bin` with its launchers, in this root, if it's there.
pub fn pyenv_win_bin(ctx: &Ctx) -> Option<PathBuf> {
    let bin = ctx.root.join("bin");
    ["pyenv.ps1", "pyenv.bat", "pyenv"]
        .iter()
        .any(|f| bin.join(f).is_file())
        .then_some(bin)
}

/// The setup itself (also the all-users first run).
pub fn run(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    for d in [ctx.versions_dir(), ctx.shims_dir()] {
        if let Err(e) = std::fs::create_dir_all(&d) {
            return Output::error(format!(
                "pyenv: cannot create {}: {}",
                d.display(),
                rpyenv_core::launch::io_reason(&e)
            ));
        }
    }
    let shims = ctx.shims_dir().display().to_string();
    let current = winenv::get(winenv::Scope::User, "Path").unwrap_or(winenv::Value {
        text: String::new(),
        expand: true,
    });
    match pathlist::put_first(&current.text, &shims, &winenv::expand) {
        None => o.out(format!("pyenv: {shims} is already first on your user PATH")),
        Some(text) => match winenv::set_user(
            "Path",
            &winenv::Value {
                text,
                expand: current.expand,
            },
        ) {
            Ok(()) => {
                o.out(format!(
                    "pyenv: added {shims} to the front of your user PATH"
                ));
                winenv::broadcast();
            }
            Err(e) => o.err(format!("pyenv: cannot change your user PATH: {e}")),
        },
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    o.stderr.push_str(&r.stderr);
    let p = crate::commands::pwsh_profile::install_all();
    o.stdout.push_str(&p.stdout);
    o.stderr.push_str(&p.stderr);
    if let Some(machine) = winenv::get(winenv::Scope::Machine, "Path") {
        for e in pathlist::split(&machine.text) {
            let py = PathBuf::from(winenv::expand(&e)).join("python.exe");
            if py.is_file() {
                o.err(format!(
                    "pyenv: warning: {} is on the machine PATH, which Windows searches before yours: it runs instead of the shims",
                    py.display()
                ));
            }
        }
    }
    if let Some(bin) = pyenv_win_bin(ctx) {
        o.err(format!(
            "pyenv: pyenv-win is installed in {}; run `pyenv migrate` to let rpyenv take over",
            bin.display()
        ));
    }
    let _ = std::fs::write(ctx.root.join(MARKER), env!("CARGO_PKG_VERSION"));
    o.out("pyenv: open a new terminal for the changes to take effect");
    if !p.stderr.is_empty() {
        o.code = 1;
    }
    o
}

/// Spec §9.4: an all-users install (this pyenv.exe under Program Files) sets the user up
/// on their first command (plan M6a, R6). `None` when nothing was needed.
pub fn first_run(ctx: &Ctx, cmd: &str) -> Option<Output> {
    if cmd == "setup" || ctx.root.join(MARKER).exists() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let pf = rpyenv_core::winenv::program_files()?;
    if !exe.starts_with(&pf) {
        return None;
    }
    let r = run(ctx);
    let mut o = Output::new();
    o.err("pyenv: set up for this user (see `pyenv setup`)");
    o.stderr.push_str(&r.stderr);
    Some(o)
}
