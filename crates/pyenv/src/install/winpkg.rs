//! pyenv-win flavor installs (spec §9.1, plan M2b Decisions 1 and 6–8): which python.org
//! package a version code means, its verified download into `install_cache`, extraction into
//! the transaction's staging folder, and the steps that need the final location.

use super::checksum::sha256_file;
use super::fetch::{Check, FetchRequest, Fetcher};
use super::txn::{is_complete_for, Txn};
use super::wincatalog::{Arch, Code};
use super::winsource::{index_zips, listing_names};
use super::{child_of, interrupted, msi, openpgp, zipx, InstallError};
use rpyenv_core::flavor::Flavor;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Component MSIs a plain install leaves out (Decision 1): debug builds, PATH and launcher
/// changes, pip (ensurepip installs it), and the free-threaded build (only zips install it).
pub fn skip_component(name: &str) -> bool {
    let stem = name.strip_suffix(".msi").unwrap_or(name);
    stem.ends_with("_d")
        || stem.ends_with("_pdb")
        || matches!(stem, "appendpath" | "launcher" | "path" | "pip")
        || stem.starts_with("freethreaded")
}

/// A python.org file and, when the listing names one, its signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
    pub asc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Package {
    Zip {
        file: String,
        url: String,
        sha256: String,
    },
    /// The MSIs of one `<arch><pre>/` folder.
    Components {
        folder_url: String,
        msis: Vec<Remote>,
    },
    SingleMsi(Remote),
}

fn no_installer(code: &Code) -> String {
    format!("python.org has no Windows installer for {}", code.text)
}

/// A folder listing; a 404 means python.org has no such package.
fn listing(f: &Fetcher, url: &str, code: &Code) -> Result<Vec<String>, String> {
    match f.get_text(url) {
        Ok(t) => Ok(listing_names(&t)),
        Err(e) if e.status == Some(404) => Err(no_installer(code)),
        Err(e) => Err(format!("cannot read {url}: {}", e.message)),
    }
}

fn remote(folder_url: &str, name: &str, names: &[String]) -> Remote {
    let asc = format!("{name}.asc");
    Remote {
        name: name.to_string(),
        url: format!("{folder_url}{name}"),
        asc: names.contains(&asc).then(|| format!("{folder_url}{asc}")),
    }
}

