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

/// A `.cmd` forwarder (spec §5.3). It resolves `name` with `pyenv which`, then runs it in
/// the caller's cmd, without `call` or `setlocal`, so whatever the batch file sets stays
/// set. The helper variable is cleared on the line that uses it: cmd expands `%…%` when it
/// reads the line. When `name` can't be resolved, `pyenv which` says why and errorlevel is
/// 127.
pub fn forwarder(pyenv: &Path, name: &str) -> String {
    let p = pyenv.display();
    format!(
        "@\"{p}\" which {name} >nul 2>&1 || (\"{p}\" which {name} & exit /b 127)\r\n\
         @for /f \"delims=\" %%i in ('\"\"{p}\" which {name}\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
         @(set \"RPYENV_FORWARD_TARGET=\") & \"%RPYENV_FORWARD_TARGET%\" %*\r\n"
    )
}

/// The shims rehash keeps in `shims` for the context's flavor.
pub fn wanted(ctx: &Ctx) -> Vec<Wanted> {
    match ctx.flavor {
        Flavor::Pyenv => executables_pyenv(&ctx.versions_dir())
            .into_iter()
            .map(|name| Wanted {
                name,
                kind: ShimKind::Console,
            })
            .collect(),
        Flavor::PyenvWin => shims_win(
            &ctx.versions_dir(),
            &forward_names(std::env::var_os("RPYENV_BATCH_FORWARD").as_deref()),
        ),
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
    fn forwarder_text() {
        assert_eq!(
            forwarder(Path::new(r"C:\bin\pyenv.exe"), "setvar"),
            "@\"C:\\bin\\pyenv.exe\" which setvar >nul 2>&1 || (\"C:\\bin\\pyenv.exe\" which setvar & exit /b 127)\r\n\
             @for /f \"delims=\" %%i in ('\"\"C:\\bin\\pyenv.exe\" which setvar\"') do @set \"RPYENV_FORWARD_TARGET=%%i\"\r\n\
             @(set \"RPYENV_FORWARD_TARGET=\") & \"%RPYENV_FORWARD_TARGET%\" %*\r\n"
        );
    }
}
