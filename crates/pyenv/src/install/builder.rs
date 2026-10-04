//! python-build's Linux source build (docs/parity/pyenv-m2-reference.md "python-build"),
//! staged through the install transaction (spec §9.3, plan Decision 4).

use super::archive::{extract, Kind};
use super::checksum::sha256_of_fragment;
use super::defs::{patches_for, Definition, Fetch, Found, Origin, Package};
use super::fetch::{Check, FetchRequest, Fetcher};
use super::log::{failed_block, spawn, BuildLog};
use super::txn::Txn;
use super::verify::plan as verify_plan;
use super::{interrupted, InstallError};
use std::ffi::OsString;
use std::io::IsTerminal;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

pub struct Options {
    pub keep: bool,
    pub verbose: bool,
    pub debug: bool,
    /// `-p`: the patch read from stdin, for the first `Python-*` package.
    pub stdin_patch: Option<Vec<u8>>,
}

pub struct Job<'a> {
    pub found: &'a Found,
    pub definition: &'a Definition,
    /// The final prefix, `versions/<name>`.
    pub prefix: PathBuf,
    pub opts: Options,
    /// The build's whole environment, in order (a later entry wins). Every child process
    /// gets exactly this, minus the variables python-build unsets; lookups read it too.
    pub env: &'a [(OsString, OsString)],
    pub fetcher: &'a Fetcher,
}

type R<T> = Result<T, InstallError>;

/// `X.Y` from a definition name: `3.12.10` → `3.12`, `3.13-dev` → `3.13`, `3.13.0t` → `3.13`.
pub fn xy_of(name: &str) -> Option<String> {
    let mut parts = name.split(|c: char| !c.is_ascii_digit());
    let x = parts.next().filter(|s| !s.is_empty())?;
    let rest = &name[x.len()..];
    let rest = rest.strip_prefix('.')?;
    let y: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!y.is_empty()).then(|| format!("{x}.{y}"))
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `YYYYmmddHHMMSS.<pid>` in UTC (upstream uses local time; only file names differ).
fn seed() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}{:02}.{}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        std::process::id()
    )
}

fn tmp_dir(env: &[(OsString, OsString)]) -> Result<PathBuf, String> {
    let raw = get(env, "TMPDIR")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/tmp".into());
    let tmp = PathBuf::from(raw.trim_end_matches('/'));
    let shown = tmp.display().to_string();
    let probe = tmp.join(format!("python-build-test.{}", std::process::id()));
    let made =
        std::fs::create_dir_all(&tmp).and_then(|_| std::fs::write(&probe, "#!/bin/sh\nexit 0\n"));
    if made.is_err() {
        return Err(format!(
            "python-build: TMPDIR={shown} is set to a non-accessible location"
        ));
    }
    let _ = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755));
    let ran = spawn(&mut command(&probe, env))
        .and_then(|mut c| c.wait())
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&probe);
    if !ran {
        return Err(format!("python-build: TMPDIR={shown} cannot hold executables (partition possibly mounted with `noexec`)"));
    }
    Ok(tmp)
}

