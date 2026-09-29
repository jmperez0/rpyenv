//! The shim program: a shim named `python` resolves and runs the selected version's
//! `python`, as `pyenv exec python` would (spec §5).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::launch::{self, ExecEnv, Mode};
use crate::rehash;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The shim binary's own name, which is never a command.
pub const SHIM_NAME: &str = "pyenv-shim";

/// The GUI-subsystem shim binary's own name, which is never a command.
pub const SHIMW_NAME: &str = "pyenv-shimw";

/// The console shim's main. Returns the exit code, unless the command replaced this process.
pub fn main() -> i32 {
    run(false)
}

/// The GUI shim's main: the same, except that a message with nowhere to go appears in a
/// message box.
pub fn main_gui() -> i32 {
    run(true)
}

/// Runs the shim and returns the exit code, unless the command replaced this process.
fn run(gui: bool) -> i32 {
    let flavor = Flavor::current();
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_default();
    let args: Vec<OsString> = argv.collect();
    let own = std::env::current_exe().ok();
    let Some(program) = command_name(flavor, &argv0, own.as_deref()) else {
        say(
            gui,
            flavor,
            &[format!(
                "{SHIM_NAME}: run this through a shim (such as `python`), not directly"
            )],
            true,
        );
        return 1;
    };
    let mut ctx = match Ctx::from_process() {
        Ok(ctx) => ctx,
        Err(e) => {
            say(gui, flavor, &[e.message()], true);
            return 1;
        }
    };
    // Upstream bakes PYENV_ROOT into each shim; rpyenv's shims share one binary, so they
    // find the root from where they live, not the caller's PYENV_ROOT / HOME. Everything
    // after this, including the PYENV_ROOT exported to the child, uses the shim's root.
    let path = std::env::var_os("PATH");
    if let Some(root) = own_root(flavor, &argv0, own.as_deref(), path.as_deref(), &ctx.pwd) {
        ctx.root = root;
    }
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
            say(gui, flavor, &report.lines, report.stderr);
            report.code
        }
        Ok(plan) => {
            #[cfg(windows)]
            let plan = launch::LaunchPlan {
                raw_tail: crate::wincmd::own_tail(1),
                ..plan
            };
            say(gui, flavor, &plan.warnings, true);
            launch::run(&plan, &ctx, rehash_with.as_deref())
        }
    }
}

/// Prints what the shim has to say on its stream, and logs it to `RPYENV_DEBUG_LOG`. The
/// GUI shim, when that stream isn't a usable handle, shows a message box instead
/// (plan decision 4).
fn say(gui: bool, flavor: Flavor, lines: &[String], to_stderr: bool) {
    if lines.is_empty() {
        return;
    }
    for line in lines {
        crate::debuglog::append(line);
    }
    #[cfg(windows)]
    if gui && !crate::winproc::std_handle_usable(to_stderr) {
        crate::winproc::message_box(&lines.join("\r\n"));
        return;
    }
    let _ = gui;
    crate::lookup::Report {
        lines: lines.to_vec(),
        stderr: to_stderr,
        code: 0,
    }
    .emit(flavor);
}

/// The command a shim stands for: `argv[0]`'s last component on Linux, where every shim
/// is a symlink to one binary; the file's own name without `.exe` on Windows, where each
/// shim is a hardlink named after its command. None for the shim binary's own name.
pub fn command_name(flavor: Flavor, argv0: &OsStr, own: Option<&Path>) -> Option<String> {
    let name = match flavor {
        Flavor::Pyenv => Path::new(argv0).file_name()?.to_string_lossy().into_owned(),
        Flavor::PyenvWin => own?.file_stem()?.to_string_lossy().into_owned(),
    };
    (!name.is_empty()
        && !name.eq_ignore_ascii_case(SHIM_NAME)
        && !name.eq_ignore_ascii_case(SHIMW_NAME))
    .then_some(name)
}

/// The parent of the `shims` folder the shim was run from: the root upstream bakes into
/// each shim at rehash time, found here instead from where the running shim actually lives,
/// since rpyenv's shims share one binary. `None` whenever any step fails, and the caller
/// falls back to `PYENV_ROOT` / `HOME`.
pub fn own_root(
    flavor: Flavor,
    argv0: &OsStr,
    own: Option<&Path>,
    path: Option<&OsStr>,
    cwd: &Path,
) -> Option<PathBuf> {
    match flavor {
        Flavor::PyenvWin => {
            let shims = own?.parent()?;
            let name = shims.file_name()?.to_string_lossy();
            if !name.eq_ignore_ascii_case("shims") {
                return None;
            }
            Some(shims.parent()?.to_path_buf())
        }
        Flavor::Pyenv => {
            let invoked = linux_invoked_path(argv0, own, path, cwd)?;
            let shims = invoked.parent()?;
            if shims.file_name()? != OsStr::new("shims") {
                return None;
            }
            Some(shims.parent()?.to_path_buf())
        }
    }
}

