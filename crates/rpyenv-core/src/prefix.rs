//! Upstream `pyenv-prefix`, and pyenv-win's `GetBinDir`.

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, latest, pathsearch, winresolve};
use std::path::PathBuf;

/// The installation directory of one version name, or upstream's message.
pub fn prefix_of(ctx: &Ctx, version: &str) -> Result<PathBuf, String> {
    if ctx.flavor == Flavor::Pyenv && version == "system" {
        let Some(python) = system_python(ctx) else {
            return Err("pyenv: system version not found in PATH".to_string());
        };
        return match pathsearch::strip_bin(&python) {
            Some(p) if p.is_dir() => Ok(p),
            _ => Err("pyenv: version `system' not installed".to_string()),
        };
    }
    let names = installed::names(&ctx.versions_dir(), ctx.flavor);
    let resolved = match ctx.flavor {
        Flavor::Pyenv => latest::latest(version, &names, &ctx.versions_dir())
            .unwrap_or_else(|| version.to_string()),
        Flavor::PyenvWin => winresolve::resolve(version, &names, ctx.arch_suffix),
    };
    let dir = ctx.versions_dir().join(&resolved);
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(format!("pyenv: version `{resolved}' not installed"))
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
            Err("pyenv: version `9.9' not installed".to_string())
        );
    }

    #[test]
    fn pyenv_system_without_python_on_path() {
        let tmp = root_with(&[]);
        let ctx = Ctx::for_test(Flavor::Pyenv, tmp.path(), tmp.path());
        assert_eq!(
            prefix_of(&ctx, "system"),
            Err("pyenv: system version not found in PATH".to_string())
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
            Err("pyenv: version `system' not installed".to_string())
        );
    }
}
