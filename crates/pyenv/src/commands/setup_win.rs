//! `pyenv setup` (rpyenv-only, spec §9.4): prepares one user's environment.

use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::{pathlist, winenv};
use std::path::PathBuf;

pub const MARKER: &str = ".rpyenv-setup";

/// Written when `--allow-scripts` set the user's execution policy, so `--undo` puts it back.
const ALLOWED_MARKER: &str = ".rpyenv-allowed-scripts";

/// Written when an all-users first run fails: the next one waits `RETRY_AFTER`.
const FAILED_MARKER: &str = ".rpyenv-setup-failed";
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

pub const HELP: &str = "Usage: pyenv setup [--allow-scripts]
       pyenv setup --undo

Prepares your Windows user for rpyenv: creates PYENV_ROOT, puts its shims folder first
on your user PATH, rehashes, and adds the PowerShell line to your profiles (Windows
PowerShell's only where its execution policy lets scripts run). It's safe to run again.

--allow-scripts first lets Windows PowerShell run local scripts for you, where it doesn't:
it sets your own execution policy (CurrentUser) to RemoteSigned, unless Group Policy or
you already chose one.

--undo takes back what setup added for this user: the shims entry on your user PATH, the
PowerShell line and an execution policy --allow-scripts set. Your Python versions stay.
";

pub fn setup(ctx: &Ctx, args: &[&str]) -> Output {
    match args {
        [] => run(ctx),
        ["--allow-scripts"] => run_with(ctx, true),
        ["--undo"] => undo(ctx),
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
    run_with(ctx, false)
}

fn run_with(ctx: &Ctx, allow_scripts: bool) -> Output {
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
    // Final review M1: any failure fails setup and leaves the root unmarked, so the
    // all-users first run tries again.
    let mut failed = false;
    match winenv::get(winenv::Scope::User, "Path") {
        // Final review I5: never write over a Path that couldn't be read.
        Err(e) => {
            o.err(format!(
                "pyenv: cannot read your user PATH ({e}); left it as it is"
            ));
            failed = true;
        }
        Ok(current) => {
            let current = current.unwrap_or(winenv::Value {
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
                    Err(e) => {
                        o.err(format!("pyenv: cannot change your user PATH: {e}"));
                        failed = true;
                    }
                },
            }
        }
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    o.stderr.push_str(&r.stderr);
    failed |= r.code != 0;
    if allow_scripts {
        failed |= !allow_local_scripts(ctx, &mut o);
    }
    let p = profiles(&mut o);
    o.stdout.push_str(&p.stdout);
    o.stderr.push_str(&p.stderr);
    failed |= p.code != 0;
    if let Ok(Some(machine)) = winenv::get(winenv::Scope::Machine, "Path") {
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
    if failed {
        o.err("pyenv: setup didn't finish; fix the above and run `pyenv setup` again");
        o.code = 1;
        return o;
    }
    let _ = std::fs::write(ctx.root.join(MARKER), env!("CARGO_PKG_VERSION"));
    o.out("pyenv: open a new terminal for the changes to take effect");
    o
}

/// `--allow-scripts` (the MSI's checkbox): where Windows PowerShell blocks local scripts and
/// neither Group Policy nor the user chose that policy, sets the user's own (CurrentUser)
/// policy to RemoteSigned, recording it for `--undo`. False when that failed.
fn allow_local_scripts(ctx: &Ctx, o: &mut Output) -> bool {
    let policy = winenv::windows_powershell_policy();
    // Group Policy's hint comes from `profiles`.
    if !policy.blocks_local_scripts() || policy.by_group_policy {
        return true;
    }
    let own = winenv::current_user_policy()
        .filter(|p| !p.trim().is_empty() && !p.trim().eq_ignore_ascii_case("Undefined"));
    if let Some(own) = own {
        o.err(format!(
            "pyenv: your own execution policy for Windows PowerShell is {own}; left it as it is"
        ));
        return true;
    }
    // The record first: a policy set without it would never be taken back.
    let record = ctx.root.join(ALLOWED_MARKER);
    if let Err(e) = std::fs::write(&record, "RemoteSigned") {
        o.err(format!("pyenv: cannot write {}: {e}", record.display()));
        return false;
    }
    match winenv::set_current_user_policy(Some("RemoteSigned")) {
        Ok(()) => {
            o.out("pyenv: Windows PowerShell may now run local scripts for you (execution policy RemoteSigned, your user only; `pyenv setup --undo` puts it back)");
            true
        }
        Err(e) => {
            let _ = std::fs::remove_file(&record);
            o.err(format!(
                "pyenv: cannot set your execution policy for Windows PowerShell: {e}"
            ));
            false
        }
    }
}

/// `--undo` for `--allow-scripts`: removes the CurrentUser policy if it's still the
/// RemoteSigned setup set; one the user changed since stays. False when that failed.
fn undo_allow_scripts(ctx: &Ctx, o: &mut Output) -> bool {
    let record = ctx.root.join(ALLOWED_MARKER);
    if !record.exists() {
        return true;
    }
    let ours = winenv::current_user_policy()
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("RemoteSigned"));
    if ours {
        if let Err(e) = winenv::set_current_user_policy(None) {
            o.err(format!(
                "pyenv: cannot take back your execution policy for Windows PowerShell: {e}"
            ));
            return false;
        }
        o.out("pyenv: took back the execution policy setup set for Windows PowerShell");
    }
    match std::fs::remove_file(&record) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            o.err(format!("pyenv: cannot remove {ALLOWED_MARKER}: {e}"));
            false
        }
    }
}

/// The PowerShell line in both profiles, except Windows PowerShell 5.1's where its execution
/// policy blocks local scripts: that profile would fail on every start (Sandbox S1). The
/// hint goes to `o`; `pyenv init --install pwsh` adds the line whatever the policy.
fn profiles(o: &mut Output) -> Output {
    use crate::commands::pwsh_profile;
    let Some(docs) = winenv::documents() else {
        return Output::error(pwsh_profile::NO_DOCUMENTS);
    };
    pwsh_profile::install(&profile_targets(&docs, o))
}

/// The profiles that get the PowerShell line (setup's and migrate's): both, or only
/// PowerShell 7's where Windows PowerShell can't run scripts, with the hint in `o`.
pub(crate) fn profile_targets(docs: &std::path::Path, o: &mut Output) -> Vec<PathBuf> {
    let [p5, p7] = crate::commands::pwsh_profile::paths(docs);
    let policy = winenv::windows_powershell_policy();
    if !policy.blocks_local_scripts() {
        return vec![p5, p7];
    }
    if policy.by_group_policy {
        o.err(format!(
            "pyenv: Windows PowerShell can't run scripts here (execution policy {}, set by Group Policy), so its profile was left alone",
            policy.name
        ));
    } else {
        o.err(format!(
            "pyenv: Windows PowerShell can't run scripts here (execution policy {}), so its profile was left alone; to use pyenv in it, run `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`, then `pyenv init --install pwsh`",
            policy.name
        ));
    }
    vec![p7]
}

/// M6b design §5: takes back what `run` added for this user (the MSI's uninstall runs
/// it). Never touches PYENV_ROOT's versions or the shims folder's files.
pub fn undo(ctx: &Ctx) -> Output {
    let mut o = Output::new();
    let mut failed = false;
    let shims = ctx.shims_dir().display().to_string();
    match winenv::get(winenv::Scope::User, "Path") {
        // M6a I5: never write over a Path that couldn't be read.
        Err(e) => {
            o.err(format!(
                "pyenv: cannot read your user PATH ({e}); left it as it is"
            ));
            failed = true;
        }
        Ok(None) => {}
        Ok(Some(v)) => {
            let (text, gone) =
                pathlist::without(&v.text, &|e| pathlist::same(&winenv::expand(e), &shims));
            if !gone.is_empty() {
                match winenv::set_user(
                    "Path",
                    &winenv::Value {
                        text,
                        expand: v.expand,
                    },
                ) {
                    Ok(()) => {
                        o.out(format!("pyenv: took {shims} off your user PATH"));
                        winenv::broadcast();
                    }
                    Err(e) => {
                        o.err(format!("pyenv: cannot change your user PATH: {e}"));
                        failed = true;
                    }
                }
            }
        }
    }
    if let Some(docs) = winenv::documents() {
        for p in crate::commands::pwsh_profile::paths(&docs) {
            match crate::commands::pwsh_profile::remove(&p) {
                Ok(true) => o.out(format!(
                    "pyenv: took the PowerShell line out of {}",
                    p.display()
                )),
                Ok(false) => {}
                Err(e) => {
                    o.err(format!("pyenv: {}: {e}", p.display()));
                    failed = true;
                }
            }
        }
    }
    failed |= !undo_allow_scripts(ctx, &mut o);
    for marker in [MARKER, FAILED_MARKER] {
        match std::fs::remove_file(ctx.root.join(marker)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                o.err(format!("pyenv: cannot remove {marker}: {e}"));
                failed = true;
            }
        }
    }
    if failed {
        o.code = 1;
    }
    o
}

