//! Encoding text the way `cmd.exe`'s `for /f` will decode it (spec §5.3,
//! `RPYENV_FORWARD_CP`): a piped child's stdout is decoded with the console's active
//! output code page, not UTF-8, so a `.cmd` forwarder that captures a path through `for
//! /f` needs that path written in that code page, with a clear failure when the code page
//! can't represent it (rather than cmd silently substituting the wrong bytes).

use windows_sys::Win32::Globalization::{MultiByteToWideChar, WideCharToMultiByte, CP_UTF8};
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
    // SAFETY: `GetConsoleOutputCP` takes no arguments; it has no documented failure.
    let cp = unsafe { GetConsoleOutputCP() };
    if cp == CP_UTF8 {
        return Ok(text.as_bytes().to_vec());
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Ok(Vec::new());
    }
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
        return Err(cp);
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
        return Err(cp);
    }
    buf.truncate(written as usize);
    if decode_matches(cp, &buf, &wide) {
        Ok(buf)
    } else {
        Err(cp)
    }
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
}
