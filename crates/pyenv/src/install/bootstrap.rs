//! A bootstrap CPython for xcb's code generator (M8 design §6.4, ruling C1): xcb-proto's and
//! libxcb's builds run Python, which a machine without root may not have. It is built minimal
//! from the CPython tarball being installed (`make python sharedmods`, about 40 s; measured on
//! 3.7.17, 3.8.20, 3.12.15 and 3.14.8, whose generated C is byte-identical), run from its build
//! tree, and removed when the store run ends.

use super::archive::{extract, Kind};
use super::builder::{apply_patch, get, logged, UNSET};
use super::fetch::{Check, FetchRequest, Fetcher};
use super::log::BuildLog;
use super::store::plain_path;
use super::InstallError;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

/// The CPython source a bootstrap is built from: the definition's tarball, its SHA-256 and
/// python-build's patches for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The package name (`Python-3.12.14`). The download is saved as `<name><ext>`, the name
    /// the main build fetches, so a download cache serves both (ruling C6).
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub patches: Vec<(String, Vec<u8>)>,
}

/// A built bootstrap. Dropping it removes its folder.
pub struct Python {
    work: PathBuf,
    pub exe: PathBuf,
}

impl Drop for Python {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.work);
    }
}

/// The two commands, run with `sh -c` in the source tree (`--disable-test-modules` is unknown
/// to 3.7, whose configure only warns; measured).
const STEPS: [&str; 2] = [
    "./configure --without-ensurepip --disable-test-modules",
    "make -j{jobs} python sharedmods",
];

