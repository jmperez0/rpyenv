//! `condacatalog::read_published` against the fake server (`RPYENV_TEST_CONDA`). One test
//! function: it sets a process-wide environment variable.

mod common;
use common::server::{start, Reply};
use pyenv::install::condacatalog::{read_published, sources};
use pyenv::install::fetch::Fetcher;

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn listing(file: &str, sha: &str) -> Vec<u8> {
    format!(
        "<tr><td><a href=\"{file}\">{file}</a></td><td>88.8M</td><td>2025-11-11 12:41:47</td><td>{sha}</td></tr>"
    )
    .into_bytes()
}

#[test]
fn read_published_collects_both_listings_and_the_releases() {
    const ASSET: &str = "Miniforge3-26.7.2-0-Windows-x86_64.exe";
    let releases = format!(
        r#"[{{"tag_name":"26.7.2-0","assets":[{{"name":"{ASSET}"}},{{"name":"{ASSET}.sha256"}}]}}]"#
    );
    let s =
        start(vec![
        (
            "/miniconda/",
            vec![Reply::Body(listing("Miniconda3-py313_25.9.1-3-Windows-x86_64.exe", SHA_A))],
        ),
        (
            "/archive/",
            vec![Reply::Body(listing("Anaconda3-2025.06-0-Windows-x86_64.exe", SHA_B))],
        ),
        (
            "/miniforge-api/releases?per_page=100&page=1",
            vec![Reply::Body(releases.into_bytes())],
        ),
        (
            "/miniforge-api/releases?per_page=100&page=2",
            vec![Reply::Body(b"[]".to_vec())],
        ),
        (
            "/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe.sha256",
            vec![Reply::Body(format!("{SHA_C}  {ASSET}\n").into_bytes())],
        ),
    ]);
    std::env::set_var("RPYENV_TEST_CONDA", s.url(""));
    let f = Fetcher::direct();

    let got = read_published(&f, &sources()).unwrap();
    let want = |path: &str, sha: &str| (s.url(path), Some(sha.to_string()));
    assert_eq!(
        got.into_iter().collect::<Vec<_>>(),
        vec![
            want("/archive/Anaconda3-2025.06-0-Windows-x86_64.exe", SHA_B),
            want(
                "/miniconda/Miniconda3-py313_25.9.1-3-Windows-x86_64.exe",
                SHA_A
            ),
            want(
                "/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe",
                SHA_C
            ),
        ]
    );

    // GitHub's rate-limit answer is an object, not a list: refused, with the reason.
    let limited = start(vec![
        (
            "/miniconda/",
            vec![Reply::Body(listing("M-Windows-x86_64.exe", SHA_A))],
        ),
        (
            "/archive/",
            vec![Reply::Body(listing("A-Windows-x86_64.exe", SHA_B))],
        ),
        (
            "/miniforge-api/releases?per_page=100&page=1",
            vec![Reply::Body(
                br#"{"message":"API rate limit exceeded"}"#.to_vec(),
            )],
        ),
    ]);
    std::env::set_var("RPYENV_TEST_CONDA", limited.url(""));
    let e = read_published(&f, &sources()).unwrap_err();
    assert!(e.contains("not a list"), "{e}");
}
