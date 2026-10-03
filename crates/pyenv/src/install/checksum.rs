//! python-build's `URL#<checksum>` convention, restricted to SHA-256 (spec §9.3, plan Decision 7).

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Splits `url#fragment` at the first `#`, as python-build does.
pub fn split_url(url: &str) -> (&str, Option<&str>) {
    match url.split_once('#') {
        Some((u, f)) => (u, Some(f)),
        None => (url, None),
    }
}

/// The SHA-256 named by a URL's fragment, lowercased. A missing fragment, an MD5 (32 hex
/// digits) or anything else is refused: every download is checked against a SHA-256.
pub fn sha256_of_fragment(url: &str, fragment: Option<&str>) -> Result<String, String> {
    match fragment {
        Some(f) if f.len() == 64 && f.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(f.to_ascii_lowercase())
        }
        _ => Err(format!("rpyenv requires a SHA-256 checksum for {url}")),
    }
}

/// Lowercase hex SHA-256 of a file's bytes.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn the_fragment_starts_after_the_first_hash() {
        assert_eq!(
            split_url("https://x/a.tgz#ab#cd"),
            ("https://x/a.tgz", Some("ab#cd"))
        );
        assert_eq!(split_url("https://x/a.tgz"), ("https://x/a.tgz", None));
    }

    #[test]
    fn a_sha256_fragment_is_lowercased() {
        let upper = ABC.to_ascii_uppercase();
        assert_eq!(sha256_of_fragment("u", Some(&upper)), Ok(ABC.to_string()));
    }

    #[test]
    fn md5_missing_and_malformed_fragments_are_refused() {
        let msg = Err("rpyenv requires a SHA-256 checksum for https://x/a.tgz".to_string());
        assert_eq!(sha256_of_fragment("https://x/a.tgz", None), msg);
        assert_eq!(
            sha256_of_fragment("https://x/a.tgz", Some(&"0".repeat(32))),
            msg
        );
        assert_eq!(
            sha256_of_fragment("https://x/a.tgz", Some(&"g".repeat(64))),
            msg
        );
    }

    #[test]
    fn hashes_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, "abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), ABC);
    }
}
