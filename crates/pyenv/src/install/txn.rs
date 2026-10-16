//! The install transaction (spec §9.3, plan Decision 4): a per-name OS lock, a staging directory
//! inside `versions/`, a swap that keeps the previous version until commit, and rollback on
//! drop.

use rpyenv_core::flavor::Flavor;
use std::path::{Path, PathBuf};

/// Present in `versions/<name>` from the move until commit: a version that still has it
/// was interrupted and isn't complete.
pub const MARKER: &str = ".rpyenv-incomplete";

/// "Installed" for `pyenv install`, minus interrupted installs (plan M2b Decision 9): upstream
/// pyenv's `bin/` test, or pyenv-win's "the folder exists".
pub fn is_complete_for(dir: &Path, flavor: Flavor) -> bool {
    let present = match flavor {
        Flavor::Pyenv => dir.join("bin").is_dir(),
        Flavor::PyenvWin => dir.is_dir(),
    };
    present && !dir.join(MARKER).exists()
}

/// The Linux flavor's test (M2a's callers).
pub fn is_complete(dir: &Path) -> bool {
    is_complete_for(dir, Flavor::Pyenv)
}

/// Why a reinstall kept `.old-<name>`, in the flavor's line ending.
pub fn kept_message(flavor: Flavor, old: &Path, e: &std::io::Error) -> String {
    format!(
        "pyenv: kept the previous installation at {}: {e}{}",
        old.display(),
        flavor.eol()
    )
}

pub struct Txn {
    versions: PathBuf,
    name: String,
    flavor: Flavor,
    /// Held for the transaction's life; the OS releases it on drop or crash. The file is
    /// never deleted: deleting a lock file reintroduces the takeover race.
    _lock: std::fs::File,
    stage: PathBuf,
    old: Option<PathBuf>,
    placed: bool,
    conda: bool,
    done: bool,
}

impl Txn {
    /// The Linux flavor's transaction (M2a's callers).
    pub fn begin(versions: &Path, name: &str) -> Result<Txn, String> {
        Txn::begin_for(versions, name, Flavor::Pyenv)
    }