/// What `code` installs from (Decision 1). Index pages and listings come from `base`.
pub fn resolve(code: &Code, base: &str, f: &Fetcher) -> Result<Package, String> {
    if code.nums >= [3, 11, 0] {
        let (zips, _) =
            index_zips(f, base).map_err(|(url, e)| format!("cannot read {url}: {}", e.message))?;
        if let Some(z) = zips
            .iter()
            .find(|z| z.version == code.version && z.ft == code.ft && z.arch == code.arch)
        {
            return Ok(Package::Zip {
                file: z.file.clone(),
                url: z.url.clone(),
                sha256: z.sha256.clone(),
            });
        }
    }
    if code.ft {
        return Err(format!(
            "python.org has no free-threaded package for {}",
            code.text
        ));
    }
    if code.nums >= [3, 5, 0] {
        let folder_url = format!(
            "{base}/{}/{}{}/",
            code.numeric,
            code.arch.word(),
            code.pre_tag()
        );
        let names = listing(f, &folder_url, code)?;
        let msis: Vec<Remote> = names
            .iter()
            .filter(|n| n.ends_with(".msi") && !skip_component(n))
            .map(|n| remote(&folder_url, n, &names))
            .collect();
        if msis.is_empty() {
            return Err(no_installer(code));
        }
        return Ok(Package::Components { folder_url, msis });
    }
    if code.arch == Arch::Arm64 {
        return Err(no_installer(code));
    }
    let folder_url = format!("{base}/{}/", code.numeric);
    let name = format!(
        "python-{}{}.msi",
        code.version,
        if code.arch == Arch::Amd64 {
            ".amd64"
        } else {
            ""
        }
    );
    let names = listing(f, &folder_url, code)?;
    if !names.contains(&name) {
        return Err(no_installer(code));
    }
    Ok(Package::SingleMsi(remote(&folder_url, &name, &names)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Msi,
}

/// The verified local files of a package, in install order.
#[derive(Debug)]
pub struct Fetched {
    pub files: Vec<PathBuf>,
    /// Files python.org publishes no signature for (Decision 6).
    pub unsigned: Vec<String>,
    pub kind: Kind,
}

/// The reason a failed fetch gives: the first line of its log, or a generic one.
fn fetch_error(file: &str, log: &[u8]) -> InstallError {
    let text = String::from_utf8_lossy(log);
    let detail = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("download failed");
    InstallError::Message(format!("cannot download {file}: {detail}"))
}

fn download(
    f: &Fetcher,
    url: &str,
    file: &str,
    dir: &Path,
    check: Check,
) -> Result<PathBuf, InstallError> {
    let req = FetchRequest {
        file_name: file.to_string(),
        url: url.to_string(),
        check,
        dest_dir: dir.to_path_buf(),
    };
    let mut log = Vec::new();
    match f.fetch(&req, &mut log, &mut |_| {}) {
        Ok(p) => Ok(p),
        Err(InstallError::Failed) => Err(fetch_error(file, &log)),
        Err(e) => Err(e),
    }
}

/// Puts every file of `pkg` in `cache_root` verified (Decisions 6 and 8): a cached file is
/// used only if it verifies against the reference fetched now; otherwise it is downloaded. The
/// `.asc` is always fetched fresh, so a cached MSI can't be swapped for another signed one.
/// `announce(from, to)` is called once, before the first payload download.
pub fn fetch_package(
    pkg: &Package,
    code_text: &str,
    cache_root: &Path,
    f: &Fetcher,
    announce: &mut dyn FnMut(&str, &Path),
) -> Result<Fetched, InstallError> {
    std::fs::create_dir_all(cache_root)
        .map_err(|e| InstallError::Message(format!("{}: {e}", cache_root.display())))?;
    match pkg {
        Package::Zip { file, url, sha256 } => {
            let path = child_of(cache_root, file)
                .ok_or_else(|| InstallError::Message(format!("invalid file name: {file}")))?;
            let cached = sha256_file(&path).ok().as_deref() == Some(sha256.as_str());
            if !cached {
                let _ = std::fs::remove_file(&path);
                announce(url, &path);
                download(f, url, file, cache_root, Check::Sha256(sha256.clone()))?;
            }
            Ok(Fetched {
                files: vec![path],
                unsigned: Vec::new(),
                kind: Kind::Zip,
            })
        }
        Package::Components { folder_url, msis } => {
            let dir = child_of(cache_root, code_text).ok_or_else(|| {
                InstallError::Message(format!("invalid version name: {code_text}"))
            })?;
            fetch_msis(msis, false, folder_url, &dir, f, announce)
        }
        Package::SingleMsi(r) => {
            let dir = child_of(cache_root, code_text).ok_or_else(|| {
                InstallError::Message(format!("invalid version name: {code_text}"))
            })?;
            fetch_msis(std::slice::from_ref(r), true, &r.url, &dir, f, announce)
        }
    }
}

/// Whether a signature by `fingerprint` (a key `verify_python_org` already accepted) may vouch
/// for this kind of MSI (ruling R4). The Löwis and Baxter keys (DSA-1024) signed only the
/// single MSIs of 2.4–3.4; every component MSI python.org publishes, 582 of them, is signed by
/// Steve Dower, so a component signed by another key is refused.
fn signer_allowed(single_msi: bool, fingerprint: &str) -> bool {
    if single_msi {
        openpgp::PINNED.contains(&fingerprint)
    } else {
        fingerprint == openpgp::PINNED[0]
    }
}

/// `verify_python_org`, plus ruling R4's signer rule for the package kind.
fn verify_msi(path: &Path, asc: &[u8], single_msi: bool) -> Result<(), String> {
    let signer = openpgp::verify_python_org(path, asc)?;
    if signer_allowed(single_msi, &signer) {
        Ok(())
    } else {
        Err(format!(
            "signed by {signer}, which does not sign python.org's component MSIs"
        ))
    }
}

fn fetch_msis(
    msis: &[Remote],
    single_msi: bool,
    from: &str,
    dir: &Path,
    f: &Fetcher,
    announce: &mut dyn FnMut(&str, &Path),
) -> Result<Fetched, InstallError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| InstallError::Message(format!("{}: {e}", dir.display())))?;
    let mut announced = false;
    let mut files = Vec::new();
    let mut unsigned = Vec::new();
    for r in msis {
        let path = child_of(dir, &r.name)
            .ok_or_else(|| InstallError::Message(format!("invalid file name: {}", r.name)))?;
        let mut fetch_msi = |announced: &mut bool| -> Result<(), InstallError> {
            if !*announced {
                announce(from, dir);
                *announced = true;
            }
            let _ = std::fs::remove_file(&path);
            download(f, &r.url, &r.name, dir, Check::Caller).map(|_| ())
        };
        match &r.asc {
            Some(asc_url) => {
                let asc_name = format!("{}.asc", r.name);
                let _ = std::fs::remove_file(dir.join(&asc_name));
                let asc_path = download(f, asc_url, &asc_name, dir, Check::Caller)?;
                let asc = std::fs::read(&asc_path)
                    .map_err(|e| InstallError::Message(format!("{}: {e}", asc_path.display())))?;
                if !(path.is_file() && verify_msi(&path, &asc, single_msi).is_ok()) {
                    fetch_msi(&mut announced)?;
                    if let Err(e) = verify_msi(&path, &asc, single_msi) {
                        let _ = std::fs::remove_file(&path);
                        return Err(InstallError::Message(format!(
                            "signature check failed for {}: {e}",
                            r.name
                        )));
                    }
                }
            }
            None => {
                // Never trusted from the cache: HTTPS is its only check (Decision 6).
                fetch_msi(&mut announced)?;
                unsigned.push(r.name.clone());
            }
        }
        if interrupted() {
            return Err(InstallError::Interrupted);
        }
        files.push(path);
    }
    Ok(Fetched {
        files,
        unsigned,
        kind: Kind::Msi,
    })
}

