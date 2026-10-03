//! Administrative-install-equivalent extraction of a Windows Installer package, in pure Rust
//! (`msi` + `cab`): no msiexec, no registry, no custom actions (spec §9.1, plan Decision 4).
//! Compared with `msiexec /a` on 12 python.org MSIs in the M2b spike: 10,409 files, equal
//! path, size and SHA-256.
//!
//! Rules (each measured against msiexec):
//! * Every directory resolves under `target` by walking `Directory_Parent` up to the root row
//!   (`Directory_Parent` NULL or equal to itself), which is `target`.
//! * A `DefaultDir` is `[target][:source]`; an admin image uses the SOURCE part (the whole value
//!   when there is no colon). Each part is `short|long`; the LONG name is used, even when the
//!   summary Word Count sets bit 0 (short names). `.` adds no segment.
//! * `File.FileName` is `short|long`; the long name is written.
//! * A file is in a cabinet if its attributes have 0x4000, beside the MSI if 0x2000, otherwise
//!   per Word Count bit 1. Files beside the MSI are read flat by name (Word Count bit 1 set);
//!   python.org's MSIs have none.
//! * Only files of features with Level != 0 are written (the Condition table, INSTALLLEVEL and
//!   Component.Condition are ignored by an admin install).
//! * `Media.Cabinet` `#name` is a stream in the MSI; anything else is a file beside it.
//! * Nothing else is written: no copy of the .msi, no empty CreateFolder directories.
//! * Each written file's size must equal `File.FileSize`, and its MD5 the `MsiFileHash` row if
//!   there is one (an integrity check of the extraction; authenticity comes from the `.asc`).

use md5::{Digest as _, Md5};
use msi::{Package, Select};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

const ATTR_NONCOMPRESSED: i32 = 0x2000;
const ATTR_COMPRESSED: i32 = 0x4000;
const WC_SHORT_NAMES: i32 = 0x1;
const WC_COMPRESSED: i32 = 0x2;

/// One payload file of the package, with the path an admin install gives it.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub key: String,
    /// Relative path below the admin-install target (components joined with '\\' on output).
    pub rel: Vec<String>,
    pub size: u64,
    pub compressed: bool,
    /// Name an uncompressed file has beside the MSI (short name if Word Count bit 0 is set).
    pub source_name: String,
    pub sequence: i32,
    /// MD5 from MsiFileHash, if present (16 bytes, little-endian parts concatenated).
    pub md5: Option<[u8; 16]>,
    /// False when every feature owning the component has Level 0: `msiexec /a` skips those.
    pub installed: bool,
}

/// Media row: files with Sequence <= last_sequence (and above the previous row's) live here.
#[derive(Debug, Clone)]
pub struct MediaRow {
    pub disk_id: i32,
    pub last_sequence: i32,
    pub cabinet: Option<String>,
}

#[derive(Debug)]
pub struct Plan {
    pub word_count: i32,
    pub files: Vec<PlannedFile>,
    pub media: Vec<MediaRow>,
}

fn pick(name: &str, short: bool) -> &str {
    match name.split_once('|') {
        Some((s, l)) => {
            if short {
                s
            } else {
                l
            }
        }
        None => name,
    }
}

/// Refuses any name that could escape the target or isn't a plain Windows name segment.
fn check_segment(seg: &str, what: &str) -> Result<(), String> {
    if super::is_safe_win_segment(seg) {
        Ok(())
    } else {
        Err(format!("unsafe {what} name segment {seg:?}"))
    }
}

fn str_col(row: &msi::Row, col: &str) -> Option<String> {
    row[col].as_str().map(|s| s.to_string())
}

fn io_err<E: std::fmt::Display>(ctx: &str) -> impl FnOnce(E) -> String + '_ {
    move |e| format!("{ctx}: {e}")
}

