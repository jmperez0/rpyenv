//! A Windows command line, handled as the C runtime parses it, without re-quoting
//! (spec §5.3). The pure functions run on every OS so their tests do too.

const SPACE: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;

fn is_blank(c: u16) -> bool {
    c == SPACE || c == TAB
}

/// What follows the first `n` arguments of `cmdline`, starting with the whitespace after
/// the `n`th, or empty. The program name and later arguments follow the C runtime's
/// rules (the module doc lists them).
pub fn after_args(cmdline: &[u16], n: usize) -> &[u16] {
    let len = cmdline.len();
    if n == 0 {
        return cmdline;
    }
    let mut i = 0;
    if cmdline.first() == Some(&QUOTE) {
        i = 1;
        while i < len && cmdline[i] != QUOTE {
            i += 1;
        }
        if i < len {
            i += 1;
        }
    } else {
        while i < len && !is_blank(cmdline[i]) {
            i += 1;
        }
    }
    for _ in 1..n {
        while i < len && is_blank(cmdline[i]) {
            i += 1;
        }
        if i >= len {
            break;
        }
        let mut quoted = false;
        while i < len {
            let c = cmdline[i];
            if c == BACKSLASH {
                let start = i;
                while i < len && cmdline[i] == BACKSLASH {
                    i += 1;
                }
                // An odd run escapes the quote after it; an even run leaves the quote to
                // the next iteration.
                if i < len && cmdline[i] == QUOTE && (i - start) % 2 == 1 {
                    i += 1;
                }
                continue;
            }
            if c == QUOTE {
                if quoted && i + 1 < len && cmdline[i + 1] == QUOTE {
                    i += 2;
                    continue;
                }
                quoted = !quoted;
                i += 1;
                continue;
            }
            if is_blank(c) && !quoted {
                break;
            }
            i += 1;
        }
    }
    &cmdline[i..]
}

/// `s` without leading spaces and tabs.
pub fn trim_blanks(s: &[u16]) -> &[u16] {
    let start = s.iter().position(|&c| !is_blank(c)).unwrap_or(s.len());
    &s[start..]
}

/// This process's command line after its first `n` arguments, without leading blanks.
/// `None` when nothing follows.
#[cfg(windows)]
pub fn own_tail(n: usize) -> Option<std::ffi::OsString> {
    use std::os::windows::ffi::OsStringExt;
    // SAFETY: GetCommandLineW returns this process's command line, NUL-terminated and
    // valid for the life of the process; it is only read here.
    let all = unsafe {
        let p = windows_sys::Win32::System::Environment::GetCommandLineW();
        if p.is_null() {
            return None;
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        std::slice::from_raw_parts(p, len)
    };
    let rest = trim_blanks(after_args(all, n));
    (!rest.is_empty()).then(|| std::ffi::OsString::from_wide(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn after(s: &str, n: usize) -> String {
        String::from_utf16(after_args(&w(s), n)).unwrap()
    }

    #[test]
    fn program_name_rules() {
        assert_eq!(after(r#"python.exe a b"#, 1), " a b");
        assert_eq!(after(r#""C:\p q\python.exe" "x y""#, 1), r#" "x y""#);
        assert_eq!(after(r#""C:\p q\python.exe"arg"#, 1), "arg");
        // argv[0] has no escapes: the backslash doesn't protect the quote.
        assert_eq!(after(r#""C:\dir\"x y"#, 1), "x y");
        assert_eq!(after("python.exe\ta", 1), "\ta");
        assert_eq!(after("python.exe", 1), "");
        assert_eq!(after("", 1), "");
    }

    #[test]
    fn later_argument_rules() {
        assert_eq!(
            after(r#"pyenv exec python "a^b" 100%"#, 3),
            r#" "a^b" 100%"#
        );
        assert_eq!(after(r#"pyenv "ex ec" python x"#, 3), " x");
        // \" is a literal quote, so the argument doesn't end at the space inside.
        assert_eq!(after(r#"p a\"b c d"#, 2), " c d");
        // \\" is one backslash, then a quote that opens quoting.
        assert_eq!(after(r#"p a\\"b c" d"#, 2), " d");
        // "" inside quotes is a literal quote, and quoting continues.
        assert_eq!(after(r#"p "a""b c" d"#, 2), " d");
        assert_eq!(after("p  a   b", 2), "   b");
        assert_eq!(after("p a", 3), "");
    }

    #[test]
    fn blanks_are_trimmed_from_the_front_only() {
        assert_eq!(trim_blanks(&w(" \t a b ")), w("a b ").as_slice());
    }
}
