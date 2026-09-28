//! Path helpers that behave like the shell code upstream uses.

use std::path::{Component, Path, PathBuf};

/// Resolves `.` and `..` by text, the way bash's logical `cd` does.
/// Symlinks are not followed and nothing is checked on disk.
pub fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.file_name().is_some() {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Drops a leading UTF-8 byte order mark (allowlist D-03).
pub fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

/// A comparison key for a Windows path: `.` and `..` resolved by text, `/` unified to `\`,
/// a trailing `\` trimmed, and ASCII-lowercased.
pub fn win_path_key(p: &Path) -> String {
    let normalized = lexical_normalize(p).to_string_lossy().replace('/', "\\");
    normalized.trim_end_matches('\\').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn dot_dot_is_resolved_by_text() {
        assert_eq!(
            lexical_normalize(Path::new("/r/versions/3.12/../3.12.1")),
            PathBuf::from("/r/versions/3.12.1")
        );
        assert_eq!(
            lexical_normalize(Path::new("/r/./versions/.")),
            PathBuf::from("/r/versions")
        );
        assert_eq!(lexical_normalize(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(lexical_normalize(Path::new("../x")), PathBuf::from("../x"));
    }

    #[test]
    fn utf8_bom_is_removed() {
        assert_eq!(strip_bom(b"\xEF\xBB\xBF3.12\n"), b"3.12\n");
        assert_eq!(strip_bom(b"3.12\n"), b"3.12\n");
    }
}
