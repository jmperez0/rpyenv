#![allow(dead_code)]
//! A temporary PYENV_ROOT and working directory, and a way to run the real `pyenv` binary in them.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Run {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

pub struct Fixture {
    _tmp: tempfile::TempDir,
    pub base: PathBuf,
    pub root: PathBuf,
    pub work: PathBuf,
    /// The only directory on PATH: `<base>/sys/bin`.
    pub syspath: PathBuf,
}

impl Fixture {
    pub fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        // Review focus 2: a space and a non-ASCII letter in every path.
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

    /// Creates `versions/<name>` (nested names like `3.12.9/envs/a` work too).
    pub fn version(&self, name: &str) -> &Fixture {
        std::fs::create_dir_all(self.root.join("versions").join(name)).unwrap();
        self
    }

    pub fn file(&self, path: &Path, content: &str) -> &Fixture {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, content).unwrap();
        self
    }

    /// A runnable file at `root/versions/<rel>` (`rel` is `/`-separated; mode 755 on Unix).
    pub fn exe(&self, rel: &str) -> PathBuf {
        let p = rel
            .split('/')
            .fold(self.root.join("versions"), |p, c| p.join(c));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    /// An executable `python` (`python.exe` on Windows) in `syspath`.
    pub fn python_on_path(&self) -> PathBuf {
        let p = self.syspath.join(if cfg!(windows) {
            "python.exe"
        } else {
            "python"
        });
        std::fs::write(&p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    pub fn pyenv(&self, args: &[&str]) -> Run {
        self.run(&self.work, args, &[])
    }

    pub fn pyenv_env(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        self.run(&self.work, args, env)
    }

    pub fn pyenv_in(&self, dir: &Path, args: &[&str]) -> Run {
        self.run(dir, args, &[])
    }

    fn run(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_pyenv"));
        cmd.args(args)
            .current_dir(dir)
            .env_clear()
            .env("PYENV_ROOT", &self.root)
            .env("HOME", &self.base)
            .env("USERPROFILE", &self.base)
            .env("PATH", &self.syspath)
            .env("PWD", dir);
        if let Some(v) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", v);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Run {
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr).unwrap(),
            code: out.status.code().unwrap(),
        }
    }
}

/// Expected text in the platform's line endings: rpyenv prints CRLF on Windows.
pub fn nl(s: &str) -> String {
    if cfg!(windows) {
        s.replace('\n', "\r\n")
    } else {
        s.to_string()
    }
}
