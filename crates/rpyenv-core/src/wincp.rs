//! Encoding text for the console's code page (spec §4, §5.3): rpyenv's redirected output is
//! written in it, as cmd.exe and pyenv-win write theirs, and a `.cmd` forwarder's `for /f`
//! decodes with it (`RPYENV_FORWARD_CP`): a piped child's stdout is decoded with the console's
//! active output code page, not UTF-8, so a `.cmd` forwarder that captures a path through `for
//! /f` needs that path written in that code page, with a clear failure when the code page
//! can't represent it (rather than cmd silently substituting the wrong bytes).

use windows_sys::Win32::Globalization::{
    GetOEMCP, MultiByteToWideChar, WideCharToMultiByte, CP_UTF8,
};
use windows_sys::Win32::System::Console::GetConsoleOutputCP;

/// Encodes `text` in the console's active output code page (`GetConsoleOutputCP`; with no
/// console at all, this is 0 — `CP_ACP`, the system's default ANSI page, not the OEM one).
/// `Err(cp)` when the round trip through that code page loses information.
///
/// `WC_NO_BEST_FIT_CHARS` would normally catch a lossy conversion directly, but
/// `WideCharToMultiByte` rejects it outright (`ERROR_INVALID_FLAGS`) for some code pages —
/// GB18030 (54936) and UTF-7 (65000) among them — even for plain ASCII text. Flags `0` is
/// accepted everywhere, so loss is instead detected by decoding the result back
/// (`MultiByteToWideChar`) and comparing it to the original UTF-16: a best-fit or "?"
/// substitution decodes back to a different character, so a mismatch means the code page
/// couldn't represent the text losslessly. `CP_UTF8` skips both calls, since UTF-8
/// represents any Rust `str` exactly.
pub fn encode_for_console(text: &str) -> Result<Vec<u8>, u32> {
    let cp = output_cp();
    if cp == CP_UTF8 {
        return Ok(text.as_bytes().to_vec());
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Ok(Vec::new());
    }
    match to_code_page(cp, &wide) {
        Some(buf) if decode_matches(cp, &buf, &wide) => Ok(buf),
        _ => Err(cp),
    }
}

/// The console's active output code page; 0 (`CP_ACP`, the ANSI code page) when this
/// process has no console.
pub fn output_cp() -> u32 {
    // SAFETY: `GetConsoleOutputCP` takes no arguments; it has no documented failure.
    unsafe { GetConsoleOutputCP() }
}

/// The OEM code page: what a console created for a child starts with.
pub fn oem_cp() -> u32 {
    // SAFETY: `GetOEMCP` takes no arguments and has no failure.
    unsafe { GetOEMCP() }
}

/// `text` for a pipe or a file. It is in code page `cp` when every character survives the
/// round trip, as cmd.exe and pyenv-win write it. Otherwise the whole text is UTF-8: a
/// reader decoding UTF-8 still gets it right, and no character becomes `?` (allowlist
/// D-48). Windows refusing the code page also gives UTF-8. Never fails.
pub fn encode_for_output(text: &str, cp: u32) -> Vec<u8> {
    if cp == CP_UTF8 {
        return text.as_bytes().to_vec();
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Vec::new();
    }
    match to_code_page(cp, &wide) {
        Some(bytes) if decode_matches(cp, &bytes, &wide) => bytes,
        _ => text.as_bytes().to_vec(),
    }
}

/// `bytes` decoded from code page `cp`; invalid sequences become U+FFFD.
pub fn decode(bytes: &[u8], cp: u32) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    // SAFETY: `bytes` is a valid buffer of `bytes.len()` bytes; a null output buffer and 0
    // length ask for the required size only.
    let needed = unsafe {
        MultiByteToWideChar(
            cp,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    // SAFETY: `wide` is a writable buffer of `needed` `u16`s, matching `cchWideChar`;
    // `bytes` is unchanged from the sizing call.
    let written = unsafe {
        MultiByteToWideChar(
            cp,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide.as_mut_ptr(),
            needed,
        )
    };
    String::from_utf16_lossy(&wide[..written.max(0) as usize])
}

/// `wide` converted to code page `cp` with flags 0 (default-character substitution), or
/// `None` when Windows refuses. Both encoders check the result with `decode_matches`.
fn to_code_page(cp: u32, wide: &[u16]) -> Option<Vec<u8>> {
    // SAFETY: `wide` is a valid UTF-16 buffer of `wide.len()` elements; a null output
    // buffer and 0 length ask for the required size only, per `WideCharToMultiByte`'s
    // documented two-call sizing pattern.
    let needed = unsafe {
        WideCharToMultiByte(
            cp,
            0,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if needed <= 0 {
        return None;
    }
    let mut buf = vec![0u8; needed as usize];
    // SAFETY: `buf` is a valid, writable buffer of `needed` bytes, matching `cbMultiByte`;
    // `wide` and its length are unchanged from the sizing call above.
    let written = unsafe {
        WideCharToMultiByte(
            cp,
            0,
            wide.as_ptr(),
            wide.len() as i32,
            buf.as_mut_ptr(),
            buf.len() as i32,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if written <= 0 {
        return None;
    }
    buf.truncate(written as usize);
    Some(buf)
}

/// True when decoding `bytes` back through code page `cp` reproduces `original` exactly.
fn decode_matches(cp: u32, bytes: &[u8], original: &[u16]) -> bool {
    // SAFETY: `bytes` is a valid buffer of `bytes.len()` bytes; a null output buffer and 0
    // length ask for the required size only, per `MultiByteToWideChar`'s documented
    // two-call sizing pattern.
    let needed = unsafe {
        MultiByteToWideChar(
            cp,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 || needed as usize != original.len() {
        return false;
    }
    let mut back = vec![0u16; needed as usize];
    // SAFETY: `back` is a valid, writable buffer of `needed` `u16`s, matching
    // `cchWideChar`; `bytes` and its length are unchanged from the sizing call above.
    let written = unsafe {
        MultiByteToWideChar(
            cp,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            back.as_mut_ptr(),
            back.len() as i32,
        )
    };
    written as usize == original.len() && back == original
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The active console code page can't be chosen from a test, so this only checks that
    /// ASCII text round-trips in whatever code page this host has (every single- and
    /// double-byte Windows code page, and UTF-8/UTF-7/GB18030, represent ASCII exactly).
    #[test]
    fn ascii_round_trips_in_any_code_page() {
        let bytes = encode_for_console("plain-name.exe").unwrap();
        assert_eq!(bytes, b"plain-name.exe");
    }

    /// Review focus 4. Explicit code pages, so the result doesn't depend on the host.
    #[test]
    fn encode_for_output_keeps_what_the_code_page_lacks() {
        assert_eq!(encode_for_output("José ñ", 850), b"Jos\x82 \xa4");
        // A character code page 850 lacks: the whole text in UTF-8, not just that character.
        assert_eq!(encode_for_output("a漢b", 850), "a漢b".as_bytes());
        assert_eq!(encode_for_output("José 漢", 850), "José 漢".as_bytes());
        assert_eq!(encode_for_output("José", 1252), b"Jos\xe9");
        assert_eq!(encode_for_output("José 漢", CP_UTF8), "José 漢".as_bytes());
        assert_eq!(encode_for_output("", 850), b"");
    }

    #[test]
    fn decode_reads_a_code_page() {
        assert_eq!(decode(b"Jos\x82 \xa4", 850), "José ñ");
        assert_eq!(decode("José".as_bytes(), CP_UTF8), "José");
        assert_eq!(decode(b"", 850), "");
    }
}