/// Extracts the package into `stage`.
pub fn unpack(fetched: &Fetched, stage: &Path) -> Result<(), String> {
    for file in &fetched.files {
        if interrupted() {
            return Err("interrupted".into());
        }
        match fetched.kind {
            Kind::Zip => zipx::extract_zip(file, stage).map(|_| ())?,
            Kind::Msi => msi::extract_msi(file, stage).map(|_| ())?,
        }
    }
    Ok(())
}

fn copy_if(src: &Path, dests: &[PathBuf]) -> Result<(), String> {
    if !src.is_file() {
        return Ok(());
    }
    for d in dests {
        std::fs::copy(src, d)
            .map_err(|e| format!("cannot copy {} to {}: {e}", src.display(), d.display()))?;
    }
    Ok(())
}

/// pip's configuration file setting for "none" (`os.devnull`).
const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// The command `python` runs. `parent` names the variables this process has: every `PIP_*` one
/// is removed (case-insensitively on Windows, whose variable names ignore case) and pip's
/// configuration file is the null device, as ensurepip does for its own pip (fix round 1, I2:
/// `PIP_REQUIRE_VIRTUALENV` made pip exit 3, and `PIP_TARGET`, `PIP_PREFIX` or `PIP_USER` could
/// write outside the version).
fn python_command(prefix: &Path, args: &[&str], parent: &[OsString]) -> Command {
    let mut cmd = Command::new(prefix.join("python.exe"));
    cmd.args(["-E", "-s"])
        .args(args)
        .current_dir(prefix)
        .env("PYTHONNOUSERSITE", "1")
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH");
    for name in parent {
        let n = name.to_string_lossy();
        let pip = if cfg!(windows) {
            n.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("PIP_"))
        } else {
            n.starts_with("PIP_")
        };
        if pip {
            cmd.env_remove(name);
        }
    }
    cmd.env("PIP_CONFIG_FILE", NULL_DEVICE);
    cmd
}

/// How a Python child failed, for `vc_runtime_hint`.
enum ChildFailure<'a> {
    /// The process couldn't be created (a side-by-side error is one way: 14001).
    Start,
    Exit {
        code: Option<i32>,
        stderr: &'a str,
    },
}

