//! The PowerShell profile line (spec §7, §9.4): ``iex ((pyenv init - pwsh) -join "`n")``,
//! in the Windows PowerShell 5.1 and PowerShell 7 profiles under Documents.

use crate::output::Output;
use std::io;
use std::path::{Path, PathBuf};

pub const LINE: &str = "iex ((pyenv init - pwsh) -join \"`n\")";

/// The current-user, current-host profiles of Windows PowerShell 5.1 and PowerShell 7.
pub fn paths(documents: &Path) -> [PathBuf; 2] {
    [
        documents
            .join("WindowsPowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
        documents
            .join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Added,
    /// The profile already mentions pyenv: left alone, as `pyenv init --install` does.
    Mentions,
}

/// Appends `LINE` to `file` (created with its folder if missing), unless it already
/// mentions pyenv in any case.
pub fn add(file: &Path) -> io::Result<Added> {
    let text = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    if text.to_ascii_lowercase().windows(5).any(|w| w == b"pyenv") {
        return Ok(Added::Mentions);
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = text;
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(LINE.as_bytes());
    out.extend_from_slice(b"\r\n");
    std::fs::write(file, out)?;
    Ok(Added::Added)
}

/// Removes every line that is exactly `LINE`; true when there was one.
pub fn remove(file: &Path) -> io::Result<bool> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut removed = false;
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == LINE {
            removed = true;
        } else {
            out.push_str(line);
        }
    }
    if removed {
        std::fs::write(file, out)?;
    }
    Ok(removed)
}

/// Adds the line to both profiles and reports each (`init --install pwsh`, `setup`).
pub fn install_all() -> Output {
    let mut o = Output::new();
    let Some(docs) = rpyenv_core::winenv::documents() else {
        return Output::error(
            "pyenv: cannot find your Documents folder for the PowerShell profiles",
        );
    };
    for p in paths(&docs) {
        match add(&p) {
            Ok(Added::Added) => o.out(format!(
                "pyenv: added the PowerShell line to {}",
                p.display()
            )),
            Ok(Added::Mentions) => o.out(format!(
                "pyenv: {} already mentions pyenv; left as it is",
                p.display()
            )),
            Err(e) => o.err(format!(
                "pyenv: {}: {}",
                p.display(),
                rpyenv_core::launch::io_reason(&e)
            )),
        }
    }
    if !o.stderr.is_empty() {
        o.code = 1;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_profiles_under_documents() {
        let [p5, p7] = paths(Path::new(r"D:\Docs"));
        assert_eq!(
            p5,
            Path::new(r"D:\Docs")
                .join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1")
        );
        assert_eq!(
            p7,
            Path::new(r"D:\Docs")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1")
        );
    }

    #[test]
    fn add_creates_appends_once_and_leaves_a_pyenv_profile_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let new = tmp.path().join("a").join("p.ps1");
        assert_eq!(add(&new).unwrap(), Added::Added);
        assert_eq!(
            std::fs::read_to_string(&new).unwrap(),
            format!("{LINE}\r\n")
        );
        assert_eq!(
            add(&new).unwrap(),
            Added::Mentions,
            "already mentions pyenv"
        );
        let other = tmp.path().join("o.ps1");
        std::fs::write(&other, "Set-Alias ll ls").unwrap();
        assert_eq!(add(&other).unwrap(), Added::Added);
        assert_eq!(
            std::fs::read_to_string(&other).unwrap(),
            format!("Set-Alias ll ls\r\n{LINE}\r\n")
        );
        let mine = tmp.path().join("m.ps1");
        std::fs::write(&mine, "# my PYENV setup\r\n").unwrap();
        assert_eq!(add(&mine).unwrap(), Added::Mentions);
        assert_eq!(
            std::fs::read_to_string(&mine).unwrap(),
            "# my PYENV setup\r\n"
        );
    }

    #[test]
    fn remove_takes_out_only_the_exact_line() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("p.ps1");
        std::fs::write(&f, format!("a\r\n{LINE}\r\n# pyenv notes\r\nb\r\n")).unwrap();
        assert!(remove(&f).unwrap());
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "a\r\n# pyenv notes\r\nb\r\n"
        );
        assert!(!remove(&f).unwrap());
        assert!(!remove(&tmp.path().join("missing.ps1")).unwrap());
    }
}
