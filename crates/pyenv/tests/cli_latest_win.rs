//! `pyenv latest` (pyenv-win flavor; pyenv-win-m1-reference.md "latest", m2 reference
//! "latest"). The installed cases are pyenv-win's own test_pyenv_feature_latest.py.
#![cfg(windows)]

mod common;
use common::Fixture;

const HELP: &str = "Usage: pyenv latest [-k|--known] [-q|--quiet] <prefix>\r\n\r\n  -k/--known      Select from all known versions instead of installed\r\n  -q/--quiet      Do not print an error message on resolution failure\r\n\r\n";

fn with(versions: &[&str]) -> Fixture {
    let f = Fixture::new();
    for v in versions {
        f.version(v);
    }
    f
}

#[test]
fn help_and_no_arguments() {
    let f = with(&[]);
    let r = f.pyenv(&["latest", "--help"]);
    assert_eq!((r.code, r.stdout), (0, HELP.to_string()));
    let r = f.pyenv(&["latest"]);
    assert_eq!((r.code, r.stdout), (1, HELP.to_string()));
    let r = f.pyenv(&["latest", "-q"]);
    assert_eq!((r.code, r.stdout), (1, String::new()));
    let r = f.pyenv(&["latest", "-k"]);
    assert_eq!(
        (r.code, r.stdout),
        (1, "pyenv-latest: missing <prefix> argument\r\n".to_string())
    );
}

#[test]
fn installed_versions_resolve_as_pyenv_win_does() {
    let f = with(&["3.1.4", "3.11.0", "3.2.0", "3.2.5", "3.9.1"]);
    for (p, want) in [("3.1", "3.1.4"), ("3.2", "3.2.5"), ("3.2.5", "3.2.5")] {
        let r = f.pyenv(&["latest", p]);
        assert_eq!((r.code, r.stdout), (0, format!("{want}\r\n")), "{p}");
    }
    let r = f.pyenv(&["latest", "1"]);
    assert_eq!(
        (r.code, r.stdout),
        (
            1,
            "pyenv-latest: no installed versions match the prefix '1'.\r\n".to_string()
        )
    );
    let r = f.pyenv(&["latest", "-q", "1"]);
    assert_eq!((r.code, r.stdout), (1, String::new()));
}

#[test]
fn the_architecture_suffix_must_match() {
    let f = with(&["3.1.0-win32", "3.1.4"]);
    let r = f.pyenv_env(&["latest", "3.1"], &[("PYENV_FORCE_ARCH", "X86")]);
    assert_eq!(r.stdout, "3.1.0-win32\r\n");
    let r = f.pyenv_env(&["latest", "3.1"], &[("PYENV_FORCE_ARCH", "AMD64")]);
    assert_eq!(r.stdout, "3.1.4\r\n");
}

#[test]
fn known_reads_the_version_cache() {
    let f = with(&[]);
    let rows: Vec<pyenv::install::wincatalog::Row> =
        ["3.12.0rc1", "3.12.1", "3.12.10", "3.12.10-win32"]
            .iter()
            .map(|c| pyenv::install::wincatalog::Row {
                code: c.to_string(),
                file: String::new(),
                url: String::new(),
                x64: true,
                web_install: false,
                msi: false,
                zip_root_dir: None,
            })
            .collect();
    let r = f.pyenv(&["latest", "-k", "3.12"]);
    assert_eq!(
        (r.code, r.stdout),
        (
            1,
            "pyenv-latest: no known versions match the prefix '3.12'.\r\n".to_string()
        ),
        "no cache: no candidates"
    );
    pyenv::install::wincatalog::write_db(&f.root, &rows).unwrap();
    let r = f.pyenv(&["latest", "-k", "3.12"]);
    assert_eq!((r.code, r.stdout), (0, "3.12.10\r\n".to_string()));
    let r = f.pyenv(&["latest", "--known", "1"]);
    assert_eq!(
        (r.code, r.stdout),
        (
            1,
            "pyenv-latest: no known versions match the prefix '1'.\r\n".to_string()
        )
    );
}
