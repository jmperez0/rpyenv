//! Which command names get a shim (spec §8).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, pathsearch};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// Activation scripts must run inside the current shell, so they never get a shim: any
/// file whose name without its last extension is `activate` or `deactivate` (allowlist D-34).
pub fn is_activation(file_name: &str) -> bool {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    stem.eq_ignore_ascii_case("activate") || stem.eq_ignore_ascii_case("deactivate")
}

/// Upstream `pyenv versions --executables`, filtered: the runnable files in every
/// `versions/*/bin` and `versions/*/envs/*/bin`, without dotfiles or activation scripts.
/// Byte order, no duplicates (allowlist D-33).
pub fn executables_pyenv(versions_dir: &Path) -> Vec<OsString> {
    let mut names = BTreeSet::new();
    for entry in installed::top_level(versions_dir, Flavor::Pyenv) {
        let envs = installed::envs_of(&entry);
        for e in std::iter::once(entry).chain(envs) {
            let Ok(rd) = std::fs::read_dir(e.path.join("bin")) else {
                continue;
            };
            for f in rd.filter_map(Result::ok) {
                let name = f.file_name();
                let text = name.to_string_lossy();
                if !text.starts_with('.')
                    && !is_activation(&text)
                    && pathsearch::is_runnable(&f.path())
                {
                    names.insert(name);
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Which shim binary a shim is. Linux shims are always `Console`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimKind {
    /// `pyenv-shim`.
    Console,
    /// `pyenv-shimw`: every version's file with this name is a GUI program.
    Gui,
    /// A `.cmd` forwarder, for a batch tool named in `RPYENV_BATCH_FORWARD`.
    Forward,
}

/// One file rehash keeps in `shims`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub name: OsString,
    pub kind: ShimKind,
}

/// pyenv-win's layout: `<stem>.exe` for every `.exe`, `.bat` and `.cmd` in a version's
/// folder, `Scripts` and `bin`, each with the shim binary it needs: GUI only when every
/// version's file is a GUI program, and a `.cmd` forwarder for a name in `forward`
/// (`RPYENV_BATCH_FORWARD`, spec §5.3). Names compare without case; when versions
/// disagree, a forwarder beats an exe shim, and a console shim beats a GUI shim.
pub fn shims_win(versions_dir: &Path, forward: &[String]) -> Vec<Wanted> {
    let mut by_key: BTreeMap<String, Wanted> = BTreeMap::new();
    for entry in installed::top_level(versions_dir, Flavor::PyenvWin) {
        for dir in [
            entry.path.clone(),
            entry.path.join("Scripts"),
            entry.path.join("bin"),
        ] {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for f in rd.filter_map(Result::ok) {
                let path = f.path();
                let (Some(stem), Some(ext)) = (
                    path.file_stem().and_then(|s| s.to_str()),
                    path.extension().and_then(|s| s.to_str()),
                ) else {
                    continue;
                };
                let ext = ext.to_ascii_lowercase();
                if !matches!(ext.as_str(), "exe" | "bat" | "cmd")
                    || stem.starts_with('.')
                    || is_activation(&f.file_name().to_string_lossy())
                    || !path.is_file()
                {
                    continue;
                }
                let lower = stem.to_ascii_lowercase();
                let is_batch = ext == "bat" || ext == "cmd";
                let candidate = if is_batch && forward.contains(&lower) {
                    Wanted {
                        name: OsString::from(format!("{stem}.cmd")),
                        kind: ShimKind::Forward,
                    }
                } else if ext == "exe" && crate::pe::is_gui(&path) {
                    Wanted {
                        name: OsString::from(format!("{stem}.exe")),
                        kind: ShimKind::Gui,
                    }
                } else {
                    Wanted {
                        name: OsString::from(format!("{stem}.exe")),
                        kind: ShimKind::Console,
                    }
                };
                let rank = |k: ShimKind| match k {
                    ShimKind::Gui => 0,
                    ShimKind::Console => 1,
                    ShimKind::Forward => 2,
                };
                let slot = by_key.entry(lower).or_insert(candidate.clone());
                if rank(candidate.kind) > rank(slot.kind) {
                    *slot = candidate;
                }
            }
        }
    }
    by_key.into_values().collect()
}

/// The names in `RPYENV_BATCH_FORWARD` (`;`-separated), lowercased.
pub fn forward_names(value: Option<&OsStr>) -> Vec<String> {
    value
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default()
        .split(';')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect()
}

/// Where a forwarder's `pyenv.exe` reference comes from. A `.cmd` file is itself read by
/// `cmd.exe` in the console's code page (spec §5.3), so a non-ASCII `pyenv` path can't
/// just be written as bytes into the file — the code page active when the forwarder runs
/// isn't known at rehash time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyenvRef {
    /// `pyenv`'s own path, pure ASCII. Kept absolute, not `%~dp0`-relative, even though
    /// relative would be simpler: `%~dp0` misresolves when a batch file is invoked quoted
    /// and without its extension (`pyenv_reference`'s doc has the details).
    Absolute(String),
    /// A path relative to `shims` (also ASCII — only the tail after their common ancestor
    /// need be, since the shared, possibly non-ASCII, root is never written to the file;
    /// `forwarder` resolves the forwarder's own folder at run time instead of reading it
    /// from the file).
    Relative(String),
}

/// `pyenv`'s reference for a forwarder, or `None` when nothing can be written safely (the
/// caller falls back to a console exe shim, as when `pyenv.exe` isn't known at all).
///
/// When `pyenv`'s path is pure ASCII, it is written absolute. Otherwise the reference is a
/// relative path from `shims` to `pyenv` — cmd resolves the forwarder's own folder itself
/// at run time (`forwarder`'s `:d` subroutine) rather than reading it from the file, so
/// only the relative suffix needs to be in the file, and that's usually ASCII even when
/// the shared root isn't. `None` when that relative path is itself non-ASCII (pyenv and
/// shims share no ASCII-only tail) or `pyenv` and `shims` are on different drives: there's
/// no relative reference and the absolute one isn't safe.
pub fn pyenv_reference(pyenv: &Path, shims: &Path) -> Option<PyenvRef> {
    let text = pyenv.display().to_string();
    if text.is_ascii() {
        return Some(PyenvRef::Absolute(text));
    }
    let rel = relative_path(&shims.display().to_string(), &text)?;
    if !rel.is_ascii() {
        return None;
    }
    Some(PyenvRef::Relative(rel))
}

/// A relative path from `from_dir` to `to`, both spelled as absolute Windows paths
/// (`C:\a\b`), built from their common leading components (`..` for each of `from_dir`'s
/// remaining components, then `to`'s own tail; components compare case-insensitively, as
/// Windows path segments do). `None` when they share no leading component at all, which
/// includes two different drives. Splits on `\` rather than `Path::components()`, which
/// parses drive letters only on Windows, so this (and its tests) run the same on every OS
/// (spec's `%~dp0` is Windows-only, but the string math behind it isn't).
fn relative_path(from_dir: &str, to: &str) -> Option<String> {
    fn split(p: &str) -> Vec<&str> {
        p.split('\\').filter(|c| !c.is_empty()).collect()
    }
    let from = split(from_dir);
    let dest = split(to);
    let common = from
        .iter()
        .zip(dest.iter())
        .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
        .count();
    if common == 0 {
        return None;
    }
    let ups = std::iter::repeat_n("..", from.len() - common);
    Some(
        ups.chain(dest[common..].iter().copied())
            .collect::<Vec<_>>()
            .join("\\"),
    )
}

/// A `.cmd` forwarder (spec §5.3). It resolves `name` with `pyenv which`, then runs it in
/// the caller's cmd, without `call` or `setlocal`, so whatever the batch file sets stays
/// set. When `name` can't be resolved at all, `pyenv which` says why (the first `which`
/// line) and errorlevel is 127. `name` itself must be ASCII (the caller checks; `pyenv_ref`
/// alone isn't enough, since a non-ASCII *name* would also need non-ASCII bytes in the
/// file).
///
/// `pyenv_ref`'s path goes into the helper variable `RPYENV_FORWARD_PYENV`, not directly
/// into the command lines: `&`, `(`, `)`, `^` and `%` are cmd metacharacters, and a real
/// Windows path (`C:\Program Files (x86)\…`) can contain any of them. Assigned inside a
/// quoted `set "VAR=value"`, they're inert; used afterward only as `%RPYENV_FORWARD_PYENV%`
/// inside quotes, they stay inert. A literal `%` in the path itself is doubled first, so
/// `set` doesn't try to expand it as a variable reference of its own. Inside the `for /f`
/// command string, the reference is doubled again (`%%RPYENV_FORWARD_PYENV%%`): that
/// string is parsed twice, once by this line and once by the child cmd `for /f` spawns to
/// run it.
///
/// A `Relative` reference needs the forwarder's own folder, which `%~dp0` misresolves for
/// a quoted invocation (`"setvar"`) or one without the extension typed (`setvar` without
/// `.cmd`). Resolved instead by an explicit `call :d` up front, into
/// `RPYENV_FORWARD_DIR`, only when needed. The `:d` subroutine comes first, jumped over by
/// `@goto :m`, so nothing follows the line that runs the target: running a `.bat`/`.cmd`
/// target without `call` abandons this script (the usual case), but an `.exe` target
/// returns here, and with any line after it (`goto :eof` and `exit /b` both do this)
/// `cmd /c setvar` exits 0 instead of with the target's code, though errorlevel is kept.
///
/// The second-to-last line sets `RPYENV_FORWARD_CP` around its `for /f`, after first
/// clearing `RPYENV_FORWARD_TARGET`: with `RPYENV_FORWARD_CP` set, `pyenv which` writes
/// the path in the console's code page instead of UTF-8, since that's how `for /f` will
/// decode the bytes it reads from the pipe; clearing `RPYENV_FORWARD_TARGET` first means a
/// value some earlier, unrelated command left behind can't survive a `for /f` that
/// captures nothing (an unmappable path) and get treated as this run's result — the final
/// line's own `%RPYENV_FORWARD_TARGET%` is substituted once, when cmd reads that line,
/// using whatever this line left behind. The final line clears every helper variable on
/// the line that uses them — for the same reason, that doesn't affect its own use of
/// them — and exits 127 if `for /f` captured nothing (a plain not-found already exited at
/// the first `which` line; this catches the code-page failure instead).
pub fn forwarder(pyenv_ref: &PyenvRef, name: &str) -> String {
    let (needs_dir, value) = match pyenv_ref {
        PyenvRef::Absolute(p) => (false, p.replace('%', "%%")),
        PyenvRef::Relative(rel) => (
            true,
            format!("%RPYENV_FORWARD_DIR%{}", rel.replace('%', "%%")),
        ),
    };
    let head = if needs_dir {
        "@call :d\r\n@goto :m\r\n:d\r\n@set \"RPYENV_FORWARD_DIR=%~dp0\"\r\n@exit /b\r\n:m\r\n"
    } else {
        ""
    };
    let clear_dir = if needs_dir {
        " & (set \"RPYENV_FORWARD_DIR=\")"
    } else {
        ""
    };
    format!(
        "{head}\
         @set \"RPYENV_FORWARD_PYENV={value}\"\r\n\
         @\"%RPYENV_FORWARD_PYENV%\" which {name} >nul 2>&1 || (\"%RPYENV_FORWARD_PYENV%\" which {name} & (set \"RPYENV_FORWARD_PYENV=\"){clear_dir} & exit /b 127)\r\n\
         @(set \"RPYENV_FORWARD_TARGET=\") & (set \"RPYENV_FORWARD_CP=1\") & for /f \"delims=\" %%i in ('\"\"%%RPYENV_FORWARD_PYENV%%\" which {name}\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
         @(set \"RPYENV_FORWARD_CP=\") & (set \"RPYENV_FORWARD_PYENV=\") & (set \"RPYENV_FORWARD_TARGET=\"){clear_dir} & if \"%RPYENV_FORWARD_TARGET%\"==\"\" (exit /b 127) else (\"%RPYENV_FORWARD_TARGET%\" %*)\r\n"
    )
}

/// The shims rehash keeps in `shims` for the context's flavor. `forward` is the batch
/// forward list (Windows); the caller decides where it comes from (`rehash::batch_forward`).
pub fn wanted(ctx: &Ctx, forward: &[String]) -> Vec<Wanted> {
    match ctx.flavor {
        Flavor::Pyenv => executables_pyenv(&ctx.versions_dir())
            .into_iter()
            .map(|name| Wanted {
                name,
                kind: ShimKind::Console,
            })
            .collect(),
        Flavor::PyenvWin => shims_win(&ctx.versions_dir(), forward),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file(p: &Path, runnable: bool) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if runnable { 0o755 } else { 0o644 };
            fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = runnable;
    }

    #[test]
    fn activation_scripts() {
        for n in [
            "activate",
            "activate.csh",
            "activate.fish",
            "activate.nu",
            "activate.bat",
            "deactivate.bat",
            "Activate.ps1",
        ] {
            assert!(is_activation(n), "{n}");
        }
        for n in ["activate_this.py", "python", "pip3", "deactivated"] {
            assert!(!is_activation(n), "{n}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn linux_names_are_runnable_files_in_bin_and_env_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        file(&v.join("3.12.1/bin/python"), true);
        file(&v.join("3.12.1/bin/has space"), true);
        file(&v.join("3.12.1/bin/readme"), false);
        file(&v.join("3.12.1/bin/.hidden"), true);
        file(&v.join("3.12.1/bin/activate"), true);
        fs::create_dir_all(v.join("3.12.1/bin/subdir")).unwrap();
        file(&v.join("3.12.1/envs/e1/bin/black"), true);
        file(&v.join("3.11.9/bin/python"), true);
        file(&v.join("3.11.9/lib/x/bin/deep"), true);
        assert_eq!(
            executables_pyenv(&v),
            ["black", "has space", "python"].map(OsString::from)
        );
    }

    #[test]
    fn windows_shims_for_exe_bat_cmd_in_folder_scripts_and_bin() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for f in [
            "3.9.1/python.exe",
            "3.9.1/python.bat",
            "3.9.1/pythonw.exe",
            "3.9.1/Scripts/pip.exe",
            "3.9.1/Scripts/hello.cmd",
            "3.9.1/Scripts/activate.bat",
            "3.9.1/Scripts/tool.py",
            "3.9.1/Scripts/.hook.bat",
            "3.9.1/bin/extra.EXE",
            "3.9.1/lib/deep.exe",
        ] {
            file(&v.join(f), true);
        }
        assert_eq!(
            shims_win(&v, &[])
                .iter()
                .map(|w| w.name.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            [
                "extra.exe",
                "hello.exe",
                "pip.exe",
                "python.exe",
                "pythonw.exe"
            ]
        );
    }

    #[test]
    fn gui_only_when_every_target_is_gui() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        let write = |rel: &str, sub: u16| {
            let p = v.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, crate::pe::image(sub)).unwrap();
        };
        write("3.9.1/pythonw.exe", 2);
        write("3.9.1/tool.exe", 2);
        write("3.8.2/tool.exe", 3);
        let kinds: Vec<(String, ShimKind)> = shims_win(&v, &[])
            .into_iter()
            .map(|w| (w.name.to_string_lossy().into_owned(), w.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("pythonw.exe".to_string(), ShimKind::Gui),
                ("tool.exe".to_string(), ShimKind::Console)
            ]
        );
    }

    #[test]
    fn forward_names_parse() {
        assert_eq!(
            forward_names(Some(OsStr::new(" Setvar ;;activate-env;"))),
            ["setvar", "activate-env"]
        );
        assert!(forward_names(None).is_empty());
    }

    #[test]
    fn listed_batch_tools_get_forwarders() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for f in [
            "3.9.1/python.exe",
            "3.9.1/Scripts/setvar.bat",
            "3.9.1/Scripts/other.bat",
        ] {
            file(&v.join(f), true);
        }
        let kinds: Vec<(String, ShimKind)> = shims_win(&v, &["setvar".to_string()])
            .into_iter()
            .map(|w| (w.name.to_string_lossy().into_owned(), w.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("other.exe".to_string(), ShimKind::Console),
                ("python.exe".to_string(), ShimKind::Console),
                ("setvar.cmd".to_string(), ShimKind::Forward),
            ]
        );
    }

    #[test]
    fn forwarder_text_absolute() {
        assert_eq!(
            forwarder(&PyenvRef::Absolute(r"C:\bin\pyenv.exe".to_string()), "setvar"),
            "@set \"RPYENV_FORWARD_PYENV=C:\\bin\\pyenv.exe\"\r\n\
             @\"%RPYENV_FORWARD_PYENV%\" which setvar >nul 2>&1 || (\"%RPYENV_FORWARD_PYENV%\" which setvar & (set \"RPYENV_FORWARD_PYENV=\") & exit /b 127)\r\n\
             @(set \"RPYENV_FORWARD_TARGET=\") & (set \"RPYENV_FORWARD_CP=1\") & for /f \"delims=\" %%i in ('\"\"%%RPYENV_FORWARD_PYENV%%\" which setvar\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
             @(set \"RPYENV_FORWARD_CP=\") & (set \"RPYENV_FORWARD_PYENV=\") & (set \"RPYENV_FORWARD_TARGET=\") & if \"%RPYENV_FORWARD_TARGET%\"==\"\" (exit /b 127) else (\"%RPYENV_FORWARD_TARGET%\" %*)\r\n"
        );
    }

    #[test]
    fn forwarder_text_relative_uses_a_call_label_subroutine() {
        assert_eq!(
            forwarder(&PyenvRef::Relative(r"..\bin\pyenv.exe".to_string()), "setvar"),
            "@call :d\r\n\
             @goto :m\r\n\
             :d\r\n\
             @set \"RPYENV_FORWARD_DIR=%~dp0\"\r\n\
             @exit /b\r\n\
             :m\r\n\
             @set \"RPYENV_FORWARD_PYENV=%RPYENV_FORWARD_DIR%..\\bin\\pyenv.exe\"\r\n\
             @\"%RPYENV_FORWARD_PYENV%\" which setvar >nul 2>&1 || (\"%RPYENV_FORWARD_PYENV%\" which setvar & (set \"RPYENV_FORWARD_PYENV=\") & (set \"RPYENV_FORWARD_DIR=\") & exit /b 127)\r\n\
             @(set \"RPYENV_FORWARD_TARGET=\") & (set \"RPYENV_FORWARD_CP=1\") & for /f \"delims=\" %%i in ('\"\"%%RPYENV_FORWARD_PYENV%%\" which setvar\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
             @(set \"RPYENV_FORWARD_CP=\") & (set \"RPYENV_FORWARD_PYENV=\") & (set \"RPYENV_FORWARD_TARGET=\") & (set \"RPYENV_FORWARD_DIR=\") & if \"%RPYENV_FORWARD_TARGET%\"==\"\" (exit /b 127) else (\"%RPYENV_FORWARD_TARGET%\" %*)\r\n"
        );
    }

    #[test]
    fn forwarder_text_doubles_a_literal_percent_in_the_absolute_path() {
        let text = forwarder(
            &PyenvRef::Absolute(r"C:\1%0\pyenv.exe".to_string()),
            "setvar",
        );
        assert!(
            text.starts_with("@set \"RPYENV_FORWARD_PYENV=C:\\1%%0\\pyenv.exe\"\r\n"),
            "{text}"
        );
    }

    #[test]
    fn pyenv_reference_ascii_path_is_absolute() {
        assert_eq!(
            pyenv_reference(Path::new(r"C:\bin\pyenv.exe"), Path::new(r"C:\root\shims")),
            Some(PyenvRef::Absolute(r"C:\bin\pyenv.exe".to_string()))
        );
    }

    #[test]
    fn pyenv_reference_non_ascii_path_is_relative_to_shims() {
        assert_eq!(
            pyenv_reference(
                Path::new(r"C:\Users\José\.pyenv\bin\pyenv.exe"),
                Path::new(r"C:\Users\José\.pyenv\shims"),
            ),
            Some(PyenvRef::Relative(r"..\bin\pyenv.exe".to_string()))
        );
    }

    #[test]
    fn pyenv_reference_non_ascii_relative_tail_is_none() {
        // The shared root ("José") is non-ASCII, but so is the part that differs
        // ("bín"), so the relative path itself can't be written into the file either.
        assert_eq!(
            pyenv_reference(
                Path::new(r"C:\Users\José\bín\pyenv.exe"),
                Path::new(r"C:\Users\José\shims"),
            ),
            None
        );
    }

    #[test]
    fn pyenv_reference_non_ascii_path_on_another_drive_is_none() {
        assert_eq!(
            pyenv_reference(Path::new(r"D:\ñ\pyenv.exe"), Path::new(r"C:\root\shims")),
            None
        );
    }
}
