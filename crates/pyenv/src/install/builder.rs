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
    /// The build store's libraries for this version (M8 design §6.3), when the build route
    /// supplied any.
    pub store: Option<&'a super::store::Linked>,
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

/// The `-I` flag that lets CPython's configure find `panel.h` where the system keeps it only
/// under `ncursesw/` (openSUSE Tumbleweed; Debian, Fedora and Arch also have it at the top):
/// without it `curses.panel` is silently not built. `minor`: the CPython 3 minor of the
/// definition's `verify_py3*` step (`stdlib::minor_of_step`), `None` for anything else; the flag
/// is only for CPython 3 on Linux. The build and the check before it both ask here, so they
/// cannot decide differently. `exists` tells whether a path exists.
pub(crate) fn panel_include(
    minor: Option<u32>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<&'static str> {
    if !(cfg!(target_os = "linux") && minor.is_some()) {
        return None;
    }
    (!exists("/usr/include/panel.h") && exists("/usr/include/ncursesw/panel.h"))
        .then_some("-I/usr/include/ncursesw")
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
    /// `exists`: whether a path exists (injected for `panel_include`).
    fn new(job: &Job, prefix: PathBuf, exists: &dyn Fn(&str) -> bool) -> Result<Plan, String> {
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
        // The store's flags come after every flag of the user's, `PYTHON_` variants included
        // (plan ruling P8): the compiler and linker take the first match.
        let store = job.store;
        let store_ld = store.map(|s| s.ldflags.join(" ")).unwrap_or_default();
        let mut store_cpp = store.map(|s| s.cppflags.join(" ")).unwrap_or_default();
        // Linux, CPython 3 (the definition has a `verify_py3*` step, as the check uses): configure
        // looks for `panel.h` with the flags it is given only.
        let minor = job
            .definition
            .packages
            .iter()
            .flat_map(|p| p.steps.iter())
            .find_map(|s| super::stdlib::minor_of_step(s));
        if let Some(flag) = panel_include(minor, exists) {
            store_cpp = join(&[&store_cpp, flag]);
        }
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
            join(&[&cppflags, &var("PYTHON_CPPFLAGS"), &store_cpp]),
        ));
        configure_env.push((
            "LDFLAGS".into(),
            join(&[&ldflags, &var("PYTHON_LDFLAGS"), &store_ld]),
        ));
        let mut child_env = env.to_vec();
        let cppflags = join(&[&cppflags, &store_cpp]);
        let ldflags = join(&[&ldflags, &store_ld]);
        for (k, v) in [("CPPFLAGS", cppflags), ("LDFLAGS", ldflags), ("LIBS", libs)] {
            child_env.push((k.into(), v.into()));
        }
        if let Some(s) = store {
            if !s.pkg_config_path.is_empty() {
                let mut v: Vec<String> = s
                    .pkg_config_path
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                v.extend(get(env, "PKG_CONFIG_PATH").filter(|x| !x.is_empty()));
                child_env.push(("PKG_CONFIG_PATH".into(), v.join(":").into()));
            }
            if let Some(bin) = &s.path {
                let path = get(env, "PATH").unwrap_or_default();
                child_env.push(("PATH".into(), format!("{}:{path}", bin.display()).into()));
            }
            for (k, v) in &s.configure_env {
                if var(k).is_empty() {
                    configure_env.push((k.clone(), v.clone()));
                }
            }
        }
        let configure_cmd = get(env, "PYTHON_CONFIGURE")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "./configure".into());
        let mut configure = words(&configure_cmd);
        configure.push(format!("--prefix={p}"));
        configure.extend(array);
        // Before the user's options, which win (ruling P8).
        if let Some(s) = store {
            configure.extend(s.configure.iter().cloned());
        }
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

