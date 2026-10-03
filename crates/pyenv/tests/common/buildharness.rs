//! Shared by the source-build tests: a fake CPython tarball on a local server, built into a
//! temporary `PYENV_ROOT` under a path with a space and a non-ASCII letter.
#![allow(dead_code)]

use super::fakebuild::{tarball, README_PATCH};
use super::server::{start, Reply};
use pyenv::install::builder::{run, Job, Options};
use pyenv::install::defs::{self, Found};
use pyenv::install::fetch::Fetcher;
use pyenv::install::txn::Txn;
use pyenv::install::InstallError;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

pub struct Env(pub HashMap<String, String>);
impl Env {
    pub fn get(&self, k: &str) -> Option<String> {
        self.0.get(k).cloned()
    }
}

pub struct Built {
    pub result: Result<(), InstallError>,
    pub said: Vec<String>,
    pub root: PathBuf,
    pub tmp: PathBuf,
    pub _dirs: Vec<tempfile::TempDir>,
}

/// Writes definition `3.12.99` (and a built-in-style patch when `patch`) next to a fake
/// tarball served locally, then builds it into `<root>/versions/3.12.99`.
pub fn build(
    steps: &str,
    vars: &[(&str, &str)],
    opts: Options,
    patch: bool,
    preexisting: bool,
) -> Built {
    let base = tempfile::tempdir().unwrap();
    let root = base.path().join("py env ñ").join("root");
    build_at(base, root, steps, vars, opts, patch, preexisting)
}

/// `build` into `root`, which may be relative to the working directory; the definition and
/// `TMPDIR` go under `base`.
pub fn build_at(
    base: tempfile::TempDir,
    root: PathBuf,
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

pub fn opts() -> Options {
    Options {
        keep: false,
        verbose: false,
        debug: false,
        stdin_patch: None,
    }
}
