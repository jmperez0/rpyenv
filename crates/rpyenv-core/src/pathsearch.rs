//! Finding a command on `PATH`, as `command -v` (pyenv) or `where` (pyenv-win) would.

use crate::flavor::Flavor;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Every match for `name` on `path`, in PATH order, skipping the directory `skip`.
/// On Windows each `PATHEXT` extension is tried (default `.COM;.EXE;.BAT;.CMD`).
pub fn find_all(
    name: &str,
    path: Option<&OsStr>,
    skip: Option<&Path>,
    flavor: Flavor,
    pathext: Option<&OsStr>,
) -> Vec<PathBuf> {
    let Some(path) = path else {
        return Vec::new();
    };
    let exts: Vec<String> = match flavor {
        Flavor::Pyenv => vec![String::new()],
        Flavor::PyenvWin => pathext_list(pathext),
    };
    let mut found = Vec::new();
    for dir in std::env::split_paths(path) {
        let dir = if dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            dir
        };
        if skip.is_some_and(|s| same_dir(&dir, s, flavor)) {
            continue;
        }
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if is_runnable(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

pub fn find_first(
    name: &str,
    path: Option<&OsStr>,
    skip: Option<&Path>,
    flavor: Flavor,
    pathext: Option<&OsStr>,
) -> Option<PathBuf> {
    find_all(name, path, skip, flavor, pathext)
        .into_iter()
        .next()
}

/// `PATHEXT`'s extensions, lowercased; `.COM;.EXE;.BAT;.CMD` when unset or empty.
fn pathext_list(pathext: Option<&OsStr>) -> Vec<String> {
    pathext
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// cmd.exe's search for a command word on a `;`-separated `PATH`: in each folder, the name
/// as typed when it has an extension, then the name plus each `PATHEXT` extension. Quotes
/// and empty entries are dropped.
pub fn find_cmd(name: &str, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    let exts = pathext_list(pathext);
    let typed_ext = Path::new(name).extension().is_some();
    let path = path.to_string_lossy();
    for dir in path
        .split(';')
        .map(|d| d.replace('"', ""))
        .filter(|d| !d.is_empty())
    {
        let dir = Path::new(&dir);
        if typed_ext && dir.join(name).is_file() {
            return Some(dir.join(name));
        }
        for e in &exts {
            let candidate = dir.join(format!("{name}{e}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn same_dir(a: &Path, b: &Path, flavor: Flavor) -> bool {
    let norm = |p: &Path| {
        let s = p
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string();
        if flavor == Flavor::PyenvWin {
            s.to_ascii_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

/// A regular file (following symlinks) that can be run: any execute bit on Unix.
#[cfg(unix)]
pub fn is_runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn is_runnable(p: &Path) -> bool {
    p.is_file()
}

/// Upstream `pyenv prefix system`: `${p%/bin/*}` then `${p%/sbin/*}`.
/// None when neither pattern matched.
pub fn strip_bin(python: &Path) -> Option<PathBuf> {
    let s = python.to_string_lossy();
    let after_bin = s.rfind("/bin/").map_or(&s[..], |i| &s[..i]);
    let after_sbin = after_bin
        .rfind("/sbin/")
        .map_or(after_bin, |i| &after_bin[..i]);
    if after_sbin.len() == s.len() {
        return None;
    }
    Some(PathBuf::from(if after_sbin.is_empty() {
        "/"
    } else {
        after_sbin
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;

    fn make_exe(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        fs::write(&p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    #[test]
    fn path_order_and_skipped_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b, shims) = (
            tmp.path().join("a"),
            tmp.path().join("b"),
            tmp.path().join("shims"),
        );
        let exe = if cfg!(windows) {
            "python.exe"
        } else {
            "python"
        };
        make_exe(&shims, exe);
        let in_b = make_exe(&b, exe);
        fs::create_dir_all(&a).unwrap();
        let path = std::env::join_paths([&shims, &a, &b]).unwrap();
        let flavor = Flavor::current();
        assert_eq!(
            find_first("python", Some(&path), Some(&shims), flavor, None),
            Some(in_b)
        );
        assert_eq!(find_all("python", Some(&path), None, flavor, None).len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn non_executable_files_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("python"), "").unwrap();
        let path = tmp.path().as_os_str();
        assert_eq!(
            find_first("python", Some(path), None, Flavor::Pyenv, None),
            None
        );
    }

    #[test]
    fn strip_bin_like_upstream() {
        assert_eq!(
            strip_bin(Path::new("/usr/bin/python")),
            Some(PathBuf::from("/usr"))
        );
        assert_eq!(
            strip_bin(Path::new("/bin/python")),
            Some(PathBuf::from("/"))
        );
        assert_eq!(
            strip_bin(Path::new("/opt/sbin/python3")),
            Some(PathBuf::from("/opt"))
        );
        assert_eq!(strip_bin(Path::new("/home/t/sysbin/python")), None);
    }

    #[test]
    fn cmd_search_tries_the_typed_name_then_pathext() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        make_exe(&a, "tool.cmd");
        make_exe(&b, "tool.exe");
        make_exe(&b, "x.py");
        let path = OsString::from(format!("{};\"{}\";", a.display(), b.display()));
        let exts = Some(OsStr::new(".EXE;.CMD"));
        assert_eq!(find_cmd("tool", &path, exts), Some(a.join("tool.cmd")));
        assert_eq!(find_cmd("tool.exe", &path, exts), Some(b.join("tool.exe")));
        assert_eq!(find_cmd("x.py", &path, None), Some(b.join("x.py")));
        assert_eq!(find_cmd("x", &path, None), None);
    }
}
