//! python-build's tarball extraction (reference "Download", step 5), refusing entries that
//! would land outside the build directory (review focus 1).

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Gz,
    Bz2,
    Xz,
}

impl Kind {
    /// python-build picks the type from the URL's end: `xz`, `bz2`, else gzip.
    pub fn of_url(url: &str) -> Kind {
        if url.ends_with("xz") {
            Kind::Xz
        } else if url.ends_with("bz2") {
            Kind::Bz2
        } else {
            Kind::Gz
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Kind::Gz => ".tar.gz",
            Kind::Bz2 => ".tar.bz2",
            Kind::Xz => ".tar.xz",
        }
    }
}

fn reader(archive: &Path, kind: Kind) -> std::io::Result<Box<dyn std::io::Read>> {
    let f = std::io::BufReader::new(std::fs::File::open(archive)?);
    Ok(match kind {
        Kind::Gz => Box::new(flate2::read::GzDecoder::new(f)),
        Kind::Bz2 => Box::new(bzip2::read::BzDecoder::new(f)),
        Kind::Xz => Box::new(lzma_rust2::XzReader::new(f, true)),
    })
}

fn safe_relative(p: &Path) -> bool {
    p.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Whether a link at `entry` (relative) pointing to `target` stays inside the root.
fn link_stays_inside(entry: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth: i32 = entry.components().count() as i32 - 1;
    for c in target.components() {
        match c {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => return false,
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

/// Extracts into `build_dir/<name>` and returns that path. An existing `build_dir/<name>`
/// is replaced.
pub fn extract(
    archive: &Path,
    kind: Kind,
    build_dir: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    let scratch = build_dir.join(format!(".extract-{name}"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let result = (|| {
        let mut ar = tar::Archive::new(reader(archive, kind).map_err(|e| e.to_string())?);
        ar.set_preserve_permissions(true);
        for entry in ar.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path().map_err(|e| e.to_string())?.into_owned();
            if !safe_relative(&path) {
                return Err(format!(
                    "unsafe path in {}: {}",
                    archive.display(),
                    path.display()
                ));
            }
            if let Some(target) = entry.link_name().map_err(|e| e.to_string())? {
                let hard = entry.header().entry_type() == tar::EntryType::Link;
                let ok = if hard {
                    safe_relative(&target)
                } else {
                    link_stays_inside(&path, &target)
                };
                if !ok {
                    return Err(format!(
                        "unsafe link in {}: {} -> {}",
                        archive.display(),
                        path.display(),
                        target.display()
                    ));
                }
            }
            entry.unpack_in(&scratch).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(e);
    }
    let dest = build_dir.join(name);
    let _ = std::fs::remove_dir_all(&dest);
    let tops: Vec<PathBuf> = std::fs::read_dir(&scratch)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    let moved = match tops.as_slice() {
        [only] if only.is_dir() => {
            std::fs::rename(only, &dest).and_then(|_| std::fs::remove_dir(&scratch))
        }
        _ => std::fs::rename(&scratch, &dest),
    };
    moved.map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok(dest)
}