/// The hint appended when a Python below 3.5 fails as one does without its Visual C++ runtime
/// (final review M5): it can't start, or exits with STATUS_DLL_NOT_FOUND (0xC0000135) or
/// ERROR_SXS_CANT_GEN_ACTCTX (14001), or says "side-by-side" or "MSVCR". 2.6–3.2 link the 2008
/// runtime and 3.3–3.4 the 2010 one; 3.5 and later use the Universal CRT, and 2.4–2.5 install
/// their own msvcr71.dll.
fn vc_runtime_hint(nums: [u64; 3], failure: &ChildFailure) -> Option<String> {
    let runtime = match (nums[0], nums[1]) {
        (2, 6..) | (3, 0..=2) => "2008",
        (3, 3..=4) => "2010",
        _ => return None,
    };
    let dll = match failure {
        ChildFailure::Start => true,
        ChildFailure::Exit { code, stderr } => {
            let lower = stderr.to_ascii_lowercase();
            matches!(code, Some(-1073741515 | 14001))
                || lower.contains("side-by-side")
                || lower.contains("msvcr")
        }
    };
    dll.then(|| format!("(Python <3.5 needs the Microsoft Visual C++ {runtime} runtime)"))
}

/// Runs the version's own Python in `prefix` with the user site and PYTHON* variables shut out
/// (measured: plain ensurepip read the user site despite `-s`). `nums`, when given, is the
/// version to name a missing Visual C++ runtime for (`vc_runtime_hint`).
fn python(prefix: &Path, args: &[&str], what: &str, nums: Option<[u64; 3]>) -> Result<(), String> {
    let with_hint = |msg: String, failure: ChildFailure| match nums
        .and_then(|n| vc_runtime_hint(n, &failure))
    {
        Some(h) => format!("{msg} {h}"),
        None => msg,
    };
    let parent: Vec<OsString> = std::env::vars_os().map(|(k, _)| k).collect();
    let out = match python_command(prefix, args, &parent)
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(out) => out,
        Err(e) => return Err(with_hint(format!("{what}: {e}"), ChildFailure::Start)),
    };
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Err(with_hint(
        format!("{what} failed ({}): {last}", out.status),
        ChildFailure::Exit {
            code: out.status.code(),
            stderr: &err,
        },
    ))
}

/// The steps that need the final location (Decision 7): executable copies, then pip.
pub fn finish(code: &Code, prefix: &Path, kind: Kind) -> Result<(), String> {
    let (x, y) = (code.nums[0], code.nums[1]);
    if code.ft {
        copy_if(
            &prefix.join(format!("python{x}.{y}t.exe")),
            &[prefix.join("python.exe")],
        )?;
        copy_if(
            &prefix.join(format!("pythonw{x}.{y}t.exe")),
            &[prefix.join("pythonw.exe")],
        )?;
    }
    let names = |stem: &str| {
        [
            format!("{stem}{x}.exe"),
            format!("{stem}{x}{y}.exe"),
            format!("{stem}{x}.{y}.exe"),
        ]
    };
    for stem in ["python", "pythonw"] {
        let dests: Vec<PathBuf> = names(stem).iter().map(|n| prefix.join(n)).collect();
        copy_if(&prefix.join(format!("{stem}.exe")), &dests)?;
    }
    let nt = prefix.join("Lib").join("venv").join("scripts").join("nt");
    let mut venv: Vec<PathBuf> = names("python").iter().map(|n| nt.join(n)).collect();
    venv.extend(names("pythonw").iter().map(|n| nt.join(n)));
    copy_if(&nt.join("python.exe"), &venv)?;
    match kind {
        Kind::Msi => {
            if prefix.join("Lib").join("ensurepip").is_dir() {
                python(
                    prefix,
                    &["-m", "ensurepip", "-U", "--default-pip"],
                    "ensurepip",
                    Some(code.nums),
                )?;
            }
        }
        Kind::Zip => {
            // pip is installed but `Scripts\` isn't (measured on 3.11.0): reinstall the bundled
            // wheel offline so pip's launchers exist, as pyenv-win's installs have them.
            let bundled = prefix.join("Lib").join("ensurepip").join("_bundled");
            let wheel = std::fs::read_dir(&bundled).ok().and_then(|rd| {
                rd.filter_map(Result::ok).map(|e| e.path()).find(|p| {
                    p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("pip-") && n.ends_with(".whl")
                    })
                })
            });
            if let Some(w) = wheel.filter(|_| !prefix.join("Scripts").join("pip.exe").exists()) {
                let w = w.to_string_lossy().into_owned();
                python(
                    prefix,
                    &[
                        "-m",
                        "pip",
                        "--isolated",
                        "install",
                        "--no-index",
                        "--no-deps",
                        "--force-reinstall",
                        "--no-warn-script-location",
                        "--disable-pip-version-check",
                        &w,
                    ],
                    "pip",
                    None,
                )?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Installed,
    /// Already installed and no `-f`: pyenv-win's silent skip.
    Skipped,
}

pub struct Job<'a> {
    pub root: &'a Path,
    pub code: &'a Code,
    pub force: bool,
    pub base: &'a str,
    pub fetcher: &'a Fetcher,
}

