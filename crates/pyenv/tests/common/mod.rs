#![allow(dead_code)]
//! A temporary PYENV_ROOT and working directory, and a way to run the real `pyenv` binary in them.

#[cfg(unix)]
pub mod buildharness;
pub mod fakebuild;
pub mod server;
pub mod winfake;
#[cfg(windows)]
pub mod winshell;

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
    /// The `HKCU` subkey standing in for the user and machine environments (plan M6a, R1).
    pub test_key: String,
}

impl Fixture {
    pub fn new() -> Fixture {
        // The binaries honor the RPYENV_TEST_* overrides only in debug builds (plan M6a, R1):
        // a release binary would edit the real user PATH and PowerShell profiles.
        if !cfg!(debug_assertions) {
            panic!(
                "run the CLI tests without --release: a release pyenv ignores the test overrides"
            );
        }
        let tmp = tempfile::tempdir().unwrap();
        // Review focus 2: a space and a non-ASCII letter in every path.
        let base = tmp.path().join("py env ñ");
        let root = base.join("root");
        let work = base.join("work");
        let syspath = base.join("sys").join("bin");
        for d in [&root.join("versions"), &work, &syspath] {
            std::fs::create_dir_all(d).unwrap();
        }
        let test_key = format!(
            "Software\\rpyenv-test\\{}",
            tmp.path().file_name().unwrap().to_string_lossy()
        );
        Fixture {
            _tmp: tmp,
            base,
            root,
            work,
            syspath,
            test_key,
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
            .env("PWD", dir)
            // Plan M6a, R1: setup, migrate and init --install never reach the real registry,
            // profiles or Program Files from a test.
            .env("RPYENV_TEST_ENV_KEY", &self.test_key)
            .env("RPYENV_TEST_DOCUMENTS", self.base.join("Documents"))
            .env("RPYENV_TEST_PROGRAM_FILES", self.base.join("Program Files"));
        if let Some(v) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", v);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd
    }

    /// Retries while a script the test just wrote can't be started because another test's
    /// fork still holds it open (ETXTBSY: `pyenv: <path>: Text file busy`, exit 126; seen
    /// on ubuntu-latest CI, runs 37242317356 and 37242607167). Nothing else prints that.
    fn run(&self, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
        for _ in 0..100 {
            let out = self
                .command(Path::new(env!("CARGO_BIN_EXE_pyenv")), dir, env)
                .args(args)
                .output()
                .unwrap();
            let r = Run {
                stdout: decode(&out.stdout),
                stderr: decode(&out.stderr),
                code: out.status.code().unwrap(),
            };
            if !(r.code == 126 && r.stderr.contains("Text file busy")) {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("a script stayed busy (ETXTBSY) for 2 s");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("reg")
                .args(["delete", &format!("HKCU\\{}", self.test_key), "/f"])
                .output();
            delete_parent_key_if_empty();
        }
    }
}

/// `HKCU\Software\rpyenv-test`, once no test is using it: RegDeleteKeyW refuses a key that
/// has subkeys, so a parallel test's key keeps it (final review M7).
#[cfg(windows)]
fn delete_parent_key_if_empty() {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyW, HKEY_CURRENT_USER};
    let name: Vec<u16> = "Software\\rpyenv-test"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: a NUL-terminated name; the call only deletes a key without subkeys.
    unsafe {
        RegDeleteKeyW(HKEY_CURRENT_USER, name.as_ptr());
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

/// Ctrl+C at a terminal: SIGINT to every process in the group `pgid` (a child spawned with
/// `process_group(0)`, so its PID). Sent with kill(2), not `kill -INT -<pgid>`: procps-ng
/// 4.0.4 (Ubuntu 24.04) parses that as `kill(-1, SIGINT)`, which signals every process the
/// user owns (traced with strace in an ubuntu:24.04 container).
#[cfg(unix)]
pub fn sigint_group(pgid: u32) {
    let pgid = i32::try_from(pgid).unwrap();
    // pgid 0 or 1 would turn this into "my own group" or "every process".
    assert!(pgid > 1, "refusing to signal process group {pgid}");
    // SAFETY: a plain syscall with a checked, positive group id.
    let rc = unsafe { libc::kill(-pgid, libc::SIGINT) };
    assert_eq!(
        rc,
        0,
        "kill(-{pgid}, SIGINT): {}",
        std::io::Error::last_os_error()
    );
}
