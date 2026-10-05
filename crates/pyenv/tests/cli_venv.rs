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
  if [ -n "$FAKE_VENV_SLEEP" ]; then sleep 30; fi
  ln -sf "$here/bin/python" "$dir/bin/python"
  : > "$dir/bin/activate"
  case " $* " in *" --without-pip "*) ;; *) [ -z "$FAKE_NO_PIP" ] && { printf '#!/bin/sh\n' > "$dir/bin/pip"; chmod 755 "$dir/bin/pip"; } ;; esac
  exit 0
fi
if [ "$1" = "-s" ] && [ "$2" = "-m" ] && [ "$3" = "ensurepip" ]; then
  echo "ensurepip" >> "$here/calls.log"
  [ -n "$FAKE_ENSUREPIP_FAIL" ] && exit 1
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

/// An env made on disk (no `pyenv virtualenv`): `versions/<base>/envs/<name>` and its link.
fn make_env(f: &Fixture, base: &str, name: &str, ssp: bool) -> PathBuf {
    let b = fake_base(f, base);
    let e = b.join("envs").join(name);
    std::fs::create_dir_all(e.join("bin")).unwrap();
    std::os::unix::fs::symlink(b.join("bin/python"), e.join("bin/python")).unwrap();
    std::fs::write(e.join("bin/activate"), "").unwrap();
    std::fs::write(
        e.join("pyvenv.cfg"),
        format!(
            "home = {}/bin\ninclude-system-site-packages = {ssp}\n",
            b.display()
        ),
    )
    .unwrap();
    let link = f.root.join("versions").join(name);
    if std::fs::symlink_metadata(&link).is_err() {
        std::os::unix::fs::symlink(&e, &link).unwrap();
    }
    e
}

#[test]
fn virtualenvs_lists_long_names_then_links() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    let b = f.root.join("versions/3.12.1");
    assert_eq!(
        run_env(&f, &["virtualenvs"], &[("PYENV_VERSION", "venv1")]).0,
        format!("  3.12.1/envs/venv1 (created from {})\n* venv1 --> {} (set by PYENV_VERSION environment variable)\n", b.display(), e1.display())
    );
    assert_eq!(
        run(&f, &["virtualenvs", "--bare"]).0,
        "3.12.1/envs/venv1\nvenv1\n"
    );
    assert_eq!(
        run(&f, &["virtualenvs", "--bare", "--skip-aliases"]).0,
        "3.12.1/envs/venv1\n"
    );
    assert_eq!(
        run(&f, &["virtualenvs", "--foo"]),
        (
            String::new(),
            "Usage: pyenv virtualenvs [--bare] [--skip-aliases]\n".into(),
            1
        )
    );
}

#[test]
fn virtualenv_prefix_as_upstream() {
    let f = Fixture::new();
    make_env(&f, "3.12.1", "venv1", false);
    make_env(&f, "3.12.1", "venv2", false);
    let b = f.root.join("versions/3.12.1").display().to_string();
    assert_eq!(run(&f, &["virtualenv-prefix", "venv1"]).0, format!("{b}\n"));
    assert_eq!(
        run(&f, &["virtualenv-prefix", "venv1", "venv2"]).0,
        format!("{b}:{b}\n")
    );
    assert_eq!(
        run(&f, &["virtualenv-prefix", "3.12.1"]),
        (
            String::new(),
            "pyenv-virtualenv: version `3.12.1' is not a virtualenv\n".into(),
            1
        )
    );
    assert_eq!(
        run(&f, &["virtualenv-prefix"]).1,
        "pyenv-virtualenv: version `system' is not a virtualenv\n"
    );
}

