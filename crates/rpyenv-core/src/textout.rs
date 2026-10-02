//! Where rpyenv's own text reaches stdout and stderr. On Windows, text going to a pipe, a
//! file or NUL is encoded in the console's output code page, as cmd.exe and pyenv-win write
//! theirs (spec §4, §11); text going to a console is handed to std, which writes it as
//! Unicode. On Linux the text's UTF-8 bytes are written as they are.

use std::borrow::Cow;
use std::io::Write;

/// Writes `text` to stderr (`to_stderr`) or stdout, flushing stdout. Errors are ignored:
/// there is nowhere left to report them.
pub fn write(to_stderr: bool, text: &str) {
    let bytes = encode(to_stderr, text);
    if to_stderr {
        let _ = std::io::stderr().write_all(&bytes);
    } else {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&bytes);
        let _ = out.flush();
    }
}

#[cfg(windows)]
fn encode(to_stderr: bool, text: &str) -> Cow<'_, [u8]> {
    if crate::winproc::std_is_console(to_stderr) {
        Cow::Borrowed(text.as_bytes())
    } else {
        Cow::Owned(crate::wincp::encode_for_output(
            text,
            crate::wincp::output_cp(),
        ))
    }
}

#[cfg(not(windows))]
fn encode(_to_stderr: bool, text: &str) -> Cow<'_, [u8]> {
    Cow::Borrowed(text.as_bytes())
}
