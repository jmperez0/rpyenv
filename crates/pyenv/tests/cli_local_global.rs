mod common;
use common::Fixture;
use std::fs;

#[cfg(unix)]
#[test]
fn local_show_set_and_unset() {
    let f = Fixture::new();
    f.version("3.12.10").version("3.11.9");
    let r = f.pyenv(&["local"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: no local version configured for this directory\n", 1)
    );
    assert_eq!(f.pyenv(&["local", "3.12", "3.11.9"]).code, 0);
    assert_eq!(
        fs::read_to_string(f.work.join(".python-version")).unwrap(),
        "3.12\n3.11.9\n"
    );
    assert_eq!(f.pyenv(&["local"]).stdout, "3.12\n3.11.9\n");
    assert_eq!(f.pyenv(&["local", "--unset"]).code, 0);
    assert!(!f.work.join(".python-version").exists());
    assert_eq!(f.pyenv(&["local", "--unset"]).code, 0);
}

#[cfg(unix)]
#[test]
fn local_validation_and_force() {
    let f = Fixture::new();
    f.file(&f.work.join(".python-version"), "keep\n");
    let r = f.pyenv(&["local", "9.9"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: version `9.9' not installed\n", 1)
    );
    assert_eq!(
        fs::read_to_string(f.work.join(".python-version")).unwrap(),
        "keep\n"
    );
    assert_eq!(f.pyenv(&["local", "-f", "9.9"]).code, 0);
    assert_eq!(
        fs::read_to_string(f.work.join(".python-version")).unwrap(),
        "9.9\n"
    );
}

#[cfg(unix)]
#[test]
fn local_shows_a_parent_file_and_fails_silently_on_an_empty_one() {
    let f = Fixture::new();
    let sub = f.work.join("sub");
    fs::create_dir_all(&sub).unwrap();
    f.file(&f.work.join(".python-version"), "3.12.10\n");
    assert_eq!(f.pyenv_in(&sub, &["local"]).stdout, "3.12.10\n");
    f.file(&f.work.join(".python-version"), "");
    let r = f.pyenv_in(&sub, &["local"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 1));
}

#[cfg(unix)]
#[test]
fn local_unset_reports_removal_failures_other_than_missing() {
    // Fix 3 (M-1): a removal failure other than "missing file" must not be silent.
    let f = Fixture::new();
    fs::create_dir_all(f.work.join(".python-version")).unwrap();
    let r = f.pyenv(&["local", "--unset"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("cannot remove"), "stderr: {}", r.stderr);
    assert!(f.work.join(".python-version").exists());
}

#[cfg(unix)]
#[test]
fn global_show_set_and_quirks() {
    let f = Fixture::new();
    f.version("3.12.10");
    assert_eq!(f.pyenv(&["global"]).stdout, "system\n");
    assert_eq!(f.pyenv(&["global", "3.12"]).code, 0);
    assert_eq!(
        fs::read_to_string(f.root.join("version")).unwrap(),
        "3.12\n"
    );
    assert_eq!(f.pyenv(&["global"]).stdout, "3.12\n");
    let r = f.pyenv(&["global", "--unset"]);
    assert_eq!(
        (r.stderr.as_str(), r.code),
        ("pyenv: version `--unset' not installed\n", 1)
    );
    assert_eq!(f.pyenv(&["global", "-f", "9.9"]).code, 0);
    assert_eq!(fs::read_to_string(f.root.join("version")).unwrap(), "9.9\n");
}

#[cfg(unix)]
#[test]
fn global_reads_the_legacy_global_file() {
    let f = Fixture::new();
    f.file(&f.root.join("global"), "3.11.9\n");
    assert_eq!(f.pyenv(&["global"]).stdout, "3.11.9\n");
}

#[cfg(windows)]
#[test]
fn win_local() {
    let f = Fixture::new();
    f.version("3.7.7");
    let r = f.pyenv(&["local"]);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("no local version configured for this directory\r\n", 0)
    );
    assert_eq!(f.pyenv(&["local", "3.7"]).code, 0);
    assert_eq!(
        fs::read_to_string(f.work.join(".python-version")).unwrap(),
        "3.7\r\n"
    );
    assert_eq!(f.pyenv(&["local"]).stdout, "3.7\r\n");
    assert_eq!(f.pyenv(&["vname"]).stdout, "3.7.7\r\n");
    let r = f.pyenv(&["local", "3.6"]);
    assert_eq!(r.code, 1);
    assert_eq!(
        r.stdout,
        "pyenv specific python requisite didn't meet. Project is using different version of python.\r\nInstall python '3.6' by typing: 'pyenv install 3.6'\r\n"
    );
    assert_eq!(f.pyenv(&["local", "--unset"]).code, 0);
    let r = f.pyenv(&["local", "--unset"]);
    assert_eq!((r.stdout.as_str(), r.stderr.as_str(), r.code), ("", "", 0));
}

#[cfg(windows)]
#[test]
fn local_unset_reports_removal_failures_other_than_missing() {
    // Fix 3 (M-1): a removal failure other than "missing file" must not be silent.
    let f = Fixture::new();
    fs::create_dir_all(f.work.join(".python-version")).unwrap();
    let r = f.pyenv(&["local", "--unset"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("cannot remove"), "stdout: {}", r.stdout);
    assert!(f.work.join(".python-version").exists());
}

#[cfg(windows)]
#[test]
fn win_global() {
    let f = Fixture::new();
    f.version("3.9.1");
    assert_eq!(
        f.pyenv(&["global"]).stdout,
        "no global version configured\r\n"
    );
    assert_eq!(f.pyenv(&["global", "3.9"]).code, 0);
    assert_eq!(f.pyenv(&["global"]).stdout, "3.9\r\n");
    assert_eq!(f.pyenv(&["global", "--unset"]).code, 0);
    assert!(!f.root.join("version").exists());
}