    pub fn begin_for(versions: &Path, name: &str, flavor: Flavor) -> Result<Txn, String> {
        // Every path below is `versions/<prefix><name>` and some are deleted (review I2).
        // Win32 strips trailing dots and spaces, so `...` would name `versions` itself.
        if !super::is_plain_name(name)
            || (flavor == Flavor::PyenvWin && !super::is_safe_win_segment(name))
        {
            return Err(format!("pyenv: invalid version name: {name}"));
        }
        std::fs::create_dir_all(versions)
            .map_err(|e| format!("pyenv: cannot create {}: {e}", versions.display()))?;
        let locks = versions.parent().unwrap_or(versions).join(".locks");
        std::fs::create_dir_all(&locks)
            .map_err(|e| format!("pyenv: cannot create {}: {e}", locks.display()))?;
        let lock_path = locks.join(format!("install-{name}"));
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| format!("pyenv: cannot create {}: {e}", lock_path.display()))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "pyenv: another install of {name} is in progress ({})",
                    lock_path.display()
                ))
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(format!("pyenv: cannot lock {}: {e}", lock_path.display()))
            }
        }
        // Recovery from a process killed mid-swap (we hold the lock, so nobody else is
        // installing this name).
        let target = versions.join(name);
        let old = versions.join(format!(".old-{name}"));
        if old.symlink_metadata().is_ok() {
            if target.symlink_metadata().is_err() {
                let _ = std::fs::rename(&old, &target);
            } else if is_complete_for(&target, flavor) {
                if is_link(&old) {
                    // A linked version set aside by `-f` (final review M2): never reached
                    // through, only the link goes.
                    let _ = remove_link(&old);
                } else if (if target.join("conda-meta").is_dir() {
                    // A conda tree: only `envs/` is ever carried over (spec D7).
                    carry_envs(&old, &target)
                } else {
                    carry_over(&old, &target)
                })
                .is_ok()
                {
                    // Killed during or before the carry-over: finish it, and keep `.old` if
                    // it fails (`place` then refuses to overwrite it).
                    let _ = std::fs::remove_dir_all(&old);
                }
            } else {
                // Killed after the final rename: the target is the new, unfinished tree
                // and `.old` (a folder or a link, renamed back as it is) is the only good copy.
                let _ = std::fs::remove_dir_all(&target);
                let _ = std::fs::rename(&old, &target);
            }
        }
        let stage = versions.join(format!(".tmp-{name}"));
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage)
            .map_err(|e| format!("pyenv: cannot create {}: {e}", stage.display()))?;
        Ok(Txn {
            versions: versions.to_path_buf(),
            name: name.to_string(),
            flavor,
            _lock: lock,
            stage,
            old: None,
            placed: false,
            conda: false,
            done: false,
        })
    }

    pub fn stage_dir(&self) -> &Path {
        &self.stage
    }

    pub fn target(&self) -> PathBuf {
        self.versions.join(&self.name)
    }

    pub fn placed(&self) -> bool {
        self.placed
    }

    /// Moves the staged prefix to `versions/<name>`, setting any existing version aside.
    pub fn place(&mut self, staged_prefix: &Path) -> std::io::Result<()> {
        let target = self.target();
        // The marker goes in before the move, so placing is one atomic rename and a failed
        // marker write changes nothing.
        std::fs::write(staged_prefix.join(MARKER), "")?;
        if target.symlink_metadata().is_ok() {
            let old = self.versions.join(format!(".old-{}", self.name));
            if old.symlink_metadata().is_ok() {
                return Err(std::io::Error::other(format!(
                    "{} exists; refusing to overwrite it",
                    old.display()
                )));
            }
            std::fs::rename(&target, &old)?;
            self.old = Some(old);
        }
        std::fs::rename(staged_prefix, &target)?;
        self.placed = true;
        Ok(())
    }

    /// M7b design §4.3 step 5: conda can't be moved after installing, so it installs into
    /// `versions/<name>` itself. As `place` does, an existing version is set aside; the target
    /// is then an empty folder holding the marker, before the installer runs.
    pub fn place_empty(&mut self) -> std::io::Result<()> {
        let empty = self.stage.join("prefix");
        std::fs::create_dir_all(&empty)?;
        self.conda = true;
        self.place(&empty)
    }

    /// Completes the install. The previous version's `envs/` and site-packages entries are
    /// carried over first; if that fails, the previous tree is kept at `.old-<name>`. A previous
    /// version that is a link is never followed: only the link is removed.
    pub fn commit(mut self) -> std::io::Result<()> {
        std::fs::remove_file(self.target().join(MARKER))?;
        if let Some(old) = self.old.take() {
            if is_link(&old) {
                // The previous "version" was a link (a junction, or a symlink): carrying over
                // would move files out of whatever it points at (final review M2). Only the
                // link goes.
                let _ = remove_link(&old);
            } else {
                // The list is computed before the carry (it reads both trees as they are) and
                // printed only when the carry succeeded.
                let mut left = None;
                let carried = if self.conda {
                    let (conda, pip) = not_carried(&old, &self.target());
                    left = not_carried_line(&self.name, &conda, &pip);
                    carry_envs(&old, &self.target())
                } else {
                    carry_over(&old, &self.target())
                };
                match carried {
                    Ok(()) => {
                        if let Some(line) = left {
                            rpyenv_core::textout::write(true, &format!("{line}\n"));
                        }
                        let _ = std::fs::remove_dir_all(&old);
                    }
                    Err(e) => rpyenv_core::textout::write(
                        // pyenv-win prints everything on stdout (reference, "Conventions").
                        self.flavor == Flavor::Pyenv,
                        &kept_message(self.flavor, &old, &e),
                    ),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&self.stage);
        self.done = true;
        Ok(())
    }
}

/// A symlink, or on Windows a junction (Rust reports both as `is_symlink`), not followed.
fn is_link(p: &Path) -> bool {
    p.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// Removes the link `p` itself: a directory link is a directory entry on Windows, a file on Unix.
fn remove_link(p: &Path) -> std::io::Result<()> {
    if cfg!(windows) {
        std::fs::remove_dir(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// `envs/` (pyenv-virtualenv): the whole folder moves when `new` has none; when `new` has one
/// (a real conda installer creates an empty `envs/`), each entry of `old/envs` that `new/envs`
/// lacks moves in, and an entry in both keeps `new`'s. Rename only, so a failure loses nothing.
fn carry_envs(old: &Path, new: &Path) -> std::io::Result<()> {
    let envs = old.join("envs");
    if envs.symlink_metadata().is_err() {
        return Ok(());
    }
    let to = new.join("envs");
    match to.symlink_metadata() {
        Err(_) => std::fs::rename(&envs, &to)?,
        Ok(m) if m.file_type().is_dir() => {
            for e in std::fs::read_dir(&envs)? {
                let e = e?;
                let dest = to.join(e.file_name());
                if dest.symlink_metadata().is_err() {
                    std::fs::rename(e.path(), dest)?;
                }
            }
        }
        Ok(_) => {}
    }
    Ok(())
}

/// Moves what upstream's build over the old tree would have kept from `old` into the new
/// tree `new`: `envs/` (pyenv-virtualenv; see `carry_envs`), and each entry that `new`
/// lacks in `bin/` and `Scripts\` (scripts pip installed) and in site-packages (`lib/python*/site-packages`,
/// `lib/pypy*/site-packages`, and `Lib/site-packages` for Windows). Entries in both keep `new`'s copy. Everything moves
/// by rename, which moves a symlink as a link, so a failure part way loses nothing: what has
/// not moved is still in `old`.
fn carry_over(old: &Path, new: &Path) -> std::io::Result<()> {
    carry_envs(old, new)?;
    // `bin/` (pyenv) and `Scripts\` (pyenv-win): console scripts pip installed.
    let mut dirs: Vec<PathBuf> = vec![PathBuf::from("bin"), PathBuf::from("Scripts")];
    if let Ok(rd) = std::fs::read_dir(old.join("lib")) {
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name();
            // `lib/python3.12/site-packages`, and PyPy's `lib/pypy3.11/site-packages`.
            let n = name.to_string_lossy();
            if n.starts_with("python") || n.starts_with("pypy") {
                dirs.push(Path::new("lib").join(name).join("site-packages"));
            }
        }
    }
    dirs.push(Path::new("Lib").join("site-packages"));
    for rel in dirs {
        let from = old.join(&rel);
        let is_dir = from
            .symlink_metadata()
            .map(|m| m.file_type().is_dir())
            .unwrap_or(false);
        if !is_dir {
            continue;
        }
        let to = new.join(&rel);
        let mut entries = std::fs::read_dir(&from)?.peekable();
        if entries.peek().is_none() {
            continue;
        }
        std::fs::create_dir_all(&to)?;
        for e in entries {
            let e = e?;
            let dest = to.join(e.file_name());
            if dest.symlink_metadata().is_err() {
                std::fs::rename(e.path(), dest)?;
            }
        }
    }
    Ok(())
}

/// Spec D7: the base packages of a conda version's previous copy that a reinstall doesn't
/// carry over. (conda names, pip names), each sorted.
pub fn not_carried(old: &Path, new: &Path) -> (Vec<String>, Vec<String>) {
    use std::collections::BTreeSet;
    // `numpy-1.26.4-py312h1_0.json`: the name is what comes before the version and the build.
    let records = |base: &Path| -> BTreeSet<String> {
        std::fs::read_dir(base.join("conda-meta"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|e| {
                let f = e.file_name().to_string_lossy().into_owned();
                let stem = f.strip_suffix(".json")?;
                let mut parts = stem.rsplitn(3, '-');
                let (_build, _version, name) = (parts.next()?, parts.next()?, parts.next()?);
                Some(name.to_string())
            })
            .collect()
    };
    // `requests-2.32.3.dist-info`: the project is what comes before the first `-`.
    let dist_infos = |base: &Path, pip_only: bool| -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut sites: Vec<PathBuf> = std::fs::read_dir(base.join("lib"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|l| l.file_name().to_string_lossy().starts_with("python"))
            .map(|l| l.path().join("site-packages"))
            .collect();
        if sites.is_empty() {
            // A Windows conda tree: `Lib\site-packages` (M7c Task 8 review M4).
            sites.push(base.join("Lib").join("site-packages"));
        }
        for site in sites {
            for e in std::fs::read_dir(site)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
            {
                let f = e.file_name().to_string_lossy().into_owned();
                let Some(stem) = f.strip_suffix(".dist-info") else {
                    continue;
                };
                let installer =
                    std::fs::read_to_string(e.path().join("INSTALLER")).unwrap_or_default();
                if pip_only && installer.trim() != "pip" {
                    continue;
                }
                out.insert(stem.split('-').next().unwrap_or(stem).to_string());
            }
        }
        out
    };
    let conda = records(old).difference(&records(new)).cloned().collect();
    let pip = dist_infos(old, true)
        .difference(&dist_infos(new, false))
        .cloned()
        .collect();
    (conda, pip)
}

/// The line spec D7 prints, or `None` when nothing was left behind.
pub fn not_carried_line(name: &str, conda: &[String], pip: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    if !conda.is_empty() {
        parts.push(format!("conda install {}", conda.join(" ")));
    }
    if !pip.is_empty() {
        parts.push(format!("pip install {}", pip.join(" ")));
    }
    (!parts.is_empty()).then(|| {
        format!(
            "rpyenv: the previous {name}'s base packages weren't carried over (only envs/ was); to reinstall them: {}",
            parts.join("; ")
        )
    })
}

impl Drop for Txn {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let target = self.target();
        if self.placed {
            let _ = std::fs::remove_dir_all(&target);
        }
        if let Some(old) = self.old.take() {
            let _ = std::fs::rename(&old, &target);
        }
        let _ = std::fs::remove_dir_all(&self.stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn not_carried_parses_names_with_dashes_and_subtracts_the_new_base() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("old"), t.path().join("new"));
        touch(
            &old.join("conda-meta/python-dateutil-2.9.0-pyhd8ed1ab_0.json"),
            "{}",
        );
        touch(&old.join("conda-meta/numpy-1.26.4-py312h1_0.json"), "{}");
        touch(&old.join("conda-meta/history"), "");
        touch(&new.join("conda-meta/numpy-1.26.4-py312h1_0.json"), "{}");
        let (conda, pip) = not_carried(&old, &new);
        assert_eq!(conda, names(&["python-dateutil"]));
        assert!(pip.is_empty());
    }

    #[test]
    fn not_carried_lists_pip_projects_only() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("old"), t.path().join("new"));
        let sp = old.join("lib/python3.12/site-packages");
        touch(&sp.join("numpy-1.26.4.dist-info/INSTALLER"), "conda\n");
        touch(&sp.join("requests-2.32.3.dist-info/INSTALLER"), "pip\n");
        touch(&sp.join("six-1.16.0.dist-info/INSTALLER"), "pip\n");
        touch(
            &new.join("lib/python3.12/site-packages/six-1.17.0.dist-info/INSTALLER"),
            "conda\n",
        );
        let (conda, pip) = not_carried(&old, &new);
        assert!(conda.is_empty());
        assert_eq!(pip, names(&["requests"]));
    }

    #[test]
    fn not_carried_reads_a_windows_conda_trees_lib_site_packages() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("old"), t.path().join("new"));
        touch(
            &old.join("Lib/site-packages/requests-2.32.3.dist-info/INSTALLER"),
            "pip\n",
        );
        touch(
            &old.join("Lib/site-packages/numpy-1.26.4.dist-info/INSTALLER"),
            "conda\n",
        );
        touch(
            &new.join("Lib/site-packages/numpy-1.26.4.dist-info/INSTALLER"),
            "conda\n",
        );
        let (conda, pip) = not_carried(&old, &new);
        assert!(conda.is_empty());
        assert_eq!(pip, names(&["requests"]));
    }

    #[test]
    fn not_carried_line_shapes() {
        assert_eq!(not_carried_line("m", &[], &[]), None);
        let l = not_carried_line("m", &[], &names(&["requests"])).unwrap();
        assert!(
            l.ends_with("to reinstall them: pip install requests"),
            "{l}"
        );
        let tail = l.split("to reinstall them: ").nth(1).unwrap();
        assert!(!l.contains("conda install") && !tail.contains("; "), "{l}");
        let l = not_carried_line("m", &names(&["a", "b"]), &names(&["c"])).unwrap();
        assert!(l.ends_with("conda install a b; pip install c"), "{l}");
    }

    #[test]
    fn recovery_of_a_killed_conda_commit_carries_only_envs() {
        let t = tempfile::tempdir().unwrap();
        let versions = t.path().join("versions");
        let (target, old) = (versions.join("m"), versions.join(".old-m"));
        touch(&target.join("conda-meta/history"), "");
        touch(&target.join("bin/python"), "");
        std::fs::create_dir_all(target.join("envs")).unwrap();
        touch(&old.join("envs/work/conda-meta/history"), "");
        touch(&old.join("bin/my-tool"), "");
        touch(
            &old.join("lib/python3.12/site-packages/numpy/__init__.py"),
            "",
        );
        let txn = Txn::begin(&versions, "m").unwrap();
        drop(txn);
        assert!(target.join("envs/work/conda-meta/history").is_file());
        assert!(!target.join("bin/my-tool").exists());
        assert!(!target.join("lib/python3.12/site-packages/numpy").exists());
        assert!(!old.exists());
    }

    #[test]
    fn carry_envs_moves_what_the_new_envs_lacks_and_keeps_the_new_copy() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("old"), t.path().join("new"));
        touch(&old.join("envs/a/from"), "old");
        touch(&old.join("envs/b/from"), "old");
        touch(&new.join("envs/a/from"), "new");
        carry_envs(&old, &new).unwrap();
        assert_eq!(
            std::fs::read_to_string(new.join("envs/b/from")).unwrap(),
            "old"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("envs/a/from")).unwrap(),
            "new"
        );
        assert!(old.join("envs/a/from").is_file());
        assert!(!old.join("envs/b").exists());
    }

    #[test]
    fn carry_envs_moves_the_whole_folder_when_the_new_tree_has_none() {
        let t = tempfile::tempdir().unwrap();
        let (old, new) = (t.path().join("old"), t.path().join("new"));
        touch(&old.join("envs/b/from"), "old");
        std::fs::create_dir_all(&new).unwrap();
        carry_envs(&old, &new).unwrap();
        assert!(new.join("envs/b/from").is_file());
        assert!(!old.join("envs").exists());
    }
}
