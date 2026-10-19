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

/// A fake `libuuid` source tarball and a catalog naming it by `file://` URL. Its `configure`
/// counts its runs in `<base>/uuid-configure-runs`, sleeps `FAKE_LIB_SLEEP` seconds and fails
/// when `FAKE_LIB_FAIL` is set; `make install` honors DESTDIR.
const UUID_CONFIGURE: &str = r#"#!/bin/sh
echo run >> 'RUNS'
sleep "${FAKE_LIB_SLEEP:-0}"
[ -n "$FAKE_LIB_FAIL" ] && { echo 'configure: error: fake failure'; exit 1; }
for a in "$@"; do case "$a" in --prefix=*) echo "${a#--prefix=}" > prefix.txt;; esac; done
printf 'all:\n\ttrue\ninstall:\n\tsh ./install.sh "$(DESTDIR)"\n' > Makefile
"#;

const UUID_INSTALL: &str = r#"p="$1$(cat prefix.txt)"
mkdir -p "$p/include/uuid" "$p/lib"
echo uuid > "$p/include/uuid/uuid.h"
echo a > "$p/lib/libuuid.a"
"#;

fn fake_catalog(f: &Fixture) -> std::path::PathBuf {
    let runs = f.base.join("uuid-configure-runs");
    let configure = UUID_CONFIGURE.replace("RUNS", &runs.display().to_string());
    let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    for (name, body, mode) in [
        ("configure", configure.as_str(), 0o755),
        ("install.sh", UUID_INSTALL, 0o644),
    ] {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(mode);
        b.append_data(&mut h, format!("util-linux-9.9/{name}"), body.as_bytes())
            .unwrap();
    }
    let gz = b.into_inner().unwrap().finish().unwrap();
    let tarball = f.base.join("util-linux-9.9.tar.gz");
    std::fs::write(&tarball, &gz).unwrap();
    let sha = pyenv::install::checksum::sha256_file(&tarball).unwrap();
    let cat = f.base.join("catalog.tsv");
    f.file(
        &cat,
        &format!("libuuid\t9.9\tfile://{}\t{sha}\n", tarball.display()),
    );
    cat
}

fn uuid_runs(f: &Fixture) -> usize {
    std::fs::read_to_string(f.base.join("uuid-configure-runs"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// The store's entries (directory names under `cache/deps`, without `.locks`).
fn store_entries(f: &Fixture) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(f.root.join("cache/deps"))
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.retain(|n| n != ".locks");
    v.sort();
    v
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
fn without_a_terminal_or_flag_a_missing_awk_stops_the_build_naming_gawk() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    // Lenient mode (D13): awk is in the hard set (CPython's configure runs it).
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "awk")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "
  awk (awk)
"
        ),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains(
            "
  sudo apt-get install gawk
"
        ),
        "{}",
        r.stderr
    );
    assert_eq!(log(&f), "", "no package manager ran");
    assert!(!installed(&f));
}

// allowlist D-117
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
  In a later rpyenv release this stops the build; choose now with --deps=install,
  --deps=build or --deps=system (or RPYENV_BUILD_DEPS).
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

// allowlist D-122
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

// allowlist D-122
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
            "pyenv: warning: 3.12.99 was built without: ssl certificates (no CA file /fake/ssl/cert.pem and no certificates in /fake/ssl/certs; needs ca-certificates, or set SSL_CERT_FILE or SSL_CERT_DIR)"
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

// allowlist D-117
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

// allowlist D-120
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

// allowlist D-120
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
fn the_prebuilt_route_says_it_comes_later() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let p = [("RPYENV_TEST_PREFLIGHT", "libuuid")];
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

