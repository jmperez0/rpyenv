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

/// A caller with another conda env active: conda follows CONDA_PREFIX, so neither the
/// installer nor `conda install pip` may see it (M7c ruling R18, now on Linux too).
// allowlist D-116
#[test]
fn the_callers_conda_env_reaches_neither_the_installer_nor_conda_install() {
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
    let r = run(
        &f,
        &["install", &def],
        &[
            ("FAKECONDA_LOG", &lp),
            ("CONDA_PREFIX", "/elsewhere/env"),
            ("CONDA_DEFAULT_ENV", "e1"),
            ("PYTHONPATH", "/x"),
            ("CONDA_CHANNEL_ALIAS", "https://mirror.example"),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let l = log(&f);
    assert!(l.contains("installer env: unset unset unset"), "{l}");
    assert!(
        l.contains("conda env: unset unset unset alias=https://mirror.example"),
        "conda's own settings still apply: {l}"
    );
    // The definition's own variable still gets through.
    assert!(l.contains("tos: true"), "{l}");
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
// allowlist D-58
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

#[test]
fn a_failing_f_reinstall_restores_the_previous_version() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let def = def_file(&f, &s, &sha, "miniconda3-t", "miniconda", "");
    assert_eq!(run(&f, &["install", &def], &[]).code, 0);
    let v = f.root.join("versions/miniconda3-t");
    std::fs::create_dir_all(v.join("envs/work")).unwrap();
    f.file(&v.join("conda-meta/history"), "previous\n");
    let r = run(&f, &["install", "-f", &def], &[("FAKECONDA_EXIT", "3")]);
    assert_ne!(r.code, 0);
    assert_eq!(
        std::fs::read_to_string(v.join("conda-meta/history")).unwrap(),
        "previous\n"
    );
    assert!(v.join("envs/work").is_dir());
    assert!(!v.join(".rpyenv-incomplete").exists());
    assert_eq!(left_in_versions(&f), ["miniconda3-t"]);
}

fn table(f: &Fixture, rows: &[(&str, String)]) -> String {
    let arch = pyenv::install::conda::host_arch().expect("tests run on x64 or aarch64");
    let d = f.base.join("tables");
    let text: String = rows
        .iter()
        .map(|(n, v)| format!("{n}\t{arch}\t{v}\n"))
        .collect();
    f.file(&d.join("linux.tsv"), &text);
    d.display().to_string()
}

// allowlist D-107
#[test]
fn a_vendored_name_installs_from_its_table_row_with_the_terms_notice() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let row = format!(
        "ok {PKG} {}#{sha} miniconda verify_py312 CONDA_PLUGINS_AUTO_ACCEPT_TOS=true",
        s.url(&format!("/{PKG}.sh"))
    );
    let t = table(&f, &[("miniconda3-3.10-23.11.0-1", row)]);
    let lp = f.base.join("fake.log").display().to_string();
    let r = run(
        &f,
        &["install", "miniconda3-3.10-23.11.0-1"],
        &[("RPYENV_TEST_CONDA_TABLES", &t), ("FAKECONDA_LOG", &lp)],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let notice = r
        .stderr
        .find("rpyenv: Miniconda's defaults channel is covered by Anaconda's terms of service")
        .unwrap_or_else(|| panic!("no notice: {}", r.stderr));
    let download = r
        .stderr
        .find("Downloading")
        .unwrap_or_else(|| panic!("no download line: {}", r.stderr));
    assert!(
        notice < download,
        "notice must precede the download: {}",
        r.stderr
    );
    assert!(log(&f).contains("tos: true"));
}

#[test]
fn no_notice_without_the_variable() {
    let f = Fixture::new();
    let (s, sha) = serve(&installer("3.12"));
    let t = table(
        &f,
        &[(
            "miniforge3-25.1.1-0",
            format!(
                "ok {PKG} {}#{sha} miniconda verify_py312",
                s.url(&format!("/{PKG}.sh"))
            ),
        )],
    );
    let r = run(
        &f,
        &["install", "miniforge3-25.1.1-0"],
        &[("RPYENV_TEST_CONDA_TABLES", &t)],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains("terms of service"));
}

// allowlist D-109
#[test]
fn an_unsupported_row_is_refused_by_name() {
    let f = Fixture::new();
    let t = table(
        &f,
        &[("mambaforge", "unsupported no published hash".into())],
    );
    let r = run(
        &f,
        &["install", "mambaforge"],
        &[("RPYENV_TEST_CONDA_TABLES", &t)],
    );
    assert_eq!(r.code, 1);
    assert_eq!(
        r.stderr.trim_end(),
        "python-build: mambaforge: rpyenv cannot install this conda definition (no published hash)"
    );
}

// A bare `miniconda3` resolves, by upstream's sort, to the MD5-only 4.7.12; the refusal names
// the newest release that installs.
#[test]
fn a_refusal_names_the_newest_installable_release_of_its_family() {
    let f = Fixture::new();
    let sha = "b".repeat(64);
    let ok = format!("ok P https://repo.anaconda.com/miniconda/P.sh#{sha} miniconda verify_py3");
    let t = table(
        &f,
        &[
            ("miniconda3-4.7.12", "unsupported md5 checksum only".into()),
            ("miniconda3-4.3.30", ok.clone()),
            ("miniconda3-3.13-26.7.1-1", ok.clone()),
            ("miniconda3-3.14-26.7.1-1", ok),
        ],
    );
    let r = run(
        &f,
        &["install", "miniconda3"],
        &[("RPYENV_TEST_CONDA_TABLES", &t)],
    );
    assert_eq!(r.code, 1);
    assert_eq!(
        r.stderr.trim_end(),
        "python-build: miniconda3-4.7.12: rpyenv cannot install this conda definition (md5 checksum only); the newest miniconda3 rpyenv can install here is miniconda3-3.14-26.7.1-1"
    );
}

#[test]
fn a_no_binary_row_prints_upstreams_lines() {
    let f = Fixture::new();
    let t = table(&f, &[("anaconda3-2025.12-2", "no-binary Anaconda".into())]);
    let arch = pyenv::install::conda::host_arch().unwrap();
    let r = run(
        &f,
        &["install", "anaconda3-2025.12-2"],
        &[("RPYENV_TEST_CONDA_TABLES", &t)],
    );
    assert_eq!(r.code, 1);
    assert_eq!(
        r.stderr,
        format!("\nERROR: The binary distribution of Anaconda is not available for {arch}.\n\n")
    );
}

#[test]
fn a_name_missing_from_the_table_is_no_binary() {
    let f = Fixture::new();
    let t = table(&f, &[("miniforge3-25.1.1-0", "no-binary x".into())]);
    let arch = pyenv::install::conda::host_arch().unwrap();
    let r = run(
        &f,
        &["install", "anaconda3-2025.12-2"],
        &[("RPYENV_TEST_CONDA_TABLES", &t)],
    );
    assert_eq!(r.code, 1);
    assert_eq!(
        r.stderr,
        format!("\nERROR: The binary distribution of anaconda3-2025.12-2 is not available for {arch}.\n\n")
    );
}

/// Review focus 3. allowlist D-109
#[test]
fn a_user_definition_without_a_hash_is_refused() {
    let f = Fixture::new();
    let (s, _) = serve(&installer("3.12"));
    let p = f.base.join("defs").join("miniforge3-mine");
    f.file(&p, &format!("case \"$(anaconda_architecture 2>/dev/null || true)\" in\n* )\n  install_script \"{PKG}\" \"{}\" \"miniconda\" verify_py312\n  ;;\nesac\n", s.url(&format!("/{PKG}.sh"))));
    let r = run(&f, &["install", &p.display().to_string()], &[]);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("python-build: miniforge3-mine: rpyenv cannot read this conda definition (no sha256 in "), "{}", r.stderr);
    assert_eq!(s.hits(&format!("/{PKG}.sh")), 0, "nothing downloaded");
}

// allowlist D-108
#[test]
fn miniconda_latest_takes_its_hash_from_the_listing() {
    let f = Fixture::new();
    let arch = pyenv::install::conda::host_arch().unwrap();
    let file = format!("Miniconda3-latest-{arch}.sh");
    let body = installer("3.12");
    let sha = sha256(body.as_bytes());
    let listing = format!("<table><tr><td><a href=\"{file}\">{file}</a></td><td>1M</td><td>2026-08-28</td><td>{sha}</td></tr></table>");
    let s = start(vec![
        ("/miniconda/", vec![Reply::Body(listing.into_bytes())]),
        (
            &format!("/miniconda/{file}"),
            vec![Reply::Body(body.into_bytes())],
        ),
    ]);
    let t = table(
        &f,
        &[(
            "miniconda3-latest",
            format!(
                "ok-latest {PKG} {} miniconda verify_py3_latest",
                s.url(&format!("/miniconda/{file}"))
            ),
        )],
    );
    let base = s.url("");
    let r = run(
        &f,
        &["install", "miniconda3-latest"],
        &[
            ("RPYENV_TEST_CONDA_TABLES", &t),
            ("RPYENV_TEST_CONDA", &base),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
}

/// Review focus 5.
#[test]
fn latest_without_a_listing_row_is_refused_before_downloading() {
    let f = Fixture::new();
    let arch = pyenv::install::conda::host_arch().unwrap();
    let file = format!("Miniconda3-latest-{arch}.sh");
    let s = start(vec![
        (
            "/miniconda/",
            vec![Reply::Body(b"<table></table>".to_vec())],
        ),
        (
            &format!("/miniconda/{file}"),
            vec![Reply::Body(b"x".to_vec())],
        ),
    ]);
    let t = table(
        &f,
        &[(
            "miniconda3-latest",
            format!(
                "ok-latest {PKG} {} miniconda verify_py3_latest",
                s.url(&format!("/miniconda/{file}"))
            ),
        )],
    );
    let base = s.url("");
    let r = run(
        &f,
        &["install", "miniconda3-latest"],
        &[
            ("RPYENV_TEST_CONDA_TABLES", &t),
            ("RPYENV_TEST_CONDA", &base),
        ],
    );
    assert_eq!(r.code, 1);
    assert_eq!(r.stderr.trim_end(), format!("python-build: miniconda3-latest: the publisher's SHA-256 for {file} couldn't be read from {base}/miniconda/, so rpyenv won't install it"));
    assert_eq!(s.hits(&format!("/miniconda/{file}")), 0);
}

// allowlist D-108
#[test]
fn miniforge_latest_installs_the_versioned_twin_with_its_sha256_file() {
    let f = Fixture::new();
    let arch = pyenv::install::conda::host_arch().unwrap();
    let file = format!("Miniforge3-26.7.2-0-{arch}.sh");
    let body = installer("3.12");
    let sha = sha256(body.as_bytes());
    let s = start(vec![
        (
            "/miniforge/releases/latest",
            vec![Reply::Redirect("/miniforge/releases/tag/26.7.2-0".into())],
        ),
        (
            &format!("/miniforge/releases/download/26.7.2-0/{file}.sha256"),
            vec![Reply::Body(format!("{sha}  {file}\n").into_bytes())],
        ),
        (
            &format!("/miniforge/releases/download/26.7.2-0/{file}"),
            vec![Reply::Body(body.into_bytes())],
        ),
    ]);
    let t = table(&f, &[("miniforge3-latest", format!("ok-latest Miniforge3-{arch} https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-{arch}.sh miniconda verify_py3_latest"))]);
    let base = s.url("");
    let r = run(
        &f,
        &["install", "miniforge3-latest"],
        &[
            ("RPYENV_TEST_CONDA_TABLES", &t),
            ("RPYENV_TEST_CONDA", &base),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        s.hits(&format!("/miniforge/releases/download/26.7.2-0/{file}")),
        1
    );
}
