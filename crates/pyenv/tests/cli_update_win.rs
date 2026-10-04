//! `pyenv update` (pyenv-win flavor) against the tier-1 fake python.org (plan M2b Task 5).
#![cfg(windows)]

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, listing};
use common::Fixture;

const BANNER: &str = ":: [Info] ::  Mirror: https://www.python.org/ftp/python\r\n:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json\r\n:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases\r\n";
const SHA: &str = "11a906a2f36cacaee938c048968d99aa68ec0db592693b5a0fe3b161bb280ec5";

fn run(f: &Fixture, base: &str, args: &[&str]) -> common::Run {
    f.pyenv_env(args, &[("RPYENV_TEST_PYTHON_ORG", base)])
}

fn fake(root: Reply, page_2_7: Reply) -> Server {
    start_with(|host| {
        let base = format!("{host}/ftp/python");
        vec![
            ("/ftp/python/".into(), vec![root.clone()]),
            ("/ftp/python/2.7.18/".into(), vec![page_2_7.clone()]),
            (
                "/ftp/python/3.10.0/".into(),
                vec![Reply::Body(listing(&["amd64/", "amd64rc2/", "win32/"]))],
            ),
            (
                "/ftp/python/3.13.0/".into(),
                vec![Reply::Body(listing(&["amd64/"]))],
            ),
            (
                "/ftp/python/index-windows.json".into(),
                vec![Reply::Body(index_json(
                    &[(&format!("{base}/3.13.0/python-3.13.0-amd64.zip"), SHA)],
                    Some("index-windows-recent.json"),
                ))],
            ),
            (
                "/ftp/python/index-windows-recent.json".into(),
                vec![Reply::Body(index_json(
                    &[(&format!("{base}/3.13.0/python-3.13.0t-amd64.zip"), SHA)],
                    None,
                ))],
            ),
        ]
    })
}

fn ok_listing() -> Reply {
    Reply::Body(listing(&["2.3.7/", "2.7.18/", "3.10.0/", "3.13.0/"]))
}

fn ok_27() -> Reply {
    Reply::Body(listing(&["python-2.7.18.msi", "python-2.7.18.amd64.msi"]))
}

fn codes(f: &Fixture) -> Vec<String> {
    pyenv::install::wincatalog::read_db(&f.root)
        .unwrap()
        .into_iter()
        .map(|r| r.code)
        .collect()
}

#[test]
fn update_writes_the_cache_from_listings_and_the_index() {
    let f = Fixture::new();
    let s = fake(ok_listing(), ok_27());
    let r = run(&f, &s.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(
        r.stdout,
        format!("{BANNER}:: [Info] ::  Scanned 5 pages and found 7 installers.\r\n")
    );
    assert_eq!(
        codes(&f),
        [
            "2.7.18-win32",
            "2.7.18",
            "3.10.0rc2",
            "3.10.0-win32",
            "3.10.0",
            "3.13.0",
            "3.13.0t"
        ]
    );
    let raw = std::fs::read(f.root.join(".versions_cache.xml")).unwrap();
    assert!(raw.ends_with(b"</versions>") && raw.windows(2).any(|w| w == b"\r\n"));
}

#[test]
fn a_failed_root_listing_writes_nothing_and_exits_1_or_0_with_ignore() {
    for (args, code) in [(&["update"][..], 1), (&["update", "--ignore"][..], 0)] {
        let f = Fixture::new();
        let s = fake(Reply::Status(503), ok_27());
        let base = s.url("/ftp/python");
        let r = run(&f, &base, args);
        assert_eq!(r.code, code, "{args:?}: {}", r.stdout);
        assert!(r.stdout.starts_with(BANNER));
        assert!(
            r.stdout.contains(&format!(
                "HTTP Error downloading from mirror \"{base}/\"\r\nError(503): HTTP 503\r\n"
            )),
            "{}",
            r.stdout
        );
        assert!(!f.root.join(".versions_cache.xml").exists());
    }
}

#[test]
fn a_failed_version_page_stops_the_update_unless_ignored() {
    let f = Fixture::new();
    let s = fake(ok_listing(), Reply::Status(404));
    let base = s.url("/ftp/python");
    let r = run(&f, &base, &["update"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains(&format!(
        "HTTP Error downloading from mirror page \"{base}/2.7.18/\""
    )));
    assert!(!f.root.join(".versions_cache.xml").exists());
    let r = run(&f, &base, &["update", "--ignore"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(!codes(&f).contains(&"2.7.18".to_string()));
    assert!(codes(&f).contains(&"3.10.0".to_string()));
}

#[test]
fn update_help_prints_the_banner_and_usage() {
    let f = Fixture::new();
    let r = f.pyenv(&["update", "--help"]);
    assert_eq!(r.code, 0);
    assert_eq!(
        r.stdout,
        format!("{BANNER}Usage: pyenv update [--ignore]\r\n\r\n  --ignore  Ignores any HTTP/VBScript errors that occur during downloads.\r\n\r\nUpdates the internal database of python installer URL's.\r\n\r\n")
    );
}

#[test]
fn a_failed_index_page_writes_nothing_and_exits_1_or_0_with_ignore() {
    for (args, code) in [(&["update"][..], 1), (&["update", "--ignore"][..], 0)] {
        let f = Fixture::new();
        let s = start_with(|_| {
            vec![
                ("/ftp/python/".into(), vec![ok_listing()]),
                ("/ftp/python/2.7.18/".into(), vec![ok_27()]),
                (
                    "/ftp/python/3.10.0/".into(),
                    vec![Reply::Body(listing(&["amd64/"]))],
                ),
                (
                    "/ftp/python/3.13.0/".into(),
                    vec![Reply::Body(listing(&["amd64/"]))],
                ),
                (
                    "/ftp/python/index-windows.json".into(),
                    vec![Reply::Status(503)],
                ),
            ]
        });
        let base = s.url("/ftp/python");
        let r = run(&f, &base, args);
        assert_eq!(r.code, code, "{args:?}: {}", r.stdout);
        assert!(
            r.stdout.contains(&format!(
                "HTTP Error downloading from mirror \"{base}/index-windows.json\"\r\nError(503): HTTP 503\r\n"
            )),
            "{}",
            r.stdout
        );
        assert!(!f.root.join(".versions_cache.xml").exists());
    }
}
