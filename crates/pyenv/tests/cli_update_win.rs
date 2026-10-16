//! `pyenv update` (pyenv-win flavor) against the tier-1 fake python.org (plan M2b Task 5).
#![cfg(windows)]

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, listing, pypy_versions_json};
use common::Fixture;

const BANNER: &str = ":: [Info] ::  Mirror: https://www.python.org/ftp/python\r\n:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json\r\n:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases\r\n";
const SHA: &str = "11a906a2f36cacaee938c048968d99aa68ec0db592693b5a0fe3b161bb280ec5";
const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const MF_26: &str = "Miniforge3-26.7.2-0-Windows-x86_64.exe";
const MF_27: &str = "Miniforge3-27.1.0-0-Windows-x86_64.exe";

/// The shipped conda table a test points `RPYENV_TEST_CONDA_TABLES` at: one `ok` row for
/// miniforge3-26.7.2-0 with `sha`, downloaded from `url_base` (M7c ruling R2).
fn shipped_table(url_base: &str, sha: &str) -> String {
    format!(
        "# generated 2026-10-01T00:00:00Z by ci\n# <name>\\t<arch>\\t<verdict>\nminiforge3-26.7.2-0\tWindows-x86_64\tok {} {url_base}/releases/download/26.7.2-0/{MF_26}#{sha} miniconda verify_py3\n",
        MF_26.trim_end_matches(".exe")
    )
}

/// `(tables folder, conda base URL)` for a run: the folder gets the default shipped table
/// unless the test wrote its own `windows.tsv` first; the base is the fake server's root.
fn conda_env(f: &Fixture, base: &str) -> (String, String) {
    let dir = f.base.join("conda-tables");
    std::fs::create_dir_all(&dir).unwrap();
    let table = dir.join("windows.tsv");
    if !table.exists() {
        std::fs::write(
            &table,
            shipped_table("https://github.com/conda-forge/miniforge", SHA_A),
        )
        .unwrap();
    }
    (
        dir.to_str().unwrap().to_string(),
        base.trim_end_matches("/ftp/python").to_string(),
    )
}

fn run(f: &Fixture, base: &str, args: &[&str]) -> common::Run {
    let pypy = format!("{}/pypy", base.trim_end_matches("/ftp/python"));
    let (tables, conda) = conda_env(f, base);
    f.pyenv_env(
        args,
        &[
            ("RPYENV_TEST_PYTHON_ORG", base),
            ("RPYENV_TEST_PYPY", &pypy),
            ("RPYENV_TEST_CONDA", &conda),
            ("RPYENV_TEST_CONDA_TABLES", &tables),
        ],
    )
}

/// A repo.anaconda.com listing row (file, size, time, SHA-256).
fn conda_listing(file: &str, sha: &str) -> Vec<u8> {
    format!(
        "<tr><td><a href=\"{file}\">{file}</a></td><td>88.8M</td><td>2025-11-11 12:41:47</td><td>{sha}</td></tr>"
    )
    .into_bytes()
}

/// One Miniforge release `tag` with `asset` and its sidecar.
fn releases(tag: &str, asset: &str) -> Reply {
    Reply::Body(
        format!(
            r#"[{{"tag_name":"{tag}","assets":[{{"name":"{asset}"}},{{"name":"{asset}.sha256"}}]}}]"#
        )
        .into_bytes(),
    )
}

fn fake(root: Reply, page_2_7: Reply) -> Server {
    fake_pypy(
        root,
        page_2_7,
        &[
            "pypy3.11-v7.3.99-win64.zip",
            "pypy3.10-v7.3.99-win64.zip",
            "pypy3.11-v7.3.99-linux64.tar.bz2",
        ],
    )
}

fn fake_pypy(root: Reply, page_2_7: Reply, pypy_files: &[&str]) -> Server {
    fake_conda(
        root,
        page_2_7,
        pypy_files,
        releases("26.7.2-0", MF_26),
        &[("26.7.2-0", MF_26, SHA_A)],
    )
}

