//! Running a command the way `pyenv exec` and the shims do: which file, which environment,
//! and how the process is started (spec §5).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::lookup::{self, Report, Skip};
use crate::paths::win_path_key;
use crate::{pathsearch, rehash, select};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// What to run and how. Built by [`plan`], started by [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The resolved file. On Linux it is also the child's `argv[0]`, as upstream's
    /// `exec "$PYENV_COMMAND_PATH"` makes it.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Windows: the caller's command line after the command, unchanged. When set, a
    /// non-batch child gets it through `raw_arg` instead of `args` (spec §5.3).
    pub raw_tail: Option<OsString>,
    /// Variables to set (`Some`) or remove (`None`) in the child.
    pub env: Vec<(OsString, Option<OsString>)>,
    /// Upstream's `invalid version` lines, for stderr before starting.
    pub warnings: Vec<String>,
    /// Spawn, wait, then run the rehash check, instead of replacing this process.
    pub wait: bool,
}

/// Inputs from the environment that `Ctx` doesn't carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecEnv {
    /// Upstream's `_PYENV_SHIM_PATH`, set by upstream's own shims.
    pub shim_path: Option<OsString>,
    /// Upstream's `_PYENV_SHIM_PATHS_<PROGRAM>`.
    pub shim_paths: Option<OsString>,
    /// `APPDATA`: pyenv-win puts the user-site `Scripts` folder on `PATH`.
    pub appdata: Option<OsString>,
    /// rpyenv's shim binary, never taken as a `system` command (allowlist D-30).
    pub shim_exe: Option<PathBuf>,
}

impl ExecEnv {
    pub fn from_process(program: &str, shim_exe: Option<PathBuf>) -> ExecEnv {
        let get = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        ExecEnv {
            shim_path: get("_PYENV_SHIM_PATH"),
            shim_paths: get(&lookup::shim_paths_var(program)),
            appdata: get("APPDATA"),
            shim_exe,
        }
    }
}

/// `pyenv exec` or a shim. They differ only on Windows, in how the command is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Exec,
    Shim,
}

/// pyenv-win `exec`'s own message when the last selected version is missing.
pub const WIN_EXEC_NO_VERSION: [&str; 3] = [
    "No global/local python version has been set yet. Please set the global/local version by typing:",
    "pyenv global 3.7.4",
    "pyenv local 3.7.4",
];

/// Resolves `command`. `Err` carries what to print and the exit code.
pub fn plan(
    ctx: &Ctx,
    mode: Mode,
    command: &str,
    args: Vec<OsString>,
    env: &ExecEnv,
) -> Result<LaunchPlan, Report> {
    match ctx.flavor {
        Flavor::Pyenv => plan_pyenv(ctx, command, args, env),
        Flavor::PyenvWin => plan_win(ctx, mode, command, args, env),
    }
}

