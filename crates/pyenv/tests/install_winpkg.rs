//! The Windows install engine against the tier-1 fake python.org (plan M2b Task 7; review focus
//! 1 and 3). The MSI cases install python.org's real tools.msi, verified with the real
//! signature; the zip cases install a small zip whose SHA-256 the fake index publishes.

mod common;
use common::server::{start_with, Reply, Server};
use common::winfake::{index_json, listing, sha256, zip_bytes};
use pyenv::install::fetch::Fetcher;
use pyenv::install::txn::Txn;
use pyenv::install::wincatalog::parse_code;
use pyenv::install::winpkg::{install, resolve, skip_component, Done, Job, Package};
use pyenv::install::InstallError;
use rpyenv_core::flavor::Flavor;
use std::path::{Path, PathBuf};

fn fx(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/msi")
            .join(name),
    )
    .unwrap()
}

fn tools() -> Vec<u8> {
    fx("tools-3.10.11-amd64.msi")
}

fn tools_asc() -> Vec<u8> {
    fx("tools-3.10.11-amd64.msi.asc")
}

/// python.org with 3.10.11 amd64 offering only tools.msi (and the given asc reply), 3.12.1 as
/// an index zip, and 2.7.18 as a single MSI (served as tools.msi's bytes, so it is unsigned
/// unless a signature is listed).
fn python_org(msi: Vec<u8>, asc: Option<Reply>, zip: Vec<u8>, zip_sha: String) -> Server {
    start_with(move |host| {
        let base = format!("{host}/ftp/python");
        let mut names = vec!["tools.msi", "tools_d.msi", "path.msi"];
        if asc.is_some() {
            names.push("tools.msi.asc");
        }
        vec![
            (
                "/ftp/python/3.10.11/amd64/".into(),
                vec![Reply::Body(listing(&names))],
            ),
            (
                "/ftp/python/3.10.11/amd64/tools.msi".into(),
                vec![Reply::Body(msi.clone())],
            ),
            (
                "/ftp/python/3.10.11/amd64/tools.msi.asc".into(),
                vec![asc.clone().unwrap_or(Reply::Status(404))],
            ),
            (
                "/ftp/python/index-windows.json".into(),
                vec![Reply::Body(index_json(
                    &[(&format!("{base}/3.12.1/python-3.12.1-amd64.zip"), &zip_sha)],
                    None,
                ))],
            ),
            (
                "/ftp/python/3.12.1/python-3.12.1-amd64.zip".into(),
                vec![Reply::Body(zip.clone())],
            ),
            (
                "/ftp/python/2.7.18/".into(),
                vec![Reply::Body(listing(&["python-2.7.18.amd64.msi"]))],
            ),
            (
                "/ftp/python/2.7.18/python-2.7.18.amd64.msi".into(),
                vec![Reply::Body(msi.clone())],
            ),
        ]
    })
}

fn small_zip() -> Vec<u8> {
    zip_bytes(&[
        ("python.exe", b"MZ-python"),
        ("pythonw.exe", b"MZ-pythonw"),
        ("Lib/os.py", b"# os"),
        ("Lib/venv/scripts/nt/python.exe", b"MZ-venvlauncher"),
    ])
}

fn fetcher() -> Fetcher {
    let mut f = Fetcher::direct();
    f.retry_delay = std::time::Duration::ZERO;
    f
}

struct Run {
    root: tempfile::TempDir,
    lines: Vec<String>,
    result: Result<Done, InstallError>,
}

fn run(s: &Server, code: &str, force: bool, root: Option<tempfile::TempDir>) -> Run {
    let root = root.unwrap_or_else(|| tempfile::tempdir().unwrap());
    let c = parse_code(code).unwrap();
    let f = fetcher();
    let base = s.url("/ftp/python");
    let job = Job {
        root: root.path(),
        code: &c,
        force,
        base: &base,
        fetcher: &f,
    };
    let mut lines = Vec::new();
    let result = install(&job, &mut |l| lines.push(l.to_string()));
    Run {
        root,
        lines,
        result,
    }
}