/// The path the shim was actually invoked as, on Linux: `argv0` itself when it contains a
/// `/` (joined onto `cwd` when relative, then lexically normalized) and it canonicalizes to
/// the same file as `own`; otherwise the first `PATH` entry whose `<dir>/<argv0>`
/// canonicalizes to the same file as `own`. The result is kept as spelled (not
/// canonicalized), matching what upstream bakes in.
fn linux_invoked_path(
    argv0: &OsStr,
    own: Option<&Path>,
    path: Option<&OsStr>,
    cwd: &Path,
) -> Option<PathBuf> {
    let own_canon = std::fs::canonicalize(own?).ok()?;
    if argv0.as_encoded_bytes().contains(&b'/') {
        let p = Path::new(argv0);
        let joined = if p.is_relative() {
            cwd.join(p)
        } else {
            p.to_path_buf()
        };
        // Any program can set argv[0]: trust it only when it really is this shim.
        let canon = std::fs::canonicalize(&joined).ok()?;
        return (canon == own_canon).then(|| crate::paths::lexical_normalize(&joined));
    }
    std::env::split_paths(path?).find_map(|dir| {
        let candidate = dir.join(argv0);
        let canon = std::fs::canonicalize(&candidate).ok()?;
        (canon == own_canon).then_some(candidate)
    })
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
        assert_eq!(win("pyenv-shimw.exe"), None);
        assert_eq!(command_name(Flavor::PyenvWin, OsStr::new("x"), None), None);
    }

    #[test]
    fn own_root_finds_the_windows_shims_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let own = tmp.path().join("root").join("shims").join("python.exe");
        assert_eq!(
            own_root(
                Flavor::PyenvWin,
                OsStr::new("x"),
                Some(&own),
                None,
                tmp.path()
            ),
            Some(tmp.path().join("root"))
        );
    }

    #[test]
    fn own_root_windows_wrong_folder_name_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let own = tmp.path().join("root").join("bin").join("python.exe");
        assert_eq!(
            own_root(
                Flavor::PyenvWin,
                OsStr::new("x"),
                Some(&own),
                None,
                tmp.path()
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn own_root_linux_trusts_an_argv0_with_a_slash_only_when_it_is_the_shim() {
        let tmp = tempfile::tempdir().unwrap();
        let shims = tmp.path().join("root").join("shims");
        std::fs::create_dir_all(&shims).unwrap();
        let own_bin = tmp.path().join("pyenv-shim");
        std::fs::write(&own_bin, b"bin").unwrap();
        std::os::unix::fs::symlink(&own_bin, shims.join("python")).unwrap();
        let argv0 = shims.join("python");
        assert_eq!(
            own_root(
                Flavor::Pyenv,
                argv0.as_os_str(),
                Some(&own_bin),
                None,
                tmp.path()
            ),
            Some(tmp.path().join("root"))
        );
        // Any program can set argv[0]; a path that isn't this shim is not trusted.
        let fake = tmp.path().join("fake").join("shims").join("python");
        std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
        std::fs::write(&fake, b"other").unwrap();
        assert_eq!(
            own_root(
                Flavor::Pyenv,
                fake.as_os_str(),
                Some(&own_bin),
                None,
                tmp.path()
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn own_root_linux_finds_itself_on_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root_shims = tmp.path().join("root").join("shims");
        std::fs::create_dir_all(&root_shims).unwrap();
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let own_bin = tmp.path().join("pyenv-shim");
        std::fs::write(&own_bin, b"bin").unwrap();
        std::os::unix::fs::symlink(&own_bin, root_shims.join("python")).unwrap();
        let path = std::env::join_paths([&other, &root_shims]).unwrap();
        assert_eq!(
            own_root(
                Flavor::Pyenv,
                OsStr::new("python"),
                Some(&own_bin),
                Some(path.as_os_str()),
                tmp.path(),
            ),
            Some(tmp.path().join("root"))
        );
    }
}