/// The python.org and PyPy routes plus minimal valid conda ones (ruling R2): one Windows row
/// per listing, `releases` on page 1, `[]` on page 2, and a sidecar per `(tag, asset, sha)`.
fn fake_conda(
    root: Reply,
    page_2_7: Reply,
    pypy_files: &[&str],
    releases: Reply,
    sidecars: &[(&str, &str, &str)],
) -> Server {
    start_with(|host| {
        let base = format!("{host}/ftp/python");
        let mut conda: Vec<(String, Vec<Reply>)> = vec![
            (
                "/miniconda/".into(),
                vec![Reply::Body(conda_listing(
                    "Miniconda3-py313_25.9.1-3-Windows-x86_64.exe",
                    SHA_B,
                ))],
            ),
            (
                "/archive/".into(),
                vec![Reply::Body(conda_listing(
                    "Anaconda3-2025.06-0-Windows-x86_64.exe",
                    SHA_B,
                ))],
            ),
            (
                "/miniforge-api/releases?per_page=100&page=1".into(),
                vec![releases.clone()],
            ),
            (
                "/miniforge-api/releases?per_page=100&page=2".into(),
                vec![Reply::Body(b"[]".to_vec())],
            ),
        ];
        for (tag, asset, sha) in sidecars {
            conda.push((
                format!("/miniforge/releases/download/{tag}/{asset}.sha256"),
                vec![Reply::Body(format!("{sha}  {asset}\n").into_bytes())],
            ));
        }
        let mut routes = vec![
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
            (
                "/pypy/versions.json".into(),
                vec![Reply::Body(pypy_versions_json(
                    pypy_files,
                    &format!("{host}/pypy"),
                ))],
            ),
        ];
        routes.extend(conda);
        routes
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

// allowlist D-79
#[test]
fn update_writes_the_cache_from_listings_and_the_index() {
    let f = Fixture::new();
    let s = fake(ok_listing(), ok_27());
    let r = run(&f, &s.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(
        r.stdout,
        format!("{BANNER}:: [Info] ::  conda: 1 installable versions (0 new)\r\n:: [Info] ::  Scanned 6 pages and found 9 installers.\r\n")
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
            "3.13.0t",
            "pypy3.10-v7.3.99-win64",
            "pypy3.11-v7.3.99-win64"
        ]
    );
    let raw = std::fs::read(f.root.join(".versions_cache.xml")).unwrap();
    assert!(raw.ends_with(b"</versions>") && raw.windows(2).any(|w| w == b"\r\n"));
}

// allowlist D-79
#[test]
fn update_adds_pypy_rows_after_cpython_in_pyenv_wins_format() {
    let f = Fixture::new();
    let s = fake(ok_listing(), ok_27());
    assert_eq!(run(&f, &s.url("/ftp/python"), &["update"]).code, 0);
    let xml = std::fs::read_to_string(f.root.join(".versions_cache.xml")).unwrap();
    assert!(xml.contains("\t<version x64=\"true\" webInstall=\"false\" msi=\"false\">\r\n\t\t<code>pypy3.10-v7.3.99-win64</code>\r\n\t\t<file>pypy3.10-v7.3.99-win64.zip</file>\r\n"), "{xml}");
    assert!(
        xml.contains("\t\t<zipRootDir>pypy3.11-v7.3.99-win64</zipRootDir>\r\n"),
        "{xml}"
    );
}

/// Ruling R3 (the user's, 2026-10-14): a pypy.org outage never blocks the python.org list.
/// The fresh CPython rows are saved, the PyPy rows already in the cache are kept, and the
/// error is printed with a warning; exit 0, with or without `--ignore`.
#[test]
fn a_failed_pypy_listing_keeps_the_cached_pypy_rows_and_saves_cpython() {
    let f = Fixture::new();
    let s = fake(ok_listing(), ok_27());
    let base = s.url("/ftp/python");
    let down = s.url("/nowhere");
    let (tables, conda) = conda_env(&f, &base);
    // No cache yet: CPython rows only.
    let r = f.pyenv_env(
        &["update"],
        &[
            ("RPYENV_TEST_PYTHON_ORG", &base),
            ("RPYENV_TEST_PYPY", &down),
            ("RPYENV_TEST_CONDA", &conda),
            ("RPYENV_TEST_CONDA_TABLES", &tables),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(
        r.stdout.contains(&format!(
            "HTTP Error downloading from mirror \"{down}/versions.json\"\r\n"
        )),
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains(":: [Warning] :: PyPy's list couldn't be read, and no earlier list could be read either; no PyPy versions are listed.\r\n"), "{}", r.stdout);
    assert!(codes(&f).contains(&"3.13.0".to_string()));
    assert!(!codes(&f).iter().any(|c| c.starts_with("pypy")));
    // A good update, then an outage: the PyPy rows survive, CPython is refreshed.
    assert_eq!(run(&f, &base, &["update"]).code, 0);
    let r = f.pyenv_env(
        &["update", "--ignore"],
        &[
            ("RPYENV_TEST_PYTHON_ORG", &base),
            ("RPYENV_TEST_PYPY", &down),
            ("RPYENV_TEST_CONDA", &conda),
            ("RPYENV_TEST_CONDA_TABLES", &tables),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(
        r.stdout
            .contains("kept the 2 PyPy versions already listed."),
        "{}",
        r.stdout
    );
    let c = codes(&f);
    assert!(c.contains(&"3.13.0".to_string()));
    assert_eq!(
        c.iter().filter(|c| c.starts_with("pypy")).count(),
        2,
        "{c:?}"
    );
}

/// A versions.json that parses but gives no win64 zip (pypy.org reshaped it) is a failed read,
/// not an empty list: the cached PyPy rows stay, with a warning (ruling R3).
#[test]
fn a_versions_json_without_win64_zips_keeps_the_cached_pypy_rows() {
    let f = Fixture::new();
    let good = fake(ok_listing(), ok_27());
    assert_eq!(run(&f, &good.url("/ftp/python"), &["update"]).code, 0);
    let before = codes(&f);
    assert_eq!(before.iter().filter(|c| c.starts_with("pypy")).count(), 2);
    let reshaped = fake_pypy(ok_listing(), ok_27(), &["pypy3.11-v7.3.99-linux64.tar.bz2"]);
    let r = run(&f, &reshaped.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(
        r.stdout.contains(
            ":: [Warning] :: PyPy's list couldn't be read; kept the 2 PyPy versions already listed.\r\n"
        ),
        "{}",
        r.stdout
    );
    assert_eq!(codes(&f), before);
}

/// Review focus 5: pyenv-win's banner names only the mirror, so PyPy isn't read.
#[test]
fn a_mirror_replaces_pypy_too() {
    let f = Fixture::new();
    let s = fake(ok_listing(), ok_27());
    let base = s.url("/ftp/python");
    let (tables, conda) = conda_env(&f, &base);
    let r = f.pyenv_env(
        &["update"],
        &[
            ("PYTHON_BUILD_MIRROR_URL", &base),
            ("RPYENV_TEST_PYPY", &s.url("/pypy")),
            ("RPYENV_TEST_CONDA", &conda),
            ("RPYENV_TEST_CONDA_TABLES", &tables),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(s.hits("/pypy/versions.json"), 0);
}

// allowlist D-79
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

// allowlist D-79
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

// allowlist D-79
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

// allowlist D-79
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

/// M7c design §4.3: `update` reads the publishers and adds a release newer than the shipped
/// table's newest, as an `ok` row.
#[test]
fn update_writes_the_conda_table_with_newer_releases() {
    let f = Fixture::new();
    let s = fake_conda(
        ok_listing(),
        ok_27(),
        &["pypy3.11-v7.3.99-win64.zip"],
        releases("27.1.0-0", MF_27),
        &[("27.1.0-0", MF_27, SHA_C)],
    );
    let r = run(&f, &s.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let text = std::fs::read_to_string(f.root.join(".conda_versions.tsv")).unwrap();
    let first = text.lines().next().unwrap();
    assert!(
        first.starts_with("# generated ") && first.ends_with(" by update"),
        "{text}"
    );
    let row = text
        .lines()
        .find(|l| l.starts_with("miniforge3-27.1.0-0\t"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        row.starts_with("miniforge3-27.1.0-0\tWindows-x86_64\tok ")
            && row.contains(&format!("#{SHA_C}")),
        "{row}"
    );
    assert!(
        r.stdout
            .contains(":: [Info] ::  conda: 2 installable versions (1 new)\r\n"),
        "{}",
        r.stdout
    );
}

/// Review focus 3, spec D9: a failed read keeps the previous table whole and costs nothing else.
#[test]
fn a_failed_conda_read_keeps_the_previous_table() {
    let f = Fixture::new();
    // `Reply` has no status-with-body; GitHub's rate-limit object (a 403 in life) is a refused
    // read all the same: it is an object, not a list.
    let s = fake_conda(
        ok_listing(),
        ok_27(),
        &["pypy3.11-v7.3.99-win64.zip"],
        Reply::Body(br#"{"message":"API rate limit exceeded"}"#.to_vec()),
        &[],
    );
    let previous = shipped_table("https://github.com/conda-forge/miniforge", SHA_A)
        .replace("by ci", "by update");
    std::fs::create_dir_all(&f.root).unwrap();
    std::fs::write(f.root.join(".conda_versions.tsv"), &previous).unwrap();
    let r = run(&f, &s.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(f.root.join(".versions_cache.xml").exists());
    assert_eq!(
        std::fs::read(f.root.join(".conda_versions.tsv")).unwrap(),
        previous.as_bytes()
    );
    assert!(r.stdout.contains(":: [Warning] :: conda: "), "{}", r.stdout);
    assert!(r.stdout.contains("kept the previous table"), "{}", r.stdout);
}

/// Spec D8: a hash the publisher changed after rpyenv pinned it stays pinned, and `update`
/// names it. Also: an unreadable table copy is printed as a warning (task 5 note 4).
#[test]
fn a_changed_hash_is_kept_named_and_update_warns() {
    let f = Fixture::new();
    let s = fake_conda(
        ok_listing(),
        ok_27(),
        &["pypy3.11-v7.3.99-win64.zip"],
        releases("26.7.2-0", MF_26),
        &[("26.7.2-0", MF_26, SHA_B)],
    );
    let dir = f.base.join("conda-tables");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("windows.tsv"),
        shipped_table(&s.url("/miniforge"), SHA_A),
    )
    .unwrap();
    // A root copy without its `# generated` line: ignored with a warning, then replaced.
    std::fs::create_dir_all(&f.root).unwrap();
    std::fs::write(f.root.join(".conda_versions.tsv"), "garbage\n").unwrap();
    let r = run(&f, &s.url("/ftp/python"), &["update"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let text = std::fs::read_to_string(f.root.join(".conda_versions.tsv")).unwrap();
    let row = text
        .lines()
        .find(|l| l.starts_with("miniforge3-26.7.2-0\t"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        row.contains(&format!("#{SHA_A}")) && !row.contains(SHA_B),
        "{row}"
    );
    assert!(
        r.stdout.contains(":: [Warning] :: conda: the publisher changed miniforge3-26.7.2-0 after rpyenv pinned them"),
        "{}",
        r.stdout
    );
    assert!(
        r.stdout.contains(":: [Warning] :: conda: rpyenv: ignored "),
        "{}",
        r.stdout
    );
}