fn os_information(env: &[(OsString, OsString)]) -> String {
    if let Ok(o) = command("lsb_release", env).arg("-sir").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !s.is_empty() {
                return s;
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
        let get = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .map(|v| v.trim_matches('"').to_string())
        };
        if let (Some(n), Some(v)) = (get("NAME"), get("VERSION_ID")) {
            return format!("{n} {v}");
        }
    }
    command("uname", env)
        .arg("-sr")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The configure, make and install commands python-build composes (reference
/// "Environment python-build sets up" and "Build steps").
struct Plan {
    /// The final prefix, made absolute as python-build does (bin/python-build:2562-2563).
    prefix: PathBuf,
    /// Every child's environment: `job.env` plus the prefix-augmented `CPPFLAGS`, `LDFLAGS`
    /// and `LIBS` python-build exports process-wide (bin/python-build:1575-1578,2847-2848).
    env: Vec<(OsString, OsString)>,
    configure: Vec<String>,
    configure_env: Vec<(String, String)>,
    make: String,
    make_args: Vec<String>,
    install_args: Vec<String>,
    altinstall: bool,
    xy: Option<String>,
}

impl Plan {
    fn new(job: &Job, prefix: PathBuf) -> Result<Plan, String> {
        let env = job.env;
        let var = |k: &str| -> String {
            job.definition
                .vars
                .iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .or_else(|| get(env, k))
                .unwrap_or_default()
        };
        let p = prefix.display().to_string();
        let (conf_opts, py_conf_opts) = (var("CONFIGURE_OPTS"), var("PYTHON_CONFIGURE_OPTS"));
        let user = format!("{conf_opts} {py_conf_opts}");
        if user.contains("--enable-framework") {
            return Err(
                "python-build: framework installation is not supported outside of MacOS.".into(),
            );
        }
        if user.contains("--enable-universalsdk") {
            return Err(
                "python-build: universal installation is not supported outside of MacOS.".into(),
            );
        }
        let mut array: Vec<String> = Vec::new();
        if job.opts.debug {
            array.push("--with-pydebug".into());
        }
        if !user.contains("--disable-shared") {
            array.push("--enable-shared".into());
        }
        array.push(format!("--libdir={p}/lib"));
        let name = &job.found.name;
        // Upstream's `2.*|3.0*|3.1*|3.2*`, without catching `3.10`–`3.19`.
        let old = name.starts_with("2.")
            || ["3.0", "3.1", "3.2"].iter().any(|v| {
                name == v
                    || name.starts_with(&format!("{v}."))
                    || name.starts_with(&format!("{v}-"))
            });
        if old && !py_conf_opts.contains("--enable-unicode=") {
            array.push("--enable-unicode=ucs4".into());
        }
        if !var("PYTHON_BUILD_FREE_THREADING").is_empty() {
            array.push("--disable-gil".into());
        }
        if !user.contains("--with-ensurepip") {
            // CPython's own ensurepip under DESTDIR would write shebangs into the build tree;
            // rpyenv runs ensurepip at the final prefix instead (plan, Task 5).
            array.push("--with-ensurepip=no".into());
        }
        let shared =
            array.iter().any(|a| a == "--enable-shared") || user.contains("--enable-shared");
        let user_ld = var("LDFLAGS");
        let rpath = if shared && !user_ld.contains("-rpath=") {
            format!("-Wl,-rpath,{p}/lib")
        } else {
            String::new()
        };
        let ldflags = join(&[&format!("-L{p}/lib"), &rpath, &user_ld]);
        let libs = join(&[&format!("-L{p}/lib"), &rpath, &var("LIBS")]);
        let cppflags = join(&[&format!("-I{p}/include"), &var("CPPFLAGS")]);
        let py_cflags = if job.opts.debug {
            format!("-O0 {}", var("PYTHON_CFLAGS"))
        } else {
            var("PYTHON_CFLAGS")
        };
        // Configure only: `CFLAGS` gains `PYTHON_CFLAGS` (and is left alone without it);
        // `CPPFLAGS` and `LDFLAGS` gain their `PYTHON_` variants.
        let mut configure_env = Vec::new();
        if !py_cflags.trim().is_empty() {
            configure_env.push((
                "CFLAGS".to_string(),
                join(&[&var("CFLAGS"), py_cflags.trim_end()]),
            ));
        }
        configure_env.push((
            "CPPFLAGS".into(),
            join(&[&cppflags, &var("PYTHON_CPPFLAGS")]),
        ));
        configure_env.push(("LDFLAGS".into(), join(&[&ldflags, &var("PYTHON_LDFLAGS")])));
        let mut child_env = env.to_vec();
        for (k, v) in [("CPPFLAGS", cppflags), ("LDFLAGS", ldflags), ("LIBS", libs)] {
            child_env.push((k.into(), v.into()));
        }
        let configure_cmd = get(env, "PYTHON_CONFIGURE")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "./configure".into());
        let mut configure = words(&configure_cmd);
        configure.push(format!("--prefix={p}"));
        configure.extend(array);
        configure.extend(words(&conf_opts));
        configure.extend(words(&py_conf_opts));
        let make_opts = match (get(env, "MAKEOPTS"), get(env, "MAKE_OPTS")) {
            (Some(v), _) => v,
            (None, Some(v)) => v,
            (None, None) => format!(
                "-j {}",
                std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(2)
            ),
        };
        let mut make_args = words(&make_opts);
        make_args.extend(words(&var("PYTHON_MAKE_OPTS")));
        let target = get(env, "PYTHON_MAKE_INSTALL_TARGET")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "install".into());
        let mut install_args = vec![target.clone()];
        install_args.extend(words(&var("MAKE_INSTALL_OPTS")));
        install_args.extend(words(&var("PYTHON_MAKE_INSTALL_OPTS")));
        Ok(Plan {
            prefix,
            env: child_env,
            configure,
            configure_env,
            make: get(env, "MAKE")
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "make".into()),
            make_args,
            install_args,
            altinstall: target.contains("altinstall"),
            xy: xy_of(name),
        })
    }
}