pub(crate) fn logged(log: &BuildLog, cmd: &mut Command) -> R<()> {
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
pub(crate) fn apply_patch(
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

fn verify(
    job: &Job,
    plan: &Plan,
    pkg: &Package,
    step: &str,
    python: &Path,
    say: &mut dyn FnMut(&str),
) -> R<()> {
    let (xy, checks) = verify_plan(step)
        .ok_or_else(|| InstallError::Message(format!("rpyenv cannot run build step `{step}'")))?;
    symlink_version_suffix(&plan.prefix, plan);
    // python-build checks PYTHON_BIN: `bin/python<X.Y>` for a CPython name, `bin/python` for a
    // name it gets no X.Y from (PyPy, M7a design §4.3).
    let exe = if plan.xy.is_some() {
        plan.prefix.join("bin").join(format!("python{xy}"))
    } else {
        python.to_path_buf()
    };
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
    // M8 design §4.4 is for CPython built from source (ruling R14): PyPy's and conda's
    // `verify_py*` steps keep upstream's checks only.
    let from_source = pkg.steps.iter().any(|s| s == "standard");
    if let Some(minor) = super::stdlib::minor_of_step(step).filter(|_| from_source) {
        stdlib_check(job, plan, minor, python, say)?;
    }
    Ok(())
}

/// The §4.4 CA check. On a failure it prints the file and the directory the built Python
/// looked in: `SSL_CERT_FILE` and `SSL_CERT_DIR` when set, else its OpenSSL's defaults.
const CA_CHECK: &str = "import os, ssl
p = ssl.get_default_verify_paths()
ok = (p.cafile and os.path.isfile(p.cafile)) or (p.capath and os.path.isdir(p.capath) and os.listdir(p.capath))
if not ok:
    print(os.environ.get(p.openssl_cafile_env, p.openssl_cafile))
    print(os.environ.get(p.openssl_capath_env, p.openssl_capath))
raise SystemExit(0 if ok else 1)
";

/// What §4.4 also checks when the build route supplied Tcl and Tk (final review I1).
pub(crate) struct StoreTcl<'a> {
    /// `<root>/cache/deps`: Tcl's library must not lie under it.
    pub cache: &'a Path,
    /// Whether a path exists (the filesystem, or a fake in tests).
    pub exists: &'a dyn Fn(&Path) -> bool,
    /// Makes a throwaway virtual environment from the built interpreter and starts Tcl in it:
    /// `Ok(false)` when Tcl doesn't start, `Err(why)` when the check itself couldn't run.
    pub in_venv: &'a dyn Fn() -> Result<bool, String>,
}

/// M8 design §4.4: every module of the table imports, CA certificates exist, Tcl runs.
/// `run(args)` runs the built interpreter: its success and its stdout.
pub(crate) fn stdlib_failures(
    run: &dyn Fn(&[&str]) -> (bool, String),
    minor: u32,
    store_tcl: Option<&StoreTcl>,
) -> Vec<String> {
    let mut out = Vec::new();
    for m in super::stdlib::modules(minor) {
        for i in m.imports {
            let code = format!("import {i}");
            if !run(&["-c", &code]).0 {
                out.push(format!("{} (import {i} failed; needs {})", m.name, m.lib));
                break;
            }
        }
    }
    let (ok, looked) = run(&["-c", CA_CHECK]);
    if !ok {
        // Name where Python looked (final review M3): a custom OpenSSL may look elsewhere than
        // the system's ca-certificates.
        let mut paths = looked.lines().map(str::trim).filter(|l| !l.is_empty());
        let place = match (paths.next(), paths.next()) {
            (Some(file), Some(dir)) => format!("no CA file {file} and no certificates in {dir}"),
            _ => "no CA file or directory".to_string(),
        };
        out.push(format!(
            "ssl certificates ({place}; needs {}, or set SSL_CERT_FILE or SSL_CERT_DIR)",
            super::stdlib::CA_KEY
        ));
    }
    let (tcl_ok, tcl_out) = run(&[
        "-c",
        "import tkinter; print(tkinter.Tcl().eval('info library'))",
    ]);
    if !tcl_ok {
        out.push("tkinter (Tcl doesn't start; needs tk)".into());
    } else if let Some(s) = store_tcl {
        // M8 design §4.4: a store-built Tcl reads its library from outside the build cache
        // (deleting the cache must not break it), and starts in a venv too (§6.3).
        let lib = tcl_out.trim();
        if Path::new(lib).starts_with(s.cache) {
            out.push(format!(
                "tkinter (Tcl's library {lib} is in the build cache; needs tk)"
            ));
        } else if !(s.exists)(Path::new(lib)) {
            out.push(format!(
                "tkinter (Tcl's library {lib} doesn't exist; needs tk)"
            ));
        }
        match (s.in_venv)() {
            Ok(true) => {}
            Ok(false) => {
                out.push("tkinter (Tcl doesn't start in a virtual environment; needs tk)".into())
            }
            Err(why) => out.push(format!(
                "tkinter (could not check Tcl in a virtual environment: {why})"
            )),
        }
    }
    out
}

/// Makes a new folder under `base` (mode 0700) named by the first of `names` that doesn't exist
/// yet. `create_dir` fails on an existing path, so a folder someone else made, or a link, is
/// never used.
pub(crate) fn private_dir(
    base: &Path,
    names: &mut dyn Iterator<Item = String>,
) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let mut tried = 0;
    for n in names {
        let path = base.join(n);
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => tried += 1,
            Err(e) => return Err(format!("cannot create {}: {e}", path.display())),
        }
    }
    Err(format!(
        "{tried} folder names under {} were taken already",
        base.display()
    ))
}

