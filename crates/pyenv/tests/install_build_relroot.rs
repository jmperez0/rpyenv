//! The one source-build test that changes the working directory. `set_current_dir` is
//! process-wide, so this test has its own binary: sibling tests that spawn `sh` would race
//! with it (a stray `getcwd() failed` line).
#![cfg(unix)]

mod common;

use common::buildharness::{build_at, opts};
use pyenv::install::txn::is_complete;
use std::path::PathBuf;

/// Restores the working directory on drop.
struct Cwd(PathBuf);
impl Drop for Cwd {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

#[test]
fn a_relative_root_builds_with_an_absolute_prefix() {
    let base = tempfile::tempdir().unwrap();
    let work = base.path().join("work ñ");
    std::fs::create_dir_all(&work).unwrap();
    let _cwd = Cwd(std::env::current_dir().unwrap());
    std::env::set_current_dir(&work).unwrap();
    let b = build_at(
        base,
        PathBuf::from("relroot"),
        "standard verify_py312 ensurepip",
        &[],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let v = work.join("relroot/versions/3.12.99");
    let p = v.display().to_string();
    assert!(is_complete(&v));
    let c = std::fs::read_to_string(v.join("lib/rpyenv-config.txt")).unwrap();
    assert!(
        c.starts_with(&format!(
            "args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no\n"
        )),
        "{c}"
    );
    assert_eq!(
        std::fs::read_to_string(v.join("bin/pip3.12")).unwrap(),
        format!("#!{p}/bin/python3.12\n")
    );
    assert_eq!(
        b.said.last(),
        Some(&format!("Installed Python-3.12.99 to {p}"))
    );
}
