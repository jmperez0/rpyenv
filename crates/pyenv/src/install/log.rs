//! The build log (python-build's fd 4) with `-v`'s copy to stdout, and the BUILD FAILED
//! block (reference "Output streams…" and "Failure output").

use std::fs::File;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};

pub struct BuildLog {
    file: Arc<Mutex<File>>,
    verbose: bool,
    pub path: PathBuf,
}

impl BuildLog {
    pub fn open(path: &Path, verbose: bool) -> std::io::Result<BuildLog> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(BuildLog {
            file: Arc::new(Mutex::new(file)),
            verbose,
            path: path.to_path_buf(),
        })
    }

    pub fn line(&self, s: &str) {
        let mut me: &BuildLog = self;
        let _ = writeln!(me, "{s}");
    }

    /// Runs `cmd` with stdout and stderr copied to the log (and to stdout with `-v`).
    pub fn run(&self, cmd: &mut Command) -> std::io::Result<ExitStatus> {
        let mut child = spawn(
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )?;
        let pumps: Vec<_> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut src| {
            let file = self.file.clone();
            let verbose = self.verbose;
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = src.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let _ = file.lock().unwrap().write_all(&buf[..n]);
                    if verbose {
                        let _ = std::io::stdout().lock().write_all(&buf[..n]);
                    }
                }
            })
        })
        .collect();
        let status = child.wait();
        for p in pumps {
            let _ = p.join();
        }
        status
    }
}

impl Write for &BuildLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.lock().unwrap().write_all(buf)?;
        if self.verbose {
            std::io::stdout().lock().write_all(buf)?;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.lock().unwrap().flush()
    }
}

/// `cmd.spawn()`, retried while the program is busy (ETXTBSY): a file this process just
/// wrote, such as the TMPDIR probe or an extracted `configure`, stays open for writing in a
/// child that another thread forked until that child execs.
pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let mut tries = 0;
    loop {
        match cmd.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 50 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            r => return r,
        }
    }
}

fn tail(path: &Path, n: usize) -> Vec<String> {
    let Ok(f) = File::open(path) else {
        return Vec::new();
    };
    let lines: Vec<String> = std::io::BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// python-build's `build_failed` (bin/python-build:191-216), naming rpyenv (plan Decision 9).
pub fn failed_block(os: &str, build_path: &Path, log_path: &Path) -> Vec<String> {
    // On a tty, `BUILD FAILED` is bold and `Results logged to …` yellow (colorize 1 / 33).
    let tty = std::io::stderr().is_terminal();
    let paint = |code: &str, s: String| {
        if tty {
            format!("\x1b[{code}m{s}\x1b[m")
        } else {
            s
        }
    };
    let mut out = vec![
        String::new(),
        format!(
            "{} ({os} using rpyenv {})",
            paint("1", "BUILD FAILED".into()),
            env!("CARGO_PKG_VERSION")
        ),
        String::new(),
    ];
    // `rmdir` succeeds only on an empty directory; then the line is left out.
    if std::fs::remove_dir(build_path).is_err() && build_path.exists() {
        out.push(format!(
            "Inspect or clean up the working tree at {}",
            build_path.display()
        ));
    }
    let last = tail(log_path, 10);
    if !last.is_empty() {
        out.push(paint(
            "33",
            format!("Results logged to {}", log_path.display()),
        ));
        out.push(String::new());
        out.push("Last 10 log lines:".into());
        let hint = last
            .iter()
            .any(|l| l.contains("no acceptable C compiler found"));
        out.extend(last);
        if hint {
            out.extend([
                String::new(),
                "Are the build dependencies for Python correctly installed?".into(),
                "Please consult to the Wiki page for more info.".into(),
                "https://github.com/pyenv/pyenv/wiki#suggested-build-environment".into(),
            ]);
        }
    }
    out
}
