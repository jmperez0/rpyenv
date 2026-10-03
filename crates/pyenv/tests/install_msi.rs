//! The MSI extractor against `msiexec /a` (plan M2b Task 2). The manifest is what msiexec
//! wrote for python.org's 3.10.11 amd64 tools.msi: every file's path, size and SHA-256.

use pyenv::install::checksum::sha256_file;
use pyenv::install::msi::extract_msi;
use std::path::Path;

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let size = std::fs::metadata(&p).unwrap().len();
            out.push(format!("{rel}\t{size}\t{}", sha256_file(&p).unwrap()));
        }
    }
}

#[test]
fn tools_msi_extracts_exactly_as_msiexec_did() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/msi");
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("t");
    let written = extract_msi(&fx.join("tools-3.10.11-amd64.msi"), &target).unwrap();
    assert_eq!(written.len(), 105);
    let mut got = Vec::new();
    walk(&target, &target, &mut got);
    got.sort();
    let want: Vec<String> = std::fs::read_to_string(fx.join("tools-3.10.11-amd64.manifest"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    assert_eq!(got, want);
}

#[test]
fn a_file_that_is_not_an_msi_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.msi");
    std::fs::write(&p, b"not an msi").unwrap();
    let e = extract_msi(&p, &dir.path().join("t")).unwrap_err();
    assert!(e.starts_with("not an MSI"), "{e}");
}
