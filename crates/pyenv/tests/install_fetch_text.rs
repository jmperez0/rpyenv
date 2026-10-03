//! `Fetcher::get_text` against a local server (plan M2b Task 3, fix round 1).

mod common;

use common::server::{start, Reply};
use pyenv::install::fetch::Fetcher;
use std::time::{Duration, Instant};

fn fetcher() -> Fetcher {
    let mut f = Fetcher::direct();
    f.retry_delay = Duration::ZERO;
    f.text_timeout = Duration::from_secs(1);
    f
}

#[test]
fn a_503_then_a_200_returns_the_body() {
    let s = start(vec![(
        "/t",
        vec![Reply::Status(503), Reply::Body(b"hello".to_vec())],
    )]);
    assert_eq!(fetcher().get_text(&s.url("/t")).unwrap(), "hello");
    assert_eq!(s.hits("/t"), 2);
}

#[test]
fn a_404_is_final_after_one_hit() {
    let s = start(vec![("/t", vec![Reply::Status(404)])]);
    let e = fetcher().get_text(&s.url("/t")).unwrap_err();
    assert_eq!(e.status, Some(404));
    assert_eq!(s.hits("/t"), 1);
}

#[test]
fn a_stalled_body_times_out_and_is_retried_within_bounds() {
    let s = start(vec![("/t", vec![Reply::Stall(b"abcdefgh".to_vec())])]);
    let t = Instant::now();
    let e = fetcher().get_text(&s.url("/t")).unwrap_err();
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    assert_eq!(e.status, None);
    assert_eq!(s.hits("/t"), 3, "a stalled body is retried: {e:?}");
}

#[test]
fn a_truncated_body_is_retried_then_succeeds() {
    let s = start(vec![(
        "/t",
        vec![
            Reply::Truncated(b"abcdefgh".to_vec()),
            Reply::Body(b"abcdefgh".to_vec()),
        ],
    )]);
    assert_eq!(fetcher().get_text(&s.url("/t")).unwrap(), "abcdefgh");
    assert_eq!(s.hits("/t"), 2);
}
