//! `pyenv install` (pyenv-win flavor) against the tier-1 fake python.org (plan M2b Task 8).
#![cfg(windows)]

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, sha256, zip_bytes};
use common::Fixture;

const BANNER: &str = ":: [Info] ::  Mirror: https://www.python.org/ftp/python\r\n:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json\r\n:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases\r\n";
const HELP_LINES: usize = 17;

fn db(f: &Fixture, codes: &[(&str, bool)]) {
    let rows: Vec<pyenv::install::wincatalog::Row> = codes
        .iter()
        .map(|(c, zip_root)| pyenv::install::wincatalog::Row {
            code: c.to_string(),
            file: format!("{c}.exe"),
            url: format!("https://www.python.org/ftp/python/x/{c}.exe"),
            x64: !c.ends_with("-win32"),
            web_install: false,
            msi: false,
            zip_root_dir: zip_root.then(|| c.to_string()),
        })
        .collect();
    pyenv::install::wincatalog::write_db(&f.root, &rows).unwrap();
}

fn fake() -> Server {
    let zip = zip_bytes(&[
        ("python.exe", b"MZ"),
        ("pythonw.exe", b"MZ"),
        ("Lib/os.py", b""),
    ]);
    let sha = sha256(&zip);
    start_with(move |host| {
        let base = format!("{host}/ftp/python");
        vec![
            (
                "/ftp/python/index-windows.json".into(),
                vec![Reply::Body(index_json(
                    &[
                        (&format!("{base}/3.12.1/python-3.12.1-amd64.zip"), &sha),
                        (
                            &format!("{base}/3.12.2/python-3.12.2-amd64.zip"),
                            &"0".repeat(64),
                        ),
                    ],
                    None,
                ))],
            ),
            (
                "/ftp/python/3.12.1/python-3.12.1-amd64.zip".into(),
                vec![Reply::Body(zip.clone())],
            ),
            (
                "/ftp/python/3.12.2/python-3.12.2-amd64.zip".into(),
                vec![Reply::Body(zip.clone())],
            ),
        ]
    })
}

fn run(f: &Fixture, s: &Server, args: &[&str]) -> common::Run {
    let base = s.url("/ftp/python");
    f.pyenv_env(args, &[("RPYENV_TEST_PYTHON_ORG", base.as_str())])
}

#[test]
fn help_and_help_install_are_identical_and_start_with_the_banner() {
    let f = Fixture::new();
    let a = f.pyenv(&["install", "--help"]);
    let b = f.pyenv(&["help", "install"]);
    assert_eq!((a.code, b.code), (0, 0));
    assert_eq!(a.stdout, b.stdout);
    assert!(a
        .stdout
        .starts_with(&format!("{BANNER}Usage: pyenv install [-s] [-f] <version>")));
    assert_eq!(a.stdout.matches("\r\n").count(), 3 + HELP_LINES);
}

#[test]
fn the_mirror_variable_replaces_the_banner() {
    let f = Fixture::new();
    let r = f.pyenv_env(
        &["install", "--help"],
        &[("PYTHON_BUILD_MIRROR_URL", "https://m.example/py")],
    );
    assert!(
        r.stdout
            .starts_with(":: [Info] ::  Mirror: https://m.example/py\r\nUsage:"),
        "{}",
        r.stdout
    );
}

#[test]
fn pre_checks_in_pyenv_wins_order() {
    let f = Fixture::new();
    let cases: [(&[&str], &str); 3] = [
        (
            &["install", "--32only", "--64only", "3.12.1"],
            "pyenv-install: only --32only or --64only may be specified, not both.\r\n",
        ),
        (
            &["install", "-r", "--32only", "3.12.1"],
            "pyenv-install: --register not supported for 32 bits.\r\n",
        ),
        (
            &["install", "-r", "-a"],
            "pyenv-install: --register not supported for all versions.\r\n",
        ),
    ];
    for (args, msg) in cases {
        let r = f.pyenv(args);
        assert_eq!(
            (r.code, r.stdout.clone()),
            (1, format!("{BANNER}{msg}")),
            "{args:?}"
        );
    }
    let r = f.pyenv(&["install", "3.12.1"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, format!("{BANNER}pyenv-install: no definitions in local database\r\n\r\nPlease update the local database cache with `pyenv update'.\r\n"));
}

#[test]
fn list_prints_the_cache_in_document_order() {
    let f = Fixture::new();
    db(
        &f,
        &[
            ("3.12.1-win32", false),
            ("3.12.1", false),
            ("pypy3.10-v7.3.19-win64", true),
        ],
    );
    let r = f.pyenv(&["install", "--list"]);
    assert_eq!(
        (r.code, r.stdout),
        (
            0,
            format!("{BANNER}3.12.1-win32\r\n3.12.1\r\npypy3.10-v7.3.19-win64\r\n")
        )
    );
}

#[test]
fn an_unknown_version_is_reported_before_anything_installs() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "3.12.1", "9.9.9"]);
    assert_eq!(r.code, 1);
    assert_eq!(r.stdout, format!("{BANNER}pyenv-install: definition not found: 9.9.9\r\n\r\nSee all available versions with `pyenv install --list`.\r\nDoes the list seem out of date? Update it using `pyenv update`.\r\n"));
    assert!(!f.root.join("versions").join("3.12.1").exists());
}

