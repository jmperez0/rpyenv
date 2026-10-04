//! python-build definitions: listing order, lookup and the interpreter (plan Decision 3).

use pyenv::install::defs::{self, Fetch, Found, Origin};
use std::path::Path;

fn no_env(_: &str) -> Option<String> {
    None
}

fn builtin(name: &str) -> Found {
    defs::find(Path::new("/nonexistent-root"), name, &no_env).unwrap()
}

fn parse_builtin(name: &str) -> defs::Definition {
    let root = Path::new("/nonexistent-root");
    defs::parse(&builtin(name), &no_env, &|n| defs::find(root, n, &no_env)).unwrap()
}

#[test]
fn every_vendored_definition_parses() {
    // Mechanical: every vendored definition, counted from the folder itself so a sync
    // keeps the check, and a construct the interpreter misses fails here, by name.
    let root = Path::new("/nonexistent-root");
    let names = defs::names(root, &no_env);
    let share = Path::new(env!("CARGO_MANIFEST_DIR")).join("python-build/share");
    let files = std::fs::read_dir(&share)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_type().unwrap().is_file())
        .count();
    assert_eq!(names.len(), files);
    let mut failures = Vec::new();
    for n in &names {
        let found = defs::find(root, n, &no_env).unwrap();
        match defs::parse(&found, &no_env, &|s| defs::find(root, s, &no_env)) {
            Ok(d) => {
                let python = d
                    .packages
                    .iter()
                    .filter(|p| p.name.starts_with("Python-"))
                    .count();
                if python != 1 {
                    failures.push(format!("{n}: {python} Python packages"));
                }
            }
            Err(e) => failures.push(format!("{n}: {e}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_current_definition_takes_the_xz_branch_and_skips_nothing_on_its_own() {
    let d = parse_builtin("3.12.10");
    let py = d
        .packages
        .iter()
        .find(|p| p.name == "Python-3.12.10")
        .unwrap();
    match &py.fetch {
        Fetch::Tarball { url, fragment } => {
            assert_eq!(
                url,
                "https://www.python.org/ftp/python/3.12.10/Python-3.12.10.tar.xz"
            );
            assert_eq!(
                fragment.as_deref(),
                Some("07ab697474595e06f06647417d3c7fa97ded07afc1a7e4454c5639919b46eaea")
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        py.steps,
        ["standard", "verify_py312", "copy_python_gdb", "ensurepip"]
    );
    let openssl = d
        .packages
        .iter()
        .find(|p| p.name.starts_with("openssl-"))
        .unwrap();
    assert_eq!(openssl.condition.as_deref(), Some("has_broken_mac_openssl"));
}

#[test]
fn a_free_threaded_definition_sources_its_twin_with_the_flag_set() {
    let d = parse_builtin("3.13.3t");
    assert!(d
        .vars
        .contains(&("PYTHON_BUILD_FREE_THREADING".into(), "1".into())));
    assert!(d.packages.iter().any(|p| p.name == "Python-3.13.3"));
}

#[test]
fn cflags_exports_expand_like_bash() {
    let d = parse_builtin("3.8.20");
    assert!(
        d.vars
            .contains(&("PYTHON_CFLAGS".into(), "-DOPENSSL_NO_SSL3".into())),
        "{:?}",
        d.vars
    );
    let with_env = |k: &str| (k == "PYTHON_CFLAGS").then(|| "-O1".to_string());
    let root = Path::new("/nonexistent-root");
    let d2 = defs::parse(&builtin("2.7.18"), &with_env, &|n| {
        defs::find(root, n, &no_env)
    })
    .unwrap();
    assert!(
        d2.vars
            .contains(&("PYTHON_CFLAGS".into(), "-O1 -std=c99".into())),
        "{:?}",
        d2.vars
    );
}

#[test]
fn the_darwin_block_of_3_0_1_is_skipped_and_its_else_branch_kept() {
    let d = parse_builtin("3.0.1");
    let names: Vec<&str> = d.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "openssl-1.0.2k",
            "readline-8.0",
            "Python-3.0.1",
            "setuptools-1.4.2",
            "pip-1.3.1"
        ]
    );
}

#[test]
fn the_continued_src_assignment_of_3_4_10_picks_xz() {
    let d = parse_builtin("3.4.10");
    let py = d
        .packages
        .iter()
        .find(|p| p.name == "Python-3.4.10")
        .unwrap();
    assert!(
        matches!(&py.fetch, Fetch::Tarball { url, .. } if url.ends_with("Python-3.4.10.tar.xz"))
    );
}

#[test]
fn dev_definitions_clone_with_git() {
    let d = parse_builtin("3.14-dev");
    let py = d
        .packages
        .iter()
        .find(|p| p.name == "Python-3.14-dev")
        .unwrap();
    assert_eq!(
        py.fetch,
        Fetch::Git {
            url: "https://github.com/python/cpython".into(),
            reference: "3.14".into()
        }
    );
}

#[test]
fn old_definitions_require_gcc() {
    assert!(parse_builtin("2.4.6").require_gcc);
    assert!(!parse_builtin("2.7.18").require_gcc);
}

// allowlist D-56
#[test]
fn other_bash_is_refused_with_its_line() {
    let f = Found {
        name: "x".into(),
        text: "install_package a b\nfor i in 1; do :; done\n".into(),
        origin: Origin::Path("/x".into()),
    };
    let err = defs::parse(&f, &no_env, &|_| None).unwrap_err();
    assert_eq!(
        err,
        "x: line 2: rpyenv cannot interpret this definition line: for i in 1; do :; done"
    );
}

/// A package name becomes `<build dir>/<name>`, which is extracted into and deleted
/// (review I2): it must be one plain path component.
#[test]
fn a_package_name_that_is_not_one_plain_component_is_refused() {
    for bad in ["../x", "/abs", "a/b", r"a\b", ".", "..", ""] {
        for line in [
            format!("install_package \"{bad}\" \"https://example.invalid/x.tgz\" standard"),
            format!("install_git \"{bad}\" \"https://example.invalid/x.git\" main standard"),
        ] {
            let f = Found {
                name: "3.99.0".into(),
                text: format!("# comment\n{line}\n"),
                origin: Origin::Path("/x".into()),
            };
            let err = defs::parse(&f, &no_env, &|_| None).unwrap_err();
            assert_eq!(
                err,
                format!("3.99.0: line 2: invalid package name: {bad}"),
                "{line}"
            );
        }
    }
    let ok = Found {
        name: "3.99.0".into(),
        text: "install_package \"Python-3.99.0\" \"https://example.invalid/x.tgz\"\n".into(),
        origin: Origin::Path("/x".into()),
    };
    assert!(defs::parse(&ok, &no_env, &|_| None).is_ok());
}

#[test]
fn the_listing_sorts_like_python_build() {
    let mut v: Vec<String> = [
        "3.12.1",
        "3.12-dev",
        "3.12.0",
        "2.7-dev",
        "2.7",
        "3.13.0t",
        "3.13.0",
        "3.13-dev",
        "3.13t-dev",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    defs::sort_versions(&mut v);
    assert_eq!(
        v,
        [
            "2.7-dev",
            "2.7",
            "3.12.0",
            "3.12-dev",
            "3.12.1",
            "3.13.0",
            "3.13.0t",
            "3.13-dev",
            "3.13t-dev"
        ]
    );
}

#[test]
fn plugin_definitions_are_listed_and_found_first() {
    let root = tempfile::tempdir().unwrap();
    let d = root.path().join("plugins/fake/share/python-build");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("3.12.99"),
        "install_package \"Python-3.12.99\" \"http://x/P.tgz#0\" standard\n",
    )
    .unwrap();
    std::fs::write(d.join("3.12.10"), "plugin copy\n").unwrap();
    let names = defs::names(root.path(), &no_env);
    assert!(names.contains(&"3.12.99".to_string()));
    assert_eq!(
        names.iter().filter(|n| *n == "3.12.10").count(),
        1,
        "adjacent duplicates removed"
    );
    let found = defs::find(root.path(), "3.12.10", &no_env).unwrap();
    assert_eq!(found.text, "plugin copy\n");
    assert!(matches!(found.origin, Origin::Dir(_)));
}

#[test]
fn a_definition_file_path_is_used_as_is() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("my-def");
    std::fs::write(&p, "require_gcc\n").unwrap();
    let found = defs::find(Path::new("/nonexistent-root"), p.to_str().unwrap(), &no_env).unwrap();
    assert_eq!(found.name, "my-def");
}

#[test]
fn built_in_patches_come_sorted() {
    let p = defs::patches_for("3.12.14", "Python-3.12.14");
    assert!(!p.is_empty());
    let names: Vec<&str> = p.iter().map(|(n, _)| n.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(defs::patches_for("3.12.14", "readline-8.2").is_empty());
}