/// One version, end to end. Errors leave no `versions\<code>` behind (the transaction rolls
/// back) and no unverified file in the cache. Any failure after Ctrl+C is reported as
/// `Interrupted` (fix round 1, M3): a step it cut short fails with its own message.
pub fn install(job: &Job, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    match install_one(job, say) {
        Err(_) if interrupted() => Err(InstallError::Interrupted),
        r => r,
    }
}

fn install_one(job: &Job, say: &mut dyn FnMut(&str)) -> Result<Done, InstallError> {
    let text = job.code.text.as_str();
    let versions = job.root.join("versions");
    let msg = InstallError::Message;
    if !super::is_plain_name(text) {
        return Err(msg(format!("invalid version name: {text}")));
    }
    if is_complete_for(&versions.join(text), Flavor::PyenvWin) && !job.force {
        return Ok(Done::Skipped);
    }
    // The version lock is held from before the first download (fix round 1, I1): MSIs are
    // renamed onto their cache names before they are verified, so a concurrent install of the
    // same code could otherwise swap the bytes between this one's check and its extraction.
    // Every `?` from here on drops `txn`, which rolls back.
    let mut txn = Txn::begin_for(&versions, text, Flavor::PyenvWin).map_err(msg)?;
    let pkg = resolve(job.code, job.base, job.fetcher).map_err(msg)?;
    let cache = job.root.join("install_cache");
    let fetched = fetch_package(&pkg, text, &cache, job.fetcher, &mut |from, to| {
        say(&format!(":: [Downloading] ::  {text} ..."));
        say(&format!(":: [Downloading] ::  From {from}"));
        say(&format!(":: [Downloading] ::  To   {}", to.display()));
    })?;
    if !fetched.unsigned.is_empty() {
        say(&format!(
            ":: [Warning] :: python.org publishes no signature for {}; checked only by HTTPS.",
            fetched.unsigned.join(", ")
        ));
    }
    say(&format!(":: [Installing] ::  {text} ..."));
    let stage = txn.stage_dir().to_path_buf();
    unpack(&fetched, &stage).map_err(msg)?;
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    txn.place(&stage)
        .map_err(|e| msg(format!("cannot move {} into place: {e}", stage.display())))?;
    finish(job.code, &txn.target(), fetched.kind).map_err(msg)?;
    if interrupted() {
        return Err(InstallError::Interrupted);
    }
    txn.commit()
        .map_err(|e| msg(format!("cannot finish installing {text}: {e}")))?;
    say(&format!(":: [Info] :: completed! {text}"));
    Ok(Done::Installed)
}

#[cfg(test)]
mod tests {
    use super::super::openpgp::PINNED;
    use super::{python_command, signer_allowed, vc_runtime_hint, ChildFailure, NULL_DEVICE};
    use std::ffi::{OsStr, OsString};
    use std::path::Path;