#[test]
fn a_prefix_installs_the_newest_known_version_then_rehashes() {
    let f = Fixture::new();
    db(
        &f,
        &[
            ("3.12.0rc1", false),
            ("3.12.1", false),
            ("3.12.1-win32", false),
        ],
    );
    let s = fake();
    let r = run(&f, &s, &["install", "3.12"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(
        r.stdout
            .ends_with(":: [Installing] ::  3.12.1 ...\r\n:: [Info] :: completed! 3.12.1\r\n"),
        "{}",
        r.stdout
    );
    let v = f.root.join("versions").join("3.12.1");
    assert!(v.join("python3.12.exe").is_file());
    assert!(
        f.root.join("shims").join("python.exe").is_file(),
        "rehash ran"
    );
    // Installed now: a second run prints only the banner and exits 0.
    let again = run(&f, &s, &["install", "3.12.1"]);
    assert_eq!((again.code, again.stdout), (0, BANNER.to_string()));
}

// allowlist D-74
#[test]
fn a_failure_stops_the_run_exits_1_and_leaves_no_version() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false), ("3.12.2", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "3.12.2", "3.12.1"]);
    assert_eq!(r.code, 1);
    assert!(
        r.stdout
            .contains(":: [Error] :: cannot download python-3.12.2-amd64.zip: checksum mismatch"),
        "{}",
        r.stdout
    );
    assert!(r
        .stdout
        .contains(":: [Error] :: couldn't install 3.12.2\r\n"));
    assert!(
        !r.stdout.contains("3.12.1 ..."),
        "stops at the first failure"
    );
    assert!(!f.root.join("versions").join("3.12.2").exists());
}

// allowlist D-82
#[test]
fn pypy_and_graalpy_codes_are_refused() {
    let f = Fixture::new();
    db(&f, &[("pypy3.10-v7.3.19-win64", true)]);
    let r = f.pyenv(&["install", "pypy3.10-v7.3.19-win64"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.ends_with(":: [Error] :: rpyenv cannot install pypy3.10-v7.3.19-win64 yet: only CPython is supported.\r\n"), "{}", r.stdout);
}

#[test]
fn no_version_and_none_selected_prints_help_and_exits_0() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let r = f.pyenv(&["install"]);
    assert_eq!(r.code, 0);
    assert!(r
        .stdout
        .starts_with(&format!("{BANNER}Usage: pyenv install")));
}

// allowlist D-76
#[test]
fn register_is_ignored_with_one_info_line() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let s = fake();
    let r = run(&f, &s, &["install", "-r", "3.12.1"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(r.stdout.contains(":: [Info] :: rpyenv does not register versions for the py launcher; --register was ignored.\r\n"));
}

/// Spec §9.3 (Decision 14): default packages run after a new install, and a failure still
/// counts as a successful install. The fixture's python.exe isn't a real program, so pip fails.
// allowlist D-86
#[test]
fn default_packages_run_after_a_new_install_and_a_failure_still_succeeds() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    std::fs::write(f.root.join("default-packages"), "six\n").unwrap();
    let s = fake();
    let r = run(&f, &s, &["install", "3.12.1"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let file = f.root.join("default-packages");
    assert!(
        r.stdout.contains(&format!(
            ":: [Info] :: completed! 3.12.1\r\npyenv: error installing packages from  `{}'\r\n",
            file.display()
        )),
        "{}",
        r.stdout
    );
}

#[test]
fn clear_empties_the_cache_and_tolerates_none() {
    let f = Fixture::new();
    db(&f, &[("3.12.1", false)]);
    let r = f.pyenv(&["install", "-c"]);
    assert_eq!(
        (r.code, r.stdout.clone()),
        (0, BANNER.to_string()),
        "no install_cache: not an error"
    );
    let cache = f.root.join("install_cache");
    std::fs::create_dir_all(cache.join("3.10.11")).unwrap();
    std::fs::write(cache.join("x.zip"), "x").unwrap();
    let r = f.pyenv(&["install", "--clear"]);
    assert_eq!((r.code, r.stdout), (0, BANNER.to_string()));
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
}

#[test]
fn no_version_argument_installs_the_selected_prefix_resolved_against_the_db() {
    let f = Fixture::new();
    db(
        &f,
        &[
            ("3.12.0rc1", false),
            ("3.12.1", false),
            ("3.12.1-win32", false),
        ],
    );
    std::fs::write(f.work.join(".python-version"), "3.12\n").unwrap();
    let s = fake();
    let r = run(&f, &s, &["install"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(
        r.stdout
            .ends_with(":: [Installing] ::  3.12.1 ...\r\n:: [Info] :: completed! 3.12.1\r\n"),
        "{}",
        r.stdout
    );
    assert!(f
        .root
        .join("versions")
        .join("3.12.1")
        .join("python.exe")
        .is_file());
}