/// Read the tables and work out where every file goes. Does not touch the cabinets.
pub fn plan_msi<F: Read + io::Seek>(pkg: &mut Package<F>) -> Result<Plan, String> {
    let word_count = pkg.summary_info().word_count().unwrap_or(0);
    let short = word_count & WC_SHORT_NAMES != 0;
    let default_compressed = word_count & WC_COMPRESSED != 0;

    // Directory table.
    let mut dirs: HashMap<String, (Option<String>, String)> = HashMap::new();
    for row in pkg
        .select_rows(Select::table("Directory"))
        .map_err(io_err("Directory"))?
    {
        let key = str_col(&row, "Directory").ok_or("Directory row without key")?;
        let parent = str_col(&row, "Directory_Parent");
        let dd = str_col(&row, "DefaultDir").ok_or("Directory row without DefaultDir")?;
        dirs.insert(key, (parent, dd));
    }
    let mut resolved: HashMap<String, Vec<String>> = HashMap::new();
    fn resolve(
        key: &str,
        dirs: &HashMap<String, (Option<String>, String)>,
        resolved: &mut HashMap<String, Vec<String>>,
        depth: usize,
    ) -> Result<Vec<String>, String> {
        if let Some(p) = resolved.get(key) {
            return Ok(p.clone());
        }
        if depth > 256 {
            return Err(format!("Directory cycle at {key:?}"));
        }
        let (parent, dd) = dirs
            .get(key)
            .ok_or_else(|| format!("unknown Directory {key:?}"))?;
        let path = match parent {
            None => Vec::new(),
            Some(p) if p == key => Vec::new(),
            Some(p) => {
                let mut base = resolve(p, dirs, resolved, depth + 1)?;
                // "[target][:source]"; the admin image follows the source part.
                let source_part = match dd.split_once(':') {
                    Some((_, src)) => src,
                    None => dd.as_str(),
                };
                let seg = pick(source_part, false); // long, even with Word Count bit 0 (synth3)
                if seg != "." {
                    check_segment(seg, "directory")?;
                    base.push(seg.to_string());
                }
                base
            }
        };
        resolved.insert(key.to_string(), path.clone());
        Ok(path)
    }
    let keys: Vec<String> = dirs.keys().cloned().collect();
    for k in &keys {
        resolve(k, &dirs, &mut resolved, 0)?;
    }

    // Component -> Directory.
    let mut comp_dir: HashMap<String, String> = HashMap::new();
    for row in pkg
        .select_rows(Select::table("Component"))
        .map_err(io_err("Component"))?
    {
        let c = str_col(&row, "Component").ok_or("Component row without key")?;
        let d = str_col(&row, "Directory_").ok_or("Component without Directory_")?;
        comp_dir.insert(c, d);
    }

    // Features: an admin install skips features whose Feature.Level is 0 (and, inferred, their
    // children). It does NOT apply the Condition table (measured: 2.7.18's PrivateCRT has a
    // true condition with Level 1 and is still skipped) and ignores INSTALLLEVEL (Level 2
    // PrependPath was installed).
    let enabled_components: Option<HashSet<String>> = if pkg.has_table("Feature")
        && pkg.has_table("FeatureComponents")
    {
        let mut feats: HashMap<String, (Option<String>, i32)> = HashMap::new();
        for row in pkg
            .select_rows(Select::table("Feature"))
            .map_err(io_err("Feature"))?
        {
            let k = str_col(&row, "Feature").ok_or("Feature row without key")?;
            feats.insert(
                k,
                (
                    str_col(&row, "Feature_Parent"),
                    row["Level"].as_int().unwrap_or(0),
                ),
            );
        }
        let enabled = |start: &str| -> bool {
            let mut k = start.to_string();
            for _ in 0..64 {
                match feats.get(&k) {
                    None => return false,
                    Some((_, 0)) => return false,
                    Some((None, _)) => return true,
                    Some((Some(p), _)) if *p == k => return true,
                    Some((Some(p), _)) => k = p.clone(),
                }
            }
            false
        };
        let mut set = HashSet::new();
        for row in pkg
            .select_rows(Select::table("FeatureComponents"))
            .map_err(io_err("FeatureComponents"))?
        {
            let f = str_col(&row, "Feature_").ok_or("FeatureComponents without Feature_")?;
            if enabled(&f) {
                set.insert(
                    str_col(&row, "Component_").ok_or("FeatureComponents without Component_")?,
                );
            }
        }
        Some(set)
    } else {
        None
    };

    // MsiFileHash (optional).
    let mut hashes: HashMap<String, [u8; 16]> = HashMap::new();
    if pkg.has_table("MsiFileHash") {
        for row in pkg
            .select_rows(Select::table("MsiFileHash"))
            .map_err(io_err("MsiFileHash"))?
        {
            let k = str_col(&row, "File_").ok_or("MsiFileHash without File_")?;
            let mut md5 = [0u8; 16];
            for (i, col) in ["HashPart1", "HashPart2", "HashPart3", "HashPart4"]
                .iter()
                .enumerate()
            {
                let v = row[*col].as_int().ok_or("MsiFileHash part not an int")?;
                md5[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            hashes.insert(k, md5);
        }
    }

    let mut files = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    for row in pkg
        .select_rows(Select::table("File"))
        .map_err(io_err("File"))?
    {
        let key = str_col(&row, "File").ok_or("File row without key")?;
        let comp = str_col(&row, "Component_").ok_or("File without Component_")?;
        let fname = str_col(&row, "FileName").ok_or("File without FileName")?;
        let size = row["FileSize"].as_int().ok_or("File without FileSize")?;
        let attrs = row["Attributes"].as_int().unwrap_or(0);
        let sequence = row["Sequence"].as_int().ok_or("File without Sequence")?;
        let dir = comp_dir
            .get(&comp)
            .ok_or_else(|| format!("File {key:?}: unknown component {comp:?}"))?;
        let mut rel = resolved
            .get(dir)
            .cloned()
            .ok_or_else(|| format!("unknown directory {dir:?}"))?;
        // The admin image always gets LONG names; Word Count bit 0 only changes the name an
        // uncompressed source file is read under (measured with synth3.msi).
        let leaf = pick(&fname, false);
        check_segment(leaf, "file")?;
        let source_name = pick(&fname, short).to_string();
        check_segment(&source_name, "file")?;
        rel.push(leaf.to_string());
        let installed = enabled_components
            .as_ref()
            .is_none_or(|s| s.contains(&comp));
        // Windows paths are case-insensitive: two rows writing one path would silently clobber.
        if installed && !seen_paths.insert(rel.join("\\").to_lowercase()) {
            return Err(format!("two File rows map to {:?}", rel.join("\\")));
        }
        let compressed = if attrs & ATTR_COMPRESSED != 0 {
            true
        } else if attrs & ATTR_NONCOMPRESSED != 0 {
            false
        } else {
            default_compressed
        };
        if size < 0 {
            return Err(format!("File {key:?} has negative size"));
        }
        files.push(PlannedFile {
            md5: hashes.get(&key).copied(),
            key,
            rel,
            size: size as u64,
            compressed,
            source_name,
            sequence,
            installed,
        });
    }

    let mut media = Vec::new();
    for row in pkg
        .select_rows(Select::table("Media"))
        .map_err(io_err("Media"))?
    {
        media.push(MediaRow {
            disk_id: row["DiskId"].as_int().ok_or("Media without DiskId")?,
            last_sequence: row["LastSequence"]
                .as_int()
                .ok_or("Media without LastSequence")?,
            cabinet: str_col(&row, "Cabinet").filter(|s| !s.is_empty()),
        });
    }
    media.sort_by_key(|m| m.last_sequence);
    Ok(Plan {
        word_count,
        files,
        media,
    })
}

fn rel_to_path(target: &Path, rel: &[String]) -> PathBuf {
    let mut p = target.to_path_buf();
    for s in rel {
        p.push(s);
    }
    p
}

/// Copy `reader` to `dest`, checking size and (if given) MD5 on the way.
fn write_checked(reader: &mut dyn Read, dest: &Path, f: &PlannedFile) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(io_err("create_dir_all"))?;
    }
    let mut tmp_name = dest.as_os_str().to_owned();
    tmp_name.push(".rpyenv-partial");
    let tmp = PathBuf::from(tmp_name);
    let mut out = File::create(&tmp).map_err(|e| format!("create {}: {e}", tmp.display()))?;
    let mut md5 = Md5::new();
    let mut total: u64 = 0;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", f.key))?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(io_err("write"))?;
        total += n as u64;
    }
    drop(out);
    if total != f.size {
        let _ = fs::remove_file(&tmp);
        return Err(format!(
            "{}: extracted {total} bytes, File.FileSize says {}",
            f.key, f.size
        ));
    }
    // MsiFileHash stores all-zero parts for 0-byte files (measured on 3.10.11 lib.msi,
    // 2.7.18): MD5("") is d41d8cd9..., so the zero row is a "no hash" marker there.
    let md5_marker_only = f.size == 0 && f.md5 == Some([0u8; 16]);
    if let (Some(want), false) = (f.md5, md5_marker_only) {
        let got: [u8; 16] = md5.finalize().into();
        if got != want {
            let _ = fs::remove_file(&tmp);
            return Err(format!("{}: MD5 differs from MsiFileHash", f.key));
        }
    }
    fs::rename(&tmp, dest).map_err(|e| format!("rename to {}: {e}", dest.display()))?;
    Ok(())
}

