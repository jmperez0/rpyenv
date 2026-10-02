//! `RPYENV_DEBUG_LOG` (spec §11, §13): a file the shim appends its decisions and errors
//! to, for when there is no console to print on.

use std::io::Write;

/// Appends `rpyenv-shim: <line>` to the file `RPYENV_DEBUG_LOG` names. Nothing happens
/// when it is unset, and failures are ignored: a diagnostic must never stop the shim.
pub fn append(line: &str) {
    let Some(path) = std::env::var_os("RPYENV_DEBUG_LOG").filter(|p| !p.is_empty()) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "rpyenv-shim: {line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_lines_when_set() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("debug.log");
        std::env::set_var("RPYENV_DEBUG_LOG", &log);
        append("a");
        append("b");
        std::env::remove_var("RPYENV_DEBUG_LOG");
        append("not logged");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "rpyenv-shim: a\nrpyenv-shim: b\n"
        );
    }
}
