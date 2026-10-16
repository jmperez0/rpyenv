//! `pyenv install` of a conda definition on Windows against a fake NSIS installer on a local
//! server (M7c design §5, §8). The fake is `fake-conda-installer`: the server hands out its
//! bytes as every installer, and the installed `python.exe` and `Scripts\conda.exe` are copies
//! of it (see its header for the environment variables).
#![cfg(windows)]

mod common;

use common::server::{start, Reply, Server};
use common::winfake::sha256;
use common::Fixture;
use std::path::PathBuf;

const FORGE: &str = "miniforge3-26.7.2-0";
const FORGE_PKG: &str = "Miniforge3-26.7.2-0-Windows-x86_64";
const FORGE_PATH: &str =
    "/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe";
const FORGE_OLD: &str = "miniforge3-4.9.2-7";
const FORGE_OLD_PATH: &str =
    "/miniforge/releases/download/4.9.2-7/Miniforge3-4.9.2-7-Windows-x86_64.exe";
const CONDA: &str = "miniconda3-3.13-26.7.1-1";
const CONDA_PATH: &str = "/miniconda/Miniconda3-py313_26.7.1-1-Windows-x86_64.exe";
const CONDA_OLD: &str = "miniconda3-4.7.12";
const CONDA_OLD_PATH: &str = "/miniconda/Miniconda3-4.7.12-Windows-x86_64.exe";
const SILENT: &str =
    "/S /InstallationType=JustMe /AddToPath=0 /RegisterPython=0 /NoRegistry=1 /NoShortcuts=1";

fn fake_bytes() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_fake-conda-installer")).unwrap()
}

struct T {
    f: Fixture,
    /// Owns the scratch folder `root` lives in.
    _scratch: tempfile::TempDir,
    root: PathBuf,
    s: Server,
    tables: PathBuf,
    log: PathBuf,
}

impl T {
    /// A pyenv root `<scratch>\<root_dir>` (the fixture's own root has a space, which
    /// Miniforge's installer refuses) and a table with `rows`: `(name, installer path on the
    /// server, step, exported variable or "")`, plus `extra` verbatim lines. `sha` is the
    /// hash the `ok` rows carry (None: the fake's real one).
    fn new(
        root_dir: &str,
        rows: &[(&str, &str, &str, &str)],
        extra: &[&str],
        sha: Option<&str>,
    ) -> T {
        let f = Fixture::new();
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().join(root_dir);
        std::fs::create_dir_all(&root).unwrap();
        let bytes = fake_bytes();
        let real = sha256(&bytes);
        let sha = sha.unwrap_or(&real);
        let routes: Vec<(&str, Vec<Reply>)> = rows
            .iter()
            .map(|(_, p, _, _)| (*p, vec![Reply::Body(bytes.clone())]))
            .collect();
        let s = start(routes);
        let mut text = String::from("# generated 2026-10-16T09:30:00Z by ci\n");
        for (name, path, step, var) in rows {
            let file = path.rsplit('/').next().unwrap().trim_end_matches(".exe");
            text.push_str(&format!(
                "{name}\tWindows-x86_64\tok {file} {}#{sha} {step} verify_py3 {var}\n",
                s.url(path)
            ));
        }
        for l in extra {
            text.push_str(l);
            text.push('\n');
        }
        let tables = scratch.path().join("tables");
        std::fs::create_dir_all(&tables).unwrap();
        std::fs::write(tables.join("windows.tsv"), text).unwrap();
        // `.versions_cache.xml`, so that `install` passes its database check.
        let db = vec![pyenv::install::wincatalog::Row {
            code: "3.12.1".into(),
            file: "3.12.1.exe".into(),
            url: "https://www.python.org/ftp/python/x/3.12.1.exe".into(),
            x64: true,
            web_install: false,
            msi: false,
            zip_root_dir: None,
        }];
        pyenv::install::wincatalog::write_db(&root, &db).unwrap();
        let log = scratch.path().join("fake.log");
        T {
            f,
            _scratch: scratch,
            root,
            s,
            tables,
            log,
        }
    }

    /// The common case: the four default rows under a root without a space.
    fn std() -> T {
        T::new(
            "root",
            &[
                (FORGE, FORGE_PATH, "miniconda", ""),
                (FORGE_OLD, FORGE_OLD_PATH, "miniconda", ""),
                (
                    CONDA,
                    CONDA_PATH,
                    "miniconda",
                    "CONDA_PLUGINS_AUTO_ACCEPT_TOS=true",
                ),
                (CONDA_OLD, CONDA_OLD_PATH, "miniconda", ""),
            ],
            &[
                "miniforge3-4.9.2-1\tWindows-x86_64\tunsupported no published SHA-256",
                "anaconda-1.5.1\tWindows-x86_64\tno-binary Anaconda",
            ],
            None,
        )
    }