#[test]
fn delete_by_link_or_long_name_removes_both() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    let e2 = make_env(&f, "3.12.1", "venv2", false);
    assert_eq!(
        run(&f, &["virtualenv-delete", "-f", "venv1"]),
        (String::new(), String::new(), 0)
    );
    assert!(!e1.exists() && std::fs::symlink_metadata(f.root.join("versions/venv1")).is_err());
    assert_eq!(
        run(&f, &["virtualenv-delete", "-f", "3.12.1/envs/venv2"]).2,
        0
    );
    assert!(!e2.exists() && std::fs::symlink_metadata(f.root.join("versions/venv2")).is_err());
    assert!(f.root.join("versions/3.12.1/bin/python").is_file());
}

#[test]
fn delete_asks_unless_forced() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    assert_eq!(run_stdin(&f, &["virtualenv-delete", "venv1"], "n\n").2, 1);
    assert!(e1.exists());
    assert_eq!(run_stdin(&f, &["virtualenv-delete", "venv1"], "Yup\n").2, 0);
    assert!(!e1.exists());
}

/// Review focus 1, 2 and 3: an escaping name, a real version, and a link pointing outside are
/// never deleted.
#[test]
fn delete_refuses_what_is_not_an_env() {
    let f = Fixture::new();
    fake_base(&f, "3.12.1");
    let outside = f.base.join("outside");
    std::fs::create_dir_all(outside.join("bin")).unwrap();
    std::os::unix::fs::symlink(&outside, f.root.join("versions/mine")).unwrap();
    assert_eq!(
        run(&f, &["virtualenv-delete", "3.12.1"]).1,
        "pyenv-virtualenv: `3.12.1' is not a virtualenv.\n"
    );
    assert_eq!(
        run(&f, &["virtualenv-delete", "-f", "3.12.1"]),
        (String::new(), String::new(), 0)
    );
    assert!(f.root.join("versions/3.12.1/bin/python").is_file());
    let link = f.root.join("versions/mine");
    assert_eq!(
        run(&f, &["virtualenv-delete", "-f", "mine"]).1,
        format!(
            "pyenv-virtualenv: `{}' is a symlink for unknown location.\n",
            link.display()
        )
    );
    assert!(outside.join("bin").is_dir());
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "../../outside"]).2, 0);
    assert!(outside.join("bin").is_dir());
    assert_eq!(
        run(&f, &["virtualenv-delete", "3.12.1/envs/nosuch"]).1,
        "pyenv-virtualenv: virtualenv `nosuch' not installed\n"
    );
}

/// Spec §10 and the v1.4.0 uninstall hook: an env by link name goes with its link; a base goes
/// with its envs and their links; refusing one env stops the whole uninstall.
#[test]
fn uninstall_cascades() {
    let f = Fixture::new();
    let e1 = make_env(&f, "3.12.1", "venv1", false);
    assert_eq!(
        run(&f, &["uninstall", "-f", "venv1"]),
        (String::new(), String::new(), 0)
    );
    assert!(!e1.exists() && std::fs::symlink_metadata(f.root.join("versions/venv1")).is_err());
    let e2 = make_env(&f, "3.12.1", "a", false);
    let e3 = make_env(&f, "3.12.1", "b", false);
    let (_, _, code) = run_stdin(&f, &["uninstall", "3.12.1"], "y\nn\n");
    assert_eq!(code, 1);
    assert!(e2.exists() && e3.exists() && f.root.join("versions/3.12.1").is_dir());
    assert_eq!(
        run(&f, &["uninstall", "-f", "3.12.1"]).0,
        "pyenv: 3.12.1 uninstalled\n"
    );
    for n in ["3.12.1", "a", "b"] {
        assert!(
            std::fs::symlink_metadata(f.root.join("versions").join(n)).is_err(),
            "{n}"
        );
    }
}

fn bash(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut e = vec![("PYENV_SHELL", "bash")];
    e.extend_from_slice(env);
    run_env(f, args, &e)
}

