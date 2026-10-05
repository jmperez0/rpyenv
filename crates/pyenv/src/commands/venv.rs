//! The virtualenv commands built in (spec §10, D4; docs/parity/pyenv-virtualenv-m4-reference.md):
//! `virtualenv`, `virtualenvs`, `virtualenv-prefix`, `virtualenv-delete`, and the `uninstall`
//! cascade. Linux matches pyenv-virtualenv v1.4.0 where it works and does what it meant where
//! it doesn't (allowlist D-96 to D-99); Windows is new (D-101).

use crate::install::{prompt, Reply};
use crate::output::Output;
use rpyenv_core::ctx::Ctx;
use rpyenv_core::flavor::Flavor;
use rpyenv_core::{lookup, pathsearch, prefix, select, venv};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const VERSION: &str = "1.4.0";

pub const HELP: &str = "Usage: pyenv virtualenv [-f|--force] [VIRTUALENV_OPTIONS] [version] <virtualenv-name>\n       pyenv virtualenv --version\n       pyenv virtualenv --help\n\n  -f/--force       Install even if the version appears to be installed already. Skip\n                   prompting for confirmation\n\nNotable VIRTUALENV_OPTIONS passed to venv-creating executable, if applicable:\n  -u/--upgrade     Imply --force\n\n";

pub(crate) fn bin_name(ctx: &Ctx) -> &'static str {
    match ctx.flavor {
        Flavor::Pyenv => "bin",
        Flavor::PyenvWin => "Scripts",
    }
}

#[derive(Default)]
struct Opts {
    force: bool,
    upgrade: bool,
    no_pip: bool,
    quiet: bool,
    verbose: bool,
    help: bool,
    version: bool,
    python: Option<String>,
    pass: Vec<String>,
}

/// `parse_options`, except that `-p`/`--python` take the next word (allowlist D-96): `--x` is
/// one option, `-xy` one per letter, a lone `-` nothing, anything else a positional.
fn parse(args: &[&str]) -> (Opts, Vec<String>) {
    let (mut o, mut pos, mut names) = (Opts::default(), Vec::new(), Vec::<String>::new());
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if let Some(long) = a.strip_prefix("--") {
            if long == "python" {
                o.python = args.get(i + 1).map(|s| s.to_string());
                i += 1;
            } else {
                names.push(long.to_string());
            }
        } else if a.len() > 1 && a.starts_with('-') {
            for c in a[1..].chars() {
                if c == 'p' {
                    o.python = args.get(i + 1).map(|s| s.to_string());
                    i += 1;
                } else {
                    names.push(c.to_string());
                }
            }
        } else if a != "-" {
            pos.push(a.to_string());
        }
        i += 1;
    }
    for n in names {
        match n.as_str() {
            "f" | "force" => o.force = true,
            "h" | "help" => o.help = true,
            "no-pip" | "no-setuptools" | "without-pip" => {
                o.no_pip = true;
                o.pass.push(format!("--{n}"));
            }
            "q" | "quiet" => o.quiet = true,
            "u" | "upgrade" => o.upgrade = true,
            "v" | "verbose" => o.verbose = true,
            "version" => o.version = true,
            _ => match n.strip_prefix("python=") {
                Some(p) => o.python = Some(p.to_string()),
                None => o.pass.push(format!("--{n}")),
            },
        }
    }
    (o, pos)
}

/// The first selected version, or `system` (Linux); the first pyenv-win selection (Windows).
pub(crate) fn current_names(ctx: &Ctx) -> Vec<String> {
    match ctx.flavor {
        Flavor::Pyenv => select::version_name(ctx, false).names,
        Flavor::PyenvWin => select::win_select(ctx)
            .into_iter()
            .map(|s| s.name)
            .collect(),
    }
}

fn not_installed(v: &str) -> Output {
    let mut o = Output::new();
    o.err(format!(
        "pyenv-virtualenv: `{v}' is not installed in pyenv."
    ));
    #[cfg(unix)]
    let known = crate::install::defs::known(&|k| std::env::var(k).ok())
        .iter()
        .any(|d| d == v);
    #[cfg(not(unix))]
    let known = true;
    if known {
        o.err(format!("Run `pyenv install {v}' to install it."));
    } else {
        o.err("It does not look like a valid Python version. See `pyenv install --list' for available versions.");
    }
    o.with_code(1)
}

fn which(c: &Ctx, cmd: &str) -> Option<PathBuf> {
    match c.flavor {
        Flavor::Pyenv => {
            let nosystem = c.pyenv_version.as_deref() != Some("system");
            lookup::which_pyenv(c, cmd, nosystem, &lookup::Skip::default())
                .ok()
                .map(|f| f.path)
        }
        Flavor::PyenvWin => lookup::which_win_runnable(c, cmd).ok().map(|f| f.path),
    }
}

