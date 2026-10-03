//! `pyenv install` end to end with a fake CPython over a local server (spec §12.5 tier 1),
//! against docs/parity/pyenv-m2-reference.md "pyenv install".
#![cfg(unix)]

mod common;

use common::fakebuild::tarball;
use common::server::{start, Reply, Server};
use common::Fixture;

/// A plugin definition `3.12.99` (so `pyenv install 3.12` resolves to it) serving the fake
/// tarball; returns the server so its hit counts can be checked.
fn plugin_def(f: &Fixture) -> Server {
    let body = tarball("3.12.99");
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("t");
    std::fs::write(&p, &body).unwrap();
    let sha = pyenv::install::checksum::sha256_file(&p).unwrap();
    let s = start(vec![("/Python-3.12.99.tar.gz", vec![Reply::Body(body)])]);
    f.file(
        &f.root.join("plugins/fake/share/python-build/3.12.99"),
        &format!(
            "install_package \"Python-3.12.99\" \"{}#{sha}\" standard verify_py312 ensurepip\n",
            s.url("/Python-3.12.99.tar.gz")
        ),
    );
    s
}

fn env<'a>(f: &'a Fixture, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
    let mut v: Vec<(&str, String)> = vec![
        (
            "PATH",
            format!("{}:{}", f.syspath.display(), std::env::var("PATH").unwrap()),
        ),
        ("TMPDIR", f.base.join("tmp").display().to_string()),
        ("PYTHON_BUILD_SKIP_MIRROR", "1".into()),
        ("RPYENV_SKIP_PREFLIGHT", "1".into()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

fn run(f: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> common::Run {
    let owned = env(f, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_env(args, &pairs)
}

fn staging_left(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect()
}

#[test]
fn a_prefix_resolves_to_a_plugin_definition_builds_and_rehashes() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    let r = run(&f, &["install", "3.12"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let p = f.root.join("versions/3.12.99");
    assert_eq!(
        r.stderr,
        format!(
            "Downloading Python-3.12.99.tar.gz...\n-> {}\nInstalling Python-3.12.99...\nInstalled Python-3.12.99 to {}\n",
            s.url("/Python-3.12.99.tar.gz"),
            p.display()
        )
    );
    assert_eq!(r.stdout, "");
    assert!(
        f.root.join("shims/python3.12").exists(),
        "rehashed after the install"
    );
    assert!(staging_left(&f).is_empty());
}

#[test]
fn an_alias_installs_under_its_name() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "3.12.99:mine"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f.root.join("versions/mine/bin/python3.12").is_file());
}

#[test]
fn an_existing_version_prompts_and_eof_stops_the_run() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    f.version("3.12.99/bin");
    let r = run(&f, &["install", "3.12.99", "3.12.99:other"], &[]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        (
            format!(
                "pyenv: {} already exists\n",
                f.root.join("versions/3.12.99").display()
            )
            .as_str(),
            1
        )
    );
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 0);
    assert!(
        !f.root.join("versions/other").exists(),
        "EOF ends the whole run, as upstream"
    );
}

// allowlist D-58
#[test]
fn skip_existing_is_silent_and_force_rebuilds() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    f.version("3.12.99/bin");
    f.file(&f.root.join("versions/3.12.99/bin/old"), "");
    let r = run(&f, &["install", "-sf", "3.12.99"], &[]);
    assert_eq!(
        (r.stdout.as_str(), r.stderr.as_str(), r.code),
        ("", "", 0),
        "-s wins over -f"
    );
    let r = run(&f, &["install", "-f", "3.12.99"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 1);
    assert!(
        !f.root.join("versions/3.12.99/bin/old").exists(),
        "replaced, not built over"
    );
}

/// `-f` over an installed version keeps its virtualenvs (`envs/`) and the packages pip put
/// in its site-packages (review I1).
// allowlist D-58
#[test]
fn force_keeps_the_versions_envs_and_site_packages() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    let r = run(&f, &["install", "3.12.99"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let p = f.root.join("versions/3.12.99");
    f.file(&p.join("lib/python3.12/site-packages/userpkg.py"), "user\n");
    f.file(&p.join("envs/myenv/pyvenv.cfg"), "home = x\n");
    let r = run(&f, &["install", "-f", "3.12.99"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 2, "rebuilt");
    assert_eq!(
        std::fs::read_to_string(p.join("lib/python3.12/site-packages/userpkg.py")).unwrap(),
        "user\n"
    );
    assert_eq!(
        std::fs::read_to_string(p.join("envs/myenv/pyvenv.cfg")).unwrap(),
        "home = x\n"
    );
    assert!(p.join("bin/python3.12").is_file());
    assert!(staging_left(&f).is_empty(), "{:?}", staging_left(&f));
}

// allowlist D-59
#[test]
fn an_unknown_version_prints_upstreams_hint_and_exits_2() {
    let f = Fixture::new();
    let r = run(&f, &["install", "9.9.9"], &[]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("python-build: definition not found: 9.9.9\n\nSee all available versions with `pyenv install --list'.\n\nIf the version you need is missing, try upgrading pyenv.\n", 2)
    );
    // A release whose pre-releases are defined but which is not (`3.15.0` beside
    // `3.15.0rc2`), derived from the definitions so a python-build sync keeps it valid.
    let bare = run(&f, &["install", "--list", "--bare"], &[]).stdout;
    let names: Vec<&str> = bare.lines().collect();
    let query = names
        .iter()
        .rev()
        .find_map(|n| {
            let base = &n[..n.find(|c: char| c.is_ascii_alphabetic())?];
            let parts: Vec<&str> = base.split('.').collect();
            let numeric = parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
            (numeric && !names.contains(&base)).then_some(base)
        })
        .expect("a pre-release whose release is not defined");
    let containing: String = names
        .iter()
        .filter(|n| n.contains(query))
        .map(|n| format!("  {n}\n"))
        .collect();
    let r = run(&f, &["install", query], &[]);
    assert_eq!(
        (r.stderr, r.code),
        (format!("python-build: definition not found: {query}\n\nThe following versions contain `{query}' in the name:\n{containing}\nSee all available versions with `pyenv install --list'.\n\nIf the version you need is missing, try upgrading pyenv.\n"), 2)
    );
}

// Upstream exits 1 here (the pre-existing directory leaks PREFIX_EXISTS); rpyenv always 2.
// allowlist D-59
#[test]
fn an_unknown_version_exits_2_even_when_its_directory_exists() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.root.join("versions/9.9.9")).unwrap();
    let r = run(&f, &["install", "9.9.9"], &[]);
    assert!(
        r.stderr
            .starts_with("python-build: definition not found: 9.9.9\n"),
        "{}",
        r.stderr
    );
    assert_eq!(r.code, 2);
    assert!(f.root.join("versions/9.9.9").is_dir());
}

#[test]
fn list_prints_the_definitions() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "--list"], &[]);
    assert!(
        r.stdout.starts_with("Available versions:\n  2.1.3\n"),
        "{}",
        r.stdout.get(..80).unwrap_or(&r.stdout)
    );
    assert!(r.stdout.contains("\n  3.12.14\n") && r.stdout.contains("\n  3.12.99\n"));
    let bare = run(&f, &["install", "-l", "--bare"], &[]);
    assert!(bare.stdout.starts_with("2.1.3\n"));
    assert_eq!(bare.stdout.lines().count() + 1, r.stdout.lines().count());
}

