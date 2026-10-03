//! Tier-1 download tests (spec §12.5) against a local server.

mod common;

use common::server::{start, Reply};
use pyenv::install::fetch::{FetchRequest, Fetcher};
use pyenv::install::InstallError;
use std::collections::HashMap;
use std::path::PathBuf;

const BODY: &[u8] = b"pretend tarball";

fn sum() -> String {
    // Computed rather than hard-coded, so the test pins the behavior, not a constant.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("b");
    std::fs::write(&p, BODY).unwrap();
    pyenv::install::checksum::sha256_file(&p).unwrap()
}

fn fetcher(vars: &[(&str, &str)], cache: Option<PathBuf>) -> Fetcher {
    let map: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let mut f = Fetcher::from_env(&|k| map.get(k).cloned(), cache);
    f.retry_delay = std::time::Duration::ZERO;
    f
}

struct Got {
    result: Result<PathBuf, InstallError>,
    said: Vec<String>,
    log: String,
}

fn get(f: &Fetcher, url: String, sha: &str, dest: &std::path::Path) -> Got {
    let mut said = Vec::new();
    let mut log = Vec::new();
    let req = FetchRequest {
        file_name: "pkg-1.0.tar.gz".into(),
        url,
        sha256: sha.into(),
        dest_dir: dest.to_path_buf(),
    };
    let result = f.fetch(&req, &mut log, &mut |s: &str| said.push(s.to_string()));
    Got {
        result,
        said,
        log: String::from_utf8_lossy(&log).into_owned(),
    }
}

const NO_MIRROR: &[(&str, &str)] = &[("PYTHON_BUILD_SKIP_MIRROR", "1")];

#[test]
fn downloads_verifies_and_says_what_it_fetched() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let d = tempfile::tempdir().unwrap();
    let g = get(
        &fetcher(NO_MIRROR, None),
        s.url("/pkg-1.0.tar.gz"),
        &sum(),
        d.path(),
    );
    let file = g.result.unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), BODY);
    assert_eq!(file, d.path().join("pkg-1.0.tar.gz"));
    assert_eq!(
        g.said,
        vec![
            "Downloading pkg-1.0.tar.gz...".to_string(),
            format!("-> {}", s.url("/pkg-1.0.tar.gz"))
        ]
    );
}

#[test]
fn a_checksum_mismatch_fails_and_leaves_no_file() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let d = tempfile::tempdir().unwrap();
    let wrong = "0".repeat(64);
    let g = get(
        &fetcher(NO_MIRROR, None),
        s.url("/pkg-1.0.tar.gz"),
        &wrong,
        d.path(),
    );
    assert_eq!(g.result, Err(InstallError::Failed));
    assert_eq!(
        std::fs::read_dir(d.path()).unwrap().count(),
        0,
        "no file or .part left"
    );
    assert!(
        g.log
            .contains("checksum mismatch: pkg-1.0.tar.gz (file is corrupt)\n"),
        "{}",
        g.log
    );
    assert!(
        g.log
            .contains(&format!("expected {wrong}, got {}\n", sum())),
        "{}",
        g.log
    );
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1, "a mismatch is not retried");
    assert!(
        !g.said
            .iter()
            .any(|l| l.starts_with("error: failed to download")),
        "a mismatch is not a failed GET: {:?}",
        g.said
    );
}

#[test]
fn a_server_error_is_retried() {
    let s = start(vec![(
        "/pkg-1.0.tar.gz",
        vec![
            Reply::Status(503),
            Reply::Truncated(BODY.to_vec()),
            Reply::Body(BODY.to_vec()),
        ],
    )]);
    let d = tempfile::tempdir().unwrap();
    let g = get(
        &fetcher(NO_MIRROR, None),
        s.url("/pkg-1.0.tar.gz"),
        &sum(),
        d.path(),
    );
    assert!(g.result.is_ok(), "{:?} {}", g.said, g.log);
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 3);
}

#[test]
fn not_found_is_final_and_reported() {
    let s = start(vec![]);
    let d = tempfile::tempdir().unwrap();
    let g = get(
        &fetcher(NO_MIRROR, None),
        s.url("/pkg-1.0.tar.gz"),
        &sum(),
        d.path(),
    );
    assert_eq!(g.result, Err(InstallError::Failed));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1);
    assert_eq!(
        g.said.last().unwrap(),
        "error: failed to download pkg-1.0.tar.gz"
    );
}

#[test]
fn a_mirror_serves_the_file_by_checksum_and_falls_back_when_it_lacks_it() {
    let sha = sum();
    let mirror_path = format!("/m/{sha}");
    let s = start(vec![
        (mirror_path.as_str(), vec![Reply::Body(BODY.to_vec())]),
        ("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())]),
    ]);
    let d = tempfile::tempdir().unwrap();
    let base = s.url("/m/");
    let f = fetcher(&[("PYTHON_BUILD_MIRROR_URL", base.as_str())], None);
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sha, d.path());
    assert!(g.result.is_ok());
    assert_eq!(g.said[1], format!("-> {}", s.url(&mirror_path)));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 0);

    let other = "1".repeat(64);
    let d2 = tempfile::tempdir().unwrap();
    let g2 = get(&f, s.url("/pkg-1.0.tar.gz"), &other, d2.path());
    // The mirror lacks it (404 on HEAD), the original is fetched, and its checksum fails.
    assert_eq!(g2.result, Err(InstallError::Failed));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1);
}

