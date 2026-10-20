//! The build store (M8 design §6.2): `<root>/cache/deps/<lib>-<version>-<hash>/`, static
//! libraries compiled from the catalog without root and reused by every CPython version that
//! needs the same library, version and options (D6, D7). It is a cache: CPython links these
//! libraries statically, so deleting `cache/deps` breaks no installed version.

use super::archive::{extract_skipping_outside_links, Kind};
use super::builder::{get, logged};
use super::catalog::{self, Origin, Recipe, Row};
use super::checksum::sha256_file;
use super::defs::Definition;
use super::fetch::{Check, FetchRequest, Fetcher};
use super::log::BuildLog;
use super::{interrupted, InstallError};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

type R<T> = Result<T, InstallError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: &'static str,
    pub version: String,
    pub origin: Origin,
    pub dir: PathBuf,
}

pub fn deps_dir(root: &Path) -> PathBuf {
    root.join("cache").join("deps")
}

/// The `OPENSSLDIR` of every OpenSSL the store builds (M8 design §6.5): a private folder
/// without an `openssl.cnf`, whose `cert.pem` and `certs` link to the system's certificates
/// (or, with none, hold rpyenv's pinned bundle). Outside `cache/deps` (ruling P9): a
/// self-built OpenSSL has this path compiled in, so it must survive the cache being deleted.
pub fn ssl_dir(root: &Path) -> PathBuf {
    root.join(".rpyenv").join("ssl")
}

/// Where Tcl's and Tk's script folders live for the virtual environments of the versions built
/// against them (M8 design §6.3): `<root>/.rpyenv/tcltk/<key>-<version>/`. Tcl and Tk have this
/// path compiled in; it is outside `cache/deps`, so deleting the cache breaks no venv.
pub fn tcltk_dir(root: &Path) -> PathBuf {
    root.join(".rpyenv").join("tcltk")
}

/// The shared script home of one Tcl or Tk release.
pub fn scripts_home(root: &Path, key: &str, version: &str) -> PathBuf {
    tcltk_dir(root).join(format!("{key}-{version}"))
}

/// The script folder a Tcl or Tk entry installs under `lib/`; `None` for other libraries.
pub fn script_dir(key: &str) -> Option<&'static str> {
    match key {
        "tcl" => Some("tcl8.6"),
        "tk" => Some("tk8.6"),
        _ => None,
    }
}

/// The file whose presence says a Tcl or Tk script folder is whole: `init.tcl` in `tcl8.6`,
/// `tk.tcl` in `tk8.6`; `None` for any other folder name.
fn script_marker(folder: &str) -> Option<&'static str> {
    match folder {
        "tcl8.6" => Some("init.tcl"),
        "tk8.6" => Some("tk.tcl"),
        _ => None,
    }
}

/// Whether `dir` counts as filled: a script folder only with its marker file (a folder that
/// was partly removed is not), any other folder when it is a directory.
pub fn is_filled(dir: &Path) -> bool {
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    match script_marker(&name) {
        Some(m) => dir.join(m).is_file(),
        None => dir.is_dir(),
    }
}

/// A version text that is safe inside a path the record names: not empty, not `.` or `..`, and
/// only ASCII letters and digits and `.`, `-`, `_`, `+` (a record is a file a user can edit).
pub fn plain_version(v: &str) -> bool {
    !v.is_empty()
        && v != "."
        && v != ".."
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c))
}

/// Copies the tree `src` to `dst` unless `dst` is already filled (see `is_filled`): into a
/// temporary sibling first, then swapped into place (a broken `dst` is renamed to `.del-<name>-
/// <pid>` first and removed after), so `dst` is never half-copied. Another process that filled
/// `dst` first wins.
pub fn place_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if is_filled(dst) {
        return Ok(());
    }
    let parent = dst
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent folder"))?;
    std::fs::create_dir_all(parent)?;
    let name = dst.file_name().unwrap_or_default().to_string_lossy();
    let pid = std::process::id();
    let tmp = parent.join(format!(".tmp-{name}-{pid}"));
    let del = parent.join(format!(".del-{name}-{pid}"));
    remove_stale(parent, &format!(".tmp-{name}-"));
    remove_stale(parent, &format!(".del-{name}-"));
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&del);
    let copied = super::builder::copy_tree(src, &tmp).and_then(|()| {
        if std::fs::symlink_metadata(dst).is_ok() {
            std::fs::rename(dst, &del)?;
            if let Err(e) = std::fs::rename(&tmp, dst) {
                let _ = std::fs::rename(&del, dst);
                return Err(e);
            }
            let _ = std::fs::remove_dir_all(&del);
            Ok(())
        } else {
            std::fs::rename(&tmp, dst)
        }
    });
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&tmp);
        if !is_filled(dst) {
            return Err(e);
        }
    }
    Ok(())
}

/// Whether process `pid` may still be running. Without `/proc` (not Linux) it says yes: a
/// folder named for a process is then never taken for a dead one's. A `PYENV_ROOT` shared
/// across pid namespaces (containers) can show another namespace's live folder as dead.
fn pid_may_run(pid: u32) -> bool {
    let proc = Path::new("/proc");
    !proc.join("self").exists() || proc.join(pid.to_string()).exists()
}

/// Removes the folders of `dir` whose name gives a pid (`pid_of`) of a process that is gone (a
/// killed run left them); never the current process's.
fn sweep(dir: &Path, pid_of: &dyn Fn(&str) -> Option<u32>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let n = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = pid_of(&n) else {
            continue;
        };
        if pid != std::process::id() && !pid_may_run(pid) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

fn digits(r: &str) -> Option<u32> {
    (!r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
        .then(|| r.parse().ok())
        .flatten()
}

/// Removes the folders of `dir` named `<prefix><pid>` whose process is gone.
pub(crate) fn remove_stale(dir: &Path, prefix: &str) {
    sweep(dir, &|n| n.strip_prefix(prefix).and_then(digits));
}

/// Removes the `.del-<name>-<pid>` folders of `dir` whose process is gone.
pub(crate) fn remove_stale_del(dir: &Path) {
    sweep(dir, &|n| {
        n.strip_prefix(".del-")
            .and_then(|r| r.rsplit_once('-'))
            .and_then(|(_, p)| digits(p))
    });
}

/// Adds the entries of `new` that `into` doesn't hold yet (same folder): a second `[b]` round
/// reuses ncurses or Tcl, and CPython's flags and the record must name each entry once.
pub fn add_unique(into: &mut Vec<Entry>, new: Vec<Entry>) {
    for e in new {
        if !into.iter().any(|x| x.dir == e.dir) {
            into.push(e);
        }
    }
}

/// A path the recipes and CPython's flags can carry unquoted (plan ruling P5): not empty, no
/// whitespace or control character, none of the characters a shell gives a meaning, and no `:`
/// (it would split `PATH` and `PKG_CONFIG_PATH`).
pub fn plain_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "\"'`$\\;&|<>*?()[]{}!#~:".contains(c))
}

/// The system's CA certificates, which `ssl_dir` links to (§6.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemCerts {
    /// A bundle file.
    pub bundle: Option<PathBuf>,
    /// A non-empty folder of certificates.
    pub dir: Option<PathBuf>,
}

