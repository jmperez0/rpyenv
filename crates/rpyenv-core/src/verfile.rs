//! Finding, reading and writing `.python-version` and the global `version` file.

use crate::flavor::Flavor;
use crate::paths::{lexical_normalize, strip_bom};
use std::path::{Path, PathBuf};

pub const LOCAL_NAME: &str = ".python-version";

/// The nearest `.python-version` in `start` or one of its parents.
/// Only regular files count; an empty file still counts.
pub fn find_local(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let f = d.join(LOCAL_NAME);
        if f.is_file() {
            return Some(f);
        }
        dir = d.parent();
    }
    None
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PyenvRead {
    pub versions: Vec<String>,
    /// Upstream's `invalid version` lines, for stderr.
    pub warnings: Vec<String>,
}

/// Upstream `pyenv version-file-read`. `shown` is how the file is named in warnings.
/// A missing or empty file gives an empty result (upstream exits 1).
pub fn read_pyenv(file: &Path, shown: &str, versions_dir: &Path) -> PyenvRead {
    let mut out = PyenvRead::default();
    let Ok(bytes) = std::fs::read(file) else {
        return out;
    };
    let text = String::from_utf8_lossy(strip_bom(&bytes)).into_owned();
    for line in text.split('\n') {
        // bash `read` skips leading spaces and tabs; `\r` is a delimiter but not
        // whitespace, so a leading `\r` makes the first word empty.
        let trimmed = line.trim_start_matches([' ', '\t']);
        let word = trimmed.split([' ', '\t', '\r']).next().unwrap_or("");
        if word.is_empty() || word.starts_with('#') {
            continue;
        }
        if (word == ".." || word.contains('/')) && !is_version_safe(word, versions_dir) {
            out.warnings.push(format!(
                "pyenv: invalid version `{word}' ignored in `{shown}'"
            ));
            continue;
        }
        out.versions.push(word.to_string());
    }
    out
}

/// The entry must be an existing directory whose logical path stays inside `versions/`.
fn is_version_safe(word: &str, versions_dir: &Path) -> bool {
    let candidate = versions_dir.join(word);
    if !candidate.is_dir() {
        return false;
    }
    let inside = lexical_normalize(&candidate);
    let vdir = lexical_normalize(versions_dir);
    inside != vdir && inside.starts_with(&vdir)
}

