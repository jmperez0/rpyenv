//! Anaconda, Miniconda and Miniforge on Windows (M7c design §5): the publisher-checked
//! installer, run silently into `versions\<name>` (conda can't be moved after installing),
//! pip as upstream's miniconda step, and an import check with conda's PATH.

use super::conda::{self, LinuxRow, Verdict, WinTable};
use super::fetch::Fetcher;
use super::txn::Txn;
use super::winpkg::{fetch_package, Done, Package};
use super::{interrupted, InstallError};
use rpyenv_core::flavor::Flavor;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

/// Characters every conda installer refuses in `/D=` (M7c design §3, §5 step 2).
const REFUSED: &[char] = &[
    '^', '%', '!', '=', '&', '(', ')', ';', ',', '\'', '#', '$', '@', '~', '[', ']', '{', '}',
];

/// The first character of `path` `family`'s installer refuses: one of `REFUSED`, anything
/// outside Latin-1 (inferred, ruling R7), or, for Miniforge only, a space (§3).
pub fn refused_char(path: &Path, family: &str) -> Option<char> {
    let space = family.starts_with("miniforge");
    path.to_string_lossy()
        .chars()
        .find(|&c| (c as u32) > 0xFF || REFUSED.contains(&c) || (space && c == ' '))
}

pub fn installer_args(target: &Path) -> Vec<String> {
    [
        "/S",
        "/InstallationType=JustMe",
        "/AddToPath=0",
        "/RegisterPython=0",
        "/NoRegistry=1",
        "/NoShortcuts=1",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain(std::iter::once(format!("/D={}", target.display())))
    .collect()
}

/// An exact name, else a bare family's newest installable row (design §6.1).
pub fn resolve_name(rows: &[LinuxRow], wanted: &str) -> String {
    if rows.iter().any(|r| r.name == wanted) || conda::family(wanted) != wanted {
        return wanted.to_string();
    }
    conda::newest_installable(rows, wanted, conda::WIN_ARCH).unwrap_or_else(|| wanted.to_string())
}

/// Waits for `child`, polling `stop` every 200 ms; when it turns true the child (and, on
/// Windows, its process tree through the job) is ended and `None` returned.
pub(crate) fn wait_or_kill(
    child: &mut Child,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(Some(s));
        }
        if stop() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn path_env(prefix: &Path) -> std::ffi::OsString {
    let mut p = std::ffi::OsString::new();
    for d in rpyenv_core::venv::conda_win_dirs(prefix) {
        p.push(d.as_os_str());
        p.push(";");
    }
    if let Some(sys) = std::env::var_os("PATH") {
        p.push(sys);
    }
    p
}

pub struct CondaJob<'a> {
    pub root: &'a Path,
    pub name: &'a str,
    pub force: bool,
    pub fetcher: &'a Fetcher,
    pub table: &'a WinTable,
}

pub fn install_conda(job: &CondaJob, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    match install_one(job, say) {
        Err(_) if interrupted() => Err(InstallError::Interrupted),
        r => r,
    }
}

