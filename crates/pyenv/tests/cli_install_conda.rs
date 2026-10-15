//! `pyenv install` of a conda definition on Linux against a fake installer on a local server
//! (M7b design §4.3, §8).
#![cfg(unix)]

mod common;

use common::fakeconda::installer;
use common::fakepypy::sha256;
use common::server::{start, Reply, Server};
use common::Fixture;

const PKG: &str = "Miniconda3-py312_25.1.1-0-Linux-x86_64";

/// The environment the CLI tests run with: the fixture's `sys/bin` first, then the real PATH
/// (for `sh`, `bash`), a scratch TMPDIR, and no mirror. The pre-flight check is not skipped:
/// a conda install must not need a compiler.
fn env<'a>(f: &'a Fixture, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
    let mut v: Vec<(&str, String)> = vec![
        (
            "PATH",
            format!("{}:{}", f.syspath.display(), std::env::var("PATH").unwrap()),
        ),
        ("TMPDIR", f.base.join("tmp").display().to_string()),
        ("PYTHON_BUILD_SKIP_MIRROR", "1".into()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

fn run(f: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> common::Run {
    let owned = env(f, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_env(args, &pairs)
}

fn left_in_versions(f: &Fixture) -> Vec<String> {
    std::fs::read_dir(f.root.join("versions"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn serve(body: &str) -> (Server, String) {
    let sha = sha256(body.as_bytes());
    (
        start(vec![(
            &format!("/{PKG}.sh"),
            vec![Reply::Body(body.as_bytes().to_vec())],
        )]),
        sha,
    )
}

/// `<base>/defs/<name>`: one `install_script` line for the served installer, after `extra`.
fn def_file(f: &Fixture, s: &Server, sha: &str, name: &str, step: &str, extra: &str) -> String {
    let p = f.base.join("defs").join(name);
    f.file(
        &p,
        &format!(
            "{extra}install_script \"{PKG}\" \"{}#{sha}\" {step} verify_py312\n",
            s.url(&format!("/{PKG}.sh"))
        ),
    );
    p.display().to_string()
}

fn log(f: &Fixture) -> String {
    std::fs::read_to_string(f.base.join("fake.log")).unwrap_or_default()
}

#[test]
fn a_miniconda_definition_installs_in_place_and_adds_pip() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(
        &f,
        &s,
        &sha,
        "miniconda3-t",
        "miniconda",
        "export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true\n",
    );
    let lp = f.base.join("fake.log").display().to_string();
    let r = run(&f, &["install", &def], &[("FAKECONDA_LOG", &lp)]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = f.root.join("versions/miniconda3-t");
    let l = log(&f);
    assert!(
        l.contains(&format!("args: -f -b -p {}", v.display())),
        "{l}"
    );
    assert!(
        l.contains("marker: yes"),
        "the installer ran in the final, marked folder: {l}"
    );
    assert!(l.contains("tos: true"), "{l}");
    assert!(l.contains("conda: install --yes pip conda=25.1.1"), "{l}");
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert!(v.join("conda-meta/history").is_file());
    assert_eq!(left_in_versions(&f), ["miniconda3-t"]);
    assert!(
        r.stderr
            .contains(&format!("Installed {PKG} to {}", v.display())),
        "{}",
        r.stderr
    );
}

#[test]
fn an_anaconda_definition_runs_no_conda_install() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "anaconda3-t", "anaconda", "");
    let lp = f.base.join("fake.log").display().to_string();
    assert_eq!(
        run(&f, &["install", &def], &[("FAKECONDA_LOG", &lp)]).code,
        0
    );
    assert!(!log(&f).contains("conda: install"), "{}", log(&f));
    assert!(log(&f).contains("tos: unset"));
}

#[test]
fn a_failing_installer_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    let r = run(&f, &["install", &def], &[("FAKECONDA_EXIT", "3")]);
    assert_ne!(r.code, 0);
    assert!(r.stderr.contains("BUILD FAILED"), "{}", r.stderr);
    assert!(
        left_in_versions(&f).is_empty(),
        "{:?}",
        left_in_versions(&f)
    );
}

#[test]
fn a_failing_conda_install_step_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    assert_ne!(
        run(&f, &["install", &def], &[("FAKECONDA_INSTALL_EXIT", "1")]).code,
        0
    );
    assert!(left_in_versions(&f).is_empty());
}

#[test]
fn a_hash_mismatch_leaves_nothing_and_runs_nothing() {
    let f = Fixture::new();
    let (s, _) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &"c".repeat(64), "miniconda3-t", "miniconda", "");
    let lp = f.base.join("fake.log").display().to_string();
    assert_ne!(
        run(&f, &["install", &def], &[("FAKECONDA_LOG", &lp)]).code,
        0
    );
    assert_eq!(log(&f), "", "the installer never ran");
    assert!(left_in_versions(&f).is_empty());
}

#[test]
fn a_failed_verify_leaves_nothing() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12").replace(
        "exit 0\\n' > \"$prefix/bin/python3.12\"",
        "exit 0\\n' > \"$prefix/bin/nothing\"",
    ));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    let r = run(&f, &["install", &def], &[]);
    assert_ne!(r.code, 0);
    assert!(
        r.stderr.contains("invalid Python executable"),
        "{}",
        r.stderr
    );
    assert!(left_in_versions(&f).is_empty());
}