/// Extract every payload file of `msi` into `target` (created if needed), laid out as
/// `msiexec /a` lays it out. Returns the written paths in the order they were written.
pub fn extract_msi(msi: &Path, target: &Path) -> Result<Vec<PathBuf>, String> {
    let file = File::open(msi).map_err(|e| format!("open {}: {e}", msi.display()))?;
    let mut pkg = Package::open(file).map_err(|e| format!("not an MSI {}: {e}", msi.display()))?;
    fs::create_dir_all(target).map_err(|e| format!("create {}: {e}", target.display()))?;
    let plan = plan_msi(&mut pkg)?;
    let msi_dir = msi.parent().unwrap_or(Path::new("."));
    let by_key: HashMap<&str, &PlannedFile> =
        plan.files.iter().map(|f| (f.key.as_str(), f)).collect();
    let mut done: HashSet<String> = HashSet::new();
    let mut written = Vec::new();

    for m in &plan.media {
        let Some(cab_name) = &m.cabinet else { continue };
        let cab_bytes: Vec<u8> = if let Some(stream) = cab_name.strip_prefix('#') {
            let mut r = pkg
                .read_stream(stream)
                .map_err(|e| format!("cabinet stream {stream:?}: {e}"))?;
            let mut v = Vec::new();
            r.read_to_end(&mut v)
                .map_err(io_err("read cabinet stream"))?;
            v
        } else {
            check_segment(cab_name, "cabinet")?;
            fs::read(msi_dir.join(cab_name))
                .map_err(|e| format!("external cabinet {cab_name:?}: {e}"))?
        };
        let mut on_entry = |name: &str, reader: &mut dyn Read| -> Result<(), String> {
            let Some(f) = by_key.get(name) else {
                return Err(format!(
                    "cabinet {cab_name:?} holds {name:?}, which is not in the File table"
                ));
            };
            if !f.compressed {
                return Err(format!(
                    "{name:?} is in a cabinet but its attributes say uncompressed"
                ));
            }
            if !done.insert(name.to_string()) {
                return Err(format!("{name:?} appears in more than one cabinet entry"));
            }
            if !f.installed {
                return Ok(()); // the sequential reader skips unread bytes itself
            }
            let dest = rel_to_path(target, &f.rel);
            write_checked(reader, &dest, f)?;
            written.push(dest);
            Ok(())
        };
        cab_for_each_sequential(cab_bytes, &mut on_entry)
            .map_err(|e| format!("cabinet {cab_name:?}: {e}"))?;
    }

    // Uncompressed (0x2000) files are read from beside the MSI. In a package whose summary
    // Word Count says "compressed" (bit 1), msiexec /a read them FLAT from the MSI's folder by
    // their long name, not from the source-directory tree (measured with synth2.msi). For a
    // package without that bit the tree path is assumed (inferred, not measured).
    let pkg_compressed = plan.word_count & WC_COMPRESSED != 0;
    for f in &plan.files {
        if done.contains(&f.key) || !f.installed {
            continue;
        }
        if f.compressed {
            return Err(format!(
                "{:?} is marked compressed but no cabinet holds it",
                f.key
            ));
        }
        if !pkg_compressed {
            // Uncompressed-image layout (source tree) is not measured; python.org never uses it.
            return Err(format!(
                "{:?}: uncompressed source image layout is not supported",
                f.key
            ));
        }
        let src = msi_dir.join(&f.source_name);
        let mut reader =
            File::open(&src).map_err(|e| format!("uncompressed source {}: {e}", src.display()))?;
        let dest = rel_to_path(target, &f.rel);
        write_checked(&mut reader, &dest, f)?;
        written.push(dest);
    }
    Ok(written)
}

