//! Tier-1 source-build tests (spec §12.5) with a fake CPython and a local server.
#![cfg(unix)]

mod common;

use common::buildharness::{build, opts, Built};
use pyenv::install::builder::Options;
use pyenv::install::txn::is_complete;
use pyenv::install::InstallError;
use std::path::Path;

fn config(b: &Built) -> String {
    std::fs::read_to_string(b.root.join("versions/3.12.99/lib/rpyenv-config.txt")).unwrap()
}

// allowlist D-67
#[test]
fn a_standard_build_installs_with_python_builds_flags_and_messages() {
    let b = build(
        "standard verify_py312 copy_python_gdb ensurepip",
        &[],
        opts(),
        true,
        false,
    );
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let v = b.root.join("versions/3.12.99");
    assert!(is_complete(&v));
    let p = v.display().to_string();
    let c = config(&b);
    assert!(
        c.starts_with(&format!(
            "args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no\n"
        )),
        "{c}"
    );
    assert!(
        c.contains(&format!("LDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib\n")),
        "{c}"
    );
    assert!(
        c.contains(&format!("LIBS=-L{p}/lib -Wl,-rpath,{p}/lib\n")),
        "{c}"
    );
    assert!(c.contains(&format!("CPPFLAGS=-I{p}/include\n")), "{c}");
    assert!(c.contains("CFLAGS_SET=\n"), "CFLAGS left unset: {c}");
    assert_eq!(
        &b.said[..],
        &[
            "Downloading Python-3.12.99.tar.gz...".to_string(),
            b.said[1].clone(),
            "Installing Python-3.12.99...".to_string(),
            format!("Installed Python-3.12.99 to {p}")
        ]
    );
    // Version-suffix symlinks, pip from ensurepip at the final prefix, gdb helper.
    assert_eq!(
        std::fs::read_link(v.join("bin/python")).unwrap(),
        Path::new("python3.12")
    );
    assert_eq!(
        std::fs::read_link(v.join("bin/pip")).unwrap(),
        Path::new("pip3.12")
    );
    assert_eq!(
        std::fs::read_to_string(v.join("bin/pip3.12")).unwrap(),
        format!("#!{p}/bin/python3.12\n")
    );
    assert!(v.join("bin/python3.12-gdb.py").is_file());
    // The build tree is gone; the log stays, as upstream's does.
    let left: Vec<String> = std::fs::read_dir(&b.tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(
        left[0].starts_with("python-build.") && left[0].ends_with(".log"),
        "{left:?}"
    );
}

// allowlist D-60
#[test]
fn built_in_patches_are_applied_and_their_output_stays_in_the_log() {
    let b = build(
        "standard",
        &[],
        Options {
            keep: true,
            ..opts()
        },
        true,
        false,
    );
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let src = std::fs::read_dir(&b.tmp)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(src.join("Python-3.12.99/README")).unwrap(),
        "patched\n"
    );
    assert!(!b.said.iter().any(|s| s.contains("patching file")));
    let tmp_files: Vec<String> = std::fs::read_dir(&b.tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !tmp_files.iter().any(|n| n.starts_with("python-patch.")),
        "{tmp_files:?}"
    );
}

