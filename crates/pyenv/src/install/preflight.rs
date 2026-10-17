//! The build pre-flight check (spec §9.2, plan Decision 8): a compiler, `make`, `patch` when
//! needed, and the headers of the modules python-build verifies. Missing required pieces
//! refuse the build before downloading; missing optional ones warn. Results are structured
//! for M8 (spec §15.2).

use super::stdlib;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    Program(String),
    Header(String),
    /// The header is there but a program using it does not link (M8 design §4.2).
    Link(String),
    /// Installed, below CPython's minimum: `found` is the raw version number, `need` the
    /// minimum as people write it.
    TooOld {
        found: String,
        need: &'static str,
    },
    Certificates,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    /// What the user loses: `C compiler`, `make`, `patch`, or a module name such as `ssl`.
    pub name: &'static str,
    pub required: bool,
    pub missing: Missing,
    /// A `stdlib` key, or `compiler`, `make`, `patch`, `pkg-config` for programs.
    pub lib: &'static str,
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

/// The `stdlib` key (or program name) behind a row of the old table.
fn lib_of(name: &str) -> &'static str {
    match name {
        "C compiler" => "compiler",
        "make" => "make",
        "patch" => "patch",
        "ssl" => "openssl",
        "zlib" => "zlib",
        "bz2" => "bzip2",
        "readline" => "readline",
        "sqlite3" => "sqlite",
        "ctypes" => "libffi",
        "curses" => "ncurses",
        "lzma" => "xz",
        _ => "tk",
    }
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
                lib: lib_of(name),
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
        Missing::Header(h) | Missing::Link(h) => h.clone(),
        Missing::TooOld { found, need } => format!("{found}, needs {need}"),
        Missing::Certificates => "CA certificates".to_string(),
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

/// `<cc> CPPFLAGS... -x c - -o <out> LDFLAGS... <libs>`: compiles and links a program that
/// includes `header` (M8 design §4.2 item 3). `run(program, args)` runs it with the source on
/// stdin; the caller deletes `out`.
fn link_probe(
    env: &dyn Fn(&str) -> Option<String>,
    run: &dyn Fn(&str, &[&str]) -> bool,
    header: &str,
    libs: &[&str],
    out: &str,
) -> bool {
    let cc = env("CC")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "cc".into());
    let words = |k: &str| {
        env(k)
            .map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut args: Vec<String> = Vec::new();
    args.extend(words("CPPFLAGS"));
    args.extend(words("PYTHON_CPPFLAGS"));
    args.extend(["-x".into(), "c".into(), "-".into(), "-o".into(), out.into()]);
    args.extend(words("LDFLAGS"));
    args.extend(words("PYTHON_LDFLAGS"));
    args.extend(libs.iter().map(|l| l.to_string()));
    let _ = header; // the source text is passed by `run`
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&cc, &refs)
}

/// The number a version macro expands to: the last non-empty, non-`#` line, hex or decimal,
/// with C suffixes (`L`, `UL`) dropped.
pub(crate) fn parse_version(preprocessed: &str) -> Option<u64> {
    let last = preprocessed
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with('#'))?;
    let digits = last.trim_end_matches(['L', 'l', 'U', 'u']);
    match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => digits.parse().ok(),
    }
}

fn too_old(
    name: &'static str,
    lib: &'static str,
    found: u64,
    need: &stdlib::MinVersion,
) -> Option<Dep> {
    (found < need.need).then(|| Dep {
        name,
        required: true,
        missing: Missing::TooOld {
            found: format!("{found:#x}"),
            need: need.shown,
        },
        lib,
    })
}

