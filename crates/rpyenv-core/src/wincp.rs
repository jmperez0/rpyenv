//! Encoding text the way `cmd.exe`'s `for /f` will decode it (spec §5.3,
//! `RPYENV_FORWARD_CP`): a piped child's stdout is decoded with the console's active
//! output code page, not UTF-8, so a `.cmd` forwarder that captures a path through `for
//! /f` needs that path written in that code page, with a clear failure when the code page
//! can't represent it (rather than cmd silently substituting the wrong bytes).

use windows_sys::Win32::Globalization::{WideCharToMultiByte, CP_UTF8, WC_NO_BEST_FIT_CHARS};
use windows_sys::Win32::System::Console::GetConsoleOutputCP;

/// Encodes `text` in the console's active output code page (`GetConsoleOutputCP`).
/// `Err(cp)` when some character isn't representable in code page `cp`: with
/// `WC_NO_BEST_FIT_CHARS`, `WideCharToMultiByte` reports that through
/// `lpUsedDefaultChar` instead of silently substituting a placeholder byte.
///
/// `CP_UTF8` (and `CP_UTF7`) can represent any text, and `WideCharToMultiByte` refuses
/// `lpDefaultChar`/`lpUsedDefaultChar` for them (`ERROR_INVALID_PARAMETER`); `CP_UTF8` is
/// encoded directly, and the rare `CP_UTF7` console falls through to the general path
/// below without the used-default check, so it can only fail on the conversion itself.
pub fn encode_for_console(text: &str) -> Result<Vec<u8>, u32> {
    // SAFETY: `GetConsoleOutputCP` takes no arguments; it has no documented failure (a
    // process with no console gets the system's OEM code page).
    let cp = unsafe { GetConsoleOutputCP() };
    if cp == CP_UTF8 {
        return Ok(text.as_bytes().to_vec());
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return Ok(Vec::new());
    }
    let check_default = cp != windows_sys::Win32::Globalization::CP_UTF7;
    let mut used_default: i32 = 0;
    let used_default_ptr = if check_default {
        std::ptr::addr_of_mut!(used_default)
    } else {
        std::ptr::null_mut()
    };
    // SAFETY: `wide` is a valid UTF-16 buffer of `wide.len()` elements; a null output
    // buffer and 0 length ask for the required size only, per `WideCharToMultiByte`'s
    // documented two-call sizing pattern. `used_default_ptr` is either null or a valid
    // `*mut i32` for the duration of this call.
    let needed = unsafe {
        WideCharToMultiByte(
            cp,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            used_default_ptr,
        )
    };
    if needed <= 0 {
        return Err(cp);
    }
    let mut buf = vec![0u8; needed as usize];
    used_default = 0;
    // SAFETY: `buf` is a valid, writable buffer of `needed` bytes, matching `cbMultiByte`;
    // `wide` and its length are unchanged from the sizing call above.
    let written = unsafe {
        WideCharToMultiByte(
            cp,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            wide.len() as i32,
            buf.as_mut_ptr(),
            buf.len() as i32,
            std::ptr::null(),
            used_default_ptr,
        )
    };
    if written <= 0 || (check_default && used_default != 0) {
        return Err(cp);
    }
    buf.truncate(written as usize);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The active console code page can't be chosen from a test, so this only checks that
    /// ASCII text round-trips in whatever code page this host has (every single- and
    /// double-byte Windows code page, and UTF-8/UTF-7, represent ASCII exactly).
    #[test]
    fn ascii_round_trips_in_any_code_page() {
        let bytes = encode_for_console("plain-name.exe").unwrap();
        assert_eq!(bytes, b"plain-name.exe");
    }
}