/// Finds the system's certificates: the first existing bundle of the distributions' files,
/// else `<openssldir>/cert.pem` (the folder `openssl version -d` names; last, because an
/// `openssl` first on `PATH` can be conda's or Linuxbrew's, not the system's); the first
/// non-empty of `/etc/ssl/certs`, `/etc/pki/tls/certs`, else `<openssldir>/certs`.
pub fn system_certs(
    openssldir: Option<&str>,
    is_file: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> SystemCerts {
    let last_file = openssldir.map(|d| format!("{d}/cert.pem"));
    let bundle = [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/etc/ssl/cert.pem",
    ]
    .into_iter()
    .chain(last_file.as_deref())
    .find(|p| is_file(p))
    .map(PathBuf::from);
    let last_dir = openssldir.map(|d| format!("{d}/certs"));
    let dir = ["/etc/ssl/certs", "/etc/pki/tls/certs"]
        .into_iter()
        .chain(last_dir.as_deref())
        .find(|d| has_files(d))
        .map(PathBuf::from);
    SystemCerts { bundle, dir }
}

/// `system_certs` for this machine. Debug builds: `RPYENV_TEST_SYSTEM_CERTS` replaces it
/// (`<bundle>:<dir>`, either part may be empty).
pub fn detect_system_certs() -> SystemCerts {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("RPYENV_TEST_SYSTEM_CERTS") {
        let (b, d) = v.split_once(':').unwrap_or((v.as_str(), ""));
        return SystemCerts {
            bundle: (!b.is_empty()).then(|| PathBuf::from(b)),
            dir: (!d.is_empty()).then(|| PathBuf::from(d)),
        };
    }
    let dir = std::process::Command::new("openssl")
        .args(["version", "-d"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split('"')
                .nth(1)
                .map(str::to_string)
        });
    let has_files = |d: &str| std::fs::read_dir(d).is_ok_and(|mut r| r.next().is_some());
    system_certs(dir.as_deref(), &|p| Path::new(p).is_file(), &has_files)
}

/// `<key>-<version>-<the first 12 hex digits of the fingerprint's SHA-256>`.
pub fn entry_name(key: &str, version: &str, fingerprint: &str) -> String {
    let h = Sha256::digest(fingerprint.as_bytes());
    let hex: String = h.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{key}-{version}-{hex}")
}

/// Everything that makes two builds of a library differ (spec D6): its source, its recipe,
/// the compiler and target, `OPENSSLDIR` for OpenSSL, the script home for Tcl and Tk (both are
/// compiled in), the entries it was built against and, for Tk built against the store's X11,
/// its X11 arguments.
#[allow(clippy::too_many_arguments)]
pub fn fingerprint(
    r: &Recipe,
    row: &Row,
    cc: &str,
    triple: &str,
    openssldir: &str,
    scripts: &str,
    deps: &[String],
    store_x11: bool,
) -> String {
    let mut s = format!(
        "{}\n{}\n{}\n{}\n{cc}\n{triple}\n",
        r.key,
        row.version,
        row.sha256,
        r.steps.join("\n")
    );
    if r.key == "openssl" {
        s.push_str(openssldir);
        s.push('\n');
    }
    if script_dir(r.key).is_some() {
        s.push_str(scripts);
        s.push('\n');
    }
    for d in deps {
        s.push_str(d);
        s.push('\n');
    }
    // Tk against the store's X11 (ruling C4): another build, another entry. Without it the
    // text is M8b's, so existing entries keep their names.
    if store_x11 {
        s.push_str(r.x11_args);
        s.push('\n');
    }
    s
}

/// A recipe's steps, with its `x11_args` added to the first when the store supplies X11.
pub fn steps_of(r: &Recipe, store_x11: bool) -> Vec<String> {
    let mut s: Vec<String> = r.steps.iter().map(|x| x.to_string()).collect();
    if store_x11 {
        if let Some(first) = s.first_mut() {
            first.push_str(r.x11_args);
        }
    }
    s
}

/// `step` with each `{name}` of `vars` replaced by its value.
pub fn expand(step: &str, vars: &[(String, String)]) -> String {
    let mut s = step.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// A store `.pc` file with `Libs.private` joined to `Libs`: the store holds static libraries
/// only, so a consumer always needs them (measured: CPython 3.12's `_sqlite3` is not found
/// without sqlite3.pc's `-lm`).
pub fn fold_private(pc: &str) -> String {
    let mut private = String::new();
    let mut kept: Vec<&str> = Vec::new();
    for l in pc.lines() {
        match l.strip_prefix("Libs.private:") {
            Some(rest) if !rest.trim().is_empty() => {
                private.push(' ');
                private.push_str(rest.trim());
            }
            Some(_) => {}
            None => kept.push(l),
        }
    }
    let mut out = String::new();
    for l in kept {
        out.push_str(l.trim_end());
        if l.starts_with("Libs:") {
            out.push_str(&private);
        }
        out.push('\n');
    }
    out
}

pub struct Build<'a> {
    /// `PYENV_ROOT`, absolute.
    pub root: &'a Path,
    pub rows: &'a [Row],
    pub definition: &'a Definition,
    /// The job's environment: `PATH`, `CC`, `TMPDIR`, `HOME` and the rest.
    pub env: &'a [(OsString, OsString)],
    pub fetcher: &'a Fetcher,
    /// `ssl_dir(root)` (§6.5).
    pub openssldir: PathBuf,
    /// What `ssl_dir` links to.
    pub certs: SystemCerts,
    pub verbose: bool,
    /// The CPython tarball a bootstrap Python is built from when xcb-proto or libxcb must be
    /// built (M8c); `None` when the definition has none (it fetches CPython with git).
    pub bootstrap: Option<&'a super::bootstrap::Source>,
}

/// The variables a library build must not inherit (plan ruling P7).
const DROPPED: [&str; 7] = [
    "CPPFLAGS",
    "LDFLAGS",
    "LIBS",
    "CFLAGS",
    "CXXFLAGS",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
];

/// The environment of a library build: the job's, without `DROPPED`, with `CFLAGS=-O2 -fPIC`,
/// and with the store's pkgconf first on `PATH` once it has been built (ruling P10).
fn step_env(env: &[(OsString, OsString)], pkgconf: Option<&Path>) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = env
        .iter()
        .filter(|(k, _)| !DROPPED.iter().any(|d| k == d))
        .cloned()
        .collect();
    out.push(("CFLAGS".into(), "-O2 -fPIC".into()));
    if let Some(bin) = pkgconf {
        let path = get(env, "PATH").unwrap_or_default();
        out.push(("PATH".into(), format!("{}:{path}", bin.display()).into()));
    }
    out
}

fn message(m: String) -> InstallError {
    InstallError::Message(m)
}

