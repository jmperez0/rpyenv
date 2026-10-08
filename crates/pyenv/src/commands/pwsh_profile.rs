//! The PowerShell profile line (spec §7, §9.4): ``iex ((pyenv init - pwsh) -join "`n")``,
//! in the Windows PowerShell 5.1 and PowerShell 7 profiles under Documents.

use crate::output::Output;
use std::io;
use std::path::{Path, PathBuf};

pub const LINE: &str = "iex ((pyenv init - pwsh) -join \"`n\")";

/// The current-user, current-host profiles of Windows PowerShell 5.1 and PowerShell 7.
pub fn paths(documents: &Path) -> [PathBuf; 2] {
    [
        documents
            .join("WindowsPowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
        documents
            .join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Added,
    /// The profile already mentions pyenv: left alone, as `pyenv init --install` does.
    Mentions,
}

/// How a profile is stored. Windows PowerShell 5.1's `>` and `Out-File` write UTF-16 with a
/// BOM; anything else (UTF-8 with or without a BOM, an ANSI code page) is kept as bytes.
#[derive(Clone, Copy)]
enum Encoding {
    Bytes,
    Utf16Le,
    Utf16Be,
}

/// The file's code units (bytes, or UTF-16 units after the BOM). Nothing is decoded, so
/// what isn't the pyenv line is written back exactly as it was (final review I1).
fn units(bytes: &[u8]) -> (Encoding, Vec<u16>) {
    let pairs = |rest: &[u8], f: fn([u8; 2]) -> u16| {
        rest.chunks(2)
            .map(|c| f([c[0], c.get(1).copied().unwrap_or(0)]))
            .collect()
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => (Encoding::Utf16Le, pairs(rest, u16::from_le_bytes)),
        [0xFE, 0xFF, rest @ ..] => (Encoding::Utf16Be, pairs(rest, u16::from_be_bytes)),
        _ => (Encoding::Bytes, bytes.iter().map(|&b| b.into()).collect()),
    }
}

fn to_bytes(encoding: Encoding, units: &[u16]) -> Vec<u8> {
    match encoding {
        Encoding::Bytes => units.iter().map(|&u| u as u8).collect(),
        Encoding::Utf16Le => [0xFF, 0xFE]
            .into_iter()
            .chain(units.iter().flat_map(|u| u.to_le_bytes()))
            .collect(),
        Encoding::Utf16Be => [0xFE, 0xFF]
            .into_iter()
            .chain(units.iter().flat_map(|u| u.to_be_bytes()))
            .collect(),
    }
}

fn ascii(s: &str) -> Vec<u16> {
    s.bytes().map(u16::from).collect()
}

/// `LINE` without its line end.
fn is_line(line: &[u16]) -> bool {
    let mut end = line.len();
    while end > 0 && (line[end - 1] == u16::from(b'\n') || line[end - 1] == u16::from(b'\r')) {
        end -= 1;
    }
    line[..end] == ascii(LINE)[..]
}

fn read_units(file: &Path) -> io::Result<Option<(Encoding, Vec<u16>)>> {
    match std::fs::read(file) {
        Ok(b) => Ok(Some(units(&b))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Appends `LINE` to `file` (created with its folder if missing), unless it already
/// mentions pyenv in any case.
pub fn add(file: &Path) -> io::Result<Added> {
    let (encoding, mut text) = read_units(file)?.unwrap_or((Encoding::Bytes, Vec::new()));
    let lower = |u: &u16| if (65..=90).contains(u) { u + 32 } else { *u };
    let pyenv = ascii("pyenv");
    if text
        .windows(5)
        .any(|w| w.iter().map(lower).eq(pyenv.iter().copied()))
    {
        return Ok(Added::Mentions);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if text.last().is_some_and(|&u| u != u16::from(b'\n')) {
        text.extend(ascii("\r\n"));
    }
    text.extend(ascii(LINE));
    text.extend(ascii("\r\n"));
    std::fs::write(file, to_bytes(encoding, &text))?;
    Ok(Added::Added)
}

/// True when `file` has a line that is exactly `LINE`, whoever added it.
pub fn has_line(file: &Path) -> io::Result<bool> {
    Ok(read_units(file)?.is_some_and(|(_, text)| {
        text.split_inclusive(|&u| u == u16::from(b'\n'))
            .any(is_line)
    }))
}

/// Removes every line that is exactly `LINE`; true when there was one.
pub fn remove(file: &Path) -> io::Result<bool> {
    let Some((encoding, text)) = read_units(file)? else {
        return Ok(false);
    };
    let mut removed = false;
    let mut out = Vec::new();
    for line in text.split_inclusive(|&u| u == u16::from(b'\n')) {
        if is_line(line) {
            removed = true;
        } else {
            out.extend_from_slice(line);
        }
    }
    if removed {
        std::fs::write(file, to_bytes(encoding, &out))?;
    }
    Ok(removed)
}

/// Adds the line to both profiles and reports each (`init --install pwsh`, `setup`).
pub fn install_all() -> Output {
    let mut o = Output::new();
    let Some(docs) = rpyenv_core::winenv::documents() else {
        return Output::error(
            "pyenv: cannot find your Documents folder for the PowerShell profiles",
        );
    };
    for p in paths(&docs) {
        match add(&p) {
            Ok(Added::Added) => o.out(format!(
                "pyenv: added the PowerShell line to {}",
                p.display()
            )),
            Ok(Added::Mentions) => o.out(format!(
                "pyenv: {} already mentions pyenv; left as it is",
                p.display()
            )),
            Err(e) => o.err(format!(
                "pyenv: {}: {}",
                p.display(),
                rpyenv_core::launch::io_reason(&e)
            )),
        }
    }
    if !o.stderr.is_empty() {
        o.code = 1;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_profiles_under_documents() {
        let [p5, p7] = paths(Path::new(r"D:\Docs"));
        assert_eq!(
            p5,
            Path::new(r"D:\Docs")
                .join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1")
        );
        assert_eq!(
            p7,
            Path::new(r"D:\Docs")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1")
        );
    }

    #[test]
    fn add_creates_appends_once_and_leaves_a_pyenv_profile_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let new = tmp.path().join("a").join("p.ps1");
        assert_eq!(add(&new).unwrap(), Added::Added);
        assert_eq!(
            std::fs::read_to_string(&new).unwrap(),
            format!("{LINE}\r\n")
        );
        assert_eq!(
            add(&new).unwrap(),
            Added::Mentions,
            "already mentions pyenv"
        );
        let other = tmp.path().join("o.ps1");
        std::fs::write(&other, "Set-Alias ll ls").unwrap();
        assert_eq!(add(&other).unwrap(), Added::Added);
        assert_eq!(
            std::fs::read_to_string(&other).unwrap(),
            format!("Set-Alias ll ls\r\n{LINE}\r\n")
        );
        let mine = tmp.path().join("m.ps1");
        std::fs::write(&mine, "# my PYENV setup\r\n").unwrap();
        assert_eq!(add(&mine).unwrap(), Added::Mentions);
        assert_eq!(
            std::fs::read_to_string(&mine).unwrap(),
            "# my PYENV setup\r\n"
        );
    }

    fn utf16(bom: [u8; 2], text: &str) -> Vec<u8> {
        let mut out = bom.to_vec();
        for u in text.encode_utf16() {
            out.extend(if bom == [0xFF, 0xFE] {
                u.to_le_bytes()
            } else {
                u.to_be_bytes()
            });
        }
        out
    }

    /// Final review I1: Windows PowerShell 5.1's `>` and `Out-File` write UTF-16 with a BOM;
    /// the line goes in, and comes out, in the file's own encoding.
    #[test]
    fn a_utf16_or_bom_profile_keeps_its_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("p.ps1");
        for bom in [[0xFF, 0xFE], [0xFE, 0xFF]] {
            std::fs::write(&f, utf16(bom, "Set-Alias ll ls ñ\r\n")).unwrap();
            assert_eq!(add(&f).unwrap(), Added::Added);
            assert_eq!(
                std::fs::read(&f).unwrap(),
                utf16(bom, &format!("Set-Alias ll ls ñ\r\n{LINE}\r\n"))
            );
            assert_eq!(add(&f).unwrap(), Added::Mentions);
            assert!(remove(&f).unwrap());
            assert_eq!(
                std::fs::read(&f).unwrap(),
                utf16(bom, "Set-Alias ll ls ñ\r\n")
            );
            std::fs::write(&f, utf16(bom, "# my PyEnv notes\r\n")).unwrap();
            assert_eq!(add(&f).unwrap(), Added::Mentions);
        }
        // UTF-8 with a BOM, and a byte that isn't UTF-8 (an ANSI profile): kept as they are.
        let mut ansi = b"\xEF\xBB\xBFa \xF1".to_vec();
        std::fs::write(&f, &ansi).unwrap();
        assert_eq!(add(&f).unwrap(), Added::Added);
        assert!(remove(&f).unwrap());
        ansi.extend_from_slice(b"\r\n");
        assert_eq!(std::fs::read(&f).unwrap(), ansi);
    }

    #[test]
    fn remove_takes_out_only_the_exact_line() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("p.ps1");
        std::fs::write(&f, format!("a\r\n{LINE}\r\n# pyenv notes\r\nb\r\n")).unwrap();
        assert!(remove(&f).unwrap());
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "a\r\n# pyenv notes\r\nb\r\n"
        );
        assert!(!remove(&f).unwrap());
        assert!(!remove(&tmp.path().join("missing.ps1")).unwrap());
    }
}
