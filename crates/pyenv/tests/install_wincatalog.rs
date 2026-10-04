//! pyenv-win's version codes and `.versions_cache.xml` (plan M2b Task 4; review focus 5).

use pyenv::install::wincatalog::{
    parse_code, parse_db, read_db, render_db, sort_rows, write_db, Arch, DbError, Row,
};
use std::path::Path;

fn real_db() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pyenv-win/versions_cache.xml"),
    )
    .unwrap()
}

#[test]
fn codes_parse_into_version_build_and_arch() {
    let c = parse_code("3.10.0rc2-win32").unwrap();
    assert_eq!(
        (c.version.as_str(), c.numeric.as_str(), c.nums),
        ("3.10.0rc2", "3.10.0", [3, 10, 0])
    );
    assert_eq!(
        (c.pre.clone(), c.ft, c.arch),
        (Some(("rc".to_string(), 2)), false, Arch::Win32)
    );
    let t = parse_code("3.13.0t-arm").unwrap();
    assert_eq!(
        (t.version.as_str(), t.ft, t.arch),
        ("3.13.0", true, Arch::Arm64)
    );
    let old = parse_code("2.4.3c1-win32").unwrap();
    assert_eq!(
        (old.numeric.as_str(), old.pre.clone()),
        ("2.4.3", Some(("c".to_string(), 1)))
    );
    assert_eq!(parse_code("2.7").unwrap().nums, [2, 7, 0]);
    assert_eq!(parse_code("3.12.1").unwrap().arch, Arch::Amd64);
    for bad in [
        "3.12.1-arm64",
        "3.12.1-amd64",
        "pypy3.10-v7.3.19-win64",
        "3",
        "3.12.",
        "3.12.1rc",
        "3.12.1x1",
        "graalpy-25.0.1-windows-amd64",
        "",
    ] {
        assert!(parse_code(bad).is_none(), "{bad}");
    }
}

#[test]
fn pyenv_wins_real_cache_reads_whole() {
    let rows = parse_db(&real_db()).unwrap();
    assert_eq!(rows.len(), 901);
    let first: Vec<&str> = rows.iter().take(5).map(|r| r.code.as_str()).collect();
    assert_eq!(
        first,
        [
            "2.4-win32",
            "2.4.1-win32",
            "2.4.2-win32",
            "2.4.3c1-win32",
            "2.4.3-win32"
        ]
    );
    assert_eq!(rows.last().unwrap().code, "graalpy-25.0.1-windows-amd64");
    let pypy = rows
        .iter()
        .find(|r| r.code == "pypy3.10-v7.3.19-win64")
        .unwrap();
    assert_eq!(pypy.zip_root_dir.as_deref(), Some("pypy3.10-v7.3.19-win64"));
    assert!(pypy.x64 && !pypy.msi);
    let msi = &rows[0];
    assert_eq!((msi.x64, msi.msi, msi.web_install), (false, true, false));
    assert_eq!(
        msi.url,
        "https://www.python.org/ftp/python/2.4/python-2.4.msi"
    );
}

/// The order pyenv-win's own cache is written in is the order `sort_rows` produces: measured
/// against upstream's 901-row file rather than assumed.
#[test]
fn sort_rows_reproduces_pyenv_wins_order() {
    let rows = parse_db(&real_db()).unwrap();
    let mut sorted = rows.clone();
    sort_rows(&mut sorted);
    let a: Vec<&str> = rows.iter().map(|r| r.code.as_str()).collect();
    let b: Vec<&str> = sorted.iter().map(|r| r.code.as_str()).collect();
    assert_eq!(a, b);
}

#[test]
fn render_round_trips_in_the_vbscript_writers_format() {
    let rows = parse_db(&real_db()).unwrap();
    let text = render_db(&rows);
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?>\r\n<versions>\r\n\t<version x64=\"false\" webInstall=\"false\" msi=\"true\">\r\n\t\t<code>2.4-win32</code>\r\n"));
    assert!(
        text.ends_with("\t</version>\r\n</versions>"),
        "no final newline"
    );
    assert!(!text.replace("\r\n", "").contains('\n'), "CRLF only");
    assert_eq!(parse_db(&text).unwrap(), rows);
}

#[test]
fn special_characters_are_escaped_and_unescaped() {
    let rows = vec![Row {
        code: "3.12.1".into(),
        file: "a&b<c>.exe".into(),
        url: "https://h/x?\"y\"".into(),
        x64: true,
        web_install: false,
        msi: false,
        zip_root_dir: None,
    }];
    assert_eq!(parse_db(&render_db(&rows)).unwrap(), rows);
}

#[test]
fn a_missing_empty_or_broken_cache_is_reported_not_a_panic() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(read_db(d.path()), Err(DbError::Missing));
    std::fs::write(
        d.path().join(".versions_cache.xml"),
        "<?xml version=\"1.0\"?>\n<versions>\n</versions>\n",
    )
    .unwrap();
    assert_eq!(read_db(d.path()), Err(DbError::Empty));
    for broken in [
        "garbage",
        "<versions><version><code>3.1</code></versions>",
        "<versions><version x64=\"true\"><file>f</file><URL>u</URL></version></versions>",
    ] {
        std::fs::write(d.path().join(".versions_cache.xml"), broken).unwrap();
        assert!(
            matches!(read_db(d.path()), Err(DbError::Malformed(_))),
            "{broken}"
        );
    }
}

#[test]
fn write_db_replaces_the_file_whole() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".versions_cache.xml"), "old").unwrap();
    let rows = parse_db(&real_db()).unwrap()[..3].to_vec();
    write_db(d.path(), &rows).unwrap();
    assert_eq!(read_db(d.path()).unwrap(), rows);
    let left: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
    assert_eq!(left.len(), 1, "no temporary file left");
}
