//! `pyenv install` and missing build libraries (M8 design §5): the routes, the prompt, the
//! install command and the lenient mode for scripts (D13). The fake CPython serves as in
//! `cli_install.rs`; detection is faked with RPYENV_TEST_PREFLIGHT and the package manager
//! with shell stubs on the fixture's PATH, so no test ever runs a real `sudo` or `apt-get`.
#![cfg(unix)]

mod common;

use common::fakebuild::tarball;
use common::server::{start, Reply, Server};
use common::Fixture;

const DEBIAN: &str =
    "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nID=debian\nVERSION_ID=\"12\"\n";

/// A plugin definition `3.12.99` serving the fake tarball.
fn plugin_def(f: &Fixture) -> Server {
    let body = tarball("3.12.99");
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("t");
    std::fs::write(&p, &body).unwrap();
    let sha = pyenv::install::checksum::sha256_file(&p).unwrap();
    let s = start(vec![("/Python-3.12.99.tar.gz", vec![Reply::Body(body)])]);
    f.file(
        &f.root.join("plugins/fake/share/python-build/3.12.99"),
        &format!(
            "install_package \"Python-3.12.99\" \"{}#{sha}\" standard verify_py312 ensurepip\n",
            s.url("/Python-3.12.99.tar.gz")
        ),
    );
    s
}

/// The environment of a run. No RPYENV_SKIP_PREFLIGHT: detection runs, with a Debian
/// os-release and apt package lists present; `extra` overrides.
fn env<'a>(f: &'a Fixture, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
    let os = f.base.join("os-release");
    f.file(&os, DEBIAN);
    let lists = f.base.join("lists");
    f.file(
        &lists.join("deb.debian.org_debian_dists_bookworm_main_binary-amd64_Packages"),
        "",
    );
    let mut v: Vec<(&str, String)> = vec![
        (
            "PATH",
            format!("{}:{}", f.syspath.display(), std::env::var("PATH").unwrap()),
        ),
        ("TMPDIR", f.base.join("tmp").display().to_string()),
        ("PYTHON_BUILD_SKIP_MIRROR", "1".into()),
        ("RPYENV_TEST_OS_RELEASE", os.display().to_string()),
        ("RPYENV_TEST_APT_LISTS", lists.display().to_string()),
    ];
    v.extend(extra.iter().map(|(k, val)| (*k, val.to_string())));
    v
}

fn run(f: &Fixture, args: &[&str], extra: &[(&str, &str)]) -> common::Run {
    let owned = env(f, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_env(args, &pairs)
}

fn run_input(f: &Fixture, args: &[&str], extra: &[(&str, &str)], input: &str) -> common::Run {
    let owned = env(f, extra);
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.pyenv_input(args, &pairs, input)
}

fn stub(f: &Fixture, name: &str, body: &str) {
    let p = f.syspath.join(name);
    f.file(&p, &format!("#!/bin/sh\n{body}\n"));
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn log_path(f: &Fixture) -> std::path::PathBuf {
    f.base.join("cmd.log")
}

/// What the stubs logged (empty when nothing ran).
fn log(f: &Fixture) -> String {
    std::fs::read_to_string(log_path(f)).unwrap_or_default()
}

/// `sudo`, `apt-get` and `id` stubs that only log; `id -u` answers 1000. `sudo` answers
/// `--version` and `-n true` itself, drops a leading `-n` before logging and `exec`ing the
/// command, and keeps the arguments it was given, `-n` included, in `sudo.raw`.
fn stubs(f: &Fixture, apt_exit: i32) {
    let l = log_path(f).display().to_string();
    let raw = f.base.join("sudo.raw").display().to_string();
    stub(
        f,
        "sudo",
        &format!(
            "case \"$1\" in --version) echo 'Sudo version 1.9.0'; exit 0;; esac\n\
             echo \"$*\" >> '{raw}'\n\
             [ \"$1\" = -n ] && shift\n\
             [ \"$1\" = true ] && exit 0\n\
             echo \"sudo $*\" >> '{l}'\n\
             exec \"$@\""
        ),
    );
    stub(
        f,
        "apt-get",
        &format!("echo \"apt-get $*\" >> '{l}'\nexit {apt_exit}"),
    );
    stub(f, "id", "echo 1000");
}

fn sudo_raw(f: &Fixture) -> String {
    std::fs::read_to_string(f.base.join("sudo.raw")).unwrap_or_default()
}

fn installed(f: &Fixture) -> bool {
    f.root.join("versions/3.12.99").is_dir()
}

#[test]
fn without_a_terminal_or_flag_openssl_still_stops_the_build() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    // Lenient mode (D13): OpenSSL is in the hard set.
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "openssl,libuuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: cannot build Python: missing build dependencies:\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("\n  sudo apt-get install libssl-dev uuid-dev\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("To build anyway, set RPYENV_SKIP_PREFLIGHT=1."),
        "{}",
        r.stderr
    );
    assert_eq!(log(&f), "", "no package manager ran");
    assert_eq!(sudo_raw(&f), "", "sudo was not even probed");
    assert!(!installed(&f));
}