fn install_one(job: &CondaJob, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    let msg = InstallError::Message;
    let name = job.name;
    if !super::is_plain_name(name) {
        return Err(msg(format!("invalid version name: {name}")));
    }
    let rows = &job.table.rows;
    let verdict = conda::linux_verdict(rows, name, conda::WIN_ARCH)
        .ok_or_else(|| msg(format!("definition not found: {name}")))?;
    let install = match verdict {
        Verdict::Ok(i) => i.clone(),
        Verdict::OkLatest(i) => {
            conda::latest_source(name, i, conda::WIN_ARCH, job.fetcher).map_err(msg)?
        }
        Verdict::NoBinary(_) => {
            return Err(msg(format!(
                "rpyenv cannot install {name}: the publisher has no Windows x64 installer for it"
            )))
        }
        Verdict::Unsupported(why) => {
            let hint = conda::newest_installable(rows, name, conda::WIN_ARCH)
                .map(|n| {
                    format!(
                        "; the newest {} rpyenv can install here is {n}",
                        conda::family(name)
                    )
                })
                .unwrap_or_default();
            return Err(msg(format!(
                "rpyenv cannot install this conda definition ({why}){hint}"
            )));
        }
    };
    let versions = job.root.join("versions");
    let target = versions.join(name);
    // Spec §5 step 2, D10: refuse before downloading, naming the way out.
    match refused_char(&target, conda::family(name)) {
        Some(' ') => return Err(msg(format!("python-build: {name}: Miniforge's installer refuses a path with a space, and rpyenv would install it into {}. Install a miniconda3 version instead (its installer accepts spaces), or set PYENV_ROOT, PYENV and PYENV_HOME to a folder without spaces.", target.display()))),
        Some(c) => return Err(msg(format!("python-build: {name}: conda's installer refuses the character {c:?} in {}; set PYENV_ROOT, PYENV and PYENV_HOME to a folder without it.", target.display()))),
        None => {}
    }
    if super::txn::is_complete_for(&target, Flavor::PyenvWin) && !job.force {
        return Ok(Done::Skipped);
    }
    let mut txn = Txn::begin_for(&versions, name, Flavor::PyenvWin).map_err(msg)?;
    let (url, sha) = install
        .url
        .split_once('#')
        .ok_or_else(|| msg(format!("{name}: no SHA-256")))?;
    let file = format!("{}.exe", install.package);
    // Spec §4.4 (D8): a hash failure gets one more line. Match the mismatch error text
    // `fetch` produces (read fetch.rs's Check::Sha256 failure message) rather than every error.
    let fetched = fetch_package(
        &Package::Zip { file, url: url.to_string(), sha256: sha.to_string() },
        name, &job.root.join("install_cache"), job.fetcher,
        &mut |from, to| {
            say(&format!(":: [Downloading] ::  {name} ..."));
            say(&format!(":: [Downloading] ::  From {from}"));
            say(&format!(":: [Downloading] ::  To   {}", to.display()));
        },
    )
    .map_err(|e| match e {
        InstallError::Message(m) if is_hash_mismatch(&m) => msg(format!("{m}\nif the publisher replaced this installer, a later rpyenv release will carry the reviewed hash")),
        e => e,
    })?;
    if install
        .env
        .iter()
        .any(|(k, _)| k == "CONDA_PLUGINS_AUTO_ACCEPT_TOS")
    {
        say(conda::TERMS_NOTICE);
    }
    say(&format!(":: [Installing] ::  {name} ..."));
    txn.place_empty()
        .map_err(|e| msg(format!("cannot prepare {}: {e}", target.display())))?;
    let mut cmd = Command::new(&fetched.files[0]);
    // NSIS reads everything after ` /D=` to the end of the command line, so the tail goes in
    // raw: `Command::args` would quote a path with a space and the quote would become part
    // of it (M7c design §3, ruling R16).
    let all = installer_args(&target);
    let (d_arg, flags) = all.split_last().expect("installer_args is never empty");
    cmd.args(flags).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(d_arg);
    }
    #[cfg(not(windows))]
    cmd.arg(d_arg);
    for (k, v) in &install.env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| msg(format!("cannot run {}: {e}", fetched.files[0].display())))?;
    #[cfg(windows)]
    let _job = rpyenv_core::winproc::Job::new().filter(|j| j.assign(&child));
    let status = wait_or_kill(&mut child, &interrupted)
        .map_err(|e| msg(e.to_string()))?
        .ok_or(InstallError::Interrupted)?;
    if !status.success() {
        return Err(msg(format!("{name}: the installer exited with {status}")));
    }
    let run = |exe: &Path, args: &[&str]| -> Result<std::process::Output, InstallError> {
        let mut c = Command::new(exe);
        c.args(args)
            .env("PATH", path_env(&target))
            .stdin(Stdio::null());
        for (k, v) in &install.env {
            c.env(k, v);
        }
        c.output()
            .map_err(|e| msg(format!("cannot run {}: {e}", exe.display())))
    };
    if install.step == "miniconda" {
        let conda_exe = target.join("Scripts").join("conda.exe");
        let out = run(&conda_exe, &["--version"])?;
        let v = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .nth(1)
            .map(str::to_string)
            .ok_or_else(|| msg(format!("{} --version gave no version", conda_exe.display())))?;
        let out = run(
            &conda_exe,
            &["install", "--yes", "pip", &format!("conda={v}")],
        )?;
        if !out.status.success() {
            return Err(msg(format!(
                "{name}: conda install pip failed ({})",
                out.status
            )));
        }
    }
    let out = run(&target.join("python.exe"), &["-c", "import sys, ssl"])?;
    if !out.status.success() {
        return Err(msg(format!(
            "{name}: python.exe can't import ssl with conda's PATH; the install is broken"
        )));
    }
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    txn.commit()
        .map_err(|e| msg(format!("cannot finish installing {name}: {e}")))?;
    say(&format!(":: [Info] :: completed! {name}"));
    Ok(Done::Installed)
}

/// `fetch_error`'s line for a failed SHA-256 check: `cannot download <file>: checksum mismatch:
/// <file> (file is corrupt)` (fetch.rs logs "checksum mismatch: … (file is corrupt)").
fn is_hash_mismatch(m: &str) -> bool {
    m.starts_with("cannot download ") && m.contains(": checksum mismatch: ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_args_put_d_last() {
        let a = installer_args(Path::new("C:\\r\\versions\\m"));
        assert_eq!(a.last().unwrap(), "/D=C:\\r\\versions\\m");
        assert_eq!(a[0], "/S");
    }
    #[test]
    fn refused_char_follows_each_installers_rules() {
        assert_eq!(
            refused_char(Path::new("C:\\Users\\José\\x"), "miniforge3"),
            None
        );
        assert_eq!(
            refused_char(Path::new("C:\\Users\\Иван\\x"), "miniconda3"),
            Some('И')
        );
        assert_eq!(
            refused_char(Path::new("C:\\Users\\Jo Smith\\x"), "miniforge3"),
            Some(' ')
        );
        assert_eq!(
            refused_char(Path::new("C:\\Users\\Jo Smith\\x"), "miniconda3"),
            None
        );
        assert_eq!(
            refused_char(Path::new("C:\\Users\\a~b\\x"), "anaconda3"),
            Some('~')
        );
        assert_eq!(
            refused_char(Path::new("C:\\Users\\jm\\x"), "miniforge3"),
            None
        );
    }
    #[test]
    fn wait_or_kill_ends_a_child_when_asked() {
        let mut c = Command::new(if cfg!(windows) { "ping" } else { "sleep" })
            .args(if cfg!(windows) {
                &["-n", "30", "127.0.0.1"][..]
            } else {
                &["30"][..]
            })
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let t = std::time::Instant::now();
        assert!(wait_or_kill(&mut c, &|| true).unwrap().is_none());
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
    }
}
