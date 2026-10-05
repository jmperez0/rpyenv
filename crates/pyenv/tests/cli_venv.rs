//! Built-in virtualenvs on Linux (spec §10; docs/parity/pyenv-virtualenv-m4-reference.md).
#![cfg(unix)]

mod common;
use common::Fixture;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A stand-in base Python: `-m venv [opts] DIR` (fails with FAKE_VENV_FAIL, skips pip with
/// FAKE_NO_PIP or --without-pip), `-m venv --help`, and `-s -m ensurepip`. Calls are logged
/// to `<prefix>/calls.log`, where `<prefix>` is the folder above the script's `bin`.
pub const FAKE_PYTHON: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
here=${0%/bin/*}
if [ "$1" = "-m" ] && [ "$2" = "venv" ]; then
  shift 2
  [ "$1" = "--help" ] && exit 0
  echo "venv $*" >> "$here/calls.log"
  if [ -n "$FAKE_VENV_FAIL" ]; then echo "venv failed"; exit 5; fi
  for a in "$@"; do dir=$a; done
  ssp=false
  for a in "$@"; do [ "$a" = "--system-site-packages" ] && ssp=true; done
  mkdir -p "$dir/bin"
  printf 'home = %s/bin\ninclude-system-site-packages = %s\n' "$here" "$ssp" > "$dir/pyvenv.cfg"
  ln -sf "$here/bin/python" "$dir/bin/python"
  : > "$dir/bin/activate"
  case " $* " in *" --without-pip "*) ;; *) [ -z "$FAKE_NO_PIP" ] && { printf '#!/bin/sh\n' > "$dir/bin/pip"; chmod 755 "$dir/bin/pip"; } ;; esac
  exit 0
fi
if [ "$1" = "-s" ] && [ "$2" = "-m" ] && [ "$3" = "ensurepip" ]; then
  echo "ensurepip" >> "$here/calls.log"
  printf '#!/bin/sh\n' > "${0%/*}/pip"
  chmod 755 "${0%/*}/pip"
  exit 0
fi
exit 3
"#;

fn exe(f: &Fixture, p: &Path, body: &str) {
    f.file(p, body);
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `versions/<v>` with the fake `bin/python`.
fn fake_base(f: &Fixture, v: &str) -> PathBuf {
    let b = f.root.join("versions").join(v);
    exe(f, &b.join("bin/python"), FAKE_PYTHON);
    b
}

fn calls(prefix: &Path) -> String {
    std::fs::read_to_string(prefix.join("calls.log")).unwrap_or_default()
}

fn run(f: &Fixture, args: &[&str]) -> (String, String, i32) {
    let r = f.pyenv(args);
    (r.stdout, r.stderr, r.code)
}

fn run_env(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let r = f.pyenv_env(args, env);
    (r.stdout, r.stderr, r.code)
}

/// `pyenv <args>` with `input` on stdin (the y/N questions).
fn run_stdin(f: &Fixture, args: &[&str], input: &str) -> (String, String, i32) {
    for _ in 0..100 {
        let mut c = f
            .command(Path::new(env!("CARGO_BIN_EXE_pyenv")), &f.work, &[])
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let o = c.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        if o.status.code() == Some(126) && err.contains("Text file busy") {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        return (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            err,
            o.status.code().unwrap(),
        );
    }
    panic!("busy");
}

#[test]
fn creates_an_env_its_link_pydoc_and_shims() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let (out, err, code) = run(&f, &["virtualenv", "3.12.1", "venv1"]);
    assert_eq!((out.as_str(), code), ("", 0), "{err}");
    let env = base.join("envs/venv1");
    assert!(env.join("pyvenv.cfg").is_file());
    assert_eq!(
        std::fs::read_link(f.root.join("versions/venv1")).unwrap(),
        env
    );
    assert_eq!(calls(&base), format!("venv {}\n", env.display()));
    let pydoc = std::fs::read_to_string(env.join("bin/pydoc")).unwrap();
    assert_eq!(
        pydoc,
        format!(
            "#!{}/bin/python\nimport pydoc\nif __name__ == '__main__':\n      pydoc.cli()\n",
            env.display()
        )
    );
    assert!(f.root.join("shims/python").exists(), "rehash ran");
}

#[test]
fn one_name_uses_the_current_version_and_a_prefix_is_resolved() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    assert_eq!(
        run_env(&f, &["virtualenv", "venv2"], &[("PYENV_VERSION", "3.12.1")]).2,
        0
    );
    assert!(base.join("envs/venv2").is_dir());
    assert_eq!(run(&f, &["virtualenv", "3.12", "venv3"]).2, 0);
    assert!(base.join("envs/venv3").is_dir());
}

/// Review focus 1: names that would leave `versions/` are refused, and nothing is made.
#[test]
fn name_checks() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    let err = |args: &[&str]| run(&f, args).1;
    assert_eq!(
        err(&["virtualenv"]),
        "pyenv-virtualenv: no virtualenv name given.\n"
    );
    assert_eq!(
        err(&["virtualenv", "3.12.1", "system"]),
        "pyenv-virtualenv: `system' is not allowed as virtualenv name.\n"
    );
    assert_eq!(
        err(&["virtualenv", "3.12.1", "a b"]),
        "pyenv-virtualenv: no whitespace allowed in virtualenv name.\n"
    );
    assert_eq!(
        err(&["virtualenv", "3.12.1", "x/y"]),
        "pyenv-virtualenv: no slash allowed in virtualenv name.\n"
    );
    assert_eq!(
        err(&["virtualenv", "3.12.1", "../x"]),
        "pyenv-virtualenv: no slash allowed in virtualenv name.\n"
    );
    assert_eq!(
        err(&["virtualenv", "3.12.1", ".."]),
        "pyenv-virtualenv: `..' is not allowed as virtualenv name.\n"
    );
    assert_eq!(run(&f, &["virtualenv", "3.12.1", "3.12.1/envs/ok"]).2, 0);
    assert!(!f.base.join("x").exists() && !f.root.join("x").exists());
}

#[test]
fn a_base_that_is_not_installed() {
    let f = Fixture::new();
    assert_eq!(
        run(&f, &["virtualenv", "9.9.9", "v"]),
        (String::new(),
         "pyenv-virtualenv: `9.9.9' is not installed in pyenv.\nIt does not look like a valid Python version. See `pyenv install --list' for available versions.\n".into(),
         1)
    );
}

/// allowlist D-97: `-u` alone reruns venv with `--upgrade`; the env's own link isn't "taken".
#[test]
fn upgrade_implies_force() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    run(&f, &["virtualenv", "3.12.1", "venv1"]);
    assert_eq!(run(&f, &["virtualenv", "-u", "3.12.1", "venv1"]).2, 0);
    let env = base.join("envs/venv1");
    assert!(calls(&base).ends_with(&format!("venv --upgrade {}\n", env.display())));
}

/// allowlist D-97 and review focus 2: a name taken by another env's link, or by a real
/// version folder (even with -f), is refused, and the folder is kept.
#[test]
fn a_taken_name_is_refused() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    let other = fake_base(&f, "3.12.2");
    run(&f, &["virtualenv", "3.12.1", "venv1"]);
    let link = f.root.join("versions/venv1");
    assert_eq!(
        run(&f, &["virtualenv", "3.12.2", "venv1"]).1,
        format!("pyenv-virtualenv: `{}' already exists.\n", link.display())
    );
    assert_eq!(
        run(&f, &["virtualenv", "-f", "3.12.1", "3.12.2"]),
        (
            String::new(),
            format!("pyenv-virtualenv: `{}' already exists.\n", other.display()),
            1
        )
    );
    assert!(other.join("bin/python").is_file());
}

#[test]
fn an_existing_env_asks_first() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    run(&f, &["virtualenv", "3.12.1", "venv1"]);
    let env = base.join("envs/venv1");
    let (_, err, code) = run_stdin(&f, &["virtualenv", "3.12.1", "venv1"], "n\n");
    assert_eq!(
        (err.as_str(), code),
        (
            format!("pyenv-virtualenv: {} already exists\n", env.display()).as_str(),
            1
        )
    );
    assert_eq!(
        run_stdin(&f, &["virtualenv", "3.12.1", "venv1"], "yes\n").2,
        0
    );
}

/// Review focus 4: a failed venv leaves no env and no link; an env that was there before
/// `-f` is kept.
#[test]
fn a_failed_venv_cleans_up() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let (out, _, code) = run_env(
        &f,
        &["virtualenv", "3.12.1", "bad"],
        &[("FAKE_VENV_FAIL", "1")],
    );
    assert_eq!((out.as_str(), code), ("venv failed\n", 5));
    assert!(!base.join("envs/bad").exists());
    assert!(std::fs::symlink_metadata(f.root.join("versions/bad")).is_err());
    run(&f, &["virtualenv", "3.12.1", "keep"]);
    assert_eq!(
        run_env(
            &f,
            &["virtualenv", "-f", "3.12.1", "keep"],
            &[("FAKE_VENV_FAIL", "1")]
        )
        .2,
        5
    );
    assert!(base.join("envs/keep/pyvenv.cfg").is_file());
}

/// allowlist D-96: `-p` takes the next word, and that interpreter runs `-m venv`.
#[test]
fn dash_p_takes_the_next_word() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let other = fake_base(&f, "3.13.1");
    let py = other.join("bin/python");
    assert_eq!(
        run(
            &f,
            &["virtualenv", "3.12.1", "-p", py.to_str().unwrap(), "pv"]
        )
        .2,
        0
    );
    assert!(calls(&other).contains(&base.join("envs/pv").display().to_string()));
    assert_eq!(calls(&base), "");
}

/// allowlist D-99: pip is ensured when venv left it out, unless declined.
#[test]
fn pip_is_ensured_unless_declined() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    assert_eq!(
        run_env(&f, &["virtualenv", "3.12.1", "e1"], &[("FAKE_NO_PIP", "1")]).2,
        0
    );
    assert!(base.join("envs/e1/bin/pip").is_file());
    assert!(calls(&base.join("envs/e1")).contains("ensurepip"));
    assert_eq!(
        run(&f, &["virtualenv", "--without-pip", "3.12.1", "e2"]).2,
        0
    );
    assert!(!base.join("envs/e2/bin/pip").exists());
}

/// allowlist D-98: an env made from `system` lives in `versions/<name>`, with no self-link.
#[test]
fn a_system_env_has_no_self_link() {
    let f = Fixture::new();
    exe(&f, &f.syspath.join("python3"), FAKE_PYTHON);
    exe(&f, &f.syspath.join("python"), FAKE_PYTHON);
    assert_eq!(run(&f, &["virtualenv", "system", "sysvenv"]).2, 0);
    let env = f.root.join("versions/sysvenv");
    assert!(env.join("pyvenv.cfg").is_file());
    assert!(std::fs::symlink_metadata(env.join("sysvenv")).is_err());
}

#[test]
fn version_help_and_completion() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    assert_eq!(
        run_env(
            &f,
            &["virtualenv", "--version"],
            &[("PYENV_VERSION", "3.12.1")]
        )
        .0,
        "pyenv-virtualenv 1.4.0 (python -m venv)\n"
    );
    let help = run(&f, &["virtualenv", "--help"]);
    assert_eq!((help.0.lines().next(), help.2),
               (Some("Usage: pyenv virtualenv [-f|--force] [VIRTUALENV_OPTIONS] [version] <virtualenv-name>"), 0));
    assert_eq!(
        run(&f, &["completions", "virtualenv"]).0,
        "--help\n3.12.1\n"
    );
}