/// M8a follow-up: a user who may not use sudo, with only an optional library missing, can
/// build without it. `[c]` builds; the run ends with the summary, without the notice that
/// is meant for scripts.
#[test]
fn without_sudo_c_builds_without_the_optional_libraries() {
    let f = Fixture::new();
    plugin_def(&f);
    stub(&f, "id", "echo 1000");
    // A sudo that refuses this user: `[i]` is left out (and the real sudo never runs).
    stub(
        &f,
        "sudo",
        "case \"$1\" in --version) echo 'Sudo version 1.9.0'; exit 0;; esac\n\
         echo 'Sorry, user jm may not run sudo on box.' >&2; exit 1",
    );
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "tk"),
            ("FAKE_PY_MISSING", "tkinter"),
        ],
        "c\n",
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("  [i] not offered: sudo says this user may not use it\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("  [c] continue without them (the build warns about them at the end)\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("pyenv: 3.12.99 was built without: tkinter\n"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("  Add them: sudo apt-get install tk-dev, then pyenv install -f 3.12.99"),
        "{}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("In a later rpyenv release"),
        "{}",
        r.stderr
    );
    assert!(installed(&f));
}

/// `[c]` is not offered when a library from the hard set is missing; `c` is refused.
#[test]
fn c_is_refused_when_openssl_is_missing() {
    let f = Fixture::new();
    plugin_def(&f);
    stub(&f, "id", "echo 1000");
    let sys = f.syspath.display().to_string();
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("PATH", &sys),
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_PREFLIGHT", "openssl,tk"),
        ],
        "c\n\n",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(!r.stderr.contains("[c]"), "{}", r.stderr);
    assert!(r.stderr.contains("pyenv: answer p\n"), "{}", r.stderr);
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
    // uuid is optional, so `[c]` is offered alongside `p` (M8a follow-up).
    assert!(r.stderr.contains("pyenv: answer p or c\n"), "{}", r.stderr);
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
    // pyenv starts the stubs this test just wrote; another test's fork may still hold one
    // open (ETXTBSY: "Text file busy", as in `Fixture::run`). Such an attempt ends before the
    // stub logs anything: retry it, up to 50 times.
    let mut attempt = 0;
    let (mut child, reader) = loop {
        attempt += 1;
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
        let exited = loop {
            // The apt-get stub's own line: the sudo stub logs `sudo apt-get …` before its
            // `exec`, which can still fail with ETXTBSY.
            if log(&f).lines().any(|l| l.starts_with("apt-get ")) {
                break false;
            }
            if child.try_wait().unwrap().is_some() {
                break true;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the stub never started"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        if !exited {
            break (child, reader);
        }
        let err = reader.join().unwrap();
        assert!(
            err.contains("Text file busy") && attempt < 50,
            "pyenv ended before the stub started (attempt {attempt}): {err}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
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

// allowlist D-121
#[test]
fn help_and_completion_list_the_deps_option() {
    let f = Fixture::new();
    let h = run(&f, &["install", "--help"], &[]);
    assert!(h.stdout.contains("--deps=<route>"), "{}", h.stdout);
    let c = run(&f, &["install", "--complete"], &[]);
    assert!(c.stdout.lines().any(|l| l == "--deps="), "{}", c.stdout);
}

const ROCKY: &str =
    "NAME=\"Rocky Linux\"\nID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.6\"\n";
const CRB: &str = "Some packages live in the CRB repository; enable it first: sudo dnf install dnf-plugins-core && sudo dnf config-manager --set-enabled crb";

/// A Rocky 9 os-release for RPYENV_TEST_OS_RELEASE, and a `dnf` stub that logs and exits
/// `dnf_exit`.
fn rocky(f: &Fixture, dnf_exit: i32) -> String {
    let os = f.base.join("os-release-rocky");
    f.file(&os, ROCKY);
    let l = log_path(f).display().to_string();
    stub(
        f,
        "dnf",
        &format!("echo \"dnf $*\" >> '{l}'\nexit {dnf_exit}"),
    );
    os.display().to_string()
}

/// Ruling R15: on a RHEL-family system the printed report names CRB before the dnf command,
/// for `--deps=system` and in lenient mode alike.
// allowlist D-120
#[test]
fn rocky_reports_show_the_crb_line_before_the_command() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let os = rocky(&f, 0);
    let want = format!("\n{CRB}\nInstall them with:\n  sudo dnf install libuuid-devel\n");
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[
            ("RPYENV_TEST_OS_RELEASE", &os),
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains(&want), "{}", r.stderr);
    // Lenient mode: OpenSSL stops the build; the report still names CRB.
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_OS_RELEASE", &os),
            ("RPYENV_TEST_PREFLIGHT", "openssl"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "\n{CRB}\nInstall them with:\n  sudo dnf install openssl-devel\n"
        )),
        "{}",
        r.stderr
    );
    assert_eq!(log(&f), "", "nothing ran");
}

