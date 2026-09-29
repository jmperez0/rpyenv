//! The PE `Subsystem` field: whether an `.exe` is a GUI or a console program (spec §8;
//! docs/windows-lazy-console.md, rule 1).

use std::io::Read;
use std::path::Path;

pub const SUBSYSTEM_WINDOWS_GUI: u16 = 2;
pub const SUBSYSTEM_WINDOWS_CUI: u16 = 3;

/// The `Subsystem` of a PE image, from its first bytes. None when they aren't a PE image.
pub fn subsystem(bytes: &[u8]) -> Option<u16> {
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let at = u32::from_le_bytes(bytes.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if bytes.get(at..at.checked_add(4)?)? != b"PE\0\0" {
        return None;
    }
    // The optional header follows the 4-byte signature and the 20-byte file header;
    // Subsystem is at offset 68 in it, in both PE32 and PE32+.
    let field = at.checked_add(4 + 20 + 68)?;
    Some(u16::from_le_bytes(
        bytes.get(field..field.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// True for a GUI-subsystem executable. Reads at most the first 64 KiB.
pub fn is_gui(path: &Path) -> bool {
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(65536).read_to_end(&mut head));
    read.is_ok() && subsystem(&head) == Some(SUBSYSTEM_WINDOWS_GUI)
}

/// A minimal PE header with the given subsystem, for tests.
#[cfg(test)]
pub(crate) fn image(subsystem: u16) -> Vec<u8> {
    let mut b = vec![0u8; 0x200];
    b[..2].copy_from_slice(b"MZ");
    b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    let field = 0x80 + 4 + 20 + 68;
    b[field..field + 2].copy_from_slice(&subsystem.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_subsystem() {
        assert_eq!(subsystem(&image(SUBSYSTEM_WINDOWS_GUI)), Some(2));
        assert_eq!(subsystem(&image(SUBSYSTEM_WINDOWS_CUI)), Some(3));
    }

    #[test]
    fn not_a_pe_image() {
        assert_eq!(subsystem(b"#!/bin/sh\n"), None);
        assert_eq!(subsystem(&image(2)[..0x90]), None);
        let mut bad = image(2);
        bad[0x80] = b'X';
        assert_eq!(subsystem(&bad), None);
    }

    #[test]
    fn is_gui_reads_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (gui, cui) = (tmp.path().join("w.exe"), tmp.path().join("c.exe"));
        std::fs::write(&gui, image(2)).unwrap();
        std::fs::write(&cui, image(3)).unwrap();
        assert!(is_gui(&gui));
        assert!(!is_gui(&cui));
        assert!(!is_gui(&tmp.path().join("missing.exe")));
    }
}
