//! Safe zip extraction (plan M2b Task 3; review focus 2).

use pyenv::install::zipx::extract_zip;
use std::io::Write;
use std::path::Path;

fn zip_of(path: &Path, entries: &[(&str, &[u8])]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in entries {
        if name.ends_with('/') {
            w.add_directory(name.trim_end_matches('/'), opts).unwrap();
        } else {
            w.start_file(*name, opts).unwrap();
            w.write_all(body).unwrap();
        }
    }
    w.finish().unwrap();
}

#[test]
fn files_land_under_the_target_with_parents_created() {
    let d = tempfile::tempdir().unwrap();
    let z = d.path().join("p.zip");
    // python.org's zips have no directory entries (measured): parents must be created.
    zip_of(
        &z,
        &[
            ("python.exe", b"MZ"),
            ("Lib/os.py", b"# os"),
            ("DLLs/x.pyd", b""),
        ],
    );
    let t = d.path().join("t");
    assert_eq!(extract_zip(&z, &t).unwrap(), 3);
    assert_eq!(std::fs::read(t.join("python.exe")).unwrap(), b"MZ");
    assert_eq!(std::fs::read(t.join("Lib").join("os.py")).unwrap(), b"# os");
    assert!(t.join("DLLs").join("x.pyd").is_file());
}

#[test]
fn escaping_and_unsafe_names_are_refused_and_nothing_lands_outside() {
    for bad in [
        "../x",
        "a/../../x",
        "/abs",
        "\\abs",
        "C:/x",
        "a\\..\\..\\x",
        "CON",
        "a/NUL.txt",
        "dir./x",
    ] {
        let d = tempfile::tempdir().unwrap();
        let z = d.path().join("p.zip");
        zip_of(&z, &[("ok.txt", b"1"), (bad, b"2")]);
        let t = d.path().join("a").join("t");
        let e = extract_zip(&z, &t).unwrap_err();
        assert!(e.contains("unsafe path"), "{bad}: {e}");
        assert!(
            !d.path().join("x").exists() && !d.path().join("a").join("x").exists(),
            "{bad}"
        );
    }
}

#[test]
fn a_name_written_twice_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let z = d.path().join("p.zip");
    zip_of(&z, &[("Lib/a.py", b"1"), ("lib/A.py", b"2")]);
    let e = extract_zip(&z, &d.path().join("t")).unwrap_err();
    assert!(e.contains("twice"), "{e}");
}

#[test]
fn a_corrupt_zip_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let z = d.path().join("p.zip");
    std::fs::write(&z, b"PK\x03\x04garbage").unwrap();
    assert!(extract_zip(&z, &d.path().join("t")).is_err());
}
