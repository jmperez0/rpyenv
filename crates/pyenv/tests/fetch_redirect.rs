mod common;
use common::server::{start, Reply};

#[test]
fn redirect_target_reads_the_location_without_following_it() {
    let s = start(vec![(
        "/releases/latest",
        vec![Reply::Redirect(
            "https://example.invalid/releases/tag/26.7.2-0".into(),
        )],
    )]);
    let f = pyenv::install::fetch::Fetcher::direct();
    assert_eq!(
        f.redirect_target(&s.url("/releases/latest")).unwrap(),
        "https://example.invalid/releases/tag/26.7.2-0"
    );
}

#[test]
fn redirect_target_of_a_plain_answer_is_an_error() {
    let s = start(vec![("/x", vec![Reply::Body(b"hi".to_vec())])]);
    assert!(pyenv::install::fetch::Fetcher::direct()
        .redirect_target(&s.url("/x"))
        .is_err());
}
