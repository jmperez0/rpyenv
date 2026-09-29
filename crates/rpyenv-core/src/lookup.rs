//! Finding the file a command name runs (`which`), and every version that has it (`whence`).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, pathsearch, prefix, select};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    /// pyenv-win: nothing is selected.
    WinNoVersion,
    /// pyenv-win: a selected version that is not installed stops the search.
    WinNotInstalled(String),
    /// pyenv-win: no selected version has it.
    WinNotFound,
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

/// The Linux flavor off Unix only runs in tests. There `~` is left alone: Windows short
/// names such as `C:\Users\RUNNER~1` contain it, and expanding it would break test paths.
#[cfg(not(unix))]
fn replace_tilde(s: &OsStr, _home: &OsStr) -> PathBuf {
    PathBuf::from(s)
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

/// pyenv-win `GetExtensions(True)`: `PATHEXT` entries as written, in order, then `.PY`
/// and `.PYW` unless present. An empty `PATHEXT` gives one empty extension first.
pub fn win_extensions(pathext: Option<&OsStr>) -> Vec<String> {
    let raw = pathext
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut exts: Vec<String> = raw
        .split(';')
        .filter(|e| !e.is_empty())
        .map(String::from)
        .collect();
    if exts.is_empty() {
        exts.push(String::new());
    }
    for add in [".PY", ".PYW"] {
        if !exts.iter().any(|e| e.eq_ignore_ascii_case(add)) {
            exts.push(add.to_string());
        }
    }
    exts
}

/// The files pyenv-win checks in one version: in the folder, then `Scripts`, then `bin`,
/// the bare name and then the first extension that exists. Every hit, in that order.
/// Names match without case, as on NTFS, and each hit carries the file's on-disk name.
fn win_hits(version_dir: &Path, program: &str, exts: &[String]) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    for dir in [
        version_dir.to_path_buf(),
        version_dir.join("Scripts"),
        version_dir.join("bin"),
    ] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let files: Vec<std::ffi::OsString> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name())
            .collect();
        let find = |want: &str| {
            files
                .iter()
                .find(|f| f.to_string_lossy().eq_ignore_ascii_case(want))
                .map(|f| dir.join(f))
        };
        if let Some(p) = find(program) {
            hits.push(p);
        }
        if let Some(p) = exts.iter().find_map(|e| find(&format!("{program}{e}"))) {
            hits.push(p);
        }
    }
    hits
}

/// pyenv-win `CommandWhich` (pyenv.vbs:101-169). The folders in the printed path are
/// as built from the root; only the file name is in its on-disk case (allowlist D-31).
pub fn which_win(ctx: &Ctx, command: &str) -> Result<Found, NotFound> {
    which_win_with(ctx, command, false)
}

/// `which_win` for launching: only files Windows can start count, so a bare `tool` next
/// to `tool.exe` is passed over (M1b review M-4).
pub fn which_win_runnable(ctx: &Ctx, command: &str) -> Result<Found, NotFound> {
    which_win_with(ctx, command, true)
}

fn which_win_with(ctx: &Ctx, command: &str, runnable_only: bool) -> Result<Found, NotFound> {
    let program = command.strip_suffix('.').unwrap_or(command);
    let selected = select::win_select(ctx);
    if selected.is_empty() {
        return Err(NotFound::WinNoVersion);
    }
    let exts = win_extensions(ctx.pathext.as_deref());
    for s in &selected {
        let dir = ctx.versions_dir().join(&s.name);
        if !dir.is_dir() {
            return Err(NotFound::WinNotInstalled(s.name.clone()));
        }
        let hits = win_hits(&dir, program, &exts);
        if let Some(path) = hits
            .into_iter()
            .find(|h| !runnable_only || is_runnable_win(h))
        {
            return Ok(Found {
                path,
                warnings: Vec::new(),
            });
        }
    }
    Err(NotFound::WinNotFound)
}

