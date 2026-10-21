#![cfg(windows)]
//! M7c design §6.3 against a real Miniforge install's `conda-meta` (Task 1 fixtures): the
//! shims `shims_win` makes are exactly the independently computed list.

use rpyenv_core::shimset;
use std::path::Path;

#[test]
fn miniforge_fixture_gets_the_expected_shims() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let t = tempfile::tempdir().unwrap();
    let versions = t.path().join("versions");
    let v = versions.join("miniforge3-x");
    let meta = v.join("conda-meta");
    std::fs::create_dir_all(&meta).unwrap();
    let mut listed = 0;
    for e in std::fs::read_dir(fx.join("miniforge3-conda-meta")).unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        std::fs::write(meta.join(p.file_name().unwrap()), &text).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        for f in json["files"].as_array().unwrap() {
            let f = f.as_str().unwrap();
            let lower = f.to_ascii_lowercase();
            let dir = lower.rsplit_once('/').map_or("", |(d, _)| d);
            if matches!(dir, "" | "scripts" | "library/bin") {
                let dest = v.join(f);
                std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                std::fs::write(&dest, b"").unwrap();
                listed += 1;
            }
        }
    }
    assert!(listed > 0);

    let mut got: Vec<String> = shimset::shims_win(&versions, &[])
        .into_iter()
        .map(|w| {
            let n = w.name.to_string_lossy().to_ascii_lowercase();
            n.strip_suffix(".exe").unwrap_or(&n).to_string()
        })
        .collect();
    got.sort();
    let mut want: Vec<String> = std::fs::read_to_string(fx.join("miniforge3-expected-shims.txt"))
        .unwrap()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    want.sort();
    assert_eq!(got, want);
}
