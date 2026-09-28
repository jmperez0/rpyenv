//! Everything version resolution reads from the process environment.

use crate::flavor::Flavor;
use crate::paths::lexical_normalize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Everything version resolution reads from the environment, gathered once
/// so the rules can be tested with made-up values.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub flavor: Flavor,
    /// `PYENV_ROOT` after normalization.
    pub root: PathBuf,
    /// Upstream's `PYENV_DIR`: where the `.python-version` search starts.
    pub dir: PathBuf,
    /// The current directory: upstream's logical `$PWD` on Linux.
    pub pwd: PathBuf,
    /// `PYENV_VERSION` when set and non-empty.
    pub pyenv_version: Option<String>,
    pub path: Option<OsString>,
    pub pathext: Option<OsString>,
    /// pyenv-win's architecture suffix for installed version names: "", "-win32" or "-arm64".
    pub arch_suffix: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CtxError {
    /// `PYENV_DIR` is not a directory. Holds the value as given.
    BadPyenvDir(String),
    NoCurrentDir(String),
}

impl CtxError {
    pub fn message(&self) -> String {
        match self {
            CtxError::BadPyenvDir(d) => format!("pyenv: cannot change working directory to `{d}'"),
            CtxError::NoCurrentDir(e) => {
                format!("pyenv: cannot determine the current directory: {e}")
            }
        }
    }
}

impl Ctx {
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn shims_dir(&self) -> PathBuf {
        self.root.join("shims")
    }

    pub fn global_version_file(&self) -> PathBuf {
        self.root.join("version")
    }

    pub fn from_process() -> Result<Ctx, CtxError> {
        let cwd = std::env::current_dir().map_err(|e| CtxError::NoCurrentDir(e.to_string()))?;
        let get = |k: &str| std::env::var_os(k);
        Ctx::build(Flavor::current(), &get, cwd, host_arch_suffix())
    }

    /// Builds a context from an environment lookup. `cwd` is the physical current directory.
    pub fn build(
        flavor: Flavor,
        get: &dyn Fn(&str) -> Option<OsString>,
        cwd: PathBuf,
        host_arch: &'static str,
    ) -> Result<Ctx, CtxError> {
        let non_empty = |k: &str| get(k).filter(|v| !v.is_empty());
        let root = discover_root(flavor, &non_empty);
        let pwd = logical_pwd(flavor, get("PWD"), cwd);
        let dir = match (flavor, non_empty("PYENV_DIR")) {
            (Flavor::Pyenv, Some(d)) => {
                let joined = pwd.join(&d);
                if !joined.is_dir() {
                    return Err(CtxError::BadPyenvDir(d.to_string_lossy().into_owned()));
                }
                lexical_normalize(&joined)
            }
            _ => pwd.clone(),
        };
        let arch_suffix =
            match non_empty("PYENV_FORCE_ARCH").map(|v| v.to_string_lossy().to_ascii_uppercase()) {
                Some(a) if a == "X86" => "-win32",
                Some(a) if a == "ARM64" => "-arm64",
                // pyenv-win maps AMD64 and any unrecognized value to no suffix.
                Some(_) => "",
                None => host_arch,
            };
        Ok(Ctx {
            flavor,
            root,
            dir,
            pwd,
            pyenv_version: non_empty("PYENV_VERSION").map(|v| v.to_string_lossy().into_owned()),
            path: get("PATH"),
            pathext: get("PATHEXT"),
            arch_suffix,
        })
    }

    /// A context for tests: no `PYENV_VERSION`, no `PATH`, no arch suffix.
    pub fn for_test(flavor: Flavor, root: &Path, pwd: &Path) -> Ctx {
        Ctx {
            flavor,
            root: root.to_path_buf(),
            dir: pwd.to_path_buf(),
            pwd: pwd.to_path_buf(),
            pyenv_version: None,
            path: None,
            pathext: None,
            arch_suffix: "",
        }
    }
}

fn discover_root(flavor: Flavor, get: &dyn Fn(&str) -> Option<OsString>) -> PathBuf {
    let val = |k: &str| get(k).map(|v| v.to_string_lossy().into_owned());
    match flavor {
        // Upstream removes exactly one trailing slash (libexec/pyenv:58-63).
        Flavor::Pyenv => match val("PYENV_ROOT") {
            Some(r) => PathBuf::from(r.strip_suffix('/').unwrap_or(&r)),
            None => PathBuf::from(format!("{}/.pyenv", val("HOME").unwrap_or_default())),
        },
        Flavor::PyenvWin => match ["PYENV_ROOT", "PYENV", "PYENV_HOME"]
            .iter()
            .find_map(|k| val(k))
        {
            Some(r) => PathBuf::from(trim_trailing_separators(&r)),
            None => PathBuf::from(format!(
                "{}\\.pyenv\\pyenv-win",
                val("USERPROFILE").unwrap_or_default()
            )),
        },
    }
}