// allowlist D-66
#[test]
fn usage_errors_and_version() {
    let f = Fixture::new();
    let r = run(&f, &["install"], &[]);
    assert_eq!(r.code, 1);
    assert!(
        r.stderr
            .starts_with("Usage: pyenv install [-f] [-kvp] <version>[:<alias>]...\n"),
        "{}",
        r.stderr
    );
    let r = run(&f, &["install", "-x", "--help"], &[]);
    assert_eq!(r.code, 1, "options are handled in order");
    let r = run(&f, &["install", "--help", "-x"], &[]);
    assert_eq!(r.code, 0);
    // The version is UPSTREAM's third field (`pyenv <commit> <version>`), so a python-build
    // sync updates it (review I3).
    let upstream = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("python-build/UPSTREAM"),
    )
    .unwrap();
    let version = upstream.split_whitespace().nth(2).unwrap();
    let r = run(&f, &["install", "--version"], &[]);
    assert_eq!(
        r.stdout,
        format!(
            "python-build {version} (rpyenv {})\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn with_no_arguments_the_local_version_file_is_used() {
    let f = Fixture::new();
    plugin_def(&f);
    f.file(&f.work.join(".python-version"), "3.12.99\n");
    let r = run(&f, &["install"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f.root.join("versions/3.12.99/bin").is_dir());
    let global_only = Fixture::new();
    global_only.file(&global_only.root.join("version"), "3.12.99\n");
    assert_eq!(
        run(&global_only, &["install"], &[]).code,
        1,
        "the global file is not read"
    );
}

// allowlist D-62
#[test]
fn default_packages_run_in_the_new_version_and_a_failure_still_succeeds() {
    let f = Fixture::new();
    plugin_def(&f);
    f.file(&f.root.join("default-packages"), "requests\n");
    let log = f.base.join("pip.log");
    let log_s = log.display().to_string();
    let r = run(
        &f,
        &["install", "3.12.99:dp"],
        &[("FAKE_PIP_LOG", log_s.as_str())],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!(
            "-m pip install -r {}\n",
            f.root.join("default-packages").display()
        )
    );
    let r = run(&f, &["install", "3.12.99:dp2"], &[("FAKE_PIP_FAIL", "1")]);
    assert_eq!(r.code, 0);
    assert!(
        r.stderr.ends_with(&format!(
            "pyenv: error installing packages from  `{}'\n",
            f.root.join("default-packages").display()
        )),
        "{}",
        r.stderr
    );
}

#[test]
fn a_failed_build_exits_1_and_stops_at_the_first_failure() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(
        &f,
        &["install", "3.12.99", "3.12.99:second"],
        &[("FAKE_CONFIGURE_FAIL", "1")],
    );
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("BUILD FAILED"), "{}", r.stderr);
    assert!(!f.root.join("versions/3.12.99").exists() && !f.root.join("versions/second").exists());
    assert!(staging_left(&f).is_empty());
}