#[test]
fn without_a_terminal_or_flag_other_libraries_only_warn_and_the_build_goes_on() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "tk,libuuid"),
            ("FAKE_PY_MISSING", "tkinter _uuid"),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(installed(&f), "{}", r.stderr);
    // The end summary (§5.2): the built interpreter really lacks both, and the run succeeds.
    assert!(
        r.stderr.contains(
            "pyenv: 3.12.99 was built without: tkinter, uuid
  Add them: sudo apt-get install tk-dev uuid-dev, then pyenv install -f 3.12.99
  In a later rpyenv release this stops the build; choose now with --deps=install or
  --deps=system (or RPYENV_BUILD_DEPS).
"
        ),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("pyenv: the build will lack these optional modules:\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("\n  sudo apt-get install tk-dev uuid-dev\n"),
        "{}",
        r.stderr
    );
    // Nothing is refused: no "cannot build", no way to skip the check.
    assert!(!r.stderr.contains("cannot build"), "{}", r.stderr);
    assert!(!r.stderr.contains("To build anyway"), "{}", r.stderr);
    assert_eq!(log(&f), "");
}

#[test]
fn a_build_missing_a_module_is_not_installed() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", ""), ("FAKE_PY_MISSING", "_uuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: 3.12.99 was built without: uuid (import _uuid failed; needs libuuid)"
        ),
        "{}",
        r.stderr
    );
    assert!(!installed(&f), "{}", r.stderr);
}

#[test]
fn skip_preflight_turns_the_failure_into_a_warning() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[("RPYENV_SKIP_PREFLIGHT", "1"), ("FAKE_PY_NO_CA", "1")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: warning: 3.12.99 was built without: ssl certificates (no CA file or directory; needs ca-certificates)"
        ),
        "{}",
        r.stderr
    );
    assert!(installed(&f), "{}", r.stderr);
}

#[test]
fn a_tcl_that_does_not_start_fails_the_check() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", ""), ("FAKE_PY_NO_TCL", "1")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("was built without: tkinter (Tcl doesn't start; needs tk)"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
}

#[test]
fn deps_system_without_a_terminal_makes_every_module_required() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "libuuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("  uuid ("), "{}", r.stderr);
    assert!(
        r.stderr.contains("\n  sudo apt-get install uuid-dev\n"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
    assert_eq!(log(&f), "");
}

#[test]
fn the_build_deps_variable_chooses_a_route_like_the_flag() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    // Without the variable this one-library run would be lenient and succeed.
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
            ("RPYENV_BUILD_DEPS", "system"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: cannot build Python: missing build dependencies:"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
}

#[test]
fn deps_install_without_a_terminal_runs_the_command_with_yes() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_PREFLIGHT_AFTER", ""),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: running: sudo -n apt-get install -y libssl-dev\n"),
        "{}",
        r.stderr
    );
    // The stub drops `-n` before logging (it asks `sudo -n true` first, which it answers).
    let l = log(&f);
    assert!(l.contains("sudo apt-get install -y libssl-dev\n"), "{l}");
    assert!(l.contains("apt-get install -y libssl-dev\n"), "{l}");
    assert!(!l.contains("update"), "the lists exist: {l}");
    let raw = sudo_raw(&f);
    assert!(
        raw.contains("-n apt-get install -y libssl-dev\n"),
        "sudo got -n: {raw}"
    );
    assert!(f.root.join("versions/3.12.99/bin/python3.12").is_file());
}

/// Review focus 1 (R4): minimal images have no package lists, so `apt-get update` runs first.
#[test]
fn empty_apt_lists_put_an_update_before_the_install() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let empty = f.base.join("nolists");
    std::fs::create_dir_all(&empty).unwrap();
    let empty = empty.display().to_string();
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_PREFLIGHT_AFTER", ""),
            ("RPYENV_TEST_APT_LISTS", &empty),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let sudo_lines: Vec<String> = log(&f)
        .lines()
        .filter(|l| l.starts_with("sudo "))
        .map(String::from)
        .collect();
    assert_eq!(
        sudo_lines,
        ["sudo apt-get update", "sudo apt-get install -y libssl-dev"]
    );
    assert!(installed(&f));
    // The printed command for the print route says the same.
    let r = run(
        &f,
        &["install", "--deps=system", "-f", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
            ("RPYENV_TEST_APT_LISTS", &empty),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("\n  sudo apt-get update && sudo apt-get install uuid-dev\n"),
        "{}",
        r.stderr
    );
}

