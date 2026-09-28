//! Which Python versions are selected: upstream `version-name`, `version-origin` and
//! `version-file`, and pyenv-win's `GetCurrentVersionsNoError`.

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, latest, verfile, winresolve};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyenvNames {
    /// Accepted names, in order. Upstream prints them joined with `:` even when some failed.
    pub names: Vec<String>,
    /// Lines for stderr: `invalid version` warnings and `is not installed` messages.
    pub stderr: Vec<String>,
    pub failed: bool,
}

/// Upstream `pyenv version-file`: the local file from `PYENV_DIR`, then from `$PWD`,
/// else `<root>/version` whether or not it exists.
pub fn version_file(ctx: &Ctx) -> PathBuf {
    verfile::find_local(&ctx.dir)
        .or_else(|| {
            (ctx.dir != ctx.pwd)
                .then(|| verfile::find_local(&ctx.pwd))
                .flatten()
        })
        .unwrap_or_else(|| ctx.global_version_file())
}

/// Upstream `pyenv version-origin`.
pub fn version_origin(ctx: &Ctx) -> String {
    if ctx.pyenv_version.is_some() {
        "PYENV_VERSION environment variable".to_string()
    } else {
        version_file(ctx).display().to_string()
    }
}

/// Splits on `:` the way bash does with `IFS=:`: a trailing empty field is dropped.
pub fn split_colon(s: &str) -> Vec<String> {
    let mut parts: Vec<String> = s.split(':').map(String::from).collect();
    if parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}

