//! Upstream `pyenv-prefix`, and pyenv-win's `GetBinDir`.

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, latest, pathsearch, winresolve};
use std::path::PathBuf;

/// Why `prefix_of` failed. `message` gives upstream's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixError {
    /// `system` with no `python`, `python3` or `python2` on `PATH`.
    SystemNotFound,
    /// Not installed. Holds the name after prefix resolution, or `system` when the
    /// system Python is not in a `bin` or `sbin` folder.
    NotInstalled(String),
}

impl PrefixError {
    pub fn message(&self) -> String {
        match self {
            PrefixError::SystemNotFound => "pyenv: system version not found in PATH".to_string(),
            PrefixError::NotInstalled(v) => format!("pyenv: version `{v}' not installed"),
        }
    }
}

/// The installation directory of one version name.
pub fn prefix_of(ctx: &Ctx, version: &str) -> Result<PathBuf, PrefixError> {
    let vdir = ctx.versions_dir();
    if ctx.flavor == Flavor::Pyenv {
        if version == "system" {
            let python = system_python(ctx).ok_or(PrefixError::SystemNotFound)?;
            return match pathsearch::strip_bin(&python) {
                Some(p) if p.is_dir() => Ok(p),
                _ => Err(PrefixError::NotInstalled("system".to_string())),
            };
        }
        // `latest` returns an installed exact name unchanged, so the listing is read
        // only for prefixes. A shim calls this on every launch (review M-6).
        if vdir.join(version).is_dir() {
            return Ok(vdir.join(version));
        }
    }
    if ctx.flavor == Flavor::PyenvWin && version.contains(['/', '\\']) {
        // `<base>/envs/<name>`, printed with `\` (allowlist D-101); no `.` or `..` segment.
        let plain = version
            .split(['/', '\\'])
            .all(|s| !s.is_empty() && s != "." && s != "..");
        let dir = vdir.join(version.replace('/', "\\"));
        return if plain && dir.is_dir() {
            Ok(dir)
        } else {
            Err(PrefixError::NotInstalled(version.to_string()))
        };
    }
    let names = installed::names(&vdir, ctx.flavor);
    let resolved = match ctx.flavor {
        Flavor::Pyenv => {
            latest::latest(version, &names, &vdir).unwrap_or_else(|| version.to_string())
        }
        Flavor::PyenvWin => winresolve::resolve(version, &names, ctx.arch_suffix),
    };
    let dir = vdir.join(&resolved);
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(PrefixError::NotInstalled(resolved))
    }
}

/// The system Python upstream finds: `python`, `python3` or `python2` on PATH, shims excluded.
pub fn system_python(ctx: &Ctx) -> Option<PathBuf> {
    let shims = ctx.shims_dir();
    ["python", "python3", "python2"].iter().find_map(|n| {
        pathsearch::find_first(
            n,
            ctx.path.as_deref(),
            Some(&shims),
            ctx.flavor,
            ctx.pathext.as_deref(),
        )
    })
}

/// pyenv-win `GetBinDir(TryResolveVersion(name))`. Both arms carry the resolved name,
/// which pyenv-win's not-installed message prints.
pub fn win_bin_dir(ctx: &Ctx, name: &str) -> Result<String, String> {
    let names = installed::names(&ctx.versions_dir(), Flavor::PyenvWin);
    let resolved = winresolve::resolve(name, &names, ctx.arch_suffix);
    let valid_name = !resolved.is_empty()
        && resolved
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if valid_name && ctx.versions_dir().join(&resolved).is_dir() {
        Ok(resolved)
    } else {
        Err(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root_with(versions: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("versions")).unwrap();
        for v in versions {
            fs::create_dir_all(tmp.path().join("versions").join(v)).unwrap();
        }
        tmp
    }

    #[test]
    fn pyenv_prefix_resolves_prefixes() {
        let tmp = root_with(&["3.12.10", "3.12.9"]);
        let ctx = Ctx::for_test(Flavor::Pyenv, tmp.path(), tmp.path());
        assert_eq!(
            prefix_of(&ctx, "3.12"),
            Ok(tmp.path().join("versions").join("3.12.10"))
        );
        assert_eq!(
            prefix_of(&ctx, "9.9"),
            Err(PrefixError::NotInstalled("9.9".to_string()))
        );
    }

    #[test]
    fn pyenv_system_without_python_on_path() {
        let tmp = root_with(&[]);
        let ctx = Ctx::for_test(Flavor::Pyenv, tmp.path(), tmp.path());
        assert_eq!(prefix_of(&ctx, "system"), Err(PrefixError::SystemNotFound));
    }

    #[test]
    fn prefix_error_messages() {
        assert_eq!(
            PrefixError::SystemNotFound.message(),
            "pyenv: system version not found in PATH"
        );
        assert_eq!(
            PrefixError::NotInstalled("3.7".to_string()).message(),
            "pyenv: version `3.7' not installed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pyenv_system_prefix_strips_bin() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = root_with(&[]);
        let bin = tmp.path().join("sys").join("bin");
        fs::create_dir_all(&bin).unwrap();
        let py = bin.join("python3");
        fs::write(&py, "").unwrap();
        fs::set_permissions(&py, fs::Permissions::from_mode(0o755)).unwrap();
        let mut ctx = Ctx::for_test(Flavor::Pyenv, tmp.path(), tmp.path());
        ctx.path = Some(bin.clone().into_os_string());
        assert_eq!(prefix_of(&ctx, "system"), Ok(tmp.path().join("sys")));
        assert_eq!(system_python(&ctx), Some(py));
    }

    #[test]
    fn pyenv_win_bin_dir() {
        let tmp = root_with(&["3.9.1", "3.8.2"]);
        let ctx = Ctx::for_test(Flavor::PyenvWin, tmp.path(), tmp.path());
        assert_eq!(win_bin_dir(&ctx, "3.9"), Ok("3.9.1".to_string()));
        assert_eq!(win_bin_dir(&ctx, "3.7"), Err("3.7".to_string()));
        assert_eq!(win_bin_dir(&ctx, "3.9 1"), Err("3.9 1".to_string()));
    }

    #[test]
    fn pyenv_win_system_is_an_ordinary_name() {
        let tmp = root_with(&[]);
        let ctx = Ctx::for_test(Flavor::PyenvWin, tmp.path(), tmp.path());
        assert_eq!(
            prefix_of(&ctx, "system"),
            Err(PrefixError::NotInstalled("system".to_string()))
        );
    }
}