/// One CFFILE record, parsed straight from the cabinet bytes.
struct CabEntry {
    name: String,
    size: u32,
    offset: u32,
    folder: u16,
    record_pos: usize,
}

fn parse_cffiles(b: &[u8]) -> Result<(Vec<CabEntry>, u16), String> {
    let rd16 = |o: usize| {
        b.get(o..o + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]))
            .ok_or("cabinet truncated")
    };
    let rd32 = |o: usize| {
        b.get(o..o + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or("cabinet truncated")
    };
    if b.get(0..4) != Some(b"MSCF".as_slice()) {
        return Err("not a cabinet".into());
    }
    let coff_files = rd32(16)? as usize;
    let c_folders = rd16(26)?;
    let c_files = rd16(28)?;
    let flags = rd16(30)?;
    if flags & 0x3 != 0 {
        return Err("multi-cabinet sets (prev/next cabinet) are not supported".into());
    }
    let mut entries = Vec::with_capacity(c_files as usize);
    let mut pos = coff_files;
    for _ in 0..c_files {
        let size = rd32(pos)?;
        let offset = rd32(pos + 4)?;
        let folder = rd16(pos + 8)?;
        if folder >= c_folders {
            return Err(format!(
                "CFFILE folder index {folder:#x} (continued folder?) not supported"
            ));
        }
        let name_start = pos + 16;
        let nul = b
            .get(name_start..)
            .and_then(|t| t.iter().position(|&c| c == 0))
            .ok_or("cabinet truncated")?;
        let raw = &b[name_start..name_start + nul];
        if !raw.is_ascii() {
            return Err("non-ASCII cabinet entry name".into());
        }
        entries.push(CabEntry {
            name: String::from_utf8_lossy(raw).into_owned(),
            size,
            offset,
            folder,
            record_pos: pos,
        });
        pos = name_start + nul + 1;
    }
    Ok((entries, c_folders))
}