/// `cc -dumpmachine`, or `unknown`.
fn triple_of(cc: &str, env: &[(OsString, OsString)]) -> String {
    let mut words = cc.split_whitespace();
    let Some(program) = words.next() else {
        return "unknown".into();
    };
    Command::new(program)
        .args(words)
        .arg("-dumpmachine")
        .env("PATH", get(env, "PATH").unwrap_or_default())
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// Builds every library of `keys` (catalog keys, in `catalog::ORDER`) that the store doesn't
/// have yet, and returns the entries of all of them. One lock per entry: a second `pyenv`
/// building the same entry waits, then reuses it (Review Focus 1).
pub fn ensure(b: &Build, keys: &[&'static str], say: &mut dyn FnMut(&str)) -> R<Vec<Entry>> {
    let dir = deps_dir(b.root);
    let locks = dir.join(".locks");
    std::fs::create_dir_all(&locks)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", locks.display())))?;
    let cc = get(b.env, "CC")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "cc".into());
    let triple = triple_of(&cc, b.env);
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .to_string();
    let openssldir = b.openssldir.display().to_string();
    // The private OPENSSLDIR a built OpenSSL reads, made before it is built or reused.
    if keys.contains(&"openssl") {
        sync_ssl_dir(b.root, &b.certs, b.rows, b.fetcher, say)?;
    }
    let mut done: Vec<Entry> = Vec::new();
    // The bootstrap Python, built on first need; dropping it at the end of `ensure`, on every
    // path, removes it.
    let mut boot: Option<super::bootstrap::Python> = None;
    for &key in keys {
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        let recipe = catalog::recipe(key)
            .ok_or_else(|| message(format!("pyenv: the build catalog has no recipe for {key}")))?;
        let (row, origin) = catalog::resolve(b.rows, key, b.definition)
            .ok_or_else(|| message(format!("pyenv: the build catalog has no {key}")))?;
        let mut vars: Vec<(String, String)> = Vec::new();
        let mut deps: Vec<String> = Vec::new();
        // Tk against the store's X11, when this run built libX11 (M8c, ruling C4).
        let store_x11 = recipe.x11 && done.iter().any(|e| e.key == "libX11");
        let extra: &[&str] = if store_x11 {
            &["xorgproto", "libX11"]
        } else {
            &[]
        };
        for n in recipe.needs.iter().chain(extra) {
            let e = done
                .iter()
                .find(|e| e.key == *n)
                .ok_or_else(|| message(format!("pyenv: {key} needs {n}, built first")))?;
            vars.push((format!("dep:{n}"), e.dir.display().to_string()));
            deps.push(
                e.dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        // Tcl and Tk have their shared script home compiled in (§6.3).
        let home = scripts_home(b.root, key, &row.version);
        let scripts = home.display().to_string();
        vars.push(("scripts".to_string(), scripts.clone()));
        let name = entry_name(
            key,
            &row.version,
            &fingerprint(
                recipe,
                &row,
                &cc,
                &triple,
                &openssldir,
                &scripts,
                &deps,
                store_x11,
            ),
        );
        let prefix = dir.join(&name);
        let lock_path = locks.join(format!("{name}.lock"));
        let lock = std::fs::File::create(&lock_path)
            .map_err(|e| message(format!("pyenv: cannot create {}: {e}", lock_path.display())))?;
        // Polled, not blocking: a blocking lock would hold off Ctrl+C until the other
        // builder finishes (final review I3).
        let mut waiting = false;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    if !waiting {
                        waiting = true;
                        say(&format!(
                            "Waiting for another pyenv building {key} {} ({})",
                            row.version,
                            lock_path.display()
                        ));
                    }
                    if interrupted() {
                        return Err(InstallError::Interrupted);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Err(std::fs::TryLockError::Error(e)) => {
                    return Err(message(format!(
                        "pyenv: cannot lock {}: {e}",
                        lock_path.display()
                    )))
                }
            }
        }
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        // Staging folders of this entry that a killed run left behind (the lock is ours).
        remove_stale(&dir, &format!(".tmp-{name}-"));
        if prefix.is_dir() {
            say(&format!(
                "Using {key} {} from {}",
                row.version,
                prefix.display()
            ));
        } else {
            vars.extend([
                ("prefix".to_string(), prefix.display().to_string()),
                ("jobs".to_string(), jobs.clone()),
                ("triple".to_string(), triple.clone()),
                ("openssldir".to_string(), openssldir.clone()),
            ]);
            let pkgconf = done
                .iter()
                .find(|e| e.key == "pkgconf")
                .map(|e| e.dir.join("bin"));
            if recipe.python {
                let src = b.bootstrap.ok_or_else(|| {
                    message(format!(
                        "pyenv: building {key} needs Python, and this definition has no CPython source tarball"
                    ))
                })?;
                if boot.is_none() {
                    boot = Some(super::bootstrap::build(
                        src,
                        &step_env(b.env, pkgconf.as_deref()),
                        b.fetcher,
                        &jobs,
                        b.verbose,
                        say,
                    )?);
                }
                if let Some(p) = &boot {
                    vars.push(("python".to_string(), p.exe.display().to_string()));
                }
            }
            build_one(
                b,
                recipe,
                &steps_of(recipe, store_x11),
                &row,
                &name,
                &prefix,
                vars,
                pkgconf.as_deref(),
                say,
            )?;
        }
        // The shared script home is filled from the entry, built or reused (§6.3).
        if let Some(d) = script_dir(key) {
            let (from, to) = (prefix.join("lib").join(d), home.join(d));
            place_tree(&from, &to).map_err(|e| {
                message(format!(
                    "pyenv: cannot copy {} into {}: {e}",
                    from.display(),
                    to.display()
                ))
            })?;
        }
        drop(lock);
        done.push(Entry {
            key,
            version: row.version.clone(),
            origin,
            dir: prefix,
        });
    }
    Ok(done)
}

/// Downloads, checks, extracts and builds one library, staged in `.tmp-<name>-<pid>` and
/// renamed to `prefix` only when every step succeeded. A failure or Ctrl+C leaves no entry
/// (Review Focus 2); a failure keeps the log and names it.
#[allow(clippy::too_many_arguments)]
fn build_one(
    b: &Build,
    recipe: &Recipe,
    steps: &[String],
    row: &Row,
    name: &str,
    prefix: &Path,
    mut vars: Vec<(String, String)>,
    pkgconf: Option<&Path>,
    say: &mut dyn FnMut(&str),
) -> R<()> {
    say(&format!("Building {} {}...", recipe.key, row.version));
    let tmp = get(b.env, "TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let pid = std::process::id();
    let work = tmp.join(format!("rpyenv-deps.{name}.{pid}"));
    let log_path = tmp.join(format!("rpyenv-deps.{name}.{pid}.log"));
    let destdir = prefix.with_file_name(format!(".tmp-{name}-{pid}"));
    let stage = destdir.join(prefix.strip_prefix("/").unwrap_or(prefix));
    let cleanup = || {
        let _ = std::fs::remove_dir_all(&destdir);
        let _ = std::fs::remove_dir_all(&work);
    };
    cleanup();
    std::fs::create_dir_all(&work)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", work.display())))?;
    let log = BuildLog::open(&log_path, b.verbose)
        .map_err(|e| message(format!("pyenv: cannot write {}: {e}", log_path.display())))?;
    let result = (|| -> R<()> {
        let kind = Kind::of_url(&row.url);
        let req = FetchRequest {
            file_name: format!("{}-{}{}", recipe.key, row.version, kind.ext()),
            url: row.url.clone(),
            check: Check::Sha256(row.sha256.clone()),
            dest_dir: work.clone(),
        };
        let mut w: &BuildLog = &log;
        let archive = b.fetcher.fetch(&req, &mut w, say)?;
        let (src, skipped) = extract_skipping_outside_links(&archive, kind, &work, recipe.key)
            .map_err(|e| {
                log.line(&e);
                InstallError::Failed
            })?;
        for link in &skipped {
            log.line(&format!(
                "skipped a link that leaves the source tree: {link}"
            ));
        }
        let cwd = if recipe.dir.is_empty() {
            src
        } else {
            src.join(recipe.dir)
        };
        vars.push(("destdir".to_string(), destdir.display().to_string()));
        vars.push(("stage".to_string(), stage.display().to_string()));
        let env = step_env(b.env, pkgconf);
        for step in steps {
            let line = expand(step, &vars);
            log.line(&format!("+ {line}"));
            let mut c = Command::new("sh");
            c.arg("-c")
                .arg(&line)
                .current_dir(&cwd)
                .env_clear()
                .envs(env.iter().map(|(k, v)| (k, v)));
            logged(&log, &mut c)?;
        }
        let pc = stage.join("lib").join("pkgconfig");
        if let Ok(rd) = std::fs::read_dir(&pc) {
            for e in rd.filter_map(Result::ok) {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "pc") {
                    if let Ok(t) = std::fs::read_to_string(&p) {
                        let _ = std::fs::write(&p, fold_private(&t));
                    }
                }
            }
        }
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        std::fs::rename(&stage, prefix).map_err(|e| {
            message(format!(
                "pyenv: cannot move {} into {}: {e}",
                stage.display(),
                prefix.display()
            ))
        })
    })();
    cleanup();
    match result {
        Ok(()) => {
            drop(log);
            let _ = std::fs::remove_file(&log_path);
            Ok(())
        }
        Err(InstallError::Interrupted) => {
            drop(log);
            let _ = std::fs::remove_file(&log_path);
            Err(InstallError::Interrupted)
        }
        Err(e) => {
            if let InstallError::Message(m) = &e {
                say(m);
            }
            say(&format!(
                "pyenv: building {} {} failed; the log is {}",
                recipe.key,
                row.version,
                log_path.display()
            ));
            Err(InstallError::Failed)
        }
    }
}

/// Makes `<root>/.rpyenv/ssl` match the system (M8 design §6.5), every time the store uses
/// OpenSSL and at the start of an install that finds the folder: `cert.pem` links to the
/// system's bundle, else it is the catalog's pinned bundle; `certs` links to the system's
/// folder, or goes away. Links are made under a temporary name and renamed. A pinned bundle
/// that is a regular file with the catalog's SHA-256 is kept. `Ok(true)` when it wrote the
/// pinned bundle.
pub fn sync_ssl_dir(
    root: &Path,
    sys: &SystemCerts,
    rows: &[Row],
    fetcher: &Fetcher,
    say: &mut dyn FnMut(&str),
) -> R<bool> {
    let dir = ssl_dir(root);
    std::fs::create_dir_all(&dir)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", dir.display())))?;
    let cert = dir.join("cert.pem");
    let mut wrote = false;
    match &sys.bundle {
        Some(b) => relink(&cert, b)?,
        None => wrote = place_pinned(&dir, &cert, rows, fetcher, say)?,
    }
    let certs = dir.join("certs");
    match &sys.dir {
        Some(d) => relink(&certs, d)?,
        None => {
            if std::fs::symlink_metadata(&certs).is_ok_and(|m| m.file_type().is_symlink()) {
                let _ = std::fs::remove_file(&certs);
            }
        }
    }
    Ok(wrote)
}

/// `link` as a symlink to `target` as found (not resolved: a deep path such as
/// `/var/lib/ca-certificates/...` can move in an upgrade, the public one stays), replaced
/// atomically unless it is that already.
fn relink(link: &Path, target: &Path) -> R<()> {
    if std::fs::read_link(link).is_ok_and(|t| t == target) {
        return Ok(());
    }
    let name = link.file_name().unwrap_or_default().to_string_lossy();
    let tmp = link.with_file_name(format!(".{name}.tmp{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(target, &tmp)
        .and_then(|()| std::fs::rename(&tmp, link))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            message(format!("pyenv: cannot link {}: {e}", link.display()))
        })
}

/// The catalog's CA bundle at `target`, unless a regular file there already has the
/// catalog's SHA-256 (a link there is replaced).
fn place_pinned(
    dir: &Path,
    target: &Path,
    rows: &[Row],
    fetcher: &Fetcher,
    say: &mut dyn FnMut(&str),
) -> R<bool> {
    // A killed run left its fetch folder: removed unless its process runs.
    remove_stale(dir, ".fetch-");
    let row = rows
        .iter()
        .find(|r| r.key == "cacert")
        .ok_or_else(|| message("pyenv: the build catalog has no cacert row".into()))?;
    let regular = std::fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_file());
    if regular && sha256_file(target).is_ok_and(|h| h == row.sha256) {
        return Ok(false);
    }
    // Fetched in a folder of its own (two runs may place the bundle at once), then renamed
    // over `cert.pem`.
    let work = dir.join(format!(".fetch-{}", std::process::id()));
    std::fs::create_dir_all(&work)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", work.display())))?;
    let req = FetchRequest {
        file_name: format!("cacert-{}.pem", row.version),
        url: row.url.clone(),
        check: Check::Sha256(row.sha256.clone()),
        dest_dir: work.clone(),
    };
    let got = fetcher.fetch(&req, &mut std::io::sink(), say);
    let placed = got.and_then(|got| {
        std::fs::rename(&got, target)
            .map_err(|e| message(format!("pyenv: cannot write {}: {e}", target.display())))
    });
    let _ = std::fs::remove_dir_all(&work);
    placed?;
    Ok(true)
}

/// What CPython's build needs from the store (M8 design §6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub entries: Vec<Entry>,
    pub cppflags: Vec<String>,
    pub ldflags: Vec<String>,
    pub pkg_config_path: Vec<PathBuf>,
    /// pkgconf's `bin/`, first on `PATH` (ruling P10).
    pub path: Option<PathBuf>,
    pub configure: Vec<String>,
    pub configure_env: Vec<(String, String)>,
    /// Tcl's and Tk's script folders, copied into `<prefix>/lib/` (§6.3).
    pub scripts: Vec<PathBuf>,
}

