//! Tier-1 source-build tests (spec §12.5) with a fake CPython and a local server.
#![cfg(unix)]

mod common;

use common::fakebuild::{tarball, README_PATCH};
use common::server::{start, Reply};
use pyenv::install::builder::{run, Job, Options};
use pyenv::install::defs::{self, Found};
use pyenv::install::fetch::Fetcher;
use pyenv::install::txn::{is_complete, Txn};
use pyenv::install::InstallError;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

struct Env(HashMap<String, String>);
impl Env {
    fn get(&self, k: &str) -> Option<String> {
        self.0.get(k).cloned()
    }
}

struct Built {
    result: Result<(), InstallError>,
    said: Vec<String>,
    root: PathBuf,
    tmp: PathBuf,
    _dirs: Vec<tempfile::TempDir>,
}

/// Writes definition `3.12.99` (and a built-in-style patch when `patch`) next to a fake
/// tarball served locally, then builds it into `<root>/versions/3.12.99`.
fn build(
    steps: &str,
    vars: &[(&str, &str)],
    opts: Options,
    patch: bool,
    preexisting: bool,
) -> Built {
    let body = tarball("3.12.99");
    let sha = {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t");
        std::fs::write(&p, &body).unwrap();
        pyenv::install::checksum::sha256_file(&p).unwrap()
    };
    let server = start(vec![("/Python-3.12.99.tar.gz", vec![Reply::Body(body)])]);
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("py env ñ").join("root");
    let defdir = base.path().join("defs");
    let tmp = base.path().join("tmp");
    std::fs::create_dir_all(root.join("versions")).unwrap();
    std::fs::create_dir_all(&defdir).unwrap();
    if preexisting {
        std::fs::create_dir_all(root.join("versions/3.12.99/bin")).unwrap();
        std::fs::write(root.join("versions/3.12.99/bin/old"), "").unwrap();
    }
    let text = format!(
        "install_package \"Python-3.12.99\" \"{}#{sha}\" {steps}\n",
        server.url("/Python-3.12.99.tar.gz")
    );
    let def = defdir.join("3.12.99");
    std::fs::write(&def, &text).unwrap();
    if patch {
        let pd = defdir.join("patches/3.12.99/Python-3.12.99");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(pd.join("0001-readme.patch"), README_PATCH).unwrap();
    }
    let mut map: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    map.insert("TMPDIR".into(), tmp.display().to_string());
    map.insert("PYTHON_BUILD_SKIP_MIRROR".into(), "1".into());
    map.entry("PATH".into())
        .or_insert_with(|| std::env::var("PATH").unwrap());
    let list: Vec<(OsString, OsString)> = map.iter().map(|(k, v)| (k.into(), v.into())).collect();
    let env = Env(map);
    let found = defs::find(&root, def.to_str().unwrap(), &|k| env.get(k)).unwrap();
    let definition = defs::parse(&found, &|k| env.get(k), &|_| None::<Found>).unwrap();
    let fetcher = Fetcher::from_env(&|k| env.get(k), None);
    let job = Job {
        found: &found,
        definition: &definition,
        prefix: root.join("versions/3.12.99"),
        opts,
        env: &list,
        fetcher: &fetcher,
    };
    let mut txn = Txn::begin(&root.join("versions"), "3.12.99").unwrap();
    let mut said = Vec::new();
    let result = run(&job, &mut txn, &mut |s: &str| said.push(s.to_string()));
    if result.is_ok() {
        txn.commit().unwrap();
    } else {
        drop(txn);
    }
    Built {
        result,
        said,
        root,
        tmp,
        _dirs: vec![base],
    }
}

fn opts() -> Options {
    Options {
        keep: false,
        verbose: false,
        debug: false,
        stdin_patch: None,
    }
}

fn config(b: &Built) -> String {
    std::fs::read_to_string(b.root.join("versions/3.12.99/lib/rpyenv-config.txt")).unwrap()
}

#[test]
fn a_standard_build_installs_with_python_builds_flags_and_messages() {
    let b = build(
        "standard verify_py312 copy_python_gdb ensurepip",
        &[],
        opts(),
        true,
        false,
    );
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let v = b.root.join("versions/3.12.99");
    assert!(is_complete(&v));
    let p = v.display().to_string();
    let c = config(&b);
    assert!(
        c.starts_with(&format!(
            "args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no\n"
        )),
        "{c}"
    );
    assert!(
        c.contains(&format!("LDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib\n")),
        "{c}"
    );
    assert!(
        c.contains(&format!("LIBS=-L{p}/lib -Wl,-rpath,{p}/lib\n")),
        "{c}"
    );
    assert!(c.contains(&format!("CPPFLAGS=-I{p}/include\n")), "{c}");
    assert_eq!(
        &b.said[..],
        &[
            "Downloading Python-3.12.99.tar.gz...".to_string(),
            b.said[1].clone(),
            "Installing Python-3.12.99...".to_string(),
            format!("Installed Python-3.12.99 to {p}")
        ]
    );
    // Version-suffix symlinks, pip from ensurepip at the final prefix, gdb helper.
    assert_eq!(
        std::fs::read_link(v.join("bin/python")).unwrap(),
        Path::new("python3.12")
    );
    assert_eq!(
        std::fs::read_link(v.join("bin/pip")).unwrap(),
        Path::new("pip3.12")
    );
    assert_eq!(
        std::fs::read_to_string(v.join("bin/pip3.12")).unwrap(),
        format!("#!{p}/bin/python3.12\n")
    );
    assert!(v.join("bin/python3.12-gdb.py").is_file());
    // The build tree is gone; the log stays, as upstream's does.
    let left: Vec<String> = std::fs::read_dir(&b.tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(
        left[0].starts_with("python-build.") && left[0].ends_with(".log"),
        "{left:?}"
    );
}

#[test]
fn built_in_patches_are_applied_and_their_output_stays_in_the_log() {
    let b = build(
        "standard",
        &[],
        Options {
            keep: true,
            ..opts()
        },
        true,
        false,
    );
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let src = std::fs::read_dir(&b.tmp)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(src.join("Python-3.12.99/README")).unwrap(),
        "patched\n"
    );
    assert!(!b.said.iter().any(|s| s.contains("patching file")));
    let tmp_files: Vec<String> = std::fs::read_dir(&b.tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !tmp_files.iter().any(|n| n.starts_with("python-patch.")),
        "{tmp_files:?}"
    );
}

