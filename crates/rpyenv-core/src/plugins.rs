//! Plugin dispatch (spec §6, decision D4): `pyenv foo` runs `pyenv-foo` from the plugin
//! folders or `PATH`, in the environment upstream's dispatcher builds (libexec/pyenv:79-106;
//! M1 reference "Environment setup").

use crate::flavor::Flavor;
use crate::pathsearch;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// `<base>/plugins/*/<sub>` that exist, in glob order (byte order, no dot names).
fn plugin_dirs(base: &Path, sub: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(base.join("plugins")) else {
        return Vec::new();
    };
    let mut names: Vec<OsString> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .filter(|n| !n.to_string_lossy().starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| base.join("plugins").join(n).join(sub))
        .filter(|d| d.is_dir())
        .collect()
}

/// The folders the dispatcher puts in front of `PATH`, first to last: `<prefix>/libexec`,
/// `$PYENV_ROOT/plugins/*/bin` (only when the prefix isn't the root), then
/// `<prefix>/plugins/*/bin`. Each glob is reversed, because upstream prepends every match
/// in turn (libexec/pyenv:84-94).
pub fn front_dirs(root: &Path, prefix: Option<&Path>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = prefix {
        v.push(p.join("libexec"));
    }
    if prefix != Some(root) {
        v.extend(plugin_dirs(root, "bin").into_iter().rev());
    }
    if let Some(p) = prefix {
        v.extend(plugin_dirs(p, "bin").into_iter().rev());
    }
    v
}

/// `front`, then the inherited `PATH`, joined as text with the flavor's separator, as
/// upstream's `export PATH="…:${PATH}"` does (no validation of the entries).
pub fn dispatch_path(front: &[PathBuf], inherited: Option<&OsStr>, flavor: Flavor) -> OsString {
    let sep = match flavor {
        Flavor::Pyenv => ":",
        Flavor::PyenvWin => ";",
    };
    let mut s = OsString::new();
    for d in front {
        s.push(d.as_os_str());
        s.push(sep);
    }
    if let Some(p) = inherited {
        s.push(p);
    }
    s
}

/// `PYENV_HOOK_PATH` as upstream exports it (libexec/pyenv:96-106).
pub fn hook_path(inherited: Option<&str>, root: &Path, prefix: Option<&Path>) -> String {
    let mut s = inherited.unwrap_or("").to_string();
    s.push_str(&format!(":{}/pyenv.d", root.display()));
    if let Some(p) = prefix.filter(|p| *p != root) {
        s.push_str(&format!(":{}/pyenv.d", p.display()));
    }
    s.push_str(":/usr/etc/pyenv.d:/usr/local/etc/pyenv.d:/etc/pyenv.d:/usr/lib/pyenv/hooks");
    for d in plugin_dirs(root, "etc/pyenv.d") {
        s.push_str(&format!(":{}", d.display()));
    }
    s.strip_prefix(':').map(str::to_string).unwrap_or(s)
}

/// `command -v pyenv-<name>` on the dispatch `PATH`: the first executable file, or on
/// Windows the first `PATHEXT` match. On Linux, with no executable one, the first file of
/// that name, as bash's `command -v` falls back to it (running it then fails).
pub fn find(name: &str, path: &OsStr, flavor: Flavor, pathext: Option<&OsStr>) -> Option<PathBuf> {
    let file = format!("pyenv-{name}");
    pathsearch::find_first(&file, Some(path), None, flavor, pathext).or_else(|| {
        (flavor == Flavor::Pyenv)
            .then(|| {
                std::env::split_paths(path)
                    .map(|d| d.join(&file))
                    .find(|p| p.is_file())
            })
            .flatten()
    })
}

/// The names `pyenv-*` files on the dispatch `PATH` give, `pyenv-` removed, each once
/// (libexec/pyenv-commands:23-47). Linux takes any file, as upstream's glob does; Windows
/// takes `PATHEXT` files and drops the extension.
pub fn listed_names(path: &OsStr, flavor: Flavor, pathext: Option<&OsStr>) -> Vec<String> {
    let exts: Vec<String> = pathext
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".com;.exe;.bat;.cmd".to_string())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect();
    let mut names = Vec::new();
    for dir in std::env::split_paths(path) {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = file.strip_prefix("pyenv-") else {
                continue;
            };
            let name = match flavor {
                Flavor::Pyenv => Some(rest.to_string()),
                Flavor::PyenvWin => {
                    let lower = rest.to_ascii_lowercase();
                    exts.iter()
                        .find(|x| lower.ends_with(x.as_str()))
                        .map(|x| rest[..rest.len() - x.len()].to_string())
                }
            };
            if let Some(n) = name.filter(|n| !n.is_empty()) {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dirs(base: &Path, plugins: &[&str]) {
        for p in plugins {
            fs::create_dir_all(base.join("plugins").join(p).join("bin")).unwrap();
        }
    }

    /// test/pyenv.bats:49-58: libexec, the root's plugins (reverse glob order), then the
    /// prefix's; a root equal to the prefix is read once.
    #[test]
    fn front_dirs_follow_upstream_order() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, prefix) = (tmp.path().join("root"), tmp.path().join("prefix"));
        dirs(&root, &["python-build", "pyenv-each", ".hidden"]);
        dirs(&prefix, &["python-build"]);
        assert_eq!(
            front_dirs(&root, Some(&prefix)),
            vec![
                prefix.join("libexec"),
                root.join("plugins/python-build/bin"),
                root.join("plugins/pyenv-each/bin"),
                prefix.join("plugins/python-build/bin"),
            ]
        );
        assert_eq!(
            front_dirs(&root, Some(&root)),
            vec![
                root.join("libexec"),
                root.join("plugins/python-build/bin"),
                root.join("plugins/pyenv-each/bin"),
            ]
        );
    }

    /// test/pyenv.bats:60-72: the inherited value first, one leading `:` dropped.
    #[test]
    fn hook_path_as_upstream() {
        let root = Path::new("/r");
        let prefix = Path::new("/p");
        let tail = ":/usr/etc/pyenv.d:/usr/local/etc/pyenv.d:/etc/pyenv.d:/usr/lib/pyenv/hooks";
        assert_eq!(
            hook_path(None, root, Some(prefix)),
            format!("/r/pyenv.d:/p/pyenv.d{tail}")
        );
        assert_eq!(
            hook_path(Some("/my/hook/path:/other/hooks"), root, Some(root)),
            format!("/my/hook/path:/other/hooks:/r/pyenv.d{tail}")
        );
    }

    #[cfg(unix)]
    #[test]
    fn find_wants_an_executable_and_list_wants_any_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("my commands");
        fs::create_dir_all(&bin).unwrap();
        for (name, mode) in [
            ("pyenv-hello", 0o755),
            ("pyenv-sh-hi", 0o755),
            ("pyenv-plain", 0o644),
        ] {
            fs::write(bin.join(name), "#!/bin/sh\n").unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(mode)).unwrap();
        }
        let path = dispatch_path(
            std::slice::from_ref(&bin),
            Some(OsStr::new("/nonexistent")),
            Flavor::Pyenv,
        );
        assert_eq!(
            find("hello", &path, Flavor::Pyenv, None),
            Some(bin.join("pyenv-hello"))
        );
        // No executable `pyenv-plain`: bash's `command -v` falls back to the file anyway.
        assert_eq!(
            find("plain", &path, Flavor::Pyenv, None),
            Some(bin.join("pyenv-plain"))
        );
        let mut names = listed_names(&path, Flavor::Pyenv, None);
        names.sort();
        assert_eq!(names, ["hello", "plain", "sh-hi"]);
    }
}