/// Upstream `pyenv version-name [-f]`.
pub fn version_name(ctx: &Ctx, force: bool) -> PyenvNames {
    let mut stderr = Vec::new();
    let raw = match &ctx.pyenv_version {
        Some(v) => v.clone(),
        None => {
            let file = version_file(ctx);
            let r = verfile::read_pyenv(&file, &file.display().to_string(), &ctx.versions_dir());
            stderr.extend(r.warnings);
            r.versions.join(":")
        }
    };
    if raw.is_empty() || raw == "system" {
        return PyenvNames {
            names: vec!["system".to_string()],
            stderr,
            failed: false,
        };
    }
    let origin = version_origin(ctx);
    let vdir = ctx.versions_dir();
    let candidates = installed::names(&vdir, Flavor::Pyenv);
    let (mut names, mut failed, mut normalization_done) = (Vec::new(), false, false);
    for v in split_colon(&raw) {
        let normalised = match v.strip_prefix("python-") {
            Some(n) => {
                normalization_done = true;
                n.to_string()
            }
            None => v.clone(),
        };
        let accepted = if v == "system" || vdir.join(&normalised).is_dir() {
            Some(normalised.clone())
        } else if normalization_done && vdir.join(&v).is_dir() {
            Some(v.clone())
        } else if let Some(r) = latest::latest(&normalised, &candidates, &vdir) {
            Some(r)
        } else if normalization_done {
            latest::latest(&v, &candidates, &vdir)
        } else {
            None
        };
        match accepted.or_else(|| force.then(|| normalised.clone())) {
            Some(n) => names.push(n),
            None => {
                stderr.push(format!(
                    "pyenv: version `{v}' is not installed (set by {origin})"
                ));
                failed = true;
            }
        }
    }
    PyenvNames {
        names,
        stderr,
        failed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinSelected {
    pub name: String,
    /// `%PYENV_VERSION%`, or the full path of the file the name came from.
    pub origin: String,
}

/// pyenv-win's selecting file: the nearest `.python-version` found upward from `ctx.pwd`
/// if `verfile::read_pyenv_win` gives it a non-empty list, else the global version file
/// (whether or not it exists).
pub fn win_version_file(ctx: &Ctx) -> PathBuf {
    if let Some(f) = verfile::find_local(&ctx.pwd) {
        if !verfile::read_pyenv_win(&f).is_empty() {
            return f;
        }
    }
    ctx.global_version_file()
}

/// pyenv-win `GetCurrentVersionsNoError`: ordered, de-duplicated, empty when nothing is set.
pub fn win_select(ctx: &Ctx) -> Vec<WinSelected> {
    let mut out: Vec<WinSelected> = Vec::new();
    let mut push = |name: String, origin: &str| {
        if !out.iter().any(|s| s.name == name) {
            out.push(WinSelected {
                name,
                origin: origin.to_string(),
            });
        }
    };
    if let Some(v) = &ctx.pyenv_version {
        for name in v.split(' ') {
            push(name.to_string(), "%PYENV_VERSION%");
        }
        return out;
    }
    let installed = installed::names(&ctx.versions_dir(), Flavor::PyenvWin);
    let file = win_version_file(ctx);
    let lines = verfile::read_pyenv_win(&file);
    let origin = file.display().to_string();
    for line in lines {
        push(
            winresolve::resolve(&line, &installed, ctx.arch_suffix),
            &origin,
        );
    }
    out
}

/// The origin pyenv-win would show for the current selection.
pub fn win_origin(ctx: &Ctx) -> String {
    if ctx.pyenv_version.is_some() {
        return "%PYENV_VERSION%".to_string();
    }
    win_version_file(ctx).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Setup {
        tmp: tempfile::TempDir,
    }

    impl Setup {
        fn new(versions: &[&str]) -> Setup {
            let tmp = tempfile::tempdir().unwrap();
            fs::create_dir_all(tmp.path().join("root").join("versions")).unwrap();
            for v in versions {
                fs::create_dir_all(tmp.path().join("root").join("versions").join(v)).unwrap();
            }
            fs::create_dir_all(tmp.path().join("work").join("sub")).unwrap();
            Setup { tmp }
        }
        fn root(&self) -> PathBuf {
            self.tmp.path().join("root")
        }
        fn work(&self) -> PathBuf {
            self.tmp.path().join("work")
        }
        fn ctx(&self, flavor: Flavor, pyenv_version: Option<&str>) -> Ctx {
            let mut c = Ctx::for_test(flavor, &self.root(), &self.work());
            c.pyenv_version = pyenv_version.map(String::from);
            c
        }
    }

    #[test]
    fn nothing_selected_is_system() {
        let s = Setup::new(&["3.12.10"]);
        let r = version_name(&s.ctx(Flavor::Pyenv, None), false);
        assert_eq!((r.names, r.failed), (vec!["system".to_string()], false));
        assert_eq!(
            version_origin(&s.ctx(Flavor::Pyenv, None)),
            s.root().join("version").display().to_string()
        );
    }

    #[test]
    fn env_prefixes_python_dash_and_system() {
        let s = Setup::new(&["3.12.10", "3.12.1"]);
        let n = |v: &str| version_name(&s.ctx(Flavor::Pyenv, Some(v)), false).names;
        assert_eq!(n("3.12"), ["3.12.10"]);
        assert_eq!(n("python-3.12"), ["3.12.10"]);
        assert_eq!(n("python-3.12.1"), ["3.12.1"]);
        assert_eq!(n("system:3.12"), ["system", "3.12.10"]);
        assert_eq!(
            version_origin(&s.ctx(Flavor::Pyenv, Some("3.12"))),
            "PYENV_VERSION environment variable"
        );
    }

    #[test]
    fn missing_versions_fail_but_the_rest_is_kept() {
        let s = Setup::new(&["3.12.10"]);
        let r = version_name(&s.ctx(Flavor::Pyenv, Some("3.12.10:9.9")), false);
        assert_eq!(r.names, ["3.12.10"]);
        assert_eq!(
            r.stderr,
            ["pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)"]
        );
        assert!(r.failed);
        let forced = version_name(&s.ctx(Flavor::Pyenv, Some("9.9")), true);
        assert_eq!(
            (forced.names, forced.failed),
            (vec!["9.9".to_string()], false)
        );
    }

    #[test]
    fn local_file_in_a_parent_then_global() {
        let s = Setup::new(&["3.12.1", "3.11.9"]);
        fs::write(s.root().join("version"), "3.11.9\n").unwrap();
        let mut ctx = s.ctx(Flavor::Pyenv, None);
        assert_eq!(version_name(&ctx, false).names, ["3.11.9"]);
        fs::write(s.work().join(".python-version"), "3.12.1\n3.11.9\n").unwrap();
        ctx.pwd = s.work().join("sub");
        ctx.dir = ctx.pwd.clone();
        assert_eq!(version_name(&ctx, false).names, ["3.12.1", "3.11.9"]);
        assert_eq!(
            version_origin(&ctx),
            s.work().join(".python-version").display().to_string()
        );
    }

    #[test]
    fn empty_local_file_shadows_global() {
        let s = Setup::new(&["3.12.1"]);
        fs::write(s.root().join("version"), "3.12.1\n").unwrap();
        fs::write(s.work().join(".python-version"), "").unwrap();
        assert_eq!(
            version_name(&s.ctx(Flavor::Pyenv, None), false).names,
            ["system"]
        );
    }

    #[test]
    fn pyenv_dir_is_searched_before_pwd() {
        let s = Setup::new(&["3.12.1", "3.11.9"]);
        fs::write(s.work().join(".python-version"), "3.11.9\n").unwrap();
        let other = s.tmp.path().join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join(".python-version"), "3.12.1\n").unwrap();
        let mut ctx = s.ctx(Flavor::Pyenv, None);
        ctx.dir = other.clone();
        assert_eq!(version_file(&ctx), other.join(".python-version"));
        ctx.dir = s.root().join("versions");
        assert_eq!(version_file(&ctx), s.work().join(".python-version"));
    }

    #[test]
    fn invalid_file_entries_warn_and_fall_back_to_system() {
        let s = Setup::new(&[]);
        fs::write(s.work().join(".python-version"), "..\n").unwrap();
        let r = version_name(&s.ctx(Flavor::Pyenv, None), false);
        assert_eq!(r.names, ["system"]);
        let shown = s.work().join(".python-version").display().to_string();
        assert_eq!(
            r.stderr,
            [format!("pyenv: invalid version `..' ignored in `{shown}'")]
        );
    }

    #[test]
    fn split_colon_like_bash() {
        assert_eq!(split_colon("3.12.1:"), ["3.12.1"]);
        assert_eq!(split_colon(":3.12.1"), ["", "3.12.1"]);
        assert_eq!(split_colon("a::b"), ["a", "", "b"]);
    }

    #[test]
    fn pyenv_win_env_is_raw_and_space_separated() {
        let s = Setup::new(&["3.9.1"]);
        let sel = win_select(&s.ctx(Flavor::PyenvWin, Some("3.9 3.8.1")));
        let got: Vec<(&str, &str)> = sel
            .iter()
            .map(|x| (x.name.as_str(), x.origin.as_str()))
            .collect();
        assert_eq!(
            got,
            [("3.9", "%PYENV_VERSION%"), ("3.8.1", "%PYENV_VERSION%")]
        );
    }

    #[test]
    fn pyenv_win_local_is_resolved_and_deduplicated() {
        let s = Setup::new(&["3.9.1", "3.8.2"]);
        let local = s.work().join(".python-version");
        fs::write(&local, "3.9\r\n3.9.1\r\n3.8\r\n").unwrap();
        let sel = win_select(&s.ctx(Flavor::PyenvWin, None));
        let names: Vec<&str> = sel.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["3.9.1", "3.8.2"]);
        assert_eq!(sel[0].origin, local.display().to_string());
    }

    #[test]
    fn pyenv_win_empty_local_falls_through_to_global() {
        let s = Setup::new(&["3.9.1"]);
        fs::write(s.work().join(".python-version"), "").unwrap();
        fs::write(s.root().join("version"), "3.9.1\r\n").unwrap();
        let sel = win_select(&s.ctx(Flavor::PyenvWin, None));
        assert_eq!(sel.len(), 1);
        assert_eq!(
            sel[0].origin,
            s.root().join("version").display().to_string()
        );
        assert_eq!(
            win_origin(&s.ctx(Flavor::PyenvWin, None)),
            s.root().join("version").display().to_string()
        );
    }

    #[test]
    fn pyenv_win_nothing_selected_is_empty() {
        let s = Setup::new(&[]);
        assert!(win_select(&s.ctx(Flavor::PyenvWin, None)).is_empty());
    }

    #[test]
    fn win_version_file_is_the_file_that_selects_the_version() {
        let s = Setup::new(&["3.9.1"]);
        let global = s.root().join("version");
        let local = s.work().join(".python-version");
        // No files anywhere: the global path, whether or not it exists.
        assert_eq!(win_version_file(&s.ctx(Flavor::PyenvWin, None)), global);
        // Non-empty local file: it wins.
        fs::write(&local, "3.9.1\r\n").unwrap();
        assert_eq!(win_version_file(&s.ctx(Flavor::PyenvWin, None)), local);
        // Empty local file: falls through to the global file.
        fs::write(&local, "").unwrap();
        fs::write(&global, "3.9.1\r\n").unwrap();
        assert_eq!(win_version_file(&s.ctx(Flavor::PyenvWin, None)), global);
    }
}
