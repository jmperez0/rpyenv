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

/// How long ending the installer's tree may take before the install gives up waiting.
#[cfg(windows)]
const END_TREE_SECS: u64 = 15;

/// The installer's process tree (M7c Task 8 review I1). On Windows a job without breakaway
/// holds the stub and everything it starts (`_conda.exe` does the real work), so Ctrl+C can
/// end all of it. Assigning right after `spawn` leaves a short window in which the stub
/// could start a child outside the job; NSIS extracts its payload for seconds before it
/// starts `_conda.exe`, so that race is accepted.
pub struct Tree {
    #[cfg(windows)]
    job: Option<rpyenv_core::winproc::Job>,
}

impl Tree {
    /// A tree holding `child`. Where Windows refuses the job, only the child itself is ended.
    pub fn new(child: &Child) -> Tree {
        #[cfg(not(windows))]
        let _ = child;
        Tree {
            #[cfg(windows)]
            job: rpyenv_core::winproc::Job::new_tree().filter(|j| j.assign(child)),
        }
    }

    /// Ends the child and, on Windows, every process in its job, and waits until none is
    /// left, so nothing writes into the folder the caller deletes next.
    fn end(&self, child: &mut Child) -> std::io::Result<()> {
        let _ = child.kill();
        let _ = child.wait();
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
            let end = std::time::Instant::now() + std::time::Duration::from_secs(END_TREE_SECS);
            while job.active_processes().is_none_or(|n| n > 0) {
                if std::time::Instant::now() >= end {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("its processes did not end within {END_TREE_SECS} s"),
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        Ok(())
    }
}

/// Waits for `child`, polling `stop` every 200 ms; when it turns true the child and its tree
/// are ended, awaited, and `None` returned. A tree that outlives `END_TREE_SECS` is an error
/// of kind `TimedOut`.
pub fn wait_or_kill(
    child: &mut Child,
    tree: &Tree,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<Option<ExitStatus>> {
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(Some(s));
        }
        if stop() {
            tree.end(child)?;
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Variables that would point the installer, `conda install` or the import check at another
/// conda or Python (M7c Task 8 review I2, ruling R18).
fn scrubbed(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    up.starts_with("CONDA_")
        || up.starts_with("_CE_")
        || matches!(
            up.as_str(),
            "PYTHONHOME" | "PYTHONPATH" | "PYTHONSTARTUP" | "PYTHONUSERBASE" | "VIRTUAL_ENV"
        )
}

/// `cmd` without the user's conda or Python environment, then the row's own variables.
fn clean_env(cmd: &mut Command, row_env: &[(String, String)]) {
    for (k, _) in std::env::vars_os() {
        if scrubbed(&k.to_string_lossy()) {
            cmd.env_remove(&k);
        }
    }
    for (k, v) in row_env {
        cmd.env(k, v);
    }
}

/// The last `n` lines of the install log, for an error message.
fn tail(log: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// `m`, then where the step's output went and its last lines (review M1).
fn with_log(m: String, log: &Path) -> InstallError {
    let t = tail(log, 5);
    let mut m = format!("{m}; output is in {}", log.display());
    if !t.is_empty() {
        m.push_str(":\n");
        m.push_str(&t);
    }
    InstallError::Message(m)
}

fn append_log(log: &Path, text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = f.write_all(text.as_bytes());
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
        Some(c) => return Err(msg(format!("python-build: {name}: conda's installer refuses the character '{c}' in {}; set PYENV_ROOT, PYENV and PYENV_HOME to a folder without it.", target.display()))),
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
    clean_env(&mut cmd, &install.env);
    // The install log (spec §5, review M1): every step appends its output here.
    let log = job.root.join("install_cache").join(format!("{name}.log"));
    let _ = std::fs::create_dir_all(job.root.join("install_cache"));
    append_log(&log, &format!("== {name}: installer {}\n", all.join(" ")));
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        if let Ok(g) = f.try_clone() {
            cmd.stdout(f).stderr(g);
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| msg(format!("cannot run {}: {e}", fetched.files[0].display())))?;
    let tree = Tree::new(&child);
    let status = wait_or_kill(&mut child, &tree, &interrupted)
        .map_err(|e| {
            msg(format!(
                "{name}: {e}; {} may be incomplete",
                target.display()
            ))
        })?
        .ok_or(InstallError::Interrupted)?;
    if !status.success() {
        return Err(with_log(
            format!("{name}: the installer exited with {status}"),
            &log,
        ));
    }
    let run = |exe: &Path, args: &[&str]| -> Result<std::process::Output, InstallError> {
        let mut c = Command::new(exe);
        c.args(args)
            .env("PATH", path_env(&target))
            .stdin(Stdio::null());
        clean_env(&mut c, &install.env);
        append_log(
            &log,
            &format!("== {name}: {} {}\n", exe.display(), args.join(" ")),
        );
        let out = c
            .output()
            .map_err(|e| msg(format!("cannot run {}: {e}", exe.display())))?;
        append_log(&log, &String::from_utf8_lossy(&out.stdout));
        append_log(&log, &String::from_utf8_lossy(&out.stderr));
        Ok(out)
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
            return Err(with_log(
                format!("{name}: conda install pip failed ({})", out.status),
                &log,
            ));
        }
    }
    let out = run(&target.join("python.exe"), &["-c", "import sys, ssl"])?;
    if !out.status.success() {
        return Err(with_log(
            format!("{name}: python.exe can't import ssl with conda's PATH; the install is broken"),
            &log,
        ));
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
    fn the_conda_and_python_environment_is_scrubbed() {
        for n in [
            "CONDA_PREFIX",
            "conda_default_env",
            "_CE_M",
            "_ce_conda",
            "PYTHONHOME",
            "pythonpath",
            "PYTHONSTARTUP",
            "PYTHONUSERBASE",
            "VIRTUAL_ENV",
        ] {
            assert!(scrubbed(n), "{n}");
        }
        for n in ["PATH", "PYTHONUTF8", "CONDAX", "HOME"] {
            assert!(!scrubbed(n), "{n}");
        }
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
        let tree = Tree::new(&c);
        assert!(wait_or_kill(&mut c, &tree, &|| true).unwrap().is_none());
        assert!(t.elapsed() < std::time::Duration::from_secs(5));
    }
}