#[test]
fn deps_install_needing_a_sudo_password_without_a_terminal_says_so() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    stub(
        &f,
        "sudo",
        "case \"$1\" in --version) echo 'Sudo version 1.9.0'; exit 0;; esac\n\
         echo 'sudo: a password is required' >&2; exit 1",
    );
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "openssl")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: can't install the packages: sudo needs a password and there is no terminal to ask it\n"
        ),
        "{}",
        r.stderr
    );
    assert_eq!(log(&f), "");
    assert!(!installed(&f));
}

/// Review focus 2.
#[test]
fn a_failed_install_reprompts_without_i_and_builds_nothing() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 100);
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
        ],
        "i\n\n",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert_eq!(
        r.stderr.matches("How should rpyenv get them?\n").count(),
        2,
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("  [i] install with sudo: sudo apt-get install libssl-dev\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("  [i] not offered: the last install command failed\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("pyenv: the install command failed\n"),
        "{}",
        r.stderr
    );
    let l = log(&f);
    assert!(l.contains("sudo apt-get install libssl-dev\n"), "{l}");
    assert!(!l.contains(" -y"), "no -y at a terminal: {l}");
    assert!(
        !sudo_raw(&f).contains("-n apt-get"),
        "no -n at a terminal: {}",
        sudo_raw(&f)
    );
    assert!(!installed(&f));
}

#[test]
fn at_a_terminal_enter_prints_and_stops() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
        ],
        "\n",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: 3.12.99 can't build these modules: ssl\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("  [p] print the commands and stop (default)\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("\n  sudo apt-get install libssl-dev\n"),
        "{}",
        r.stderr
    );
    assert_eq!(r.stderr.matches("How should rpyenv get them?").count(), 1);
    assert_eq!(log(&f), "");
    assert!(!installed(&f));
}

/// Review focus 3.
#[test]
fn eof_at_the_prompt_builds_nothing() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
        ],
        "",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains("How should rpyenv get them?"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
    assert_eq!(log(&f), "");
    // Ctrl+C itself (exit 130) is the shared `prompt` reader's, covered with the other
    // prompts; resolve_deps maps `Reply::Interrupted` to 130 without running anything.
}

#[test]
fn build_and_prebuilt_routes_say_they_come_later() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let p = [("RPYENV_TEST_PREFLIGHT", "libuuid")];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: the build route comes in a later rpyenv release\n"),
        "{}",
        r.stderr
    );
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
            ("RPYENV_BUILD_DEPS", "prebuilt"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: the prebuilt route comes in a later rpyenv release\n"),
        "{}",
        r.stderr
    );
    let r = run(&f, &["install", "--deps=bogus", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: --deps takes system, install, build or prebuilt\n"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
    assert_eq!(log(&f), "");
}

#[test]
fn not_root_and_no_sudo_leaves_i_out() {
    let f = Fixture::new();
    plugin_def(&f);
    // Only `id` is on PATH: no sudo.
    stub(&f, "id", "echo 1000");
    let sys = f.syspath.display().to_string();
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("PATH", &sys),
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
        ],
        "i\n\n",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains("  [i] not offered: no sudo on PATH\n"),
        "{}",
        r.stderr
    );
    // The `i` answer is refused and the prompt shows again; Enter then prints and stops.
    assert!(
        r.stderr.contains("pyenv: answer p\n"),
        "answer p only: {}",
        r.stderr
    );
    assert!(!r.stderr.contains("answer p or i"), "{}", r.stderr);
    assert!(r.stderr.contains("Install them with:"), "{}", r.stderr);
    assert!(!installed(&f));
}