/// The variables python-build unsets before building (reference step 7).
const UNSET: [&str; 4] = [
    "PIP_REQUIRE_VENV",
    "PIP_REQUIRE_VIRTUALENV",
    "PYTHONHOME",
    "PYTHONPATH",
];

/// `k`'s value in `env` (the last entry wins), lossily as text; set-but-empty is `Some("")`.
pub fn get(env: &[(OsString, OsString)], k: &str) -> Option<String> {
    env.iter()
        .rev()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.to_string_lossy().into_owned())
}

/// A child process with exactly `env`, minus python-build's unset variables. `program` is
/// looked up on `env`'s `PATH`.
fn command(program: impl AsRef<std::ffi::OsStr>, env: &[(OsString, OsString)]) -> Command {
    let mut c = Command::new(program);
    c.env_clear();
    for (k, v) in env {
        if !UNSET.iter().any(|u| k == *u) {
            c.env(k, v);
        }
    }
    c
}

/// A child that died from Ctrl+C's SIGINT (2; `libc` isn't a dependency of this crate).
fn sigint(status: &ExitStatus) -> bool {
    status.signal() == Some(2)
}

fn logged(log: &BuildLog, cmd: &mut Command) -> R<()> {
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    let status = log.run(cmd).map_err(|e| {
        log.line(&format!("{cmd:?}: {e}"));
        InstallError::Failed
    })?;
    if interrupted() || sigint(&status) {
        return Err(InstallError::Interrupted);
    }
    if status.success() {
        Ok(())
    } else {
        Err(InstallError::Failed)
    }
}

