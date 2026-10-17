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

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Extracts into `build_dir/<name>` and returns that path. An existing `build_dir/<name>`
/// is replaced, so `name` must make a direct child of `build_dir`.
pub fn extract(
    archive: &Path,
    kind: Kind,
    build_dir: &Path,
    name: &str,
) -> Result<PathBuf, String> {
    extract_with(archive, kind, build_dir, name, false).map(|(dest, _)| dest)
}

/// Like `extract`, but a symbolic link whose target leaves the tree (libtirpc's `INSTALL`
/// points at `/usr/share/automake-1.18/INSTALL`) is skipped instead of refusing the archive:
/// it is not created, and the returned list names each one as `<path> -> <target>`. Hard
/// links and unsafe paths are refused as in `extract`.
pub fn extract_skipping_outside_links(
    archive: &Path,
    kind: Kind,
    build_dir: &Path,
    name: &str,
) -> Result<(PathBuf, Vec<String>), String> {
    extract_with(archive, kind, build_dir, name, true)
}

fn extract_with(
    archive: &Path,
    kind: Kind,
    build_dir: &Path,
    name: &str,
    skip_outside_symlinks: bool,
) -> Result<(PathBuf, Vec<String>), String> {
    let mut skipped: Vec<String> = Vec::new();
    let Some(dest) = super::child_of(build_dir, name) else {
        return Err(format!("invalid package name: {name}"));
    };
    let scratch = build_dir.join(format!(".extract-{name}"));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    // Removes the scratch directory on every exit; after a successful rename it is gone
    // already and this does nothing.
    let _guard = Scratch(scratch.clone());
    let result = (|| {
        let mut ar = tar::Archive::new(reader(archive, kind).map_err(|e| e.to_string())?);
        ar.set_preserve_permissions(true);
        // No setuid/setgid bits from an untrusted archive.
        ar.set_mask(0o6000);
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
                if !ok && !hard && skip_outside_symlinks {
                    skipped.push(format!("{} -> {}", path.display(), target.display()));
                    continue;
                }
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
    result?;
    let _ = std::fs::remove_dir_all(&dest);
    let tops: Vec<PathBuf> = std::fs::read_dir(&scratch)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    let moved = match tops.as_slice() {
        [only]
            if std::fs::symlink_metadata(only)
                .map(|m| m.file_type().is_dir())
                .unwrap_or(false) =>
        {
            std::fs::rename(only, &dest).and_then(|_| std::fs::remove_dir(&scratch))
        }
        _ => std::fs::rename(&scratch, &dest),
    };
    moved.map_err(|e| format!("{}: {e}", dest.display()))?;
    Ok((dest, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgz(entries: &[(&str, tar::EntryType, &str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, ty, link, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(*ty);
            h.set_mode(0o644);
            h.set_size(data.len() as u64);
            if !link.is_empty() {
                h.set_link_name(link).unwrap();
            }
            b.append_data(&mut h, path, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    fn write(dir: &Path, bytes: &[u8]) -> PathBuf {
        let p = dir.join("a.tar.gz");
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn outside_symlink() -> Vec<u8> {
        tgz(&[
            (
                "lib-1/INSTALL",
                tar::EntryType::Symlink,
                "/usr/share/automake-1.18/INSTALL",
                b"",
            ),
            ("lib-1/README", tar::EntryType::Regular, "", b"hello"),
        ])
    }

    #[test]
    fn extract_refuses_a_symlink_that_leaves_the_tree() {
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), &outside_symlink());
        let err = extract(&a, Kind::Gz, d.path(), "lib").unwrap_err();
        assert!(err.contains("unsafe link"), "{err}");
    }

    #[test]
    fn the_store_variant_skips_and_reports_a_symlink_that_leaves_the_tree() {
        let d = tempfile::tempdir().unwrap();
        let a = write(d.path(), &outside_symlink());
        let (dest, skipped) =
            extract_skipping_outside_links(&a, Kind::Gz, d.path(), "lib").unwrap();
        assert_eq!(
            skipped,
            ["lib-1/INSTALL -> /usr/share/automake-1.18/INSTALL"]
        );
        assert!(std::fs::symlink_metadata(dest.join("INSTALL")).is_err());
        assert_eq!(
            std::fs::read_to_string(dest.join("README")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn the_store_variant_still_refuses_an_outside_hard_link() {
        let d = tempfile::tempdir().unwrap();
        let a = write(
            d.path(),
            &tgz(&[("lib-1/x", tar::EntryType::Link, "../../etc/passwd", b"")]),
        );
        let err = extract_skipping_outside_links(&a, Kind::Gz, d.path(), "lib").unwrap_err();
        assert!(err.contains("unsafe link"), "{err}");
    }
}