fn venv_works(p: &Path) -> bool {
    Command::new(p)
        .args(["-m", "venv", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The interpreter that runs `-m venv` (Decision 2, allowlist D-96).
fn interpreter(ctx: &Ctx, base: &str, wanted: Option<&str>) -> Result<PathBuf, Output> {
    let mut c = ctx.clone();
    c.pyenv_version = Some(base.to_string());
    if let Some(w) = wanted {
        let p = Path::new(w);
        let bare = !w.contains(['/', '\\']) || p.parent() == Some(ctx.shims_dir().as_path());
        if !bare {
            return Ok(p.to_path_buf());
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(found) = which(&c, &name) {
            return Ok(found);
        }
        let last = match ctx.flavor {
            Flavor::Pyenv => lookup::whence_pyenv(ctx, &name).pop().map(|(_, p)| p),
            Flavor::PyenvWin => lookup::whence_win(ctx, &name, true)
                .pop()
                .map(PathBuf::from),
        };
        return last.ok_or_else(|| not_installed(&name));
    }
    if base == "system" {
        for py in ["python3", "python", "python2"] {
            if let Some(p) = which(&c, py).filter(|p| venv_works(p)) {
                return Ok(p);
            }
        }
        return Err(Output::error(
            "pyenv-virtualenv: no Python with `venv' found on the system",
        ));
    }
    which(&c, "python").ok_or_else(|| {
        Output::error(format!(
            "pyenv-virtualenv: `python' not found in version `{base}'"
        ))
    })
}

fn version_line(ctx: &Ctx) -> Output {
    let cur = current_names(ctx)
        .into_iter()
        .next()
        .unwrap_or_else(|| "system".into());
    let backend = match prefix::prefix_of(ctx, &cur) {
        Ok(p) if ctx.flavor == Flavor::Pyenv && pathsearch::is_runnable(&p.join("bin/conda")) => {
            let v = Command::new(p.join("bin/conda"))
                .arg("--version")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".into());
            format!("conda {v}")
        }
        _ if cur == "system" => "python3 -m venv".into(),
        _ => "python -m venv".into(),
    };
    let mut o = Output::new();
    o.out(format!("pyenv-virtualenv {VERSION} ({backend})"));
    o
}

fn is_link(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

/// `versions/<name>` pointing at `env_dir` (Decision 3; Windows: Decision 4, Task 7).
/// Replaces an existing link only; the caller has refused anything else.
pub(crate) fn link_env(ctx: &Ctx, env_dir: &Path, link: &Path) -> std::io::Result<()> {
    match ctx.flavor {
        Flavor::Pyenv => {
            #[cfg(unix)]
            {
                let tmp = link.with_file_name(format!(
                    ".{}.{}.tmp",
                    link.file_name().unwrap().to_string_lossy(),
                    std::process::id()
                ));
                let _ = std::fs::remove_file(&tmp);
                std::os::unix::fs::symlink(env_dir, &tmp)?;
                std::fs::rename(&tmp, link).inspect_err(|_| {
                    let _ = std::fs::remove_file(&tmp);
                })
            }
            #[cfg(not(unix))]
            {
                let _ = (env_dir, link);
                Err(std::io::ErrorKind::Unsupported.into())
            }
        }
        Flavor::PyenvWin => {
            // Junctions arrive with Task 7 (plan M4b Decision 4).
            let _ = (env_dir, link);
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }
}

/// A name segment that can't leave its folder (review focus 1).
fn plain(seg: &str, ctx: &Ctx) -> bool {
    !seg.is_empty()
        && seg != "."
        && seg != ".."
        && !seg.contains('\0')
        && (ctx.flavor == Flavor::Pyenv || crate::install::is_safe_win_segment(seg))
}

pub fn virtualenv(ctx: &Ctx, args: &[&str]) -> Output {
    if args.first() == Some(&"--complete") {
        return crate::commands::versions::versions(ctx, &["--bare", "--skip-envs"]);
    }
    let (o, pos) = parse(args);
    if o.help {
        return Output {
            stdout: HELP.into(),
            ..Output::new()
        };
    }
    if o.version {
        return version_line(ctx);
    }
    let (base, name) = match pos.as_slice() {
        [] => return Output::error("pyenv-virtualenv: no virtualenv name given."),
        [name] => (
            current_names(ctx).into_iter().next().unwrap_or_default(),
            name.clone(),
        ),
        [base, name, ..] => (base.clone(), name.clone()),
    };
    let base = if base.is_empty() {
        base
    } else {
        let r = crate::commands::latest::latest(ctx, &["-f", &base]);
        Some(r.stdout.trim().to_string())
            .filter(|s| r.code == 0 && !s.is_empty())
            .unwrap_or(base)
    };
    if base.is_empty() || name.is_empty() {
        return Output {
            stdout: HELP.into(),
            code: 1,
            ..Output::new()
        };
    }
    let sep = |c: char| c == '/' || (ctx.flavor == Flavor::PyenvWin && c == '\\');
    let last = name.rsplit(sep).next().unwrap_or("").to_string();
    if last == "system" {
        return Output::error("pyenv-virtualenv: `system' is not allowed as virtualenv name.");
    }
    if name.chars().any(char::is_whitespace) {
        return Output::error("pyenv-virtualenv: no whitespace allowed in virtualenv name.");
    }
    let base_first = base.split(sep).next().unwrap_or("");
    if name.contains(sep) && name.replace('\\', "/") != format!("{base_first}/envs/{last}") {
        return Output::error("pyenv-virtualenv: no slash allowed in virtualenv name.");
    }
    if !plain(&last, ctx) || rpyenv_core::installed::is_staging_name(&last) {
        return Output::error(format!(
            "pyenv-virtualenv: `{last}' is not allowed as virtualenv name."
        ));
    }
    if !base.split(sep).all(|s| plain(s, ctx)) {
        return not_installed(&base);
    }
    let base_dir = match prefix::prefix_of(ctx, &base) {
        Ok(d) if d.is_dir() => d,
        _ => return not_installed(&base),
    };
    let versions = ctx.versions_dir();
    let (full, env_dir, link) = if base == "system" {
        (last.clone(), versions.join(&last), None)
    } else {
        let owner = venv::base_prefix(ctx, &base)
            .ok()
            .filter(|p| p.parent() == Some(versions.as_path()))
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let full = format!("{}/envs/{last}", owner.unwrap_or_else(|| base.clone()));
        let dir = venv::version_dir(ctx, &full);
        (full, dir, Some(versions.join(&last)))
    };
    let force = o.force || o.upgrade;
    if let Some(l) = &link {
        if std::fs::symlink_metadata(l).is_ok() {
            let ours =
                is_link(l) && std::fs::read_link(l).ok().as_deref() == Some(env_dir.as_path());
            if !is_link(l) || (!ours && !force) {
                return Output::error(format!(
                    "pyenv-virtualenv: `{}' already exists.",
                    l.display()
                ));
            }
        }
    }
    let existed = env_dir.is_dir();
    if env_dir.join(bin_name(ctx)).exists() && !force {
        rpyenv_core::textout::write(
            true,
            &format!("pyenv-virtualenv: {} already exists\n", env_dir.display()),
        );
        match prompt("continue with installation? (y/N) ") {
            Reply::Line(r) if r.starts_with(['y', 'Y']) => {}
            Reply::Interrupted => return Output::new().with_code(130),
            _ => return Output::new().with_code(1),
        }
    }
    let cleanup = |code: i32| -> Output {
        if let Some(l) = &link {
            if is_link(l) && std::fs::read_link(l).ok().as_deref() == Some(env_dir.as_path()) {
                let _ = if ctx.flavor == Flavor::Pyenv {
                    std::fs::remove_file(l)
                } else {
                    std::fs::remove_dir(l)
                };
            }
        }
        if !existed {
            let _ = std::fs::remove_dir_all(&env_dir);
        }
        Output::new().with_code(code)
    };
    let cache = std::env::var_os("PYENV_VIRTUALENV_CACHE_PATH")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("PYTHON_BUILD_CACHE_PATH").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.root.join("cache"));
    let conda =
        ctx.flavor == Flavor::Pyenv && pathsearch::is_runnable(&base_dir.join("bin").join("conda"));
    let status = if conda {
        conda_create(&base_dir, &last, &o, &cache, &base)
    } else {
        let python = match interpreter(ctx, &base, o.python.as_deref()) {
            Ok(p) => p,
            Err(out) => return out,
        };
        let mut cmd = Command::new(&python);
        cmd.args(["-m", "venv"]);
        if o.upgrade {
            cmd.arg("--upgrade");
        }
        cmd.args(&o.pass).arg(&env_dir);
        if std::fs::create_dir_all(&cache).is_ok() {
            cmd.current_dir(&cache);
        }
        cmd.env("PYENV_VERSION", &base)
            .env_remove("PIP_REQUIRE_VENV")
            .env_remove("PIP_REQUIRE_VIRTUALENV")
            .env_remove("VIRTUALENV_PYTHON");
        cmd.status().map_err(|e| (python, e))
    };
    let code = match status {
        Ok(s) => s.code().unwrap_or(1),
        Err((p, e)) => {
            let mut out = cleanup(126);
            out.err(format!(
                "pyenv: {}: {}",
                p.display(),
                rpyenv_core::launch::io_reason(&e)
            ));
            return out;
        }
    };
    if code != 0 {
        return cleanup(code);
    }
    #[cfg(unix)]
    if ctx.flavor == Flavor::Pyenv && !conda {
        config_links_and_pydoc(&base_dir, &env_dir);
    }
    if let Some(l) = &link {
        if let Err(e) = link_env(ctx, &env_dir, l) {
            let mut out = cleanup(1);
            out.err(format!(
                "pyenv-virtualenv: cannot link {} to {}: {e}",
                l.display(),
                env_dir.display()
            ));
            return out;
        }
    }
    if !o.no_pip && !conda {
        if let Err(msg) = ensure_pip(ctx, &full, &env_dir) {
            let mut out = cleanup(1);
            out.err(msg);
            return out;
        }
    }
    let r = crate::commands::rehash::rehash(ctx, &[]);
    Output {
        stderr: r.stderr,
        code: r.code,
        ..Output::new()
    }
}

/// `python*-config` links from the base, and `bin/pydoc` (reference "The creation run", 4 and 6).
#[cfg(unix)]
fn config_links_and_pydoc(base_dir: &Path, env_dir: &Path) {
    let bin = env_dir.join("bin");
    if let Ok(rd) = std::fs::read_dir(base_dir.join("bin")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("python")
                && n.ends_with("-config")
                && std::fs::symlink_metadata(bin.join(&n)).is_err()
            {
                let _ = std::os::unix::fs::symlink(e.path(), bin.join(&n));
            }
        }
    }
    let pydoc = bin.join("pydoc");
    if std::fs::symlink_metadata(&pydoc).is_err() {
        use std::os::unix::fs::PermissionsExt;
        let body = format!(
            "#!{}/bin/python\nimport pydoc\nif __name__ == '__main__':\n      pydoc.cli()\n",
            env_dir.display()
        );
        if std::fs::write(&pydoc, body).is_ok() {
            let _ = std::fs::set_permissions(&pydoc, std::fs::Permissions::from_mode(0o755));
        }
    }
}

/// allowlist D-99: ensurepip when the env has no pip, then a local `GET_PIP`; never a download.
fn ensure_pip(ctx: &Ctx, full: &str, env_dir: &Path) -> Result<(), String> {
    let pip = env_dir
        .join(bin_name(ctx))
        .join(if ctx.flavor == Flavor::Pyenv {
            "pip"
        } else {
            "pip.exe"
        });
    if pip.exists() {
        return Ok(());
    }
    let mut c = ctx.clone();
    c.pyenv_version = Some(full.to_string());
    let Some(py) = which(&c, "python") else {
        return Ok(());
    };
    let ok = Command::new(&py)
        .args(["-s", "-m", "ensurepip"])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        return Ok(());
    }
    match std::env::var_os("GET_PIP").map(PathBuf::from).filter(|p| p.is_file()) {
        Some(get_pip) => {
            rpyenv_core::textout::write(true, &format!("Installing pip from {}...\n", get_pip.display()));
            let opts = std::env::var("GET_PIP_OPTS").unwrap_or_default();
            let ok = Command::new(&py).arg("-s").arg(&get_pip).args(opts.split_whitespace())
                .stdout(Stdio::from(std::io::stderr()))
                .status().is_ok_and(|s| s.success());
            if ok { Ok(()) } else { Err("error: failed to install pip via get-pip.py".into()) }
        }
        None => Err(format!("pyenv-virtualenv: pip could not be installed in `{full}': ensurepip failed, and rpyenv doesn't download get-pip.py (set GET_PIP to a local copy)")),
    }
}

/// A conda base: `conda create` (reference "The creation run", step 3).
fn conda_create(
    base_dir: &Path,
    last: &str,
    o: &Opts,
    cache: &Path,
    base: &str,
) -> Result<std::process::ExitStatus, (PathBuf, std::io::Error)> {
    let conda = base_dir.join("bin").join("conda");
    let mut cmd = Command::new(&conda);
    cmd.arg("create");
    if o.quiet {
        cmd.arg("--quiet");
    }
    if o.verbose {
        cmd.arg("--verbose");
    }
    cmd.args(["--name", last, "--yes"])
        .args(&o.pass)
        .env("PYENV_VERSION", base);
    let list = cache.join(format!("conda-python.{}.txt", std::process::id()));
    match &o.python {
        Some(p) => {
            let n = Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            cmd.arg(format!("python={}", n.strip_prefix("python").unwrap_or(&n)));
        }
        None => {
            let out = Command::new(&conda)
                .args(["list", "python", "--full-name", "--export"])
                .output()
                .map_err(|e| (conda.clone(), e))?;
            let _ = std::fs::create_dir_all(cache);
            std::fs::write(&list, out.stdout).map_err(|e| (list.clone(), e))?;
            cmd.arg("--file").arg(&list);
        }
    }
    let r = cmd.status().map_err(|e| (conda.clone(), e));
    let _ = std::fs::remove_file(&list);
    r
}
