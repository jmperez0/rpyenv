#![allow(dead_code)]
//! A temporary PYENV_ROOT whose versions hold copies of `argv-echo`, with real shims made
//! by the real `pyenv rehash`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const EXE: &str = std::env::consts::EXE_SUFFIX;

/// A binary built next to `argv-echo`.
pub fn built(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_argv-echo")).with_file_name(format!("{name}{EXE}"));
    assert!(
        p.is_file(),
        "{} is missing: run `cargo build --workspace` first",
        p.display()
    );
    p
}

/// The line `argv-echo` prints for one value.
pub fn line(key: &str, value: impl AsRef<OsStr>) -> String {
    format!("{key}={:?}", value.as_ref())
}

pub struct Fixture {
    _tmp: tempfile::TempDir,
    pub base: PathBuf,
    pub root: PathBuf,
    pub work: PathBuf,
    /// On PATH after the shims: `<base>/sys/bin`.
    pub syspath: PathBuf,
}

impl Fixture {
    pub fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        // A space and a non-ASCII letter in every path.
        let base = tmp.path().join("py env ñ");
        let root = base.join("root");
        let work = base.join("work");
        let syspath = base.join("sys").join("bin");
        for d in [&root.join("versions"), &work, &syspath] {
            std::fs::create_dir_all(d).unwrap();
        }
        Fixture {
            _tmp: tmp,
            base,
            root,
            work,
            syspath,
        }
    }

    /// Copies `argv-echo` to `versions/<rel>` (`/`-separated, e.g. `3.12.10/bin/python`).
    pub fn install(&self, rel: &str) -> PathBuf {
        let p = rel
            .split('/')
            .fold(self.root.join("versions"), |p, c| p.join(c));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::copy(built("argv-echo"), &p).unwrap();
        p
    }

    pub fn shim(&self, name: &str) -> PathBuf {
        self.root.join("shims").join(format!("{name}{EXE}"))
    }

    /// A command with a clean environment: the root, the shims first on PATH, then `syspath`.
    pub fn command(&self, program: &Path, env: &[(&str, &OsStr)]) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(&self.work)
            .env_clear()
            .env("PYENV_ROOT", &self.root)
            .env("HOME", &self.base)
            .env("USERPROFILE", &self.base)
            .env(
                "PATH",
                std::env::join_paths([self.root.join("shims"), self.syspath.clone()]).unwrap(),
            )
            .env("PWD", &self.work);
        for k in ["SystemRoot", "PATHEXT"] {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    pub fn pyenv(&self, args: &[&str], env: &[(&str, &OsStr)]) -> Output {
        self.command(&built("pyenv"), env)
            .args(args)
            .output()
            .unwrap()
    }

    pub fn rehash(&self) {
        let out = self.pyenv(&["rehash"], &[]);
        assert!(
            out.status.success(),
            "rehash failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub fn shim_command(&self, name: &str, env: &[(&str, &OsStr)]) -> Command {
        self.command(&self.shim(name), env)
    }

    pub fn run_shim(
        &self,
        name: &str,
        args: &[std::ffi::OsString],
        env: &[(&str, &OsStr)],
    ) -> Output {
        self.shim_command(name, env).args(args).output().unwrap()
    }
}
