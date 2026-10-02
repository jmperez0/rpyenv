//! `RPYENV_DEBUG_LOG` (spec §11, §13): a file a shim, or `pyenv exec`, appends its
//! decisions and errors to, for when there is no console to print on.

use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

static SOURCE: OnceLock<&'static str> = OnceLock::new();

/// Names the program at the start of every later line: `rpyenv-shim` unless this is
/// called first. The first call wins.
pub fn set_source(name: &'static str) {
    let _ = SOURCE.set(name);
}

/// Appends `<source>: <line>` to the file `RPYENV_DEBUG_LOG` names. Nothing happens when
/// it is unset or empty.
pub fn append(line: &str) {
    let Some(path) = std::env::var_os("RPYENV_DEBUG_LOG").filter(|p| !p.is_empty()) else {
        return;
    };
    append_to(
        Path::new(&path),
        SOURCE.get().copied().unwrap_or("rpyenv-shim"),
        line,
    );
}

/// Appends `<source>: <line>` and the platform's line ending to `path` in one write, so
/// lines from nested shims sharing one log don't interleave. Failures are ignored: a
/// diagnostic must never stop the shim.
pub fn append_to(path: &Path, source: &str, line: &str) {
    let eol = if cfg!(windows) { "\r\n" } else { "\n" };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(format!("{source}: {line}{eol}").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_whole_lines_with_their_source() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("debug.log");
        append_to(&log, "rpyenv-shim", "a");
        append_to(&log, "pyenv exec", "b");
        let eol = if cfg!(windows) { "\r\n" } else { "\n" };
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            format!("rpyenv-shim: a{eol}pyenv exec: b{eol}")
        );
    }

    #[test]
    fn an_unopenable_log_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        append_to(&tmp.path().join("missing-dir").join("debug.log"), "x", "y");
    }
}
