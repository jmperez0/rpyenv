//! pyenv-win's `TryResolveVersion`: a prefix like `3.9` becomes the newest installed `3.9.x`
//! with this machine's architecture suffix.

struct Parsed {
    major: u64,
    minor: u64,
    patch: u64,
    pre: bool,
    /// The architecture group as written, including its `.` or `-`.
    arch: String,
}

/// Matches `^(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:([a-z]+)(\d*))?([\.-](?:amd64|arm64|win32))?$`, ignoring case.
fn parse(s: &str) -> Option<Parsed> {
    let b = s.as_bytes();
    let mut i = 0;
    let number = |i: &mut usize| -> Option<u64> {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        (*i > start).then(|| s[start..*i].parse::<u64>().unwrap_or(u64::MAX))
    };
    let major = number(&mut i)?;
    let (mut minor, mut patch) = (0, 0);
    if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        i += 1;
        minor = number(&mut i)?;
        if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            i += 1;
            patch = number(&mut i)?;
        }
    }
    let pre_start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let pre = i > pre_start;
    if pre {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    let arch = &s[i..];
    if !arch.is_empty() {
        let lower = arch.to_ascii_lowercase();
        let valid = (lower.starts_with('.') || lower.starts_with('-'))
            && matches!(&lower[1..], "amd64" | "arm64" | "win32");
        if !valid {
            return None;
        }
    }
    Some(Parsed {
        major,
        minor,
        patch,
        pre,
        arch: arch.to_string(),
    })
}

/// The resolved name, or `prefix` unchanged when nothing qualifies. `installed` is in
/// directory order; on a numeric tie the first one wins (allowlist D-05).
pub fn resolve(prefix: &str, installed: &[String], arch: &str) -> String {
    find_latest(prefix, installed, arch).unwrap_or_else(|| prefix.to_string())
}

/// pyenv-win's `FindLatestVersion`: the newest qualifying candidate, or `None`; `latest` has
/// no fall-back to the argument (M1 reference, *latest*).
pub fn find_latest(prefix: &str, installed: &[String], arch: &str) -> Option<String> {
    let mut best: Option<(&String, (u64, u64, u64))> = None;
    for c in installed {
        let Some(rest) = c.strip_prefix(prefix) else {
            continue;
        };
        if rest != arch && !rest.starts_with('.') {
            continue;
        }
        let Some(p) = parse(c) else {
            continue;
        };
        if p.pre || p.arch != arch {
            continue;
        }
        let key = (p.major, p.minor, p.patch);
        if best.is_none_or(|(_, k)| key > k) {
            best = Some((c, key));
        }
    }
    best.map(|(c, _)| c.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn find_latest_distinguishes_a_match_from_no_match() {
        let i = v(&["3.12.1", "3.12.10"]);
        assert_eq!(find_latest("3.12.1", &i, ""), Some("3.12.1".to_string()));
        assert_eq!(find_latest("3.12", &i, ""), Some("3.12.10".to_string()));
        assert_eq!(find_latest("9", &i, ""), None);
        assert_eq!(resolve("9", &i, ""), "9");
    }

    #[test]
    fn reference_examples() {
        let i = v(&["3.1.4", "3.11.0", "3.2.0", "3.2.5", "3.9.1"]);
        assert_eq!(resolve("3.1", &i, ""), "3.1.4");
        assert_eq!(resolve("3.2", &i, ""), "3.2.5");
        assert_eq!(resolve("3.2.5", &i, ""), "3.2.5");
        assert_eq!(resolve("1", &i, ""), "1");
        let j = v(&["3.9.4", "3.7.2", "3.7.7", "3.9.1"]);
        assert_eq!(resolve("3", &j, ""), "3.9.4");
        assert_eq!(resolve("3.7", &j, ""), "3.7.7");
    }

    #[test]
    fn architecture_suffix_must_match_exactly() {
        let i = v(&["3.1.0-win32", "3.1.4"]);
        assert_eq!(resolve("3.1", &i, "-win32"), "3.1.0-win32");
        assert_eq!(resolve("3.1", &i, ""), "3.1.4");
        assert_eq!(resolve("3.9", &v(&["3.9.1-WIN32"]), "-win32"), "3.9");
    }

    #[test]
    fn prereleases_and_trailing_dots_never_qualify() {
        assert_eq!(resolve("3.9", &v(&["3.9.0rc1"]), ""), "3.9");
        assert_eq!(resolve("3.", &v(&["3.9.1"]), ""), "3.");
    }

    #[test]
    fn a_numeric_tie_picks_the_first_in_directory_order() {
        // Review focus 5: pyenv-win raises a VBScript runtime error here (D-05).
        assert_eq!(resolve("3", &v(&["3.9", "3.9.0"]), ""), "3.9");
        assert_eq!(resolve("3", &v(&["3.9.0", "3.9"]), ""), "3.9.0");
    }
}
