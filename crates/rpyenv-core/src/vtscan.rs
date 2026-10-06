//! The pure parts of LAZY mode (docs/windows-lazy-console.md, LAZY steps 3 and 5): finding
//! the first printable character in a pseudo-console's VT output, and encoding key
//! presses for its input. Pure, so the tests run on every OS.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Str,
    StrEsc,
}

/// Scans VT output for the first printable character, across reads. It also tracks
/// whether the pseudo-console asked for win32-input-mode (`CSI ? 9001 h` / `l`).
#[derive(Debug, Clone)]
pub struct Scanner {
    state: State,
    params: Vec<u8>,
    win32_input: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner::new()
    }
}

impl Scanner {
    pub fn new() -> Scanner {
        Scanner {
            state: State::Ground,
            params: Vec::new(),
            win32_input: false,
        }
    }

    /// Feeds one read's bytes. Returns the index of the first printable character outside
    /// any escape sequence. Space, C0 controls and DEL don't count; any byte from 0x80 up
    /// does (UTF-8). The whole chunk is always scanned, so the mode stays current.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<usize> {
        let mut first = None;
        for (i, &b) in bytes.iter().enumerate() {
            if self.step(b) && first.is_none() {
                first = Some(i);
            }
        }
        first
    }

    /// Whether the last `?9001` mode change turned win32-input-mode on.
    pub fn win32_input(&self) -> bool {
        self.win32_input
    }

    fn step(&mut self, b: u8) -> bool {
        match self.state {
            State::Ground => match b {
                0x1b => {
                    self.state = State::Esc;
                    false
                }
                0x00..=0x20 | 0x7f => false,
                _ => true,
            },
            State::Esc => {
                self.state = match b {
                    b'[' => {
                        self.params.clear();
                        State::Csi
                    }
                    b']' => State::Osc,
                    b'P' | b'X' | b'^' | b'_' => State::Str,
                    // ESC itself, or an intermediate (as in `ESC ( B`): still in the escape.
                    0x1b | 0x20..=0x2f => State::Esc,
                    _ => State::Ground,
                };
                false
            }
            State::Csi => {
                match b {
                    0x30..=0x3f => self.params.push(b),
                    0x40..=0x7e => {
                        self.csi_final(b);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Esc,
                    _ => {}
                }
                false
            }
            State::Osc => {
                match b {
                    0x07 => self.state = State::Ground,
                    0x1b => self.state = State::OscEsc,
                    _ => {}
                }
                false
            }
            State::Str => {
                if b == 0x1b {
                    self.state = State::StrEsc;
                }
                false
            }
            State::OscEsc | State::StrEsc => {
                if b == b'\\' {
                    self.state = State::Ground;
                    false
                } else {
                    // An ESC that isn't the terminator starts a new sequence.
                    self.state = State::Esc;
                    self.step(b)
                }
            }
        }
    }

    fn csi_final(&mut self, last: u8) {
        if (last == b'h' || last == b'l')
            && self.params.first() == Some(&b'?')
            && self.params[1..].split(|&c| c == b';').any(|p| p == b"9001")
        {
            self.win32_input = last == b'h';
        }
    }
}

/// One key event in win32-input-mode: `ESC [ Vk ; Sc ; Uc ; Kd ; Cs ; Rc _`.
pub fn win32_key(vk: u16, sc: u16, uc: u16, down: bool, state: u32, repeat: u16) -> String {
    format!("\x1b[{vk};{sc};{uc};{};{state};{repeat}_", u8::from(down))
}

/// The bytes one key event sends to the pseudo-console: a win32-input-mode sequence while
/// that mode is on (key-ups and Ctrl+C included, which ConPTY turns into Ctrl+C for the
/// program), else the key-down's characters (Ctrl+C as the byte 0x03).
#[allow(clippy::too_many_arguments)]
pub fn key_bytes(
    vk: u16,
    sc: u16,
    uc: u16,
    down: bool,
    state: u32,
    repeat: u16,
    win32: bool,
    keys: &mut VtKeys,
    out: &mut Vec<u8>,
) {
    if win32 {
        out.extend_from_slice(win32_key(vk, sc, uc, down, state, repeat).as_bytes());
    } else if down {
        keys.push(uc, repeat, out);
    }
}

/// Turns key-down characters (UTF-16 units, as `KEY_EVENT_RECORD` carries them) into
/// UTF-8, joining surrogate pairs that arrive as two records.
#[derive(Debug, Default)]
pub struct VtKeys {
    high: Option<u16>,
}

impl VtKeys {
    pub fn push(&mut self, unit: u16, repeat: u16, out: &mut Vec<u8>) {
        if unit == 0 {
            return;
        }
        if (0xD800..0xDC00).contains(&unit) {
            self.high = Some(unit);
            return;
        }
        let s = match self.high.take() {
            Some(h) if (0xDC00..0xE000).contains(&unit) => String::from_utf16_lossy(&[h, unit]),
            _ => String::from_utf16_lossy(&[unit]),
        };
        for _ in 0..repeat.max(1) {
            out.extend_from_slice(s.as_bytes());
        }
    }
}

