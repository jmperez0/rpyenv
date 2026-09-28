//! Finding the file a command name runs (`which`), and every version that has it (`whence`).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, pathsearch, prefix, select};
use std::ffi::OsStr;
use std::io::Write;
use std::path::PathBuf;

/// What a failed command prints, on which stream, and its exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub lines: Vec<String>,
    pub stderr: bool,
    pub code: i32,
}

impl Report {
    /// Writes the lines with the flavor's line ending.
    pub fn emit(&self, flavor: Flavor) {
        let text: String = self
            .lines
            .iter()
            .map(|l| format!("{l}{}", flavor.eol()))
            .collect();
        if self.stderr {
            let _ = std::io::stderr().write_all(text.as_bytes());
        } else {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
    }
}

/// What the `system` search leaves out besides `<root>/shims`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skip {
    /// Upstream's `_PYENV_SHIM_PATHS_<PROGRAM>` directories.
    pub dirs: Vec<PathBuf>,
    /// rpyenv's shim binary. A `PATH` hit that resolves to it is a shim, not a system
    /// command, so taking it would recurse (allowlist D-30).
    pub exe: Option<PathBuf>,
}

impl Skip {
    /// Reads `_PYENV_SHIM_PATHS_<PROGRAM>` from the process environment.
    pub fn from_env(program: &str, exe: Option<PathBuf>) -> Skip {
        let dirs = std::env::var_os(shim_paths_var(program))
            .filter(|v| !v.is_empty())
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        Skip { dirs, exe }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// Upstream's `invalid version` lines from reading the version file, for stderr.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotFound {
    /// Upstream: the selected versions that are not installed, then "command not found".
    Pyenv {
        missing: Vec<String>,
        origin: String,
        warnings: Vec<String>,
    },
}

/// The name of upstream's `_PYENV_SHIM_PATHS_<PROGRAM>` variable: the program name
/// uppercased, with every character outside `[A-Z0-9_]` turned into `_`.
pub fn shim_paths_var(program: &str) -> String {
    let mut s = String::from("_PYENV_SHIM_PATHS_");
    s.extend(program.chars().map(|c| {
        let u = c.to_ascii_uppercase();
        if u.is_ascii_alphanumeric() || u == '_' {
            u
        } else {
            '_'
        }
    }));
    s
}

/// Upstream `pyenv which <command> [--nosystem]` (libexec/pyenv-which:65-126).
pub fn which_pyenv(
    ctx: &Ctx,
    command: &str,
    nosystem: bool,
    skip: &Skip,
) -> Result<Found, NotFound> {
    let (versions, warnings) = match &ctx.pyenv_version {
        // Raw entries: prefixes are resolved by `prefix_of`, and `python-` is kept.
        Some(v) => (select::split_colon(v), Vec::new()),
        None => {
            let r = select::version_name(ctx, true);
            (r.names, r.stderr)
        }
    };
    let mut missing = Vec::new();
    for v in &versions {
        if v == "system" {
            if let Some(path) = system_command(ctx, command, skip) {
                return Ok(Found { path, warnings });
            }
            continue;
        }
        match prefix::prefix_of(ctx, v) {
            Ok(dir) => {
                let candidate = dir.join("bin").join(command);
                // Upstream's `-x` also accepts a directory (allowlist D-29).
                if pathsearch::is_runnable(&candidate) {
                    return Ok(Found {
                        path: candidate,
                        warnings,
                    });
                }
            }
            Err(_) => missing.push(v.clone()),
        }
    }
    if !nosystem {
        if let Some(path) = system_command(ctx, command, skip) {
            return Ok(Found { path, warnings });
        }
    }
    Err(NotFound::Pyenv {
        missing,
        origin: select::version_origin(ctx),
        warnings,
    })
}

/// Upstream's `system` search: `PATH` with every `~` replaced by `$HOME`, minus
/// `<root>/shims` and `skip.dirs`, then the first runnable `command`. A hit that resolves
/// to rpyenv's shim binary is passed over (allowlist D-30).
fn system_command(ctx: &Ctx, command: &str, skip: &Skip) -> Option<PathBuf> {
    let path = ctx.path.as_ref()?;
    let shims = ctx.shims_dir();
    let home = ctx.home.clone().unwrap_or_default().into_os_string();
    let dirs: Vec<PathBuf> = std::env::split_paths(path)
        .map(|d| replace_tilde(d.as_os_str(), &home))
        .filter(|d| *d != shims && !skip.dirs.contains(d))
        .collect();
    let path = std::env::join_paths(dirs).ok()?;
    let own = skip
        .exe
        .as_ref()
        .and_then(|e| std::fs::canonicalize(e).ok());
    pathsearch::find_all(
        command,
        Some(&path),
        None,
        ctx.flavor,
        ctx.pathext.as_deref(),
    )
    .into_iter()
    .find(|p| own.is_none() || std::fs::canonicalize(p).ok() != own)
}

#[cfg(unix)]
fn replace_tilde(s: &OsStr, home: &OsStr) -> PathBuf {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut out = Vec::new();
    for &b in s.as_bytes() {
        if b == b'~' {
            out.extend_from_slice(home.as_bytes());
        } else {
            out.push(b);
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn replace_tilde(s: &OsStr, home: &OsStr) -> PathBuf {
    PathBuf::from(s.to_string_lossy().replace('~', &home.to_string_lossy()))
}

/// Upstream `pyenv whence`: each entry of `versions --bare` (envs and aliases included)
/// whose `bin` has a runnable `command`, with that file's path.
pub fn whence_pyenv(ctx: &Ctx, command: &str) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for entry in installed::top_level(&ctx.versions_dir(), Flavor::Pyenv) {
        let envs = installed::envs_of(&entry);
        for e in std::iter::once(entry).chain(envs) {
            let candidate = e.path.join("bin").join(command);
            if pathsearch::is_runnable(&candidate) {
                out.push((e.name, candidate));
            }
        }
    }
    out
}

/// What `which` prints when `command` was not found. `advice` is false for `--skip-advice`.
pub fn not_found_report(ctx: &Ctx, command: &str, nf: &NotFound, advice: bool) -> Report {
    match nf {
        NotFound::Pyenv {
            missing,
            origin,
            warnings,
        } => {
            let mut lines = warnings.clone();
            for m in missing {
                lines.push(format!(
                    "pyenv: version `{m}' is not installed (set by {origin})"
                ));
            }
            lines.push(format!("pyenv: {command}: command not found"));
            let versions = whence_pyenv(ctx, command);
            if advice && !versions.is_empty() {
                lines.push(String::new());
                lines.push(format!(
                    "The `{command}' command exists in these Python versions:"
                ));
                lines.extend(versions.into_iter().map(|(n, _)| format!("  {n}")));
                lines.push(String::new());
                lines.push("Note: See 'pyenv help global' for tips on allowing multiple".into());
                lines.push("      Python versions to be found at the same time.".into());
            }
            Report {
                lines,
                stderr: true,
                code: 127,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// A file the platform can run: mode 755 on Unix.
    fn exe(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    struct Root {
        tmp: tempfile::TempDir,
    }

    impl Root {
        fn new(versions: &[&str]) -> Root {
            let tmp = tempfile::tempdir().unwrap();
            fs::create_dir_all(tmp.path().join("root").join("versions")).unwrap();
            fs::create_dir_all(tmp.path().join("work")).unwrap();
            for v in versions {
                fs::create_dir_all(tmp.path().join("root").join("versions").join(v)).unwrap();
            }
            Root { tmp }
        }
        fn root(&self) -> PathBuf {
            self.tmp.path().join("root")
        }
        fn bin(&self, version: &str, name: &str) -> PathBuf {
            let p = self
                .root()
                .join("versions")
                .join(version)
                .join("bin")
                .join(name);
            exe(&p);
            p
        }
        fn ctx(&self, pyenv_version: Option<&str>) -> Ctx {
            let mut ctx = Ctx::for_test(Flavor::Pyenv, &self.root(), &self.tmp.path().join("work"));
            ctx.pyenv_version = pyenv_version.map(String::from);
            ctx
        }
    }

    #[test]
    fn first_version_with_the_command_wins() {
        let r = Root::new(&["3.11.9", "3.12.1"]);
        let tool = r.bin("3.12.1", "tool");
        r.bin("3.11.9", "python");
        let found = which_pyenv(
            &r.ctx(Some("3.11.9:3.12.1")),
            "tool",
            false,
            &Skip::default(),
        );
        assert_eq!(found.map(|f| f.path), Ok(tool));
    }

    #[test]
    fn missing_versions_are_silent_when_a_later_one_has_it() {
        let r = Root::new(&["3.12.1"]);
        let tool = r.bin("3.12.1", "tool");
        let found = which_pyenv(&r.ctx(Some("9.9:3.12.1")), "tool", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(tool));
    }

    #[test]
    fn prefixes_resolve_through_prefix_of() {
        let r = Root::new(&["3.12.10"]);
        let py = r.bin("3.12.10", "python");
        let found = which_pyenv(&r.ctx(Some("3.12")), "python", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(py));
    }

    #[test]
    fn not_found_report_matches_upstream() {
        // Reference probe: PYENV_VERSION=9.9:8.8, `tool` in 3.11.9 and in the env 3.12.1/envs/venv1.
        let r = Root::new(&["3.11.9", "3.12.1/envs/venv1"]);
        r.bin("3.11.9", "tool");
        r.bin("3.12.1/envs/venv1", "tool");
        let ctx = r.ctx(Some("9.9:8.8"));
        let nf = which_pyenv(&ctx, "tool", false, &Skip::default()).unwrap_err();
        assert_eq!(
            nf,
            NotFound::Pyenv {
                missing: vec!["9.9".to_string(), "8.8".to_string()],
                origin: "PYENV_VERSION environment variable".to_string(),
                warnings: vec![],
            }
        );
        let report = not_found_report(&ctx, "tool", &nf, true);
        assert_eq!(
            report.lines,
            [
                "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)",
                "pyenv: version `8.8' is not installed (set by PYENV_VERSION environment variable)",
                "pyenv: tool: command not found",
                "",
                "The `tool' command exists in these Python versions:",
                "  3.11.9",
                "  3.12.1/envs/venv1",
                "",
                "Note: See 'pyenv help global' for tips on allowing multiple",
                "      Python versions to be found at the same time.",
            ]
        );
        assert!(report.stderr);
        assert_eq!(report.code, 127);
        let short = not_found_report(&ctx, "tool", &nf, false);
        assert_eq!(short.lines.len(), 3);
    }

    #[test]
    fn no_advice_block_when_no_version_has_it() {
        let r = Root::new(&["3.12.1"]);
        let ctx = r.ctx(Some("3.12.1"));
        let nf = which_pyenv(&ctx, "nothing", true, &Skip::default()).unwrap_err();
        assert_eq!(
            not_found_report(&ctx, "nothing", &nf, true).lines,
            ["pyenv: nothing: command not found"]
        );
    }

    #[test]
    fn a_directory_does_not_count_as_found() {
        // Upstream's `-x` accepts a directory (allowlist D-29).
        let r = Root::new(&["3.12.1"]);
        fs::create_dir_all(r.root().join("versions/3.12.1/bin/tool")).unwrap();
        assert!(which_pyenv(&r.ctx(Some("3.12.1")), "tool", true, &Skip::default()).is_err());
    }

    #[test]
    fn version_file_names_are_normalized() {
        let r = Root::new(&["3.12.10"]);
        let py = r.bin("3.12.10", "python");
        fs::write(r.tmp.path().join("work/.python-version"), "python-3.12\n").unwrap();
        let found = which_pyenv(&r.ctx(None), "python", false, &Skip::default());
        assert_eq!(found.map(|f| f.path), Ok(py));
    }

    #[test]
    fn shim_paths_variable_name() {
        assert_eq!(shim_paths_var("python3.12"), "_PYENV_SHIM_PATHS_PYTHON3_12");
        assert_eq!(
            shim_paths_var("pip-compile"),
            "_PYENV_SHIM_PATHS_PIP_COMPILE"
        );
    }

    #[test]
    fn whence_lists_versions_and_envs_in_versions_order() {
        let r = Root::new(&["3.11.9", "3.12.1/envs/venv1", "3.12.2"]);
        let a = r.bin("3.11.9", "tool");
        let b = r.bin("3.12.1/envs/venv1", "tool");
        assert_eq!(
            whence_pyenv(&r.ctx(None), "tool"),
            [
                ("3.11.9".to_string(), a),
                ("3.12.1/envs/venv1".to_string(), b)
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn system_search_skips_shims_listed_dirs_home_and_the_shim_binary() {
        use std::os::unix::fs::symlink;
        let r = Root::new(&[]);
        let base = r.tmp.path();
        let shims = r.root().join("shims");
        exe(&shims.join("tool"));
        exe(&base.join("listed/tool"));
        let shim_exe = base.join("pyenv-shim");
        exe(&shim_exe);
        fs::create_dir_all(base.join("links")).unwrap();
        symlink(&shim_exe, base.join("links/tool")).unwrap();
        let real = base.join("home/sys/tool");
        exe(&real);
        let mut ctx = r.ctx(Some("system"));
        ctx.home = Some(base.join("home"));
        let path = format!(
            "{}:{}:{}:~/sys",
            shims.display(),
            base.join("listed").display(),
            base.join("links").display()
        );
        ctx.path = Some(path.into());
        let skip = Skip {
            dirs: vec![base.join("listed")],
            exe: Some(shim_exe),
        };
        let found = which_pyenv(&ctx, "tool", false, &skip).map(|f| f.path);
        assert_eq!(found, Ok(base.join("home").join("sys").join("tool")));
    }

    #[test]
    fn nosystem_leaves_out_the_final_path_search() {
        // With nothing selected, `version-name -f` gives `system`, which is searched at its
        // own position even with `--nosystem`; so select a real version here.
        let r = Root::new(&["3.12.1"]);
        let sys = r.tmp.path().join("sys");
        exe(&sys.join("tool"));
        let mut ctx = r.ctx(Some("3.12.1"));
        ctx.path = Some(sys.clone().into_os_string());
        assert!(which_pyenv(&ctx, "tool", false, &Skip::default()).is_ok());
        assert!(which_pyenv(&ctx, "tool", true, &Skip::default()).is_err());
    }
}
