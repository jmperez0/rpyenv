//! `versions/<v>/.rpyenv-deps` (M8 design §6.3): the libraries the build route compiled into a
//! version, and where each one's version came from, so `pyenv install` can warn when rpyenv's
//! catalog moves past one of them.

use super::catalog::Origin;
use super::store::{self, Entry};
use std::path::Path;

pub const FILE: &str = ".rpyenv-deps";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The definition the version was built from (its pins are compared, ruling P12).
    pub definition: String,
    /// (library, version, origin)
    pub libs: Vec<(String, String, Origin)>,
    /// (release, arch) of a python-build-standalone build (`prebuilt <release> <arch>`).
    pub prebuilt: Option<(String, String)>,
}

/// The file's text: a comment, `definition <name>`, then `<library> <version> <origin>` per
/// library linked in (pkgconf is a build tool, not a library, and is left out).
pub fn text(definition: &str, entries: &[Entry]) -> String {
    let mut s = String::from(
        "# Libraries rpyenv's build route compiled into this version: name, version, origin.\n",
    );
    s.push_str(&format!("definition {definition}\n"));
    for e in entries.iter().filter(|e| e.key != "pkgconf") {
        s.push_str(&format!("{} {} {}\n", e.key, e.version, e.origin.word()));
    }
    s
}

/// The record of a prebuilt build (M8 design §7): the definition, then
/// `prebuilt <release> <arch>`, which `parse` doesn't take as a library.
pub fn prebuilt_text(definition: &str, row: &super::prebuilt::Row) -> String {
    format!(
        "# The python-build-standalone build rpyenv installed (M8 design §7).\ndefinition {definition}\nprebuilt {} {}\n",
        row.release, row.arch
    )
}

/// The record in `text`, or `None` when it has no `definition` line (or one that is not a
/// plain name).
pub fn parse(text: &str) -> Option<Record> {
    let mut definition = None;
    let mut libs = Vec::new();
    let mut prebuilt = None;
    for l in text.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        match w.as_slice() {
            // The name is looked up as a path later, so it must be one plain name.
            ["definition", d] if super::is_plain_name(d) => definition = Some(d.to_string()),
            ["prebuilt", release, arch] => {
                prebuilt = Some((release.to_string(), arch.to_string()));
            }
            [k, v, o] if !k.starts_with('#') => {
                if let Some(o) = Origin::from_word(o) {
                    libs.push((k.to_string(), v.to_string(), o));
                }
            }
            _ => {}
        }
    }
    Some(Record {
        definition: definition?,
        libs,
        prebuilt,
    })
}

/// (release, OpenSSL version) of a prebuilt table row.
pub type Build = (String, String);

/// One line per installed version whose recorded libraries differ from what rpyenv would build
/// today (M8 design §6.3, ruling P12), in name order. `now(library, definition, origin)`: that
/// version, or `None` when unknown (the line then leaves the library out). A prebuilt version
/// gets a line when `prebuilt_now(definition, arch)`, the table's (release, OpenSSL), is a later
/// release than the one recorded.
pub fn stale_lines(
    versions: &Path,
    now: &dyn Fn(&str, &str, Origin) -> Option<String>,
    prebuilt_now: &dyn Fn(&str, &str) -> Option<Build>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (name, r) in records(versions) {
        if let Some((release, arch)) = &r.prebuilt {
            if let Some((newer, openssl)) = prebuilt_now(&r.definition, arch) {
                if newer.as_str() > release.as_str() {
                    let reinstall = if name == r.definition {
                        name.clone()
                    } else {
                        format!("{}:{name}", r.definition)
                    };
                    out.push(format!(
                        "pyenv: {name} is the python-build-standalone build of {}; rpyenv now offers the one of {} (OpenSSL {openssl}). To reinstall it: pyenv install -f --deps=prebuilt {reinstall}",
                        super::prebuilt::date(release),
                        super::prebuilt::date(&newer)
                    ));
                }
            }
        }
        let (mut was, mut is) = (Vec::new(), Vec::new());
        for (lib, v, origin) in &r.libs {
            if let Some(cur) = now(lib, &r.definition, *origin) {
                if &cur != v {
                    was.push(format!("{lib} {v}"));
                    is.push(format!("{lib} {cur}"));
                }
            }
        }
        if was.is_empty() {
            continue;
        }
        let rebuild = if name == r.definition {
            name.clone()
        } else {
            format!("{}:{name}", r.definition)
        };
        out.push(format!(
            "pyenv: {name} was built with {}; rpyenv now builds {}. To rebuild it: pyenv install -f --deps=build {rebuild}",
            was.join(", "),
            is.join(", ")
        ));
    }
    out
}