/// pyenv-win's version file reader: every line except an exactly empty one.
/// Lines are terminated by LF or CRLF; a lone CR is not a line terminator.
pub fn read_pyenv_win(file: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(file) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(strip_bom(&bytes)).into_owned();
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\n' {
            if !current_line.is_empty() {
                lines.push(current_line.clone());
                current_line.clear();
            }
        } else if c == '\r' {
            if chars.peek() == Some(&'\n') {
                // CRLF: consume the \n and end the line
                chars.next();
                if !current_line.is_empty() {
                    lines.push(current_line.clone());
                    current_line.clear();
                }
            } else {
                // Lone \r: treat as data, not a line ending
                current_line.push(c);
            }
        } else {
            current_line.push(c);
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

/// Writes one version per line, truncating the file.
pub fn write_versions(file: &Path, versions: &[&str], flavor: Flavor) -> std::io::Result<()> {
    let mut s = String::new();
    for v in versions {
        s.push_str(v);
        s.push_str(flavor.eol());
    }
    std::fs::write(file, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file_with(dir: &Path, content: &[u8]) -> PathBuf {
        let f = dir.join("my-version");
        fs::write(&f, content).unwrap();
        f
    }

    fn pyenv(content: &[u8]) -> PyenvRead {
        let tmp = tempfile::tempdir().unwrap();
        let versions = tmp.path().join("versions");
        fs::create_dir_all(&versions).unwrap();
        read_pyenv(&file_with(tmp.path(), content), "my-version", &versions)
    }

    #[test]
    fn pyenv_crlf_and_several_lines() {
        assert_eq!(
            pyenv(b"3.12.1\r\n3.11.9\r\n").versions,
            ["3.12.1", "3.11.9"]
        );
    }

    #[test]
    fn pyenv_first_word_comments_blank_lines_and_no_final_newline() {
        let r = pyenv(b"  3.12.1 extra words\n# comment\n  #3.11.9\n\n2.7.18");
        assert_eq!(r.versions, ["3.12.1", "2.7.18"]);
        assert_eq!(pyenv(b"\t3.10.4\tfoo\n").versions, ["3.10.4"]);
        assert_eq!(pyenv(b"3.12#foo\n").versions, ["3.12#foo"]);
    }

    #[test]
    fn pyenv_empty_inputs_give_nothing() {
        assert!(pyenv(b"").versions.is_empty());
        assert!(pyenv(b"\n").versions.is_empty());
        assert!(pyenv(b"\r\n").versions.is_empty());
    }

    #[test]
    fn pyenv_bom_is_ignored() {
        assert_eq!(pyenv(b"\xEF\xBB\xBF3.12.1\r\n").versions, ["3.12.1"]);
    }

    #[test]
    fn pyenv_dot_dot_is_rejected_with_a_warning() {
        let r = pyenv(b"..\n");
        assert!(r.versions.is_empty());
        assert_eq!(
            r.warnings,
            ["pyenv: invalid version `..' ignored in `my-version'"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn pyenv_paths_must_exist_and_stay_inside_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let versions = tmp.path().join("versions");
        fs::create_dir_all(versions.join("3.12.1")).unwrap();
        let f = file_with(tmp.path(), b"..\n../3.12.1\n3.12/../3.12.1\n3.12.1\n");
        let r = read_pyenv(&f, ".python-version", &versions);
        assert_eq!(r.versions, ["3.12.1"]);
        assert_eq!(
            r.warnings,
            [
                "pyenv: invalid version `..' ignored in `.python-version'",
                "pyenv: invalid version `../3.12.1' ignored in `.python-version'",
                "pyenv: invalid version `3.12/../3.12.1' ignored in `.python-version'",
            ]
        );
    }

    #[test]
    fn pyenv_env_paths_inside_versions_are_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let versions = tmp.path().join("versions");
        fs::create_dir_all(versions.join("3.10.3").join("envs")).unwrap();
        fs::create_dir_all(versions.join("3.10.3").join("test")).unwrap();
        let f = file_with(tmp.path(), b"3.10.3/envs/../test\n");
        assert_eq!(
            read_pyenv(&f, "f", &versions).versions,
            ["3.10.3/envs/../test"]
        );
    }

    #[test]
    fn pyenv_missing_file_gives_nothing() {
        let r = read_pyenv(Path::new("/no/such/file"), "x", Path::new("/no/versions"));
        assert!(r.versions.is_empty() && r.warnings.is_empty());
    }

    fn win(content: &[u8]) -> Vec<String> {
        let tmp = tempfile::tempdir().unwrap();
        read_pyenv_win(&file_with(tmp.path(), content))
    }

    #[test]
    fn pyenv_win_lines() {
        assert_eq!(win(b"3.9.1\r\n3.8.2\r\n"), ["3.9.1", "3.8.2"]);
        assert_eq!(win(b"3.9.1\n3.8.2\n"), ["3.9.1", "3.8.2"]);
        assert_eq!(win(b"3.9.1"), ["3.9.1"]);
        assert_eq!(win(b"3.9.1\r3.8.2\r"), ["3.9.1\r3.8.2\r"]);
        assert!(win(b"\r\n\r\n").is_empty());
    }

    #[test]
    fn pyenv_win_keeps_whitespace_and_hash_but_not_the_bom() {
        assert_eq!(win(b"  3.9.1  "), ["  3.9.1  "]);
        assert_eq!(win(b"# comment"), ["# comment"]);
        assert_eq!(win(b"\xEF\xBB\xBF3.9.1\r\n"), ["3.9.1"]);
    }

    #[test]
    fn find_local_walks_up_and_accepts_empty_files() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("a").join("b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_local(&deep), None);
        let f = tmp.path().join("a").join(LOCAL_NAME);
        fs::write(&f, "").unwrap();
        assert_eq!(find_local(&deep), Some(f));
    }

    #[test]
    fn find_local_skips_a_directory_named_like_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(LOCAL_NAME)).unwrap();
        assert_eq!(find_local(tmp.path()), None);
    }

    #[test]
    fn write_uses_the_flavor_line_ending() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("v");
        write_versions(&f, &["3.12.1", "3.11.9"], Flavor::Pyenv).unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"3.12.1\n3.11.9\n");
        write_versions(&f, &["3.7"], Flavor::PyenvWin).unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"3.7\r\n");
    }
}