/// `path` is inside `dir`, comparing names as Windows does: in any case (final review M5).
fn under(path: &std::path::Path, dir: &std::path::Path) -> bool {
    let names = |p: &std::path::Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let (path, dir) = (names(path), names(dir));
    path.len() > dir.len() && path[..dir.len()] == dir[..]
}

/// Spec §9.4: an all-users install (this pyenv.exe under Program Files) sets the user up
/// on their first command (plan M6a, R6). `None` when nothing was needed.
pub fn first_run(ctx: &Ctx, cmd: &str) -> Option<Output> {
    // Once per process: `completions` runs pyenv again in-process (re-review 6).
    static TRIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if cmd == "setup" || ctx.root.join(MARKER).exists() {
        return None;
    }
    // A first run that failed is retried a day later, not on every command (and every new
    // shell's `pyenv init`); `pyenv setup` retries at once (re-review 6).
    let failed = ctx.root.join(FAILED_MARKER);
    let recent = std::fs::metadata(&failed)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < RETRY_AFTER));
    if recent {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let pf = rpyenv_core::winenv::program_files()?;
    if !under(&exe, &pf) || TRIED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let r = run(ctx);
    let mut o = Output::new();
    o.err("pyenv: set up for this user (see `pyenv setup`)");
    o.stderr.push_str(&r.stderr);
    if r.code != 0 {
        let _ = std::fs::write(&failed, "");
        o.err("pyenv: this is tried again tomorrow, or now with `pyenv setup`");
    } else {
        let _ = std::fs::remove_file(&failed);
    }
    Some(o)
}
