//! How `pyenv install` gets missing build dependencies (M8 design §5): the routes, the
//! package manager and its command, whether `sudo` can work, and the prompt's text.

use super::preflight::{Dep, Missing};
use super::stdlib;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    System,
    Install,
    Build,
    Prebuilt,
}

impl Route {
    pub fn parse(s: &str) -> Option<Route> {
        match s {
            "system" => Some(Route::System),
            "install" => Some(Route::Install),
            "build" => Some(Route::Build),
            "prebuilt" => Some(Route::Prebuilt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
    Zypper,
    Pacman,
    Apk,
}

/// An os-release value without its quotes: os-release(5) allows `"…"` and `'…'`.
pub(crate) fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

fn ids(os_release: &str) -> Vec<String> {
    os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| {
            unquote(v)
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

impl Manager {
    pub fn from_os_release(text: &str) -> Option<Manager> {
        let ids = ids(text);
        let has = |n: &[&str]| ids.iter().any(|i| n.contains(&i.as_str()));
        if has(&["debian", "ubuntu"]) {
            Some(Manager::Apt)
        } else if has(&["fedora", "rhel", "centos"]) {
            Some(Manager::Dnf)
        } else if has(&["suse", "opensuse", "sles"])
            || ids.iter().any(|i| i.starts_with("opensuse"))
        {
            Some(Manager::Zypper)
        } else if has(&["arch"]) {
            Some(Manager::Pacman)
        } else if has(&["alpine"]) {
            Some(Manager::Apk)
        } else {
            None
        }
    }

    fn column(self) -> usize {
        match self {
            Manager::Apt => 0,
            Manager::Dnf => 1,
            Manager::Zypper => 2,
            Manager::Pacman => 3,
            Manager::Apk => 4,
        }
    }
}

/// Programs (compiler, make, patch, pkg-config) by manager.
fn program_packages(lib: &str) -> Option<[&'static str; 5]> {
    match lib {
        "compiler" => Some(["build-essential", "gcc", "gcc", "base-devel", "build-base"]),
        "make" => Some(["make"; 5]),
        "patch" => Some(["patch"; 5]),
        "pkg-config" => Some([
            "pkg-config",
            "pkgconf-pkg-config",
            "pkg-config",
            "pkgconf",
            "pkgconf",
        ]),
        _ => None,
    }
}

/// Fedora 42 made `tk-devel` Tk 9, which CPython 3.12 and older can't build `_tkinter`
/// against; Tk 8.6 is `tk8-devel` there (measured 2026-10-17: Fedora 41 has only tk-devel
/// 8.6.14; 42 has tk-devel 9.0.0 and tk8-devel 8.6.15; 44 has 9.0.2 and 8.6.17).
const DNF_TK8: &str = "tk8-devel";

/// True on Fedora 42 and later, where `tk-devel` is Tk 9.
pub fn fedora_tk9(os_release: &str) -> bool {
    let id = os_release
        .lines()
        .find_map(|l| l.strip_prefix("ID="))
        .map(unquote);
    let major = os_release
        .lines()
        .find_map(|l| l.strip_prefix("VERSION_ID="))
        .and_then(|v| unquote(v).split('.').next()?.parse::<u32>().ok());
    id == Some("fedora") && major.is_some_and(|m| m >= 42)
}

pub fn packages(m: Manager, deps: &[Dep]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in deps {
        // A Tk that is too new (Tk 9 for CPython <= 3.12) needs the Tk 8 package, which only
        // dnf names separately.
        if d.lib == "tk" && m == Manager::Dnf && matches!(d.missing, Missing::TooNew { .. }) {
            if !out.iter().any(|o| o == DNF_TK8) {
                out.push(DNF_TK8.to_string());
            }
            continue;
        }
        let row = stdlib::lib(d.lib)
            .map(|l| l.packages)
            .or_else(|| program_packages(d.lib));
        if let Some(row) = row {
            for p in row[m.column()].split_whitespace() {
                if !out.iter().any(|o| o == p) {
                    out.push(p.to_string());
                }
            }
        }
    }
    out
}

pub fn install_argv(m: Manager, pkgs: &[String], sudo: bool, assume_yes: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if sudo {
        v.push("sudo".into());
    }
    let (head, yes): (&[&str], &[&str]) = match m {
        Manager::Apt => (&["apt-get", "install"], &["-y"]),
        Manager::Dnf => (&["dnf", "install"], &["-y"]),
        Manager::Zypper => (&["zypper"], &["--non-interactive"]),
        Manager::Pacman => (&["pacman", "-S", "--needed"], &["--noconfirm"]),
        Manager::Apk => (&["apk", "add"], &[]),
    };
    v.extend(head.iter().map(|s| s.to_string()));
    if assume_yes {
        v.extend(yes.iter().map(|s| s.to_string()));
    }
    if m == Manager::Zypper {
        v.push("install".into());
    }
    v.extend(pkgs.iter().cloned());
    v
}

/// `apt-get update`, for a system whose package lists are empty (M8 design §5.3).
pub fn apt_update_argv(sudo: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if sudo {
        v.push("sudo".into());
    }
    v.extend(["apt-get", "update"].map(String::from));
    v
}

/// True when `dir` (apt's `/var/lib/apt/lists`) holds a package list (`*_Packages`, possibly
/// compressed). Minimal images have none, so `apt-get install` can't find any package.
pub fn apt_lists_present(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.file_name().to_string_lossy().contains("_Packages"))
    })
}

pub enum SudoUse {
    Root,
    Allowed,
    Not(&'static str),
}

/// `n_status`: `sudo -n true`'s success and stderr. A password prompt is fine at a terminal.
pub fn classify_sudo(is_root: bool, has_sudo: bool, n_status: Option<(bool, String)>) -> SudoUse {
    if is_root {
        return SudoUse::Root;
    }
    if !has_sudo {
        return SudoUse::Not("no sudo on PATH");
    }
    match n_status {
        Some((true, _)) => SudoUse::Allowed,
        Some((false, err)) if err.contains("password is required") => SudoUse::Allowed,
        _ => SudoUse::Not("sudo says this user may not use it"),
    }
}

/// Some `-devel` packages sit in a repo that is off by default. Rocky, Alma and CentOS Stream
/// have CRB (9) or PowerTools (8) for `dnf config-manager`, which `dnf-plugins-core` provides
/// (minimal images lack it); real RHEL needs `subscription-manager` for its CodeReady Builder
/// repo (M8 design §5.3). `root`: no `sudo`.
pub fn rhel_crb_hint(os_release: &str, root: bool) -> Option<String> {
    if !ids(os_release).iter().any(|i| i == "rhel") {
        return None;
    }
    let version = os_release
        .lines()
        .find_map(|l| l.strip_prefix("VERSION_ID="))?;
    let major: u32 = unquote(version).split('.').next()?.parse().ok()?;
    let id = os_release
        .lines()
        .find_map(|l| l.strip_prefix("ID="))
        .map(unquote);
    let sudo = if root { "" } else { "sudo " };
    if id == Some("rhel") {
        return Some(format!(
            "{sudo}subscription-manager repos --enable codeready-builder-for-rhel-{major}-$(arch)-rpms"
        ));
    }
    let repo = if major >= 9 { "crb" } else { "powertools" };
    Some(format!(
        "{sudo}dnf install dnf-plugins-core && {sudo}dnf config-manager --set-enabled {repo}"
    ))
}

/// The CRB line RHEL-family systems see wherever rpyenv offers or prints a `dnf install`
/// (ruling R15, M8 design §5.3): in the prompt, in the printed report, and after a failed
/// install that showed neither.
pub fn crb_line(os_release: &str, root: bool) -> Option<String> {
    rhel_crb_hint(os_release, root)
        .map(|cmd| format!("Some packages live in the CRB repository; enable it first: {cmd}"))
}

/// The library's name as its project writes it, for the version messages.
fn lib_name(d: &Dep) -> &str {
    match d.lib {
        "openssl" => "OpenSSL",
        "sqlite" => "SQLite",
        "tk" => "Tk",
        other => other,
    }
}

/// The reason a module or program is missing, as the prompt words it (M8 design §5.1).
fn why(d: &Dep) -> String {
    match &d.missing {
        Missing::Program(p) => format!("no {p} program"),
        Missing::Header(h) => format!("no usable {h}"),
        Missing::Link(flags) => format!("can't link {flags}"),
        Missing::TooOld { found, need } => format!("{} {found} found, needs {need}+", lib_name(d)),
        Missing::TooNew { found, need } => format!("{} {found} found, needs {need}", lib_name(d)),
        Missing::Certificates => "no CA certificates".to_string(),
    }
}

/// The prompt's lines (M8 design §5.1). The reason column is as wide as the longest reason
/// among `deps`, so the package names line up within one prompt (ruling R7). Programs and CA
/// certificates are not modules: they get a heading of their own (final review M5). `crb`:
/// the CRB line (ruling R15).
pub fn prompt_lines(
    version: &str,
    deps: &[Dep],
    m: Option<Manager>,
    install: Result<Vec<String>, &'static str>,
    crb: Option<&str>,
) -> Vec<String> {
    let names = |module: bool| {
        deps.iter()
            .filter(|d| is_module(d) == module)
            .map(|d| d.name)
            .fold(Vec::new(), |mut v: Vec<&str>, n| {
                if !v.contains(&n) {
                    v.push(n);
                }
                v
            })
    };
    let (modules, others) = (names(true), names(false));
    let mut out = Vec::new();
    if !modules.is_empty() {
        out.push(format!(
            "pyenv: {version} can't build these modules: {}",
            modules.join(", ")
        ));
    }
    if !others.is_empty() {
        let head = if modules.is_empty() {
            "needs"
        } else {
            "also needs"
        };
        out.push(format!("pyenv: {version} {head}: {}", others.join(", ")));
    }
    let reasons: Vec<String> = deps.iter().map(why).collect();
    let width = reasons.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    for (d, reason) in deps.iter().zip(&reasons) {
        let pkg = m
            .map(|m| packages(m, std::slice::from_ref(d)).join(" "))
            .unwrap_or_default();
        out.push(if pkg.is_empty() {
            format!("  {:<9} {reason}", d.name)
        } else {
            format!("  {:<9} {:<width$}  ({pkg})", d.name, reason)
        });
    }
    out.extend(crb.map(str::to_string));
    out.push("How should rpyenv get them?".into());
    out.push("  [p] print the commands and stop (default)".into());
    match install {
        Ok(argv) => {
            let label = if argv.first().map(String::as_str) == Some("sudo") {
                "install with sudo"
            } else {
                "install"
            };
            out.push(format!("  [i] {label}: {}", argv.join(" ")));
        }
        Err(reason) => out.push(format!("  [i] not offered: {reason}")),
    }
    out
}

/// A standard-library module, not a program (compiler, `make`, `pkg-config`) and not the CA
/// certificates.
fn is_module(d: &Dep) -> bool {
    !matches!(d.missing, Missing::Program(_)) && d.lib != stdlib::CA_KEY
}

pub fn choice(line: &str, install_offered: bool) -> Option<Route> {
    match line.trim().to_ascii_lowercase().as_str() {
        "" | "p" => Some(Route::System),
        "i" if install_offered => Some(Route::Install),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::preflight::{Dep, Missing};

    fn dep(name: &'static str, lib: &'static str) -> Dep {
        Dep {
            name,
            required: true,
            missing: Missing::Header("x.h".into()),
            lib,
        }
    }

    /// Review focus 1.
    #[test]
    fn derivatives_map_to_their_parents_manager() {
        assert!(matches!(
            Manager::from_os_release("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"),
            Some(Manager::Apt)
        ));
        assert!(matches!(
            Manager::from_os_release("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"),
            Some(Manager::Dnf)
        ));
        assert!(matches!(
            Manager::from_os_release("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n"),
            Some(Manager::Zypper)
        ));
        assert!(matches!(
            Manager::from_os_release("ID=endeavouros\nID_LIKE=arch\n"),
            Some(Manager::Pacman)
        ));
        assert!(Manager::from_os_release("ID=gentoo\n").is_none());
    }

    #[test]
    fn packages_are_split_and_deduplicated() {
        let d = [
            dep("dbm", "gdbm"),
            dep("ssl", "openssl"),
            dep("ssl", "openssl"),
        ];
        assert_eq!(
            packages(Manager::Apt, &d),
            ["libgdbm-dev", "libgdbm-compat-dev", "libssl-dev"]
        );
    }

    #[test]
    fn the_install_command_adds_yes_only_when_asked() {
        let p = vec!["libssl-dev".to_string()];
        assert_eq!(
            install_argv(Manager::Apt, &p, true, false),
            ["sudo", "apt-get", "install", "libssl-dev"]
        );
        assert_eq!(
            install_argv(Manager::Apt, &p, false, true),
            ["apt-get", "install", "-y", "libssl-dev"]
        );
        assert_eq!(
            install_argv(Manager::Pacman, &p, true, true),
            [
                "sudo",
                "pacman",
                "-S",
                "--needed",
                "--noconfirm",
                "libssl-dev"
            ]
        );
        assert_eq!(
            install_argv(Manager::Zypper, &p, false, true),
            ["zypper", "--non-interactive", "install", "libssl-dev"]
        );
        assert_eq!(
            install_argv(Manager::Dnf, &p, true, false),
            ["sudo", "dnf", "install", "libssl-dev"]
        );
    }

    #[test]
    fn apt_update_is_its_own_command_and_lists_are_found_by_name() {
        assert_eq!(apt_update_argv(true), ["sudo", "apt-get", "update"]);
        assert_eq!(apt_update_argv(false), ["apt-get", "update"]);
        let d = std::env::temp_dir().join(format!("rpyenv-lists-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(!apt_lists_present(&d));
        std::fs::write(d.join("lock"), "").unwrap();
        assert!(!apt_lists_present(&d));
        std::fs::write(
            d.join("deb.debian.org_debian_dists_bookworm_main_binary-amd64_Packages.lz4"),
            "",
        )
        .unwrap();
        assert!(apt_lists_present(&d));
        std::fs::remove_dir_all(&d).unwrap();
        assert!(!apt_lists_present(&d));
    }

    #[test]
    fn sudo_is_offered_unless_it_cannot_work() {
        assert!(matches!(classify_sudo(true, false, None), SudoUse::Root));
        assert!(matches!(classify_sudo(false, false, None), SudoUse::Not(_)));
        assert!(matches!(
            classify_sudo(false, true, Some((true, String::new()))),
            SudoUse::Allowed
        ));
        assert!(matches!(
            classify_sudo(
                false,
                true,
                Some((false, "sudo: a password is required\n".into()))
            ),
            SudoUse::Allowed
        ));
        assert!(matches!(
            classify_sudo(
                false,
                true,
                Some((false, "Sorry, user jm may not run sudo on box.\n".into()))
            ),
            SudoUse::Not(_)
        ));
    }

    #[test]
    fn rhel_family_gets_the_crb_hint_and_fedora_does_not() {
        assert_eq!(
            rhel_crb_hint(
                "ID=rocky\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n",
                false
            )
            .unwrap(),
            "sudo dnf install dnf-plugins-core && sudo dnf config-manager --set-enabled crb"
        );
        assert_eq!(
            rhel_crb_hint(
                "ID=almalinux\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"8.10\"\n",
                false
            )
            .unwrap(),
            "sudo dnf install dnf-plugins-core && sudo dnf config-manager --set-enabled powertools"
        );
        assert!(rhel_crb_hint("ID=fedora\nVERSION_ID=41\n", false).is_none());
    }

    #[test]
    fn real_rhel_gets_the_subscription_manager_repo_and_centos_stream_gets_crb() {
        assert_eq!(
            rhel_crb_hint("ID=\"rhel\"\nVERSION_ID=\"9.4\"\n", false).unwrap(),
            "sudo subscription-manager repos --enable codeready-builder-for-rhel-9-$(arch)-rpms"
        );
        assert_eq!(
            rhel_crb_hint(
                "ID=centos\nID_LIKE=\"rhel fedora\"\nVERSION_ID=\"9\"\n",
                false
            )
            .unwrap(),
            "sudo dnf install dnf-plugins-core && sudo dnf config-manager --set-enabled crb"
        );
    }

    /// Ruling R15: root gets the commands without sudo; the line names CRB and says "first".
    #[test]
    fn the_crb_line_drops_sudo_for_root() {
        let rocky = "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.6\"\n";
        assert_eq!(
            crb_line(rocky, true).unwrap(),
            "Some packages live in the CRB repository; enable it first: dnf install dnf-plugins-core && dnf config-manager --set-enabled crb"
        );
        assert_eq!(
            rhel_crb_hint("ID=rhel\nVERSION_ID=9.4\n", true).unwrap(),
            "subscription-manager repos --enable codeready-builder-for-rhel-9-$(arch)-rpms"
        );
        assert!(crb_line("ID=debian\nVERSION_ID=\"12\"\n", false).is_none());
    }

    /// Final review T3: os-release(5) allows single quotes too.
    #[test]
    fn single_quoted_os_release_values_are_read() {
        assert!(matches!(
            Manager::from_os_release("ID='ubuntu'\nID_LIKE='debian'\n"),
            Some(Manager::Apt)
        ));
        assert!(matches!(
            Manager::from_os_release("ID='rocky'\nID_LIKE='rhel centos fedora'\n"),
            Some(Manager::Dnf)
        ));
        assert_eq!(
            rhel_crb_hint(
                "ID='almalinux'\nID_LIKE='rhel centos fedora'\nVERSION_ID='9.4'\n",
                true
            )
            .unwrap(),
            "dnf install dnf-plugins-core && dnf config-manager --set-enabled crb"
        );
        assert_eq!(unquote("'a b'"), "a b");
        assert_eq!(unquote("\"a b\""), "a b");
        assert_eq!(unquote("plain"), "plain");
        assert_eq!(unquote("'"), "'");
    }

    #[test]
    fn program_rows_are_by_manager() {
        assert_eq!(
            program_packages("compiler").unwrap()[Manager::Apt.column()],
            "build-essential"
        );
        assert_eq!(
            program_packages("pkg-config").unwrap()[Manager::Dnf.column()],
            "pkgconf-pkg-config"
        );
        assert!(program_packages("no-such-lib").is_none());
    }

    #[test]
    fn the_prompt_lists_the_modules_and_the_offer() {
        let d = [
            Dep {
                name: "ssl",
                required: true,
                missing: Missing::TooOld {
                    found: "1.0.2".into(),
                    need: "1.1.1",
                },
                lib: "openssl",
            },
            Dep {
                name: "uuid",
                required: true,
                missing: Missing::Link("-luuid".into()),
                lib: "libuuid",
            },
        ];
        let argv = install_argv(Manager::Apt, &packages(Manager::Apt, &d), true, false);
        let lines = prompt_lines("3.13.1", &d, Some(Manager::Apt), Ok(argv), None);
        assert_eq!(
            lines,
            [
                "pyenv: 3.13.1 can't build these modules: ssl, uuid",
                "  ssl       OpenSSL 1.0.2 found, needs 1.1.1+  (libssl-dev)",
                "  uuid      can't link -luuid                  (uuid-dev)",
                "How should rpyenv get them?",
                "  [p] print the commands and stop (default)",
                "  [i] install with sudo: sudo apt-get install libssl-dev uuid-dev",
            ]
        );
        let lines = prompt_lines("3.13.1", &d, None, Err("no sudo on PATH"), None);
        assert!(
            lines.contains(&"  [i] not offered: no sudo on PATH".to_string()),
            "{lines:?}"
        );
        let root = install_argv(Manager::Apt, &packages(Manager::Apt, &d), false, false);
        let lines = prompt_lines("3.13.1", &d, Some(Manager::Apt), Ok(root), None);
        assert_eq!(
            lines[5],
            "  [i] install: apt-get install libssl-dev uuid-dev"
        );
    }

    #[test]
    fn the_prompt_without_a_manager_has_no_package_column() {
        let d = [
            Dep {
                name: "ssl",
                required: true,
                missing: Missing::TooOld {
                    found: "1.0.2".into(),
                    need: "1.1.1",
                },
                lib: "openssl",
            },
            Dep {
                name: "uuid",
                required: true,
                missing: Missing::Link("-luuid".into()),
                lib: "libuuid",
            },
        ];
        let lines = prompt_lines("3.13.1", &d, None, Err("no sudo on PATH"), None);
        assert_eq!(lines[1], "  ssl       OpenSSL 1.0.2 found, needs 1.1.1+");
        assert_eq!(lines[2], "  uuid      can't link -luuid");
    }

    /// Final review M5: programs and certificates are not "modules", and a header that compiles
    /// but doesn't link says so. M4: a Tk that is too new gives the version found and needed.
    #[test]
    fn programs_and_certificates_get_their_own_heading() {
        let d = [
            Dep {
                name: "dbm",
                required: true,
                missing: Missing::Link("-lgdbm -lgdbm_compat".into()),
                lib: "gdbm",
            },
            Dep {
                name: "tkinter",
                required: true,
                missing: Missing::TooNew {
                    found: "9.0.2".into(),
                    need: "8.5 or 8.6",
                },
                lib: "tk",
            },
            Dep {
                name: "pkg-config",
                required: true,
                missing: Missing::Program("pkg-config".into()),
                lib: "pkg-config",
            },
            Dep {
                name: "ssl certificates",
                required: true,
                missing: Missing::Certificates,
                lib: stdlib::CA_KEY,
            },
        ];
        let lines = prompt_lines("3.12.14", &d, None, Err("no sudo on PATH"), None);
        assert_eq!(
            lines[0],
            "pyenv: 3.12.14 can't build these modules: dbm, tkinter"
        );
        assert_eq!(
            lines[1],
            "pyenv: 3.12.14 also needs: pkg-config, ssl certificates"
        );
        assert_eq!(lines[2], "  dbm       can't link -lgdbm -lgdbm_compat");
        assert_eq!(lines[3], "  tkinter   Tk 9.0.2 found, needs 8.5 or 8.6");
        let only = prompt_lines("3.12.14", &d[2..], None, Err("x"), None);
        assert_eq!(
            only[0],
            "pyenv: 3.12.14 needs: pkg-config, ssl certificates"
        );
        assert!(!only.iter().any(|l| l.contains("modules")), "{only:?}");
    }

    /// Ruling R15: the CRB line comes before the question.
    #[test]
    fn the_prompt_shows_the_crb_line_before_the_question() {
        let d = [dep("uuid", "libuuid")];
        let crb = "Some packages live in the CRB repository; enable it first: x";
        let lines = prompt_lines("3.12.14", &d, Some(Manager::Dnf), Err("x"), Some(crb));
        let at = lines.iter().position(|l| l == crb).expect("the CRB line");
        assert_eq!(lines[at + 1], "How should rpyenv get them?");
    }

    #[test]
    fn choices() {
        assert!(matches!(choice("", true), Some(Route::System)));
        assert!(matches!(choice("p", true), Some(Route::System)));
        assert!(matches!(choice("I", true), Some(Route::Install)));
        assert!(choice("i", false).is_none());
        assert!(choice("x", true).is_none());
        assert!(matches!(Route::parse("install"), Some(Route::Install)));
        assert!(matches!(Route::parse("prebuilt"), Some(Route::Prebuilt)));
        assert!(Route::parse("yes").is_none());
    }

    /// A Tk that is too new (Tk 9 for CPython <= 3.12) is the Tk 8 package on dnf only;
    /// a Tk that is simply missing keeps the distribution's usual package.
    #[test]
    fn a_too_new_tk_asks_dnf_for_tk8_devel() {
        let too_new = Dep {
            name: "tkinter",
            required: true,
            missing: Missing::TooNew {
                found: "9.0.2".into(),
                need: "8.5 or 8.6",
            },
            lib: "tk",
        };
        assert_eq!(
            packages(Manager::Dnf, std::slice::from_ref(&too_new)),
            ["tk8-devel"]
        );
        assert_eq!(
            packages(Manager::Apt, std::slice::from_ref(&too_new)),
            ["tk-dev"]
        );
        assert_eq!(
            packages(Manager::Dnf, &[dep("tkinter", "tk")]),
            ["tk-devel"]
        );
    }

    #[test]
    fn fedora_42_and_later_ship_tk9_as_tk_devel() {
        assert!(fedora_tk9("ID=fedora\nVERSION_ID=42\n"));
        assert!(fedora_tk9("ID=\"fedora\"\nVERSION_ID=\"44\"\n"));
        assert!(!fedora_tk9("ID=fedora\nVERSION_ID=41\n"));
        assert!(!fedora_tk9(
            "ID=rocky\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n"
        ));
        assert!(!fedora_tk9("ID=fedora\n"));
    }
}