/// Runs the §4.4 check on the built interpreter and decides what a failure means: an error
/// (strict), a warning (RPYENV_SKIP_PREFLIGHT=1, ruling P1) or the `[c]` summary.
fn stdlib_check(
    job: &Job,
    plan: &Plan,
    minor: u32,
    python: &Path,
    say: &mut dyn FnMut(&str),
) -> R<()> {
    let sigint_seen = std::cell::Cell::new(false);
    let run = |args: &[&str]| -> (bool, String) {
        if interrupted() {
            return (true, String::new());
        }
        let out = command(python, &plan.env)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        if out.as_ref().is_ok_and(|o| sigint(&o.status)) {
            sigint_seen.set(true);
        }
        match out {
            Ok(o) => (
                o.status.success(),
                String::from_utf8_lossy(&o.stdout).into_owned(),
            ),
            Err(_) => (false, String::new()),
        }
    };
    // A throwaway venv from the built interpreter, where Tcl must start as well (§4.4).
    let in_venv = || -> Result<bool, String> {
        if interrupted() {
            return Ok(true);
        }
        let tmp = get(job.env, "TMPDIR")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/tmp".into());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let mut names = (0..5u32).map(|i| {
            format!(
                "rpyenv-tcl-venv.{}.{:x}",
                std::process::id(),
                nanos.wrapping_add(i)
            )
        });
        let venv = private_dir(Path::new(&tmp), &mut names)?;
        let quiet = |c: &mut Command| -> bool {
            let st = c
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if st.as_ref().is_ok_and(sigint) {
                sigint_seen.set(true);
            }
            st.is_ok_and(|s| s.success())
        };
        let ok = quiet(
            command(python, &plan.env)
                .args(["-m", "venv", "--without-pip"])
                .arg(&venv),
        ) && quiet(
            command(venv.join("bin").join("python"), &plan.env)
                .args(["-c", "import tkinter; tkinter.Tcl()"]),
        );
        let _ = std::fs::remove_dir_all(&venv);
        Ok(ok)
    };
    // These checks apply only when the store supplied Tcl/Tk (its script folders).
    let exists = |p: &Path| p.exists();
    let cache = job
        .store
        .filter(|s| !s.scripts.is_empty())
        .and_then(|s| s.entries.iter().find(|e| e.key == "tcl"))
        .and_then(|e| e.dir.parent());
    let store_tcl = cache.map(|cache| StoreTcl {
        cache,
        exists: &exists,
        in_venv: &in_venv,
    });
    let fails = stdlib_failures(&run, minor, store_tcl.as_ref());
    if interrupted() || sigint_seen.get() {
        return Err(InstallError::Interrupted);
    }
    if fails.is_empty() {
        return Ok(());
    }
    let name = plan
        .prefix
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // RPYENV_SKIP_PREFLIGHT=1 always gives P1's warning, the `[c]` summary included (review M7).
    let skip = get(job.env, "RPYENV_SKIP_PREFLIGHT").as_deref() == Some("1");
    // `continue`: the prompt's `[c]`, chosen by the user (rpyenv sets it; a user's is dropped).
    let mode = get(job.env, "RPYENV_DEPS_MODE");
    if !skip && mode.as_deref() == Some("continue") {
        // The `[c]` summary (M8 design §5.1); the build counts as installed. Root is read
        // the way `pyenv install` reads it: `id -u` on the build's PATH.
        let root = command("id", job.env)
            .arg("-u")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0");
        let os_path = if cfg!(debug_assertions) {
            get(job.env, "RPYENV_TEST_OS_RELEASE")
        } else {
            None
        }
        .unwrap_or_else(|| "/etc/os-release".into());
        let os = std::fs::read_to_string(os_path).ok();
        say(&continue_summary(&name, &fails, minor, os.as_deref(), root));
        return Ok(());
    }
    let lead = if skip { "pyenv: warning: " } else { "pyenv: " };
    say(&format!(
        "{lead}{name} was built without: {}",
        fails.join(", ")
    ));
    if skip {
        return Ok(());
    }
    say("Install the missing libraries (pyenv install asks how), or set RPYENV_SKIP_PREFLIGHT=1 to keep such a build.");
    Err(InstallError::Failed)
}