/// Upstream `pyenv-exec` (libexec/pyenv-exec:24-57).
fn plan_pyenv(
    ctx: &Ctx,
    command: &str,
    args: Vec<OsString>,
    env: &ExecEnv,
) -> Result<LaunchPlan, Report> {
    let names = select::version_name(ctx, true);
    let version = names.names.join(":");
    // `_PYENV_SHIM_PATH` is prepended to `_PYENV_SHIM_PATHS_<PROGRAM>` with `:`.
    let shim_paths = match (&env.shim_path, &env.shim_paths) {
        (Some(p), Some(ps)) => {
            let mut s = p.clone();
            s.push(":");
            s.push(ps);
            Some(s)
        }
        (Some(p), None) => Some(p.clone()),
        (None, ps) => ps.clone(),
    };
    let skip = Skip {
        dirs: shim_paths
            .as_ref()
            .map(|v| std::env::split_paths(v).collect())
            .unwrap_or_default(),
        exe: env.shim_exe.clone(),
    };
    // `pyenv-which` sees the resolved PYENV_VERSION only when the caller had exported one.
    let mut which_ctx = ctx.clone();
    if which_ctx.pyenv_version.is_some() {
        which_ctx.pyenv_version = Some(version.clone());
    }
    let found = match lookup::which_pyenv(&which_ctx, command, false, &skip) {
        Ok(f) => f,
        Err(nf) => {
            let mut report = lookup::not_found_report(&which_ctx, command, &nf, true);
            let mut lines = names.stderr.clone();
            lines.append(&mut report.lines);
            report.lines = lines;
            return Err(report);
        }
    };
    let mut vars = vec![
        (
            OsString::from("PYENV_VERSION"),
            Some(OsString::from(&version)),
        ),
        (
            OsString::from("PYENV_ROOT"),
            Some(ctx.root.clone().into_os_string()),
        ),
        (
            OsString::from("PYENV_DIR"),
            Some(ctx.dir.clone().into_os_string()),
        ),
    ];
    if env.shim_path.is_some() {
        vars.push((OsString::from(lookup::shim_paths_var(command)), shim_paths));
        vars.push((OsString::from("_PYENV_SHIM_PATH"), None));
    }
    // The command's folder goes first on PATH only when it lies under PYENV_ROOT, by
    // plain string prefix (libexec/pyenv-exec:53-56). No libexec or plugin folders (D-38).
    let bin = found.path.parent().unwrap_or(Path::new("")).as_os_str();
    if starts_with(bin, ctx.root.as_os_str()) {
        let mut p = bin.to_os_string();
        p.push(":");
        if let Some(old) = &ctx.path {
            p.push(old);
        }
        vars.push((OsString::from("PATH"), Some(p)));
    }
    let mut warnings = names.stderr;
    warnings.extend(found.warnings);
    Ok(LaunchPlan {
        wait: is_pip_like(command, &args),
        program: found.path,
        args,
        raw_tail: None,
        env: vars,
        warnings,
    })
}

