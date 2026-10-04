//! Extraction of python.org's Install Manager zips (spec §9.1). They have no root folder and
//! no directory entries (measured on 3.11.0 and 3.13.0t), so parents are created as needed.
//! Every name must be relative and made of plain Windows segments, and no path (compared
//! case-insensitively, as Windows does) may be written twice.

use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// `name` as a relative path of safe segments; `/` and `\` both separate.
fn safe_rel(name: &str) -> Option<PathBuf> {
    if name.starts_with(['/', '\\']) || name.contains(':') {
        return None;
    }
    let segs: Vec<&str> = name.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if segs.is_empty() || !segs.iter().all(|s| super::is_safe_win_segment(s)) {
        return None;
    }
    Some(segs.iter().collect())
}

pub fn extract_zip(archive: &Path, target: &Path) -> Result<usize, String> {
    let f = std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut z = zip::ZipArchive::new(BufReader::new(f))
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    std::fs::create_dir_all(target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut files = 0;
    for i in 0..z.len() {
        let mut e = z
            .by_index(i)
            .map_err(|e| format!("{}: {e}", archive.display()))?;
        let raw = e.name().to_string();
        let Some(rel) = safe_rel(&raw) else {
            return Err(format!("unsafe path in {}: {raw}", archive.display()));
        };
        let is_link = e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        if is_link {
            return Err(format!(
                "unsafe path in {}: {raw} is a link",
                archive.display()
            ));
        }
        let dest = target.join(&rel);
        if e.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|er| format!("{}: {er}", dest.display()))?;
            continue;
        }
        if !seen.insert(rel.to_string_lossy().to_lowercase()) {
            return Err(format!("{} writes {raw} twice", archive.display()));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|er| format!("{}: {er}", parent.display()))?;
        }
        let mut out =
            std::fs::File::create(&dest).map_err(|er| format!("{}: {er}", dest.display()))?;
        let want = e.size();
        let mut buf = vec![0u8; 1 << 16];
        let mut got: u64 = 0;
        loop {
            let n = e.read(&mut buf).map_err(|er| format!("{raw}: {er}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|er| format!("{}: {er}", dest.display()))?;
            got += n as u64;
        }
        if got != want {
            return Err(format!("{raw}: extracted {got} bytes, the zip says {want}"));
        }
        files += 1;
    }
    Ok(files)
}