/// `X:` is `X`: upstream's `${VERSION_ALIAS:-$VERSION_NAME}` (review I1).
#[test]
fn an_empty_alias_installs_under_the_version_name() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "3.12.99:"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f.root.join("versions/3.12.99/bin/python3.12").is_file());
    assert!(!f.root.join(".locks/install-").exists());
    assert!(staging_left(&f).is_empty());
}

/// A name that isn't one directory under `versions/` is refused before anything is
/// locked, downloaded or built (review I1).
// allowlist D-69
#[test]
fn a_name_that_is_not_one_directory_is_refused() {
    let f = Fixture::new();
    let s = plugin_def(&f);
    for alias in ["..", ".", "a/b"] {
        let arg = format!("3.12.99:{alias}");
        let r = run(&f, &["install", &arg, "3.12.99:later"], &[]);
        assert_eq!(
            (r.stderr.as_str(), r.code),
            (
                format!("pyenv: invalid version name: {alias}\n").as_str(),
                1
            )
        );
    }
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 0);
    assert!(!f.root.join("versions/a").exists());
    assert!(!f.root.join("versions/later").exists(), "the run stops");
    assert!(!f.root.join(".locks").exists());
}

/// `-k` keeps the source tree and the tarball in `$PYENV_ROOT/sources/<name>`; a non-empty
/// PYENV_BUILD_ROOT does the same there, even without `-k`.
#[test]
fn keep_leaves_the_sources_in_the_build_root() {
    let f = Fixture::new();
    plugin_def(&f);
    let r = run(&f, &["install", "-k", "3.12.99"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let kept = f.root.join("sources/3.12.99");
    assert!(kept.join("Python-3.12.99/configure").is_file());
    assert!(kept.join("Python-3.12.99.tar.gz").is_file());
    let br = f.base.join("br");
    let br_s = br.display().to_string();
    let r = run(
        &f,
        &["install", "3.12.99:other"],
        &[("PYENV_BUILD_ROOT", br_s.as_str())],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(br.join("other/Python-3.12.99/configure").is_file());
    assert!(br.join("other/Python-3.12.99.tar.gz").is_file());
}

/// Ctrl+C while `continue with installation?` waits for a reply ends the run at once
/// with 130, as upstream does (review I2). stdin is a pipe that is never written to.
// allowlist D-71
#[test]
fn ctrl_c_at_the_existing_version_prompt_exits_130() {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let s = plugin_def(&f);
    f.version("3.12.99/bin");
    let owned = env(&f, &[]);
    let mut cmd = f.command(
        std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
        &f.work,
        &[],
    );
    for (k, v) in &owned {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["install", "3.12.99", "3.12.99:other"])
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _held_open = child.stdin.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            let _ = tx.send(line);
        }
    });
    let line = rx.recv_timeout(Duration::from_secs(20));
    assert_eq!(
        line.as_deref(),
        Ok(format!(
            "pyenv: {} already exists",
            f.root.join("versions/3.12.99").display()
        )
        .as_str())
    );
    std::process::Command::new("kill")
        .args(["-INT", &format!("-{}", child.id())])
        .status()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if start.elapsed() > Duration::from_secs(3) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert_eq!(
        status.and_then(|st| st.code()),
        Some(130),
        "exit within 3 s of SIGINT"
    );
    assert_eq!(s.hits("/Python-3.12.99.tar.gz"), 0);
    assert!(f.root.join("versions/3.12.99/bin").is_dir());
    assert!(!f.root.join("versions/other").exists());
}