fn is_runnable_win(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["exe", "com", "bat", "cmd"]
            .iter()
            .any(|r| e.eq_ignore_ascii_case(r))
    })
}

/// pyenv-win `CommandWhence` (pyenv.vbs:171-261): every installed version, selection
/// ignored. Without `--path`, each version that has the program is listed once
/// (allowlist D-32); with `--path`, every hit.
pub fn whence_win(ctx: &Ctx, program: &str, with_path: bool) -> Vec<String> {
    let exts = win_extensions(ctx.pathext.as_deref());
    let mut out = Vec::new();
    for name in installed::names(&ctx.versions_dir(), Flavor::PyenvWin) {
        let hits = win_hits(&ctx.versions_dir().join(&name), program, &exts);
        if with_path {
            out.extend(hits.iter().map(|h| h.display().to_string()));
        } else if !hits.is_empty() {
            out.push(name);
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
        NotFound::WinNoVersion => Report {
            lines: select::WIN_NO_VERSION
                .iter()
                .map(|l| l.to_string())
                .collect(),
            stderr: false,
            code: 1,
        },
        // pyenv-win repeats the name where the origin would go (pyenv.vbs:125).
        NotFound::WinNotInstalled(v) => Report {
            lines: vec![format!(
                "pyenv: version '{v}' is not installed (set by {v})"
            )],
            stderr: false,
            code: 1,
        },
        NotFound::WinNotFound => {
            let program = command.strip_suffix('.').unwrap_or(command);
            let mut lines = vec![format!("pyenv: {command}: command not found")];
            let versions = whence_win(ctx, program, false);
            if !versions.is_empty() {
                lines.push(String::new());
                lines.push(format!(
                    "The '{command}' command exists in these Python versions:"
                ));
                lines.extend(versions.iter().map(|v| format!("  {v}")));
                // pyenv-win indents the CRLF that ends whence's output, too.
                lines.push("  ".to_string());
            }
            Report {
                lines,
                stderr: false,
                code: 127,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    fn win_root(versions: &[&str]) -> (tempfile::TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        for v in versions {
            fs::create_dir_all(root.join("versions").join(v)).unwrap();
        }
        fs::create_dir_all(root.join("versions")).unwrap();
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let mut ctx = Ctx::for_test(Flavor::PyenvWin, &root, &tmp.path().join("work"));
        // Lowercase, so the tests behave the same on case-sensitive file systems.
        ctx.pathext = Some(".exe;.bat".into());
        (tmp, ctx)
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
    }

    #[test]
    fn win_extension_list() {
        assert_eq!(
            win_extensions(Some(OsStr::new(".COM;.EXE"))),
            [".COM", ".EXE", ".PY", ".PYW"]
        );
        assert_eq!(
            win_extensions(Some(OsStr::new(".py;;.EXE"))),
            [".py", ".EXE", ".PYW"]
        );
        assert_eq!(win_extensions(None), ["", ".PY", ".PYW"]);
    }

    #[test]
    fn win_which_walks_selected_versions_folder_scripts_bin() {
        let (_t, mut ctx) = win_root(&["3.8.2", "3.9.1"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.8.2/python38.exe"));
        touch(&v.join("3.9.1/Scripts/pip.exe"));
        touch(&v.join("3.9.1/bin/pip.exe"));
        touch(&v.join("3.9.1/bin/tool.bat"));
        ctx.pyenv_version = Some("3.9.1 3.8.2".to_string());
        let path = |c: &str| which_win(&ctx, c).map(|f| f.path);
        assert_eq!(path("python38"), Ok(v.join("3.8.2").join("python38.exe")));
        assert_eq!(
            path("pip"),
            Ok(v.join("3.9.1").join("Scripts").join("pip.exe"))
        );
        assert_eq!(
            path("tool"),
            Ok(v.join("3.9.1").join("bin").join("tool.bat"))
        );
        // One trailing dot is dropped.
        assert_eq!(
            path("pip."),
            Ok(v.join("3.9.1").join("Scripts").join("pip.exe"))
        );
    }

    #[test]
    fn win_runnable_lookup_skips_files_windows_cannot_start() {
        let (_t, mut ctx) = win_root(&["3.9.1"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.9.1/Scripts/tool"));
        touch(&v.join("3.9.1/Scripts/tool.exe"));
        ctx.pyenv_version = Some("3.9.1".to_string());
        assert_eq!(
            which_win(&ctx, "tool").map(|f| f.path),
            Ok(v.join("3.9.1").join("Scripts").join("tool"))
        );
        assert_eq!(
            which_win_runnable(&ctx, "tool").map(|f| f.path),
            Ok(v.join("3.9.1").join("Scripts").join("tool.exe"))
        );
    }

    #[test]
    fn win_which_bare_name_before_extensions() {
        let (_t, mut ctx) = win_root(&["3.9.1"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.9.1/tool"));
        touch(&v.join("3.9.1/tool.exe"));
        ctx.pyenv_version = Some("3.9.1".to_string());
        assert_eq!(
            which_win(&ctx, "tool").map(|f| f.path),
            Ok(v.join("3.9.1").join("tool"))
        );
    }

    #[test]
    fn win_which_failures_and_reports() {
        let (_t, mut ctx) = win_root(&["3.8.2", "3.8.6"]);
        let v = ctx.versions_dir();
        touch(&v.join("3.8.2/python38.exe"));
        touch(&v.join("3.8.6/python38.exe"));
        assert_eq!(which_win(&ctx, "python"), Err(NotFound::WinNoVersion));
        let r = not_found_report(&ctx, "python", &NotFound::WinNoVersion, true);
        assert_eq!((r.lines.len(), r.stderr, r.code), (5, false, 1));

        ctx.pyenv_version = Some("3.7.7".to_string());
        let nf = which_win(&ctx, "python").unwrap_err();
        assert_eq!(nf, NotFound::WinNotInstalled("3.7.7".to_string()));
        assert_eq!(
            not_found_report(&ctx, "python", &nf, true).lines,
            ["pyenv: version '3.7.7' is not installed (set by 3.7.7)"]
        );

        ctx.pyenv_version = Some("3.8.2".to_string());
        fs::remove_file(v.join("3.8.2/python38.exe")).unwrap();
        let nf = which_win(&ctx, "python38").unwrap_err();
        assert_eq!(nf, NotFound::WinNotFound);
        let r = not_found_report(&ctx, "python38", &nf, true);
        assert_eq!(
            r.lines,
            [
                "pyenv: python38: command not found",
                "",
                "The 'python38' command exists in these Python versions:",
                "  3.8.6",
                "  ",
            ]
        );
        assert_eq!((r.stderr, r.code), (false, 127));
        assert_eq!(
            not_found_report(&ctx, "unknown3.8", &NotFound::WinNotFound, true).lines,
            ["pyenv: unknown3.8: command not found"]
        );
    }

    #[test]
    fn win_whence_names_once_and_paths_in_search_order() {
        let (_t, ctx) = win_root(&["3.8.2"]);
        let v = ctx.versions_dir().join("3.8.2");
        touch(&v.join("foo.exe"));
        touch(&v.join("Scripts/foo.exe"));
        touch(&v.join("bin/foo.exe"));
        // pyenv-win prints `3.8.2` twice here (allowlist D-32).
        assert_eq!(whence_win(&ctx, "foo", false), ["3.8.2"]);
        assert_eq!(
            whence_win(&ctx, "foo", true),
            [
                v.join("foo.exe").display().to_string(),
                v.join("Scripts").join("foo.exe").display().to_string(),
                v.join("bin").join("foo.exe").display().to_string(),
            ]
        );
        assert!(whence_win(&ctx, "bar", false).is_empty());
    }
}
