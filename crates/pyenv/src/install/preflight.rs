//! The build pre-flight check (spec §9.2, plan Decision 8): a compiler, `make`, `patch` when
//! needed, and the headers of the modules python-build verifies. Missing required pieces
//! refuse the build before downloading; missing optional ones warn. Results are structured
//! for M8 (spec §15.2).

use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    Program(String),
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// What the user loses: `C compiler`, `make`, `patch`, or a module name such as `ssl`.
    pub name: &'static str,
    pub required: bool,
    pub missing: Missing,
}

pub enum Probe<'a> {
    Program(&'a str),
    /// Any one of these headers is enough.
    Headers(&'a [&'a str]),
}

/// (name, required, probe, apt, dnf, zypper, pacman, apk)
type Row = (&'static str, bool, Probe<'static>, [&'static str; 5]);

fn rows(compiler: &'static str, make: &'static str) -> Vec<Row> {
    vec![
        (
            "C compiler",
            true,
            Probe::Program(compiler),
            ["build-essential", "gcc", "gcc", "base-devel", "build-base"],
        ),
        (
            "make",
            true,
            Probe::Program(make),
            ["make", "make", "make", "make", "make"],
        ),
        (
            "patch",
            true,
            Probe::Program("patch"),
            ["patch", "patch", "patch", "patch", "patch"],
        ),
        (
            "ssl",
            true,
            Probe::Headers(&["openssl/ssl.h"]),
            [
                "libssl-dev",
                "openssl-devel",
                "libopenssl-devel",
                "openssl",
                "openssl-dev",
            ],
        ),
        (
            "zlib",
            true,
            Probe::Headers(&["zlib.h"]),
            ["zlib1g-dev", "zlib-devel", "zlib-devel", "zlib", "zlib-dev"],
        ),
        (
            "bz2",
            false,
            Probe::Headers(&["bzlib.h"]),
            [
                "libbz2-dev",
                "bzip2-devel",
                "libbz2-devel",
                "bzip2",
                "bzip2-dev",
            ],
        ),
        (
            "readline",
            false,
            Probe::Headers(&["readline/readline.h", "editline/readline.h"]),
            [
                "libreadline-dev",
                "readline-devel",
                "readline-devel",
                "readline",
                "readline-dev",
            ],
        ),
        (
            "sqlite3",
            false,
            Probe::Headers(&["sqlite3.h"]),
            [
                "libsqlite3-dev",
                "sqlite-devel",
                "sqlite3-devel",
                "sqlite",
                "sqlite-dev",
            ],
        ),
        (
            "ctypes",
            false,
            Probe::Headers(&["ffi.h"]),
            [
                "libffi-dev",
                "libffi-devel",
                "libffi-devel",
                "libffi",
                "libffi-dev",
            ],
        ),
        (
            "curses",
            false,
            Probe::Headers(&["ncurses.h", "curses.h"]),
            [
                "libncurses-dev",
                "ncurses-devel",
                "ncurses-devel",
                "ncurses",
                "ncurses-dev",
            ],
        ),
        (
            "lzma",
            false,
            Probe::Headers(&["lzma.h"]),
            ["liblzma-dev", "xz-devel", "xz-devel", "xz", "xz-dev"],
        ),
        (
            "tkinter",
            false,
            Probe::Headers(&["tk.h"]),
            ["tk-dev", "tk-devel", "tk-devel", "tk", "tk-dev"],
        ),
    ]
}

/// Which missing items there are, given a way to test each probe. `display`: tkinter is only
/// checked by upstream when `$DISPLAY` is set, so it is only probed then.
pub fn evaluate(present: &dyn Fn(&Probe) -> bool, needs_patch: bool, display: bool) -> Vec<Dep> {
    evaluate_with(present, needs_patch, display, "cc", "make")
}

fn evaluate_with(
    present: &dyn Fn(&Probe) -> bool,
    needs_patch: bool,
    display: bool,
    cc: &'static str,
    make: &'static str,
) -> Vec<Dep> {
    let mut out = Vec::new();
    let mut compiler_ok = true;
    for (name, required, probe, _) in rows(cc, make) {
        if (name == "patch" && !needs_patch) || (name == "tkinter" && !display) {
            continue;
        }
        // Without a compiler the headers can't be probed; report the compiler alone.
        if matches!(probe, Probe::Headers(_)) && !compiler_ok {
            continue;
        }
        if !present(&probe) {
            if name == "C compiler" {
                compiler_ok = false;
            }
            let missing = match probe {
                Probe::Program(p) => Missing::Program(p.to_string()),
                Probe::Headers(h) => Missing::Header(h[0].to_string()),
            };
            out.push(Dep {
                name,
                required,
                missing,
            });
        }
    }
    out
}

pub struct Report {
    pub refuse: bool,
    pub lines: Vec<String>,
}

fn manager(os_release: &str) -> Option<(usize, &'static str)> {
    let ids: Vec<String> = os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| {
            v.trim_matches('"')
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    let has = |names: &[&str]| ids.iter().any(|i| names.contains(&i.as_str()));
    if has(&["debian", "ubuntu"]) {
        Some((0, "apt-get install"))
    } else if has(&["fedora", "rhel", "centos"]) {
        Some((1, "dnf install"))
    } else if has(&["suse", "opensuse", "sles"]) || ids.iter().any(|i| i.starts_with("opensuse")) {
        Some((2, "zypper install"))
    } else if has(&["arch"]) {
        Some((3, "pacman -S"))
    } else if has(&["alpine"]) {
        Some((4, "apk add"))
    } else {
        None
    }
}

fn shown(d: &Dep) -> String {
    let what = match &d.missing {
        Missing::Program(p) => p.clone(),
        Missing::Header(h) => h.clone(),
    };
    format!("{} ({what})", d.name)
}

/// The text for stderr. `root`: no `sudo` in the suggested command.
pub fn report(missing: &[Dep], os_release: Option<&str>, root: bool) -> Report {
    let refuse = missing.iter().any(|d| d.required);
    let mut lines = Vec::new();
    if missing.is_empty() {
        return Report { refuse, lines };
    }
    if refuse {
        lines.push("pyenv: cannot build Python: missing build dependencies:".to_string());
    } else {
        lines.push("pyenv: the build will lack these optional modules:".to_string());
    }
    for d in missing {
        lines.push(format!("  {}", shown(d)));
    }
    if let Some((col, cmd)) = os_release.and_then(manager) {
        let mut pkgs: Vec<&str> = Vec::new();
        for d in missing {
            if let Some(row) = rows("cc", "make").into_iter().find(|r| r.0 == d.name) {
                if !pkgs.contains(&row.3[col]) {
                    pkgs.push(row.3[col]);
                }
            }
        }
        lines.push("Install them with:".to_string());
        lines.push(format!(
            "  {}{cmd} {}",
            if root { "" } else { "sudo " },
            pkgs.join(" ")
        ));
    }
    if refuse {
        lines.push("To build anyway, set RPYENV_SKIP_PREFLIGHT=1.".to_string());
    }
    Report { refuse, lines }
}

/// The compiler to use: `$CC` when set; otherwise `cc`, else `gcc` (as configure does).
fn resolve_compiler(configured: Option<String>, works: &dyn Fn(&str) -> bool) -> String {
    match configured.filter(|v| !v.is_empty()) {
        Some(cc) => cc,
        None if works("cc") => "cc".into(),
        None if works("gcc") => "gcc".into(),
        None => "cc".into(),
    }
}

/// True when one of `headers` compiles plainly or, for each extra flag set in turn, with it.
/// `try_header(header, extra_flags)` does the compiling.
fn headers_present(
    headers: &[&str],
    fallbacks: &dyn Fn() -> Vec<Vec<String>>,
    try_header: &dyn Fn(&str, &[String]) -> bool,
) -> bool {
    if headers.iter().any(|h| try_header(h, &[])) {
        return true;
    }
    fallbacks()
        .iter()
        .any(|extra| headers.iter().any(|h| try_header(h, extra)))
}

/// Where Tk's header may live when it is not on the default path: what `pkg-config --cflags tk`
/// says (as CPython's configure uses), else Debian's Tcl/Tk include directories.
fn tk_fallbacks(pkg_config: Option<String>) -> Vec<Vec<String>> {
    match pkg_config {
        Some(out) if !out.trim().is_empty() => {
            vec![out.split_whitespace().map(str::to_string).collect()]
        }
        _ => ["/usr/include/tcl8.6", "/usr/include/tcl8.5"]
            .iter()
            .map(|d| vec![format!("-I{d}")])
            .collect(),
    }
}

/// Probes the real system: `$CC` (else `cc`, else `gcc`), `$MAKE` (else `make`), and each
/// header through `<cc> -E` with `CPPFLAGS` and `PYTHON_CPPFLAGS`. Tk's header is also tried
/// with the flags `pkg-config --cflags tk` gives.
pub fn check(env: &dyn Fn(&str) -> Option<String>, needs_patch: bool) -> Vec<Dep> {
    let make = env("MAKE")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "make".into());
    let flags: Vec<String> = [env("CPPFLAGS"), env("PYTHON_CPPFLAGS")]
        .into_iter()
        .flatten()
        .flat_map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect();
    let path = env("PATH");
    // `$CC` may carry words (`ccache gcc`): the first is the program, the rest lead the args.
    let spawn = |program: &str,
                 args: &[&str],
                 input: Option<&str>,
                 capture: bool|
     -> Option<(bool, Vec<u8>)> {
        let mut words = program.split_whitespace();
        let first = words.next()?;
        let mut c = Command::new(first);
        c.args(words);
        if let Some(p) = &path {
            c.env("PATH", p);
        }
        c.args(args)
            .stdout(if capture {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null());
        c.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = c.spawn().ok()?;
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        let out = child.wait_with_output().ok()?;
        Some((out.status.success(), out.stdout))
    };
    let runs = |program: &str, args: &[&str], input: Option<&str>| -> bool {
        spawn(program, args, input, false).is_some_and(|(ok, _)| ok)
    };
    let cc = resolve_compiler(env("CC"), &|c| runs(c, &["--version"], None));
    let present = |p: &Probe| match p {
        Probe::Program(name) => {
            let name = if *name == "cc" {
                cc.as_str()
            } else if *name == "make" {
                make.as_str()
            } else {
                name
            };
            runs(name, &["--version"], None)
        }
        Probe::Headers(hs) => {
            let tk = hs.contains(&"tk.h");
            headers_present(
                hs,
                &|| {
                    if !tk {
                        return Vec::new();
                    }
                    let pc = spawn("pkg-config", &["--cflags", "tk"], None, true)
                        .filter(|(ok, _)| *ok)
                        .map(|(_, out)| String::from_utf8_lossy(&out).into_owned());
                    tk_fallbacks(pc)
                },
                &|h, extra| {
                    let mut args: Vec<&str> = flags.iter().map(String::as_str).collect();
                    args.extend(extra.iter().map(String::as_str));
                    args.extend(["-E", "-x", "c", "-o", "/dev/null", "-"]);
                    runs(
                        &cc,
                        &args,
                        Some(&format!(
                            "#include <{h}>
"
                        )),
                    )
                },
            )
        }
    };
    let display = env("DISPLAY").is_some_and(|d| !d.is_empty());
    evaluate(&present, needs_patch, display)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn missing_only(names: &'static [&'static str]) -> impl Fn(&Probe) -> bool {
        move |p: &Probe| match p {
            Probe::Program(n) => !names.contains(n),
            Probe::Headers(hs) => !hs.iter().all(|h| names.contains(h)),
        }
    }

    const DEBIAN: &str = "PRETTY_NAME=\"Debian GNU/Linux 13 (trixie)\"\nID=debian\n";
    const UBUNTU: &str = "ID=ubuntu\nID_LIKE=debian\n";
    const FEDORA: &str = "ID=fedora\n";

    #[test]
    fn nothing_missing_means_no_report() {
        let deps = evaluate(&|_: &Probe| true, true, true);
        assert!(deps.is_empty());
        let r = report(&deps, Some(DEBIAN), false);
        assert!(!r.refuse && r.lines.is_empty());
    }

    #[test]
    fn missing_ssl_refuses_with_the_debian_package() {
        let deps = evaluate(&missing_only(&["openssl/ssl.h"]), false, false);
        assert_eq!(
            deps,
            vec![Dep {
                name: "ssl",
                required: true,
                missing: Missing::Header("openssl/ssl.h".into())
            }]
        );
        let r = report(&deps, Some(DEBIAN), false);
        assert!(r.refuse);
        assert_eq!(
            r.lines,
            [
                "pyenv: cannot build Python: missing build dependencies:",
                "  ssl (openssl/ssl.h)",
                "Install them with:",
                "  sudo apt-get install libssl-dev",
                "To build anyway, set RPYENV_SKIP_PREFLIGHT=1.",
            ]
        );
    }

    #[test]
    fn optional_modules_warn_only() {
        let deps = evaluate(&missing_only(&["bzlib.h", "lzma.h"]), false, false);
        let r = report(&deps, Some(FEDORA), true);
        assert!(!r.refuse);
        assert_eq!(
            r.lines,
            [
                "pyenv: the build will lack these optional modules:",
                "  bz2 (bzlib.h)",
                "  lzma (lzma.h)",
                "Install them with:",
                "  dnf install bzip2-devel xz-devel",
            ]
        );
    }

    #[test]
    fn readline_accepts_libedit_and_tk_is_probed_only_with_a_display() {
        assert!(evaluate(&missing_only(&["readline/readline.h"]), false, false).is_empty());
        assert!(evaluate(&missing_only(&["tk.h"]), false, false).is_empty());
        assert_eq!(evaluate(&missing_only(&["tk.h"]), false, true).len(), 1);
    }

    #[test]
    fn a_missing_compiler_hides_the_header_probes() {
        let deps = evaluate(&missing_only(&["cc", "openssl/ssl.h"]), false, false);
        assert_eq!(
            deps.iter().map(|d| d.name).collect::<Vec<_>>(),
            ["C compiler"]
        );
        let r = report(&deps, Some(UBUNTU), false);
        assert!(r
            .lines
            .contains(&"  sudo apt-get install build-essential".to_string()));
    }

    #[test]
    fn a_compiler_with_a_wrapper_is_probed_as_program_plus_args() {
        // CI sets CC="ccache gcc"; `ccache` must run with `gcc --version`, not a program
        // named "ccache gcc". `sh -c true` stands in: one program, leading args.
        let env = |k: &str| match k {
            "CC" => Some("sh -c true".to_string()),
            "PATH" => std::env::var("PATH").ok(),
            _ => None,
        };
        let deps = check(&env, false);
        assert!(!deps.iter().any(|d| d.name == "C compiler"), "{deps:?}");
    }

    #[test]
    fn tk_found_only_through_a_fallback_include_dir_is_not_reported() {
        let tcl = vec!["-I/usr/include/tcl8.6".to_string()];
        let try_header = |h: &str, extra: &[String]| h == "tk.h" && extra == tcl.as_slice();
        let fallbacks = || tk_fallbacks(None);
        assert!(headers_present(&["tk.h"], &fallbacks, &try_header));
        // pkg-config's answer is used when there is one, and not the guesses.
        let pc = || {
            tk_fallbacks(Some(
                "-I/usr/include/tcl8.6
"
                .into(),
            ))
        };
        assert!(headers_present(&["tk.h"], &pc, &try_header));
        let wrong = || tk_fallbacks(Some("-I/elsewhere".into()));
        assert!(!headers_present(&["tk.h"], &wrong, &try_header));
        // Nowhere at all: still missing.
        assert!(!headers_present(&["tk.h"], &fallbacks, &|_, _| false));
    }

    #[test]
    fn an_unset_cc_falls_back_to_gcc_only_when_cc_fails() {
        assert_eq!(resolve_compiler(None, &|c| c == "gcc"), "gcc");
        assert_eq!(resolve_compiler(None, &|_| true), "cc");
        assert_eq!(resolve_compiler(None, &|_| false), "cc");
        assert_eq!(resolve_compiler(Some("clang".into()), &|_| false), "clang");
        assert_eq!(
            resolve_compiler(Some(String::new()), &|c| c == "gcc"),
            "gcc"
        );
    }

    #[test]
    fn patch_is_needed_only_with_patches_and_unknown_distros_get_no_command() {
        assert!(evaluate(&missing_only(&["patch"]), false, false).is_empty());
        let deps = evaluate(&missing_only(&["patch"]), true, false);
        let r = report(&deps, Some("ID=gentoo\n"), false);
        assert_eq!(
            r.lines,
            [
                "pyenv: cannot build Python: missing build dependencies:",
                "  patch (patch)",
                "To build anyway, set RPYENV_SKIP_PREFLIGHT=1."
            ]
        );
    }

    #[test]
    #[ignore = "probes this machine; run by hand"]
    fn probe_this_machine() {
        let env = |k: &str| std::env::var(k).ok();
        let deps = check(&env, true);
        let os = std::fs::read_to_string("/etc/os-release").ok();
        for l in report(&deps, os.as_deref(), false).lines {
            eprintln!("{l}");
        }
        eprintln!("{deps:?}");
    }
}