const DEACTIVATE_TAIL: &str = "if [ -n \"${_OLD_VIRTUAL_PATH:-}\" ]; then\n  export PATH=\"${_OLD_VIRTUAL_PATH}\";\n  unset _OLD_VIRTUAL_PATH;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PYTHONHOME:-}\" ]; then\n  export PYTHONHOME=\"${_OLD_VIRTUAL_PYTHONHOME}\";\n  unset _OLD_VIRTUAL_PYTHONHOME;\nfi;\nif [ -n \"${_OLD_VIRTUAL_PS1:-}\" ]; then\n  export PS1=\"${_OLD_VIRTUAL_PS1}\";\n  unset _OLD_VIRTUAL_PS1;\nfi;\nif declare -f deactivate 1>/dev/null 2>&1; then\n  unset -f deactivate;\nfi;\n";

#[test]
fn sh_activate_prints_upstream_s_posix_code() {
    let f = Fixture::new();
    let e = make_env(&f, "3.12.1", "venv1", false);
    let p = e.display();
    let (out, err, code) = bash(&f, &["sh-activate", "venv1"], &[]);
    assert_eq!((err.as_str(), code), ("", 0));
    assert_eq!(out, format!(
        "unset PYENV_VIRTUAL_ENV;\nunset VIRTUAL_ENV;\n{DEACTIVATE_TAIL}export PYENV_VERSION=\"venv1\";\nexport PYENV_ACTIVATE_SHELL=1;\nexport PYENV_VIRTUAL_ENV=\"{p}\";\nexport VIRTUAL_ENV=\"{p}\";\nexport _OLD_VIRTUAL_PS1=\"${{PS1:-}}\";\nexport PS1=\"(venv1) ${{PS1:-}}\";\n"
    ));
}

#[test]
fn sh_activate_refusals() {
    let f = Fixture::new();
    let e = make_env(&f, "3.12.1", "venv1", false);
    make_env(&f, "3.12.1", "venv2", false);
    assert_eq!(
        bash(&f, &["sh-activate", "3.12.1"], &[]),
        (
            "false\n".into(),
            "pyenv-virtualenv: version `3.12.1' is not a virtualenv\n".into(),
            1
        )
    );
    assert_eq!(
        bash(&f, &["sh-activate", "--quiet", "3.12.1"], &[]),
        ("false\n".into(), String::new(), 1)
    );
    assert_eq!(
        bash(&f, &["sh-activate", "venv1", "venv2"], &[]).1,
        "pyenv-virtualenv: cannot activate multiple versions at once: venv1 venv2\n"
    );
    assert_eq!(
        bash(
            &f,
            &["sh-activate", "venv1"],
            &[("VIRTUAL_ENV", "/opt/other")]
        ),
        (
            "true\n".into(),
            "pyenv-virtualenv: virtualenv `/opt/other' is already activated\n".into(),
            0
        )
    );
    let p = e.display().to_string();
    assert_eq!(
        bash(
            &f,
            &["sh-activate", "venv1"],
            &[("VIRTUAL_ENV", &p), ("PYENV_VIRTUAL_ENV", &p)]
        ),
        (
            "true\n".into(),
            "pyenv-virtualenv: version `venv1' is already activated\n".into(),
            0
        )
    );
}

#[test]
fn sh_deactivate_as_upstream() {
    let f = Fixture::new();
    assert_eq!(
        bash(&f, &["sh-deactivate"], &[]),
        (
            "false\n".into(),
            "pyenv-virtualenv: no virtualenv has been activated.\n".into(),
            1
        )
    );
    let (out, _, code) = bash(
        &f,
        &["sh-deactivate"],
        &[("VIRTUAL_ENV", "/opt/other"), ("PYENV_ACTIVATE_SHELL", "1")],
    );
    assert_eq!(code, 0);
    assert_eq!(out, format!("unset PYENV_VERSION;\nunset PYENV_ACTIVATE_SHELL;\nunset PYENV_VIRTUAL_ENV;\nunset VIRTUAL_ENV;\n{DEACTIVATE_TAIL}"));
}