/// Decompress each CAB folder exactly once.
///
/// The `cab` crate (0.6.0) only exposes `read_file(name)`, which builds a fresh folder reader
/// and decompresses from the folder's first block every time: O(n^2) in a folder of n files.
/// Its decompressors are fine, so this patches (in memory) the CFFILE record of the folder's
/// first file to span the whole folder, reads that one "file" as a stream, and cuts it at the
/// real entries' offsets.
pub fn cab_for_each_sequential(
    mut bytes: Vec<u8>,
    on_entry: &mut dyn FnMut(&str, &mut dyn Read) -> Result<(), String>,
) -> Result<(), String> {
    let (entries, c_folders) = parse_cffiles(&bytes)?;
    let mut per_folder: Vec<Vec<&CabEntry>> = vec![Vec::new(); c_folders as usize];
    for e in &entries {
        per_folder[e.folder as usize].push(e);
    }
    let mut span_names = Vec::new();
    for list in per_folder.iter_mut() {
        // Zero-length entries first at a shared offset, then duplicates adjacent.
        list.sort_by(|a, b| (a.offset, a.size, &a.name).cmp(&(b.offset, b.size, &b.name)));
        let Some(first) = list.first() else {
            span_names.push(None);
            continue;
        };
        let end = list
            .iter()
            .map(|e| e.offset as u64 + e.size as u64)
            .max()
            .unwrap_or(0);
        let end = u32::try_from(end).map_err(|_| "folder larger than 4 GiB")?;
        bytes[first.record_pos..first.record_pos + 4].copy_from_slice(&end.to_le_bytes());
        bytes[first.record_pos + 4..first.record_pos + 8].copy_from_slice(&0u32.to_le_bytes());
        span_names.push(Some(first.name.clone()));
    }
    let mut cab = cab::Cabinet::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for (list, span) in per_folder.iter().zip(span_names) {
        let Some(span) = span else { continue };
        let mut reader = cab.read_file(&span).map_err(|e| e.to_string())?;
        let mut cur: u64 = 0;
        let mut i = 0;
        while i < list.len() {
            let e = list[i];
            // Smart-cabbed duplicates: several entries share one (offset, size) range
            // (measured: 3.10.11 lib.msi, 2.7.18's VC90 merge module).
            let mut j = i + 1;
            while j < list.len() && list[j].offset == e.offset && list[j].size == e.size {
                j += 1;
            }
            let off = e.offset as u64;
            if off < cur {
                return Err(format!("partially overlapping entries at {:?}", e.name));
            }
            io::copy(&mut (&mut reader).take(off - cur), &mut io::sink())
                .map_err(|e| e.to_string())?;
            if j - i == 1 {
                let mut part = (&mut reader).take(e.size as u64);
                // write_checked() fails on a short read, so the part is consumed fully.
                on_entry(&e.name, &mut part)?;
                // Entries the caller skipped (features at Level 0) must still be read past.
                io::copy(&mut part, &mut io::sink()).map_err(|e| e.to_string())?;
            } else {
                let mut buf = Vec::with_capacity(e.size as usize);
                (&mut reader)
                    .take(e.size as u64)
                    .read_to_end(&mut buf)
                    .map_err(|e| e.to_string())?;
                for d in &list[i..j] {
                    on_entry(&d.name, &mut Cursor::new(&buf))?;
                }
            }
            cur = off + e.size as u64;
            i = j;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_and_the_source_part() {
        assert_eq!(pick("PYTHON~1|Python Tools", false), "Python Tools");
        assert_eq!(pick("PYTHON~1|Python Tools", true), "PYTHON~1");
        assert_eq!(pick("Lib", false), "Lib");
    }

    #[test]
    fn unsafe_segments_are_refused() {
        assert!(check_segment("..", "directory").is_err());
        assert!(check_segment("a\\b", "file").is_err());
        assert!(check_segment("CON", "file").is_err());
        assert!(check_segment("os.py", "file").is_ok());
    }

    fn planned(size: u64, md5: Option<[u8; 16]>) -> PlannedFile {
        PlannedFile {
            key: "k".into(),
            rel: vec!["f".into()],
            size,
            compressed: true,
            source_name: "f".into(),
            sequence: 1,
            md5,
            installed: true,
        }
    }

    #[test]
    fn a_size_that_differs_from_the_file_table_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let e =
            write_checked(&mut &b"abc"[..], &d.path().join("f"), &planned(4, None)).unwrap_err();
        assert!(e.contains("File.FileSize says 4"), "{e}");
        assert!(!d.path().join("f").exists());
    }

    #[test]
    fn an_md5_that_differs_from_msifilehash_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let e = write_checked(
            &mut &b"abc"[..],
            &d.path().join("f"),
            &planned(3, Some([7; 16])),
        )
        .unwrap_err();
        assert!(e.contains("MD5 differs"), "{e}");
    }

    #[test]
    fn an_all_zero_hash_on_an_empty_file_is_a_marker_not_a_mismatch() {
        let d = tempfile::tempdir().unwrap();
        write_checked(
            &mut &b""[..],
            &d.path().join("f"),
            &planned(0, Some([0; 16])),
        )
        .unwrap();
        assert!(d.path().join("f").is_file());
    }

    /// A two-folder MSZIP cabinet with a zero-length entry and a smart-cab duplicate, built
    /// with the `cab` crate; the sequential reader must hand back each entry's exact bytes.
    #[test]
    fn the_sequential_reader_returns_every_entry_once_and_whole() {
        let mut b = cab::CabinetBuilder::new();
        {
            let f = b.add_folder(cab::CompressionType::MsZip);
            f.add_file("a");
            f.add_file("empty");
            f.add_file("b");
        }
        b.add_folder(cab::CompressionType::None).add_file("c");
        let mut w = b.build(Cursor::new(Vec::new())).unwrap();
        let bodies: [&[u8]; 4] = [b"alpha".as_slice(), b"", &[b'x'; 70_000], b"charlie"];
        let mut i = 0;
        while let Some(mut fw) = w.next_file().unwrap() {
            fw.write_all(bodies[i]).unwrap();
            i += 1;
        }
        let bytes = w.finish().unwrap().into_inner();
        let mut got: Vec<(String, Vec<u8>)> = Vec::new();
        cab_for_each_sequential(bytes, &mut |name, r| {
            let mut v = Vec::new();
            r.read_to_end(&mut v).map_err(|e| e.to_string())?;
            got.push((name.to_string(), v));
            Ok(())
        })
        .unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("a".to_string(), b"alpha".to_vec()),
                ("b".to_string(), vec![b'x'; 70_000]),
                ("c".to_string(), b"charlie".to_vec()),
                ("empty".to_string(), Vec::new()),
            ]
        );
    }

    /// Smart-cabbed duplicates: two entries sharing one (offset, size) range must both be
    /// delivered with the same bytes. Built by pointing the second entry's CFFILE offset at the
    /// first's, as the MSI toolchain does for identical files.
    #[test]
    fn entries_sharing_one_range_are_both_delivered() {
        let mut b = cab::CabinetBuilder::new();
        {
            let f = b.add_folder(cab::CompressionType::MsZip);
            f.add_file("d1");
            f.add_file("d2");
        }
        let mut w = b.build(Cursor::new(Vec::new())).unwrap();
        while let Some(mut fw) = w.next_file().unwrap() {
            fw.write_all(b"dupe!").unwrap();
        }
        let mut bytes = w.finish().unwrap().into_inner();
        let (entries, _) = parse_cffiles(&bytes).unwrap();
        let d2 = entries.iter().find(|e| e.name == "d2").unwrap().record_pos;
        bytes[d2 + 4..d2 + 8].copy_from_slice(&0u32.to_le_bytes());
        let mut got: Vec<(String, Vec<u8>)> = Vec::new();
        cab_for_each_sequential(bytes, &mut |name, r| {
            let mut v = Vec::new();
            r.read_to_end(&mut v).map_err(|e| e.to_string())?;
            got.push((name.to_string(), v));
            Ok(())
        })
        .unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("d1".to_string(), b"dupe!".to_vec()),
                ("d2".to_string(), b"dupe!".to_vec())
            ]
        );
    }

    #[test]
    fn a_non_cabinet_is_refused() {
        let e = cab_for_each_sequential(b"PK\x03\x04".to_vec(), &mut |_, _| Ok(())).unwrap_err();
        assert_eq!(e, "not a cabinet");
    }

    // ---- Hostile packages: one test per `check_segment` call site -------------------------

    /// A minimal package with a TARGETDIR root, one child directory `D` (DefaultDir
    /// `dir_default`), one component and one file. The `msi` crate's writer doesn't validate
    /// like Windows Installer does, which is what a hostile package would look like to us.
    fn hostile_msi(
        dir_default: &str,
        file_name: &str,
        attrs: i32,
        cabinet: Option<&str>,
        word_count: i32,
    ) -> Vec<u8> {
        use ::msi::{Column, Insert, PackageType, Value};
        let mut p =
            ::msi::Package::create(PackageType::Installer, Cursor::new(Vec::new())).unwrap();
        p.summary_info_mut().set_word_count(word_count);
        let id = |n: &str| Column::build(n).primary_key().id_string(72);
        p.create_table(
            "Directory",
            vec![
                id("Directory"),
                Column::build("Directory_Parent").nullable().id_string(72),
                Column::build("DefaultDir").string(255),
            ],
        )
        .unwrap();
        p.create_table(
            "Component",
            vec![id("Component"), Column::build("Directory_").id_string(72)],
        )
        .unwrap();
        p.create_table(
            "File",
            vec![
                id("File"),
                Column::build("Component_").id_string(72),
                Column::build("FileName").string(255),
                Column::build("FileSize").int32(),
                Column::build("Attributes").nullable().int16(),
                Column::build("Sequence").int16(),
            ],
        )
        .unwrap();
        p.create_table(
            "Media",
            vec![
                Column::build("DiskId").primary_key().int16(),
                Column::build("LastSequence").int16(),
                Column::build("Cabinet").nullable().string(255),
            ],
        )
        .unwrap();
        let s = |v: &str| Value::Str(v.to_string());
        p.insert_rows(Insert::into("Directory").row(vec![
            s("TARGETDIR"),
            Value::Null,
            s("SourceDir"),
        ]))
        .unwrap();
        p.insert_rows(Insert::into("Directory").row(vec![s("D"), s("TARGETDIR"), s(dir_default)]))
            .unwrap();
        p.insert_rows(Insert::into("Component").row(vec![s("C"), s("D")]))
            .unwrap();
        p.insert_rows(Insert::into("File").row(vec![
            s("F"),
            s("C"),
            s(file_name),
            Value::Int(3),
            Value::Int(attrs),
            Value::Int(1),
        ]))
        .unwrap();
        p.insert_rows(Insert::into("Media").row(vec![
            Value::Int(1),
            Value::Int(1),
            cabinet.map_or(Value::Null, s),
        ]))
        .unwrap();
        p.flush().unwrap();
        p.into_inner().unwrap().into_inner()
    }

    /// Extracts `bytes` next to a canary and asserts the error names `needle` and that nothing
    /// was written outside `target` (nor anything inside it).
    fn assert_refused(bytes: Vec<u8>, needle: &str) {
        let d = tempfile::tempdir().unwrap();
        let canary = d.path().join("canary.txt");
        fs::write(&canary, b"untouched").unwrap();
        let msi_path = d.path().join("h.msi");
        fs::write(&msi_path, bytes).unwrap();
        let target = d.path().join("target");
        let e = extract_msi(&msi_path, &target).unwrap_err();
        assert!(e.contains(needle), "{e}");
        assert_eq!(fs::read(&canary).unwrap(), b"untouched");
        let mut names: Vec<String> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["canary.txt", "h.msi", "target"]);
        assert_eq!(
            fs::read_dir(&target).unwrap().count(),
            0,
            "target must stay empty"
        );
    }

    #[test]
    fn a_directory_that_climbs_out_is_refused() {
        assert_refused(
            hostile_msi("..", "f.txt", 0x4000, Some("#c"), 2),
            "unsafe directory name segment \"..\"",
        );
        assert_refused(
            hostile_msi("T:CON", "f.txt", 0x4000, Some("#c"), 2),
            "unsafe directory name segment \"CON\"",
        );
    }

    #[test]
    fn a_file_name_with_a_separator_is_refused() {
        // Word Count bit 0 set, so the safe short part is not the leaf: only the leaf check sees it.
        assert_refused(
            hostile_msi("ok", "OK|a\\b", 0x4000, Some("#c"), 3),
            "unsafe file name segment \"a\\\\b\"",
        );
    }

    #[test]
    fn an_unsafe_short_source_name_is_refused() {
        // Word Count bit 0: the source name is the short part; the long leaf is fine.
        assert_refused(
            hostile_msi("ok", "SH:ORT|good.txt", 0x2000, None, 3),
            "unsafe file name segment \"SH:ORT\"",
        );
    }

    #[test]
    fn an_external_cabinet_name_that_climbs_out_is_refused() {
        assert_refused(
            hostile_msi("ok", "f.txt", 0x4000, Some("..\\x"), 2),
            "unsafe cabinet name segment \"..\\\\x\"",
        );
    }
}
