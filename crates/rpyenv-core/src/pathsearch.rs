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

/// cmd.exe's search for a command word. A name with `\` or `/` in it is run from `cwd`
/// (or as given, when absolute), without searching `PATH`. Otherwise each folder of the
/// `;`-separated `path` is searched: the name as typed when it has an extension, then the
/// name plus each `PATHEXT` extension. Quotes and empty entries are dropped.
pub fn find_cmd(name: &str, path: &OsStr, pathext: Option<&OsStr>, cwd: &Path) -> Option<PathBuf> {
    let exts = pathext_list(pathext);
    let typed_ext = Path::new(name).extension().is_some();
    let try_dir = |dir: &Path| -> Option<PathBuf> {
        if typed_ext && dir.join(name).is_file() {
            return Some(dir.join(name));
        }
        exts.iter()
            .map(|e| dir.join(format!("{name}{e}")))
            .find(|c| c.is_file())
    };
    if name.contains(['\\', '/']) {
        return try_dir(cwd);
    }
    let path = path.to_string_lossy();
    path.split(';')
        .map(|d| d.replace('"', ""))
        .filter(|d| !d.is_empty())
        .find_map(|d| try_dir(Path::new(&d)))
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

/// The lookup's test: a regular file (following symlinks) that the caller may run. On Unix
/// that is `faccessat(X_OK, AT_EACCESS)`, the effective IDs, which is what upstream's
/// `[ -x ]` checks (allowlist D-29). A path with a NUL can't be.
#[cfg(unix)]
pub fn is_runnable(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    if !p.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return false;
    }
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call; `faccessat`
    // only reads it, and `AT_FDCWD` makes a relative path resolve against the current
    // folder, as `access` would.
    unsafe { libc::faccessat(libc::AT_FDCWD, c.as_ptr(), libc::X_OK, libc::AT_EACCESS) == 0 }
}

#[cfg(not(unix))]
pub fn is_runnable(p: &Path) -> bool {
    p.is_file()
}

/// The shim listing's test: a regular file (following symlinks) with any execute bit, for
/// anyone. It differs from [`is_runnable`] on purpose: upstream's listing is a plain glob
/// with no `-x`, and the shim set must not depend on who rehashed last (allowlist D-33).
#[cfg(unix)]
pub fn has_exec_bit(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn has_exec_bit(p: &Path) -> bool {
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
        assert_eq!(
            find_cmd("tool", &path, exts, tmp.path()),
            Some(a.join("tool.cmd"))
        );
        assert_eq!(
            find_cmd("tool.exe", &path, exts, tmp.path()),
            Some(b.join("tool.exe"))
        );
        assert_eq!(
            find_cmd("x.py", &path, None, tmp.path()),
            Some(b.join("x.py"))
        );
        assert_eq!(find_cmd("x", &path, None, tmp.path()), None);
    }

    /// A file only group and others may run isn't runnable for its owner, as with `[ -x ]`
    /// (allowlist D-29). Root may run it, as `-x` says too, so the check is skipped there.
    #[cfg(unix)]
    #[test]
    fn a_file_the_caller_may_not_run_is_not_runnable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = make_exe(tmp.path(), "locked");
        assert!(is_runnable(&p));
        fs::set_permissions(&p, fs::Permissions::from_mode(0o011)).unwrap();
        if fs::read(&p).is_ok() {
            eprintln!("running as root: skipping a_file_the_caller_may_not_run_is_not_runnable");
            return;
        }
        assert!(!is_runnable(&p));
    }

    /// cmd.exe would run a `tool.exe` from the current folder before searching PATH; rpyenv
    /// searches only the child's PATH (allowlist D-40).
    #[test]
    fn cmd_search_skips_the_current_folder() {
        let tmp = tempfile::tempdir().unwrap();
        make_exe(tmp.path(), "tool.exe");
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let path = OsString::from(elsewhere.display().to_string());
        let exts = Some(OsStr::new(".EXE"));
        assert_eq!(find_cmd("tool", &path, exts, tmp.path()), None);
        assert_eq!(find_cmd("tool.exe", &path, exts, tmp.path()), None);
    }

    #[cfg(windows)]
    #[test]
    fn cmd_search_runs_a_name_with_a_folder_from_the_current_folder() {
        let tmp = tempfile::tempdir().unwrap();
        make_exe(&tmp.path().join("sub"), "tool.exe");
        let elsewhere = tmp.path().join("elsewhere");
        make_exe(&elsewhere, "tool.exe");
        let path = OsString::from(elsewhere.display().to_string());
        assert_eq!(
            find_cmd("sub\\tool", &path, Some(OsStr::new(".exe")), tmp.path()),
            Some(tmp.path().join("sub\\tool.exe"))
        );
        assert_eq!(
            find_cmd("sub/tool.exe", &path, None, tmp.path()),
            Some(tmp.path().join("sub/tool.exe"))
        );
    }
}