    /// Fix round 1, I2: the child sees none of the parent's `PIP_*` variables and no pip
    /// configuration file, as ensurepip arranges for its own pip.
    #[test]
    fn python_children_get_no_pip_configuration() {
        let parent: Vec<OsString> = [
            "PIP_REQUIRE_VIRTUALENV",
            "PIP_TARGET",
            "PATH",
            "PIPX_HOME",
            "pip_user",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        let cmd = python_command(Path::new("prefix"), &["-m", "pip"], &parent);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = cmd.get_envs().collect();
        let get = |k: &str| {
            envs.iter()
                .find(|(n, _)| *n == OsStr::new(k))
                .map(|(_, v)| *v)
        };
        assert_eq!(get("PIP_REQUIRE_VIRTUALENV"), Some(None), "{envs:?}");
        assert_eq!(get("PIP_TARGET"), Some(None), "{envs:?}");
        assert_eq!(
            get("PIP_CONFIG_FILE"),
            Some(Some(OsStr::new(NULL_DEVICE))),
            "{envs:?}"
        );
        assert_eq!(
            get("PYTHONNOUSERSITE"),
            Some(Some(OsStr::new("1"))),
            "{envs:?}"
        );
        assert_eq!(get("PYTHONHOME"), Some(None), "{envs:?}");
        assert_eq!(get("PYTHONPATH"), Some(None), "{envs:?}");
        assert_eq!(get("PATH"), None, "inherited untouched: {envs:?}");
        assert_eq!(get("PIPX_HOME"), None, "not a PIP_ variable: {envs:?}");
        // Windows variable names ignore case; on Linux pip reads only upper-case names.
        assert_eq!(get("pip_user"), cfg!(windows).then_some(None), "{envs:?}");
        let args: Vec<&OsStr> = cmd.get_args().collect();
        assert_eq!(args, ["-E", "-s", "-m", "pip"]);
    }

    /// Ruling R4: any pinned key for a single MSI; only Steve Dower's for a component MSI.
    #[test]
    fn component_msis_accept_only_dowers_signature() {
        let [dower, loewis, baxter] = PINNED;
        assert!(signer_allowed(true, dower));
        assert!(signer_allowed(true, loewis));
        assert!(signer_allowed(true, baxter));
        assert!(signer_allowed(false, dower));
        assert!(!signer_allowed(false, loewis));
        assert!(!signer_allowed(false, baxter));
        let other = "0000000000000000000000000000000000000000";
        assert!(!signer_allowed(true, other));
        assert!(!signer_allowed(false, other));
    }

    /// Final review M5: a Python below 3.5 whose child can't start, or exits with a missing-DLL
    /// or side-by-side error, names the Visual C++ runtime it links.
    #[test]
    fn old_pythons_that_cannot_load_their_runtime_get_a_hint() {
        let exit = |code: i32, stderr: &'static str| ChildFailure::Exit {
            code: Some(code),
            stderr,
        };
        let h2008 = Some("(Python <3.5 needs the Microsoft Visual C++ 2008 runtime)".to_string());
        let h2010 = Some("(Python <3.5 needs the Microsoft Visual C++ 2010 runtime)".to_string());
        // The runtime by version.
        for (nums, want) in [
            ([2, 6, 9], &h2008),
            ([2, 7, 18], &h2008),
            ([3, 0, 1], &h2008),
            ([3, 2, 5], &h2008),
            ([3, 3, 5], &h2010),
            ([3, 4, 4], &h2010),
        ] {
            assert_eq!(
                vc_runtime_hint(nums, &ChildFailure::Start),
                *want,
                "{nums:?}"
            );
        }
        // No hint from 3.5 on (the Universal CRT), nor below 2.6 (bundled msvcr71).
        for nums in [[3, 5, 0], [3, 12, 10], [2, 5, 4]] {
            assert_eq!(
                vc_runtime_hint(nums, &ChildFailure::Start),
                None,
                "{nums:?}"
            );
        }
        // Which failures count.
        let v = [2, 7, 18];
        assert_eq!(
            vc_runtime_hint(v, &exit(-1073741515, "")),
            h2008,
            "0xC0000135"
        );
        assert_eq!(
            vc_runtime_hint(v, &exit(14001, "")),
            h2008,
            "ERROR_SXS_CANT_GEN_ACTCTX"
        );
        assert_eq!(
            vc_runtime_hint(v, &exit(1, "the side-by-side configuration is incorrect")),
            h2008
        );
        assert_eq!(
            vc_runtime_hint(v, &exit(1, "MSVCR90.dll was not found")),
            h2008
        );
        assert_eq!(vc_runtime_hint(v, &exit(1, "ImportError: no module")), None);
        assert_eq!(
            vc_runtime_hint(
                v,
                &ChildFailure::Exit {
                    code: None,
                    stderr: ""
                }
            ),
            None
        );
    }
}