/// The patches python-build applies to `package`: `<definition dir>/patches/<def>/<package>/*`,
/// sorted by name; the built-in ones for vendored definitions.
pub fn patch_files(found: &Found, package: &str) -> Vec<(String, Vec<u8>)> {
    let def = &found.name;
    let dir = match &found.origin {
        Origin::Builtin => return patches_for(def, package),
        Origin::Dir(d) => d.clone(),
        Origin::Path(p) => p.parent().map(Path::to_path_buf).unwrap_or_default(),
    };
    let pdir = dir.join("patches").join(def).join(package);
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(&pdir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.path().is_file())
                .filter_map(|e| {
                    Some((
                        e.file_name().to_string_lossy().into_owned(),
                        std::fs::read(e.path()).ok()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `apply_patch`: `patch -p0|-p1 --force -i <tmp>` in the package directory, output to the log,
/// the temporary file removed afterwards (plan Decision 5).
fn apply_patch(
    log: &BuildLog,
    dir: &Path,
    tmp: &Path,
    text: &[u8],
    env: &[(OsString, OsString)],
) -> R<()> {
    let file = tmp.join(format!("python-patch.{}", std::process::id()));
    std::fs::write(&file, text).map_err(|_| InstallError::Failed)?;
    let level = if String::from_utf8_lossy(text)
        .lines()
        .any(|l| l.starts_with("diff --git a/"))
    {
        "-p1"
    } else {
        "-p0"
    };
    let r = logged(
        log,
        command("patch", env)
            .current_dir(dir)
            .args([level, "--force", "-i"])
            .arg(&file),
    );
    let _ = std::fs::remove_file(&file);
    r
}

fn symlink_version_suffix(prefix: &Path, plan: &Plan) {
    if plan.altinstall {
        return;
    }
    let bin = prefix.join("bin");
    let Ok(rd) = std::fs::read_dir(&bin) else {
        return;
    };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let Some(version_bin) = names
        .iter()
        .rfind(|n| n.starts_with("python") && n.ends_with(|c: char| c.is_ascii_digit()))
    else {
        return;
    };
    let suffix = version_bin["python".len()..].to_string();
    if suffix.is_empty() {
        return;
    }
    for name in &names {
        let link = if *name == format!("python{suffix}-config") {
            "python-config".to_string()
        } else if let Some(s) = name.strip_suffix(&format!("-{suffix}")) {
            s.to_string()
        } else if let Some(s) = name.strip_suffix(&suffix) {
            s.to_string()
        } else {
            continue;
        };
        if !link.is_empty() && bin.join(&link).symlink_metadata().is_err() {
            let _ = std::os::unix::fs::symlink(name, bin.join(&link));
        }
    }
}

fn colorize(word: &str) -> String {
    if std::io::stderr().is_terminal() {
        format!("\x1b[1m{word}\x1b[m")
    } else {
        word.to_string()
    }
}

fn verify(job: &Job, plan: &Plan, step: &str, python: &Path, say: &mut dyn FnMut(&str)) -> R<()> {
    let (xy, checks) = verify_plan(step)
        .ok_or_else(|| InstallError::Message(format!("rpyenv cannot run build step `{step}'")))?;
    symlink_version_suffix(&plan.prefix, plan);
    let exe = plan.prefix.join("bin").join(format!("python{xy}"));
    if std::fs::metadata(&exe)
        .map(|m| m.permissions().mode() & 0o111 == 0)
        .unwrap_or(true)
    {
        say(&format!(
            "{}: invalid Python executable: {}",
            colorize("ERROR"),
            exe.display()
        ));
        say("");
        say("The python-build could not find proper executable of Python after successful build.");
        say("Please open an issue for future improvements.");
        say("https://github.com/pyenv/pyenv/issues");
        return Err(InstallError::Failed);
    }
    let display = get(job.env, "DISPLAY").is_some_and(|d| !d.is_empty());
    for c in checks {
        if c.needs_display && !display {
            continue;
        }
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        let status = command(python, &plan.env)
            .args(["-c", &format!("import {}", c.module)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status();
        if interrupted() || status.as_ref().is_ok_and(sigint) {
            return Err(InstallError::Interrupted);
        }
        let ok = status.is_ok_and(|s| s.success());
        if ok {
            continue;
        }
        let extra = c.extra.map(|e| format!(" {e}")).unwrap_or_default();
        if c.fatal {
            say(&format!(
                "{}: The Python {} extension was not compiled. Missing the {}?",
                colorize("ERROR"),
                c.module,
                c.lib
            ));
            say("");
            say("Please consult to the Wiki page to fix the problem.");
            say("https://github.com/pyenv/pyenv/wiki/Common-build-problems");
            say("");
            return Err(InstallError::Failed);
        }
        say(&format!(
            "{}: The Python {} extension was not compiled{extra}. Missing the {}?",
            colorize("WARNING"),
            c.module,
            c.lib
        ));
    }
    Ok(())
}

fn fix_directory_permissions(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if let Ok(m) = std::fs::symlink_metadata(&p) {
            if m.is_dir() {
                let mode = m.permissions().mode();
                if mode & 0o022 != 0 {
                    let _ = std::fs::set_permissions(
                        &p,
                        std::fs::Permissions::from_mode(mode & !0o022),
                    );
                }
                fix_directory_permissions(&p);
            }
        }
    }
}

/// Builds every package of the definition, in order, and places the version through `txn`
/// (not committed). Prints python-build's progress lines through `say` (stderr).
pub fn run(job: &Job, txn: &mut Txn, say: &mut dyn FnMut(&str)) -> R<()> {
    // A relative PYENV_ROOT stays relative in rpyenv-core; configure, the rpath and DESTDIR
    // need it absolute, as python-build makes it (bin/python-build:2562-2563).
    let absolute = |p: &Path| {
        std::path::absolute(p).map_err(|e| {
            InstallError::Message(format!("pyenv: cannot resolve {}: {e}", p.display()))
        })
    };
    let prefix = absolute(&job.prefix)?;
    let target = absolute(&txn.target())?;
    if prefix != target {
        return Err(InstallError::Message(format!(
            "pyenv: the build prefix {} is not the install target {}",
            prefix.display(),
            target.display()
        )));
    }
    let tmp = tmp_dir(job.env);
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    let tmp = tmp.map_err(|m| {
        say(&m);
        InstallError::Failed
    })?;
    let seed = seed();
    let log_path = tmp.join(format!("python-build.{seed}.log"));
    let build_path = get(job.env, "PYTHON_BUILD_BUILD_PATH")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| tmp.join(format!("python-build.{seed}")));
    let plan = Plan::new(job, prefix).map_err(|m| {
        say(&m);
        InstallError::Failed
    })?;
    if let Some(home) = get(job.env, "HOME") {
        let cfg = Path::new(&home).join(".pydistutils.cfg");
        if cfg.exists() {
            say(&format!(
                "{}: Please make sure you remove any previous custom paths from your {} file.",
                colorize("WARNING"),
                cfg.display()
            ));
        }
    }
    let log = BuildLog::open(&log_path, job.opts.verbose).map_err(|e| {
        InstallError::Message(format!("pyenv: cannot write {}: {e}", log_path.display()))
    })?;
    std::fs::create_dir_all(&build_path).map_err(|e| {
        InstallError::Message(format!(
            "pyenv: cannot create {}: {e}",
            build_path.display()
        ))
    })?;
    let result = packages(job, txn, &plan, &build_path, &tmp, &log, say);
    match &result {
        Ok(()) => {
            if !job.opts.keep {
                let _ = std::fs::remove_dir_all(&build_path);
            }
        }
        // No report on Ctrl+C, but an empty build directory goes, as `rmdir` would.
        Err(InstallError::Interrupted) => {
            let _ = std::fs::remove_dir(&build_path);
        }
        // python-build's ERR trap reports every failing step once the build has started.
        Err(e) => {
            if let InstallError::Message(m) = e {
                say(m);
            }
            for l in failed_block(&os_information(job.env), &build_path, &log_path) {
                say(&l);
            }
        }
    }
    match result {
        Err(InstallError::Message(_)) => Err(InstallError::Failed),
        other => other,
    }
}

fn packages(
    job: &Job,
    txn: &mut Txn,
    plan: &Plan,
    build_path: &Path,
    tmp: &Path,
    log: &BuildLog,
    say: &mut dyn FnMut(&str),
) -> R<()> {
    let python = plan
        .prefix
        .join("bin")
        .join(format!("python{}", plan.xy.clone().unwrap_or_default()));
    let mut stdin_patch = job.opts.stdin_patch.clone();
    for pkg in &job.definition.packages {
        match pkg.condition.as_deref() {
            None => {}
            // macOS-only bundled builds: false on Linux (reference, DSL table).
            Some("has_broken_mac_openssl" | "has_broken_mac_readline") => continue,
            Some(other) => {
                return Err(InstallError::Message(format!(
                    "rpyenv cannot evaluate `--if {other}' for {}",
                    pkg.name
                )))
            }
        }
        let src = fetch_package(job, plan, pkg, build_path, log, say)?;
        say(&format!("Installing {}...", pkg.name));
        let patch = if pkg.name.starts_with("Python-") && stdin_patch.is_some() {
            stdin_patch.take()
        } else {
            let files = patch_files(job.found, &pkg.name);
            (!files.is_empty()).then(|| files.into_iter().flat_map(|(_, b)| b).collect())
        };
        if let Some(text) = patch {
            apply_patch(log, &src, tmp, &text, &plan.env)?;
        }
        if job.definition.require_gcc && get(job.env, "CC").is_none() {
            // Only the 2.1–2.4 definitions; a missing gcc fails at configure with its own error.
            log.line("require_gcc: using gcc from PATH");
        }
        for step in &pkg.steps {
            run_step(job, txn, plan, pkg, step, &src, &python, log, say)?;
        }
        if txn.placed() {
            fix_directory_permissions(&plan.prefix);
        }
        say(&format!(
            "Installed {} to {}",
            pkg.name,
            plan.prefix.display()
        ));
    }
    if !txn.placed() {
        return Err(InstallError::Message(format!(
            "rpyenv: {} installed nothing",
            job.found.name
        )));
    }
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    Ok(())
}

fn fetch_package(
    job: &Job,
    plan: &Plan,
    pkg: &Package,
    build_path: &Path,
    log: &BuildLog,
    say: &mut dyn FnMut(&str),
) -> R<PathBuf> {
    match &pkg.fetch {
        Fetch::Tarball { url, fragment } => {
            let sha =
                sha256_of_fragment(url, fragment.as_deref()).map_err(InstallError::Message)?;
            let kind = Kind::of_url(url);
            let file_name = format!("{}{}", pkg.name, kind.ext());
            let req = FetchRequest {
                file_name,
                url: url.clone(),
                check: Check::Sha256(sha),
                dest_dir: build_path.to_path_buf(),
            };
            let mut w: &BuildLog = log;
            let archive = job.fetcher.fetch(&req, &mut w, say)?;
            let dir = extract(&archive, kind, build_path, &pkg.name).map_err(|e| {
                log.line(&e);
                InstallError::Failed
            })?;
            if !job.opts.keep {
                let _ = std::fs::remove_file(&archive);
            }
            Ok(dir)
        }
        Fetch::Git { url, reference } => {
            let dir = build_path.join(&pkg.name);
            let git = command("git", &plan.env)
                .arg("--version")
                .stdout(Stdio::null())
                .status();
            if interrupted() || git.as_ref().is_ok_and(sigint) {
                return Err(InstallError::Interrupted);
            }
            if git.is_err() {
                return Err(InstallError::Message(
                    "error: please install `git` and try again".into(),
                ));
            }
            say(&format!("Cloning {url}..."));
            if dir.is_dir() {
                logged(
                    log,
                    command("git", &plan.env).current_dir(&dir).args([
                        "fetch",
                        "--depth",
                        "1",
                        "origin",
                        &format!("+{reference}"),
                    ]),
                )?;
                logged(
                    log,
                    command("git", &plan.env).current_dir(&dir).args([
                        "checkout",
                        "-q",
                        "-B",
                        reference,
                        &format!("origin/{reference}"),
                    ]),
                )?;
            } else {
                logged(
                    log,
                    command("git", &plan.env).current_dir(build_path).args([
                        "clone", "--depth", "1", "--branch", reference, url, &pkg.name,
                    ]),
                )?;
            }
            Ok(dir)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_step(
    job: &Job,
    txn: &mut Txn,
    plan: &Plan,
    pkg: &Package,
    step: &str,
    src: &Path,
    python: &Path,
    log: &BuildLog,
    say: &mut dyn FnMut(&str),
) -> R<()> {
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    match step {
        "standard" => {
            let mut conf = command(&plan.configure[0], &plan.env);
            conf.current_dir(src).args(&plan.configure[1..]);
            for (k, v) in &plan.configure_env {
                conf.env(k, v);
            }
            logged(log, &mut conf)?;
            logged(
                log,
                command(&plan.make, &plan.env)
                    .current_dir(src)
                    .args(&plan.make_args),
            )?;
            let mut install = command(&plan.make, &plan.env);
            install.current_dir(src).args(&plan.install_args);
            let staging = pkg.name.starts_with("Python-") && !txn.placed();
            // Both absolute: DESTDIR is read relative to make's directory, the source tree.
            let stage = std::path::absolute(txn.stage_dir()).map_err(|e| {
                InstallError::Message(format!(
                    "pyenv: cannot resolve {}: {e}",
                    txn.stage_dir().display()
                ))
            })?;
            if staging {
                let mut destdir = OsString::from("DESTDIR=");
                destdir.push(&stage);
                install.arg(destdir);
            }
            logged(log, &mut install)?;
            if staging {
                let moved = plan
                    .prefix
                    .strip_prefix("/")
                    .map_err(std::io::Error::other)
                    .and_then(|rel| txn.place(&stage.join(rel)));
                moved.map_err(|e| {
                    InstallError::Message(format!(
                        "pyenv: cannot move the build into {}: {e}",
                        plan.prefix.display()
                    ))
                })?;
            }
            Ok(())
        }
        s if s.starts_with("verify_py") => verify(job, plan, s, python, say),
        "ensurepip" | "ensurepip_lt21" => {
            let isolation = if step == "ensurepip" { "-I" } else { "-s" };
            let mut c = command(python, &plan.env);
            c.args([isolation, "-m", "ensurepip"]);
            if plan.altinstall {
                c.arg("--altinstall");
            }
            let status = c
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if interrupted() || status.as_ref().is_ok_and(sigint) {
                return Err(InstallError::Interrupted);
            }
            let ok = status.is_ok_and(|s| s.success());
            if !ok {
                say("error: failed to install pip via ensurepip");
                return Err(InstallError::Failed);
            }
            symlink_version_suffix(&plan.prefix, plan);
            Ok(())
        }
        "copy_python_gdb" => {
            let gdb = src.join("Tools/gdb/libpython.py");
            let v = pkg.name.split('-').nth(1).and_then(xy_of);
            if let (true, Some(v)) = (gdb.exists(), v) {
                let _ = std::fs::copy(
                    &gdb,
                    plan.prefix.join("bin").join(format!("python{v}-gdb.py")),
                );
            }
            Ok(())
        }
        "python" => logged(
            log,
            command(python, &plan.env)
                .current_dir(src)
                .args(["setup.py", "install"]),
        ),
        other => Err(InstallError::Message(format!(
            "rpyenv cannot run build step `{other}'"
        ))),
    }
}
