//! `read -p` for the commands that ask a question (install, uninstall).

use super::interrupted;
use std::io::{BufRead, IsTerminal};

/// What `read -p` got.
pub enum Reply {
    Line(String),
    Eof,
    Interrupted,
}

/// `read -p`: the prompt shows only on a terminal. The line is read on a helper thread so
/// that a Ctrl+C while waiting ends the run at once, as it does upstream, instead of being
/// noticed only after Enter or EOF.
pub fn prompt(text: &str) -> Reply {
    if std::io::stdin().is_terminal() {
        rpyenv_core::textout::write(true, text);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let got = match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        };
        let _ = tx.send(got);
    });
    loop {
        if interrupted() {
            return Reply::Interrupted;
        }
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Some(line)) => return Reply::Line(line),
            Ok(None) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Reply::Eof,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