#[test]
fn activate_without_the_shell_function() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["activate", "venv1"]),
               (String::new(), "\u{1b}[31;1m\n`pyenv activate' requires Pyenv and Pyenv-Virtualenv to be loaded into your shell.\nCheck your shell configuration and Pyenv and Pyenv-Virtualenv installation instructions.\n\n\u{1b}[0m".into(), 1));
    make_env(&f, "3.12.1", "venv1", false);
    assert_eq!(
        run(&f, &["completions", "activate"]).0,
        "--help\n--unset\n3.12.1/envs/venv1\nvenv1\n"
    );
}

/// allowlist D-100: pwsh gets PowerShell, every value through ps_literal (review focus 5).
#[test]
fn pwsh_gets_powershell() {
    let f = Fixture::new();
    make_env(&f, "3.12.1", "v ñ", false);
    let (out, _, code) = run_env(&f, &["sh-activate", "v ñ"], &[("PYENV_SHELL", "pwsh")]);
    assert_eq!(code, 0);
    assert!(out.contains("$Env:VIRTUAL_ENV = "), "{out}");
    assert!(out.is_ascii(), "{out}");
    assert!(!out.contains("export "), "{out}");
}

/// A selected version that isn't installed stops the command with core's message, as upstream's
/// `pyenv-version-name` does under `set -e` (found while running the venv bats, Task 5).
#[test]
fn a_missing_current_version_is_reported_as_such() {
    let f = Fixture::new();
    let core = run_env(&f, &["version-name"], &[("PYENV_VERSION", "nosuch")]).1;
    assert!(core.contains("nosuch"), "{core}");
    for args in [
        &["sh-activate"][..],
        &["virtualenv-prefix"],
        &["virtualenv", "v"],
    ] {
        let (out, err, code) = run_env(
            &f,
            args,
            &[("PYENV_VERSION", "nosuch"), ("PYENV_SHELL", "bash")],
        );
        assert_eq!(
            (out.as_str(), err.as_str(), code),
            ("", core.as_str(), 1),
            "{args:?}"
        );
    }
}

#[test]
fn virtualenv_init_prints_the_hook_for_bash() {
    let f = Fixture::new();
    let (out, _, code) = run(&f, &["virtualenv-init", "-", "bash"]);
    assert_eq!(code, 0);
    let shims = f.root.join(".rpyenv/virtualenv/shims");
    assert!(out.starts_with(&format!("export PATH=\"{}:${{PATH}}\";\nexport PYENV_VIRTUALENV_INIT=1;\n_pyenv_virtualenv_hook() {{\n", shims.display())), "{out}");
    assert!(out.ends_with("if ! [[ \"${PROMPT_COMMAND-}\" =~ _pyenv_virtualenv_hook ]]; then\n  PROMPT_COMMAND=\"_pyenv_virtualenv_hook;${PROMPT_COMMAND-}\"\nfi\n"), "{out}");
    let act = std::fs::read_to_string(shims.join("activate")).unwrap();
    assert!(act.contains("eval \"$(pyenv sh-activate --verbose \"$@\" || true)\""));
    // ksh: only the two lines; `bash -` drops the shell name.
    assert_eq!(
        run(&f, &["virtualenv-init", "-", "ksh"]).0.lines().count(),
        2
    );
    assert_eq!(
        run(&f, &["virtualenv-init", "bash", "-"]).0.lines().count(),
        2
    );
}

#[test]
fn virtualenv_init_help_mode() {
    let f = Fixture::new();
    assert_eq!(run(&f, &["virtualenv-init", "zsh"]),
               (String::new(), "# Load pyenv-virtualenv automatically by adding\n# the following to ~/.zshrc:\n\neval \"$(pyenv virtualenv-init -)\"\n\n".into(), 1));
}

/// allowlist D-102: the helper scripts are never written through a symlinked folder.
#[test]
fn virtualenv_init_never_writes_through_a_link() {
    let f = Fixture::new();
    let mine = f.base.join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(f.root.join(".rpyenv")).unwrap();
    std::os::unix::fs::symlink(&mine, f.root.join(".rpyenv/virtualenv")).unwrap();
    assert_eq!(run(&f, &["virtualenv-init", "-", "bash"]).2, 0);
    assert_eq!(std::fs::read_dir(&mine).unwrap().count(), 0);
}