/// Ruling R15: the prompt shows the CRB line, and a failed install does not repeat it.
#[test]
fn rocky_prompt_shows_crb_once_per_prompt() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let os = rocky(&f, 1);
    let r = run_input(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_TTY", "1"),
            ("RPYENV_TEST_OS_RELEASE", &os),
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ],
        "i\n\n",
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains(&format!("{CRB}\nHow should rpyenv get them?\n")),
        "{}",
        r.stderr
    );
    // Not repeated after the failure: the next prompt follows at once (it shows it again).
    assert!(
        r.stderr.contains(
            "pyenv: the install command failed\npyenv: 3.12.99 can't build these modules: uuid\n"
        ),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("pyenv: the install command failed\n"),
        "{}",
        r.stderr
    );
    assert!(
        log(&f).contains("dnf install libuuid-devel\n"),
        "{}",
        log(&f)
    );
}

/// Ruling R15: `--deps=install` shows no prompt, so the CRB line comes once, after the failure.
#[test]
fn rocky_deps_install_shows_crb_after_a_failure() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let os = rocky(&f, 1);
    let r = run(
        &f,
        &["install", "--deps=install", "3.12.99"],
        &[
            ("RPYENV_TEST_OS_RELEASE", &os),
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains(&format!("pyenv: the install command failed\n{CRB}\n")),
        "{}",
        r.stderr
    );
    assert_eq!(r.stderr.matches(CRB).count(), 1, "{}", r.stderr);
    assert!(!installed(&f));
}

/// Final review M7: RPYENV_SKIP_PREFLIGHT=1 in lenient mode gives P1's warning line, not the
/// summary that promises a later release will stop the build.
// allowlist D-122
#[test]
fn skip_preflight_in_lenient_mode_warns_without_the_summary() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[("RPYENV_SKIP_PREFLIGHT", "1"), ("FAKE_PY_MISSING", "_uuid")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: warning: 3.12.99 was built without: uuid (import _uuid failed; needs libuuid)\n"
        ),
        "{}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("In a later rpyenv release"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("Add them:"), "{}", r.stderr);
    assert!(installed(&f));
}

/// Final review M8: the lenient mode is rpyenv's to set; a user's RPYENV_DEPS_MODE is dropped.
#[test]
fn a_users_deps_mode_does_not_make_a_strict_run_lenient() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[
            ("RPYENV_DEPS_MODE", "lenient"),
            ("RPYENV_TEST_PREFLIGHT", ""),
            ("FAKE_PY_MISSING", "_uuid"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: 3.12.99 was built without: uuid (import _uuid failed; needs libuuid)\n"
        ),
        "{}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("In a later rpyenv release"),
        "{}",
        r.stderr
    );
    assert!(!installed(&f));
}

/// Final review M10: a bad RPYENV_BUILD_DEPS names the variable, not the flag.
#[test]
fn a_bad_build_deps_value_names_the_variable() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
            ("RPYENV_BUILD_DEPS", "yes"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: RPYENV_BUILD_DEPS takes system, install, build or prebuilt\n"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("--deps takes"), "{}", r.stderr);
    assert!(!installed(&f));
}

