//! Which command names get a shim (spec §8).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, pathsearch};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
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

/// pyenv-win's layout: `<stem>.exe` for every `.exe`, `.bat` and `.cmd` in a version's
/// folder, `Scripts` and `bin`. Names compare without case; the first spelling found wins.
pub fn shims_win(versions_dir: &Path) -> Vec<String> {
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
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
                by_key
                    .entry(stem.to_ascii_lowercase())
                    .or_insert_with(|| format!("{stem}.exe"));
            }
        }
    }
    by_key.into_values().collect()
}

/// The shim file names `rehash` keeps in `shims` for the context's flavor.
pub fn wanted(ctx: &Ctx) -> Vec<OsString> {
    match ctx.flavor {
        Flavor::Pyenv => executables_pyenv(&ctx.versions_dir()),
        Flavor::PyenvWin => shims_win(&ctx.versions_dir())
            .into_iter()
            .map(OsString::from)
            .collect(),
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
            shims_win(&v),
            [
                "extra.exe",
                "hello.exe",
                "pip.exe",
                "python.exe",
                "pythonw.exe"
            ]
        );
    }
}
