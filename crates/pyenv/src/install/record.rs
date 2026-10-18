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

/// The record in `text`, or `None` when it has no `definition` line.
pub fn parse(text: &str) -> Option<Record> {
    let mut definition = None;
    let mut libs = Vec::new();
    for l in text.lines() {
        let w: Vec<&str> = l.split_whitespace().collect();
        match w.as_slice() {
            ["definition", d] => definition = Some(d.to_string()),
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
    })
}

/// One line per installed version whose recorded libraries differ from what rpyenv would build
/// today (M8 design §6.3, ruling P12), in name order. `now(library, definition, origin)`: that
/// version, or `None` when unknown (the line then leaves the library out).
pub fn stale_lines(
    versions: &Path,
    now: &dyn Fn(&str, &str, Origin) -> Option<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (name, r) in records(versions) {
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
            let to = store::scripts_home(root, lib, v).join(d);
            let from = versions.join(&name).join("lib").join(d);
            if !to.is_dir() && from.is_dir() {
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
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let n = e.file_name().to_string_lossy().into_owned();
        let ours = n.starts_with("tcl-") || n.starts_with("tk-");
        if ours && !named.contains(&n) {
            let _ = std::fs::remove_dir_all(e.path());
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
            stale_lines(v, &now),
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
            std::fs::write(v.join("lib").join(d).join("init.tcl"), name).unwrap();
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
        assert!(home.join("tk-8.6.18/tk8.6/init.tcl").is_file());
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
}