/// `C:\a\b\` becomes `C:\a\b`; `C:\` and `\` are kept (review focus 1).
fn trim_trailing_separators(s: &str) -> String {
    let mut t = s;
    while t.len() > 1 && (t.ends_with('\\') || t.ends_with('/')) {
        let shorter = &t[..t.len() - 1];
        if shorter.ends_with(':') {
            break;
        }
        t = shorter;
    }
    t.to_string()
}

/// Upstream uses bash's logical `$PWD`. Trust `PWD` when it names the same directory as `cwd`.
fn logical_pwd(flavor: Flavor, pwd_env: Option<OsString>, cwd: PathBuf) -> PathBuf {
    if flavor == Flavor::Pyenv {
        if let Some(p) = pwd_env.map(PathBuf::from) {
            if p.is_absolute() && same_dir(&p, &cwd) {
                return p;
            }
        }
    }
    cwd
}

#[cfg(unix)]
fn same_dir(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
}

fn host_arch_suffix() -> &'static str {
    if cfg!(target_arch = "x86") {
        "-win32"
    } else if cfg!(target_arch = "aarch64") {
        "-arm64"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavor::Flavor::{Pyenv, PyenvWin};

    fn build(flavor: Flavor, pairs: &[(&str, &str)]) -> Result<Ctx, CtxError> {
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(*v))
        };
        Ctx::build(flavor, &get, std::env::current_dir().unwrap(), "")
    }

    #[test]
    fn pyenv_root_defaults_to_home() {
        let ctx = build(Pyenv, &[("HOME", "/home/u")]).unwrap();
        assert_eq!(ctx.root, PathBuf::from("/home/u/.pyenv"));
        let ctx = build(Pyenv, &[("HOME", "/home/u"), ("PYENV_ROOT", "")]).unwrap();
        assert_eq!(ctx.root, PathBuf::from("/home/u/.pyenv"));
    }

    #[test]
    fn pyenv_root_loses_exactly_one_trailing_slash() {
        assert_eq!(
            build(Pyenv, &[("PYENV_ROOT", "/tmp/x/")]).unwrap().root,
            PathBuf::from("/tmp/x")
        );
        assert_eq!(
            build(Pyenv, &[("PYENV_ROOT", "/tmp/x//")]).unwrap().root,
            PathBuf::from("/tmp/x/")
        );
    }

    #[test]
    fn pyenv_win_root_trailing_backslash_is_removed() {
        let ctx = build(
            PyenvWin,
            &[("PYENV_ROOT", "C:\\Users\\JM\\.pyenv\\pyenv-win\\")],
        )
        .unwrap();
        assert_eq!(ctx.root, PathBuf::from("C:\\Users\\JM\\.pyenv\\pyenv-win"));
        assert_eq!(
            build(PyenvWin, &[("PYENV_ROOT", "C:\\")]).unwrap().root,
            PathBuf::from("C:\\")
        );
    }

    #[test]
    fn pyenv_win_root_falls_back_to_pyenv_then_pyenv_home_then_profile() {
        assert_eq!(
            build(PyenvWin, &[("PYENV", "D:\\p\\")]).unwrap().root,
            PathBuf::from("D:\\p")
        );
        assert_eq!(
            build(PyenvWin, &[("PYENV_HOME", "E:\\h")]).unwrap().root,
            PathBuf::from("E:\\h")
        );
        assert_eq!(
            build(PyenvWin, &[("USERPROFILE", "C:\\Users\\JM")])
                .unwrap()
                .root,
            PathBuf::from("C:\\Users\\JM\\.pyenv\\pyenv-win")
        );
    }

    #[test]
    fn bad_pyenv_dir_is_an_error() {
        let err = build(Pyenv, &[("PYENV_DIR", "/definitely/not/here")]).unwrap_err();
        assert_eq!(
            err.message(),
            "pyenv: cannot change working directory to `/definitely/not/here'"
        );
    }

    #[test]
    fn valid_pyenv_dir_is_used() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = build(Pyenv, &[("PYENV_DIR", tmp.path().to_str().unwrap())]).unwrap();
        assert_eq!(ctx.dir, tmp.path());
    }

    #[test]
    fn forced_architecture() {
        assert_eq!(
            build(PyenvWin, &[("PYENV_FORCE_ARCH", "X86")])
                .unwrap()
                .arch_suffix,
            "-win32"
        );
        assert_eq!(
            build(PyenvWin, &[("PYENV_FORCE_ARCH", "arm64")])
                .unwrap()
                .arch_suffix,
            "-arm64"
        );
        assert_eq!(
            build(PyenvWin, &[("PYENV_FORCE_ARCH", "AMD64")])
                .unwrap()
                .arch_suffix,
            ""
        );
        assert_eq!(
            build(PyenvWin, &[("PYENV_FORCE_ARCH", "bogus")])
                .unwrap()
                .arch_suffix,
            ""
        );
    }

    #[test]
    fn empty_pyenv_version_counts_as_unset() {
        assert_eq!(
            build(Pyenv, &[("PYENV_VERSION", "")])
                .unwrap()
                .pyenv_version,
            None
        );
        assert_eq!(
            build(Pyenv, &[("PYENV_VERSION", "3.12")])
                .unwrap()
                .pyenv_version
                .as_deref(),
            Some("3.12")
        );
    }
}
