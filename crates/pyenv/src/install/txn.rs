//! The install transaction (spec §9.3, plan Decision 4): a per-name lock, a staging directory
//! inside `versions/`, a swap that keeps the previous version until commit, and rollback on
//! drop.

use std::io::Write;
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
    lock: PathBuf,
    stage: PathBuf,
    old: Option<PathBuf>,
    placed: bool,
    done: bool,
}

/// Whether the process that wrote `lock` may still run. Only Linux can tell (`/proc`);
/// elsewhere a lock is never taken over (M2b handles Windows).
fn holder_alive(lock: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(lock) else {
        return true;
    };
    let Ok(pid) = text.trim().parse::<u32>() else {
        return true;
    };
    if cfg!(target_os = "linux") {
        Path::new("/proc").join(pid.to_string()).exists()
    } else {
        true
    }
}

impl Txn {
    pub fn begin(versions: &Path, name: &str) -> Result<Txn, String> {
        std::fs::create_dir_all(versions)
            .map_err(|e| format!("pyenv: cannot create {}: {e}", versions.display()))?;
        let lock = versions.join(format!(".lock-{name}"));
        let busy = || {
            format!(
                "pyenv: another install of {name} is in progress ({})",
                lock.display()
            )
        };
        let open = || {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
        };
        let mut f = match open() {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if holder_alive(&lock) {
                    return Err(busy());
                }
                let _ = std::fs::remove_file(&lock);
                open().map_err(|_| busy())?
            }
            Err(e) => return Err(format!("pyenv: cannot create {}: {e}", lock.display())),
        };
        let _ = writeln!(f, "{}", std::process::id());
        let stage = versions.join(format!(".tmp-{name}"));
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage)
            .map_err(|e| format!("pyenv: cannot create {}: {e}", stage.display()))?;
        Ok(Txn {
            versions: versions.to_path_buf(),
            name: name.to_string(),
            lock,
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
        if target.symlink_metadata().is_ok() {
            let old = self.versions.join(format!(".old-{}", self.name));
            let _ = std::fs::remove_dir_all(&old);
            std::fs::rename(&target, &old)?;
            self.old = Some(old);
        }
        std::fs::rename(staged_prefix, &target)?;
        std::fs::write(target.join(MARKER), "")?;
        self.placed = true;
        Ok(())
    }

    pub fn commit(mut self) -> std::io::Result<()> {
        std::fs::remove_file(self.target().join(MARKER))?;
        if let Some(old) = self.old.take() {
            let _ = std::fs::remove_dir_all(&old);
        }
        let _ = std::fs::remove_dir_all(&self.stage);
        let _ = std::fs::remove_file(&self.lock);
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
        let _ = std::fs::remove_file(&self.lock);
    }
}