/// Review focus 5.
#[test]
fn a_python2_definition_keeps_todays_behavior() {
    let f = Fixture::new();
    f.file(
        &f.root.join("plugins/fake/share/python-build/2.7.99"),
        &format!(
            "install_package \"Python-2.7.99\" \"http://127.0.0.1:9/Python-2.7.99.tgz#{}\" standard verify_py27\n",
            "0".repeat(64)
        ),
    );
    stub(&f, "id", "echo 1000");
    // No compiler on PATH: the old check refuses with its own wording. The new detection
    // (which this hook feeds) would list `ssl` instead.
    let sys = f.syspath.display().to_string();
    let r = run(
        &f,
        &["install", "2.7.99"],
        &[("PATH", &sys), ("RPYENV_TEST_PREFLIGHT", "openssl")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: cannot build Python: missing build dependencies:\n"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("  C compiler"), "{}", r.stderr);
    assert!(!r.stderr.contains("  ssl ("), "{}", r.stderr);
    assert!(
        !r.stderr.contains("can't build these modules"),
        "{}",
        r.stderr
    );
}

/// Ruling R9: an explicit route wins at a terminal; nothing is asked.
#[test]
fn an_explicit_route_at_a_terminal_does_not_prompt() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run_input(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ],
        "",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        !r.stderr.contains("How should rpyenv get them?"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("pyenv: cannot build Python: missing build dependencies:\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("\n  sudo apt-get install uuid-dev\n"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
    assert_eq!(log(&f), "");
}

#[test]
fn a_successful_install_at_a_terminal_then_builds() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_PREFLIGHT_AFTER", ""),
        ],
        "i\n",
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(f.root.join("versions/3.12.99/bin/python3.12").is_file());
    let l = log(&f);
    assert_eq!(
        l.lines().collect::<Vec<_>>(),
        [
            "sudo apt-get install libssl-dev",
            "apt-get install libssl-dev"
        ]
    );
    assert!(!sudo_raw(&f).contains("-n apt-get"), "{}", sudo_raw(&f));
    assert_eq!(r.stderr.matches("How should rpyenv get them?").count(), 1);
}

#[test]
fn an_install_that_leaves_items_missing_ends_without_looping() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    // At a terminal: the second prompt has no `i`; the second `i` is refused; Enter stops.
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_PREFLIGHT_AFTER", "libuuid"),
        ],
        "i\ni\n\n",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert_eq!(r.stderr.matches("How should rpyenv get them?").count(), 2);
    assert!(
        r.stderr
            .contains("  [i] not offered: the last install left these missing\n"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("pyenv: answer p\n"), "{}", r.stderr);
    assert_eq!(log(&f).matches("apt-get install").count(), 2, "one install");
    assert!(!installed(&f));
    // Without a terminal: exactly one install command, then it stops.
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_PREFLIGHT_AFTER", "libuuid"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: can't install the packages: the last install left these missing\n"),
        "{}",
        r.stderr
    );
    assert_eq!(
        r.stderr.matches("pyenv: running:").count(),
        1,
        "{}",
        r.stderr
    );
    assert_eq!(
        log(&f).matches("apt-get install").count(),
        2,
        "sudo line and apt line"
    );
    assert!(!installed(&f));
}

#[test]
fn a_failed_apt_update_stops_the_install() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let l = log_path(&f).display().to_string();
    stub(
        &f,
        "apt-get",
        &format!("echo \"apt-get $*\" >> '{l}'\n[ \"$1\" = update ] && exit 100\nexit 0"),
    );
    let empty = f.base.join("nolists");
    std::fs::create_dir_all(&empty).unwrap();
    let empty = empty.display().to_string();
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_APT_LISTS", &empty),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    let out = log(&f);
    assert!(out.contains("apt-get update\n"), "{out}");
    assert!(!out.contains("install"), "{out}");
    assert!(
        r.stderr.contains("pyenv: the install command failed\n"),
        "{}",
        r.stderr
    );
    // The update was part of the command, so the hint to run it is not repeated.
    assert!(
        !r.stderr.contains("run sudo apt-get update first"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
}

#[test]
fn root_without_a_terminal_installs_without_sudo() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    stub(&f, "id", "echo 0");
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
            ("RPYENV_TEST_PREFLIGHT_AFTER", ""),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let l = log(&f);
    assert_eq!(
        l.lines().collect::<Vec<_>>(),
        ["apt-get install -y libssl-dev"]
    );
    assert!(!l.contains("sudo"), "{l}");
    assert!(installed(&f));
}

#[test]
fn an_interrupt_during_the_install_command_ends_the_run_with_130() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    // The package manager logs its line, then runs long enough to be interrupted.
    let l = log_path(&f).display().to_string();
    stub(
        &f,
        "apt-get",
        &format!(
            "echo \"apt-get $*\" >> '{l}'
sleep 30"
        ),
    );
    let owned = env(&f, &[("RPYENV_TEST_PREFLIGHT", "openssl")]);
    let mut cmd = f.command(
        std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
        &f.work,
        &[],
    );
    for (k, v) in &owned {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["install", "--deps=install", "3.12.99"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut { stderr }, &mut s).ok();
        s
    });
    let start = Instant::now();
    while !log(&f).contains("apt-get") {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the stub never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    common::sigint_group(child.id());
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("pyenv did not stop after SIGINT");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let err = reader.join().unwrap();
    assert_eq!(status.code(), Some(130), "{err}");
    assert!(!err.contains("install command failed"), "{err}");
    assert!(!installed(&f), "{err}");
}