/// `name='value'` from a `tclConfig.sh`/`tkConfig.sh`, with its runs of spaces collapsed.
fn config_value(text: &str, name: &str) -> Option<String> {
    let v = text
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{name}=")))?;
    let v = v.trim().trim_matches('\'');
    Some(v.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// CPython's flags for `entries` and CPython 3.`minor` (measured, plan M8b): `-I`/`-L` and
/// `pkg-config` folders for every library but OpenSSL (`--with-openssl`) and pkgconf (`PATH`);
/// Tcl/Tk through `TCLTK_CFLAGS`/`TCLTK_LIBS` from 3.11 on and through
/// `--with-tcltk-includes`/`--with-tcltk-libs` before. `read` reads Tk's `tkConfig.sh`.
pub fn link(entries: Vec<Entry>, minor: u32, read: &dyn Fn(&Path) -> Option<String>) -> Linked {
    let mut l = Linked {
        entries: Vec::new(),
        cppflags: Vec::new(),
        ldflags: Vec::new(),
        pkg_config_path: Vec::new(),
        path: None,
        configure: Vec::new(),
        configure_env: Vec::new(),
        scripts: Vec::new(),
    };
    let dir_of = |k: &str| entries.iter().find(|e| e.key == k).map(|e| e.dir.clone());
    for e in &entries {
        match e.key {
            "pkgconf" => l.path = Some(e.dir.join("bin")),
            "openssl" => l
                .configure
                .push(format!("--with-openssl={}", e.dir.display())),
            // The X11 entries reach CPython only through Tk's `TK_LIBS` (ruling C5).
            k if catalog::X11.contains(&k) => {}
            k => {
                l.cppflags.push(format!("-I{}/include", e.dir.display()));
                for inc in catalog::recipe(k).map(|r| r.includes).unwrap_or(&[]) {
                    l.cppflags.push(format!("-I{}/{inc}", e.dir.display()));
                }
                l.ldflags.push(format!("-L{}/lib", e.dir.display()));
                l.pkg_config_path.push(e.dir.join("lib").join("pkgconfig"));
            }
        }
    }
    if let (Some(tcl), Some(tk)) = (dir_of("tcl"), dir_of("tk")) {
        let tk_libs = read(&tk.join("lib").join("tkConfig.sh"))
            .and_then(|t| config_value(&t, "TK_LIBS"))
            .unwrap_or_default();
        let cflags = format!("-I{}/include -I{}/include", tcl.display(), tk.display());
        let libs = format!(
            "-L{}/lib -ltk8.6 -L{}/lib -ltcl8.6 {tk_libs}",
            tk.display(),
            tcl.display()
        )
        .trim_end()
        .to_string();
        if minor <= 10 {
            l.configure.push(format!("--with-tcltk-includes={cflags}"));
            l.configure.push(format!("--with-tcltk-libs={libs}"));
        } else {
            l.configure_env.push(("TCLTK_CFLAGS".into(), cflags));
            l.configure_env.push(("TCLTK_LIBS".into(), libs));
        }
        l.scripts = vec![tcl.join("lib").join("tcl8.6"), tk.join("lib").join("tk8.6")];
    }
    l.entries = entries;
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::bootstrap;
    use crate::install::defs::Definition;
    use std::io::Write;

    /// The fake's `configure`: counts its runs in COUNTER, records its environment, stops at
    /// FAIL when asked to, remembers `--prefix`, and writes a Makefile whose `install` runs
    /// `install.sh` with DESTDIR.
    const CONFIGURE: &str = r#"#!/bin/sh
echo run >> 'COUNTER'
env > configure-env.txt
FAIL
for a in "$@"; do case "$a" in --prefix=*) echo "${a#--prefix=}" > prefix.txt;; esac; done
printf 'all:\n\ttrue\ninstall:\n\tsh ./install.sh "$(DESTDIR)"\n' > Makefile
"#;

    /// The fake's `make install`: a header, the recorded environment and a `.pc` file with a
    /// `Libs.private` line, under `$DESTDIR<prefix>`.
    const INSTALL: &str = r#"p="$1$(cat prefix.txt)"
mkdir -p "$p/include" "$p/lib/pkgconfig"
echo zlib > "$p/include/zlib.h"
cp configure-env.txt "$p/lib/env.txt"
printf 'Libs: -lz\nLibs.private: -lm\n' > "$p/lib/pkgconfig/zlib.pc"
"#;

    /// A fake OpenSSL `config`: a Makefile with the two targets the recipe runs.
    const OPENSSL_CONFIG: &str = r#"#!/bin/sh
for a in "$@"; do case "$a" in --prefix=*) echo "${a#--prefix=}" > prefix.txt;; esac; done
printf 'build_libs:
	true
install_dev:
	mkdir -p "$(DESTDIR)$$(cat prefix.txt)/include"
' > Makefile
"#;

    /// A `zlib`-named fake source tarball. `fail`: its `configure` exits 1.
    fn fake_tarball(dir: &Path, counter: &Path, fail: bool) -> (PathBuf, String) {
        let configure = CONFIGURE
            .replace("COUNTER", &counter.display().to_string())
            .replace("FAIL", if fail { "exit 1" } else { "" });
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, body, mode) in [
            ("configure", configure.as_str(), 0o755),
            ("install.sh", INSTALL, 0o644),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(mode);
            b.append_data(&mut h, format!("zlib-0.1/{name}"), body.as_bytes())
                .unwrap();
        }
        let mut gz = b.into_inner().unwrap().finish().unwrap();
        gz.flush().unwrap();
        let path = dir.join("zlib-0.1.tar.gz");
        std::fs::write(&path, &gz).unwrap();
        let sha = crate::install::checksum::sha256_file(&path).unwrap();
        (path, sha)
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        counter: PathBuf,
        rows: Vec<Row>,
        env: Vec<(OsString, OsString)>,
    }

    fn fixture(fail: bool) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let counter = tmp.path().join("configure-runs");
        let (tarball, sha) = fake_tarball(tmp.path(), &counter, fail);
        let rows = vec![Row {
            key: "zlib".into(),
            version: "0.1".into(),
            url: format!("file://{}", tarball.display()),
            sha256: sha,
        }];
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        env.push(("TMPDIR".into(), tmp.path().join("tmp").into()));
        env.push(("CPPFLAGS".into(), "-I/user/include".into()));
        std::fs::create_dir_all(tmp.path().join("tmp")).unwrap();
        Fixture {
            _tmp: tmp,
            root,
            counter,
            rows,
            env,
        }
    }

    fn run(f: &Fixture, said: &mut Vec<String>) -> Result<Vec<Entry>, InstallError> {
        run_keys(f, &["zlib"], &mut |s: &str| said.push(s.to_string()))
    }

    fn run_keys(
        f: &Fixture,
        keys: &[&'static str],
        say: &mut dyn FnMut(&str),
    ) -> Result<Vec<Entry>, InstallError> {
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &f.rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: ssl_dir(&f.root),
            certs: SystemCerts::default(),
            verbose: false,
            bootstrap: None,
        };
        ensure(&b, keys, say)
    }

    /// A fake Tcl source tree: `unix/configure` writes a Makefile whose build records the
    /// `TCL_LIBRARY` it was given and whose install puts `lib/tcl8.6/init.tcl` and that record
    /// under `$INSTALL_ROOT<prefix>`.
    const TCL_CONFIGURE: &str = r#"#!/bin/sh