#[cfg(unix)]
fn starts_with(s: &OsStr, prefix: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().starts_with(prefix.as_bytes())
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn starts_with(s: &OsStr, prefix: &OsStr) -> bool {
    s.to_string_lossy().starts_with(&*prefix.to_string_lossy())
}

/// pyenv-win `exec` (pyenv.bat:21-129) and rpyenv's Windows shims.
fn plan_win(
    ctx: &Ctx,
    mode: Mode,
    command: &str,
    args: Vec<OsString>,
    env: &ExecEnv,
) -> Result<LaunchPlan, Report> {
    let names: Vec<String> = select::win_select(ctx)
        .into_iter()
        .map(|s| s.name)
        .collect();
    let path = win_child_path(ctx, &names, env.appdata.as_deref());
    let program = match mode {
        // A shim finds its file like `pyenv which`, launching only files Windows can start
        // (M1b review M-4); not found is exit 127 (D-42).
        Mode::Shim => {
            lookup::which_win_runnable(ctx, command)
                .map_err(|nf| lookup::not_found_report(ctx, command, &nf, true))?
                .path
        }
        // `exec` lets the command line find it on the new PATH (D-40).
        Mode::Exec => {
            win_exec_version_check(ctx, &names)?;
            pathsearch::find_cmd(command, &path, ctx.pathext.as_deref(), &ctx.pwd).ok_or_else(
                || Report {
                    lines: vec![
                        format!(
                            "'{command}' is not recognized as an internal or external command,"
                        ),
                        "operable program or batch file.".to_string(),
                    ],
                    stderr: true,
                    code: 1,
                },
            )?
        }
    };
    Ok(LaunchPlan {
        program,
        args,
        raw_tail: None,
        env: vec![(OsString::from("PATH"), Some(path))],
        warnings: Vec::new(),
        wait: true,
    })
}

/// pyenv-win `exec` checks only the last selected version (pyenv.bat:67-72).
pub fn win_exec_version_check(ctx: &Ctx, names: &[String]) -> Result<(), Report> {
    if names
        .last()
        .is_some_and(|n| ctx.versions_dir().join(n).is_dir())
    {
        Ok(())
    } else {
        Err(Report {
            lines: WIN_EXEC_NO_VERSION.iter().map(|l| l.to_string()).collect(),
            stderr: false,
            code: 1,
        })
    }
}

/// pyenv-win's child `PATH`: each selected version's folder, `Scripts` and `bin`; the
/// user-site `Scripts` of the last one; then the caller's `PATH` entries, unquoted, without
/// empty ones and without the shims folder in any spelling. Every entry ends with `;`
/// (allowlist D-40).
pub fn win_child_path(ctx: &Ctx, names: &[String], appdata: Option<&OsStr>) -> OsString {
    let mut out = OsString::new();
    let mut add = |p: &OsStr| {
        out.push(p);
        out.push(";");
    };
    for n in names {
        let v = ctx.versions_dir().join(n);
        add(v.as_os_str());
        add(v.join("Scripts").as_os_str());
        add(v.join("bin").as_os_str());
    }
    if let (Some(last), Some(appdata)) = (names.last(), appdata) {
        add(OsStr::new(&user_site_scripts(appdata, last)));
    }
    let shims = win_path_key(&ctx.shims_dir());
    if let Some(path) = &ctx.path {
        for entry in path.to_string_lossy().split(';') {
            let entry = entry.replace('"', "");
            if !entry.is_empty() && win_path_key(Path::new(&entry)) != shims {
                add(OsStr::new(&entry));
            }
        }
    }
    out
}

/// `%APPDATA%\Python\Python<x><y>[-32]\Scripts` for a name like `3.8.9` or `3.8.9-win32`
/// (pyenv.bat:26-36).
fn user_site_scripts(appdata: &OsStr, name: &str) -> String {
    let (version, arch) = name.split_once('-').unwrap_or((name, ""));
    let mut fields = version.split('.');
    let (x, y) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
    let suffix = if arch.eq_ignore_ascii_case("win32") {
        "-32"
    } else {
        ""
    };
    format!(
        "{}\\Python\\Python{x}{y}{suffix}\\Scripts",
        appdata.to_string_lossy()
    )
}

/// Commands after which a Linux shim waits and runs the rehash check (spec §5.2): `pip`,
/// `easy_install` and `conda`, with any trailing version digits (`pip3.12`), and anything
/// run with `-m pip` (allowlist D-39).
pub fn is_pip_like(command: &str, args: &[OsString]) -> bool {
    let name = Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    matches!(base, "pip" | "easy_install" | "conda")
        || args.windows(2).any(|w| w[0] == "-m" && w[1] == "pip")
}

/// Starts the plan. Without `wait` (Linux) this process becomes the command, and `run`
/// returns only if that fails. With `wait`, the child runs to the end, the rehash check
/// runs (`rehash_with` is the shim binary rehash uses), and the child's exit code is
/// returned. On Linux, a child that died from a signal makes this process die from it too.
/// `Err` means the program couldn't be started at all: the caller decides where that goes
/// (a GUI shim may have nowhere to print it).
pub fn run(plan: &LaunchPlan, ctx: &Ctx, rehash_with: Option<&Path>) -> Result<i32, Report> {
    let mut cmd = Command::new(&plan.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        match &plan.raw_tail {
            Some(tail) if !is_batch(&plan.program) => {
                cmd.raw_arg(tail);
            }
            _ => {
                cmd.args(&plan.args);
            }
        }
    }
    #[cfg(not(windows))]
    cmd.args(&plan.args);
    for (k, v) in &plan.env {
        match v {
            Some(v) => {
                cmd.env(k, v);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }
    #[cfg(unix)]
    if !plan.wait {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        return Err(cannot_run(&plan.program, &err));
    }
    #[cfg(unix)]
    let status = sig::spawn_and_wait(&mut cmd);
    #[cfg(windows)]
    let status = crate::winproc::spawn_and_wait(&mut cmd, &plan.program);
    #[cfg(not(any(unix, windows)))]
    let status = cmd.status();
    if let Some(exe) = rehash_with {
        rehash::check(ctx, exe);
    }
    match status {
        Ok(s) => Ok(exit_code(s)),
        Err(e) => Err(cannot_run(&plan.program, &e)),
    }
}

/// A `.bat` or `.cmd` file: std starts it through cmd.exe with its batch-file escaping,
/// so it gets CRT-split arguments rather than the raw tail (spec §5.3).
#[cfg(windows)]
fn is_batch(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
}

fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            sig::die_by(signal);
        }
    }
    status.code().unwrap_or(1)
}

