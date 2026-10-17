//! The build store (M8 design §6.2): `<root>/cache/deps/<lib>-<version>-<hash>/`, static
//! libraries compiled from the catalog without root and reused by every CPython version that
//! needs the same library, version and options (D6, D7). It is a cache: CPython links these
//! libraries statically, so deleting `cache/deps` breaks no installed version.

use super::archive::{extract, Kind};
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

/// Where the pinned CA bundle lives when the system has no certificates (M8 design §6.5).
/// Outside `cache/deps` (ruling P9): a self-built OpenSSL has this path compiled in, so the
/// file must survive the cache being deleted.
pub fn ca_dir(root: &Path) -> PathBuf {
    root.join(".rpyenv").join("ca")
}

/// A path the recipes and CPython's flags can carry unquoted (plan ruling P5): not empty, no
/// whitespace or control character, and none of the characters a shell gives a meaning.
pub fn plain_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    !s.is_empty()
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "\"'`$\\;&|<>*?()[]{}!#~".contains(c))
}

/// Where the system keeps CA certificates, for a self-built OpenSSL's `OPENSSLDIR` (§6.5):
/// `/etc/pki/tls` (Fedora, RHEL) or `/etc/ssl` (Debian, Arch, SUSE), whichever has a
/// `cert.pem` or a non-empty `certs/`. `None`: rpyenv's pinned bundle instead.
pub fn system_openssldir(
    exists: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> Option<&'static str> {
    ["/etc/pki/tls", "/etc/ssl"]
        .into_iter()
        .find(|d| exists(&format!("{d}/cert.pem")) || has_files(&format!("{d}/certs")))
}

/// `<key>-<version>-<the first 12 hex digits of the fingerprint's SHA-256>`.
pub fn entry_name(key: &str, version: &str, fingerprint: &str) -> String {
    let h = Sha256::digest(fingerprint.as_bytes());
    let hex: String = h.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("{key}-{version}-{hex}")
}

