//! Real Windows shells for the shell-integration tests: cmd, Windows PowerShell 5.1,
//! PowerShell 7 and Git Bash. Every shell gets the fixture's root and HOME, and PowerShell
//! runs with `-NoProfile`, so nothing reads or writes a real profile.
use super::Fixture;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A copy of rpyenv's `pyenv.exe` in the fixture's `syspath`, so a shell finds it by name.
pub fn install_pyenv(f: &Fixture) -> PathBuf {
    let dst = f.syspath.join("pyenv.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_pyenv"), &dst).unwrap();
    dst
}

pub fn system32() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot")).join("System32")
}

pub fn cmd_exe() -> PathBuf {
    system32().join("cmd.exe")
}

pub fn powershell() -> PathBuf {
    system32().join(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// `pwsh.exe` from this process's PATH, when PowerShell 7 is installed.
pub fn pwsh() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("pwsh.exe"))
        .find(|p| p.is_file())
}

/// Git for Windows' bash, when installed.
pub fn git_bash() -> Option<PathBuf> {
    let p = PathBuf::from(r"C:\Program Files\Git\bin\bash.exe");
    p.is_file().then_some(p)
}

/// `program` with the fixture's environment; `PATH` is `syspath`, System32, then `extra`.
pub fn host(f: &Fixture, program: &Path, env: &[(&str, &str)], extra: &[&Path]) -> Command {
    let mut dirs = vec![f.syspath.clone(), system32()];
    dirs.extend(extra.iter().map(|p| p.to_path_buf()));
    let mut cmd = f.command(program, &f.work, env);
    cmd.env("PATH", std::env::join_paths(dirs).unwrap());
    // PowerShell finds `pyenv.exe` by name only through PATHEXT; cmd has a default.
    for k in [
        "TEMP",
        "TMP",
        "LOCALAPPDATA",
        "APPDATA",
        "windir",
        "ComSpec",
        "PATHEXT",
    ] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    cmd
}

/// stdout, stderr and the exit code, lossily decoded: compare only ASCII text.
pub fn output(mut cmd: Command) -> (String, String, i32) {
    let o = cmd.output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}