/// A file that can't be started: exit 127 when the file itself is missing, else 126, as
/// bash does (allowlist D-43). A NotFound for a file that exists (a missing `#!`
/// interpreter) is 126, bash's `bad interpreter`. Only builds the report; the caller
/// prints and logs it.
fn cannot_run(program: &Path, err: &std::io::Error) -> Report {
    let code = if err.kind() == std::io::ErrorKind::NotFound && !program.exists() {
        127
    } else {
        126
    };
    #[cfg(unix)]
    if code == 126 && err.kind() == std::io::ErrorKind::NotFound {
        if let Some(interp) = shebang_interpreter(program) {
            return Report {
                lines: vec![format!(
                    "pyenv: {}: {}: bad interpreter: No such file or directory",
                    program.display(),
                    interp
                )],
                stderr: true,
                code,
            };
        }
    }
    Report {
        lines: vec![format!(
            "pyenv: {}: {}",
            program.display(),
            start_failure_reason(program, err)
        )],
        stderr: true,
        code,
    }
}

/// The interpreter a script's `#!` line names: the first word after `#!`, from the first 256
/// bytes. None when the file has no `#!` or can't be read (bash's `bad interpreter` message).
/// Words end at a space or tab only, as the kernel splits them, so a CRLF line's `\r` stays
/// part of the name; bash shows it as `^M`, which is why such a script fails to start.
#[cfg(unix)]
fn shebang_interpreter(program: &Path) -> Option<String> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(program)
        .ok()?
        .take(256)
        .read_to_end(&mut head)
        .ok()?;
    let line = head.strip_prefix(b"#!")?;
    let line = line.split(|&b| b == b'\n').next()?;
    let word = line
        .split(|&b| b == b' ' || b == b'\t')
        .find(|w| !w.is_empty())?;
    let name = String::from_utf8_lossy(word);
    Some(match name.strip_suffix('\r') {
        Some(stem) => format!("{stem}^M"),
        None => name.into_owned(),
    })
}

/// `io_reason`, with the `%1` that some Windows messages leave for the program's name
/// filled in (`FormatMessage` is called without inserts).
fn start_failure_reason(program: &Path, err: &std::io::Error) -> String {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    io_reason(err).replace("%1", &name)
}

/// An I/O error's text without Rust's ` (os error N)` suffix, as a shell prints it (D-43).
pub fn io_reason(err: &std::io::Error) -> String {
    let text = err.to_string();
    match text.rfind(" (os error ") {
        Some(i) if text.ends_with(')') => text[..i].to_string(),
        _ => text,
    }
}

