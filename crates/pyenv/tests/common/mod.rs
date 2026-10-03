#![allow(dead_code)]
//! A temporary PYENV_ROOT and working directory, and a way to run the real `pyenv` binary in them.

pub mod fakebuild;
pub mod server;

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

    /// A command for `program` with this fixture's clean environment, run in `dir`.
    pub fn command(&self, program: &Path, dir: &Path, env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(dir)
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
        cmd
    }

    fn run(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
        let out = self
            .command(Path::new(env!("CARGO_BIN_EXE_pyenv")), dir, env)
            .args(args)
            .output()
            .unwrap();
        Run {
            stdout: decode(&out.stdout),
            stderr: decode(&out.stderr),
            code: out.status.code().unwrap(),
        }
    }
}

/// rpyenv's redirected output as text. On Windows it is in the console's output code page
/// (spec §11); this test process shares that console with the `pyenv` it ran. With no
/// console (`output_cp()` is 0), the `pyenv` child gets a new console with the OEM code
/// page, so that is what the bytes are in.
pub fn decode(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        let cp = match rpyenv_core::wincp::output_cp() {
            0 => rpyenv_core::wincp::oem_cp(),
            cp => cp,
        };
        rpyenv_core::wincp::decode(bytes, cp)
    }
    #[cfg(not(windows))]
    {
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

/// `pyenv-shim` next to the `pyenv` under test. `cargo test --workspace` builds it; when
/// running one test file, run `cargo build --workspace` first.
pub fn shim_exe() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_pyenv"))
        .with_file_name(format!("pyenv-shim{}", std::env::consts::EXE_SUFFIX));
    assert!(
        p.is_file(),
        "{} is missing: run `cargo build --workspace` first",
        p.display()
    );
    p
}

/// Expected text in the platform's line endings: rpyenv prints CRLF on Windows.
pub fn nl(s: &str) -> String {
    if cfg!(windows) {
        s.replace('\n', "\r\n")
    } else {
        s.to_string()
    }
}
