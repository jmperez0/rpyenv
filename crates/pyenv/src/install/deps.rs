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

fn ids(os_release: &str) -> Vec<String> {
    os_release
        .lines()
        .filter_map(|l| l.strip_prefix("ID=").or_else(|| l.strip_prefix("ID_LIKE=")))
        .flat_map(|v| {
            v.trim_matches('"')
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

pub fn packages(m: Manager, deps: &[Dep]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in deps {
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
/// have CRB (9) or PowerTools (8) for `dnf config-manager`; real RHEL needs
/// `subscription-manager` for its CodeReady Builder repo (M8 design §5.1).
pub fn rhel_crb_hint(os_release: &str) -> Option<String> {
    if !ids(os_release).iter().any(|i| i == "rhel") {
        return None;
    }
    let major: u32 = os_release
        .lines()
        .find_map(|l| l.strip_prefix("VERSION_ID="))?
        .trim_matches('"')
        .split('.')
        .next()?
        .parse()
        .ok()?;
    let id = os_release
        .lines()
        .find_map(|l| l.strip_prefix("ID="))
        .map(|v| v.trim_matches('"'));
    if id == Some("rhel") {
        return Some(format!(
            "sudo subscription-manager repos --enable codeready-builder-for-rhel-{major}-$(arch)-rpms"
        ));
    }
    let repo = if major >= 9 { "crb" } else { "powertools" };
    Some(format!("sudo dnf config-manager --set-enabled {repo}"))
}

/// The reason a module or program is missing, as the prompt words it (M8 design §5.1).
fn why(d: &Dep) -> String {
    match &d.missing {
        Missing::Program(p) => format!("no {p} program"),
        Missing::Header(h) | Missing::Link(h) => format!("no usable {h}"),
        Missing::TooOld { found, need } => {
            let name = match d.lib {
                "openssl" => "OpenSSL",
                "sqlite" => "SQLite",
                "zstd" => "zstd",
                other => other,
            };
            format!("{name} {found} found, needs {need}+")
        }
        Missing::Certificates => "no CA certificates".to_string(),
    }
}

/// The prompt's lines (M8 design §5.1). The reason column is as wide as the longest reason
/// among `deps`, so the package names line up within one prompt (ruling R7).
pub fn prompt_lines(
    version: &str,
    deps: &[Dep],
    m: Option<Manager>,
    install: Result<Vec<String>, &'static str>,
) -> Vec<String> {
    let names: Vec<&str> = deps.iter().map(|d| d.name).fold(Vec::new(), |mut v, n| {
        if !v.contains(&n) {
            v.push(n);
        }
        v
    });
    let mut out = vec![format!(
        "pyenv: {version} can't build these modules: {}",
        names.join(", ")
    )];
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
            rhel_crb_hint("ID=rocky\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"9.4\"\n")
                .unwrap(),
            "sudo dnf config-manager --set-enabled crb"
        );
        assert_eq!(
            rhel_crb_hint("ID=almalinux\nID_LIKE=\"rhel centos fedora\"\nVERSION_ID=\"8.10\"\n")
                .unwrap(),
            "sudo dnf config-manager --set-enabled powertools"
        );
        assert!(rhel_crb_hint("ID=fedora\nVERSION_ID=41\n").is_none());
    }

    #[test]
    fn real_rhel_gets_the_subscription_manager_repo_and_centos_stream_gets_crb() {
        assert_eq!(
            rhel_crb_hint("ID=\"rhel\"\nVERSION_ID=\"9.4\"\n").unwrap(),
            "sudo subscription-manager repos --enable codeready-builder-for-rhel-9-$(arch)-rpms"
        );
        assert_eq!(
            rhel_crb_hint("ID=centos\nID_LIKE=\"rhel fedora\"\nVERSION_ID=\"9\"\n").unwrap(),
            "sudo dnf config-manager --set-enabled crb"
        );
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
                missing: Missing::Link("uuid/uuid.h".into()),
                lib: "libuuid",
            },
        ];
        let argv = install_argv(Manager::Apt, &packages(Manager::Apt, &d), true, false);
        let lines = prompt_lines("3.13.1", &d, Some(Manager::Apt), Ok(argv));
        assert_eq!(
            lines,
            [
                "pyenv: 3.13.1 can't build these modules: ssl, uuid",
                "  ssl       OpenSSL 1.0.2 found, needs 1.1.1+  (libssl-dev)",
                "  uuid      no usable uuid/uuid.h              (uuid-dev)",
                "How should rpyenv get them?",
                "  [p] print the commands and stop (default)",
                "  [i] install with sudo: sudo apt-get install libssl-dev uuid-dev",
            ]
        );
        let lines = prompt_lines("3.13.1", &d, None, Err("no sudo on PATH"));
        assert!(
            lines.contains(&"  [i] not offered: no sudo on PATH".to_string()),
            "{lines:?}"
        );
        let root = install_argv(Manager::Apt, &packages(Manager::Apt, &d), false, false);
        let lines = prompt_lines("3.13.1", &d, Some(Manager::Apt), Ok(root));
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
                missing: Missing::Link("uuid/uuid.h".into()),
                lib: "libuuid",
            },
        ];
        let lines = prompt_lines("3.13.1", &d, None, Err("no sudo on PATH"));
        assert_eq!(lines[1], "  ssl       OpenSSL 1.0.2 found, needs 1.1.1+");
        assert_eq!(lines[2], "  uuid      no usable uuid/uuid.h");
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
}