#[test]
fn user_flags_combine_as_python_build_does() {
    let vars = [
        ("CONFIGURE_OPTS", "--global-opt"),
        ("PYTHON_CONFIGURE_OPTS", "--with-foo --enable-optimizations"),
        ("CFLAGS", "-gcflag"),
        ("PYTHON_CFLAGS", "-pycflag"),
        ("CPPFLAGS", "-gcpp"),
        ("PYTHON_CPPFLAGS", "-pycpp"),
        ("LDFLAGS", "-gld"),
        ("PYTHON_LDFLAGS", "-pyld"),
    ];
    let b = build("standard", &vars, opts(), false, false);
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let p = b.root.join("versions/3.12.99").display().to_string();
    let c = config(&b);
    assert!(c.starts_with(&format!("args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no --global-opt --with-foo --enable-optimizations\n")), "{c}");
    assert!(c.contains("CFLAGS=-gcflag -pycflag\n"), "{c}");
    assert!(
        c.contains(&format!("CPPFLAGS=-I{p}/include -gcpp -pycpp\n")),
        "{c}"
    );
    assert!(
        c.contains(&format!(
            "LDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib -gld -pyld\n"
        )),
        "{c}"
    );
}

#[test]
fn disable_shared_drops_shared_and_the_rpath() {
    let b = build(
        "standard",
        &[("PYTHON_CONFIGURE_OPTS", "--disable-shared")],
        opts(),
        false,
        false,
    );
    let p = b.root.join("versions/3.12.99").display().to_string();
    let c = config(&b);
    assert!(
        c.starts_with(&format!(
            "args: --prefix={p} --libdir={p}/lib --with-ensurepip=no --disable-shared\n"
        )),
        "{c}"
    );
    assert!(c.contains(&format!("LDFLAGS=-L{p}/lib\n")), "{c}");
}

#[test]
fn a_missing_optional_module_warns_and_a_missing_ssl_fails_and_rolls_back() {
    let b = build(
        "standard verify_py312",
        &[("FAKE_PY_MISSING", "bz2")],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Ok(()));
    let b = build(
        "standard verify_py312 ensurepip",
        &[("FAKE_PY_MISSING", "bz2 ssl")],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(!b.root.join("versions/3.12.99").exists());
    let names: Vec<String> = std::fs::read_dir(b.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.is_empty(), "{names:?}");
    let all = b.said.join("\n");
    assert!(all.contains("BUILD FAILED ("), "{all}");
    assert!(all.contains(" using rpyenv "), "{all}");
}

#[test]
fn a_failed_build_over_an_existing_version_restores_it() {
    let b = build(
        "standard",
        &[("FAKE_CONFIGURE_FAIL", "1")],
        opts(),
        false,
        true,
    );
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(
        b.root.join("versions/3.12.99/bin/old").is_file(),
        "previous version restored"
    );
    let all = b.said.join("\n");
    assert!(
        all.contains("Inspect or clean up the working tree at "),
        "{all}"
    );
    assert!(all.contains("Last 10 log lines:"), "{all}");
    assert!(all.contains("no acceptable C compiler found"), "{all}");
    assert!(
        all.contains("Are the build dependencies for Python correctly installed?"),
        "{all}"
    );
}

#[test]
fn a_failed_ensurepip_does_not_fall_back_to_get_pip() {
    let b = build(
        "standard ensurepip",
        &[("FAKE_PY_NO_PIP", "1")],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(
        b.said
            .iter()
            .any(|s| s == "error: failed to install pip via ensurepip"),
        "{:?}",
        b.said
    );
}

#[test]
fn debug_builds_add_pydebug_and_o0() {
    let b = build(
        "standard",
        &[],
        Options {
            debug: true,
            ..opts()
        },
        false,
        false,
    );
    let c = config(&b);
    assert!(c.contains(" --with-pydebug --enable-shared "), "{c}");
    assert!(
        c.contains("CFLAGS=-O0\n") || c.contains("CFLAGS=-O0 \n"),
        "{c}"
    );
}

#[test]
fn free_threading_adds_disable_gil() {
    let b = build(
        "standard",
        &[("PYTHON_BUILD_FREE_THREADING", "1")],
        opts(),
        false,
        false,
    );
    let c = config(&b);
    assert!(c.contains(" --disable-gil"), "{c}");
}