/// Downloads, patches and builds `src` minimal in `$TMPDIR/rpyenv-bootstrap.<pid>`; the result
/// imports `xml.etree.ElementTree` (xcbgen parses XML; `sharedmods` builds `pyexpat`). A
/// failure keeps the log and names it; a failure or Ctrl+C leaves no folder.
pub fn build(
    src: &Source,
    env: &[(OsString, OsString)],
    fetcher: &Fetcher,
    jobs: &str,
    verbose: bool,
    say: &mut dyn FnMut(&str),
) -> Result<Python, InstallError> {
    say(&format!(
        "Building a bootstrap Python from {} (xcb's code generator needs one)...",
        src.name
    ));
    let tmp = get(env, "TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    // `{python}` is spliced unquoted into the recipes' shell lines (plan ruling P5).
    let tmp = std::path::absolute(&tmp).unwrap_or(tmp);
    if !plain_path(&tmp) {
        say(&format!(
            "pyenv: TMPDIR is {}, which has a space or a shell character; building xcb's code generator needs a plain path. Set TMPDIR to one.",
            tmp.display()
        ));
        return Err(InstallError::Failed);
    }
    // The build sees what python-build's builds see: no PYTHONHOME or PYTHONPATH.
    let env: Vec<(OsString, OsString)> = env
        .iter()
        .filter(|(k, _)| !UNSET.iter().any(|u| k == *u))
        .cloned()
        .collect();
    let env = &env[..];
    let pid = std::process::id();
    let work = tmp.join(format!("rpyenv-bootstrap.{pid}"));
    let log_path = tmp.join(format!("rpyenv-bootstrap.{pid}.log"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| {
        InstallError::Message(format!("pyenv: cannot create {}: {e}", work.display()))
    })?;
    // From here on, dropping `py` removes the folder, on every path out.
    let mut py = Python {
        work: work.clone(),
        exe: PathBuf::new(),
    };
    let log = BuildLog::open(&log_path, verbose).map_err(|e| {
        InstallError::Message(format!("pyenv: cannot write {}: {e}", log_path.display()))
    })?;
    let result = (|| -> Result<PathBuf, InstallError> {
        let kind = Kind::of_url(&src.url);
        let req = FetchRequest {
            file_name: format!("{}{}", src.name, kind.ext()),
            url: src.url.clone(),
            check: Check::Sha256(src.sha256.clone()),
            dest_dir: work.clone(),
        };
        let mut w: &BuildLog = &log;
        let archive = fetcher.fetch(&req, &mut w, say)?;
        let dir = extract(&archive, kind, &work, &src.name).map_err(|e| {
            log.line(&e);
            InstallError::Failed
        })?;
        let _ = std::fs::remove_file(&archive);
        for (name, text) in &src.patches {
            log.line(&format!("patch {name}"));
            apply_patch(&log, &dir, &work, text, env)?;
        }
        for step in STEPS {
            let line = step.replace("{jobs}", jobs);
            log.line(&format!("+ {line}"));
            let mut c = Command::new("sh");
            c.arg("-c")
                .arg(&line)
                .current_dir(&dir)
                .env_clear()
                .envs(env.iter().map(|(k, v)| (k, v)));
            logged(&log, &mut c)?;
        }
        let exe = dir.join("python");
        log.line("+ ./python -c 'import xml.etree.ElementTree'");
        let mut c = Command::new(&exe);
        c.args(["-c", "import xml.etree.ElementTree"])
            .current_dir(&dir)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)));
        logged(&log, &mut c)?;
        Ok(exe)
    })();
    match result {
        Ok(exe) => {
            drop(log);
            let _ = std::fs::remove_file(&log_path);
            py.exe = exe;
            Ok(py)
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
                "pyenv: building the bootstrap Python from {} failed; the log is {}",
                src.name,
                log_path.display()
            ));
            Err(InstallError::Failed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::Write;

    // `r##`: the text holds `"#`, which would end an `r#"…"#` string.
    const CONFIGURE: &str = r##"#!/bin/sh
[ -f patched.txt ] || { echo 'not patched'; exit 1; }
printf 'python:\n\tprintf "#!/bin/sh\\n[ -f sharedmods.txt ] || exit 1\\n[ -z \\"$$FAKE_NO_EXPAT\\" ] || exit 1\\n[ -z \\"$$PYTHONHOME\\" ]\\n" > python && chmod +x python\nsharedmods: python\n\ttouch sharedmods.txt\n' > Makefile
"##;

    const PATCH: &str = "diff --git a/patched.txt b/patched.txt\nnew file mode 100644\n--- /dev/null\n+++ b/patched.txt\n@@ -0,0 +1 @@\n+yes\n";

    struct Fake {
        tmp: tempfile::TempDir,
        src: Source,
        env: Vec<(OsString, OsString)>,
    }

    fn fake() -> Fake {
        let tmp = tempfile::tempdir().unwrap();
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut h = tar::Header::new_gnu();
        h.set_size(CONFIGURE.len() as u64);
        h.set_mode(0o755);
        b.append_data(&mut h, "Python-9.9.9/configure", CONFIGURE.as_bytes())
            .unwrap();
        let mut gz = b.into_inner().unwrap().finish().unwrap();
        gz.flush().unwrap();
        let tarball = tmp.path().join("Python-9.9.9.tar.gz");
        std::fs::write(&tarball, &gz).unwrap();
        let sha256 = crate::install::checksum::sha256_file(&tarball).unwrap();
        std::fs::create_dir_all(tmp.path().join("tmp")).unwrap();
        let mut env: Vec<(OsString, OsString)> =
            std::env::vars_os().filter(|(k, _)| k == "PATH").collect();
        env.push(("TMPDIR".into(), tmp.path().join("tmp").into()));
        Fake {
            src: Source {
                name: "Python-9.9.9".into(),
                url: format!("file://{}", tarball.display()),
                sha256,
                patches: vec![("0001-fake.patch".into(), PATCH.as_bytes().to_vec())],
            },
            env,
            tmp,
        }
    }

    fn leftovers(f: &Fake) -> Vec<String> {
        std::fs::read_dir(f.tmp.path().join("tmp"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn builds_from_the_patched_tarball_and_removes_its_folder_when_dropped() {
        let f = fake();
        let mut said = Vec::new();
        let py = build(
            &f.src,
            &f.env,
            &Fetcher::direct(),
            "2",
            false,
            &mut |s: &str| said.push(s.to_string()),
        )
        .unwrap();
        assert!(py.exe.is_file(), "{:?}", py.exe);
        assert!(
            said[0].starts_with("Building a bootstrap Python from Python-9.9.9"),
            "{said:?}"
        );
        assert_eq!(
            leftovers(&f).len(),
            1,
            "only the bootstrap folder, no log: {:?}",
            leftovers(&f)
        );
        drop(py);
        assert!(leftovers(&f).is_empty(), "{:?}", leftovers(&f));
    }

    #[test]
    fn a_tmpdir_with_a_space_or_a_shell_character_is_refused() {
        let mut f = fake();
        f.env.retain(|(k, _)| k != "TMPDIR");
        f.env.push(("TMPDIR".into(), "/tmp/a b".into()));
        let mut said = Vec::new();
        let r = build(
            &f.src,
            &f.env,
            &Fetcher::direct(),
            "2",
            false,
            &mut |s: &str| said.push(s.to_string()),
        );
        assert!(matches!(r, Err(InstallError::Failed)));
        let last = said.last().unwrap();
        assert!(
            last.contains("TMPDIR") && last.contains("/tmp/a b"),
            "{last}"
        );
    }

    #[test]
    fn a_relative_tmpdir_gives_an_absolute_python() {
        let mut f = fake();
        // The folder's path from the current directory, by way of the root.
        let up: PathBuf = std::env::current_dir()
            .unwrap()
            .components()
            .skip(1)
            .map(|_| "..")
            .collect();
        let target = f.tmp.path().join("tmp");
        let rel = up.join(target.strip_prefix("/").unwrap());
        f.env.retain(|(k, _)| k != "TMPDIR");
        f.env.push(("TMPDIR".into(), rel.into()));
        let py = build(
            &f.src,
            &f.env,
            &Fetcher::direct(),
            "2",
            false,
            &mut |_: &str| {},
        )
        .unwrap();
        assert!(py.exe.is_absolute(), "{:?}", py.exe);
    }

    #[test]
    fn the_build_does_not_see_pythonhome() {
        let mut f = fake();
        f.env.push(("PYTHONHOME".into(), "/user/pyhome".into()));
        let py = build(
            &f.src,
            &f.env,
            &Fetcher::direct(),
            "2",
            false,
            &mut |_: &str| {},
        );
        assert!(py.is_ok(), "the fake python fails when it sees PYTHONHOME");
    }

    /// A failed bootstrap names its log, keeps only the log, and removes its folder.
    #[test]
    fn a_failed_bootstrap_names_its_log_and_leaves_no_folder() {
        let mut f = fake();
        f.env.push(("FAKE_NO_EXPAT".into(), "1".into()));
        let mut said = Vec::new();
        let r = build(
            &f.src,
            &f.env,
            &Fetcher::direct(),
            "2",
            false,
            &mut |s: &str| said.push(s.to_string()),
        );
        assert!(matches!(r, Err(InstallError::Failed)));
        let last = said.last().unwrap();
        assert!(
            last.starts_with(
                "pyenv: building the bootstrap Python from Python-9.9.9 failed; the log is "
            ),
            "{last}"
        );
        let left = leftovers(&f);
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].ends_with(".log"), "only the log stays: {left:?}");
    }

    #[test]
    fn the_download_is_cached_under_the_name_the_main_build_fetches() {
        let f = fake();
        let cache = f.tmp.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let env = |k: &str| (k == "PYTHON_BUILD_SKIP_MIRROR").then(|| "1".to_string());
        let fetcher = Fetcher::from_env(&env, Some(cache.clone()));
        let py = build(&f.src, &f.env, &fetcher, "2", false, &mut |_: &str| {}).unwrap();
        drop(py);
        assert!(cache.join("Python-9.9.9.tar.gz").is_file());
    }
}