/// The host's real `python3`, for the one test that runs a real `python -m venv`. CI must have
/// one; a developer machine without it skips the test with a note.
fn real_python3() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("python3"))
            .find(|p| p.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI needs a python3 on PATH"
        );
        eprintln!("skipped: no python3 on PATH");
    }
    found
}

/// A real `python3 -m venv` (side-agent note, 2026-10-05: the other tests use a fake Python).
/// A `system` env made with `--without-pip --copies`: no network, and the interpreter is
/// copied, so nothing links to the real install. Then `exec`, `virtualenv-prefix`, uninstall.
#[test]
fn a_real_python_makes_a_working_env() {
    let Some(py) = real_python3() else {
        return;
    };
    let f = Fixture::new();
    let path =
        std::env::join_paths([f.syspath.clone(), py.parent().unwrap().to_path_buf()]).unwrap();
    let path = path.to_str().unwrap().to_string();
    let env = &[("PATH", path.as_str())];
    let r = f.pyenv_env(
        &[
            "virtualenv",
            "--without-pip",
            "--copies",
            "system",
            "realenv",
        ],
        env,
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let dir = f.root.join("versions/realenv");
    assert!(dir.join("pyvenv.cfg").is_file());
    let mut with_version = env.to_vec();
    with_version.push(("PYENV_VERSION", "realenv"));
    let r = f.pyenv_env(
        &["exec", "python", "-c", "import sys; print(sys.prefix)"],
        &with_version,
    );
    assert_eq!(
        (r.stdout.trim_end(), r.code),
        (dir.to_str().unwrap(), 0),
        "{}",
        r.stderr
    );
    let r = f.pyenv_env(&["virtualenv-prefix", "realenv"], env);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        PathBuf::from(r.stdout.trim_end()).join("bin").is_dir(),
        "{}",
        r.stdout
    );
    let r = f.pyenv_env(&["uninstall", "-f", "realenv"], env);
    assert_eq!(
        (r.stdout.as_str(), r.code),
        ("pyenv: realenv uninstalled\n", 0),
        "{}",
        r.stderr
    );
    assert!(!dir.exists());
}

/// Final review C1 (and M3): uninstalling a user's link into `versions/`, or a base whose
/// `envs` is a link, never follows it. Everything outside the root survives.
#[test]
fn uninstall_never_follows_a_users_link() {
    let f = Fixture::new();
    let outside = f.base.join("outside");
    f.file(&outside.join("conda/envs/userenv/data.txt"), "mine");
    std::os::unix::fs::symlink(outside.join("conda"), f.root.join("versions/conda")).unwrap();
    assert_eq!(run(&f, &["uninstall", "-f", "conda"]).2, 0);
    assert!(outside.join("conda/envs/userenv/data.txt").is_file());
    assert!(std::fs::symlink_metadata(f.root.join("versions/conda")).is_err());
    let base = fake_base(&f, "3.12.1");
    f.file(&outside.join("moved/e1/data.txt"), "mine");
    std::os::unix::fs::symlink(outside.join("moved"), base.join("envs")).unwrap();
    assert_eq!(run(&f, &["uninstall", "-f", "3.12.1"]).2, 0);
    assert!(outside.join("moved/e1/data.txt").is_file());
    assert!(!base.exists());
}

/// Final review C2 (allowlist D-97): a `system` env never lands on an installed version or
/// through a user's link, even with `-f`.
#[test]
fn a_system_env_never_lands_on_a_version_or_a_link() {
    let f = Fixture::new();
    exe(&f, &f.syspath.join("python3"), FAKE_PYTHON);
    exe(&f, &f.syspath.join("python"), FAKE_PYTHON);
    let v = fake_base(&f, "3.12.2");
    let (_, err, code) = run(&f, &["virtualenv", "-f", "system", "3.12.2"]);
    assert_eq!(
        (err, code),
        (
            format!("pyenv-virtualenv: `{}' already exists.\n", v.display()),
            1
        )
    );
    assert!(!v.join("pyvenv.cfg").exists());
    let outside = f.base.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, f.root.join("versions/mylink")).unwrap();
    assert_eq!(run(&f, &["virtualenv", "-f", "system", "mylink"]).2, 1);
    assert!(!outside.join("pyvenv.cfg").exists());
}