    fn run(&self, args: &[&str], extra: &[(&str, &str)]) -> common::Run {
        let root = self.root.display().to_string();
        let home = self.root.parent().unwrap().display().to_string();
        let tables = self.tables.display().to_string();
        let log = self.log.display().to_string();
        let org = self.s.url("/ftp/python");
        let mut env: Vec<(&str, &str)> = vec![
            ("PYENV", &root),
            ("PYENV_ROOT", &root),
            ("PYENV_HOME", &root),
            ("HOME", &home),
            ("RPYENV_TEST_PYTHON_ORG", &org),
            ("RPYENV_TEST_CONDA_TABLES", &tables),
            ("FAKECONDA_LOG", &log),
        ];
        env.extend_from_slice(extra);
        self.f.pyenv_env(args, &env)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn version(&self, name: &str) -> PathBuf {
        self.root.join("versions").join(name)
    }

    fn left_in_versions(&self) -> Vec<String> {
        std::fs::read_dir(self.root.join("versions"))
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn lines(r: &common::Run) -> Vec<&str> {
    r.stdout.lines().collect()
}

// allowlist D-110
#[test]
fn a_conda_name_installs_in_place_with_the_silent_flags() {
    let t = T::std();
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    let target = t.version(FORGE);
    assert!(target.join("python.exe").is_file());
    assert!(!target.join(".rpyenv-incomplete").exists());
    let log = t.log();
    let want = format!("args: {SILENT} /D={}", target.display());
    assert!(log.lines().any(|l| l == want), "{want}\n{log}");
    assert!(log.lines().any(|l| l == "marker: yes"), "{log}");
    // Miniforge's step is upstream's miniconda step: pip, pinned to the installed conda.
    assert!(
        log.lines()
            .any(|l| l == "conda: install --yes pip conda=25.1.1"),
        "{log}"
    );
    assert!(
        log.lines().any(|l| l == "python: -c import sys, ssl"),
        "{log}"
    );
    assert!(log.lines().any(|l| l == "tos: unset"), "{log}");
    assert!(t.root.join("shims").join("python.exe").is_file());
    assert!(t.root.join("shims").join("conda.exe").is_file());
    let out = lines(&r);
    assert!(
        out.contains(&format!(":: [Installing] ::  {FORGE} ...").as_str()),
        "{out:?}"
    );
    assert!(
        out.contains(&format!(":: [Info] :: completed! {FORGE}").as_str()),
        "{out:?}"
    );
    assert_eq!(t.s.hits(FORGE_PATH), 1);
}

#[test]
fn a_tos_row_sets_the_variable_and_prints_the_notice() {
    let t = T::std();
    let r = t.run(&["install", CONDA], &[]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(t.log().lines().any(|l| l == "tos: true"), "{}", t.log());
    assert!(
        lines(&r).contains(&pyenv::install::conda::TERMS_NOTICE),
        "{}",
        r.stdout
    );
}

#[test]
fn a_hash_mismatch_is_refused_and_leaves_nothing() {
    let t = T::new(
        "root",
        &[(FORGE, FORGE_PATH, "miniconda", "")],
        &[],
        Some(&"0".repeat(64)),
    );
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    let out = lines(&r);
    let at = out
        .iter()
        .position(|l| l.starts_with(":: [Error] :: cannot download"))
        .unwrap_or_else(|| panic!("{out:?}"));
    assert_eq!(
        out[at],
        format!(
            ":: [Error] :: cannot download {FORGE_PKG}.exe: checksum mismatch: {FORGE_PKG}.exe (file is corrupt)"
        )
    );
    // Spec §4.4 (D8): the one extra line for a failed hash.
    assert_eq!(
        out[at + 1],
        "if the publisher replaced this installer, a later rpyenv release will carry the reviewed hash"
    );
    assert_eq!(
        out[at + 2],
        format!(":: [Error] :: couldn't install {FORGE}")
    );
    assert_eq!(t.log(), "", "the installer must not have run");
    assert!(
        t.left_in_versions().is_empty(),
        "{:?}",
        t.left_in_versions()
    );
}

#[test]
fn an_installer_failure_deletes_the_folder() {
    let t = T::std();
    let r = t.run(&["install", FORGE], &[("FAKECONDA_EXIT", "3")]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    let out = lines(&r);
    // The message names the install log (review M1); its first line is the step's header.
    let log = t.root.join("install_cache").join(format!("{FORGE}.log"));
    let want = format!(
        ":: [Error] :: {FORGE}: the installer exited with exit code: 3; output is in {}:",
        log.display()
    );
    assert!(
        out.contains(&want.as_str()),
        "{want}
{out:?}"
    );
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .starts_with(&format!("== {FORGE}: installer /S ")),);
    assert!(out.contains(&format!(":: [Error] :: couldn't install {FORGE}").as_str()));
    assert!(
        t.left_in_versions().is_empty(),
        "{:?}",
        t.left_in_versions()
    );
}

#[test]
fn a_failed_import_check_deletes_the_folder() {
    let t = T::std();
    let r = t.run(&["install", FORGE], &[("FAKECONDA_PYTHON_EXIT", "1")]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    let out = lines(&r);
    assert!(
        t.left_in_versions().is_empty(),
        "{:?}",
        t.left_in_versions()
    );
    // Review M1: the message names the install log and quotes its last lines.
    let log = t.root.join("install_cache").join(format!("{FORGE}.log"));
    let at = out
        .iter()
        .position(|l| l.starts_with(&format!(":: [Error] :: {FORGE}: python.exe can't import")))
        .unwrap_or_else(|| panic!("{out:?}"));
    assert_eq!(
        out[at],
        format!(
            ":: [Error] :: {FORGE}: python.exe can't import ssl with conda's PATH; the install is broken; output is in {}:",
            log.display()
        )
    );
    assert_eq!(out[at + 5], "fake python: ssl failed to import");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("python.exe -c import sys, ssl"), "{text}");
    assert!(text.contains("fake python: ssl failed to import"), "{text}");
}

#[test]
fn a_failed_force_reinstall_keeps_the_previous_install_and_its_envs() {
    let t = T::std();
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let x = t.version(FORGE).join("envs").join("e1").join("x");
    std::fs::create_dir_all(x.parent().unwrap()).unwrap();
    std::fs::write(&x, "mine").unwrap();
    let r = t.run(&["install", "-f", FORGE], &[("FAKECONDA_PYTHON_EXIT", "1")]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    assert_eq!(std::fs::read_to_string(&x).unwrap(), "mine");
    assert!(t.version(FORGE).join("python.exe").is_file());
    assert!(!t.version(FORGE).join(".rpyenv-incomplete").exists());
    assert_eq!(t.left_in_versions(), [FORGE]);
}

#[test]
fn the_users_conda_and_python_variables_do_not_reach_the_installer_or_its_steps() {
    let t = T::std();
    let r = t.run(
        &["install", CONDA],
        &[
            ("CONDA_PREFIX", "C:/elsewhere"),
            ("CONDA_DEFAULT_ENV", "e1"),
            ("PYTHONPATH", "x"),
        ],
    );
    assert_eq!(
        r.code, 0,
        "{}
{}",
        r.stdout, r.stderr
    );
    let log = t.log();
    let envs: Vec<&str> = log.lines().filter(|l| l.starts_with("env: ")).collect();
    // The installer, `conda --version`, `conda install` and the import check.
    assert_eq!(
        envs, ["env: CONDA_PREFIX=unset CONDA_DEFAULT_ENV=unset PYTHONPATH=unset"; 4],
        "{log}"
    );
    // The row's own variable still reaches the installer.
    assert!(log.lines().any(|l| l == "tos: true"), "{log}");
}

#[test]
fn force_reinstall_keeps_envs() {
    let t = T::std();
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let x = t.version(FORGE).join("envs").join("e1").join("x");
    std::fs::create_dir_all(x.parent().unwrap()).unwrap();
    std::fs::write(&x, "mine").unwrap();
    let r = t.run(&["install", "-f", FORGE], &[]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(std::fs::read_to_string(&x).unwrap(), "mine");
    assert_eq!(
        t.log().lines().filter(|l| l.starts_with("args: ")).count(),
        2,
        "{}",
        t.log()
    );
    // Without -f an installed version is skipped silently, as for CPython.
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(
        t.log().lines().filter(|l| l.starts_with("args: ")).count(),
        2
    );
}

#[test]
fn an_unsupported_row_is_refused_with_the_newest_hint() {
    let t = T::std();
    let r = t.run(&["install", "miniforge3-4.9.2-1"], &[]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    let out = lines(&r);
    assert!(
        out.contains(
            &":: [Error] :: rpyenv cannot install this conda definition (no published SHA-256); the newest miniforge3 rpyenv can install here is miniforge3-26.7.2-0"
        ),
        "{out:?}"
    );
    assert!(out.contains(&":: [Error] :: couldn't install miniforge3-4.9.2-1"));
    let r = t.run(&["install", "anaconda-1.5.1"], &[]);
    assert_eq!(r.code, 1);
    assert!(
        lines(&r).contains(
            &":: [Error] :: rpyenv cannot install anaconda-1.5.1: the publisher has no Windows x64 installer for it"
        ),
        "{}",
        r.stdout
    );
    assert_eq!(t.log(), "");
}

#[test]
fn a_root_with_a_refused_character_is_refused_before_download() {
    // Miniforge under a root with a space (spec D10).
    let t = T::new(
        "a b",
        &[
            (FORGE, FORGE_PATH, "miniconda", ""),
            (CONDA, CONDA_PATH, "miniconda", ""),
        ],
        &[],
        None,
    );
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    let target = t.version(FORGE);
    let want = format!(
        ":: [Error] :: python-build: {FORGE}: Miniforge's installer refuses a path with a space, and rpyenv would install it into {}. Install a miniconda3 version instead (its installer accepts spaces), or set PYENV_ROOT, PYENV and PYENV_HOME to a folder without spaces.",
        target.display()
    );
    assert!(lines(&r).contains(&want.as_str()), "{want}\n{}", r.stdout);
    assert!(r.stdout.contains("Install a miniconda3 version instead"));
    assert_eq!(t.s.hits(FORGE_PATH), 0);
    assert!(t.left_in_versions().is_empty());
    // A '^' is refused by every family, naming the character.
    let t = T::new("a^b", &[(CONDA, CONDA_PATH, "miniconda", "")], &[], None);
    let r = t.run(&["install", CONDA], &[]);
    assert_eq!(r.code, 1, "{}", r.stdout);
    let want = format!(
        ":: [Error] :: python-build: {CONDA}: conda's installer refuses the character '^' in {}; set PYENV_ROOT, PYENV and PYENV_HOME to a folder without it.",
        t.version(CONDA).display()
    );
    assert!(lines(&r).contains(&want.as_str()), "{want}\n{}", r.stdout);
    assert_eq!(t.s.hits(CONDA_PATH), 0);
}

/// Spec §3 (ruling R16): NSIS reads everything after ` /D=` as the folder, so a Miniconda
/// root with a space installs only when the tail reaches the installer unquoted. The fake
/// reads the raw command line as NSIS does and exits 2 if the tail starts with a quote.
#[test]
fn a_miniconda_root_with_a_space_installs_with_an_unquoted_d_tail() {
    let t = T::new("a b", &[(CONDA, CONDA_PATH, "miniconda", "")], &[], None);
    let r = t.run(&["install", CONDA], &[]);
    assert_eq!(r.code, 0, "{}\n{}\n{}", r.stdout, r.stderr, t.log());
    let target = t.version(CONDA);
    assert!(target.to_string_lossy().contains("a b"));
    let want = format!("args: {SILENT} /D={}", target.display());
    assert!(t.log().lines().any(|l| l == want), "{want}\n{}", t.log());
    assert!(!t.log().contains("bad args"), "{}", t.log());
    assert!(target.join("python.exe").is_file());
}

#[test]
fn a_bare_family_installs_its_newest_installable_release() {
    let t = T::std();
    let r = t.run(&["install", "miniforge3"], &[]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(t.version(FORGE).join("python.exe").is_file());
    assert!(!t.version(FORGE_OLD).exists());
    assert_eq!(t.s.hits(FORGE_OLD_PATH), 0);
    assert_eq!(t.s.hits(FORGE_PATH), 1);
}

// allowlist D-110
#[test]
fn install_list_shows_installable_conda_names_after_cpython() {
    let t = T::std();
    let r = t.run(&["install", "-l"], &[]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let out = lines(&r);
    let at = out
        .iter()
        .position(|l| *l == "3.12.1")
        .unwrap_or_else(|| panic!("{out:?}"));
    // Table order; the unsupported and no-binary rows are not listed.
    assert_eq!(&out[at..], ["3.12.1", FORGE, FORGE_OLD, CONDA, CONDA_OLD]);
}

// allowlist D-110
#[test]
fn latest_known_resolves_a_conda_family() {
    let t = T::std();
    let r = t.run(&["latest", "-k", "miniconda3"], &[]);
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (0, "miniconda3-3.13-26.7.1-1\r\n")
    );
    let r = t.run(&["latest", "-k", "miniforge3"], &[]);
    assert_eq!((r.code, r.stdout.as_str()), (0, "miniforge3-26.7.2-0\r\n"));
    // A name that isn't installable matches nothing.
    let r = t.run(&["latest", "-k", "miniforge3-4.9.2-1"], &[]);
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (
            1,
            "pyenv-latest: no known versions match the prefix 'miniforge3-4.9.2-1'.\r\n"
        )
    );
}

#[test]
fn no_table_names_pyenv_update() {
    let t = T::std();
    let empty = t.tables.join("none");
    std::fs::create_dir_all(&empty).unwrap();
    let r = t.run(
        &["install", FORGE],
        &[("RPYENV_TEST_CONDA_TABLES", &empty.display().to_string())],
    );
    assert_eq!(r.code, 1, "{}", r.stdout);
    assert!(
        lines(&r).contains(&"pyenv-install: rpyenv has no conda version table; run pyenv update"),
        "{}",
        r.stdout
    );
    assert_eq!(t.log(), "");
}

#[test]
fn a_conda_name_installs_without_the_cpython_database() {
    // Ruling R19: the table ships with rpyenv.
    let t = T::std();
    std::fs::remove_file(t.root.join(pyenv::install::wincatalog::DB_NAME)).unwrap();
    let r = t.run(&["install", FORGE], &[]);
    assert_eq!(
        r.code, 0,
        "{}
{}",
        r.stdout, r.stderr
    );
    assert!(t.version(FORGE).join("python.exe").is_file());
    assert!(!r.stdout.contains("no definitions in local database"));
}

#[test]
fn install_list_without_the_database_lists_the_conda_names_alone() {
    let t = T::std();
    std::fs::remove_file(t.root.join(pyenv::install::wincatalog::DB_NAME)).unwrap();
    let r = t.run(&["install", "-l"], &[]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let out = lines(&r);
    let at = out
        .iter()
        .position(|l| *l == FORGE)
        .unwrap_or_else(|| panic!("{out:?}"));
    assert_eq!(&out[at..], [FORGE, FORGE_OLD, CONDA, CONDA_OLD]);
    assert!(!out.contains(&"3.12.1"));
}

/// Review I1: Ctrl+C must end the NSIS stub's children too, and only then may the folder go.
/// The fake starts a grandchild that grows a file in the target after a second, as
/// `_conda.exe` would; the stop closure plays Ctrl+C once the file has grown.
#[test]
fn interrupting_the_installer_ends_its_grandchildren() {
    use pyenv::install::winconda::{wait_or_kill, Tree};
    use std::os::windows::process::CommandExt;
    let scratch = tempfile::tempdir().unwrap();
    let exe = scratch.path().join("Fake-Windows-x86_64.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_fake-conda-installer"), &exe).unwrap();
    let d = scratch.path().join("target");
    std::fs::create_dir_all(&d).unwrap();
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("/S")
        .raw_arg(format!("/D={}", d.display()))
        .env("FAKECONDA_GRANDCHILD", "1")
        .env_remove("FAKECONDA_LOG")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().unwrap();
    let tree = Tree::new(&child);
    let grow = d.join("grow.txt");
    let len = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let started = std::time::Instant::now();
    let r = wait_or_kill(&mut child, &tree, &|| {
        len(&grow) >= 3 || started.elapsed() > std::time::Duration::from_secs(20)
    });
    assert!(matches!(r, Ok(None)), "{r:?}");
    assert!(len(&grow) >= 3, "the grandchild never wrote");
    let pid = std::fs::read_to_string(d.join("grandchild.pid")).unwrap();
    // wait_or_kill returned: nothing may write any more.
    let before = len(&grow);
    std::thread::sleep(std::time::Duration::from_millis(700));
    assert_eq!(len(&grow), before, "the grandchild is still writing");
    let tasks = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&tasks.stdout).contains(&format!(" {pid} ")),
        "{}",
        String::from_utf8_lossy(&tasks.stdout)
    );
    // The folder can go, and stays gone.
    std::fs::remove_dir_all(&d).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!d.exists());
}
