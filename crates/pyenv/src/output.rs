//! What one `pyenv` invocation prints.

use rpyenv_core::flavor::Flavor;
use std::io::Write;

/// Text uses `\n`; `emit` converts it to the flavor's line ending.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
    /// When set, `emit` writes these bytes to stdout as-is instead of `stdout` (Windows
    /// `RPYENV_FORWARD_CP`, spec §5.3): a path encoded in the console's code page isn't
    /// valid UTF-8 in general, so it can't go through the `String` field.
    pub raw_stdout: Option<Vec<u8>>,
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
        rpyenv_core::textout::write(true, &convert(&self.stderr));
        match &self.raw_stdout {
            // Already encoded for the console (`RPYENV_FORWARD_CP`): written as it is.
            Some(bytes) => {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(bytes);
                let _ = stdout.flush();
            }
            None => rpyenv_core::textout::write(false, &convert(&self.stdout)),
        }
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