/// Final review M1: Ctrl+C while detection runs its compilers ends the run with 130, before
/// any prompt, report or install command. The compiler stub sleeps on its first run and
/// fails at once after the interrupt, so detection would report it missing.
#[test]
fn an_interrupt_during_detection_ends_the_run_before_any_install() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    // A stub that fails to start (ETXTBSY, another test's fork holding it) never logs; such
    // an attempt is retried with a fresh fixture.
    for attempt in 1..=20 {
        let f = Fixture::new();
        plugin_def(&f);
        stubs(&f, 0);
        let marker = f.base.join("interrupted");
        let cclog = f.base.join("cc.log");
        stub(
            &f,
            "slowcc",
            &format!(
                "[ -e '{}' ] && exit 1\necho started >> '{}'\nexec sleep 30",
                marker.display(),
                cclog.display()
            ),
        );
        // By name, found on PATH: the fixture's path has spaces, and `$CC` is split into words.
        let owned = env(&f, &[("CC", "slowcc")]);
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
        let started = loop {
            if cclog.exists() {
                break true;
            }
            if child.try_wait().unwrap().is_some() {
                break false;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the compiler stub never started"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        if !started {
            let err = reader.join().unwrap();
            assert!(
                attempt < 20,
                "the compiler stub never started in 20 attempts: {err}"
            );
            continue;
        }
        f.file(&marker, "");
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
        assert!(!err.contains("pyenv: running:"), "{err}");
        assert!(!err.contains("missing build dependencies"), "{err}");
        assert_eq!(log(&f), "", "no package manager ran: {err}");
        assert!(!installed(&f), "{err}");
        return;
    }
}

// allowlist D-123
#[test]
fn deps_build_compiles_the_missing_library_into_the_store_and_links_it() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f);
    let cat = cat.display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr.contains("Building libuuid 9.9...\n"),
        "{}",
        r.stderr
    );
    let entries = store_entries(&f);
    assert_eq!(entries.len(), 1, "only the missing library: {entries:?}");
    assert!(entries[0].starts_with("libuuid-9.9-"), "{entries:?}");
    let entry = f.root.join("cache/deps").join(&entries[0]);
    let config =
        std::fs::read_to_string(f.root.join("versions/3.12.99/lib/rpyenv-config.txt")).unwrap();
    assert!(
        config.contains(&format!("-I{}/include", entry.display())),
        "{config}"
    );
    assert!(
        config.contains(&format!("-L{}/lib", entry.display())),
        "{config}"
    );
    let record = std::fs::read_to_string(f.root.join("versions/3.12.99/.rpyenv-deps")).unwrap();
    assert!(
        record.contains("\ndefinition 3.12.99\nlibuuid 9.9 catalog\n"),
        "{record}"
    );
    assert_eq!(log(&f), "", "no package manager ran");
}

#[test]
fn a_second_build_reuses_the_store_entry() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    assert_eq!(run(&f, &["install", "--deps=build", "3.12.99"], &p).code, 0);
    let r = run(&f, &["install", "-f", "--deps=build", "3.12.99:second"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("Using libuuid 9.9 from "), "{}", r.stderr);
    assert!(!r.stderr.contains("Building libuuid"), "{}", r.stderr);
    assert_eq!(uuid_runs(&f), 1);
    assert_eq!(store_entries(&f).len(), 1);
}

// allowlist D-124
#[test]
fn at_a_terminal_b_builds_the_library_then_python() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("RPYENV_TEST_TTY", "1"),
    ];
    let r = run_input(&f, &["install", "3.12.99"], &p, "b\n");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("  [b] build them from source for rpyenv, no root\n"),
        "{}",
        r.stderr
    );
    assert_eq!(store_entries(&f).len(), 1);
    assert!(installed(&f));
}

#[test]
fn without_x11_headers_deps_build_refuses_tkinter_before_building_anything() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid,tk"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: --deps=build can't build tkinter here: needs the X11 headers (libx11-dev)\n"
        ),
        "{}",
        r.stderr
    );
    assert_eq!(uuid_runs(&f), 0, "nothing was built");
    assert!(store_entries(&f).is_empty());
    assert!(!installed(&f));
}

