//! Virtualenv facts shared by `which`, the virtualenv commands, `uninstall` and `sh-activate`
//! (spec §10; docs/parity/pyenv-virtualenv-m4-reference.md "virtualenv-prefix" and
//! "Hooks shipped in etc/pyenv.d").

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{pathsearch, prefix};
use std::path::{Path, PathBuf};

/// What an env's `pyvenv.cfg` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cfg {
    /// `home`: on Linux minus one trailing `/bin` (the base prefix); on Windows the base
    /// folder itself, minus a trailing `\`.
    pub home: Option<PathBuf>,
    /// `include-system-site-packages = true`, in any case (`grep -q -i`).
    pub system_site_packages: bool,
}

/// `key *= *value` at the start of `line`, leading spaces allowed (`sed -n '/^ *home *= */s///p'`).
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.trim_start_matches(' ');
    let rest = rest
        .get(..key.len())
        .filter(|k| k.eq_ignore_ascii_case(key))
        .and_then(|_| rest.get(key.len()..))?;
    let rest = rest.trim_start_matches(' ').strip_prefix('=')?;
    Some(rest.trim_start_matches(' ').trim_end_matches(['\r', ' ']))
}

pub fn read_cfg(env_dir: &Path, flavor: Flavor) -> Option<Cfg> {
    let bytes = std::fs::read(env_dir.join("pyvenv.cfg")).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut home = None;
    let mut ssp = false;
    for line in text.lines() {
        if home.is_none() {
            if let Some(h) = value_of(line, "home") {
                let h = match flavor {
                    Flavor::Pyenv => h.strip_suffix("/bin").unwrap_or(h),
                    Flavor::PyenvWin => h.trim_end_matches('\\'),
                };
                home = Some(PathBuf::from(h));
            }
        }
        if value_of(line, "include-system-site-packages")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        {
            ssp = true;
        }
    }
    Some(Cfg {
        home,
        system_site_packages: ssp,
    })
}