fn version(r: &Run, code: &str) -> PathBuf {
    r.root.path().join("versions").join(code)
}

fn leftovers(r: &Run) -> Vec<String> {
    std::fs::read_dir(r.root.path().join("versions"))
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

// allowlist D-73
#[test]
fn components_skip_debug_path_launcher_pip_and_free_threaded() {
    for skip in [
        "core_d.msi",
        "lib_pdb.msi",
        "appendpath.msi",
        "launcher.msi",
        "path.msi",
        "pip.msi",
        "freethreaded.msi",
        "freethreaded_d.msi",
    ] {
        assert!(skip_component(skip), "{skip}");
    }
    for keep in [
        "core.msi",
        "exe.msi",
        "lib.msi",
        "dev.msi",
        "tcltk.msi",
        "test.msi",
        "tools.msi",
        "doc.msi",
        "ucrt.msi",
    ] {
        assert!(!skip_component(keep), "{keep}");
    }
}

// allowlist D-73
#[test]
fn resolve_picks_the_zip_the_components_or_the_single_msi() {
    let zip = small_zip();
    let s = python_org(
        tools(),
        Some(Reply::Body(tools_asc())),
        zip.clone(),
        sha256(&zip),
    );
    let base = s.url("/ftp/python");
    let f = fetcher();
    match resolve(&parse_code("3.12.1").unwrap(), &base, &f).unwrap() {
        Package::Zip {
            file, sha256: h, ..
        } => assert_eq!(
            (file.as_str(), h),
            ("python-3.12.1-amd64.zip", sha256(&zip))
        ),
        p => panic!("{p:?}"),
    }
    match resolve(&parse_code("3.10.11").unwrap(), &base, &f).unwrap() {
        Package::Components { msis, .. } => {
            let names: Vec<&str> = msis.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, ["tools.msi"]);
            assert_eq!(
                msis[0].asc.as_deref(),
                Some(format!("{base}/3.10.11/amd64/tools.msi.asc").as_str())
            );
        }
        p => panic!("{p:?}"),
    }
    assert!(
        matches!(resolve(&parse_code("2.7.18").unwrap(), &base, &f).unwrap(), Package::SingleMsi(r) if r.asc.is_none())
    );
    let e = resolve(&parse_code("3.10.11-arm").unwrap(), &base, &f).unwrap_err();
    assert_eq!(e, "python.org has no Windows installer for 3.10.11-arm");
    let e = resolve(&parse_code("3.10.11t").unwrap(), &base, &f).unwrap_err();
    assert_eq!(e, "python.org has no free-threaded package for 3.10.11t");
}

// allowlist D-73
// allowlist D-85
#[test]
fn a_signed_msi_installs_exactly_its_files_and_is_cached() {
    let s = python_org(
        tools(),
        Some(Reply::Body(tools_asc())),
        small_zip(),
        "0".repeat(64),
    );
    let r = run(&s, "3.10.11", false, None);
    assert!(
        matches!(r.result, Ok(Done::Installed)),
        "{:?} {:?}",
        r.result,
        r.lines
    );
    let v = version(&r, "3.10.11");
    assert!(v.join("Tools").join("demo").join("beer.py").is_file());
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert_eq!(leftovers(&r), ["3.10.11"]);
    let base = s.url("/ftp/python");
    let cache = r.root.path().join("install_cache").join("3.10.11");
    assert_eq!(
        r.lines,
        [
            ":: [Downloading] ::  3.10.11 ...".to_string(),
            format!(":: [Downloading] ::  From {base}/3.10.11/amd64/"),
            format!(":: [Downloading] ::  To   {}", cache.display()),
            ":: [Installing] ::  3.10.11 ...".to_string(),
            ":: [Info] :: completed! 3.10.11".to_string(),
        ]
    );
    assert!(cache.join("tools.msi").is_file() && cache.join("tools.msi.asc").is_file());
    assert_eq!(
        s.hits("/ftp/python/3.10.11/amd64/tools_d.msi"),
        0,
        "debug MSIs are never fetched"
    );
    // Again with -f: the cached MSI is verified against a fresh signature and reused.
    let again = run(&s, "3.10.11", true, Some(r.root));
    assert!(
        matches!(again.result, Ok(Done::Installed)),
        "{:?}",
        again.lines
    );
    assert_eq!(
        s.hits("/ftp/python/3.10.11/amd64/tools.msi"),
        1,
        "the MSI was downloaded once"
    );
    assert_eq!(
        s.hits("/ftp/python/3.10.11/amd64/tools.msi.asc"),
        2,
        "the signature is fetched every time"
    );
    assert!(
        !again.lines.iter().any(|l| l.contains("[Downloading]")),
        "{:?}",
        again.lines
    );
}

