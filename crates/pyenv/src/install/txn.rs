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
                } else if carry_over(&old, &target).is_ok() {
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
                match carry_over(&old, &self.target()) {
                    Ok(()) => {
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

/// Moves what upstream's build over the old tree would have kept from `old` into the new
/// tree `new`: `envs/` (pyenv-virtualenv) when `new` has none, and each entry that `new`
/// lacks in `bin/` and `Scripts\` (scripts pip installed) and in site-packages (`lib/python*/site-packages`,
/// and `Lib/site-packages` for Windows). Entries in both keep `new`'s copy. Everything moves
/// by rename, which moves a symlink as a link, so a failure part way loses nothing: what has
/// not moved is still in `old`.
fn carry_over(old: &Path, new: &Path) -> std::io::Result<()> {
    let envs = old.join("envs");
    if envs.symlink_metadata().is_ok() && new.join("envs").symlink_metadata().is_err() {
        std::fs::rename(&envs, new.join("envs"))?;
    }
    // `bin/` (pyenv) and `Scripts\` (pyenv-win): console scripts pip installed.
    let mut dirs: Vec<PathBuf> = vec![PathBuf::from("bin"), PathBuf::from("Scripts")];
    if let Ok(rd) = std::fs::read_dir(old.join("lib")) {
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name();
            if name.to_string_lossy().starts_with("python") {
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
