//! Administrative-install-equivalent extraction of a Windows Installer package,
//! in pure Rust (`msi` + `cab` crates). No msiexec, no registry, no custom actions.
//!
//! Rules (each one checked against `msiexec /a` in the spike, see REPORT.md):
//! * Every directory resolves under `target` by walking `Directory_Parent` up to the
//!   root row (`Directory_Parent` NULL or equal to itself), which maps to `target`.
//! * A `DefaultDir` value is `[target][:source]`; an admin image uses the SOURCE part
//!   (the whole value when there is no colon). Each part is `short|long`; the LONG name is
//!   used, even when the summary Word Count sets bit 0 (short names). `.` adds no segment.
//! * `File.FileName` is `short|long`; the long name is used for the output.
//! * A file is compressed (in a cabinet) if its attributes have msidbFileAttributesCompressed
//!   (0x4000), uncompressed if msidbFileAttributesNoncompressed (0x2000), otherwise per the
//!   summary Word Count bit 1. Uncompressed files are read from beside the MSI (flat, by long
//!   name, when Word Count bit 1 is set). python.org's MSIs have none.
//! * Only files of features with Level != 0 are written (Condition table, INSTALLLEVEL and
//!   Component.Condition are all ignored by an admin install).
//! * Cabinets come from `Media.Cabinet`: `#name` is a stream inside the MSI, anything else is a
//!   file next to the MSI. Cabinet entry names are `File.File` keys.
//! * Nothing else is written: no copy of the .msi, no empty CreateFolder directories, no
//!   DuplicateFile/MoveFile processing (python.org's AdminExecuteSequence runs none of those).
//! * File times are NOT set (msiexec /a keeps the cabinet entry's date as local time).
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

