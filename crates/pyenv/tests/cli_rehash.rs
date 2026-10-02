mod common;
use common::{shim_exe, Fixture};
use std::fs;

#[cfg(unix)]
#[test]
fn rehash_links_each_executable_and_shims_lists_them() {
    let f = Fixture::new();
    f.exe("3.12.1/bin/python");
    f.exe("3.12.1/bin/pip");
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0));
    let shims = f.root.join("shims");
    use std::os::unix::fs::MetadataExt;
    let template = shims.join(".template").join("pyenv-shim");
    assert_eq!(fs::read(&template).unwrap(), fs::read(shim_exe()).unwrap());
    let (s, t) = (
        fs::symlink_metadata(shims.join("python")).unwrap(),
        fs::metadata(&template).unwrap(),
    );
    assert!(s.is_file(), "python should be a hardlink, not a symlink");
    assert_eq!((s.dev(), s.ino()), (t.dev(), t.ino()));
    assert_eq!(
        f.pyenv(&["shims"]).stdout,
        format!(
            "{}\n{}\n",
            shims.join("pip").display(),
            shims.join("python").display()
        )
    );
    assert_eq!(f.pyenv(&["shims", "--short"]).stdout, "pip\npython\n");
}

/// The listing counts a regular file with any x bit, whoever may run it, as upstream's
/// listing does: a file only group and others may run still gets a shim and is listed, so
/// the shim set doesn't depend on who rehashed (allowlist D-33). The lookup's `-x` check
/// (D-29) applies only when a shim or `exec` looks the name up. Root may run any file with
/// an x bit, so the check is skipped there.
#[cfg(unix)]
#[test]
fn rehash_shims_a_file_the_caller_may_not_run() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.exe("3.12.1/bin/python");
    let locked = f.exe("3.12.1/bin/locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o011)).unwrap();
    if fs::read(&locked).is_ok() {
        eprintln!("running as root: skipping rehash_shims_a_file_the_caller_may_not_run");
        return;
    }
    let r = f.pyenv(&["versions", "--executables"]);
    assert_eq!((r.stdout.as_str(), r.code), ("locked\npython\n", 0));
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0));
    assert_eq!(f.pyenv(&["shims", "--short"]).stdout, "locked\npython\n");
}

#[cfg(unix)]
#[test]
fn rehash_lock_timeout_and_unwritable_messages() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let shims = f.root.join("shims");
    let lock = shims.join(".rehash.lock");
    fs::create_dir_all(&shims).unwrap();
    fs::write(&lock, "").unwrap();
    let r = f.pyenv_env(&["rehash"], &[("PYENV_REHASH_TIMEOUT", "0")]);
    assert_eq!(
        r.stderr,
        format!(
            "pyenv: cannot rehash: couldn't acquire lock {l} for 0 seconds. Last error message:\n\
             {l}: cannot overwrite existing file\n",
            l = lock.display()
        )
    );
    assert_eq!(r.code, 1);
    fs::remove_file(&lock).unwrap();
    fs::set_permissions(&shims, fs::Permissions::from_mode(0o555)).unwrap();
    let root_user = fs::write(shims.join("probe"), "").is_ok();
    if !root_user {
        let r = f.pyenv(&["rehash"]);
        assert_eq!(
            (r.stderr, r.code),
            (
                format!("pyenv: cannot rehash: {} isn't writable\n", shims.display()),
                1
            )
        );
    }
    fs::set_permissions(&shims, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn help_for_rehash_and_shims() {
    let f = Fixture::new();
    assert_eq!(
        f.pyenv(&["help", "rehash"]).stdout,
        "Usage: pyenv rehash\n\nRehash pyenv shims (run this after installing executables)\n\n"
    );
    assert_eq!(f.pyenv(&["help", "--usage", "rehash"]).stdout, "");
    assert_eq!(
        f.pyenv(&["help", "--usage", "shims"]).stdout,
        "Usage: pyenv shims [--short]\n"
    );
    let r = f.pyenv(&["shims"]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 0));
}

#[cfg(windows)]
#[test]
fn win_rehash_without_versions() {
    let r = Fixture::new().pyenv(&["rehash"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        (
            "No version installed. Please install one with 'pyenv install <version>'.\r\n",
            0
        )
    );
}

#[cfg(windows)]
#[test]
fn win_rehash_and_shims() {
    let f = Fixture::new();
    f.exe("3.9.1/python.exe");
    f.exe("3.9.1/Scripts/pip.exe");
    let shims = f.root.join("shims");
    fs::create_dir_all(&shims).unwrap();
    fs::write(shims.join("python.bat"), "@echo off\r\n").unwrap();
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.code), ("", 0));
    assert_eq!(
        fs::read(shims.join("python.exe")).unwrap(),
        fs::read(shim_exe()).unwrap()
    );
    assert!(!shims.join("python.bat").exists());
    assert_eq!(
        f.pyenv(&["shims"]).stdout,
        format!(
            "{}\r\n{}\r\n\r\n",
            shims.join("pip.exe").display(),
            shims.join("python.exe").display()
        )
    );
    assert_eq!(
        f.pyenv(&["shims", "--short"]).stdout,
        "pip.exe\r\npython.exe\r\n\r\n"
    );
    let help = "Usage: pyenv shims\r\n       pyenv shims --short\r\n\r\nList the existing pyenv shims\r\n\r\n";
    assert_eq!(f.pyenv(&["shims", "--help"]).stdout, help);
    let r = f.pyenv(&["shims", "--other"]);
    assert_eq!((r.stdout, r.code), (format!("{help}\r\n"), 0));
}

/// A version with `bin/conda` doesn't hide tools from rehash: `curl` and `clear` get shims,
/// and with that version selected `which` finds conda's copies. Upstream's conda.bash hook
/// drops the names in conda.d/default.list (allowlist D-44).
#[cfg(unix)]
#[test]
fn rehash_keeps_the_tools_of_a_conda_version() {
    let f = Fixture::new();
    f.exe("miniconda3-latest/bin/python");
    f.exe("miniconda3-latest/bin/conda");
    let curl = f.exe("miniconda3-latest/bin/curl");
    f.exe("miniconda3-latest/bin/clear");
    let r = f.pyenv(&["rehash"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0));
    assert_eq!(
        f.pyenv(&["shims", "--short"]).stdout,
        "clear\nconda\ncurl\npython\n"
    );
    let r = f.pyenv_env(
        &["which", "curl"],
        &[("PYENV_VERSION", "miniconda3-latest")],
    );
    assert_eq!(r.stdout, format!("{}\n", curl.display()));
}