/// CA certificates where the system's OpenSSL looks (`openssl version -d`), else the
/// distributions' bundle files (M8 design §4.3).
pub(crate) fn ca_present(
    openssldir: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> bool {
    if let Some(d) = openssldir {
        if exists(&format!("{d}/cert.pem")) || has_files(&format!("{d}/certs")) {
            return true;
        }
    }
    [
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/etc/ssl/cert.pem",
    ]
    .iter()
    .any(|p| exists(p))
}

/// A fresh output path for one link probe: unique per process and per call, so parallel
/// probes (and parallel tests) never share a file.
fn probe_path() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "rpyenv-probe-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// CPython 3.`minor` (M8 design §4): programs as `check`, then every module of the table:
/// header, link, minimum version; then CA certificates. Every item is required (spec D1).
/// Debug builds: `RPYENV_TEST_PREFLIGHT` lists the library keys to report missing instead.
pub fn check3(env: &dyn Fn(&str) -> Option<String>, needs_patch: bool, minor: u32) -> Vec<Dep> {
    if cfg!(debug_assertions) {
        if let Some(list) = env("RPYENV_TEST_PREFLIGHT") {
            let keys: Vec<&str> = list
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .collect();
            let mut out = Vec::new();
            for k in keys {
                let name = stdlib::modules(minor)
                    .iter()
                    .find(|m| m.lib == k)
                    .map(|m| m.name)
                    .unwrap_or(match k {
                        stdlib::CA_KEY => "ssl certificates",
                        _ => "build tools",
                    });
                let lib = stdlib::lib(k).map(|l| l.key).unwrap_or("compiler");
                out.push(Dep {
                    name,
                    required: true,
                    missing: Missing::Header(k.to_string()),
                    lib,
                });
            }
            return out;
        }
    }
    // Programs first, exactly as `check` (a missing compiler hides the rest).
    let mut out: Vec<Dep> = check(env, needs_patch)
        .into_iter()
        .filter(|d| matches!(d.missing, Missing::Program(_)))
        .map(|d| Dep {
            required: true,
            ..d
        })
        .collect();
    if out.iter().any(|d| d.lib == "compiler") {
        return out;
    }
    let path = env("PATH");
    let runs_with_input =
        |program: &str, args: &[&str], input: &str, capture: bool| -> Option<(bool, String)> {
            let mut words = program.split_whitespace();
            let mut c = Command::new(words.next()?);
            c.args(words).args(args);
            if let Some(p) = &path {
                c.env("PATH", p);
            }
            c.stdin(Stdio::piped())
                .stdout(if capture {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stderr(Stdio::null());
            let mut child = c.spawn().ok()?;
            if let Some(mut s) = child.stdin.take() {
                use std::io::Write;
                let _ = s.write_all(input.as_bytes());
            }
            let o = child.wait_with_output().ok()?;
            Some((
                o.status.success(),
                String::from_utf8_lossy(&o.stdout).into_owned(),
            ))
        };
    if runs_with_input("pkg-config", &["--version"], "", false).is_none_or(|(ok, _)| !ok) {
        out.push(Dep {
            name: "pkg-config",
            required: true,
            missing: Missing::Program("pkg-config".into()),
            lib: "pkg-config",
        });
    }
    for m in stdlib::modules(minor) {
        let Some(l) = stdlib::lib(m.lib) else {
            continue;
        };
        if l.key == stdlib::CA_KEY || out.iter().any(|d| d.lib == l.key) {
            continue;
        }
        let pc = l
            .pkg_config
            .and_then(|p| runs_with_input("pkg-config", &["--cflags", "--libs", p], "", true))
            .filter(|(ok, _)| *ok)
            .map(|(_, s)| s);
        let mut found = false;
        'h: for h in l.headers {
            let src = format!("#include <{h}>\nint main(void) {{ return 0; }}\n");
            let mut alts: Vec<Vec<String>> = l
                .links
                .iter()
                .map(|a| a.iter().map(|s| s.to_string()).collect())
                .collect();
            if let Some(flags) = &pc {
                alts.insert(0, flags.split_whitespace().map(str::to_string).collect());
            }
            for alt in &alts {
                let libs: Vec<&str> = alt.iter().map(String::as_str).collect();
                let tmp = probe_path();
                let linked = link_probe(
                    env,
                    &|p, a| runs_with_input(p, a, &src, false).is_some_and(|(ok, _)| ok),
                    h,
                    &libs,
                    &tmp.display().to_string(),
                );
                let _ = std::fs::remove_file(&tmp);
                if linked {
                    found = true;
                    break 'h;
                }
            }
        }
        if !found {
            out.push(Dep {
                name: m.name,
                required: true,
                missing: Missing::Link(l.headers[0].to_string()),
                lib: l.key,
            });
            continue;
        }
        if let Some(need) = stdlib::min_version(l.key, minor) {
            let src = format!("#include <{}>\n{}\n", need.header, need.expr);
            let cc = env("CC")
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "cc".into());
            let v = runs_with_input(&cc, &["-E", "-P", "-x", "c", "-"], &src, true)
                .and_then(|(ok, s)| ok.then_some(s))
                .and_then(|s| parse_version(&s));
            if let Some(d) = v.and_then(|v| too_old(m.name, l.key, v, &need)) {
                out.push(d);
            }
        }
    }
    let dir = runs_with_input("openssl", &["version", "-d"], "", true)
        .and_then(|(ok, s)| ok.then_some(s))
        .and_then(|s| s.split('"').nth(1).map(str::to_string));
    let has_files = |d: &str| std::fs::read_dir(d).is_ok_and(|mut r| r.next().is_some());
    if !ca_present(
        dir.as_deref(),
        &|p| std::path::Path::new(p).is_file(),
        &has_files,
    ) {
        out.push(Dep {
            name: "ssl certificates",
            required: true,
            missing: Missing::Certificates,
            lib: stdlib::CA_KEY,
        });
    }
    out
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

    // allowlist D-65
    #[test]
    fn missing_ssl_refuses_with_the_debian_package() {
        let deps = evaluate(&missing_only(&["openssl/ssl.h"]), false, false);
        assert_eq!(
            deps,
            vec![Dep {
                name: "ssl",
                required: true,
                missing: Missing::Header("openssl/ssl.h".into()),
                lib: "openssl"
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
    fn version_macros_parse_in_hex_and_decimal() {
        assert_eq!(
            parse_version("# 1 \"x\"\n\n0x30000020L\n"),
            Some(0x3000_0020)
        );
        assert_eq!(parse_version("3045001\n"), Some(3_045_001));
        assert_eq!(parse_version("OPENSSL_VERSION_NUMBER\n"), None);
    }

    #[test]
    fn ca_certificates_are_found_as_a_file_or_a_directory() {
        let none = |_: &str| false;
        assert!(ca_present(
            Some("/usr/lib/ssl"),
            &|p| p == "/usr/lib/ssl/cert.pem",
            &none
        ));
        assert!(ca_present(Some("/usr/lib/ssl"), &none, &|d| d == "/usr/lib/ssl/certs"));
        // No openssl binary: the distributions' bundle paths.
        assert!(ca_present(
            None,
            &|p| p == "/etc/pki/tls/certs/ca-bundle.crt",
            &none
        ));
        assert!(!ca_present(Some("/usr/lib/ssl"), &none, &none));
    }

    /// Review focus 4: a compiler wrapper and custom flags reach the link probe.
    #[test]
    fn the_link_probe_passes_cppflags_and_ldflags() {
        let seen = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |program: &str, args: &[&str]| -> bool {
            seen.borrow_mut().push(
                std::iter::once(program.to_string())
                    .chain(args.iter().map(|a| a.to_string()))
                    .collect(),
            );
            true
        };
        let env = |k: &str| match k {
            "CC" => Some("ccache gcc".to_string()),
            "CPPFLAGS" => Some("-I/opt/x/include".to_string()),
            "LDFLAGS" => Some("-L/opt/x/lib".to_string()),
            _ => None,
        };
        assert!(link_probe(&env, &run, "zlib.h", &["-lz"], "/tmp/out"));
        let call = &seen.borrow()[0];
        assert_eq!(call[0], "ccache gcc");
        for want in ["-I/opt/x/include", "-L/opt/x/lib", "-lz", "-o", "/tmp/out"] {
            assert!(
                call.iter().any(|a| a == want),
                "{want} missing from {call:?}"
            );
        }
        // Libraries after the source, as linkers need.
        let src = call.iter().position(|a| a == "-").unwrap();
        assert!(call.iter().position(|a| a == "-lz").unwrap() > src);
    }

    #[test]
    fn the_test_hook_reports_exactly_the_listed_libraries() {
        let env = |k: &str| match k {
            "RPYENV_TEST_PREFLIGHT" => Some("openssl,libuuid".to_string()),
            _ => None,
        };
        let deps = check3(&env, false, 12);
        let libs: Vec<&str> = deps.iter().map(|d| d.lib).collect();
        assert_eq!(libs, ["openssl", "libuuid"]);
        assert!(deps.iter().all(|d| d.required));
        assert_eq!(deps[0].name, "ssl");
        let none = |k: &str| (k == "RPYENV_TEST_PREFLIGHT").then(String::new);
        assert!(check3(&none, false, 12).is_empty());
    }

    #[test]
    fn a_too_old_library_is_reported_with_both_versions() {
        let d = too_old(
            "ssl",
            "openssl",
            0x1000_2000,
            &crate::install::stdlib::min_version("openssl", 13).unwrap(),
        );
        assert_eq!(
            d.unwrap().missing,
            Missing::TooOld {
                found: "0x10002000".into(),
                need: "1.1.1"
            }
        );
        assert!(too_old(
            "ssl",
            "openssl",
            0x3000_0000,
            &crate::install::stdlib::min_version("openssl", 13).unwrap()
        )
        .is_none());
    }

    #[test]
    fn probe_files_are_unique_per_call() {
        assert_ne!(probe_path(), probe_path());
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
        for minor in [13, 14] {
            eprintln!("check3 3.{minor}: {:#?}", check3(&env, true, minor));
        }
    }
}