/// Review focus 2; spec D7: only `envs/` is carried over, and the base packages left behind
/// are named with ready commands.
#[test]
fn force_reinstall_keeps_conda_envs_only_and_names_what_it_left() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    assert_eq!(run(&f, &["install", &def], &[]).code, 0);
    let v = f.root.join("versions/miniconda3-t");
    let env = v.join("envs/work");
    std::fs::create_dir_all(env.join("conda-meta")).unwrap();
    // A conda-installed numpy (record, library, package) and a pip-installed requests.
    f.file(&v.join("conda-meta/numpy-1.26.4-py312h1_0.json"), "{}");
    f.file(&v.join("lib/libnumpyfoo.so"), "");
    let sp = v.join("lib/python3.12/site-packages");
    f.file(&sp.join("numpy/__init__.py"), "");
    f.file(&sp.join("numpy-1.26.4.dist-info/INSTALLER"), "conda\n");
    f.file(&sp.join("requests/__init__.py"), "");
    f.file(&sp.join("requests-2.32.3.dist-info/INSTALLER"), "pip\n");
    f.file(&v.join("bin/my-tool"), "");
    let r = run(&f, &["install", "-f", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(env.join("conda-meta").is_dir(), "the env was carried over");
    for gone in [
        "lib/libnumpyfoo.so",
        "lib/python3.12/site-packages/numpy",
        "lib/python3.12/site-packages/requests",
        "bin/my-tool",
        "conda-meta/numpy-1.26.4-py312h1_0.json",
    ] {
        assert!(!v.join(gone).exists(), "{gone} was carried over");
    }
    assert!(
        r.stderr.contains("rpyenv: the previous miniconda3-t's base packages weren't carried over (only envs/ was); to reinstall them: conda install numpy; pip install requests\n"),
        "{}", r.stderr
    );
    assert_eq!(left_in_versions(&f), ["miniconda3-t"]);
}

#[test]
fn a_reinstall_over_a_bare_base_prints_no_list() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    assert_eq!(run(&f, &["install", &def], &[]).code, 0);
    let r = run(&f, &["install", "-f", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains("weren't carried over"), "{}", r.stderr);
}

/// Review focus 1.
#[test]
fn a_folder_left_with_the_marker_is_reinstalled_without_f() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    let v = f.root.join("versions/miniconda3-t");
    f.file(&v.join("bin/python"), "");
    f.file(&v.join(".rpyenv-incomplete"), "");
    let r = run(&f, &["install", &def], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert!(v.join("conda-meta/history").is_file());
}