#[test]
fn at_a_terminal_b_builds_what_it_can_and_the_rest_comes_back_to_the_prompt() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid,tk"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("RPYENV_TEST_TTY", "1"),
    ];
    // `b` builds libuuid; tkinter is left, `[b]` is no longer offered, `c` builds Python.
    let r = run_input(&f, &["install", "3.12.99"], &p, "b\nc\n");
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("(not tkinter: needs the X11 headers, libx11-dev)"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("  [b] not offered: nothing missing can be built here\n"),
        "{}",
        r.stderr
    );
    assert_eq!(uuid_runs(&f), 1);
    assert!(installed(&f));
    let record = std::fs::read_to_string(f.root.join("versions/3.12.99/.rpyenv-deps")).unwrap();
    assert!(record.contains("libuuid 9.9 catalog"), "{record}");
}

#[test]
fn a_library_that_fails_to_build_names_its_log_and_installs_nothing() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("FAKE_LIB_FAIL", "1"),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: building libuuid 9.9 failed; the log is "),
        "{}",
        r.stderr
    );
    assert!(store_entries(&f).is_empty(), "{:?}", store_entries(&f));
    assert!(!installed(&f));
}

// Review Focus 4
#[test]
fn a_root_with_a_space_refuses_the_build_route_before_downloading() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let spaced = f.base.join("my root");
    // The definition must be found under this root too, or the run stops before detection.
    let def = "plugins/fake/share/python-build/3.12.99";
    std::fs::create_dir_all(spaced.join(def).parent().unwrap()).unwrap();
    std::fs::copy(f.root.join(def), spaced.join(def)).unwrap();
    let spaced_s = spaced.display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("PYENV_ROOT", spaced_s.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(&format!(
            "pyenv: --deps=build can't run: PYENV_ROOT has a space, a colon or a shell character in its path ({spaced_s})\n"
        )),
        "{}",
        r.stderr
    );
    assert_eq!(uuid_runs(&f), 0);
}

