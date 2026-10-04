//! The versions under `PYENV_ROOT/versions`, as `pyenv versions` lists them.

use crate::flavor::Flavor;
use crate::vsort::sort_version_names;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionEntry {
    /// Name as `pyenv versions` prints it: `3.12.9` or `3.12.9/envs/foo`.
    pub name: String,
    pub path: PathBuf,
    /// Raw symlink target, one level, when the entry is a symlink.
    pub link: Option<PathBuf>,
    /// A top-level symlink to another version or to an env (`--skip-aliases`).
    pub alias: bool,
}

/// Get subdirectory names in a directory, handling missing directories gracefully.
fn subdirs(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// `versions/.tmp-<name>` and `versions/.old-<name>`: the installer's staging (plan M2a, Decision 4);
/// `versions\.del-<name>-<pid>`: a version pyenv-win's `uninstall` renamed away before deleting it.
pub fn is_staging_name(name: &str) -> bool {
    name.starts_with(".tmp-") || name.starts_with(".old-") || name.starts_with(".del-")
}

/// Top-level entries of `versions/` that are directories (following links),
/// in the order the flavor lists them. Envs are not included.
pub fn top_level(versions_dir: &Path, flavor: Flavor) -> Vec<VersionEntry> {
    let mut names = subdirs(versions_dir);
    // The installer's staging names, for both flavors (plan M2a Decision 4, M2b Decision 9).
    // Upstream lists other dot entries too, so only these are hidden (allowlist row, Task 10).
    names.retain(|n| !is_staging_name(n));
    if flavor == Flavor::Pyenv {
        sort_version_names(&mut names, &versions_dir.to_string_lossy());
    }
    names
        .into_iter()
        .map(|name| {
            let path = versions_dir.join(&name);
            let link = std::fs::read_link(&path).ok();
            let alias = link.is_some() && is_alias(versions_dir, &path);
            VersionEntry {
                name,
                path,
                link,
                alias,
            }
        })
        .collect()
}

/// Upstream `--skip-aliases`: a symlink into `versions/` or to `versions/<v>/envs/<e>`.
fn is_alias(versions_dir: &Path, path: &Path) -> bool {
    let (Ok(target), Ok(vdir)) = (
        std::fs::canonicalize(path),
        std::fs::canonicalize(versions_dir),
    ) else {
        return false;
    };
    let parent = target.parent();
    if parent == Some(vdir.as_path()) {
        return true;
    }
    parent
        .and_then(Path::file_name)
        .is_some_and(|n| n == "envs")
        && parent.and_then(Path::parent).and_then(Path::parent) == Some(vdir.as_path())
}

/// `<entry>/envs/*` directories, in byte order (allowlist D-07).
pub fn envs_of(entry: &VersionEntry) -> Vec<VersionEntry> {
    let envs = entry.path.join("envs");
    let mut names = subdirs(&envs);
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let path = envs.join(&n);
            VersionEntry {
                name: format!("{}/envs/{n}", entry.name),
                link: std::fs::read_link(&path).ok(),
                path,
                alias: false,
            }
        })
        .collect()
}

/// Names only, as `pyenv versions --bare --skip-envs` lists them.
pub fn names(versions_dir: &Path, flavor: Flavor) -> Vec<String> {
    top_level(versions_dir, flavor)
        .into_iter()
        .map(|e| e.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn names_of(entries: &[VersionEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn only_directories_in_version_order() {
        // Review focus 4: a plain file and an empty folder under versions/.
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for d in ["3.12.10", "3.12.9", ".venv", "empty"] {
            fs::create_dir_all(v.join(d)).unwrap();
        }
        fs::write(v.join("plainfile"), "").unwrap();
        assert_eq!(
            names_of(&top_level(&v, Flavor::Pyenv)),
            [".venv", "3.12.9", "3.12.10", "empty"]
        );
    }

    #[test]
    fn staging_names_are_hidden_for_pyenv_win_too() {
        let d = std::env::temp_dir().join(format!(
            "rpyenv-installed-{}-staging_win",
            std::process::id()
        ));
        for n in [
            "3.12.1",
            ".tmp-3.12.2",
            ".old-3.12.1",
            ".del-3.12.3-77",
            ".hidden",
        ] {
            fs::create_dir_all(d.join(n)).unwrap();
        }
        let got = names(&d, Flavor::PyenvWin);
        assert!(got.contains(&"3.12.1".to_string()) && got.contains(&".hidden".to_string()));
        assert!(!got
            .iter()
            .any(|n| n.starts_with(".tmp-") || n.starts_with(".old-") || n.starts_with(".del-")));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_versions_dir_is_empty() {
        assert!(top_level(Path::new("/no/such/versions"), Flavor::Pyenv).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_aliases_and_dangling_links() {
        // Review focus 4: a dangling symlink is skipped, not an error.
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        let external = tmp.path().join("external");
        fs::create_dir_all(v.join("3.12.10")).unwrap();
        fs::create_dir_all(v.join("3.12.9").join("envs").join("alpha")).unwrap();
        fs::create_dir_all(&external).unwrap();
        symlink("3.12.10", v.join("3.12")).unwrap();
        symlink("3.12.9/envs/alpha", v.join("alpha")).unwrap();
        symlink(&external, v.join("ext")).unwrap();
        symlink("/nonexistent", v.join("dangling")).unwrap();
        let e = top_level(&v, Flavor::Pyenv);
        assert_eq!(names_of(&e), ["3.12", "3.12.9", "3.12.10", "alpha", "ext"]);
        let by = |n: &str| e.iter().find(|x| x.name == n).unwrap().clone();
        assert!(by("3.12").alias);
        assert_eq!(by("3.12").link, Some(PathBuf::from("3.12.10")));
        assert!(by("alpha").alias);
        assert!(!by("ext").alias);
        assert!(!by("3.12.9").alias);
    }

    #[test]
    fn envs_are_named_after_their_parent_in_byte_order() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for env in ["zeta", "alpha", "Beta"] {
            fs::create_dir_all(v.join("3.12.9").join("envs").join(env)).unwrap();
        }
        let parent = top_level(&v, Flavor::Pyenv).remove(0);
        assert_eq!(
            names_of(&envs_of(&parent)),
            ["3.12.9/envs/Beta", "3.12.9/envs/alpha", "3.12.9/envs/zeta"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn pyenv_win_keeps_directory_order() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path().join("versions");
        for d in ["3.9.1", "3.10.1", "3.8.2"] {
            fs::create_dir_all(v.join(d)).unwrap();
        }
        // NTFS returns names in case-insensitive name order, not version order.
        assert_eq!(names(&v, Flavor::PyenvWin), ["3.10.1", "3.8.2", "3.9.1"]);
    }
}