/// The length of `b`'s longest prefix that doesn't end inside a UTF-8 sequence: what can
/// be written now, the rest waiting for the next read.
pub fn complete_utf8_len(b: &[u8]) -> usize {
    let len = b.len();
    for back in 1..=len.min(4) {
        let i = len - back;
        let c = b[i];
        if c & 0xC0 == 0x80 {
            continue;
        }
        let need = match c {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        return if i + need > len { i } else { len };
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ConPTY's startup output, captured on build 26200 (plan M5a, probe 4).
    const STARTUP: &[u8] = b"\x1b[?9001h\x1b[?1004h";
    const HELLO: &[u8] =
        b"\x1b[?25l\x1b[2J\x1b[m\x1b[Hhello\r\n\x1b]0;C:\\tmp\\m5probe\\target\\release\\plain.exe\x07\x1b[?25h";
    const SILENT_END: &[u8] =
        b"\x1b[?25l\x1b[?9001l\x1b[?1004l\x1b[2J\x1b[m\x1b[H\x1b]0;C:\\Windows\\System32\\cmd.exe\x07\x1b[?25h";

    #[test]
    fn conpty_startup_is_not_a_trigger_and_turns_win32_input_on() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(STARTUP), None);
        assert!(s.win32_input());
    }

    #[test]
    fn the_first_printable_character_is_the_trigger() {
        let mut s = Scanner::new();
        s.feed(STARTUP);
        assert_eq!(s.feed(HELLO), Some(16));
    }

    /// Review focus 2: escapes, a title and a reset only: never a trigger.
    #[test]
    fn a_silent_program_never_triggers_and_turns_win32_input_off() {
        let mut s = Scanner::new();
        s.feed(STARTUP);
        assert_eq!(s.feed(SILENT_END), None);
        assert!(!s.win32_input());
    }

    #[test]
    fn sequences_split_across_reads() {
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b]0;tit"), None);
        assert_eq!(s.feed(b"le\x07 x"), Some(4));
        let mut s = Scanner::new();
        assert_eq!(s.feed(b"\x1b["), None);
        assert_eq!(s.feed(b"?9001"), None);
        assert_eq!(s.feed(b"h"), None);
        assert!(s.win32_input());
    }

    #[test]
    fn string_sequences_and_charset_escapes_are_skipped() {
        assert_eq!(Scanner::new().feed(b"\x1bP1$r0m\x1b\\Z"), Some(9));
        assert_eq!(Scanner::new().feed(b"\x1b]2;t\x1b\\Q"), Some(7));
        assert_eq!(Scanner::new().feed(b"\x1b(BA"), Some(3));
        assert_eq!(Scanner::new().feed(b"\x1b_apc\x1b\\"), None);
    }

    #[test]
    fn whitespace_and_controls_are_not_triggers_but_utf8_is() {
        assert_eq!(Scanner::new().feed(b" \t\r\n\x08\x07\x7f"), None);
        assert_eq!(Scanner::new().feed("ñ".as_bytes()), Some(0));
        assert_eq!(Scanner::new().feed(b"\x1b[31m!"), Some(5));
    }

    #[test]
    fn win32_input_mode_key_sequences() {
        // Ctrl+C down: VK_C, scan 46, U+0003, left Ctrl.
        assert_eq!(
            win32_key(0x43, 46, 3, true, 0x0008, 1),
            "\x1b[67;46;3;1;8;1_"
        );
        assert_eq!(win32_key(0x41, 30, 97, false, 0, 1), "\x1b[65;30;97;0;0;1_");
    }

    #[test]
    fn key_events_by_input_mode() {
        let mut keys = VtKeys::default();
        let mut out = Vec::new();
        key_bytes(0x43, 46, 3, true, 0x0008, 1, true, &mut keys, &mut out);
        key_bytes(0x43, 46, 3, false, 0x0008, 1, true, &mut keys, &mut out);
        assert_eq!(out, b"\x1b[67;46;3;1;8;1_\x1b[67;46;3;0;8;1_");
        out.clear();
        key_bytes(0x43, 46, 3, true, 0x0008, 1, false, &mut keys, &mut out);
        assert_eq!(out, b"\x03");
        out.clear();
        key_bytes(0x41, 30, 97, true, 0, 1, true, &mut keys, &mut out);
        key_bytes(0x41, 30, 97, false, 0, 1, true, &mut keys, &mut out);
        assert_eq!(out, b"\x1b[65;30;97;1;0;1_\x1b[65;30;97;0;0;1_");
        out.clear();
        key_bytes(0x41, 30, 97, true, 0, 1, false, &mut keys, &mut out);
        key_bytes(0x41, 30, 97, false, 0, 1, false, &mut keys, &mut out);
        assert_eq!(out, b"a");
    }

    #[test]
    fn vt_keys_join_surrogates_and_repeat() {
        let mut k = VtKeys::default();
        let mut out = Vec::new();
        k.push(u16::from(b'a'), 2, &mut out);
        k.push(0, 1, &mut out);
        let units: Vec<u16> = "😀".encode_utf16().collect();
        k.push(units[0], 1, &mut out);
        k.push(units[1], 1, &mut out);
        assert_eq!(out, "aa😀".as_bytes());
    }

    /// Review focus 3: a character split across reads is held until it's whole.
    #[test]
    fn complete_utf8_prefix() {
        assert_eq!(complete_utf8_len(b"ab"), 2);
        assert_eq!(complete_utf8_len(b""), 0);
        assert_eq!(complete_utf8_len(&[0xC3]), 0);
        assert_eq!(complete_utf8_len(&[b'a', 0xE2, 0x82]), 1);
        assert_eq!(complete_utf8_len(&[0xE2, 0x82, 0xAC]), 3);
        assert_eq!(complete_utf8_len(&[b'x', 0xF0, 0x9F, 0x98]), 1);
        assert_eq!(complete_utf8_len(&[0xF0, 0x9F, 0x98, 0x80]), 4);
        // Stray continuation bytes are passed on, not held forever.
        assert_eq!(complete_utf8_len(&[0x80, 0x80, 0x80, 0x80, 0x80]), 5);
    }
}
