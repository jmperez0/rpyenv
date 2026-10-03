//! The install transaction (spec §9.3, plan Decision 4): a per-name OS lock, a staging directory
//! inside `versions/`, a swap that keeps the previous version until commit, and rollback on
//! drop.

use std::path::{Path, PathBuf};

/// Present in `versions/<name>` from the move until commit: a version that still has it
/// was interrupted and isn't complete.
pub const MARKER: &str = ".rpyenv-incomplete";

/// "Installed" for `pyenv install`: upstream's `bin/` test, minus interrupted installs.
pub fn is_complete(dir: &Path) -> bool {
    dir.join("bin").is_dir() && !dir.join(MARKER).exists()
}

pub struct Txn {
    versions: PathBuf,
    name: String,
    /// Held for the transaction's life; the OS releases it on drop or crash. The file is
    /// never deleted: deleting a lock file reintroduces the takeover race.
    _lock: std::fs::File,
    stage: PathBuf,
    old: Option<PathBuf>,
    placed: bool,
    done: bool,
}

impl Txn {
    pub fn begin(versions: &Path, name: &str) -> Result<Txn, String> {
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
            } else if is_complete(&target) {
                let _ = std::fs::remove_dir_all(&old);
            } else {
                // Killed after the final rename: the target is the new, unfinished tree
                // and `.old` is the only good copy.
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

    pub fn commit(mut self) -> std::io::Result<()> {
        std::fs::remove_file(self.target().join(MARKER))?;
        if let Some(old) = self.old.take() {
            let _ = std::fs::remove_dir_all(&old);
        }
        let _ = std::fs::remove_dir_all(&self.stage);
        self.done = true;
        Ok(())
    }
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
