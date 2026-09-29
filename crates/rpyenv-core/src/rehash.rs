//! `pyenv rehash`: create the missing shims, remove stale ones, and store what the versions
//! looked like, so a shim can cheaply tell when another rehash is needed (spec §8).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::{installed, shimset};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Only one rehash runs at a time.
pub const LOCK_NAME: &str = ".rehash.lock";
/// What the versions looked like at the last rehash.
pub const STATE_NAME: &str = ".rehash-state";
/// Windows: the per-user copy of the shim binary that every shim is a hardlink to.
pub const TEMPLATE_DIR: &str = ".template";
pub const TEMPLATE_EXE: &str = "pyenv-shim.exe";
/// Upstream deletes a lock older than two minutes (libexec/pyenv-rehash:15-43).
const STALE_LOCK: Duration = Duration::from_secs(120);
/// The exit check waits this long for another rehash to finish, then skips: a compromise
/// between spec §5.2, which needs the rehash done before the caller continues, and spec §8's
/// "a shim that can't get the lock skips".
const CHECK_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// `pyenv rehash`: retry every 0.1 s for up to this long, as upstream does.
    Upto(Duration),
    /// A shim's exit check: give up at once; the next check catches up.
    No,
}

#[derive(Debug)]
pub enum RehashError {
    NotWritable(PathBuf),
    /// `Wait::Upto` ran out. Holds the lock's path.
    Timeout(PathBuf),
    /// `Wait::No` found the lock held.
    Busy,
    Io(io::Error),
}

/// What one rehash changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RehashStats {
    /// Shims created or re-created.
    pub linked: usize,
    /// Stale shims removed or renamed aside.
    pub removed: usize,
}