/// The fake `configure` writes the Makefile as its last act, in
/// `$TMPDIR/python-build.<seed>/Python-3.12.99/`. Once it exists, the download, extraction
/// and configure are done and the build is in (or entering) `make`, which sleeps.
fn makefile_written(f: &Fixture) -> bool {
    std::fs::read_dir(f.base.join("tmp"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|e| e.path().join("Python-3.12.99/Makefile").is_file())
}

/// Review focus 3: Ctrl+C mid-build exits 130 and leaves nothing behind, or restores what
/// was there.
// allowlist D-59
#[test]
fn ctrl_c_rolls_back_and_exits_130() {
    use std::os::unix::process::CommandExt;
    for preexisting in [false, true] {
        let f = Fixture::new();
        plugin_def(&f);
        if preexisting {
            f.file(&f.root.join("versions/3.12.99/bin/old"), "");
        }
        let owned = env(&f, &[("FAKE_MAKE_SLEEP", "30")]);
        let mut cmd = f.command(
            std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
            &f.work,
            &[],
        );
        for (k, v) in &owned {
            cmd.env(k, v);
        }
        let mut args = vec!["install".to_string()];
        if preexisting {
            args.push("-f".into());
        }
        args.push("3.12.99".into());
        let mut child = cmd
            .args(&args)
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        while !makefile_written(&f) && start.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            makefile_written(&f),
            "the build never reached make (preexisting={preexisting})"
        );
        assert!(
            f.root.join(".locks/install-3.12.99").is_file(),
            "the install holds its lock"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::Command::new("kill")
            .args(["-INT", &format!("-{}", child.id())])
            .status()
            .unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(130), "preexisting={preexisting}");
        assert!(staging_left(&f).is_empty(), "{:?}", staging_left(&f));
        assert_eq!(
            f.root.join("versions/3.12.99/bin/old").is_file(),
            preexisting
        );
        if !preexisting {
            assert!(!f.root.join("versions/3.12.99").exists());
        }
    }
}