// Review Focus 2
#[test]
fn an_interrupt_during_a_library_build_leaves_no_store_entry() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let owned = env(
        &f,
        &[
            ("RPYENV_TEST_PREFLIGHT", "libuuid"),
            ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
            ("RPYENV_TEST_CATALOG", cat.as_str()),
            ("FAKE_LIB_SLEEP", "30"),
        ],
    );
    let mut cmd = f.command(
        std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
        &f.work,
        &[],
    );
    for (k, v) in &owned {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["install", "--deps=build", "3.12.99"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while uuid_runs(&f) == 0 {
        assert!(
            child.try_wait().unwrap().is_none(),
            "pyenv ended before configure ran"
        );
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "configure never ran"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    common::sigint_group(child.id());
    let status = child.wait().unwrap();
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert_eq!(status.code(), Some(130), "{err}");
    assert!(store_entries(&f).is_empty(), "{:?}", store_entries(&f));
    assert!(!installed(&f));
}

// allowlist D-125
#[test]
fn a_refusal_names_the_build_route_for_users_without_root() {
    // A plain root: the hint is left out where the build route would refuse (ruling P5).
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "libuuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("Without root, pyenv install --deps=build compiles them from source.\n"),
        "{}",
        r.stderr
    );
}

/// Rulings P2 and P5: the hint names `--deps=build` only where that route can run. CPython
/// 3.0-3.6 keep today's report exactly, and a `PYENV_ROOT` with a space gets no hint either.
#[test]
fn the_build_route_hint_is_left_out_where_the_route_cannot_run() {
    let f = Fixture::plain();
    stubs(&f, 0);
    // `plugin_def`'s shape, for a 3.6 definition; the run stops before any download.
    f.file(
        &f.root.join("plugins/fake/share/python-build/3.6.99"),
        &format!(
            "install_package \"Python-3.6.99\" \"http://127.0.0.1:9/Python-3.6.99.tar.gz#{}\" standard verify_py36 ensurepip\n",
            "0".repeat(64)
        ),
    );
    let r = run(
        &f,
        &["install", "--deps=system", "3.6.99"],
        &[("RPYENV_TEST_PREFLIGHT", "libuuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("pyenv: cannot build Python: missing build dependencies:\n"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("Without root"), "{}", r.stderr);
    // A root with a space (the standard fixture's): the build route would refuse.
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "--deps=system", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "libuuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr
            .contains("To build anyway, set RPYENV_SKIP_PREFLIGHT=1.\n"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("Without root"), "{}", r.stderr);
}

// allowlist D-126
#[test]
fn pyenv_install_warns_once_per_version_built_with_an_older_library() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    f.file(
        &f.root.join("versions/3.12.98/.rpyenv-deps"),
        "definition 3.12.98\nlibuuid 9.8 catalog\n",
    );
    f.file(
        &f.root.join("versions/3.12.97/.rpyenv-deps"),
        "definition 3.12.97\nlibuuid 9.9 catalog\n",
    );
    let p = [
        ("RPYENV_TEST_PREFLIGHT", ""),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    let r = run(&f, &["install", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let want = "pyenv: 3.12.98 was built with libuuid 9.8; rpyenv now builds libuuid 9.9. To rebuild it: pyenv install -f --deps=build 3.12.98\n";
    assert_eq!(r.stderr.matches(want).count(), 1, "{}", r.stderr);
    assert!(!r.stderr.contains("3.12.97 was built"), "{}", r.stderr);
    // Listing versions is not installing: no warning.
    let l = run(&f, &["install", "--list"], &p);
    assert!(!l.stderr.contains("was built with"), "{}", l.stderr);
}

// allowlist D-127
#[test]
fn pyenv_install_replaces_an_outdated_ca_bundle() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let pem = f.base.join("cacert-2099-01-01.pem");
    f.file(
        &pem,
        "-----BEGIN CERTIFICATE-----\nnew\n-----END CERTIFICATE-----\n",
    );
    let sha = pyenv::install::checksum::sha256_file(&pem).unwrap();
    let cat = f.base.join("catalog.tsv");
    f.file(
        &cat,
        &format!("cacert\t2099-01-01\tfile://{}\t{sha}\n", pem.display()),
    );
    let bundle = f.root.join(".rpyenv/ssl/cert.pem");
    f.file(&bundle, "old\n");
    let cat_s = cat.display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", ""),
        ("RPYENV_TEST_CATALOG", cat_s.as_str()),
        ("RPYENV_TEST_SYSTEM_CERTS", ""),
    ];
    let r = run(&f, &["install", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(
        std::fs::read_to_string(&bundle).unwrap(),
        std::fs::read_to_string(&pem).unwrap()
    );
    assert!(
        r.stderr.contains(&format!(
            "pyenv: updated the CA bundle in {} (2099-01-01)\n",
            bundle.display()
        )),
        "{}",
        r.stderr
    );
    // Up to date: nothing is fetched or said.
    let again = run(&f, &["install", "-f", "3.12.99"], &p);
    assert!(!again.stderr.contains("CA bundle"), "{}", again.stderr);
    // The system gains certificates: the next run links to them.
    let sys = f.base.join("system.crt");
    f.file(
        &sys, "system
",
    );
    let sys_s = format!("{}:", sys.display());
    let p2 = [
        ("RPYENV_TEST_PREFLIGHT", ""),
        ("RPYENV_TEST_CATALOG", cat_s.as_str()),
        ("RPYENV_TEST_SYSTEM_CERTS", sys_s.as_str()),
    ];
    let r = run(&f, &["install", "-f", "3.12.99"], &p2);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(std::fs::read_link(&bundle).unwrap(), sys.clone());
}

/// Final review S2: a library a definition pinned, whose definition can't be found any more,
/// is unknown, not "now the catalog's": no warning that would never go away.
#[test]
fn a_version_whose_definition_is_gone_gets_no_stale_warning() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = f.base.join("catalog.tsv");
    f.file(
        &cat,
        &format!("openssl\t3.6.5\tfile:///nonexistent\t{}\n", "a".repeat(64)),
    );
    f.file(
        &f.root.join("versions/old/.rpyenv-deps"),
        "definition 3.7.99-gone\nopenssl 1.1.1u definition\n",
    );
    // A control: the same library from the catalog does warn.
    f.file(
        &f.root.join("versions/other/.rpyenv-deps"),
        "definition 3.7.98-gone\nopenssl 3.6.4 catalog\n",
    );
    let cat_s = cat.display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", ""),
        ("RPYENV_TEST_CATALOG", cat_s.as_str()),
    ];
    let r = run(&f, &["install", "3.12.99"], &p);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains("old was built"), "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: other was built with openssl 3.6.4; rpyenv now builds openssl 3.6.5."
        ),
        "{}",
        r.stderr
    );
}

/// A version the build route built with Tcl/Tk `release`, with its own script copies.
fn tcltk_version(f: &Fixture, name: &str, release: &str) {
    for (d, marker) in [("tcl8.6", "init.tcl"), ("tk8.6", "tk.tcl")] {
        f.file(
            &f.root.join(format!("versions/{name}/lib/{d}/{marker}")),
            name,
        );
    }
    f.file(
        &f.root.join(format!("versions/{name}/.rpyenv-deps")),
        &format!("definition {name}\ntcl {release} catalog\ntk {release} catalog\n"),
    );
}

// allowlist D-123
/// Final review I1: `pyenv rehash` re-creates a deleted shared Tcl/Tk script folder, and
/// `pyenv uninstall` removes the folders no remaining version names.
#[test]
fn rehash_recreates_the_shared_tcltk_folders_and_uninstall_removes_unused_ones() {
    let f = Fixture::plain();
    tcltk_version(&f, "3.12.98", "8.6.18");
    tcltk_version(&f, "3.13.98", "8.6.17");
    let home = f.root.join(".rpyenv/tcltk");
    let r = f.pyenv(&["rehash"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    for (d, marker) in [
        ("tcl-8.6.18/tcl8.6", "init.tcl"),
        ("tk-8.6.18/tk8.6", "tk.tcl"),
        ("tcl-8.6.17/tcl8.6", "init.tcl"),
        ("tk-8.6.17/tk8.6", "tk.tcl"),
    ] {
        assert!(home.join(d).join(marker).is_file(), "{d}");
    }
    let r = f.pyenv(&["uninstall", "-f", "3.13.98"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let mut left: Vec<String> = std::fs::read_dir(&home)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["tcl-8.6.18", "tk-8.6.18"]);
}

// allowlist D-123
/// Final review I3: while another pyenv holds a library's lock, the run says it is waiting,
/// and Ctrl+C ends it at once with 130.
#[test]
fn ctrl_c_while_waiting_for_another_builders_lock_ends_the_run() {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let extra = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl,x11"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    assert_eq!(
        run(&f, &["install", "--deps=build", "3.12.99"], &extra).code,
        0
    );
    let locks = f.root.join("cache/deps/.locks");
    let lock_path = std::fs::read_dir(&locks)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let held = std::fs::File::create(&lock_path).unwrap();
    held.lock().unwrap();
    let owned = env(&f, &extra);
    let mut cmd = f.command(
        std::path::Path::new(env!("CARGO_BIN_EXE_pyenv")),
        &f.work,
        &[],
    );
    for (k, v) in &owned {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(["install", "-f", "--deps=build", "3.12.99"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        for l in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            let _ = tx.send(l);
        }
    });
    let want = format!(
        "Waiting for another pyenv building libuuid 9.9 ({})",
        lock_path.display()
    );
    let mut seen = Vec::new();
    while !seen.contains(&want) {
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(l) => seen.push(l),
            Err(_) => {
                let _ = child.kill();
                panic!("no waiting line: {seen:?}");
            }
        }
    }
    let signalled = Instant::now();
    common::sigint_group(child.id());
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            assert_eq!(st.code(), Some(130), "{seen:?}");
            break;
        }
        if signalled.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("pyenv did not stop after SIGINT while the lock was held");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(held);
    // The version from the first run is still there; the store is unchanged.
    assert!(installed(&f));
    assert_eq!(uuid_runs(&f), 1);
}
