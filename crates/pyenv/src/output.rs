//! What one `pyenv` invocation prints.

use rpyenv_core::flavor::Flavor;
use std::io::Write;

/// Text uses `\n`; `emit` converts it to the flavor's line ending.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Output {
    pub fn new() -> Output {
        Output::default()
    }

    /// One stderr line and exit code 1.
    pub fn error(message: impl AsRef<str>) -> Output {
        let mut o = Output::new();
        o.err(message);
        o.code = 1;
        o
    }

    /// Appends a line to stdout.
    pub fn out(&mut self, line: impl AsRef<str>) {
        self.stdout.push_str(line.as_ref());
        self.stdout.push('\n');
    }

    /// Appends a line to stderr.
    pub fn err(&mut self, line: impl AsRef<str>) {
        self.stderr.push_str(line.as_ref());
        self.stderr.push('\n');
    }

    pub fn with_code(mut self, code: i32) -> Output {
        self.code = code;
        self
    }

    /// Writes stderr, then stdout.
    pub fn emit(&self, flavor: Flavor) {
        let convert = |s: &str| match flavor {
            Flavor::Pyenv => s.to_string(),
            Flavor::PyenvWin => s.replace('\n', "\r\n"),
        };
        let _ = std::io::stderr().write_all(convert(&self.stderr).as_bytes());
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(convert(&self.stdout).as_bytes());
        let _ = stdout.flush();
    }
}

impl From<rpyenv_core::lookup::Report> for Output {
    fn from(r: rpyenv_core::lookup::Report) -> Output {
        let mut o = Output::new();
        for line in &r.lines {
            if r.stderr {
                o.err(line);
            } else {
                o.out(line);
            }
        }
        o.with_code(r.code)
    }
}