/// Held while a rehash runs. Dropping it removes the lock file, on failure too.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Takes `shims/.rehash.lock`, creating `shims` if needed.
pub fn lock(shims: &Path, wait: Wait) -> Result<Lock, RehashError> {
    let not_writable = || RehashError::NotWritable(shims.to_path_buf());
    fs::create_dir_all(shims).map_err(|_| not_writable())?;
    let path = shims.join(LOCK_NAME);
    let start = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(Lock { path }),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if is_stale(&path) {
                    let _ = fs::remove_file(&path);
                    continue;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(not_writable()),
            Err(e) => return Err(RehashError::Io(e)),
        }
        match wait {
            Wait::No => return Err(RehashError::Busy),
            Wait::Upto(limit) if start.elapsed() >= limit => {
                return Err(RehashError::Timeout(path))
            }
            Wait::Upto(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn is_stale(lock: &Path) -> bool {
    fs::metadata(lock)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or_default() > STALE_LOCK)
        .unwrap_or(false)
}

/// The watched folders' modification times and entry counts, one line each.
pub fn snapshot(ctx: &Ctx) -> String {
    let vdir = ctx.versions_dir();
    let mut dirs = vec![vdir.clone()];
    for entry in installed::top_level(&vdir, ctx.flavor) {
        match ctx.flavor {
            Flavor::Pyenv => {
                dirs.push(entry.path.join("bin"));
                dirs.push(entry.path.join("envs"));
                dirs.extend(
                    installed::envs_of(&entry)
                        .into_iter()
                        .map(|e| e.path.join("bin")),
                );
            }
            Flavor::PyenvWin => {
                dirs.push(entry.path.join("Scripts"));
                dirs.push(entry.path.join("bin"));
                dirs.push(entry.path);
            }
        }
    }
    let mut s = String::from("rpyenv rehash state 2\n");
    for d in dirs {
        let Ok(meta) = fs::metadata(&d) else {
            continue;
        };
        let t = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .unwrap_or_default();
        let count = fs::read_dir(&d).map(|r| r.count()).unwrap_or(0);
        s.push_str(&format!(
            "{}.{:09} {count} {}\n",
            t.as_secs(),
            t.subsec_nanos(),
            state_path(&d, &vdir)
        ));
    }
    s
}

/// `d` relative to `versions`, with `/` separators (`.` for `versions` itself), so two
/// spellings of one root give the same state (M1b review M-3).
fn state_path(d: &Path, vdir: &Path) -> String {
    match d.strip_prefix(vdir) {
        Ok(r) if r.as_os_str().is_empty() => ".".to_string(),
        Ok(r) => r.to_string_lossy().replace('\\', "/"),
        Err(_) => d.display().to_string(),
    }
}

/// True when the stored state is missing or differs from the disk.
pub fn needed(ctx: &Ctx) -> bool {
    fs::read_to_string(ctx.shims_dir().join(STATE_NAME))
        .map(|stored| stored != snapshot(ctx))
        .unwrap_or(true)
}

/// A shim's exit check (spec §8). When the state changed, it waits up to `CHECK_WAIT` for
/// the lock, then checks again (the rehash it waited for may have done the work) before
/// rehashing. Failures are ignored; the next check catches up. True when it rehashed.
pub fn check(ctx: &Ctx, shim_exe: &Path) -> bool {
    if !needed(ctx) {
        return false;
    }
    let Ok(_lock) = lock(&ctx.shims_dir(), Wait::Upto(CHECK_WAIT)) else {
        return false;
    };
    needed(ctx) && rehash_locked(ctx, shim_exe).is_ok()
}

/// Brings `shims` in line with the installed versions.
pub fn rehash(ctx: &Ctx, shim_exe: &Path, wait: Wait) -> Result<RehashStats, RehashError> {
    let _lock = lock(&ctx.shims_dir(), wait)?;
    rehash_locked(ctx, shim_exe)
}

/// The rehash itself, for a caller that holds the lock.
pub fn rehash_locked(ctx: &Ctx, shim_exe: &Path) -> Result<RehashStats, RehashError> {
    let shims = ctx.shims_dir();
    // Taken before scanning: a change during the scan makes the next check rehash again.
    let state = snapshot(ctx);
    let wanted = shimset::wanted(ctx);
    let stats = match ctx.flavor {
        Flavor::Pyenv => apply_links(&shims, shim_exe, &wanted),
        Flavor::PyenvWin => apply_hardlinks(&shims, shim_exe, &wanted),
    }
    .map_err(RehashError::Io)?;
    write_state(&shims, &state).map_err(RehashError::Io)?;
    Ok(stats)
}

fn write_state(shims: &Path, state: &str) -> io::Result<()> {
    let tmp = shims.join(".rehash-state.tmp");
    fs::write(&tmp, state)?;
    fs::rename(&tmp, shims.join(STATE_NAME))
}

/// Linux: each shim is a symlink to the shim binary. An existing file that isn't that
/// link, such as upstream's bash shim, is replaced; directories are left alone.
fn apply_links(shims: &Path, target: &Path, wanted: &[OsString]) -> io::Result<RehashStats> {
    let mut linked = 0;
    for name in wanted {
        let p = shims.join(name);
        match fs::symlink_metadata(&p) {
            Ok(m)
                if m.file_type().is_symlink()
                    && fs::read_link(&p).ok().as_deref() == Some(target) =>
            {
                continue
            }
            Ok(m) if m.is_dir() => continue,
            Ok(_) => fs::remove_file(&p)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        symlink(target, &p)?;
        linked += 1;
    }
    Ok(RehashStats {
        linked,
        removed: remove_stale(shims, wanted, false)?,
    })
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// The Linux flavor off Unix only runs in tests.
#[cfg(not(unix))]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    fs::copy(target, link).map(|_| ())
}

/// Windows: each shim is a hardlink to `shims\.template\pyenv-shim.exe`, or a copy when a
/// hardlink isn't possible. A failed shim doesn't stop the others; the first error is
/// returned, so the state isn't stored and the next check tries again.
fn apply_hardlinks(shims: &Path, source: &Path, wanted: &[OsString]) -> io::Result<RehashStats> {
    let template = refresh_template(shims, source)?;
    let tmeta = fs::metadata(&template)?;
    let mut first_err = None;
    let mut linked = 0;
    for name in wanted {
        let p = shims.join(name);
        match fs::metadata(&p) {
            Ok(m) if same_file_data(&m, &tmeta) => continue,
            Ok(m) if m.is_dir() => continue,
            Ok(_) => remove_or_rename(&p),
            Err(_) => {}
        }
        match fs::hard_link(&template, &p).or_else(|_| fs::copy(&template, &p).map(|_| ())) {
            Ok(()) => linked += 1,
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    let removed = remove_stale(shims, wanted, true)?;
    first_err.map_or(Ok(RehashStats { linked, removed }), Err)
}

/// A hardlink shares its size and modification time with the template. A refreshed
/// template, copied from a different shim binary, differs in at least one of them.
fn same_file_data(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.is_file() && a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

/// `shims\.template\pyenv-shim.exe`, copied again when `source` has different bytes
/// (spec §8). Old copies renamed aside by an earlier refresh are deleted first.
fn refresh_template(shims: &Path, source: &Path) -> io::Result<PathBuf> {
    let dir = shims.join(TEMPLATE_DIR);
    fs::create_dir_all(&dir)?;
    for e in fs::read_dir(&dir)?.filter_map(Result::ok) {
        if e.file_name().to_string_lossy().ends_with(".old") {
            let _ = fs::remove_file(e.path());
        }
    }
    let template = dir.join(TEMPLATE_EXE);
    if source == template {
        return Ok(template);
    }
    let fresh = match (fs::read(source), fs::read(&template)) {
        (Ok(a), Ok(b)) => a == b,
        (Ok(_), Err(_)) => false,
        (Err(e), _) if !template.is_file() => return Err(e),
        (Err(_), _) => true,
    };
    if !fresh {
        // Copied to a temp file first and renamed into place, so a crash mid-copy leaves
        // the old template rather than a truncated one.
        let tmp = dir.join(format!("{TEMPLATE_EXE}.tmp"));
        if let Err(e) = fs::copy(source, &tmp) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        remove_or_rename(&template);
        fs::rename(&tmp, &template)?;
    }
    Ok(template)
}

/// Removes shim files that are no longer wanted. Names starting with `.` are rpyenv's
/// own files and are kept, except `.old` leftovers, which are deleted. Directories are
/// left alone (allowlist D-35). `fold_case` compares names without case (Windows).
fn remove_stale(shims: &Path, wanted: &[OsString], fold_case: bool) -> io::Result<usize> {
    let key = |n: &OsString| -> OsString {
        if fold_case {
            OsString::from(n.to_string_lossy().to_lowercase())
        } else {
            n.clone()
        }
    };
    let keep: HashSet<OsString> = wanted.iter().map(key).collect();
    let mut removed = 0;
    for e in fs::read_dir(shims)?.filter_map(Result::ok) {
        let name = e.file_name();
        let text = name.to_string_lossy();
        if text.starts_with('.') {
            if text.ends_with(".old") {
                let _ = fs::remove_file(e.path());
            }
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir && !keep.contains(&key(&name)) {
            remove_or_rename(&e.path());
            removed += 1;
        }
    }
    Ok(removed)
}

/// Deletes a shim. A running `.exe` can't be deleted on Windows, so it is renamed to
/// `.<name>.old` instead (hidden, and never a command name); a later rehash deletes it.
fn remove_or_rename(p: &Path) {
    remove_or_rename_with(p, |p| fs::remove_file(p))
}

/// `remove_or_rename`, taking the removal function so a host that allows deleting a
/// running file can still exercise the rename-aside branch in a test.
fn remove_or_rename_with(p: &Path, remove: impl Fn(&Path) -> io::Result<()>) {
    if remove(p).is_ok() || !p.exists() {
        return;
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 0..100u32 {
        let aside = if n == 0 {
            format!(".{name}.old")
        } else {
            format!(".{name}.{n}.old")
        };
        let aside = p.with_file_name(aside);
        if !aside.exists() {
            let _ = fs::rename(p, aside);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// A root, its context, and a fake shim binary.
    fn setup(flavor: Flavor) -> (tempfile::TempDir, Ctx, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        fs::create_dir_all(root.join("versions")).unwrap();
        let ctx = Ctx::for_test(flavor, &root, tmp.path());
        let shim = tmp.path().join("pyenv-shim");
        fs::write(&shim, b"shim binary").unwrap();
        (tmp, ctx, shim)
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let (_t, ctx, _) = setup(Flavor::current());
        let shims = ctx.shims_dir();
        let held = lock(&shims, Wait::No).unwrap();
        assert!(matches!(lock(&shims, Wait::No), Err(RehashError::Busy)));
        let waited = lock(&shims, Wait::Upto(Duration::from_millis(200)));
        assert!(matches!(waited, Err(RehashError::Timeout(p)) if p == shims.join(LOCK_NAME)));
        drop(held);
        assert!(!shims.join(LOCK_NAME).exists());
        assert!(lock(&shims, Wait::No).is_ok());
    }

    #[test]
    fn a_lock_older_than_two_minutes_is_broken() {
        let (_t, ctx, _) = setup(Flavor::current());
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        let f = fs::File::create(shims.join(LOCK_NAME)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(180))
            .unwrap();
        drop(f);
        assert!(lock(&shims, Wait::No).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn linux_rehash_links_replaces_and_removes() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/bin/python"));
        exe(&v.join("3.12.1/bin/pip"));
        let shims = ctx.shims_dir();
        fs::create_dir_all(shims.join("mdir")).unwrap();
        // An upstream bash shim is replaced by a link.
        fs::write(shims.join("python"), "#!/usr/bin/env bash\n").unwrap();
        fs::write(shims.join("gone"), "").unwrap();
        fs::write(shims.join(".keep"), "").unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        for n in ["python", "pip"] {
            assert_eq!(fs::read_link(shims.join(n)).unwrap(), shim);
        }
        assert!(!shims.join("gone").exists());
        assert!(shims.join(".keep").exists());
        assert!(shims.join("mdir").is_dir());
        assert!(!shims.join(LOCK_NAME).exists());
        assert!(!needed(&ctx));
    }

    #[cfg(unix)]
    #[test]
    fn the_check_notices_a_new_script() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let v = ctx.versions_dir();
        exe(&v.join("3.12.1/bin/python"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert!(!needed(&ctx));
        exe(&v.join("3.12.1/bin/black"));
        assert!(needed(&ctx));
        check(&ctx, &shim);
        assert_eq!(fs::read_link(ctx.shims_dir().join("black")).unwrap(), shim);
        assert!(!needed(&ctx));
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_shims_folder() {
        use std::os::unix::fs::PermissionsExt;
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        fs::set_permissions(&shims, fs::Permissions::from_mode(0o555)).unwrap();
        let root_user = fs::write(shims.join("probe"), "").is_ok();
        if !root_user {
            let r = rehash(&ctx, &shim, Wait::No);
            assert!(matches!(r, Err(RehashError::NotWritable(d)) if d == shims));
        }
        fs::set_permissions(&shims, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_check_waits_for_a_held_lock() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1").join("python.exe"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        let shims = ctx.shims_dir();
        let held = lock(&shims, Wait::No).unwrap();
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        exe(&v.join("3.9.1").join("Scripts").join("black.exe"));
        check(&ctx, &shim);
        holder.join().unwrap();
        assert!(shims.join("black.exe").exists());
    }

    #[test]
    fn rename_aside_when_removal_fails() {
        let (_t, ctx, _) = setup(Flavor::current());
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        let fail = |_: &Path| -> io::Result<()> { Err(io::Error::other("in use")) };
        let tool = shims.join("tool.exe");
        fs::write(&tool, "").unwrap();
        remove_or_rename_with(&tool, fail);
        assert!(shims.join(".tool.exe.old").exists());
        assert!(!tool.exists());
        fs::write(&tool, "").unwrap();
        remove_or_rename_with(&tool, fail);
        assert!(shims.join(".tool.exe.1.old").exists());
    }

    #[test]
    fn windows_rehash_hardlinks_and_removes_pyenv_win_shims() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/pip.exe"));
        let shims = ctx.shims_dir();
        fs::create_dir_all(&shims).unwrap();
        for old in ["python.bat", "python", "aws.lnk"] {
            fs::write(shims.join(old), "old").unwrap();
        }
        rehash(&ctx, &shim, Wait::No).unwrap();
        let template = shims.join(TEMPLATE_DIR).join(TEMPLATE_EXE);
        assert_eq!(fs::read(&template).unwrap(), b"shim binary");
        for n in ["python.exe", "pip.exe"] {
            assert_eq!(fs::read(shims.join(n)).unwrap(), b"shim binary");
        }
        for old in ["python.bat", "python", "aws.lnk"] {
            assert!(!shims.join(old).exists(), "{old}");
        }
        // A new shim binary refreshes the template and every link.
        fs::write(&shim, b"new shim binary").unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(fs::read(shims.join("pip.exe")).unwrap(), b"new shim binary");
    }

    #[test]
    fn state_does_not_depend_on_how_the_root_is_spelled() {
        let (_t, ctx, _) = setup(Flavor::PyenvWin);
        exe(&ctx.versions_dir().join("3.9.1/python.exe"));
        let mut other = ctx.clone();
        other.root = ctx.root.join(".");
        assert_ne!(
            ctx.versions_dir().display().to_string(),
            other.versions_dir().display().to_string()
        );
        assert_eq!(snapshot(&ctx), snapshot(&other));
    }

    #[test]
    fn a_second_rehash_changes_nothing() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/pip.exe"));
        fs::create_dir_all(ctx.shims_dir()).unwrap();
        fs::write(ctx.shims_dir().join("python.bat"), "old").unwrap();
        let first = rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(
            first,
            RehashStats {
                linked: 2,
                removed: 1
            }
        );
        assert_eq!(
            rehash(&ctx, &shim, Wait::No).unwrap(),
            RehashStats::default()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_second_linux_rehash_changes_nothing() {
        let (_t, ctx, shim) = setup(Flavor::Pyenv);
        exe(&ctx.versions_dir().join("3.12.1/bin/python"));
        assert_eq!(rehash(&ctx, &shim, Wait::No).unwrap().linked, 1);
        assert_eq!(
            rehash(&ctx, &shim, Wait::No).unwrap(),
            RehashStats::default()
        );
    }

    #[test]
    fn the_check_skips_when_another_rehash_already_did_it() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        exe(&ctx.versions_dir().join("3.9.1/python.exe"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        exe(&ctx.versions_dir().join("3.9.1/Scripts/black.exe"));
        let held = lock(&ctx.shims_dir(), Wait::No).unwrap();
        let (other_ctx, other_shim) = (ctx.clone(), shim.clone());
        let other = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let stats = rehash_locked(&other_ctx, &other_shim).unwrap();
            drop(held);
            stats
        });
        assert!(!check(&ctx, &shim), "the check rehashed again");
        assert_eq!(other.join().unwrap().linked, 1);
        assert!(ctx.shims_dir().join("black.exe").exists());
    }
}