/// Everything that makes two builds of a library differ (spec D6): its source, its recipe,
/// the compiler and target, `OPENSSLDIR` for OpenSSL, and the entries it was built against.
pub fn fingerprint(
    r: &Recipe,
    row: &Row,
    cc: &str,
    triple: &str,
    openssldir: &str,
    deps: &[String],
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
    for d in deps {
        s.push_str(d);
        s.push('\n');
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
    /// A system folder, or `ca_dir(root)` for the pinned bundle (§6.5).
    pub openssldir: PathBuf,
    pub verbose: bool,
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
    let mut done: Vec<Entry> = Vec::new();
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
        for n in recipe.needs {
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
        let name = entry_name(
            key,
            &row.version,
            &fingerprint(recipe, &row, &cc, &triple, &openssldir, &deps),
        );
        let prefix = dir.join(&name);
        let lock_path = locks.join(format!("{name}.lock"));
        let lock = std::fs::File::create(&lock_path)
            .map_err(|e| message(format!("pyenv: cannot create {}: {e}", lock_path.display())))?;
        lock.lock()
            .map_err(|e| message(format!("pyenv: cannot lock {}: {e}", lock_path.display())))?;
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
            build_one(
                b,
                recipe,
                &row,
                &name,
                &prefix,
                vars,
                pkgconf.as_deref(),
                say,
            )?;
        }
        drop(lock);
        done.push(Entry {
            key,
            version: row.version.clone(),
            origin,
            dir: prefix,
        });
    }
    if b.openssldir == ca_dir(b.root) && done.iter().any(|e| e.key == "openssl") {
        place_ca(b.root, b.rows, b.fetcher, say)?;
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
        let src = extract(&archive, kind, &work, recipe.key).map_err(|e| {
            log.line(&e);
            InstallError::Failed
        })?;
        let cwd = if recipe.dir.is_empty() {
            src
        } else {
            src.join(recipe.dir)
        };
        vars.push(("destdir".to_string(), destdir.display().to_string()));
        vars.push(("stage".to_string(), stage.display().to_string()));
        let env = step_env(b.env, pkgconf);
        for step in recipe.steps {
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
        Err(InstallError::Interrupted) => Err(InstallError::Interrupted),
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

/// Puts the catalog's CA bundle at `<root>/.rpyenv/ca/cert.pem` (M8 design §6.5), unless
/// the file there already has the catalog's SHA-256. `Ok(true)` when it wrote the file.
pub fn place_ca(
    root: &Path,
    rows: &[Row],
    fetcher: &Fetcher,
    say: &mut dyn FnMut(&str),
) -> R<bool> {
    let row = rows
        .iter()
        .find(|r| r.key == "cacert")
        .ok_or_else(|| message("pyenv: the build catalog has no cacert row".into()))?;
    let dir = ca_dir(root);
    let target = dir.join("cert.pem");
    if sha256_file(&target).is_ok_and(|h| h == row.sha256) {
        return Ok(false);
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| message(format!("pyenv: cannot create {}: {e}", dir.display())))?;
    let req = FetchRequest {
        file_name: format!("cacert-{}.pem", row.version),
        url: row.url.clone(),
        check: Check::Sha256(row.sha256.clone()),
        dest_dir: dir.clone(),
    };
    let got = fetcher.fetch(&req, &mut std::io::sink(), say)?;
    std::fs::rename(&got, &target)
        .map_err(|e| message(format!("pyenv: cannot write {}: {e}", target.display())))?;
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
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &f.rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: PathBuf::from("/etc/ssl"),
            verbose: false,
        };
        ensure(&b, &["zlib"], &mut |s: &str| said.push(s.to_string()))
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
    fn the_entry_name_changes_with_every_input_of_the_fingerprint() {
        let r = catalog::recipe("openssl").unwrap();
        let row = Row {
            key: "openssl".into(),
            version: "3.6.5".into(),
            url: "u".into(),
            sha256: "a".repeat(64),
        };
        let base = fingerprint(r, &row, "cc", "x86_64-linux-gnu", "/etc/ssl", &[]);
        let name = entry_name("openssl", "3.6.5", &base);
        assert!(
            name.starts_with("openssl-3.6.5-") && name.len() == "openssl-3.6.5-".len() + 12,
            "{name}"
        );
        assert_eq!(name, entry_name("openssl", "3.6.5", &base), "stable");
        for other in [
            fingerprint(r, &row, "gcc-14", "x86_64-linux-gnu", "/etc/ssl", &[]),
            fingerprint(r, &row, "cc", "aarch64-linux-gnu", "/etc/ssl", &[]),
            fingerprint(r, &row, "cc", "x86_64-linux-gnu", "/etc/pki/tls", &[]),
            fingerprint(
                r,
                &row,
                "cc",
                "x86_64-linux-gnu",
                "/etc/ssl",
                &["dep-1".into()],
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
                &[],
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
            fingerprint(z, &zrow, "cc", "t", "/etc/ssl", &[]),
            fingerprint(z, &zrow, "cc", "t", "/etc/pki/tls", &[])
        );
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
            "",
        ] {
            assert!(!plain_path(Path::new(bad)), "{bad:?}");
        }
    }

    #[test]
    fn openssldir_prefers_pki_then_ssl_then_the_bundle() {
        let fedora = |p: &str| p == "/etc/pki/tls/cert.pem";
        assert_eq!(system_openssldir(&fedora, &|_| false), Some("/etc/pki/tls"));
        let debian = |d: &str| d == "/etc/ssl/certs";
        assert_eq!(system_openssldir(&|_| false, &debian), Some("/etc/ssl"));
        assert_eq!(system_openssldir(&|_| false, &|_| false), None);
    }

    /// Ruling P9: deleting the cache must not break HTTPS for versions that read the bundle.
    #[test]
    fn the_ca_bundle_lives_outside_the_cache() {
        let root = Path::new("/r");
        assert_eq!(ca_dir(root), Path::new("/r/.rpyenv/ca"));
        assert!(!ca_dir(root).starts_with(deps_dir(root)));
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