/// Reject anything that could escape the target or is not a plain Windows name segment.
fn check_segment(seg: &str, what: &str) -> Result<(), String> {
    let bad = seg.is_empty()
        || seg == "."
        || seg == ".."
        || seg.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0' | '<' | '>' | '"' | '|' | '?' | '*') || (c as u32) < 0x20);
    if bad {
        return Err(format!("unsafe {what} name segment {seg:?}"));
    }
    Ok(())
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
    for row in pkg.select_rows(Select::table("Directory")).map_err(io_err("Directory"))? {
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
        short: bool,
        depth: usize,
    ) -> Result<Vec<String>, String> {
        if let Some(p) = resolved.get(key) {
            return Ok(p.clone());
        }
        if depth > 256 {
            return Err(format!("Directory cycle at {key:?}"));
        }
        let (parent, dd) = dirs.get(key).ok_or_else(|| format!("unknown Directory {key:?}"))?;
        let path = match parent {
            None => Vec::new(),
            Some(p) if p == key => Vec::new(),
            Some(p) => {
                let mut base = resolve(p, dirs, resolved, short, depth + 1)?;
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
        resolve(k, &dirs, &mut resolved, short, 0)?;
    }

    // Component -> Directory.
    let mut comp_dir: HashMap<String, String> = HashMap::new();
    for row in pkg.select_rows(Select::table("Component")).map_err(io_err("Component"))? {
        let c = str_col(&row, "Component").ok_or("Component row without key")?;
        let d = str_col(&row, "Directory_").ok_or("Component without Directory_")?;
        comp_dir.insert(c, d);
    }

    // Features: an admin install skips features whose Feature.Level is 0 (and, inferred, their
    // children). It does NOT apply the Condition table (measured: 2.7.18's PrivateCRT has a
    // true condition with Level 1 and is still skipped) and ignores INSTALLLEVEL (Level 2
    // PrependPath was installed).
    let enabled_components: Option<HashSet<String>> = if pkg.has_table("Feature") && pkg.has_table("FeatureComponents") {
        let mut feats: HashMap<String, (Option<String>, i32)> = HashMap::new();
        for row in pkg.select_rows(Select::table("Feature")).map_err(io_err("Feature"))? {
            let k = str_col(&row, "Feature").ok_or("Feature row without key")?;
            feats.insert(k, (str_col(&row, "Feature_Parent"), row["Level"].as_int().unwrap_or(0)));
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
        for row in pkg.select_rows(Select::table("FeatureComponents")).map_err(io_err("FeatureComponents"))? {
            let f = str_col(&row, "Feature_").ok_or("FeatureComponents without Feature_")?;
            if enabled(&f) {
                set.insert(str_col(&row, "Component_").ok_or("FeatureComponents without Component_")?);
            }
        }
        Some(set)
    } else {
        None
    };

    // MsiFileHash (optional).
    let mut hashes: HashMap<String, [u8; 16]> = HashMap::new();
    if pkg.has_table("MsiFileHash") {
        for row in pkg.select_rows(Select::table("MsiFileHash")).map_err(io_err("MsiFileHash"))? {
            let k = str_col(&row, "File_").ok_or("MsiFileHash without File_")?;
            let mut md5 = [0u8; 16];
            for (i, col) in ["HashPart1", "HashPart2", "HashPart3", "HashPart4"].iter().enumerate() {
                let v = row[*col].as_int().ok_or("MsiFileHash part not an int")?;
                md5[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            hashes.insert(k, md5);
        }
    }

    let mut files = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    for row in pkg.select_rows(Select::table("File")).map_err(io_err("File"))? {
        let key = str_col(&row, "File").ok_or("File row without key")?;
        let comp = str_col(&row, "Component_").ok_or("File without Component_")?;
        let fname = str_col(&row, "FileName").ok_or("File without FileName")?;
        let size = row["FileSize"].as_int().ok_or("File without FileSize")?;
        let attrs = row["Attributes"].as_int().unwrap_or(0);
        let sequence = row["Sequence"].as_int().ok_or("File without Sequence")?;
        let dir = comp_dir.get(&comp).ok_or_else(|| format!("File {key:?}: unknown component {comp:?}"))?;
        let mut rel = resolved.get(dir).cloned().ok_or_else(|| format!("unknown directory {dir:?}"))?;
        // The admin image always gets LONG names; Word Count bit 0 only changes the name an
        // uncompressed source file is read under (measured with synth3.msi).
        let leaf = pick(&fname, false);
        check_segment(leaf, "file")?;
        let source_name = pick(&fname, short).to_string();
        check_segment(&source_name, "file")?;
        rel.push(leaf.to_string());
        let installed = enabled_components.as_ref().map_or(true, |s| s.contains(&comp));
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
    for row in pkg.select_rows(Select::table("Media")).map_err(io_err("Media"))? {
        media.push(MediaRow {
            disk_id: row["DiskId"].as_int().ok_or("Media without DiskId")?,
            last_sequence: row["LastSequence"].as_int().ok_or("Media without LastSequence")?,
            cabinet: str_col(&row, "Cabinet").filter(|s| !s.is_empty()),
        });
    }
    media.sort_by_key(|m| m.last_sequence);
    Ok(Plan { word_count, files, media })
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
        let n = reader.read(&mut buf).map_err(|e| format!("read {}: {e}", f.key))?;
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
        return Err(format!("{}: extracted {total} bytes, File.FileSize says {}", f.key, f.size));
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

/// How cabinet entries are decompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CabMode {
    /// One decompression pass per CAB folder (header-patch trick, see `cab_for_each_sequential`).
    Sequential,
    /// `Cabinet::read_file` per entry: restarts the folder for every file (O(n^2) per folder).
    PerFile,
}

/// Extract every payload file of `msi` into `target`, laid out as `msiexec /a` lays it out.
/// Returns the written paths in the order they were written.
pub fn extract_msi(msi: &Path, target: &Path) -> Result<Vec<PathBuf>, String> {
    extract_msi_mode(msi, target, CabMode::Sequential)
}

pub fn extract_msi_mode(msi: &Path, target: &Path, mode: CabMode) -> Result<Vec<PathBuf>, String> {
    let file = File::open(msi).map_err(|e| format!("open {}: {e}", msi.display()))?;
    let mut pkg = Package::open(file).map_err(|e| format!("not an MSI {}: {e}", msi.display()))?;
    let plan = plan_msi(&mut pkg)?;
    let msi_dir = msi.parent().unwrap_or(Path::new("."));
    let by_key: HashMap<&str, &PlannedFile> = plan.files.iter().map(|f| (f.key.as_str(), f)).collect();
    let mut done: HashSet<String> = HashSet::new();
    let mut written = Vec::new();

    for m in &plan.media {
        let Some(cab_name) = &m.cabinet else { continue };
        let cab_bytes: Vec<u8> = if let Some(stream) = cab_name.strip_prefix('#') {
            let mut r = pkg.read_stream(stream).map_err(|e| format!("cabinet stream {stream:?}: {e}"))?;
            let mut v = Vec::new();
            r.read_to_end(&mut v).map_err(io_err("read cabinet stream"))?;
            v
        } else {
            check_segment(cab_name, "cabinet")?;
            fs::read(msi_dir.join(cab_name)).map_err(|e| format!("external cabinet {cab_name:?}: {e}"))?
        };
        let mut on_entry = |name: &str, reader: &mut dyn Read| -> Result<(), String> {
            let Some(f) = by_key.get(name) else {
                return Err(format!("cabinet {cab_name:?} holds {name:?}, which is not in the File table"));
            };
            if !f.compressed {
                return Err(format!("{name:?} is in a cabinet but its attributes say uncompressed"));
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
        match mode {
            CabMode::Sequential => cab_for_each_sequential(cab_bytes, &mut on_entry)
                .map_err(|e| format!("cabinet {cab_name:?}: {e}"))?,
            CabMode::PerFile => cab_for_each_per_file(cab_bytes, &mut on_entry)
                .map_err(|e| format!("cabinet {cab_name:?}: {e}"))?,
        }
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
            return Err(format!("{:?} is marked compressed but no cabinet holds it", f.key));
        }
        if !pkg_compressed {
            // Uncompressed-image layout (source tree) is not measured; python.org never uses it.
            return Err(format!("{:?}: uncompressed source image layout is not supported", f.key));
        }
        let src = msi_dir.join(&f.source_name);
        let mut reader = File::open(&src).map_err(|e| format!("uncompressed source {}: {e}", src.display()))?;
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
    let rd16 = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or("cabinet truncated");
    let rd32 = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or("cabinet truncated");
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
            return Err(format!("CFFILE folder index {folder:#x} (continued folder?) not supported"));
        }
        let name_start = pos + 16;
        let nul = b.get(name_start..).and_then(|t| t.iter().position(|&c| c == 0)).ok_or("cabinet truncated")?;
        let raw = &b[name_start..name_start + nul];
        if !raw.is_ascii() {
            return Err("non-ASCII cabinet entry name".into());
        }
        entries.push(CabEntry { name: String::from_utf8_lossy(raw).into_owned(), size, offset, folder, record_pos: pos });
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
        let end = list.iter().map(|e| e.offset as u64 + e.size as u64).max().unwrap_or(0);
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
            io::copy(&mut (&mut reader).take(off - cur), &mut io::sink()).map_err(|e| e.to_string())?;
            if j - i == 1 {
                let mut part = (&mut reader).take(e.size as u64);
                // write_checked() fails on a short read, so the part is consumed fully.
                on_entry(&e.name, &mut part)?;
                // Entries the caller skipped (features at Level 0) must still be read past.
                io::copy(&mut part, &mut io::sink()).map_err(|e| e.to_string())?;
            } else {
                let mut buf = Vec::with_capacity(e.size as usize);
                (&mut reader).take(e.size as u64).read_to_end(&mut buf).map_err(|e| e.to_string())?;
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

/// Reference path: `read_file` per entry (quadratic per folder). Kept for the measurements.
pub fn cab_for_each_per_file(
    bytes: Vec<u8>,
    on_entry: &mut dyn FnMut(&str, &mut dyn Read) -> Result<(), String>,
) -> Result<(), String> {
    let mut cab = cab::Cabinet::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let names: Vec<String> = cab
        .folder_entries()
        .flat_map(|fe| fe.file_entries().map(|e| e.name().to_string()).collect::<Vec<_>>())
        .collect();
    for name in names {
        let mut reader = cab.read_file(&name).map_err(|e| e.to_string())?;
        on_entry(&name, &mut reader)?;
    }
    Ok(())
}