/// The records of the installed versions under `versions`, in name order.
fn records(versions: &Path) -> Vec<(String, Record)> {
    let Ok(rd) = std::fs::read_dir(versions) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Record)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let t = std::fs::read_to_string(e.path().join(FILE)).ok()?;
            Some((name, parse(&t)?))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Re-creates each missing shared Tcl/Tk script folder (`<root>/.rpyenv/tcltk/<key>-<version>/`,
/// M8 design §6.3) from an installed version whose record names that release and that holds
/// its own copy in `lib/`. Run by `pyenv install` and `pyenv rehash`; the store is not needed.
pub fn heal_scripts(root: &Path) {
    let versions = root.join("versions");
    for (name, r) in records(&versions) {
        for (lib, v, _) in &r.libs {
            let Some(d) = store::script_dir(lib) else {
                continue;
            };
            if !store::plain_version(v) {
                continue;
            }
            let to = store::scripts_home(root, lib, v).join(d);
            let from = versions.join(&name).join("lib").join(d);
            if !store::is_filled(&to) && store::is_filled(&from) {
                let _ = store::place_tree(&from, &to);
            }
        }
    }
}

/// Removes every shared Tcl/Tk script folder that no installed version's record names (after
/// `pyenv uninstall`, M8 design §6.3).
pub fn prune_scripts(root: &Path) {
    let named: Vec<String> = records(&root.join("versions"))
        .iter()
        .flat_map(|(_, r)| r.libs.iter())
        .filter(|(lib, _, _)| store::script_dir(lib).is_some())
        .map(|(lib, v, _)| format!("{lib}-{v}"))
        .collect();
    let dir = store::tcltk_dir(root);
    store::remove_stale_del(&dir);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let n = e.file_name().to_string_lossy().into_owned();
        let ours = n.starts_with("tcl-") || n.starts_with("tk-");
        if ours && !named.contains(&n) {
            // Renamed first: a removal that fails or races never leaves a half folder under
            // the real name.
            let gone = dir.join(format!(".del-{n}-{}", std::process::id()));
            if std::fs::rename(e.path(), &gone).is_ok() {
                let _ = std::fs::remove_dir_all(&gone);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_record_round_trips_and_leaves_out_pkgconf() {
        let e = |key: &'static str, v: &str, origin| Entry {
            key,
            version: v.into(),
            origin,
            dir: PathBuf::from("/s"),
        };
        let t = text(
            "3.12.15",
            &[
                e("pkgconf", "3.0.9", Origin::Catalog),
                e("openssl", "3.6.5", Origin::Definition),
                e("sqlite", "3.54.1", Origin::Catalog),
            ],
        );
        assert!(t.starts_with('#'), "{t}");
        assert!(
            t.contains("\ndefinition 3.12.15\nopenssl 3.6.5 definition\nsqlite 3.54.1 catalog\n"),
            "{t}"
        );
        assert!(!t.contains("pkgconf"), "{t}");
        let r = parse(&t).unwrap();
        assert_eq!(r.definition, "3.12.15");
        assert_eq!(
            r.libs,
            [
                (
                    "openssl".to_string(),
                    "3.6.5".to_string(),
                    Origin::Definition
                ),
                ("sqlite".to_string(), "3.54.1".to_string(), Origin::Catalog),
            ]
        );
        assert!(
            parse("openssl 3.6.5 catalog\n").is_none(),
            "no definition line"
        );
    }

    #[test]
    fn the_prebuilt_record_names_the_definition_release_and_arch_and_no_library() {
        let row = crate::install::prebuilt::Row {
            name: "3.13.16".into(),
            arch: "x86_64".into(),
            release: "20261020".into(),
            sha256: "0".repeat(64),
            openssl: "3.5.9".into(),
            tk: "9.0".into(),
        };
        let t = prebuilt_text("3.13.16", &row);
        assert_eq!(
            t,
            "# The python-build-standalone build rpyenv installed (M8 design §7).\ndefinition 3.13.16\nprebuilt 20261020 x86_64\n"
        );
        let r = parse(&t).unwrap();
        assert_eq!(r.definition, "3.13.16");
        assert!(r.libs.is_empty(), "{:?}", r.libs);
    }

    #[test]
    fn a_prebuilt_record_names_a_newer_build_in_the_table() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path();
        std::fs::create_dir_all(v.join("mine")).unwrap();
        std::fs::write(
            v.join("mine").join(FILE),
            "# c\ndefinition 3.13.16\nprebuilt 20261020 x86_64\n",
        )
        .unwrap();
        let none = |_: &str, _: &str, _: Origin| None;
        let newer = |d: &str, a: &str| {
            (d == "3.13.16" && a == "x86_64")
                .then(|| ("20261105".to_string(), "3.5.10".to_string()))
        };
        assert_eq!(
            stale_lines(v, &none, &newer),
            vec!["pyenv: mine is the python-build-standalone build of 2026-10-20; rpyenv now offers the one of 2026-11-05 (OpenSSL 3.5.10). To reinstall it: pyenv install -f --deps=prebuilt 3.13.16:mine".to_string()]
        );
        let same = |_: &str, _: &str| Some(("20261020".to_string(), "3.5.9".to_string()));
        assert!(stale_lines(v, &none, &same).is_empty());
        let gone = |_: &str, _: &str| None;
        assert!(stale_lines(v, &none, &gone).is_empty());
        let r = parse("definition 3.13.16\nprebuilt 20261020 x86_64\n").unwrap();
        assert_eq!(r.prebuilt, Some(("20261020".into(), "x86_64".into())));
        assert!(r.libs.is_empty());
    }

    #[test]
    fn stale_lines_name_each_changed_library_and_how_to_rebuild() {
        let tmp = tempfile::tempdir().unwrap();
        let v = tmp.path();
        let put = |name: &str, body: &str| {
            std::fs::create_dir_all(v.join(name)).unwrap();
            std::fs::write(v.join(name).join(FILE), body).unwrap();
        };
        put(
            "3.12.15",
            "definition 3.12.15
openssl 3.6.4 definition
sqlite 3.54.0 catalog
zlib 1.3.2 catalog
",
        );
        put(
            "fresh",
            "definition 3.13.16
zlib 1.3.2 catalog
",
        );
        put(
            "mine",
            "definition 3.14.8
sqlite 3.54.0 catalog
",
        );
        std::fs::create_dir_all(v.join("no-record")).unwrap();
        let now = |lib: &str, def: &str, o: Origin| -> Option<String> {
            match (lib, o) {
                ("openssl", Origin::Definition) => (def == "3.12.15").then(|| "3.6.5".to_string()),
                ("sqlite", _) => Some("3.54.1".into()),
                ("zlib", _) => Some("1.3.2".into()),
                _ => None,
            }
        };
        assert_eq!(
            stale_lines(v, &now, &|_, _| None),
            [
                "pyenv: 3.12.15 was built with openssl 3.6.4, sqlite 3.54.0; rpyenv now builds openssl 3.6.5, sqlite 3.54.1. To rebuild it: pyenv install -f --deps=build 3.12.15",
                "pyenv: mine was built with sqlite 3.54.0; rpyenv now builds sqlite 3.54.1. To rebuild it: pyenv install -f --deps=build 3.14.8:mine",
            ]
        );
    }

    /// A version built through the store, with its own copies of Tcl's and Tk's scripts.
    fn tcltk_version(root: &Path, name: &str, release: &str) {
        let v = root.join("versions").join(name);
        for d in ["tcl8.6", "tk8.6"] {
            std::fs::create_dir_all(v.join("lib").join(d)).unwrap();
            let marker = if d == "tcl8.6" { "init.tcl" } else { "tk.tcl" };
            std::fs::write(v.join("lib").join(d).join(marker), name).unwrap();
        }
        std::fs::write(
            v.join(FILE),
            format!("definition {name}\ntcl {release} catalog\ntk {release} catalog\n"),
        )
        .unwrap();
    }

    /// Final review I1: a deleted shared script folder comes back from a version's own copy.
    #[test]
    fn heal_recreates_the_shared_script_folders_from_an_installed_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        tcltk_version(root, "3.12.15", "8.6.18");
        // A version whose copies are gone can't supply them; one without Tcl names nothing.
        std::fs::create_dir_all(root.join("versions/3.13.9")).unwrap();
        std::fs::write(
            root.join("versions/3.13.9").join(FILE),
            "definition 3.13.9\ntcl 8.6.17 catalog\n",
        )
        .unwrap();
        heal_scripts(root);
        let home = store::tcltk_dir(root);
        assert_eq!(
            std::fs::read_to_string(home.join("tcl-8.6.18/tcl8.6/init.tcl")).unwrap(),
            "3.12.15"
        );
        assert!(home.join("tk-8.6.18/tk8.6/tk.tcl").is_file());
        assert!(!home.join("tcl-8.6.17").exists());
        // An existing folder is left as it is.
        std::fs::write(home.join("tcl-8.6.18/tcl8.6/init.tcl"), "kept").unwrap();
        heal_scripts(root);
        assert_eq!(
            std::fs::read_to_string(home.join("tcl-8.6.18/tcl8.6/init.tcl")).unwrap(),
            "kept"
        );
    }

    /// Final review I1: `pyenv uninstall` keeps the folders a remaining version names.
    #[test]
    fn prune_removes_only_the_script_folders_no_version_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        tcltk_version(root, "a", "8.6.18");
        tcltk_version(root, "b", "8.6.17");
        heal_scripts(root);
        let home = store::tcltk_dir(root);
        std::fs::create_dir_all(home.join("other")).unwrap();
        std::fs::remove_dir_all(root.join("versions/b")).unwrap();
        prune_scripts(root);
        let mut left: Vec<String> = std::fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["other", "tcl-8.6.18", "tk-8.6.18"]);
    }

    /// H1: a script folder that was partly removed (its marker is gone) is refilled by heal.
    // allowlist D-123
    #[test]
    fn heal_refills_a_script_folder_that_lost_its_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        tcltk_version(root, "3.12.15", "8.6.18");
        heal_scripts(root);
        let home = store::tcltk_dir(root);
        let tcl = home.join("tcl-8.6.18/tcl8.6");
        let tk = home.join("tk-8.6.18/tk8.6");
        std::fs::remove_file(tcl.join("init.tcl")).unwrap();
        std::fs::write(tcl.join("stray"), "x").unwrap();
        std::fs::remove_dir_all(&tk).unwrap();
        std::fs::create_dir(&tk).unwrap();
        heal_scripts(root);
        assert!(tcl.join("init.tcl").is_file());
        assert!(
            !tcl.join("stray").exists(),
            "the broken folder was replaced"
        );
        assert!(tk.join("tk.tcl").is_file());
        let leftovers: Vec<String> = std::fs::read_dir(home.join("tcl-8.6.18"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, ["tcl8.6"], "no .tmp- or .del- folder is left");
    }

    /// H2: a record's version is joined into a path only when it is plain.
    #[test]
    fn heal_skips_record_versions_that_are_not_plain() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        tcltk_version(root, "a", "8.6.18");
        for bad in ["../x", "a/b", "..", ".", "a b", r"x\y", ""] {
            std::fs::write(
                root.join("versions/a").join(FILE),
                format!(
                    "definition a
tcl {bad} catalog
"
                ),
            )
            .unwrap();
            heal_scripts(root);
        }
        assert!(!root.join(".rpyenv/x").exists());
        assert!(!root.join(".rpyenv/tcltk").exists(), "nothing was placed");
        std::fs::write(
            root.join("versions/a").join(FILE),
            "definition a
tcl 8.6.18+x_1-2 catalog
",
        )
        .unwrap();
        heal_scripts(root);
        assert!(root
            .join(".rpyenv/tcltk/tcl-8.6.18+x_1-2/tcl8.6/init.tcl")
            .is_file());
        assert!(store::plain_version("8.6.18"));
        assert!(!store::plain_version("../x"));
        // The definition name is joined into a lookup path too.
        assert!(parse(
            "definition ../x
"
        )
        .is_none());
        assert!(parse(
            "definition a
"
        )
        .is_some());
    }

    /// H1: uninstall's prune renames a folder before removing it, so a removal that fails
    /// leaves no half folder under the real name.
    #[cfg(unix)]
    #[test]
    fn prune_leaves_no_half_folder_under_the_real_name_when_removal_fails() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let home = store::tcltk_dir(root);
        let locked = home.join("tcl-8.6.9/tcl8.6/sub");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("f"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
        std::fs::create_dir_all(root.join("versions")).unwrap();
        // As root the 0500 folder is removable anyway, so the assertion would pass without
        // the rename: it runs only for a user that cannot remove it.
        let root_user = unsafe { libc::geteuid() } == 0;
        prune_scripts(root);
        if !root_user {
            assert!(!home.join("tcl-8.6.9").exists());
            assert!(
                std::fs::read_dir(&home).unwrap().any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".del-tcl-8.6.9-")),
                "the failed removal left only the renamed folder"
            );
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).ok();
        if let Ok(rd) = std::fs::read_dir(&home) {
            for e in rd.flatten() {
                let l = e.path().join("tcl8.6/sub");
                std::fs::set_permissions(&l, std::fs::Permissions::from_mode(0o700)).ok();
            }
        }
    }
}