for a in "$@"; do case "$a" in --prefix=*) echo "${a#--prefix=}" > prefix.txt;; esac; done
printf 'binaries libraries:\n\techo "$(TCL_LIBRARY)" > tcl_library.txt\ninstall-binaries install-libraries install-headers:\n\tsh ./install.sh "$(INSTALL_ROOT)"\n' > Makefile
"#;

    const TCL_INSTALL: &str = r#"p="$1$(cat prefix.txt)"
mkdir -p "$p/lib/tcl8.6/encoding"
echo init > "$p/lib/tcl8.6/init.tcl"
echo enc > "$p/lib/tcl8.6/encoding/ascii.enc"
cp tcl_library.txt "$p/lib/tcl_library.txt"
"#;

    fn tcl_fixture() -> Fixture {
        let mut f = fixture(false);
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, body, mode) in [
            ("configure", TCL_CONFIGURE, 0o755),
            ("install.sh", TCL_INSTALL, 0o644),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(mode);
            b.append_data(&mut h, format!("tcl0.1/unix/{name}"), body.as_bytes())
                .unwrap();
        }
        let gz = b.into_inner().unwrap().finish().unwrap();
        let path = f._tmp.path().join("tcl0.1-src.tar.gz");
        std::fs::write(&path, &gz).unwrap();
        f.rows = vec![Row {
            key: "tcl".into(),
            version: "0.1".into(),
            url: format!("file://{}", path.display()),
            sha256: crate::install::checksum::sha256_file(&path).unwrap(),
        }];
        f
    }

    /// Final review I1: a Tcl entry, built or reused, fills `<root>/.rpyenv/tcltk/tcl-<v>/`,
    /// the folder its build compiled in, so venvs find `init.tcl`.
    #[test]
    fn a_tcl_entry_fills_the_shared_script_home_and_refills_it_when_deleted() {
        let f = tcl_fixture();
        let e = run_keys(&f, &["tcl"], &mut |_| {}).unwrap();
        let home = f.root.join(".rpyenv/tcltk/tcl-0.1");
        assert!(home.join("tcl8.6/init.tcl").is_file());
        assert!(home.join("tcl8.6/encoding/ascii.enc").is_file());
        let compiled = std::fs::read_to_string(e[0].dir.join("lib/tcl_library.txt")).unwrap();
        assert_eq!(compiled.trim(), home.join("tcl8.6").display().to_string());
        let names = |d: &Path| -> Vec<String> {
            std::fs::read_dir(d)
                .unwrap()
                .map(|x| x.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(&home), ["tcl8.6"], "nothing temporary is left");
        std::fs::remove_dir_all(&home).unwrap();
        let mut said = Vec::new();
        let again = run_keys(&f, &["tcl"], &mut |s: &str| said.push(s.to_string())).unwrap();
        assert_eq!(again, e);
        assert!(
            said.iter().any(|l| l.starts_with("Using tcl 0.1 from ")),
            "{said:?}"
        );
        assert!(
            home.join("tcl8.6/init.tcl").is_file(),
            "the reused entry refilled it"
        );
    }

    /// H1: a Tcl script folder that lost its marker (partly removed) is replaced by a whole
    /// one when the entry is reused.
    // allowlist D-123
    #[test]
    fn a_partly_removed_script_folder_is_refilled_by_ensure() {
        let f = tcl_fixture();
        run_keys(&f, &["tcl"], &mut |_| {}).unwrap();
        let home = f.root.join(".rpyenv/tcltk/tcl-0.1");
        let folder = home.join("tcl8.6");
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("junk"), "x").unwrap();
        run_keys(&f, &["tcl"], &mut |_| {}).unwrap();
        assert!(folder.join("init.tcl").is_file());
        assert!(folder.join("encoding/ascii.enc").is_file());
        assert!(!folder.join("junk").exists());
        let names: Vec<String> = std::fs::read_dir(&home)
            .unwrap()
            .map(|x| x.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["tcl8.6"], "no .tmp- or .del- folder is left");
    }

    /// H10: while it holds an entry's lock, `ensure` removes that entry's staging folders left
    /// by a run that died; a running process's, and other entries', stay.
    #[test]
    fn ensure_removes_the_staging_folders_of_dead_runs_of_its_entry() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        let deps = deps_dir(&f.root);
        let name = e[0].dir.file_name().unwrap().to_string_lossy().into_owned();
        let stale = deps.join(format!(".tmp-{name}-4294967294"));
        let live = deps.join(format!(".tmp-{name}-1"));
        let other = deps.join(".tmp-other-0.1-abc-4294967294");
        for d in [&stale, &live, &other] {
            std::fs::create_dir_all(d.join("x")).unwrap();
        }
        run(&f, &mut Vec::new()).unwrap();
        assert!(!stale.exists());
        assert!(live.is_dir());
        assert!(other.is_dir());
    }

    /// Final review I3: a builder waiting for another one's lock says so, and goes on when the
    /// lock is released.
    #[test]
    fn a_builder_waiting_for_the_lock_says_so_then_reuses_the_entry() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        let f = Arc::new(fixture(false));
        run(&f, &mut Vec::new()).unwrap();
        let locks = deps_dir(&f.root).join(".locks");
        let lock_path = std::fs::read_dir(&locks)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let held = std::fs::File::create(&lock_path).unwrap();
        held.lock().unwrap();
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let t = {
            let (f, said) = (f.clone(), said.clone());
            std::thread::spawn(move || {
                run_keys(&f, &["zlib"], &mut |s: &str| {
                    said.lock().unwrap().push(s.into())
                })
            })
        };
        let want = format!(
            "Waiting for another pyenv building zlib 0.1 ({})",
            lock_path.display()
        );
        let start = Instant::now();
        while !said.lock().unwrap().contains(&want) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{:?}",
                said.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300));
        assert!(!t.is_finished(), "it went on while the lock was held");
        drop(held);
        t.join().unwrap().unwrap();
        let said = said.lock().unwrap();
        assert_eq!(said.iter().filter(|l| **l == want).count(), 1, "{said:?}");
        assert!(
            said.iter().any(|l| l.starts_with("Using zlib 0.1 from ")),
            "{said:?}"
        );
        assert_eq!(runs(&f), 1);
    }

    fn runs(f: &Fixture) -> usize {
        std::fs::read_to_string(&f.counter)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    #[test]
    fn a_library_is_built_once_and_reused() {
        let f = fixture(false);
        let mut said = Vec::new();
        let e = run(&f, &mut said).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(
            (e[0].key, e[0].version.as_str(), e[0].origin),
            ("zlib", "0.1", Origin::Catalog)
        );
        assert!(
            e[0].dir.starts_with(deps_dir(&f.root)),
            "{}",
            e[0].dir.display()
        );
        assert!(e[0].dir.join("include/zlib.h").is_file());
        assert_eq!(
            said.iter()
                .filter(|l| l.starts_with("Building zlib"))
                .count(),
            1,
            "{said:?}"
        );
        assert!(said.iter().any(|l| l == "Building zlib 0.1..."), "{said:?}");
        // Nothing staged is left beside the entry.
        let left: Vec<String> = std::fs::read_dir(deps_dir(&f.root))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".tmp-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let mut again = Vec::new();
        let e2 = run(&f, &mut again).unwrap();
        assert_eq!(e2, e);
        assert_eq!(runs(&f), 1, "configure ran again");
        assert!(
            again.iter().any(|l| l.starts_with("Using zlib 0.1 from ")),
            "{again:?}"
        );
    }

    /// Ruling P7: the library build sees `CFLAGS=-O2 -fPIC`, not the user's `CPPFLAGS`.
    #[test]
    fn the_library_build_gets_pic_flags_and_not_the_users_cppflags() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        let env = std::fs::read_to_string(e[0].dir.join("lib/env.txt")).unwrap();
        assert!(env.lines().any(|l| l == "CFLAGS=-O2 -fPIC"), "{env}");
        assert!(!env.contains("/user/include"), "{env}");
    }

    #[test]
    fn libs_private_is_folded_into_libs_in_the_entrys_pc_files() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        let pc = std::fs::read_to_string(e[0].dir.join("lib/pkgconfig/zlib.pc")).unwrap();
        assert_eq!(pc, "Libs: -lz -lm\n");
    }

    #[test]
    fn a_failed_build_leaves_no_entry_and_names_its_log() {
        let f = fixture(true);
        let mut said = Vec::new();
        assert_eq!(run(&f, &mut said), Err(InstallError::Failed));
        let names: Vec<String> = std::fs::read_dir(deps_dir(&f.root))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != ".locks")
            .collect();
        assert!(names.is_empty(), "{names:?}");
        let line = said
            .iter()
            .find(|l| l.starts_with("pyenv: building zlib 0.1 failed; the log is "))
            .expect("no failure line");
        let log = line.rsplit(' ').next().unwrap();
        assert!(Path::new(log).is_file(), "{log}");
    }

    // Review Focus 3
    #[test]
    fn a_missing_entry_is_built_again() {
        let f = fixture(false);
        let e = run(&f, &mut Vec::new()).unwrap();
        std::fs::remove_dir_all(deps_dir(&f.root)).unwrap();
        let e2 = run(&f, &mut Vec::new()).unwrap();
        assert_eq!(e2, e);
        assert_eq!(runs(&f), 2);
    }

    // Review Focus 1
    #[test]
    fn two_builders_of_one_entry_build_it_once() {
        let f = std::sync::Arc::new(fixture(false));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let f = f.clone();
                std::thread::spawn(move || run(&f, &mut Vec::new()).unwrap())
            })
            .collect();
        let got: Vec<Vec<Entry>> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(got[0], got[1]);
        assert_eq!(runs(&f), 1, "both threads built it");
    }

    #[test]
    fn tk_with_the_stores_x11_gets_the_x11_args_and_another_entry() {
        let tk = catalog::recipe("tk").unwrap();
        assert_eq!(
            steps_of(tk, false),
            tk.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        );
        let with = steps_of(tk, true);
        assert!(with[0].ends_with(tk.x11_args), "{}", with[0]);
        assert_eq!(&with[1..], &steps_of(tk, false)[1..]);
        let row = Row {
            key: "tk".into(),
            version: "1".into(),
            url: "u".into(),
            sha256: "s".into(),
        };
        let deps = vec!["tcl-1-x".to_string()];
        assert_ne!(
            fingerprint(tk, &row, "cc", "t", "o", "s", &deps, true),
            fingerprint(tk, &row, "cc", "t", "o", "s", &deps, false)
        );
    }

    // Review Focus 3
    #[test]
    fn a_python_recipe_reused_from_the_store_needs_no_bootstrap() {
        let f = fixture(false);
        let row = Row {
            key: "xcb-proto".into(),
            version: "1.0".into(),
            url: "file:///nonexistent/xcb-proto-1.0.tar.xz".into(),
            sha256: "a".repeat(64),
        };
        let rows = vec![row.clone()];
        let cc = get(&f.env, "CC")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "cc".into());
        let triple = triple_of(&cc, &f.env);
        let name = entry_name(
            "xcb-proto",
            "1.0",
            &fingerprint(
                catalog::recipe("xcb-proto").unwrap(),
                &row,
                &cc,
                &triple,
                &ssl_dir(&f.root).display().to_string(),
                &scripts_home(&f.root, "xcb-proto", "1.0")
                    .display()
                    .to_string(),
                &[],
                false,
            ),
        );
        std::fs::create_dir_all(deps_dir(&f.root).join(&name)).unwrap();
        let boot = bootstrap::Source {
            name: "Python-9.9.9".into(),
            url: "file:///nonexistent/Python-9.9.9.tar.xz".into(),
            sha256: "b".repeat(64),
            patches: vec![],
        };
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: ssl_dir(&f.root),
            certs: SystemCerts::default(),
            verbose: false,
            bootstrap: Some(&boot),
        };
        let mut said = Vec::new();
        ensure(&b, &["xcb-proto"], &mut |s: &str| said.push(s.to_string())).unwrap();
        assert!(
            said.iter()
                .any(|s| s.starts_with("Using xcb-proto 1.0 from ")),
            "{said:?}"
        );
        assert!(!said.iter().any(|s| s.contains("bootstrap")), "{said:?}");
    }

    #[test]
    fn a_python_recipe_without_a_cpython_tarball_fails_before_downloading() {
        let f = fixture(false);
        let rows = vec![Row {
            key: "xcb-proto".into(),
            version: "1.0".into(),
            url: "file:///nonexistent/xcb-proto-1.0.tar.xz".into(),
            sha256: "a".repeat(64),
        }];
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: ssl_dir(&f.root),
            certs: SystemCerts::default(),
            verbose: false,
            bootstrap: None,
        };
        let r = ensure(&b, &["xcb-proto"], &mut |_: &str| {});
        match r {
            Err(InstallError::Message(m)) => assert_eq!(
                m,
                "pyenv: building xcb-proto needs Python, and this definition has no CPython source tarball"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn link_leaves_the_x11_entries_to_tks_flags() {
        let e = |key: &'static str, dir: &str| Entry {
            key,
            version: "1".into(),
            origin: Origin::Catalog,
            dir: PathBuf::from(dir),
        };
        let entries = vec![
            e("xorgproto", "/s/xorgproto-1-a"),
            e("libX11", "/s/libX11-1-b"),
            e("tcl", "/s/tcl-1-c"),
            e("tk", "/s/tk-1-d"),
        ];
        let l = link(entries, 12, &|p: &Path| {
            p.ends_with("tkConfig.sh").then(|| {
                "TK_LIBS='-L/s/libX11-1-b/lib -lX11   -lpthread -lm'
"
                .to_string()
            })
        });
        let all = format!("{:?} {:?} {:?}", l.cppflags, l.ldflags, l.pkg_config_path);
        assert!(
            !all.contains("libX11") && !all.contains("xorgproto"),
            "{all}"
        );
        let libs = &l
            .configure_env
            .iter()
            .find(|(k, _)| k == "TCLTK_LIBS")
            .unwrap()
            .1;
        assert!(
            libs.ends_with("-L/s/libX11-1-b/lib -lX11 -lpthread -lm"),
            "{libs}"
        );
        assert_eq!(l.entries.len(), 4, "the record still names every entry");
    }

    // Review Focus 2: with the system's X11, an rpyenv upgrade keeps M8b's entries.
    #[test]
    fn tk_without_store_x11_keeps_its_m8b_entry_name() {
        let rows = catalog::parse(include_str!("../../deps/catalog.tsv")).unwrap();
        let row = rows.iter().find(|r| r.key == "tk").unwrap();
        assert_eq!(
            row.version, "8.6.18",
            "update this pin when the catalog moves Tk"
        );
        let fp = fingerprint(
            catalog::recipe("tk").unwrap(),
            row,
            "cc",
            "x86_64-linux-gnu",
            "/r/.rpyenv/ssl",
            "/r/.rpyenv/tcltk/tk-8.6.18",
            &["tcl-8.6.18-aaaaaaaaaaaa".to_string()],
            false,
        );
        assert_eq!(entry_name("tk", "8.6.18", &fp), "tk-8.6.18-c8a45ea4f16c");
    }

    #[test]
    fn the_entry_name_changes_with_every_input_of_the_fingerprint() {
        let r = catalog::recipe("openssl").unwrap();
        let row = Row {
            key: "openssl".into(),
            version: "3.6.5".into(),
            url: "u".into(),
            sha256: "a".repeat(64),
        };
        let base = fingerprint(
            r,
            &row,
            "cc",
            "x86_64-linux-gnu",
            "/etc/ssl",
            "/h",
            &[],
            false,
        );
        let name = entry_name("openssl", "3.6.5", &base);
        assert!(
            name.starts_with("openssl-3.6.5-") && name.len() == "openssl-3.6.5-".len() + 12,
            "{name}"
        );
        assert_eq!(name, entry_name("openssl", "3.6.5", &base), "stable");
        for other in [
            fingerprint(
                r,
                &row,
                "gcc-14",
                "x86_64-linux-gnu",
                "/etc/ssl",
                "/h",
                &[],
                false,
            ),
            fingerprint(
                r,
                &row,
                "cc",
                "aarch64-linux-gnu",
                "/etc/ssl",
                "/h",
                &[],
                false,
            ),
            fingerprint(
                r,
                &row,
                "cc",
                "x86_64-linux-gnu",
                "/etc/pki/tls",
                "/h",
                &[],
                false,
            ),
            fingerprint(
                r,
                &row,
                "cc",
                "x86_64-linux-gnu",
                "/etc/ssl",
                "/h",
                &["dep-1".into()],
                false,
            ),
            fingerprint(
                r,
                &Row {
                    sha256: "b".repeat(64),
                    ..row.clone()
                },
                "cc",
                "x86_64-linux-gnu",
                "/etc/ssl",
                "/h",
                &[],
                false,
            ),
        ] {
            assert_ne!(entry_name("openssl", "3.6.5", &other), name);
        }
        // OPENSSLDIR only matters for OpenSSL.
        let z = catalog::recipe("zlib").unwrap();
        let zrow = Row {
            key: "zlib".into(),
            ..row.clone()
        };
        assert_eq!(
            fingerprint(z, &zrow, "cc", "t", "/etc/ssl", "/h", &[], false),
            fingerprint(z, &zrow, "cc", "t", "/etc/pki/tls", "/h", &[], false)
        );
        // The script home only matters for Tcl and Tk, which have it compiled in.
        assert_eq!(
            fingerprint(z, &zrow, "cc", "t", "/etc/ssl", "/h", &[], false),
            fingerprint(z, &zrow, "cc", "t", "/etc/ssl", "/other", &[], false)
        );
        for k in ["tcl", "tk"] {
            let r = catalog::recipe(k).unwrap();
            let row = Row {
                key: k.into(),
                ..row.clone()
            };
            assert_ne!(
                fingerprint(
                    r,
                    &row,
                    "cc",
                    "t",
                    "/etc/ssl",
                    "/r1/.rpyenv/tcltk/x",
                    &[],
                    false
                ),
                fingerprint(
                    r,
                    &row,
                    "cc",
                    "t",
                    "/etc/ssl",
                    "/r2/.rpyenv/tcltk/x",
                    &[],
                    false
                ),
                "{k}"
            );
        }
    }

    /// Final review I1: Tcl and Tk compile in the shared script home, never a store path.
    #[test]
    fn tcl_and_tk_compile_in_the_shared_script_home() {
        for (k, var) in [
            ("tcl", "TCL_LIBRARY={scripts}/tcl8.6"),
            ("tk", "TK_LIBRARY={scripts}/tk8.6"),
        ] {
            let r = catalog::recipe(k).unwrap();
            assert!(
                r.steps.iter().any(|s| s.contains(var)),
                "{k}: {:?}",
                r.steps
            );
            assert!(!r.steps.iter().any(|s| s.contains("/nonexistent")), "{k}");
        }
        assert_eq!(
            scripts_home(Path::new("/r"), "tcl", "8.6.18"),
            Path::new("/r/.rpyenv/tcltk/tcl-8.6.18")
        );
        assert!(!tcltk_dir(Path::new("/r")).starts_with(deps_dir(Path::new("/r"))));
    }

    #[test]
    fn expand_fills_every_placeholder() {
        let vars = vec![
            ("prefix".to_string(), "/r/e".to_string()),
            ("dep:tcl".to_string(), "/r/t".to_string()),
        ];
        assert_eq!(
            expand("--prefix={prefix} --with-tcl={dep:tcl}/lib", &vars),
            "--prefix=/r/e --with-tcl=/r/t/lib"
        );
    }

    #[test]
    fn fold_private_moves_libs_private_into_libs() {
        assert_eq!(
            fold_private(
                "prefix=/x\nLibs: -L${libdir} -lsqlite3\nLibs.private: -lm -lz\nCflags: -I/x\n"
            ),
            "prefix=/x\nLibs: -L${libdir} -lsqlite3 -lm -lz\nCflags: -I/x\n"
        );
        assert_eq!(fold_private("Libs: -lz\n"), "Libs: -lz\n");
    }

    #[test]
    fn plain_paths_have_no_spaces_or_shell_characters() {
        assert!(plain_path(Path::new("/home/u/.pyenv")));
        assert!(plain_path(Path::new("/opt/py-env_2.0/root")));
        for bad in [
            "/home/u/my pyenv",
            "/a\tb",
            "/a'b",
            "/a\"b",
            "/a$b",
            "/a;b",
            "/a`b",
            "/a\\b",
            "/a*b",
            "/a(b",
            // A `:` would split PATH and PKG_CONFIG_PATH.
            "/home/u/a:b",
            "",
        ] {
            assert!(!plain_path(Path::new(bad)), "{bad:?}");
        }
    }

    /// A second `[b]` round reuses ncurses: CPython's flags and the record name it once.
    #[test]
    fn add_unique_keeps_one_entry_per_folder() {
        let mut all = vec![
            entry("ncurses", "/s/ncurses-1-a"),
            entry("readline", "/s/rl-1-b"),
        ];
        add_unique(
            &mut all,
            vec![
                entry("ncurses", "/s/ncurses-1-a"),
                entry("sqlite", "/s/sq-1-c"),
            ],
        );
        let dirs: Vec<&Path> = all.iter().map(|e| e.dir.as_path()).collect();
        assert_eq!(
            dirs,
            ["/s/ncurses-1-a", "/s/rl-1-b", "/s/sq-1-c"].map(Path::new)
        );
    }

    #[test]
    fn system_certs_prefer_the_distribution_files_over_the_openssl_dir() {
        // A distribution file and an `openssl version -d` answer (conda's, say): the file wins.
        let files = [
            "/etc/ssl/certs/ca-certificates.crt",
            "/opt/conda/ssl/cert.pem",
        ];
        let is_file = |p: &str| files.contains(&p);
        let dirs = |d: &str| d == "/etc/pki/tls/certs" || d == "/opt/conda/ssl/certs";
        let c = system_certs(Some("/opt/conda/ssl"), &is_file, &dirs);
        assert_eq!(
            c.bundle,
            Some(PathBuf::from("/etc/ssl/certs/ca-certificates.crt"))
        );
        assert_eq!(c.dir, Some(PathBuf::from("/etc/pki/tls/certs")));
        // Only the openssl dir has any: the last fallback.
        let only = |p: &str| p == "/opt/conda/ssl/cert.pem";
        let c = system_certs(Some("/opt/conda/ssl"), &only, &|d| {
            d == "/opt/conda/ssl/certs"
        });
        assert_eq!(c.bundle, Some(PathBuf::from("/opt/conda/ssl/cert.pem")));
        assert_eq!(c.dir, Some(PathBuf::from("/opt/conda/ssl/certs")));
        // Several distribution files: Debian's comes before Fedora's.
        let both = |p: &str| {
            p == "/etc/pki/tls/certs/ca-bundle.crt" || p == "/etc/ssl/certs/ca-certificates.crt"
        };
        assert_eq!(
            system_certs(None, &both, &|_| false).bundle,
            Some(PathBuf::from("/etc/ssl/certs/ca-certificates.crt"))
        );
        assert_eq!(
            system_certs(None, &|_| false, &|_| false),
            SystemCerts::default()
        );
    }

    /// A catalog with only the pinned CA bundle, and the folder it lives in.
    fn ca_fixture() -> (tempfile::TempDir, PathBuf, Vec<Row>, String) {
        let tmp = tempfile::tempdir().unwrap();
        let pem = tmp.path().join("cacert-1.pem");
        std::fs::write(&pem, "pinned\n").unwrap();
        let sha = crate::install::checksum::sha256_file(&pem).unwrap();
        let rows = vec![Row {
            key: "cacert".into(),
            version: "1".into(),
            url: format!("file://{}", pem.display()),
            sha256: sha.clone(),
        }];
        let root = tmp.path().join("root");
        (tmp, root, rows, sha)
    }

    fn sync(root: &Path, rows: &[Row], sys: &SystemCerts) -> bool {
        sync_ssl_dir(root, sys, rows, &Fetcher::direct(), &mut |_: &str| {}).unwrap()
    }

    /// The folder follows the system both ways, and a private folder never holds a config.
    #[test]
    fn the_ssl_dir_links_the_system_certificates_and_follows_changes() {
        let (tmp, root, rows, _) = ca_fixture();
        let bundle = tmp.path().join("sys-bundle.crt");
        let certs = tmp.path().join("sys-certs");
        std::fs::write(&bundle, "system\n").unwrap();
        std::fs::create_dir_all(&certs).unwrap();
        std::fs::write(certs.join("a.pem"), "x").unwrap();
        let dir = ssl_dir(&root);
        let (cert, cdir) = (dir.join("cert.pem"), dir.join("certs"));
        let none = SystemCerts::default();
        let some = SystemCerts {
            bundle: Some(bundle.clone()),
            dir: Some(certs.clone()),
        };
        // No system certificates: a regular file with the pinned bundle, no `certs`.
        assert!(sync(&root, &rows, &none));
        assert!(std::fs::symlink_metadata(&cert)
            .unwrap()
            .file_type()
            .is_file());
        assert_eq!(std::fs::read_to_string(&cert).unwrap(), "pinned\n");
        assert!(std::fs::symlink_metadata(&cdir).is_err());
        // Unchanged: nothing is fetched again.
        assert!(!sync(&root, &rows, &none));
        // The system gains certificates: both become links, and the pinned file is gone.
        assert!(!sync(&root, &rows, &some));
        assert_eq!(std::fs::read_link(&cert).unwrap(), bundle.clone());
        assert_eq!(std::fs::read_link(&cdir).unwrap(), certs.clone());
        assert_eq!(std::fs::read_to_string(&cert).unwrap(), "system\n");
        assert!(!dir.join("openssl.cnf").exists());
        // Idempotent, and no temporary link is left behind.
        assert!(!sync(&root, &rows, &some));
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["cert.pem", "certs"]);
        // The system's bundle moves: the link follows.
        let other = tmp.path().join("other-bundle.crt");
        std::fs::write(
            &other, "pinned
",
        )
        .unwrap();
        let moved = SystemCerts {
            bundle: Some(other.clone()),
            dir: Some(certs.clone()),
        };
        assert!(!sync(&root, &rows, &moved));
        assert_eq!(std::fs::read_link(&cert).unwrap(), other.clone());
        // The system loses them (the old link even points at the pinned content): the pinned bundle again (a file, not a link into the system).
        assert!(sync(&root, &rows, &none));
        assert!(std::fs::symlink_metadata(&cert)
            .unwrap()
            .file_type()
            .is_file());
        assert_eq!(std::fs::read_to_string(&cert).unwrap(), "pinned\n");
        assert!(std::fs::symlink_metadata(&cdir).is_err());
        assert_eq!(std::fs::read_to_string(&bundle).unwrap(), "system\n");
    }

    /// D-127: an outdated pinned file is replaced.
    #[test]
    fn an_outdated_pinned_bundle_is_replaced() {
        let (_tmp, root, rows, sha) = ca_fixture();
        let cert = ssl_dir(&root).join("cert.pem");
        std::fs::create_dir_all(cert.parent().unwrap()).unwrap();
        std::fs::write(&cert, "old\n").unwrap();
        assert!(sync(&root, &rows, &SystemCerts::default()));
        assert_eq!(crate::install::checksum::sha256_file(&cert).unwrap(), sha);
    }

    /// H8: a `.fetch-<pid>` folder of a process that is gone is removed before the bundle is
    /// placed; one of a running process (pid 1 here) is kept.
    #[test]
    fn a_dead_runs_fetch_folder_is_removed_and_a_live_ones_kept() {
        let (_tmp, root, rows, _) = ca_fixture();
        let dir = ssl_dir(&root);
        for n in [".fetch-4294967294", ".fetch-1", ".fetch-notapid"] {
            std::fs::create_dir_all(dir.join(n)).unwrap();
            std::fs::write(dir.join(n).join("partial"), "x").unwrap();
        }
        assert!(sync(&root, &rows, &SystemCerts::default()));
        assert!(!dir.join(".fetch-4294967294").exists());
        assert!(dir.join(".fetch-1/partial").is_file(), "a live pid is kept");
        assert!(dir.join(".fetch-notapid").exists());
        assert!(dir.join("cert.pem").is_file());
    }

    /// A crashed swap's `.tmp-`/`.del-` folders of dead runs are removed by the next
    /// `place_tree`; a live run's (pid 1) stay.
    #[test]
    fn place_tree_removes_the_leftovers_of_dead_runs_only() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("init.tcl"), "x").unwrap();
        let home = t.path().join("home");
        for n in [
            ".tmp-tcl8.6-4294967294",
            ".del-tcl8.6-4294967294",
            ".tmp-tcl8.6-1",
            ".del-tcl8.6-1",
            ".tmp-tk8.6-4294967294",
        ] {
            std::fs::create_dir_all(home.join(n)).unwrap();
        }
        place_tree(&src, &home.join("tcl8.6")).unwrap();
        assert!(home.join("tcl8.6/init.tcl").is_file());
        assert!(!home.join(".tmp-tcl8.6-4294967294").exists());
        assert!(!home.join(".del-tcl8.6-4294967294").exists());
        assert!(home.join(".tmp-tcl8.6-1").is_dir());
        assert!(home.join(".del-tcl8.6-1").is_dir());
        assert!(
            home.join(".tmp-tk8.6-4294967294").is_dir(),
            "another folder's"
        );
    }

    /// H8/H10: the current process's own folder is never taken for a dead one's.
    #[test]
    fn stale_removal_never_touches_the_current_pid() {
        let tmp = tempfile::tempdir().unwrap();
        let mine = tmp.path().join(format!("p-{}", std::process::id()));
        let dead = tmp.path().join("p-4294967294");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&dead).unwrap();
        remove_stale(tmp.path(), "p-");
        assert!(mine.is_dir());
        assert!(!dead.exists());
    }

    /// `ensure` makes the folder when it uses OpenSSL (here a fake one), and not for others.
    #[test]
    fn ensure_makes_the_ssl_dir_only_for_openssl() {
        let f = fixture(false);
        let mut said = Vec::new();
        run(&f, &mut said).unwrap();
        assert!(!ssl_dir(&f.root).exists(), "zlib alone does not need it");

        let tmp = f.root.parent().unwrap();
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut h = tar::Header::new_gnu();
        h.set_size(OPENSSL_CONFIG.len() as u64);
        h.set_mode(0o755);
        tar.append_data(&mut h, "openssl-0.1/config", OPENSSL_CONFIG.as_bytes())
            .unwrap();
        let mut gz = tar.into_inner().unwrap().finish().unwrap();
        gz.flush().unwrap();
        let path = tmp.join("openssl-0.1.tar.gz");
        std::fs::write(&path, &gz).unwrap();
        let sha = crate::install::checksum::sha256_file(&path).unwrap();
        let sys = tmp.join("sys.crt");
        std::fs::write(
            &sys, "system
",
        )
        .unwrap();
        let rows = vec![Row {
            key: "openssl".into(),
            version: "0.1".into(),
            url: format!("file://{}", path.display()),
            sha256: sha,
        }];
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: ssl_dir(&f.root),
            certs: SystemCerts {
                bundle: Some(sys.clone()),
                dir: None,
            },
            verbose: false,
            bootstrap: None,
        };
        ensure(&b, &["openssl"], &mut |_: &str| {}).unwrap();
        assert_eq!(
            std::fs::read_link(ssl_dir(&f.root).join("cert.pem")).unwrap(),
            sys.clone()
        );
    }

    /// Ruling P9: deleting the cache must not break HTTPS for versions that read the folder.
    #[test]
    fn the_ssl_dir_lives_outside_the_cache() {
        let root = Path::new("/r");
        assert_eq!(ssl_dir(root), Path::new("/r/.rpyenv/ssl"));
        assert!(!ssl_dir(root).starts_with(deps_dir(root)));
    }

    fn entry(key: &'static str, dir: &str) -> Entry {
        Entry {
            key,
            version: "1".into(),
            origin: Origin::Catalog,
            dir: PathBuf::from(dir),
        }
    }

    #[test]
    fn link_composes_cpythons_flags_from_the_entries() {
        let entries = vec![
            entry("pkgconf", "/s/pkgconf-1-a"),
            entry("ncurses", "/s/ncurses-1-b"),
            entry("tcl", "/s/tcl-1-c"),
            entry("tk", "/s/tk-1-d"),
            entry("openssl", "/s/openssl-1-e"),
        ];
        let read = |p: &Path| {
            (p == Path::new("/s/tk-1-d/lib/tkConfig.sh"))
                .then(|| "TK_VERSION='8.6'\nTK_LIBS='-lX11   -lpthread -lm'\n".to_string())
        };
        let l = link(entries.clone(), 12, &read);
        assert_eq!(l.path, Some(PathBuf::from("/s/pkgconf-1-a/bin")));
        assert!(l.cppflags.contains(&"-I/s/ncurses-1-b/include".to_string()));
        assert!(l
            .cppflags
            .contains(&"-I/s/ncurses-1-b/include/ncursesw".to_string()));
        assert!(l.ldflags.contains(&"-L/s/ncurses-1-b/lib".to_string()));
        assert!(
            !l.cppflags
                .iter()
                .any(|f| f.contains("openssl") || f.contains("pkgconf")),
            "{:?}",
            l.cppflags
        );
        assert!(l
            .configure
            .contains(&"--with-openssl=/s/openssl-1-e".to_string()));
        assert!(l
            .pkg_config_path
            .contains(&PathBuf::from("/s/ncurses-1-b/lib/pkgconfig")));
        assert_eq!(
            l.configure_env,
            [
                (
                    "TCLTK_CFLAGS".to_string(),
                    "-I/s/tcl-1-c/include -I/s/tk-1-d/include".to_string()
                ),
                (
                    "TCLTK_LIBS".to_string(),
                    "-L/s/tk-1-d/lib -ltk8.6 -L/s/tcl-1-c/lib -ltcl8.6 -lX11 -lpthread -lm"
                        .to_string()
                ),
            ]
        );
        assert_eq!(
            l.scripts,
            [
                PathBuf::from("/s/tcl-1-c/lib/tcl8.6"),
                PathBuf::from("/s/tk-1-d/lib/tk8.6")
            ]
        );
        assert_eq!(l.entries, entries);
        // 3.10 and older take Tcl/Tk through configure options instead.
        let old = link(entries, 10, &read);
        assert!(old.configure_env.is_empty());
        assert!(old.configure.contains(
            &"--with-tcltk-includes=-I/s/tcl-1-c/include -I/s/tk-1-d/include".to_string()
        ));
        assert!(old.configure.contains(&"--with-tcltk-libs=-L/s/tk-1-d/lib -ltk8.6 -L/s/tcl-1-c/lib -ltcl8.6 -lX11 -lpthread -lm".to_string()));
    }
}
