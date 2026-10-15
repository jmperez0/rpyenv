//! `conda::latest_source` against the fake server (`RPYENV_TEST_CONDA`). One test function:
//! it sets a process-wide environment variable.

mod common;
use common::server::{start, Reply};
use pyenv::install::conda::{latest_source, Install};
use pyenv::install::fetch::Fetcher;

const SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn install(url: &str) -> Install {
    Install {
        package: "P".into(),
        url: url.into(),
        step: "miniconda".into(),
        verify: "verify_py3_latest".into(),
        env: vec![],
    }
}

#[test]
fn latest_installers_get_the_publishers_hash_or_are_refused() {
    let html = include_str!("fixtures/conda/miniconda.html");
    let s = start(vec![
        ("/miniconda/", vec![Reply::Body(html.as_bytes().to_vec())]),
        (
            "/miniforge/releases/latest",
            vec![Reply::Redirect(
                "https://example.invalid/conda-forge/miniforge/releases/tag/26.7.2-0".into(),
            )],
        ),
        (
            "/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Linux-x86_64.sh.sha256",
            vec![Reply::Body(
                format!("{SHA}  Miniforge3-26.7.2-0-Linux-x86_64.sh\n").into_bytes(),
            )],
        ),
    ]);
    std::env::set_var("RPYENV_TEST_CONDA", s.url(""));
    let f = Fetcher::direct();

    let url = "https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh";
    let got = latest_source("miniconda3-latest", &install(url), "Linux-x86_64", &f).unwrap();
    let want = format!("{url}#e8b25b92b262499141c5bd57a98d3c008024185fa951494b9cd9b6d94e72338b");
    assert_eq!(got.url, want);
    // A file the listing lacks is refused.
    let missing = "https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-s390x.sh";
    let e = latest_source("miniconda3-latest", &install(missing), "Linux-s390x", &f).unwrap_err();
    assert!(e.contains("won't install it"), "{e}");

    let forge = "https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-Linux-x86_64.sh";
    let got = latest_source("miniforge3-latest", &install(forge), "Linux-x86_64", &f).unwrap();
    assert_eq!(
        got.url,
        format!(
            "{}/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Linux-x86_64.sh#{SHA}",
            s.url("")
        )
    );
    // aarch64 has no `.sha256` on the server: refused.
    let e = latest_source("miniforge3-latest", &install(forge), "Linux-aarch64", &f).unwrap_err();
    assert!(e.contains("Miniforge3-26.7.2-0-Linux-aarch64.sh"), "{e}");
}