/// The summary after `[c]` (M8 design §5.1): what the build lacks and the command that adds it.
fn continue_summary(
    name: &str,
    fails: &[String],
    minor: u32,
    os_release: Option<&str>,
    root: bool,
) -> String {
    use super::{deps, preflight, stdlib};
    let mut names: Vec<&str> = Vec::new();
    let mut libs: Vec<preflight::Dep> = Vec::new();
    for f in fails {
        let n = f.split(" (").next().unwrap_or(f);
        if !names.contains(&n) {
            names.push(n);
        }
        let lib = if n == "ssl certificates" {
            Some(("ssl certificates", stdlib::CA_KEY))
        } else {
            stdlib::modules(minor)
                .into_iter()
                .find(|m| m.name == n)
                .map(|m| (m.name, m.lib))
        };
        if let Some((dn, lib)) = lib {
            if !libs.iter().any(|d| d.lib == lib) {
                // No probe result here: on Fedora 42+ an older CPython's Tk is the Tk 8
                // package (`deps::packages`), as the check before the build says.
                let tk9 = lib == "tk" && minor <= 12 && os_release.is_some_and(deps::fedora_tk9);
                libs.push(preflight::Dep {
                    name: dn,
                    required: false,
                    missing: if tk9 {
                        preflight::Missing::TooNew {
                            found: "9".into(),
                            need: "8.5 or 8.6",
                        }
                    } else {
                        preflight::Missing::Certificates
                    },
                    lib,
                });
            }
        }
    }
    let list = names.join(", ");
    let add = os_release
        .and_then(deps::Manager::from_os_release)
        .map(|m| {
            let pkgs = deps::packages(m, &libs);
            deps::install_argv(m, &pkgs, !root, false).join(" ")
        })
        .unwrap_or_else(|| format!("install the development packages for {list}"));
    // RHEL-family systems: the `dnf install` above needs CRB first (ruling R15).
    let crb = os_release
        .and_then(|os| deps::crb_line(os, root))
        .map(|l| format!("\n  {l}"))
        .unwrap_or_default();
    format!(
        "pyenv: {name} was built without: {list}
  Add them: {add}, then pyenv install -f {name}{crb}"
    )
}