#[cfg(unix)]
mod sig {
    use std::io;
    use std::mem;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, ExitStatus};
    use std::sync::atomic::{AtomicI32, Ordering};

    static CHILD: AtomicI32 = AtomicI32::new(0);

    /// The four signals whose disposition this process changes around a wait, in a fixed
    /// order shared by every array indexed by signal here.
    const SIGNALS: [libc::c_int; 4] = [libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP];

    extern "C" fn forward(signal: libc::c_int) {
        let pid = CHILD.load(Ordering::SeqCst);
        if pid > 0 {
            // SAFETY: kill is async-signal-safe.
            unsafe {
                libc::kill(pid, signal);
            }
        }
    }

    /// Reads a signal's current disposition without changing it (`act` is null).
    unsafe fn current(signal: libc::c_int) -> libc::sighandler_t {
        let mut old: libc::sigaction = mem::zeroed();
        libc::sigaction(signal, std::ptr::null(), &mut old);
        old.sa_sigaction
    }

    /// Installs `handler` (a real handler, `SIG_IGN` or `SIG_DFL`) for `signal`.
    unsafe fn install(signal: libc::c_int, handler: libc::sighandler_t) {
        let mut act: libc::sigaction = mem::zeroed();
        act.sa_sigaction = handler;
        libc::sigemptyset(&mut act.sa_mask);
        act.sa_flags = 0;
        libc::sigaction(signal, &act, std::ptr::null_mut());
    }

    /// Spawns and waits like a shell does (spec §5.2): SIGINT and SIGQUIT are ignored here,
    /// because the terminal sends them to the child too; SIGTERM and SIGHUP are forwarded to
    /// the child. A signal already ignored on entry stays ignored throughout, in the child
    /// too, the way a shell keeps an inherited ignore for the commands it runs (`nohup ...
    /// &` must survive logout, and a backgrounded `pip` in a script must survive Ctrl-C).
    /// A signal that arrives before the child's PID is stored is lost; the window is the
    /// few instructions between `spawn` and `store`. This process's own dispositions are
    /// restored once the wait is over, before the rehash check that follows runs.
    pub fn spawn_and_wait(cmd: &mut Command) -> io::Result<ExitStatus> {
        let handler = forward as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: only reads each signal's disposition; nothing is changed yet.
        let on_entry: [libc::sighandler_t; 4] = SIGNALS.map(|s| unsafe { current(s) });
        let was_ignored: [bool; 4] = on_entry.map(|h| h == libc::SIG_IGN);
        // What the child restores each signal to before exec: SIG_IGN when it was already
        // ignored on entry, else SIG_DFL. Computed now so `pre_exec` only calls the
        // async-signal-safe `libc::sigaction` on values already known.
        let child_dispositions: [libc::sighandler_t; 4] = was_ignored.map(|ignored| {
            if ignored {
                libc::SIG_IGN
            } else {
                libc::SIG_DFL
            }
        });
        // SAFETY: changing this process's signal dispositions before spawning. The child
        // restores the precomputed dispositions before exec, using only async-signal-safe
        // calls.
        unsafe {
            install(libc::SIGINT, libc::SIG_IGN);
            install(libc::SIGQUIT, libc::SIG_IGN);
            if !was_ignored[2] {
                install(libc::SIGTERM, handler);
            }
            if !was_ignored[3] {
                install(libc::SIGHUP, handler);
            }
            cmd.pre_exec(move || {
                for (signal, disposition) in SIGNALS.iter().zip(child_dispositions) {
                    install(*signal, disposition);
                }
                Ok(())
            });
        }
        let result = match cmd.spawn() {
            Ok(mut child) => {
                CHILD.store(child.id() as i32, Ordering::SeqCst);
                child.wait()
            }
            Err(e) => Err(e),
        };
        // Restore first, then clear CHILD, on both paths: a TERM or HUP that arrives in
        // between meets the caller's own disposition, not a forwarder with no child.
        // SAFETY: restoring this process's own dispositions to exactly what `current` read
        // on entry, before anything was changed.
        unsafe {
            for (signal, original) in SIGNALS.iter().zip(on_entry) {
                install(*signal, original);
            }
        }
        CHILD.store(0, Ordering::SeqCst);
        result
    }

    /// Dies from `signal`, as the child did, so the caller sees the same status. No core
    /// file: the child made one if it was going to, and the shim's would overwrite it.
    pub fn die_by(signal: i32) -> ! {
        // SAFETY: plain libc calls on this process; lowering our own core limit is allowed.
        unsafe {
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn exe(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn var<'a>(plan: &'a LaunchPlan, name: &str) -> Option<&'a Option<OsString>> {
        plan.env
            .iter()
            .find(|(k, _)| k.as_os_str() == name)
            .map(|(_, v)| v)
    }

    fn pyenv_ctx() -> (tempfile::TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("versions")).unwrap();
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let ctx = Ctx::for_test(Flavor::Pyenv, &root, &tmp.path().join("work"));
        (tmp, ctx)
    }

    #[test]
    fn pyenv_plan_sets_the_upstream_environment() {
        let (tmp, mut ctx) = pyenv_ctx();
        let py = ctx
            .versions_dir()
            .join("3.12.10")
            .join("bin")
            .join("python");
        exe(&py);
        let sys = tmp.path().join("sys");
        ctx.path = Some(sys.clone().into_os_string());
        ctx.pyenv_version = Some("3.12".to_string());
        let p = plan(
            &ctx,
            Mode::Exec,
            "python",
            vec!["-V".into()],
            &ExecEnv::default(),
        )
        .unwrap();
        assert_eq!(p.program, py);
        assert_eq!(p.args, [OsString::from("-V")]);
        assert_eq!(
            var(&p, "PYENV_VERSION"),
            Some(&Some(OsString::from("3.12.10")))
        );
        assert_eq!(
            var(&p, "PYENV_ROOT"),
            Some(&Some(ctx.root.clone().into_os_string()))
        );
        assert_eq!(
            var(&p, "PYENV_DIR"),
            Some(&Some(ctx.dir.clone().into_os_string()))
        );
        let mut path = py.parent().unwrap().as_os_str().to_os_string();
        path.push(":");
        path.push(&sys);
        assert_eq!(var(&p, "PATH"), Some(&Some(path)));
        assert!(!p.wait);
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn pyenv_plan_for_system_leaves_path_alone() {
        let (tmp, mut ctx) = pyenv_ctx();
        let tool = tmp.path().join("sys").join("tool");
        exe(&tool);
        ctx.path = Some(tmp.path().join("sys").into_os_string());
        let p = plan(&ctx, Mode::Shim, "tool", vec![], &ExecEnv::default()).unwrap();
        assert_eq!(p.program, tool);
        assert_eq!(
            var(&p, "PYENV_VERSION"),
            Some(&Some(OsString::from("system")))
        );
        assert_eq!(var(&p, "PATH"), None);
    }

    #[test]
    fn pyenv_plan_not_found_is_the_which_report() {
        let (_t, mut ctx) = pyenv_ctx();
        ctx.pyenv_version = Some("9.9".to_string());
        let r = plan(&ctx, Mode::Exec, "tool", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!(
            r.lines,
            [
                "pyenv: version `9.9' is not installed (set by PYENV_VERSION environment variable)",
                "pyenv: tool: command not found"
            ]
        );
        assert_eq!((r.stderr, r.code), (true, 127));
    }

    #[test]
    fn upstream_shim_path_variables() {
        let (_t, mut ctx) = pyenv_ctx();
        exe(&ctx
            .versions_dir()
            .join("3.12.10")
            .join("bin")
            .join("python"));
        ctx.pyenv_version = Some("3.12.10".to_string());
        let env = ExecEnv {
            shim_path: Some("/opt/shims".into()),
            shim_paths: Some("/a".into()),
            ..ExecEnv::default()
        };
        let p = plan(&ctx, Mode::Exec, "python", vec![], &env).unwrap();
        assert_eq!(
            var(&p, "_PYENV_SHIM_PATHS_PYTHON"),
            Some(&Some(OsString::from("/opt/shims:/a")))
        );
        assert_eq!(var(&p, "_PYENV_SHIM_PATH"), Some(&None));
    }

    #[test]
    fn pip_like_commands() {
        let args = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        for c in [
            "pip",
            "pip3",
            "pip3.12",
            "easy_install",
            "easy_install3",
            "conda",
        ] {
            assert!(is_pip_like(c, &[]), "{c}");
        }
        assert!(is_pip_like(
            "python3",
            &args(&["-m", "pip", "install", "x"])
        ));
        for c in ["pipx", "pipenv", "black", "python"] {
            assert!(!is_pip_like(c, &args(&["-m", "venv"])), "{c}");
        }
    }

    fn win_ctx(versions: &[&str]) -> (tempfile::TempDir, Ctx) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("versions")).unwrap();
        for v in versions {
            fs::create_dir_all(root.join("versions").join(v)).unwrap();
        }
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let mut ctx = Ctx::for_test(Flavor::PyenvWin, &root, &tmp.path().join("work"));
        ctx.pathext = Some(".exe;.bat".into());
        (tmp, ctx)
    }

    #[test]
    fn win_child_path_matches_pyenv_win() {
        let (_t, mut ctx) = win_ctx(&[]);
        let shims = ctx.shims_dir().display().to_string();
        ctx.path = Some(format!("C:\\Windows;{shims};;\"C:\\q d\";{shims}\\").into());
        let names = ["3.7.7".to_string(), "3.8.9-win32".to_string()];
        let v = |n: &str, sub: &str| {
            let p = ctx.versions_dir().join(n);
            let p = if sub.is_empty() { p } else { p.join(sub) };
            p.display().to_string()
        };
        let expected = format!(
            "{};{};{};{};{};{};C:\\AppData\\Python\\Python38-32\\Scripts;C:\\Windows;C:\\q d;",
            v("3.7.7", ""),
            v("3.7.7", "Scripts"),
            v("3.7.7", "bin"),
            v("3.8.9-win32", ""),
            v("3.8.9-win32", "Scripts"),
            v("3.8.9-win32", "bin"),
        );
        assert_eq!(
            win_child_path(&ctx, &names, Some(OsStr::new("C:\\AppData"))),
            OsString::from(expected)
        );
    }

    /// The shims folder leaves the child's PATH however it is spelled: with a trailing `\`,
    /// in other letter case, or both (allowlist D-40).
    #[test]
    fn win_child_path_drops_the_shims_folder_in_any_spelling() {
        let (_t, mut ctx) = win_ctx(&[]);
        let shims = ctx.shims_dir().display().to_string();
        ctx.path = Some(
            format!(
                "C:\\Windows;{0}\\;{1};{2}\\",
                shims.to_ascii_uppercase(),
                shims.to_ascii_lowercase(),
                shims.to_ascii_uppercase()
            )
            .into(),
        );
        assert_eq!(
            win_child_path(&ctx, &[], None),
            OsString::from("C:\\Windows;")
        );
    }

    /// A program that is missing when it is started: `pyenv: <path>: <reason>` on stderr,
    /// without Rust's ` (os error N)`, and exit 127 (allowlist D-43). No `pyenv` command
    /// reaches this on its own (both look the file up first), so the plan is built here.
    #[test]
    fn a_program_missing_at_start_exits_127() {
        let (tmp, ctx) = win_ctx(&[]);
        let program = tmp.path().join("gone").join("tool.exe");
        let plan = LaunchPlan {
            program: program.clone(),
            args: vec![],
            raw_tail: None,
            env: vec![],
            warnings: vec![],
            wait: true,
        };
        let r = run(&plan, &ctx, None).unwrap_err();
        assert_eq!((r.stderr, r.code), (true, 127));
        let prefix = format!("pyenv: {}: ", program.display());
        assert!(
            r.lines.len() == 1 && r.lines[0].starts_with(&prefix),
            "{:?}",
            r.lines
        );
        assert!(!r.lines[0].contains("os error"), "{:?}", r.lines);
    }

    #[test]
    fn win_exec_plan() {
        let (_t, mut ctx) = win_ctx(&["3.9.1"]);
        let py = ctx.versions_dir().join("3.9.1").join("python.exe");
        exe(&py);
        ctx.pyenv_version = Some("3.9.1".to_string());
        let p = plan(&ctx, Mode::Exec, "python", vec![], &ExecEnv::default()).unwrap();
        assert_eq!(p.program, py);
        assert!(p.wait);
        assert_eq!(var(&p, "PYENV_VERSION"), None);
        let r = plan(&ctx, Mode::Exec, "nothing", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!(
            r.lines,
            [
                "'nothing' is not recognized as an internal or external command,",
                "operable program or batch file."
            ]
        );
        assert_eq!((r.stderr, r.code), (true, 1));
        ctx.pyenv_version = Some("3.9.1 3.7.7".to_string());
        let r = plan(&ctx, Mode::Exec, "python", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!(r.lines, WIN_EXEC_NO_VERSION);
    }

    #[test]
    fn io_reasons_drop_the_os_error_suffix() {
        let reason = io_reason(&std::io::Error::from_raw_os_error(2));
        assert!(!reason.is_empty());
        assert!(!reason.contains("os error"), "{reason}");
        assert_eq!(io_reason(&std::io::Error::other("plain")), "plain");
    }

    /// Review focus 5: Windows leaves `%1` in some messages for the program's name
    /// (`ERROR_BAD_EXE_FORMAT`: "%1 is not a valid Win32 application.").
    #[cfg(windows)]
    #[test]
    fn start_failure_names_the_program() {
        let err = std::io::Error::from_raw_os_error(193);
        let reason = start_failure_reason(std::path::Path::new(r"C:\v\bad.exe"), &err);
        assert!(!reason.contains("%1"), "{reason}");
        assert!(reason.contains("bad.exe"), "{reason}");
    }

    #[test]
    fn win_shim_plan_uses_which() {
        let (_t, mut ctx) = win_ctx(&["3.8.2", "3.9.1"]);
        exe(&ctx.versions_dir().join("3.8.2").join("python38.exe"));
        ctx.pyenv_version = Some("3.9.1".to_string());
        let r = plan(&ctx, Mode::Shim, "python38", vec![], &ExecEnv::default()).unwrap_err();
        assert_eq!((r.stderr, r.code), (false, 127));
        assert_eq!(r.lines[0], "pyenv: python38: command not found");
    }
}