/// Final review I1: `virtualenv-delete` never deletes a conda install (it isn't a venv).
#[test]
fn virtualenv_delete_never_deletes_a_conda_install() {
    let f = Fixture::new();
    let conda = f.root.join("versions/miniforge3");
    exe(&f, &conda.join("bin/python"), "#!/bin/sh\n");
    exe(&f, &conda.join("bin/conda"), "#!/bin/sh\n");
    f.file(&conda.join("bin/activate"), "");
    assert_eq!(run(&f, &["virtualenv-delete", "-f", "miniforge3"]).2, 0);
    assert!(conda.join("bin/conda").is_file());
    assert_eq!(
        run(&f, &["virtualenv-delete", "miniforge3"]).1,
        "pyenv-virtualenv: `miniforge3' is not a virtualenv.\n"
    );
}

/// Final review I2: Ctrl+C while `python -m venv` runs leaves no half-made env, exit 130.
#[test]
fn ctrl_c_during_creation_cleans_up() {
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let mut child = f
        .command(
            Path::new(env!("CARGO_BIN_EXE_pyenv")),
            &f.work,
            &[("FAKE_VENV_SLEEP", "1")],
        )
        .args(["virtualenv", "3.12.1", "half"])
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let cfg = base.join("envs/half/pyvenv.cfg");
    let start = Instant::now();
    while !cfg.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "venv never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    common::sigint_group(child.id());
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break Some(st);
        }
        if start.elapsed() > Duration::from_secs(10) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert_eq!(status.and_then(|s| s.code()), Some(130));
    assert!(!base.join("envs/half").exists());
}

/// Final review I3: a link to an env that is gone is removed by `uninstall -f`.
#[test]
fn uninstall_removes_a_dangling_env_link() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    std::os::unix::fs::symlink(base.join("envs/gone"), f.root.join("versions/gone")).unwrap();
    assert_eq!(run(&f, &["uninstall", "-f", "gone"]).2, 0);
    assert!(std::fs::symlink_metadata(f.root.join("versions/gone")).is_err());
}

/// Final review M1 (allowlist D-99): when pip can't be ensured, a set GET_PIP_URL is named as
/// ignored, and nothing is downloaded.
#[test]
fn get_pip_url_is_ignored_with_a_message() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let (_, err, code) = run_env(
        &f,
        &["virtualenv", "3.12.1", "np"],
        &[
            ("FAKE_NO_PIP", "1"),
            ("FAKE_ENSUREPIP_FAIL", "1"),
            ("GET_PIP_URL", "https://example.invalid/get-pip.py"),
        ],
    );
    assert_eq!(code, 1);
    assert!(err.contains("GET_PIP_URL is ignored"), "{err}");
    assert!(!base.join("envs/np").exists());
}

/// Final review M2 (allowlist D-96): `-p <name>` falls back to the system Python, as upstream's
/// `pyenv-which` does.
#[test]
fn dash_p_finds_a_system_python() {
    let f = Fixture::new();
    let base = fake_base(&f, "3.12.1");
    let sys = f.base.join("sysroot");
    exe(&f, &sys.join("bin/python3"), FAKE_PYTHON);
    let path = std::env::join_paths([f.syspath.clone(), sys.join("bin")]).unwrap();
    let r = f.pyenv_env(
        &["virtualenv", "-p", "python3", "3.12.1", "pv2"],
        &[("PATH", path.to_str().unwrap())],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(calls(&sys).contains(&base.join("envs/pv2").display().to_string()));
}