#[test]
fn a_verified_cache_hit_prints_nothing_and_skips_the_network() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let cache = tempfile::tempdir().unwrap();
    let f = fetcher(NO_MIRROR, Some(cache.path().to_path_buf()));
    let d1 = tempfile::tempdir().unwrap();
    assert!(get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d1.path())
        .result
        .is_ok());
    assert!(cache.path().join("pkg-1.0.tar.gz").is_file());
    let d2 = tempfile::tempdir().unwrap();
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d2.path());
    assert!(g.result.is_ok());
    assert!(g.said.is_empty(), "{:?}", g.said);
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 1);
}

#[test]
fn an_invalid_cached_file_is_downloaded_again() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(cache.path().join("pkg-1.0.tar.gz"), b"stale").unwrap();
    let d = tempfile::tempdir().unwrap();
    let g = get(
        &fetcher(NO_MIRROR, Some(cache.path().to_path_buf())),
        s.url("/pkg-1.0.tar.gz"),
        &sum(),
        d.path(),
    );
    assert!(g.result.is_ok());
    assert_eq!(
        std::fs::read(cache.path().join("pkg-1.0.tar.gz")).unwrap(),
        BODY
    );
}

#[test]
fn a_stalled_download_is_abandoned_and_retried() {
    let s = start(vec![(
        "/pkg-1.0.tar.gz",
        vec![Reply::Stall(BODY.to_vec()), Reply::Body(BODY.to_vec())],
    )]);
    let d = tempfile::tempdir().unwrap();
    let mut f = fetcher(NO_MIRROR, None);
    f.stall_timeout = std::time::Duration::from_millis(2000);
    let t = std::time::Instant::now();
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d.path());
    assert!(g.result.is_ok(), "{:?} {}", g.said, g.log);
    // Hits are counted on the server thread, so allow it a moment.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while s.hits("/pkg-1.0.tar.gz") < 2 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 2);
    assert!(
        t.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        t.elapsed()
    );
}

#[test]
fn a_mirror_with_a_bad_checksum_falls_back_to_the_original() {
    let sha = sum();
    let mirror_path = format!("/m/{sha}");
    let s = start(vec![
        (
            mirror_path.as_str(),
            vec![Reply::Body(b"wrong bytes".to_vec())],
        ),
        ("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())]),
    ]);
    let d = tempfile::tempdir().unwrap();
    let base = s.url("/m/");
    let f = fetcher(&[("PYTHON_BUILD_MIRROR_URL", base.as_str())], None);
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sha, d.path());
    assert_eq!(g.result.unwrap(), d.path().join("pkg-1.0.tar.gz"));
    assert_eq!(
        g.said,
        vec![
            "Downloading pkg-1.0.tar.gz...".to_string(),
            format!("-> {}", s.url(&mirror_path)),
            format!("-> {}", s.url("/pkg-1.0.tar.gz")),
        ]
    );
    assert!(g.log.contains("checksum mismatch"), "{}", g.log);
}

#[test]
fn three_server_errors_fail_after_exactly_three_attempts() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Status(503)])]);
    let d = tempfile::tempdir().unwrap();
    let g = get(
        &fetcher(NO_MIRROR, None),
        s.url("/pkg-1.0.tar.gz"),
        &sum(),
        d.path(),
    );
    assert_eq!(g.result, Err(InstallError::Failed));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 3);
    assert_eq!(
        g.said.last().unwrap(),
        "error: failed to download pkg-1.0.tar.gz"
    );
}

#[test]
fn a_malformed_expected_checksum_is_refused_up_front() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let d = tempfile::tempdir().unwrap();
    let g = get(
        &fetcher(NO_MIRROR, None),
        s.url("/pkg-1.0.tar.gz"),
        "ABC",
        d.path(),
    );
    assert!(matches!(g.result, Err(InstallError::Message(_))));
    assert_eq!(s.hits("/pkg-1.0.tar.gz"), 0);
}

#[test]
fn an_abandoned_attempt_leaves_no_part_file() {
    let s = start(vec![(
        "/pkg-1.0.tar.gz",
        vec![Reply::Stall(BODY.to_vec()), Reply::Body(BODY.to_vec())],
    )]);
    let d = tempfile::tempdir().unwrap();
    let mut f = fetcher(NO_MIRROR, None);
    f.stall_timeout = std::time::Duration::from_millis(2000);
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d.path());
    assert!(g.result.is_ok(), "{:?} {}", g.said, g.log);
    std::thread::sleep(std::time::Duration::from_millis(500));
    let names: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["pkg-1.0.tar.gz".to_string()]);
}

#[test]
fn the_cache_gets_exactly_the_file_and_no_temp_file() {
    let s = start(vec![("/pkg-1.0.tar.gz", vec![Reply::Body(BODY.to_vec())])]);
    let cache = tempfile::tempdir().unwrap();
    let d = tempfile::tempdir().unwrap();
    let f = fetcher(NO_MIRROR, Some(cache.path().to_path_buf()));
    assert!(get(&f, s.url("/pkg-1.0.tar.gz"), &sum(), d.path())
        .result
        .is_ok());
    let names: Vec<_> = std::fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["pkg-1.0.tar.gz".to_string()]);
}

#[test]
fn a_successful_mirror_head_is_logged() {
    let sha = sum();
    let mirror_path = format!("/m/{sha}");
    let s = start(vec![(
        mirror_path.as_str(),
        vec![Reply::Body(BODY.to_vec())],
    )]);
    let d = tempfile::tempdir().unwrap();
    let base = s.url("/m/");
    let f = fetcher(&[("PYTHON_BUILD_MIRROR_URL", base.as_str())], None);
    let g = get(&f, s.url("/pkg-1.0.tar.gz"), &sha, d.path());
    assert!(g.result.is_ok());
    assert!(
        g.log.contains(&format!(
            "mirror HEAD ok: {}
",
            s.url(&mirror_path)
        )),
        "{}",
        g.log
    );
}