/// `cp -fR src/. dst`: directories, regular files with their modes, and links as links.
pub(crate) fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let (from, to) = (e.path(), dst.join(e.file_name()));
        let t = e.file_type()?;
        if t.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&from)?, &to)?;
        } else if t.is_dir() {
            copy_tree(&from, &to)?;
            std::fs::set_permissions(&to, std::fs::metadata(&from)?.permissions())?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// The links `build_package_pypy` adds: for each regular, executable, non-link file in `bin/`,
/// in name order, `libpypy*` gets `lib/<name>` → `../bin/<name>` and `pypy*` becomes the target
/// of `bin/python` (the last one wins, as with `ln -fs` in a glob loop).
fn link_pypy(prefix: &Path) -> std::io::Result<()> {
    let (bin, lib) = (prefix.join("bin"), prefix.join("lib"));
    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&lib)?;
    let mut names: Vec<String> = std::fs::read_dir(&bin)?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let replace = |at: &Path, target: &str| -> std::io::Result<()> {
        if at.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
            std::fs::remove_file(at)?;
        }
        std::os::unix::fs::symlink(target, at)
    };
    for n in names {
        let m = std::fs::symlink_metadata(bin.join(&n))?;
        if !m.file_type().is_file() || m.permissions().mode() & 0o111 == 0 {
            continue;
        }
        if n.starts_with("libpypy") {
            replace(&lib.join(&n), &format!("../bin/{n}"))?;
        } else if n.starts_with("pypy") {
            replace(&bin.join("python"), &n)?;
        }
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
    let plan = Plan::new(job, prefix, &|p| Path::new(p).exists()).map_err(|m| {
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
            if let Some(s) = job.store {
                let file = plan.prefix.join(super::record::FILE);
                if let Err(e) =
                    std::fs::write(&file, super::record::text(&job.found.name, &s.entries))
                {
                    say(&format!(
                        "pyenv: warning: cannot write {}: {e}",
                        file.display()
                    ));
                }
            }
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
        Fetch::Script { url, fragment } => {
            let sha =
                sha256_of_fragment(url, fragment.as_deref()).map_err(InstallError::Message)?;
            let req = FetchRequest {
                file_name: format!("{}.sh", pkg.name),
                url: url.clone(),
                check: Check::Sha256(sha),
                dest_dir: build_path.to_path_buf(),
            };
            let mut w: &BuildLog = log;
            Ok(job.fetcher.fetch(&req, &mut w, say)?)
        }
        Fetch::Git {
            url,
            reference,
            sha1,
        } => {
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
            if let Some(want) = sha1 {
                verify_checkout(&dir, reference, want, &plan.env, say)?;
            }
            Ok(dir)
        }
    }
}

/// pyenv 2.8.9's `fetch_git` check of a `<ref>:<sha1>` reference: the checkout's commit must
/// be `want`, or, when HEAD is exactly the annotated tag `reference`, the tag object may be.
fn verify_checkout(
    dir: &Path,
    reference: &str,
    want: &str,
    env: &[(OsString, OsString)],
    say: &mut dyn FnMut(&str),
) -> R<()> {
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    let git = |args: &[&str]| {
        command("git", env)
            .current_dir(dir)
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
    };
    let tag = git(&["describe", "--exact-match"])
        .filter(|d| d.lines().any(|l| l == reference))
        .and_then(|_| git(&["rev-parse", reference]));
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    if commit == want || tag.as_deref() == Some(want) {
        return Ok(());
    }
    let mut msg = format!("error: ref `{reference}' SHA-1 mismatch: expected {want}, got {commit}");
    if let Some(t) = tag {
        msg.push_str(&format!("(commit) and {t}(tag)"));
    }
    say(&msg);
    Err(InstallError::Failed)
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
            // Tcl and Tk find their script folders beside the executable (M8 design §6.3).
            if staging {
                if let Some(s) = job.store {
                    for d in &s.scripts {
                        let to = plan
                            .prefix
                            .join("lib")
                            .join(d.file_name().unwrap_or_default());
                        copy_tree(d, &to).map_err(|e| {
                            InstallError::Message(format!(
                                "pyenv: cannot copy {} into {}: {e}",
                                d.display(),
                                to.display()
                            ))
                        })?;
                    }
                }
            }
            Ok(())
        }
        // python-build's `build_package_pypy` (M7a design §4.3): the binary tree is copied as it
        // is, then `bin/python` and the `lib/libpypy*` links are added, and the whole is placed.
        "pypy" => {
            let staged = txn.stage_dir().join("pypy");
            copy_tree(src, &staged)
                .and_then(|_| link_pypy(&staged))
                .map_err(|e| {
                    InstallError::Message(format!(
                        "pyenv: cannot copy {} into {}: {e}",
                        src.display(),
                        staged.display()
                    ))
                })?;
            if interrupted() {
                return Err(InstallError::Interrupted);
            }
            txn.place(&staged).map_err(|e| {
                InstallError::Message(format!(
                    "pyenv: cannot move the build into {}: {e}",
                    plan.prefix.display()
                ))
            })
        }
        // python-build's `build_package_anaconda` and `build_package_miniconda` (M7b design §4.3):
        // the installer writes straight into the final folder, which `place_empty` created with
        // the marker; `-f` lets it use the existing folder.
        "anaconda" | "miniconda" => {
            if !txn.placed() {
                txn.place_empty().map_err(|e| {
                    InstallError::Message(format!(
                        "pyenv: cannot create {}: {e}",
                        plan.prefix.display()
                    ))
                })?;
            }
            let vars = &job.definition.vars;
            let mut c = command("bash", &plan.env);
            c.arg(src).args(["-f", "-b", "-p"]).arg(&plan.prefix);
            // Not the caller's active conda env or Python settings (conda follows
            // CONDA_PREFIX), then the definition's own variables.
            super::conda::clean_env(&mut c, vars);
            logged(log, &mut c)?;
            if step == "miniconda" {
                // Upstream's workaround for pyenv issue #2070: pip, without upgrading conda.
                let conda = plan.prefix.join("bin").join("conda");
                let mut ver = command(&conda, &plan.env);
                super::conda::clean_env(&mut ver, &[]);
                let out = ver.arg("--version").stdin(Stdio::null()).output();
                if interrupted() {
                    return Err(InstallError::Interrupted);
                }
                let v = out
                    .ok()
                    .and_then(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .split_whitespace()
                            .nth(1)
                            .map(str::to_string)
                    })
                    .ok_or_else(|| {
                        InstallError::Message(format!(
                            "pyenv: {} --version gave no version",
                            conda.display()
                        ))
                    })?;
                let mut c = command(&conda, &plan.env);
                c.args(["install", "--yes", "pip", &format!("conda={v}")]);
                super::conda::clean_env(&mut c, vars);
                logged(log, &mut c)?;
            }
            Ok(())
        }
        s if s.starts_with("verify_py") => verify(job, plan, pkg, s, python, say),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdlib_failures_name_each_missing_import_and_the_certificates() {
        let fails = |args: &[&str]| -> (bool, String) {
            let ok = !(args[1] == "import _uuid" || args[1].contains("get_default_verify_paths"));
            (ok, String::new())
        };
        let f = stdlib_failures(&fails, 12, None);
        assert!(
            f.contains(&"uuid (import _uuid failed; needs libuuid)".to_string()),
            "{f:?}"
        );
        assert!(
            f.contains(
                &"ssl certificates (no CA file or directory; needs ca-certificates, or set SSL_CERT_FILE or SSL_CERT_DIR)".to_string()
            ),
            "{f:?}"
        );
        assert!(stdlib_failures(&|_| (true, String::new()), 12, None).is_empty());
    }

    /// Final review M3: a failed CA check names the file and directory the built Python looked
    /// in (a custom OpenSSL's, here) and the variables that point it elsewhere.
    #[test]
    fn a_missing_ca_names_where_python_looked() {
        let run = |args: &[&str]| -> (bool, String) {
            if args[1].contains("get_default_verify_paths") {
                (
                    false,
                    "/opt/openssl/ssl/cert.pem\n/opt/openssl/ssl/certs\n".into(),
                )
            } else {
                (true, String::new())
            }
        };
        assert_eq!(
            stdlib_failures(&run, 13, None),
            ["ssl certificates (no CA file /opt/openssl/ssl/cert.pem and no certificates in /opt/openssl/ssl/certs; needs ca-certificates, or set SSL_CERT_FILE or SSL_CERT_DIR)"]
        );
        // The script prints both places and honors the variables, as Python's own lookup does.
        for want in [
            "p.openssl_cafile_env, p.openssl_cafile",
            "p.openssl_capath_env, p.openssl_capath",
        ] {
            assert!(CA_CHECK.contains(want), "{want}");
        }
    }

    #[test]
    fn panel_h_only_under_ncursesw_adds_the_include_flag() {
        let only = |paths: &'static [&'static str]| move |p: &str| paths.contains(&p);
        // Tumbleweed: only the ncursesw folder has it.
        assert_eq!(
            panel_include(Some(12), &only(&["/usr/include/ncursesw/panel.h"])),
            Some("-I/usr/include/ncursesw")
        );
        // Debian, Fedora, Arch: the top-level header is there, and no flag is needed.
        assert_eq!(
            panel_include(
                Some(12),
                &only(&["/usr/include/panel.h", "/usr/include/ncursesw/panel.h"])
            ),
            None
        );
        // No ncurses development files at all.
        assert_eq!(panel_include(Some(12), &only(&[])), None);
        // Not CPython 3 (no `verify_py3*` step): never, wherever the header is.
        assert_eq!(
            panel_include(None, &only(&["/usr/include/ncursesw/panel.h"])),
            None
        );
    }

    /// A plan for 3.12.99 with the user's flags and a store; `verify`: the definition has a
    /// `verify_py312` step (a CPython 3 source build); `exists`: which files the system has.
    fn store_plan(verify: bool, exists: &dyn Fn(&str) -> bool) -> Plan {
        let found = Found {
            name: "3.12.99".into(),
            text: String::new(),
            origin: Origin::Builtin,
        };
        let mut definition = Definition::default();
        if verify {
            definition.packages.push(Package {
                name: "Python-3.12.99".into(),
                fetch: Fetch::Tarball {
                    url: "http://x/y.tgz".into(),
                    fragment: None,
                },
                steps: vec!["standard".into(), "verify_py312".into()],
                condition: None,
            });
        }
        let env: Vec<(OsString, OsString)> = [
            ("CPPFLAGS", "-I/user/cpp"),
            ("PYTHON_CPPFLAGS", "-I/user/pycpp"),
            ("LDFLAGS", "-L/user/ld"),
            ("PYTHON_LDFLAGS", "-L/user/pyld"),
            ("CONFIGURE_OPTS", "--user-conf"),
            ("PYTHON_CONFIGURE_OPTS", "--user-pyconf"),
            ("PKG_CONFIG_PATH", "/user/pc"),
            ("PATH", "/usr/bin"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let store = super::super::store::Linked {
            entries: Vec::new(),
            cppflags: vec!["-I/store/inc".into()],
            ldflags: vec!["-L/store/lib".into()],
            pkg_config_path: vec![PathBuf::from("/store/pc")],
            path: Some(PathBuf::from("/store/bin")),
            configure: vec!["--with-openssl=/store/ssl".into()],
            configure_env: Vec::new(),
            scripts: Vec::new(),
        };
        let fetcher = Fetcher::from_env(&|_| None, None);
        let job = Job {
            found: &found,
            definition: &definition,
            prefix: PathBuf::from("/r/versions/3.12.99"),
            opts: Options {
                keep: false,
                verbose: false,
                debug: false,
                stdin_patch: None,
            },
            env: &env,
            fetcher: &fetcher,
            store: Some(&store),
        };
        Plan::new(&job, job.prefix.clone(), exists).unwrap()
    }

    /// Plan ruling P8: every flag of the user's, `PYTHON_CPPFLAGS` and `PYTHON_LDFLAGS`
    /// included, comes before the store's (the compiler and linker take the first match); the
    /// store's configure arguments come before the user's options; the store's pkg-config
    /// folders and pkgconf's `bin/` come first.
    #[test]
    fn the_users_flags_come_before_the_stores() {
        let plan = store_plan(false, &|_| false);
        let conf = |k: &str| {
            plan.configure_env
                .iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let p = "/r/versions/3.12.99";
        assert_eq!(
            conf("CPPFLAGS"),
            format!("-I{p}/include -I/user/cpp -I/user/pycpp -I/store/inc")
        );
        assert_eq!(
            conf("LDFLAGS"),
            format!("-L{p}/lib -Wl,-rpath,{p}/lib -L/user/ld -L/user/pyld -L/store/lib")
        );
        // The children's environment has no `PYTHON_` variants; the store still comes last.
        assert_eq!(
            get(&plan.env, "CPPFLAGS").unwrap(),
            format!("-I{p}/include -I/user/cpp -I/store/inc")
        );
        assert_eq!(
            get(&plan.env, "LDFLAGS").unwrap(),
            format!("-L{p}/lib -Wl,-rpath,{p}/lib -L/user/ld -L/store/lib")
        );
        let at = |a: &str| plan.configure.iter().position(|x| x == a).unwrap();
        assert!(
            at("--with-openssl=/store/ssl") < at("--user-conf"),
            "{:?}",
            plan.configure
        );
        assert!(
            at("--user-conf") < at("--user-pyconf"),
            "{:?}",
            plan.configure
        );
        assert_eq!(
            get(&plan.env, "PKG_CONFIG_PATH").unwrap(),
            "/store/pc:/user/pc"
        );
        assert_eq!(get(&plan.env, "PATH").unwrap(), "/store/bin:/usr/bin");
    }

    // allowlist D-129
    #[test]
    fn panel_h_only_under_ncursesw_puts_the_include_flag_after_the_stores() {
        let only_under = |p: &str| p == "/usr/include/ncursesw/panel.h";
        let plan = store_plan(true, &only_under);
        let conf = plan
            .configure_env
            .iter()
            .rev()
            .find(|(n, _)| n == "CPPFLAGS")
            .map(|(_, v)| v.clone())
            .unwrap();
        let p = "/r/versions/3.12.99";
        assert_eq!(
            conf,
            format!("-I{p}/include -I/user/cpp -I/user/pycpp -I/store/inc -I/usr/include/ncursesw")
        );
        assert_eq!(
            get(&plan.env, "CPPFLAGS").unwrap(),
            format!("-I{p}/include -I/user/cpp -I/store/inc -I/usr/include/ncursesw")
        );
        // Not a CPython 3 build, or a system with the top-level header: no flag.
        let other = store_plan(false, &only_under);
        assert!(!get(&other.env, "CPPFLAGS").unwrap().contains("ncursesw"));
        let top = store_plan(true, &|_: &str| true);
        assert!(!get(&top.env, "CPPFLAGS").unwrap().contains("ncursesw"));
    }

    /// M8 design §4.4's last clause (final review I1): when the build route supplied Tcl/Tk,
    /// Tcl's `info library` exists outside the build cache, and Tcl starts in a venv.
    #[test]
    fn a_store_built_tcl_must_read_its_library_outside_the_cache_and_start_in_a_venv() {
        let says = |lib: &'static str| {
            move |args: &[&str]| -> (bool, String) {
                if args[1].contains("tkinter.Tcl") {
                    (true, format!("{lib}\n"))
                } else {
                    (true, String::new())
                }
            }
        };
        let cache = Path::new("/r/cache/deps");
        let real = [
            "/r/.rpyenv/tcltk/tcl-8.6.18/tcl8.6",
            "/r/versions/3.12.15/lib/tcl8.6",
        ];
        let exists = |p: &Path| real.iter().any(|r| p == Path::new(r));
        let venv_ok = || Ok(true);
        let venv_fails = || Ok(false);
        let good = StoreTcl {
            cache,
            exists: &exists,
            in_venv: &venv_ok,
        };
        for lib in real {
            let f = stdlib_failures(&says(lib), 12, Some(&good));
            assert!(f.is_empty(), "{lib}: {f:?}");
        }
        assert_eq!(
            stdlib_failures(&says("/r/cache/deps/tcl-8.6.18-abc/lib/tcl8.6"), 12, Some(&good)),
            ["tkinter (Tcl's library /r/cache/deps/tcl-8.6.18-abc/lib/tcl8.6 is in the build cache; needs tk)"]
        );
        assert_eq!(
            stdlib_failures(&says("/r/.rpyenv/tcltk/gone/tcl8.6"), 12, Some(&good)),
            ["tkinter (Tcl's library /r/.rpyenv/tcltk/gone/tcl8.6 doesn't exist; needs tk)"]
        );
        let no_venv = StoreTcl {
            cache,
            exists: &exists,
            in_venv: &venv_fails,
        };
        assert_eq!(
            stdlib_failures(&says(real[0]), 12, Some(&no_venv)),
            ["tkinter (Tcl doesn't start in a virtual environment; needs tk)"]
        );
        let venv_cannot = || Err("no private folder".to_string());
        let cannot = StoreTcl {
            cache,
            exists: &exists,
            in_venv: &venv_cannot,
        };
        assert_eq!(
            stdlib_failures(&says(real[0]), 12, Some(&cannot)),
            ["tkinter (could not check Tcl in a virtual environment: no private folder)"]
        );
        // Without the store, neither where Tcl's library is nor venvs are checked.
        assert!(stdlib_failures(&says("/usr/share/tcltk/tcl8.6"), 12, None).is_empty());
    }

    /// H3: the throwaway venv folder is always a new one (mode 0700); a folder or a link that
    /// exists at a candidate name is skipped, never used.
    #[test]
    fn the_throwaway_folder_is_never_one_that_already_exists() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let first = t.path().join("a");
        std::fs::create_dir(&first).unwrap();
        std::fs::write(first.join("planted"), "x").unwrap();
        std::os::unix::fs::symlink(&first, t.path().join("b")).unwrap();
        let mut names = ["a", "b", "c"].into_iter().map(String::from);
        let got = private_dir(t.path(), &mut names).unwrap();
        assert_eq!(got, t.path().join("c"));
        assert_eq!(
            std::fs::metadata(&got).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(
            first.join("planted").is_file(),
            "the pre-made folder is untouched"
        );
        let mut taken = ["a", "b"].into_iter().map(String::from);
        let e = private_dir(t.path(), &mut taken).unwrap_err();
        assert!(
            e.contains("2 folder names") && e.contains("taken already"),
            "{e}"
        );
        let mut missing = ["x"].into_iter().map(String::from);
        let e = private_dir(&t.path().join("nope"), &mut missing).unwrap_err();
        assert!(e.starts_with("cannot create "), "{e}");
    }

    #[test]
    fn the_continue_summary_is_the_three_lines_of_the_spec() {
        let os = "ID=debian
VERSION_ID=\"12\"
";
        let fails = vec![
            "tkinter (import tkinter failed; needs tk)".to_string(),
            "uuid (import _uuid failed; needs libuuid)".to_string(),
        ];
        assert_eq!(
            continue_summary("3.12.14", &fails, 12, Some(os), false),
            "pyenv: 3.12.14 was built without: tkinter, uuid
  Add them: sudo apt-get install tk-dev uuid-dev, then pyenv install -f 3.12.14"
        );
        let root = continue_summary("3.12.14", &fails, 12, Some(os), true);
        assert!(
            root.contains(
                "
  Add them: apt-get install tk-dev uuid-dev, then"
            ),
            "{root}"
        );
        let none = continue_summary("3.12.14", &fails, 12, None, false);
        assert!(
            none.contains("Add them: install the development packages for tkinter, uuid, then"),
            "{none}"
        );
    }

    /// Fedora 42+ ships Tk 9 as tk-devel: CPython 3.12 and older need tk8-devel, 3.13 and
    /// newer keep tk-devel; Fedora 41 has no tk8-devel (measured with dnf repoquery).
    #[test]
    fn the_continue_summary_names_tk8_devel_for_old_cpython_on_new_fedora() {
        let fails = vec!["tkinter (import tkinter failed; needs tk)".to_string()];
        let f44 = "ID=fedora\nVERSION_ID=44\n";
        assert!(continue_summary("3.12.14", &fails, 12, Some(f44), false)
            .contains("Add them: sudo dnf install tk8-devel, then"),);
        assert!(continue_summary("3.13.16", &fails, 13, Some(f44), false)
            .contains("Add them: sudo dnf install tk-devel, then"),);
        let f41 = "ID=fedora\nVERSION_ID=41\n";
        assert!(continue_summary("3.12.14", &fails, 12, Some(f41), false)
            .contains("Add them: sudo dnf install tk-devel, then"),);
    }

    /// Rocky, Alma and RHEL: the summary's dnf command needs CRB, so the line is there too.
    #[test]
    fn the_continue_summary_names_crb_on_the_rhel_family() {
        let fails = vec!["uuid (import _uuid failed; needs libuuid)".to_string()];
        let rocky = "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n";
        let s = continue_summary("3.12.14", &fails, 12, Some(rocky), false);
        assert!(
            s.contains("\n  Some packages live in the CRB repository; enable it first: sudo dnf install dnf-plugins-core && sudo dnf config-manager --set-enabled crb"),
            "{s}"
        );
        let fedora = continue_summary(
            "3.12.14",
            &fails,
            12,
            Some("ID=fedora\nVERSION_ID=44\n"),
            false,
        );
        assert!(!fedora.contains("CRB"), "{fedora}");
    }
}
