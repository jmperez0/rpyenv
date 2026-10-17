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
    /// The header compiles but a program using it does not link (M8 design §4.2): the `-l`
    /// flags that were tried first.
    Link(String),
    /// Installed, below CPython's minimum: `found` is the raw version number, `need` the
    /// minimum as people write it.
    TooOld {
        found: String,
        need: &'static str,
    },
    /// Installed, newer than this CPython can build against (Tk 9 for 3.12 and older):
    /// `found` as `TooOld`'s, `need` what it needs instead.
    TooNew {
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
        "tkinter" => "tk",
        _ => unreachable!("lib_of: no library for the old-table row {name:?}"),
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
            super::deps::unquote(v)
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
        Missing::Link(flags) => format!("can't link {flags}"),
        Missing::TooOld { found, need } | Missing::TooNew { found, need } => {
            format!("{found}, needs {need}")
        }
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

/// The text for stderr when detection (`check3`) found something missing (M8 design §5).
/// `refuse`: the build stops here (header "cannot build", and how to build anyway); otherwise
/// lenient mode (D13) found only optional modules and the build goes on. `root`: no `sudo`.
pub fn report_deps(
    missing: &[Dep],
    os_release: Option<&str>,
    root: bool,
    refuse: bool,
) -> Vec<String> {
    report_deps_with(missing, os_release, root, refuse, false)
}

/// As `report_deps`; `apt_update`: the apt package lists are empty, so the command starts
/// with `apt-get update &&` (M8 design §5.3).
pub fn report_deps_with(
    missing: &[Dep],
    os_release: Option<&str>,
    root: bool,
    refuse: bool,
    apt_update: bool,
) -> Vec<String> {
    use super::deps;
    let mut lines = Vec::new();
    if missing.is_empty() {
        return lines;
    }
    lines.push(if refuse {
        "pyenv: cannot build Python: missing build dependencies:".to_string()
    } else {
        "pyenv: the build will lack these optional modules:".to_string()
    });
    for d in missing {
        lines.push(format!("  {}", shown(d)));
    }
    if let Some(m) = os_release.and_then(deps::Manager::from_os_release) {
        let install = deps::install_argv(m, &deps::packages(m, missing), !root, false);
        let mut cmd = install.join(" ");
        if apt_update && m == deps::Manager::Apt {
            cmd = format!("{} && {cmd}", deps::apt_update_argv(!root).join(" "));
        }
        // RHEL family: the command fails until CRB is on (ruling R15).
        lines.extend(os_release.and_then(|o| deps::crb_line(o, root)));
        lines.push("Install them with:".to_string());
        lines.push(format!("  {cmd}"));
    }
    // M8b: the way out without root (not when the compiler or make itself is missing).
    if refuse
        && !missing
            .iter()
            .any(|d| matches!(d.lib, "compiler" | "make" | "patch"))
    {
        lines.push(
            "Without root, pyenv install --deps=build compiles them from source.".to_string(),
        );
    }
    if refuse {
        lines.push("To build anyway, set RPYENV_SKIP_PREFLIGHT=1.".to_string());
    }
    lines
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

/// The program a header probe compiles. stdio.h comes first: Fedora's readline.h uses FILE
/// without including it.
fn header_probe_source(header: &str) -> String {
    format!("#include <stdio.h>\n#include <{header}>\nint main(void) {{ return 0; }}\n")
}

/// The link flag sets to try for a library: what `pkg-config --cflags --libs` printed first
/// (it carries include paths such as libtirpc's /usr/include/tirpc), then the fixed `links`.
fn link_alternatives(links: &[&[&str]], pkg_config_flags: Option<&str>) -> Vec<Vec<String>> {
    let mut alts: Vec<Vec<String>> = links
        .iter()
        .map(|a| a.iter().map(|s| s.to_string()).collect())
        .collect();
    if let Some(flags) = pkg_config_flags {
        alts.insert(0, flags.split_whitespace().map(str::to_string).collect());
    }
    alts
}

/// `<cc> CPPFLAGS... -x c - -o <out> LDFLAGS... <libs>`: compiles and links a program (M8
/// design §4.2 item 3). `cc` is the compiler `check` found (final review M2). `run(program,
/// args)` runs it with the source on stdin; the caller deletes `out`.
fn link_probe(
    env: &dyn Fn(&str) -> Option<String>,
    cc: &str,
    run: &dyn Fn(&str, &[&str]) -> bool,
    libs: &[&str],
    out: &str,
) -> bool {
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
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(cc, &refs)
}

/// The program a version check compiles (R6): the preprocessor evaluates the macro, so
/// OpenSSL 3's `( (3<<28) |(5<<20) ... )` is compared correctly. A failed `#error` means the
/// library is older than `need`, or, for a `too_new` bound (M4), at least `need`.
fn version_source(need: &stdlib::MinVersion, too_new: bool) -> String {
    let (op, word) = if too_new { (">=", "new") } else { ("<", "old") };
    format!(
        "#include <{}>\n#if {} {op} {:#x}\n#error too {word}\n#endif\n",
        need.header, need.expr, need.need
    )
}

/// The source that prints the installed version's text: one line per macro of `need.text`.
fn version_text_source(need: &stdlib::MinVersion) -> String {
    let mut src = format!("#include <{}>\n", need.header);
    for name in need.text.split(' ') {
        src.push_str(name);
        src.push('\n');
    }
    src
}

/// The readable version out of the preprocessed `version_text_source`: the quoted text of a
/// string macro (OpenSSL's `"OpenSSL 3.5.5 30 Sep 2026"` gives `3.5.5`), or the `names`
/// integers joined with dots.
fn readable_version(preprocessed: &str, names: usize) -> Option<String> {
    let lines: Vec<&str> = preprocessed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let tail = lines.get(lines.len().checked_sub(names)?..)?;
    if names == 1 {
        let line = tail[0];
        let inner = line.get(line.find('"')? + 1..line.rfind('"')?)?;
        let inner = inner.strip_prefix("OpenSSL ").unwrap_or(inner);
        let word = inner.split_whitespace().next()?;
        return Some(word.to_string());
    }
    let parts: Vec<String> = tail
        .iter()
        .map(|l| l.parse::<u64>().ok().map(|n| n.to_string()))
        .collect::<Option<_>>()?;
    Some(parts.join("."))
}

type Runner<'a> = &'a dyn Fn(&str, &[&str], &str, bool) -> Option<(bool, String)>;

/// `CPPFLAGS` and `PYTHON_CPPFLAGS`, then `extra` (a library's own `-I` flags).
fn cpp_flags(env: &dyn Fn(&str) -> Option<String>, extra: &[String]) -> Vec<String> {
    ["CPPFLAGS", "PYTHON_CPPFLAGS"]
        .iter()
        .filter_map(|k| env(k))
        .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .chain(extra.iter().cloned())
        .collect()
}

/// Whether the installed library meets `need` (R6): `Some(Dep)` only when the header
/// compiles on its own but the `#if` version test fails. `too_new`: `need` is the first
/// version that is too new (M4). `cc` is the compiler `check` found (M2); `extra` the library's
/// own `-I` flags (Tk's from pkg-config). `run(program, args, stdin, capture)`.
#[allow(clippy::too_many_arguments)]
fn version_verdict(
    env: &dyn Fn(&str) -> Option<String>,
    cc: &str,
    extra: &[String],
    run: Runner,
    name: &'static str,
    lib: &'static str,
    need: &stdlib::MinVersion,
    too_new: bool,
) -> Option<Dep> {
    let flags = cpp_flags(env, extra);
    let compile = |mode: &[&str], src: &str, capture: bool| {
        let mut args: Vec<&str> = flags.iter().map(String::as_str).collect();
        args.extend(mode);
        args.extend(["-x", "c", "-"]);
        run(cc, &args, src, capture)
    };
    let ok = |r: Option<(bool, String)>| r.is_some_and(|(ok, _)| ok);
    if ok(compile(
        &["-fsyntax-only"],
        &version_source(need, too_new),
        false,
    )) {
        return None;
    }
    // The header must be includable, or the failure says nothing about the version.
    let include = format!("#include <{}>\n", need.header);
    if !ok(compile(&["-fsyntax-only"], &include, false)) {
        return None;
    }
    let found = compile(&["-E", "-P"], &version_text_source(need), true)
        .and_then(|(ok, s)| ok.then_some(s))
        .and_then(|s| readable_version(&s, need.text.split(' ').count()))
        .unwrap_or_else(|| {
            if too_new {
                "a newer version"
            } else {
                "an older version"
            }
            .to_string()
        });
    let missing = if too_new {
        Missing::TooNew {
            found,
            need: need.shown,
        }
    } else {
        Missing::TooOld {
            found,
            need: need.shown,
        }
    };
    Some(Dep {
        name,
        required: true,
        missing,
        lib,
    })
}

/// CA certificates where the system's OpenSSL looks (`openssl version -d`), else the
/// distributions' bundle files (M8 design §4.3). `SSL_CERT_FILE` and `SSL_CERT_DIR` (a
/// `:`-separated list), when set, replace the file and the directory OpenSSL would read, as
/// they do for the built Python (final review M3).
pub(crate) fn ca_present(
    openssldir: Option<&str>,
    cert_file: Option<&str>,
    cert_dir: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> bool {
    let file = match cert_file {
        Some(f) => exists(f),
        None => {
            openssldir.is_some_and(|d| exists(&format!("{d}/cert.pem")))
                || [
                    "/etc/ssl/certs/ca-certificates.crt",
                    "/etc/pki/tls/certs/ca-bundle.crt",
                    "/etc/ssl/ca-bundle.pem",
                    "/etc/ssl/cert.pem",
                ]
                .iter()
                .any(|p| exists(p))
        }
    };
    let dir = match cert_dir {
        Some(list) => list.split(':').any(|d| !d.is_empty() && has_files(d)),
        None => openssldir.is_some_and(|d| has_files(&format!("{d}/certs"))),
    };
    file || dir
}

/// What the build route needs besides a compiler and `make` (plan rulings P3, P4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tools {
    /// Perl with the core modules OpenSSL's `Configure` and libxcrypt's scripts load.
    pub perl: bool,
    /// The X11 headers and libX11, which Tk needs until M8c.
    pub x11: bool,
}

/// The Perl modules OpenSSL's and libxcrypt's build scripts load that Fedora 44's
/// `perl-interpreter` alone lacks (measured 2026-10-17; `perl-core` has them).
pub const PERL_MODULES: [&str; 6] = [
    "FindBin",
    "lib",
    "IPC::Cmd",
    "File::Compare",
    "File::Copy",
    "Digest::SHA",
];

/// Probes for `Tools`. Debug builds: `RPYENV_TEST_BUILD_TOOLS` (a comma list of `perl`, `x11`)
/// names the tools present instead.
pub fn build_tools(env: &dyn Fn(&str) -> Option<String>) -> Tools {
    if cfg!(debug_assertions) {
        if let Some(list) = env("RPYENV_TEST_BUILD_TOOLS") {
            let has = |t: &str| list.split(',').map(str::trim).any(|w| w == t);
            return Tools {
                perl: has("perl"),
                x11: has("x11"),
            };
        }
    }
    let path = env("PATH");
    let run = |program: &str, args: &[&str], input: &str, capture: bool| {
        run_with_input(path.as_deref(), program, args, input, capture)
    };
    let mut perl_args: Vec<String> = PERL_MODULES.iter().map(|m| format!("-M{m}")).collect();
    perl_args.push("-e1".into());
    let perl_args: Vec<&str> = perl_args.iter().map(String::as_str).collect();
    let perl = run("perl", &perl_args, "", false).is_some_and(|(ok, _)| ok);
    let cc = probe_compiler(env, &run);
    let tmp = probe_path();
    let x11 = link_probe(
        env,
        &cc,
        &|p, a| {
            run(
                p,
                a,
                "#include <X11/Xlib.h>\nint main(void) { return XOpenDisplay(0) != 0; }\n",
                false,
            )
            .is_some_and(|(ok, _)| ok)
        },
        &["-lX11"],
        &tmp.display().to_string(),
    );
    let _ = std::fs::remove_file(&tmp);
    Tools { perl, x11 }
}

/// Runs `program` (its words split, as `$CC` may be `ccache gcc`) with `input` on stdin;
/// `(success, stdout)` when it could be started. Stderr is dropped.
fn run_with_input(
    path: Option<&str>,
    program: &str,
    args: &[&str],
    input: &str,
    capture: bool,
) -> Option<(bool, String)> {
    let mut words = program.split_whitespace();
    let mut c = Command::new(words.next()?);
    c.args(words).args(args);
    if let Some(p) = path {
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

/// The compiler `check` uses: `$CC`, else `cc`, else `gcc` (`resolve_compiler`), each tried
/// with `--version` through `run`.
fn probe_compiler(env: &dyn Fn(&str) -> Option<String>, run: Runner) -> String {
    resolve_compiler(env("CC"), &|c| {
        run(c, &["--version"], "", false).is_some_and(|(ok, _)| ok)
    })
}

/// Every module of the table for 3.`minor` whose library is missing (M8 design §4.2): a header
/// that doesn't compile, one that compiles but doesn't link (M5), or a version outside the
/// bounds (R6, M4). `cc`: the compiler to probe with (M2). `run(program, args, stdin,
/// capture)` runs the compiler and `pkg-config`.
fn library_deps(
    env: &dyn Fn(&str) -> Option<String>,
    cc: &str,
    run: Runner,
    minor: u32,
) -> Vec<Dep> {
    let mut out: Vec<Dep> = Vec::new();
    let ok = |r: Option<(bool, String)>| r.is_some_and(|(ok, _)| ok);
    for m in stdlib::modules(minor) {
        let Some(l) = stdlib::lib(m.lib) else {
            continue;
        };
        if l.key == stdlib::CA_KEY || out.iter().any(|d| d.lib == l.key) {
            continue;
        }
        let pc = l
            .pkg_config
            .and_then(|p| run("pkg-config", &["--cflags", "--libs", p], "", true))
            .filter(|(ok, _)| *ok)
            .map(|(_, s)| s);
        let alts = link_alternatives(l.links, pc.as_deref());
        let mut linked: Option<&Vec<String>> = None;
        'h: for h in l.headers {
            let src = header_probe_source(h);
            for alt in &alts {
                let libs: Vec<&str> = alt.iter().map(String::as_str).collect();
                let tmp = probe_path();
                let works = link_probe(
                    env,
                    cc,
                    &|p, a| ok(run(p, a, &src, false)),
                    &libs,
                    &tmp.display().to_string(),
                );
                let _ = std::fs::remove_file(&tmp);
                if works {
                    linked = Some(alt);
                    break 'h;
                }
            }
        }
        // pkg-config's `-I` flags, which the header may need (Tk on Debian, libtirpc).
        let include = |alt: &[String]| -> Vec<String> {
            alt.iter()
                .filter(|f| f.starts_with("-I") || f.starts_with("-D"))
                .cloned()
                .collect()
        };
        let Some(alt) = linked else {
            // No link: a header that compiles means the library itself is what's missing (M5).
            let extra = pc
                .as_deref()
                .map(|p| include(&link_alternatives(&[], Some(p))[0]))
                .unwrap_or_default();
            let flags = cpp_flags(env, &extra);
            let compiles = l.headers.iter().any(|h| {
                let mut args: Vec<&str> = flags.iter().map(String::as_str).collect();
                args.extend(["-fsyntax-only", "-x", "c", "-"]);
                ok(run(cc, &args, &header_probe_source(h), false))
            });
            let missing = if compiles {
                Missing::Link(l.links.first().map(|f| f.join(" ")).unwrap_or_default())
            } else {
                Missing::Header(l.headers[0].to_string())
            };
            out.push(Dep {
                name: m.name,
                required: true,
                missing,
                lib: l.key,
            });
            continue;
        };
        let extra = include(alt);
        if let Some(need) = stdlib::min_version(l.key, minor) {
            out.extend(version_verdict(
                env, cc, &extra, run, m.name, l.key, &need, false,
            ));
        }
        if let Some(limit) = stdlib::too_new(l.key, minor) {
            out.extend(version_verdict(
                env, cc, &extra, run, m.name, l.key, &limit, true,
            ));
        }
    }
    out
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
                let program = match k {
                    "compiler" => Some(("C compiler", "compiler", "cc")),
                    "make" => Some(("make", "make", "make")),
                    "patch" => Some(("patch", "patch", "patch")),
                    "pkg-config" => Some(("pkg-config", "pkg-config", "pkg-config")),
                    _ => None,
                };
                if let Some((name, lib, prog)) = program {
                    out.push(Dep {
                        name,
                        required: true,
                        missing: Missing::Program(prog.to_string()),
                        lib,
                    });
                    continue;
                }
                let Some(l) = stdlib::lib(k) else {
                    panic!("RPYENV_TEST_PREFLIGHT: unknown key {k:?} (a stdlib library key, compiler, make, patch or pkg-config)");
                };
                let name = stdlib::modules(minor)
                    .iter()
                    .find(|m| m.lib == k)
                    .map_or("ssl certificates", |m| m.name);
                out.push(Dep {
                    name,
                    required: true,
                    missing: Missing::Header(k.to_string()),
                    lib: l.key,
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
            run_with_input(path.as_deref(), program, args, input, capture)
        };
    if runs_with_input("pkg-config", &["--version"], "", false).is_none_or(|(ok, _)| !ok) {
        out.push(Dep {
            name: "pkg-config",
            required: true,
            missing: Missing::Program("pkg-config".into()),
            lib: "pkg-config",
        });
    }
    // The compiler `check` found: on a host with `gcc` and no `cc`, that is gcc (M2).
    let cc = probe_compiler(env, &runs_with_input);
    out.extend(library_deps(env, &cc, &runs_with_input, minor));
    let dir = runs_with_input("openssl", &["version", "-d"], "", true)
        .and_then(|(ok, s)| ok.then_some(s))
        .and_then(|s| s.split('"').nth(1).map(str::to_string));
    let has_files = |d: &str| std::fs::read_dir(d).is_ok_and(|mut r| r.next().is_some());
    let var = |k: &str| env(k).filter(|v| !v.is_empty());
    let (cert_file, cert_dir) = (var("SSL_CERT_FILE"), var("SSL_CERT_DIR"));
    if !ca_present(
        dir.as_deref(),
        cert_file.as_deref(),
        cert_dir.as_deref(),
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

    #[test]
    fn the_build_tools_hook_names_the_tools_present() {
        let env = |k: &str| (k == "RPYENV_TEST_BUILD_TOOLS").then(|| "x11".to_string());
        assert_eq!(
            build_tools(&env),
            Tools {
                perl: false,
                x11: true
            }
        );
        let none = |k: &str| (k == "RPYENV_TEST_BUILD_TOOLS").then(String::new);
        assert_eq!(
            build_tools(&none),
            Tools {
                perl: false,
                x11: false
            }
        );
        assert!(PERL_MODULES.contains(&"IPC::Cmd") && PERL_MODULES.contains(&"FindBin"));
    }

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

    // allowlist D-119
    #[test]
    fn ca_certificates_are_found_as_a_file_or_a_directory() {
        let none = |_: &str| false;
        assert!(ca_present(
            Some("/usr/lib/ssl"),
            None,
            None,
            &|p| p == "/usr/lib/ssl/cert.pem",
            &none
        ));
        assert!(ca_present(Some("/usr/lib/ssl"), None, None, &none, &|d| d
            == "/usr/lib/ssl/certs"));
        // No openssl binary: the distributions' bundle paths.
        assert!(ca_present(
            None,
            None,
            None,
            &|p| p == "/etc/pki/tls/certs/ca-bundle.crt",
            &none
        ));
        assert!(!ca_present(Some("/usr/lib/ssl"), None, None, &none, &none));
    }

    /// Final review M3: SSL_CERT_FILE and SSL_CERT_DIR replace what OpenSSL would read, before
    /// the build as after it.
    #[test]
    fn ssl_cert_file_and_dir_replace_the_defaults() {
        let none = |_: &str| false;
        let bundle = |p: &str| p == "/etc/ssl/certs/ca-certificates.crt";
        // The variable names a file that is not there: the system bundle no longer counts.
        assert!(!ca_present(
            None,
            Some("/opt/certs.pem"),
            None,
            &bundle,
            &none
        ));
        assert!(ca_present(
            None,
            Some("/opt/certs.pem"),
            None,
            &|p| p == "/opt/certs.pem",
            &none
        ));
        // SSL_CERT_DIR is a list; one directory with certificates is enough.
        let dir = |d: &str| d == "/opt/b";
        assert!(ca_present(None, None, Some("/opt/a:/opt/b"), &none, &dir));
        assert!(!ca_present(
            Some("/usr/lib/ssl"),
            None,
            Some("/opt/a"),
            &none,
            &|d| d == "/usr/lib/ssl/certs"
        ));
        // Only the directory is replaced: the default file still counts.
        assert!(ca_present(None, None, Some("/opt/a"), &bundle, &none));
    }

    /// Review focus 4: a compiler wrapper and custom flags reach the link probe.
    // allowlist D-118
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
        assert!(link_probe(&env, "ccache gcc", &run, &["-lz"], "/tmp/out"));
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
    fn the_version_test_is_an_if_on_the_macro() {
        let need = crate::install::stdlib::min_version("openssl", 13).unwrap();
        let src = version_source(&need, false);
        assert!(
            src.contains(
                "#include <openssl/opensslv.h>\n#if OPENSSL_VERSION_NUMBER < 0x1010100f\n#error"
            ),
            "{src}"
        );
        // M4: the upper bound is the same test the other way.
        let tk = crate::install::stdlib::too_new("tk", 12).unwrap();
        assert_eq!(
            version_source(&tk, true),
            "#include <tk.h>\n#if TK_MAJOR_VERSION >= 0x9\n#error too new\n#endif\n"
        );
    }

    #[test]
    fn readable_versions_come_from_the_string_macros() {
        assert_eq!(
            readable_version("# 1 \"x\"\n\"OpenSSL 3.5.5 30 Sep 2026\"\n", 1).as_deref(),
            Some("3.5.5")
        );
        assert_eq!(
            readable_version("\"3.45.1\"\n", 1).as_deref(),
            Some("3.45.1")
        );
        assert_eq!(readable_version("1\n5\n5\n", 3).as_deref(), Some("1.5.5"));
        assert_eq!(readable_version("OPENSSL_VERSION_TEXT\n", 1), None);
        assert_eq!(readable_version("", 1), None);
    }

    /// A fake compiler: `#error` programs fail when `old`; `-E` prints OpenSSL 3.5.5's text.
    fn fake_cc(
        old: bool,
        includable: bool,
    ) -> impl Fn(&str, &[&str], &str, bool) -> Option<(bool, String)> {
        move |_, args, src, _| {
            if args.contains(&"-E") {
                return Some((true, "\"OpenSSL 3.5.5 30 Sep 2026\"\n".into()));
            }
            Some((
                includable && !(old && src.contains("#error")),
                String::new(),
            ))
        }
    }

    // allowlist D-118
    #[test]
    fn a_too_old_library_is_reported_with_both_versions() {
        let need = crate::install::stdlib::min_version("openssl", 13).unwrap();
        let env = |_: &str| None;
        let verdict = |old: bool, includable: bool| {
            version_verdict(
                &env,
                "cc",
                &[],
                &fake_cc(old, includable),
                "ssl",
                "openssl",
                &need,
                false,
            )
        };
        assert_eq!(
            verdict(true, true).unwrap().missing,
            Missing::TooOld {
                found: "3.5.5".into(),
                need: "1.1.1"
            }
        );
        assert!(verdict(false, true).is_none());
        // A header that does not compile at all is not "too old".
        assert!(verdict(true, false).is_none());
    }

    /// Final review M4: CPython 3.12 and older can't build `_tkinter` against Tk 9. The check
    /// compiles with Tk's own `-I` flags (Debian keeps tk.h in /usr/include/tcl8.6).
    #[test]
    fn tk_9_is_too_new_for_3_12() {
        let seen = std::cell::RefCell::new(Vec::<String>::new());
        let run = |_: &str, args: &[&str], src: &str, _: bool| {
            seen.borrow_mut().push(args.join(" "));
            if args.contains(&"-E") {
                return Some((true, "\"9.0.2\"\n".to_string()));
            }
            Some((!src.contains("#error too new"), String::new()))
        };
        let env = |_: &str| None;
        let limit = crate::install::stdlib::too_new("tk", 12).unwrap();
        let extra = vec!["-I/usr/include/tcl9.0".to_string()];
        let d = version_verdict(&env, "cc", &extra, &run, "tkinter", "tk", &limit, true).unwrap();
        assert_eq!(
            d.missing,
            Missing::TooNew {
                found: "9.0.2".into(),
                need: "8.5 or 8.6"
            }
        );
        assert!(
            seen.borrow()
                .iter()
                .all(|a| a.contains("-I/usr/include/tcl9.0")),
            "{:?}",
            seen.borrow()
        );
        // Tk 8.6 passes.
        let tk8 = |_: &str, _: &[&str], _: &str, _: bool| Some((true, String::new()));
        assert!(version_verdict(&env, "cc", &[], &tk8, "tkinter", "tk", &limit, true).is_none());
    }

    /// Final review M2 and M5, with a fake runner: on a host with `gcc` and no `cc`, every
    /// probe uses gcc; a library whose header compiles but doesn't link is `Link` with its
    /// flags, and one whose header doesn't compile is `Header`.
    #[test]
    fn probes_use_the_compiler_check_found_and_tell_link_from_header() {
        let seen = std::cell::RefCell::new(Vec::<String>::new());
        let run = |p: &str, args: &[&str], src: &str, _: bool| -> Option<(bool, String)> {
            seen.borrow_mut().push(p.to_string());
            match p {
                "gcc" => {}
                "pkg-config" => return Some((false, String::new())),
                _ => return None, // no `cc` on this host
            }
            let linking = args.contains(&"-o") && !args.contains(&"--version");
            if src.contains("gdbm.h") && linking {
                return Some((false, String::new())); // libgdbm_compat missing
            }
            if src.contains("uuid/uuid.h") {
                return Some((false, String::new())); // no uuid-dev at all
            }
            Some((true, String::new()))
        };
        let env = |_: &str| None;
        let cc = probe_compiler(&env, &run);
        assert_eq!(cc, "gcc");
        let deps = library_deps(&env, &cc, &run, 12);
        let got: Vec<(&str, &Missing)> = deps.iter().map(|d| (d.name, &d.missing)).collect();
        assert_eq!(
            got,
            [
                ("dbm", &Missing::Link("-lgdbm -lgdbm_compat".into())),
                ("uuid", &Missing::Header("uuid/uuid.h".into())),
            ]
        );
        assert!(
            seen.borrow()
                .iter()
                .skip(2) // `probe_compiler`'s `cc --version`, then `gcc --version`
                .all(|p| p == "gcc" || p == "pkg-config"),
            "{:?}",
            seen.borrow()
        );
    }

    /// Final review M2, on real processes: a PATH with `gcc` and no `cc` (and `make`,
    /// `pkg-config`) finds every library, where the probes once ran `cc` and failed to start.
    #[test]
    fn a_host_with_gcc_and_no_cc_finds_its_libraries() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("rpyenv-gcc-only-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let stub = |name: &str, body: &str| {
            let p = d.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        // gcc: succeeds at everything; `-E` prints nothing, so no version text is read.
        stub("gcc", "exit 0");
        stub("make", "exit 0");
        stub("pkg-config", "[ \"$1\" = --version ] && exit 0\nexit 1");
        let certs = d.join("certs.pem");
        std::fs::write(&certs, "x").unwrap();
        // Only the stubs: the real /usr/bin has a `cc`.
        let path = d.display().to_string();
        let certs = certs.display().to_string();
        let env = |k: &str| match k {
            "PATH" => Some(path.clone()),
            "SSL_CERT_FILE" => Some(certs.clone()),
            _ => None,
        };
        let deps = check3(&env, false, 12);
        let _ = std::fs::remove_dir_all(&d);
        assert!(deps.is_empty(), "{deps:#?}");
    }

    #[test]
    fn the_version_test_gets_the_compiler_words_and_cppflags() {
        let seen = std::cell::RefCell::new(Vec::<(String, Vec<String>)>::new());
        let run = |p: &str, a: &[&str], _: &str, _: bool| {
            seen.borrow_mut()
                .push((p.to_string(), a.iter().map(|s| s.to_string()).collect()));
            Some((true, String::new()))
        };
        let env = |k: &str| match k {
            "CC" => Some("ccache gcc".to_string()),
            "CPPFLAGS" => Some("-I/opt/x/include".to_string()),
            _ => None,
        };
        let need = crate::install::stdlib::min_version("openssl", 13).unwrap();
        assert!(version_verdict(
            &env,
            "ccache gcc",
            &[],
            &run,
            "ssl",
            "openssl",
            &need,
            false
        )
        .is_none());
        let (prog, args) = &seen.borrow()[0];
        assert_eq!(prog, "ccache gcc");
        assert!(
            args.contains(&"-I/opt/x/include".to_string())
                && args.contains(&"-fsyntax-only".to_string())
        );
    }

    #[test]
    fn the_test_hook_knows_program_keys() {
        let env = |k: &str| {
            (k == "RPYENV_TEST_PREFLIGHT").then(|| "make,pkg-config,compiler".to_string())
        };
        let deps = check3(&env, false, 12);
        let got: Vec<(&str, &str)> = deps.iter().map(|d| (d.name, d.lib)).collect();
        assert_eq!(
            got,
            [
                ("make", "make"),
                ("pkg-config", "pkg-config"),
                ("C compiler", "compiler")
            ]
        );
        assert!(deps
            .iter()
            .all(|d| matches!(d.missing, Missing::Program(_))));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "unknown key")]
    fn the_test_hook_rejects_a_typo() {
        let env = |k: &str| (k == "RPYENV_TEST_PREFLIGHT").then(|| "opnssl".to_string());
        check3(&env, false, 12);
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
        // R6: the version decision on the real headers. A need above the real version must
        // come back TooOld with the real version; the real minimum must pass.
        let run = |p: &str, a: &[&str], i: &str, c: bool| run_with_input(None, p, a, i, c);
        let real = stdlib::min_version("openssl", 13).unwrap();
        let future = stdlib::MinVersion {
            need: 0x4000_0000,
            shown: "4.0.0",
            ..real
        };
        let cc = probe_compiler(&env, &run);
        let verdict = version_verdict(&env, &cc, &[], &run, "ssl", "openssl", &future, false);
        eprintln!("openssl need 4.0.0: {verdict:?}");
        let real = stdlib::min_version("openssl", 13).unwrap();
        eprintln!(
            "openssl need 1.1.1: {:?}",
            version_verdict(&env, &cc, &[], &run, "ssl", "openssl", &real, false)
        );
        match verdict.map(|d| d.missing) {
            Some(Missing::TooOld { found, .. }) => assert!(found.starts_with("3."), "{found}"),
            other => panic!("expected TooOld, got {other:?}"),
        }
        for minor in [13, 14] {
            eprintln!("check3 3.{minor}: {:#?}", check3(&env, true, minor));
        }
    }

    #[test]
    fn report_deps_refuses_or_warns_by_its_flag() {
        let d = [Dep {
            name: "uuid",
            required: true,
            missing: Missing::Link("-luuid".into()),
            lib: "libuuid",
        }];
        let os = "ID=debian
";
        let refuse = report_deps(&d, Some(os), false, true);
        assert_eq!(
            refuse,
            [
                "pyenv: cannot build Python: missing build dependencies:",
                "  uuid (can't link -luuid)",
                "Install them with:",
                "  sudo apt-get install uuid-dev",
                "Without root, pyenv install --deps=build compiles them from source.",
                "To build anyway, set RPYENV_SKIP_PREFLIGHT=1.",
            ]
        );
        // M8b: no build-route hint when the compiler itself is missing (the route needs it).
        let cc = [Dep {
            name: "C compiler",
            required: true,
            missing: Missing::Program("cc".into()),
            lib: "compiler",
        }];
        let no_cc = report_deps(&cc, Some(os), false, true);
        assert!(
            !no_cc.iter().any(|l| l.contains("--deps=build")),
            "{no_cc:?}"
        );
        assert_eq!(
            no_cc.last().map(String::as_str),
            Some("To build anyway, set RPYENV_SKIP_PREFLIGHT=1.")
        );
        let warn = report_deps(&d, Some(os), true, false);
        assert_eq!(
            warn,
            [
                "pyenv: the build will lack these optional modules:",
                "  uuid (can't link -luuid)",
                "Install them with:",
                "  apt-get install uuid-dev",
            ]
        );
        let upd = report_deps_with(&d, Some(os), false, false, true);
        assert_eq!(
            upd[3],
            "  sudo apt-get update && sudo apt-get install uuid-dev"
        );
        assert!(report_deps(&[], Some(os), false, true).is_empty());
    }

    /// Ruling R15: on Rocky the printed report names CRB before the dnf command, without sudo
    /// for root; in lenient mode too.
    #[test]
    fn rocky_reports_show_the_crb_line_before_the_command() {
        let d = [Dep {
            name: "uuid",
            required: true,
            missing: Missing::Header("uuid/uuid.h".into()),
            lib: "libuuid",
        }];
        let rocky = "NAME=\"Rocky Linux\"\nID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.6\"\n";
        for (root, refuse) in [(true, true), (false, false)] {
            let lines = report_deps(&d, Some(rocky), root, refuse);
            let sudo = if root { "" } else { "sudo " };
            let crb = format!("Some packages live in the CRB repository; enable it first: {sudo}dnf install dnf-plugins-core && {sudo}dnf config-manager --set-enabled crb");
            let at = lines
                .iter()
                .position(|l| *l == crb)
                .unwrap_or_else(|| panic!("{lines:?}"));
            assert_eq!(lines[at + 1], "Install them with:");
            assert_eq!(lines[at + 2], format!("  {sudo}dnf install libuuid-devel"));
        }
        let debian = report_deps(&d, Some("ID=debian\n"), false, true);
        assert!(!debian.iter().any(|l| l.contains("CRB")), "{debian:?}");
    }

    #[test]
    fn header_probe_includes_stdio_first() {
        assert_eq!(
            header_probe_source("readline/readline.h"),
            "#include <stdio.h>\n#include <readline/readline.h>\nint main(void) { return 0; }\n"
        );
    }

    #[test]
    fn pkg_config_flags_come_before_the_fixed_links() {
        let links: &[&[&str]] = &[&["-lnsl", "-ltirpc"]];
        assert_eq!(
            link_alternatives(links, Some("-I/usr/include/tirpc -ltirpc\n")),
            [
                vec!["-I/usr/include/tirpc".to_string(), "-ltirpc".to_string()],
                vec!["-lnsl".to_string(), "-ltirpc".to_string()],
            ]
        );
        assert_eq!(
            link_alternatives(links, None),
            [vec!["-lnsl".to_string(), "-ltirpc".to_string()]]
        );
    }
}