#[test]
fn a_tampered_cached_msi_is_downloaded_again() {
    let s = python_org(
        tools(),
        Some(Reply::Body(tools_asc())),
        small_zip(),
        "0".repeat(64),
    );
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("install_cache").join("3.10.11");
    std::fs::create_dir_all(&cache).unwrap();
    let mut bad = tools();
    bad[0x8000] ^= 1;
    std::fs::write(cache.join("tools.msi"), bad).unwrap();
    let r = run(&s, "3.10.11", false, Some(root));
    assert!(
        matches!(r.result, Ok(Done::Installed)),
        "{:?} {:?}",
        r.result,
        r.lines
    );
    assert_eq!(
        s.hits("/ftp/python/3.10.11/amd64/tools.msi"),
        1,
        "the tampered copy was replaced"
    );
    assert_eq!(std::fs::read(cache.join("tools.msi")).unwrap(), tools());
}

#[test]
fn an_installed_version_is_skipped_silently_without_force() {
    let s = python_org(
        tools(),
        Some(Reply::Body(tools_asc())),
        small_zip(),
        "0".repeat(64),
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("versions").join("3.10.11")).unwrap();
    let r = run(&s, "3.10.11", false, Some(root));
    assert!(matches!(r.result, Ok(Done::Skipped)));
    assert!(r.lines.is_empty());
    assert_eq!(s.hits("/ftp/python/3.10.11/amd64/"), 0);
}

// allowlist D-75
#[test]
fn a_tampered_msi_is_refused_leaves_nothing_and_is_not_cached() {
    let mut bad = tools();
    bad[0x8000] ^= 1;
    let s = python_org(
        bad,
        Some(Reply::Body(tools_asc())),
        small_zip(),
        "0".repeat(64),
    );
    let r = run(&s, "3.10.11", false, None);
    match &r.result {
        Err(InstallError::Message(m)) => assert!(
            m.starts_with("signature check failed for tools.msi: bad signature"),
            "{m}"
        ),
        other => panic!("{other:?}"),
    }
    assert!(leftovers(&r).is_empty(), "{:?}", leftovers(&r));
    assert!(!r
        .root
        .path()
        .join("install_cache")
        .join("3.10.11")
        .join("tools.msi")
        .exists());
}

#[test]
fn a_listed_signature_that_cannot_be_fetched_never_downgrades_to_unsigned() {
    let s = python_org(
        tools(),
        Some(Reply::Status(500)),
        small_zip(),
        "0".repeat(64),
    );
    let r = run(&s, "3.10.11", false, None);
    match &r.result {
        Err(InstallError::Message(m)) => {
            assert!(m.starts_with("cannot download tools.msi.asc"), "{m}")
        }
        other => panic!("{other:?}"),
    }
    assert!(!r.lines.iter().any(|l| l.contains("[Warning]")));
    assert!(leftovers(&r).is_empty());
}

// allowlist D-78
#[test]
fn an_unsigned_msi_installs_with_one_warning() {
    let s = python_org(tools(), None, small_zip(), "0".repeat(64));
    let r = run(&s, "2.7.18", false, None);
    assert!(
        matches!(r.result, Ok(Done::Installed)),
        "{:?} {:?}",
        r.result,
        r.lines
    );
    let warnings: Vec<&String> = r.lines.iter().filter(|l| l.contains("[Warning]")).collect();
    assert_eq!(warnings, [":: [Warning] :: python.org publishes no signature for python-2.7.18.amd64.msi; checked only by HTTPS."]);
}

