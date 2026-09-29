//! `pyenv rehash`: create the missing shims, remove stale ones, and store what the versions
//! looked like, so a shim can cheaply tell when another rehash is needed (spec §8).

use crate::ctx::Ctx;
use crate::flavor::Flavor;
use crate::shimset::{ShimKind, Wanted};
use crate::{installed, shimset};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
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
/// Windows: the GUI shim's template name, and its installed name next to `pyenv-shim.exe`.
pub const TEMPLATE_GUI_EXE: &str = "pyenv-shimw.exe";
/// Where `pyenv rehash` records the `pyenv.exe` that forwarders call (plan decision 6).
pub const PYENV_PATH_FILE: &str = "pyenv-path.txt";
/// Where `pyenv rehash` records its `RPYENV_BATCH_FORWARD` list, for the exit check.
pub const BATCH_FORWARD_FILE: &str = "batch-forward.txt";
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
    let wanted = shimset::wanted(ctx, &batch_forward(ctx, shim_exe));
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
fn apply_links(shims: &Path, target: &Path, wanted: &[Wanted]) -> io::Result<RehashStats> {
    let mut linked = 0;
    for w in wanted {
        let p = shims.join(&w.name);
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

/// Windows: each shim is a hardlink to `shims\.template\pyenv-shim.exe` or
/// `shims\.template\pyenv-shimw.exe`, matching its kind, or a copy when a hardlink isn't
/// possible. A `.cmd` forwarder is a text file instead (spec §5.3), and falls back to the
/// console shim when `pyenv.exe` isn't known, or (`shimset::pyenv_reference`) when its path
/// can't be written into the forwarder safely. A failed shim doesn't stop the others; the
/// first error is returned, so the state isn't stored and the next check tries again.
fn apply_hardlinks(shims: &Path, source: &Path, wanted: &[Wanted]) -> io::Result<RehashStats> {
    let console = refresh_template(shims, source, TEMPLATE_EXE)?;
    // The GUI shim is installed next to the console one, and its template next to the
    // console template. Without it, GUI programs get the console shim.
    let gui_source = source.with_file_name(TEMPLATE_GUI_EXE);
    let gui = if gui_source.is_file() {
        refresh_template(shims, &gui_source, TEMPLATE_GUI_EXE)?
    } else {
        console.clone()
    };
    let console_meta = fs::metadata(&console)?;
    let mut gui_meta = fs::metadata(&gui)?;
    // The two shim binaries can have one size, and a zip or an installer can give them one
    // modification time; `fs::copy` keeps it. Then a shim whose kind changed would look
    // current. Moving the GUI template's time apart tells them apart again; its hardlinked
    // shims share the file, so they follow it.
    if gui != console && same_file_data(&console_meta, &gui_meta) {
        let apart = console_meta.modified()? + Duration::from_secs(2);
        set_mtime(&gui, apart)?;
        gui_meta = fs::metadata(&gui)?;
    }
    let pyenv = pyenv_exe(source, &shims.join(TEMPLATE_DIR));
    let pyenv_ref = pyenv
        .as_deref()
        .and_then(|p| shimset::pyenv_reference(p, shims));
    // A forwarder that can't be written safely becomes a console exe shim instead — in the
    // name it's written under as much as in what it runs — so the fallback and
    // `remove_stale` agree; otherwise a fallback shim would be deleted as stale right
    // after being created, and a stale forwarder from a previous rehash that could write
    // one is left behind uncleaned. Two independent reasons: no known `pyenv.exe`, or
    // `pyenv_reference` finding nothing safe to write, doesn't vary by name — one decision
    // for every forwarder; the command's own name having a character outside
    // `[A-Za-z0-9._+-]` (`café`, `set var`, `a&b`) does vary by name, since the name is
    // written into the forwarder's command lines (`shimset::forwardable_name`).
    let effective: Vec<Wanted> = wanted
        .iter()
        .cloned()
        .map(|w| {
            let keep_forward =
                pyenv_ref.is_some() && shimset::forwardable_name(&w.name.to_string_lossy());
            if w.kind == ShimKind::Forward && !keep_forward {
                console_fallback(w)
            } else {
                w
            }
        })
        .collect();
    let mut first_err = None;
    let mut linked = 0;
    for w in &effective {
        if let (ShimKind::Forward, Some(pyenv_ref)) = (w.kind, &pyenv_ref) {
            let p = shims.join(&w.name);
            let file = w.name.to_string_lossy();
            let text = shimset::forwarder(pyenv_ref, file.strip_suffix(".cmd").unwrap_or(&file));
            if fs::read_to_string(&p).ok().as_deref() != Some(text.as_str()) {
                remove_or_rename(&p);
                match fs::write(&p, &text) {
                    Ok(()) => linked += 1,
                    Err(e) => {
                        first_err.get_or_insert(e);
                    }
                }
            }
            continue;
        }
        let (template, tmeta) = match w.kind {
            ShimKind::Gui => (&gui, &gui_meta),
            ShimKind::Console | ShimKind::Forward => (&console, &console_meta),
        };
        let p = shims.join(&w.name);
        match fs::metadata(&p) {
            Ok(m) if same_file_data(&m, tmeta) => continue,
            Ok(m) if m.is_dir() => continue,
            Ok(_) => remove_or_rename(&p),
            Err(_) => {}
        }
        match fs::hard_link(template, &p).or_else(|_| fs::copy(template, &p).map(|_| ())) {
            Ok(()) => linked += 1,
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    let removed = remove_stale(shims, &effective, true)?;
    first_err.map_or(Ok(RehashStats { linked, removed }), Err)
}

/// A `Forward` entry (`<stem>.cmd`) that can't get a forwarder becomes a `Console` entry
/// (`<stem>.exe`) instead; every other entry is unchanged.
fn console_fallback(w: Wanted) -> Wanted {
    if w.kind != ShimKind::Forward {
        return w;
    }
    let file = w.name.to_string_lossy();
    let stem = file.strip_suffix(".cmd").unwrap_or(&file);
    Wanted {
        name: OsString::from(format!("{stem}.exe")),
        kind: ShimKind::Console,
    }
}

/// True for a shim's exit check, which rehashes from the template itself; `pyenv rehash`
/// passes the shim binary installed next to `pyenv.exe`.
fn is_exit_check(source: &Path, template_dir: &Path) -> bool {
    source.parent() == Some(template_dir)
}

/// The names that get a `.cmd` forwarder (Windows). `pyenv rehash` takes them from
/// `RPYENV_BATCH_FORWARD` and records them, an empty list when the variable is unset or
/// empty. A shim's exit check usually runs without the variable (`pip install` through a
/// shim), so it uses the variable when set and the record otherwise; it never rewrites
/// the record.
fn batch_forward(ctx: &Ctx, source: &Path) -> Vec<String> {
    if ctx.flavor != Flavor::PyenvWin {
        return Vec::new();
    }
    let dir = ctx.shims_dir().join(TEMPLATE_DIR);
    let record = dir.join(BATCH_FORWARD_FILE);
    if is_exit_check(source, &dir) {
        return match &ctx.batch_forward {
            Some(v) => shimset::forward_names(Some(v)),
            None => fs::read_to_string(&record)
                .map(|s| shimset::forward_names(Some(OsStr::new(&s))))
                .unwrap_or_default(),
        };
    }
    let names = shimset::forward_names(ctx.batch_forward.as_deref());
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(&record, names.join(";"));
    names
}

/// The `pyenv.exe` forwarders call. `pyenv rehash` passes the shim binary installed next
/// to `pyenv.exe`, and the path is recorded; a shim's exit check passes the template and
/// reads the record back.
fn pyenv_exe(source: &Path, template_dir: &Path) -> Option<PathBuf> {
    let record = template_dir.join(PYENV_PATH_FILE);
    if is_exit_check(source, template_dir) {
        return fs::read_to_string(&record)
            .ok()
            .map(|s| PathBuf::from(s.trim()));
    }
    let pyenv = source.with_file_name("pyenv.exe");
    if !pyenv.is_file() {
        return None;
    }
    let _ = fs::write(&record, pyenv.display().to_string());
    Some(pyenv)
}

/// A hardlink shares its size and modification time with the template. A refreshed
/// template, copied from a different shim binary, differs in at least one of them.
fn same_file_data(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.is_file() && a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

/// Sets `p`'s modification time. On Windows the file is opened for its attributes only,
/// which works while a shim linked to it is running.
fn set_mtime(p: &Path, t: SystemTime) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES);
    }
    #[cfg(not(windows))]
    options.read(true);
    options.open(p)?.set_modified(t)
}

/// `shims\.template\<name>`, copied again when `source` has different bytes (spec §8).
/// Old copies renamed aside by an earlier refresh, and leftover `.tmp` files, are deleted
/// first.
fn refresh_template(shims: &Path, source: &Path, name: &str) -> io::Result<PathBuf> {
    let dir = shims.join(TEMPLATE_DIR);
    fs::create_dir_all(&dir)?;
    for e in fs::read_dir(&dir)?.filter_map(Result::ok) {
        let n = e.file_name();
        let text = n.to_string_lossy();
        if text.ends_with(".old") || text.ends_with(".tmp") {
            let _ = fs::remove_file(e.path());
        }
    }
    let template = dir.join(name);
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
        let tmp = dir.join(format!("{name}.tmp"));
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
fn remove_stale(shims: &Path, wanted: &[Wanted], fold_case: bool) -> io::Result<usize> {
    let key = |n: &OsString| -> OsString {
        if fold_case {
            OsString::from(n.to_string_lossy().to_lowercase())
        } else {
            n.clone()
        }
    };
    let keep: HashSet<OsString> = wanted.iter().map(|w| key(&w.name)).collect();
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

    #[test]
    fn gui_programs_link_to_the_gui_shim() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        fs::write(tmp.path().join(TEMPLATE_GUI_EXE), b"gui shim").unwrap();
        let v = ctx.versions_dir().join("3.9.1");
        fs::create_dir_all(&v).unwrap();
        fs::write(v.join("python.exe"), crate::pe::image(3)).unwrap();
        fs::write(v.join("pythonw.exe"), crate::pe::image(2)).unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        let shims = ctx.shims_dir();
        assert_eq!(fs::read(shims.join("python.exe")).unwrap(), b"shim binary");
        assert_eq!(fs::read(shims.join("pythonw.exe")).unwrap(), b"gui shim");
        assert_eq!(
            rehash(&ctx, &shim, Wait::No).unwrap(),
            RehashStats::default()
        );
    }

    /// An installer or a zip can give both shim binaries one size and one modification
    /// time. A shim whose kind changed must still be relinked, both ways.
    #[test]
    fn a_kind_switch_relinks_when_both_templates_share_size_and_mtime() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        // The same length as the console shim's "shim binary".
        fs::write(tmp.path().join(TEMPLATE_GUI_EXE), b"gui  binary").unwrap();
        let v = ctx.versions_dir();
        let write = |rel: &str, sub: u16| {
            let p = v.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, crate::pe::image(sub)).unwrap();
        };
        write("3.9.1/tool.exe", 2);
        let shims = ctx.shims_dir();
        let dir = shims.join(TEMPLATE_DIR);
        let same_mtime = || {
            let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
            for name in [TEMPLATE_EXE, TEMPLATE_GUI_EXE] {
                set_mtime(&dir.join(name), t).unwrap();
            }
            let meta = |n: &str| fs::metadata(dir.join(n)).unwrap();
            assert!(same_file_data(&meta(TEMPLATE_EXE), &meta(TEMPLATE_GUI_EXE)));
        };
        let tool = || fs::read(shims.join("tool.exe")).unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(tool(), b"gui  binary");
        // GUI -> console: another version's tool.exe is a console program.
        same_mtime();
        write("3.8.2/tool.exe", 3);
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(tool(), b"shim binary");
        // Console -> GUI: that version is gone again.
        same_mtime();
        fs::remove_dir_all(v.join("3.8.2")).unwrap();
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(tool(), b"gui  binary");
    }

    #[test]
    fn forwarders_use_the_recorded_pyenv_path() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        let pyenv = tmp.path().join("pyenv.exe");
        fs::write(&pyenv, b"cli").unwrap();
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/setvar.bat"));
        let wanted = shimset::shims_win(&v, &["setvar".to_string()]);
        let shims = ctx.shims_dir();
        let stats = apply_hardlinks(&shims, &shim, &wanted).unwrap();
        assert_eq!(stats.linked, 2);
        let pyenv_ref = shimset::pyenv_reference(&pyenv, &shims).unwrap();
        assert_eq!(
            fs::read_to_string(shims.join("setvar.cmd")).unwrap(),
            shimset::forwarder(&pyenv_ref, "setvar")
        );
        assert!(!shims.join("setvar.exe").exists());
        // A shim's exit check passes the template; the recorded path keeps the forwarder.
        let template = shims.join(TEMPLATE_DIR).join(TEMPLATE_EXE);
        assert_eq!(
            apply_hardlinks(&shims, &template, &wanted).unwrap(),
            RehashStats::default()
        );
    }

    /// A shim's exit check usually runs without `RPYENV_BATCH_FORWARD` (`pip install`
    /// through a shim); it uses the list the last `pyenv rehash` recorded, so forwarders
    /// stay forwarders. The variable wins when it is set; the CLI rewrites the record.
    #[test]
    fn the_exit_check_keeps_forwarders_without_the_variable() {
        let (tmp, mut ctx, shim) = setup(Flavor::PyenvWin);
        fs::write(tmp.path().join("pyenv.exe"), b"cli").unwrap();
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/setvar.bat"));
        let shims = ctx.shims_dir();
        let template = shims.join(TEMPLATE_DIR).join(TEMPLATE_EXE);
        ctx.batch_forward = Some(OsString::from("setvar"));
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert!(shims.join("setvar.cmd").is_file());
        // The exit check, without the variable: the record keeps the forwarder.
        ctx.batch_forward = None;
        exe(&v.join("3.9.1/Scripts/newtool.exe"));
        assert!(check(&ctx, &template));
        assert!(shims.join("newtool.exe").is_file());
        assert!(shims.join("setvar.cmd").is_file());
        assert!(!shims.join("setvar.exe").exists());
        // The exit check with the variable set (empty): it wins over the record.
        ctx.batch_forward = Some(OsString::new());
        exe(&v.join("3.9.1/Scripts/other.exe"));
        assert!(check(&ctx, &template));
        assert!(shims.join("setvar.exe").is_file());
        assert!(!shims.join("setvar.cmd").exists());
        // ...and it didn't rewrite the record.
        ctx.batch_forward = None;
        exe(&v.join("3.9.1/Scripts/third.exe"));
        assert!(check(&ctx, &template));
        assert!(shims.join("setvar.cmd").is_file());
        // `pyenv rehash` without the variable records an empty list.
        rehash(&ctx, &shim, Wait::No).unwrap();
        assert_eq!(
            fs::read_to_string(shims.join(TEMPLATE_DIR).join(BATCH_FORWARD_FILE)).unwrap(),
            ""
        );
        assert!(shims.join("setvar.exe").is_file());
        exe(&v.join("3.9.1/Scripts/fourth.exe"));
        assert!(check(&ctx, &template));
        assert!(shims.join("setvar.exe").is_file());
        assert!(!shims.join("setvar.cmd").exists());
    }

    #[test]
    fn without_a_known_pyenv_a_forwarder_falls_back_to_a_working_console_shim() {
        let (_t, ctx, shim) = setup(Flavor::PyenvWin);
        // No `pyenv.exe` next to `shim`, so `pyenv_exe` finds nothing.
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/setvar.bat"));
        let wanted = shimset::shims_win(&v, &["setvar".to_string()]);
        let shims = ctx.shims_dir();
        let stats = apply_hardlinks(&shims, &shim, &wanted).unwrap();
        assert_eq!(stats.linked, 2);
        // A real console shim, not a `.cmd` forwarder pointing nowhere, and not deleted by
        // this same rehash's `remove_stale` pass.
        assert_eq!(fs::read(shims.join("setvar.exe")).unwrap(), b"shim binary");
        assert!(!shims.join("setvar.cmd").exists());
    }

    #[test]
    fn a_forwarder_that_loses_its_pyenv_becomes_a_console_shim_and_the_old_cmd_is_removed() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        let pyenv = tmp.path().join("pyenv.exe");
        fs::write(&pyenv, b"cli").unwrap();
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/setvar.bat"));
        let wanted = shimset::shims_win(&v, &["setvar".to_string()]);
        let shims = ctx.shims_dir();
        apply_hardlinks(&shims, &shim, &wanted).unwrap();
        assert!(shims.join("setvar.cmd").is_file());
        // `pyenv.exe` disappears (or a differently-placed `shim` source is used, as when
        // rehash is called with a binary that isn't installed next to `pyenv.exe`).
        fs::remove_file(&pyenv).unwrap();
        let other_shim = tmp.path().join("elsewhere").join("pyenv-shim");
        fs::create_dir_all(other_shim.parent().unwrap()).unwrap();
        fs::write(&other_shim, b"shim binary").unwrap();
        apply_hardlinks(&shims, &other_shim, &wanted).unwrap();
        assert_eq!(fs::read(shims.join("setvar.exe")).unwrap(), b"shim binary");
        assert!(!shims.join("setvar.cmd").exists());
    }

    /// A forwarded name is written into the forwarder's command lines, so only
    /// `[A-Za-z0-9._+-]` is allowed; a space or `&` would break them.
    #[test]
    fn a_forwarded_name_outside_the_safe_set_falls_back_to_a_console_shim() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        fs::write(tmp.path().join("pyenv.exe"), b"cli").unwrap();
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        let names = ["set var", "a&b", "Ok_1.2+x-y"];
        for n in names {
            exe(&v.join(format!("3.9.1/Scripts/{n}.bat")));
        }
        let forward: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        let wanted = shimset::shims_win(&v, &forward);
        let shims = ctx.shims_dir();
        apply_hardlinks(&shims, &shim, &wanted).unwrap();
        for n in ["set var", "a&b"] {
            assert_eq!(
                fs::read(shims.join(format!("{n}.exe"))).unwrap(),
                b"shim binary"
            );
            assert!(!shims.join(format!("{n}.cmd")).exists(), "{n}");
        }
        assert!(shims.join("Ok_1.2+x-y.cmd").is_file());
        assert!(!shims.join("Ok_1.2+x-y.exe").exists());
    }

    #[test]
    fn a_non_ascii_forwarded_name_falls_back_to_a_console_shim_even_with_a_known_pyenv() {
        let (tmp, ctx, shim) = setup(Flavor::PyenvWin);
        let pyenv = tmp.path().join("pyenv.exe");
        fs::write(&pyenv, b"cli").unwrap();
        let v = ctx.versions_dir();
        exe(&v.join("3.9.1/python.exe"));
        exe(&v.join("3.9.1/Scripts/café.bat"));
        exe(&v.join("3.9.1/Scripts/setvar.bat"));
        let wanted = shimset::shims_win(&v, &["café".to_string(), "setvar".to_string()]);
        let shims = ctx.shims_dir();
        apply_hardlinks(&shims, &shim, &wanted).unwrap();
        // The ASCII name still gets a real forwarder...
        assert!(shims.join("setvar.cmd").is_file());
        // ...but the non-ASCII one, even though `pyenv.exe` is known and ASCII, falls
        // back: its own name can't go into the file any more safely than a non-ASCII
        // `pyenv.exe` path could.
        assert_eq!(fs::read(shims.join("café.exe")).unwrap(), b"shim binary");
        assert!(!shims.join("café.cmd").exists());
    }
}