// allowlist D-70
#[test]
fn user_flags_combine_as_python_build_does() {
    let vars = [
        ("CONFIGURE_OPTS", "--global-opt"),
        ("PYTHON_CONFIGURE_OPTS", "--with-foo --enable-optimizations"),
        ("CFLAGS", "-gcflag"),
        ("PYTHON_CFLAGS", "-pycflag"),
        ("CPPFLAGS", "-gcpp"),
        ("PYTHON_CPPFLAGS", "-pycpp"),
        ("LDFLAGS", "-gld"),
        ("PYTHON_LDFLAGS", "-pyld"),
        ("MAKEOPTS", "-j7"),
        ("MAKE_OPTS", "-j3"),
    ];
    let b = build("standard", &vars, opts(), false, false);
    assert_eq!(b.result, Ok(()), "{:?}", b.said);
    let p = b.root.join("versions/3.12.99").display().to_string();
    let c = config(&b);
    assert!(c.starts_with(&format!("args: --prefix={p} --enable-shared --libdir={p}/lib --with-ensurepip=no --global-opt --with-foo --enable-optimizations\n")), "{c}");
    assert!(c.contains("CFLAGS=-gcflag -pycflag\n"), "{c}");
    assert!(
        c.contains(&format!("CPPFLAGS=-I{p}/include -gcpp -pycpp\n")),
        "{c}"
    );
    assert!(
        c.contains(&format!(
            "LDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib -gld -pyld\n"
        )),
        "{c}"
    );
    // make (and every later child) sees the exported prefix flags; `CFLAGS` and the
    // `PYTHON_` variants are configure's only (python-build:1575-1578,2847-2848).
    let m = std::fs::read_to_string(b.root.join("versions/3.12.99/lib/rpyenv-make.txt")).unwrap();
    // D-70 (measured against upstream python-build, 2026-10-03): with both `MAKEOPTS=-j7` and
    // `MAKE_OPTS=-j3` exported, make runs with `-j7` on both sides, but upstream's
    // `MAKE_OPTS="$MAKEOPTS"` assignment makes its children see `MAKE_OPTS=-j7`; rpyenv
    // passes `MAKE_OPTS=-j3` as given.
    let kept: String = m
        .lines()
        .filter(|l| !l.starts_with("MAKE"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(
        kept,
        format!(
            "CFLAGS=-gcflag\nCPPFLAGS=-I{p}/include -gcpp\nLDFLAGS=-L{p}/lib -Wl,-rpath,{p}/lib -gld\nLIBS=-L{p}/lib -Wl,-rpath,{p}/lib\n"
        )
    );
    assert!(
        m.contains("MAKE_OPTS=-j3\n") && m.contains("MAKEOPTS=-j7\n"),
        "{m}"
    );
    let jobs =
        std::fs::read_to_string(b.root.join("versions/3.12.99/lib/rpyenv-make-jobs.txt")).unwrap();
    assert_eq!(jobs.trim(), "-j7");
}

// A user's own `--with-ensurepip` option replaces rpyenv's `--with-ensurepip=no`.
// allowlist D-67
#[test]
fn a_users_with_ensurepip_option_replaces_the_default() {
    let b = build(
        "standard",
        &[("PYTHON_CONFIGURE_OPTS", "--with-ensurepip=install")],
        opts(),
        false,
        false,
    );
    let c = config(&b);
    assert!(c.contains(" --with-ensurepip=install"), "{c}");
    assert!(!c.contains("--with-ensurepip=no"), "{c}");
}

#[test]
fn disable_shared_drops_shared_and_the_rpath() {
    let b = build(
        "standard",
        &[("PYTHON_CONFIGURE_OPTS", "--disable-shared")],
        opts(),
        false,
        false,
    );
    let p = b.root.join("versions/3.12.99").display().to_string();
    let c = config(&b);
    assert!(
        c.starts_with(&format!(
            "args: --prefix={p} --libdir={p}/lib --with-ensurepip=no --disable-shared\n"
        )),
        "{c}"
    );
    assert!(c.contains(&format!("LDFLAGS=-L{p}/lib\n")), "{c}");
}

#[test]
fn a_missing_optional_module_warns_and_a_missing_ssl_fails_and_rolls_back() {
    let b = build(
        "standard verify_py312",
        &[("FAKE_PY_MISSING", "bz2")],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Ok(()));
    let b = build(
        "standard verify_py312 ensurepip",
        &[("FAKE_PY_MISSING", "bz2 ssl")],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(!b.root.join("versions/3.12.99").exists());
    let names: Vec<String> = std::fs::read_dir(b.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.is_empty(), "{names:?}");
    let all = b.said.join("\n");
    assert!(all.contains("BUILD FAILED ("), "{all}");
    assert!(all.contains(" using rpyenv "), "{all}");
}

#[test]
fn a_failed_build_over_an_existing_version_restores_it() {
    let b = build(
        "standard",
        &[("FAKE_CONFIGURE_FAIL", "1")],
        opts(),
        false,
        true,
    );
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(
        b.root.join("versions/3.12.99/bin/old").is_file(),
        "previous version restored"
    );
    let all = b.said.join("\n");
    assert!(
        all.contains("Inspect or clean up the working tree at "),
        "{all}"
    );
    assert!(all.contains("Last 10 log lines:"), "{all}");
    assert!(all.contains("no acceptable C compiler found"), "{all}");
    assert!(
        all.contains("Are the build dependencies for Python correctly installed?"),
        "{all}"
    );
}

// allowlist D-64
#[test]
fn a_failed_ensurepip_does_not_fall_back_to_get_pip() {
    let b = build(
        "standard ensurepip",
        &[("FAKE_PY_NO_PIP", "1")],
        opts(),
        false,
        false,
    );
    assert_eq!(b.result, Err(InstallError::Failed));
    assert!(
        b.said
            .iter()
            .any(|s| s == "error: failed to install pip via ensurepip"),
        "{:?}",
        b.said
    );
}

// allowlist D-70
#[test]
fn debug_builds_add_pydebug_and_o0() {
    let b = build(
        "standard",
        &[],
        Options {
            debug: true,
            ..opts()
        },
        false,
        false,
    );
    let c = config(&b);
    assert!(c.contains(" --with-pydebug --enable-shared "), "{c}");
    assert!(c.contains("CFLAGS_SET=yes\n"), "{c}");
    // D-70: no trailing space after `-O0`.
    assert!(c.contains("CFLAGS=-O0\n"), "{c}");
}

#[test]
fn free_threading_adds_disable_gil() {
    let b = build(
        "standard",
        &[("PYTHON_BUILD_FREE_THREADING", "1")],
        opts(),
        false,
        false,
    );
    let c = config(&b);
    assert!(c.contains(" --disable-gil"), "{c}");
}