#[test]
fn a_zip_installs_with_its_copies_and_a_bad_hash_is_refused() {
    let zip = small_zip();
    let s = python_org(tools(), None, zip.clone(), sha256(&zip));
    let r = run(&s, "3.12.1", false, None);
    assert!(
        matches!(r.result, Ok(Done::Installed)),
        "{:?} {:?}",
        r.result,
        r.lines
    );
    let v = version(&r, "3.12.1");
    let cases: [(&str, &[u8]); 5] = [
        ("python3.exe", b"MZ-python"),
        ("python312.exe", b"MZ-python"),
        ("python3.12.exe", b"MZ-python"),
        ("pythonw3.exe", b"MZ-pythonw"),
        ("pythonw3.12.exe", b"MZ-pythonw"),
    ];
    for (name, body) in cases {
        assert_eq!(std::fs::read(v.join(name)).unwrap(), body, "{name}");
    }
    let nt = v.join("Lib").join("venv").join("scripts").join("nt");
    for name in [
        "python3.exe",
        "python312.exe",
        "python3.12.exe",
        "pythonw3.exe",
        "pythonw312.exe",
        "pythonw3.12.exe",
    ] {
        assert_eq!(
            std::fs::read(nt.join(name)).unwrap(),
            b"MZ-venvlauncher",
            "{name}"
        );
    }
    assert!(r
        .root
        .path()
        .join("install_cache")
        .join("python-3.12.1-amd64.zip")
        .is_file());

    let s = python_org(tools(), None, zip.clone(), "0".repeat(64));
    let r = run(&s, "3.12.1", false, None);
    match &r.result {
        Err(InstallError::Message(m)) => assert!(m.contains("checksum mismatch"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(leftovers(&r).is_empty());
}

/// Free-threaded zips have no `python.exe` (measured on 3.13.0t): it is a copy of
/// `python3.13t.exe`, and the usual copies follow from it.
// allowlist D-84
#[test]
fn a_free_threaded_zip_gets_python_exe_from_python3_13t_exe() {
    let zip = zip_bytes(&[
        ("python3.13t.exe", b"MZ-t"),
        ("pythonw3.13t.exe", b"MZ-tw"),
        ("Lib/os.py", b""),
    ]);
    let sha = sha256(&zip);
    let s = start_with(move |host| {
        let base = format!("{host}/ftp/python");
        vec![
            (
                "/ftp/python/index-windows.json".into(),
                vec![Reply::Body(index_json(
                    &[(&format!("{base}/3.13.0/python-3.13.0t-amd64.zip"), &sha)],
                    None,
                ))],
            ),
            (
                "/ftp/python/3.13.0/python-3.13.0t-amd64.zip".into(),
                vec![Reply::Body(zip.clone())],
            ),
        ]
    });
    let r = run(&s, "3.13.0t", false, None);
    assert!(
        matches!(r.result, Ok(Done::Installed)),
        "{:?} {:?}",
        r.result,
        r.lines
    );
    let v = version(&r, "3.13.0t");
    let cases: [(&str, &[u8]); 4] = [
        ("python.exe", b"MZ-t"),
        ("python3.13.exe", b"MZ-t"),
        ("pythonw.exe", b"MZ-tw"),
        ("pythonw313.exe", b"MZ-tw"),
    ];
    for (name, body) in cases {
        assert_eq!(std::fs::read(v.join(name)).unwrap(), body, "{name}");
    }
}

#[test]
fn a_tampered_cached_zip_is_downloaded_again() {
    let zip = small_zip();
    let s = python_org(tools(), None, zip.clone(), sha256(&zip));
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("install_cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("python-3.12.1-amd64.zip"), b"tampered").unwrap();
    let r = run(&s, "3.12.1", false, Some(root));
    assert!(matches!(r.result, Ok(Done::Installed)), "{:?}", r.lines);
    assert_eq!(s.hits("/ftp/python/3.12.1/python-3.12.1-amd64.zip"), 1);
}

/// Ruling R4, the positive half: a single MSI may be signed by any pinned key, Dower's
/// included (tools.msi's signature is his). Components accepting only Dower is unit-tested
/// in `winpkg::tests`, since no real component MSI is signed by another key.
#[test]
fn a_signed_single_msi_installs() {
    let s = start_with(|_| {
        vec![
            (
                "/ftp/python/2.7.18/".into(),
                vec![Reply::Body(listing(&[
                    "python-2.7.18.amd64.msi",
                    "python-2.7.18.amd64.msi.asc",
                ]))],
            ),
            (
                "/ftp/python/2.7.18/python-2.7.18.amd64.msi".into(),
                vec![Reply::Body(tools())],
            ),
            (
                "/ftp/python/2.7.18/python-2.7.18.amd64.msi.asc".into(),
                vec![Reply::Body(tools_asc())],
            ),
        ]
    });
    let r = run(&s, "2.7.18", false, None);
    assert!(
        matches!(r.result, Ok(Done::Installed)),
        "{:?} {:?}",
        r.result,
        r.lines
    );
    assert!(
        !r.lines.iter().any(|l| l.contains("[Warning]")),
        "{:?}",
        r.lines
    );
    assert!(version(&r, "2.7.18")
        .join("Tools")
        .join("demo")
        .join("beer.py")
        .is_file());
    assert_eq!(s.hits("/ftp/python/2.7.18/python-2.7.18.amd64.msi.asc"), 1);
}

/// A failure after the transaction began, in extraction (before the move) or in `finish`
/// (after it), leaves neither a staging folder nor `versions\<code>`.
#[test]
fn a_failure_after_the_transaction_began_leaves_nothing() {
    type Entries<'a> = &'a [(&'a str, &'a [u8])];
    let cases: [(Entries, &str); 2] = [
        // Extraction writes python.exe, then refuses a device name.
        (
            &[("python.exe", b"MZ-python"), ("Lib/CON", b"")],
            "unsafe path in ",
        ),
        // Extraction succeeds; copying python.exe onto the directory python3.exe fails.
        (
            &[("python.exe", b"MZ-python"), ("python3.exe/x", b"")],
            "cannot copy ",
        ),
    ];
    for (entries, why) in cases {
        let zip = zip_bytes(entries);
        let s = python_org(tools(), None, zip.clone(), sha256(&zip));
        let r = run(&s, "3.12.1", false, None);
        match &r.result {
            Err(InstallError::Message(m)) => assert!(m.starts_with(why), "{m}"),
            other => panic!("{other:?} {:?}", r.lines),
        }
        assert!(
            r.lines.iter().any(|l| l.contains("[Installing]")),
            "the transaction began: {:?}",
            r.lines
        );
        assert!(r.root.path().join("versions").is_dir());
        assert!(leftovers(&r).is_empty(), "{:?}", leftovers(&r));
    }
}

/// Fix round 1, I1: the version lock is taken before anything is downloaded, so a second
/// install of the same code can't rename bytes under the cache name the first one verified.
#[test]
fn a_locked_version_is_refused_before_any_download() {
    let s = python_org(
        tools(),
        Some(Reply::Body(tools_asc())),
        small_zip(),
        "0".repeat(64),
    );
    let root = tempfile::tempdir().unwrap();
    let held = Txn::begin_for(&root.path().join("versions"), "3.10.11", Flavor::PyenvWin).unwrap();
    let r = run(&s, "3.10.11", false, Some(root));
    match &r.result {
        Err(InstallError::Message(m)) => {
            assert!(
                m.contains("another install of 3.10.11 is in progress"),
                "{m}"
            )
        }
        other => panic!("{other:?} {:?}", r.lines),
    }
    assert!(r.lines.is_empty(), "{:?}", r.lines);
    for path in [
        "/ftp/python/3.10.11/amd64/",
        "/ftp/python/3.10.11/amd64/tools.msi",
        "/ftp/python/3.10.11/amd64/tools.msi.asc",
    ] {
        assert_eq!(s.hits(path), 0, "{path}");
    }
    assert!(!r.root.path().join("install_cache").exists());
    drop(held);
}