/// `versions/<name>`; on Windows `<base>/envs/<name>` is accepted with either separator
/// (allowlist D-101).
pub fn version_dir(ctx: &Ctx, name: &str) -> PathBuf {
    match ctx.flavor {
        Flavor::Pyenv => ctx.versions_dir().join(name),
        Flavor::PyenvWin => ctx.versions_dir().join(name.replace('/', "\\")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VenvError {
    NotVenv(String),
    NoPython(String),
    /// Core's `pyenv-prefix` message for a version that isn't installed.
    Prefix(String),
}

impl VenvError {
    pub fn message(&self) -> String {
        match self {
            VenvError::NotVenv(v) => {
                format!("pyenv-virtualenv: version `{v}' is not a virtualenv")
            }
            VenvError::NoPython(v) => {
                format!("pyenv-virtualenv: `python' not found in version `{v}'")
            }
            VenvError::Prefix(m) => m.clone(),
        }
    }
}

/// A conda install or env: `conda-meta/`, or an executable `bin/conda`.
pub fn is_conda(dir: &Path) -> bool {
    dir.join("conda-meta").is_dir() || pathsearch::is_runnable(&dir.join("bin").join("conda"))
}

/// The libdir an old virtualenv keeps `orig-prefix.txt` in: `Lib` (jython), `lib-python`
/// (pypy) or `lib`.
fn libdir(dir: &Path) -> Option<PathBuf> {
    ["Lib", "lib-python", "lib"]
        .iter()
        .map(|l| dir.join(l))
        .find(|p| p.is_dir())
}

/// The first `name` within `<libdir>/` to depth 2 (`find <libdir>/ -maxdepth 2`).
fn within(dir: &Path, name: &str) -> Option<PathBuf> {
    let lib = libdir(dir)?;
    if lib.join(name).is_file() {
        return Some(lib.join(name));
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(&lib)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    subs.sort();
    subs.into_iter().map(|s| s.join(name)).find(|p| p.is_file())
}

fn orig_prefix(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(within(dir, "orig-prefix.txt")?).ok()?;
    Some(PathBuf::from(text.trim_end_matches(['\n', '\r'])))
}

/// A venv (`pyvenv.cfg`) or an old virtualenv (`orig-prefix.txt`), never a conda install:
/// what `virtualenv-delete` may delete by its own name (final review I1).
pub fn is_virtualenv(dir: &Path, flavor: Flavor) -> bool {
    (read_cfg(dir, flavor).is_some() || orig_prefix(dir).is_some())
        && !dir.join("bin").join("conda").exists()
}

/// `pyenv virtualenv-prefix <name>` for one version (reference "virtualenv-prefix", steps 1-6).
pub fn base_prefix(ctx: &Ctx, name: &str) -> Result<PathBuf, VenvError> {
    let not_venv = || VenvError::NotVenv(name.to_string());
    if name == "system" {
        return Err(not_venv());
    }
    let mut p = version_dir(ctx, name);
    if !p.is_dir() {
        p = prefix::prefix_of(ctx, name).map_err(|e| VenvError::Prefix(e.message()))?;
    }
    let found = match ctx.flavor {
        Flavor::Pyenv => {
            if !pathsearch::is_runnable(&p.join("bin").join("python")) {
                return Err(VenvError::NoPython(name.to_string()));
            }
            if p.join("bin").join("activate").is_file() {
                if p.join("bin").join("conda").is_file() {
                    Some(p.clone())
                } else if let Some(cfg) = read_cfg(&version_dir(ctx, name), Flavor::Pyenv) {
                    cfg.home
                } else {
                    orig_prefix(&p)
                }
            } else if p.join("conda-meta").is_dir() {
                std::fs::canonicalize(p.join("..").join("..")).ok()
            } else {
                None
            }
        }
        Flavor::PyenvWin => {
            if !(p.join("python.exe").is_file() || p.join("Scripts").join("python.exe").is_file()) {
                return Err(VenvError::NoPython(name.to_string()));
            }
            read_cfg(&p, Flavor::PyenvWin).and_then(|c| c.home)
        }
    };
    found.filter(|d| d.is_dir()).ok_or_else(not_venv)
}

/// The command fallbacks of spec §10 for one selected Linux version whose own `bin` lacks
/// `command` (allowlist D-95: always a file, never a folder):
/// - `conda` in a conda env: the owning install's `bin/conda`;
/// - in a venv or an old virtualenv (not a conda one), the base's `bin/<command>` when the
///   env includes system site packages, or when `command` is `python*-config`.
pub fn fallback(ctx: &Ctx, version: &str, dir: &Path, command: &str) -> Option<PathBuf> {
    let file = |p: PathBuf| (p.is_file() && pathsearch::is_runnable(&p)).then_some(p);
    if command == "conda" && dir.join("conda-meta").is_dir() {
        return base_prefix(ctx, version)
            .ok()
            .and_then(|b| file(b.join("bin").join("conda")));
    }
    if !dir.join("bin").join("activate").is_file() || dir.join("bin").join("conda").is_file() {
        return None;
    }
    let (base, ssp) = match read_cfg(dir, Flavor::Pyenv) {
        Some(c) => (c.home?, c.system_site_packages),
        None => (
            orig_prefix(dir)?,
            within(dir, "no-global-site-packages.txt").is_none(),
        ),
    };
    let config = command.starts_with("python") && command.ends_with("-config");
    (ssp || config)
        .then(|| file(base.join("bin").join(command)))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> (tempfile::TempDir, Ctx) {
        let t = tempfile::tempdir().unwrap();
        let ctx = Ctx::for_test(Flavor::Pyenv, t.path(), t.path());
        (t, ctx)
    }

    #[cfg(unix)]
    fn exe(p: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "#!/bin/sh\n").unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// `versions/<base>/envs/<name>`, a venv of `versions/<base>` (reference "virtualenv-prefix").
    #[cfg(unix)]
    fn env(ctx: &Ctx, base: &str, name: &str, cfg: &str) -> PathBuf {
        let b = ctx.versions_dir().join(base);
        exe(&b.join("bin/python"));
        let e = b.join("envs").join(name);
        exe(&e.join("bin/python"));
        fs::write(e.join("bin/activate"), "").unwrap();
        fs::write(
            e.join("pyvenv.cfg"),
            cfg.replace("{base}", &b.display().to_string()),
        )
        .unwrap();
        std::os::unix::fs::symlink(&e, ctx.versions_dir().join(name)).unwrap();
        e
    }

    #[test]
    fn cfg_home_loses_one_bin_on_linux_and_reads_the_flag_in_any_case() {
        let t = tempfile::tempdir().unwrap();
        fs::write(
            t.path().join("pyvenv.cfg"),
            "  home = /opt/py/bin\nInclude-System-Site-Packages = TRUE\n",
        )
        .unwrap();
        assert_eq!(
            read_cfg(t.path(), Flavor::Pyenv),
            Some(Cfg {
                home: Some(PathBuf::from("/opt/py")),
                system_site_packages: true
            })
        );
        fs::write(
            t.path().join("pyvenv.cfg"),
            "home = C:\\py\\3.13\\\r\ninclude-system-site-packages = false\r\n",
        )
        .unwrap();
        assert_eq!(
            read_cfg(t.path(), Flavor::PyenvWin),
            Some(Cfg {
                home: Some(PathBuf::from("C:\\py\\3.13")),
                system_site_packages: false
            })
        );
        let _ = root();
    }

    #[cfg(unix)]
    #[test]
    fn base_prefix_as_virtualenv_prefix() {
        let (_t, ctx) = root();
        fs::create_dir_all(ctx.versions_dir()).unwrap();
        env(&ctx, "3.12.1", "venv1", "home = {base}/bin\n");
        let base = ctx.versions_dir().join("3.12.1");
        assert_eq!(base_prefix(&ctx, "venv1"), Ok(base.clone()));
        assert_eq!(base_prefix(&ctx, "3.12.1/envs/venv1"), Ok(base));
        assert_eq!(
            base_prefix(&ctx, "system").unwrap_err().message(),
            "pyenv-virtualenv: version `system' is not a virtualenv"
        );
        assert_eq!(
            base_prefix(&ctx, "3.12.1").unwrap_err().message(),
            "pyenv-virtualenv: version `3.12.1' is not a virtualenv"
        );
        fs::create_dir_all(ctx.versions_dir().join("nopy")).unwrap();
        assert_eq!(
            base_prefix(&ctx, "nopy").unwrap_err().message(),
            "pyenv-virtualenv: `python' not found in version `nopy'"
        );
        assert_eq!(
            base_prefix(&ctx, "nosuch").unwrap_err().message(),
            "pyenv: version `nosuch' not installed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn conda_base_conda_env_and_old_virtualenv() {
        let (_t, ctx) = root();
        let conda = ctx.versions_dir().join("miniconda3-4.7.12");
        exe(&conda.join("bin/python"));
        exe(&conda.join("bin/conda"));
        fs::write(conda.join("bin/activate"), "").unwrap();
        let cenv = conda.join("envs/cenv");
        exe(&cenv.join("bin/python"));
        fs::create_dir_all(cenv.join("conda-meta")).unwrap();
        assert_eq!(base_prefix(&ctx, "miniconda3-4.7.12"), Ok(conda.clone()));
        assert_eq!(
            base_prefix(&ctx, "miniconda3-4.7.12/envs/cenv"),
            Ok(fs::canonicalize(&conda).unwrap())
        );
        let old = ctx.versions_dir().join("oldvenv");
        exe(&old.join("bin/python"));
        fs::write(old.join("bin/activate"), "").unwrap();
        fs::create_dir_all(old.join("lib/python2.7")).unwrap();
        fs::write(
            old.join("lib/python2.7/orig-prefix.txt"),
            conda.display().to_string(),
        )
        .unwrap();
        assert_eq!(base_prefix(&ctx, "oldvenv"), Ok(conda));
    }

    /// allowlist D-95: the fallbacks give a file, never a folder.
    #[cfg(unix)]
    #[test]
    fn fallbacks_follow_spec_10() {
        let (_t, ctx) = root();
        fs::create_dir_all(ctx.versions_dir()).unwrap();
        let base = ctx.versions_dir().join("3.12.1");
        let ssp = env(
            &ctx,
            "3.12.1",
            "ssp",
            "home = {base}/bin\ninclude-system-site-packages = true\n",
        );
        let plain = env(&ctx, "3.12.1", "plain", "home = {base}/bin\n");
        exe(&base.join("bin/basetool"));
        exe(&base.join("bin/python3.12-config"));
        fs::create_dir_all(base.join("bin/adir")).unwrap();
        assert_eq!(
            fallback(&ctx, "ssp", &ssp, "basetool"),
            Some(base.join("bin/basetool"))
        );
        assert_eq!(fallback(&ctx, "plain", &plain, "basetool"), None);
        assert_eq!(
            fallback(&ctx, "plain", &plain, "python3.12-config"),
            Some(base.join("bin/python3.12-config"))
        );
        assert_eq!(fallback(&ctx, "ssp", &ssp, "adir"), None);
        assert_eq!(fallback(&ctx, "ssp", &ssp, "nosuch"), None);
    }
}
