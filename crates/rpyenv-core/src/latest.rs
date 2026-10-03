//! Upstream `pyenv latest`: the newest installed version matching a prefix like `3.12`.

use std::cmp::Ordering;
use std::path::Path;

/// `pyenv latest <prefix>` over `candidates`, the installed names in
/// `versions --bare --skip-envs` order. None when nothing matches.
pub fn latest(prefix: &str, candidates: &[String], versions_dir: &Path) -> Option<String> {
    if versions_dir.join(prefix).is_dir() {
        return Some(prefix.to_string());
    }
    best(prefix, candidates)
}

/// Steps 3-8 of upstream's algorithm (M1 reference): the exact match, the `t` suffix, the
/// prefix filter, the exclusions and the sort. Known mode uses this directly.
pub fn best(prefix: &str, candidates: &[String]) -> Option<String> {
    if candidates.iter().any(|c| c == prefix) {
        return Some(prefix.to_string());
    }
    let (base, suffix) = match prefix.strip_suffix('t') {
        Some(b) if b.ends_with(|c: char| c.is_ascii_digit()) => (b, "t"),
        _ => (prefix, ""),
    };
    let mut matches: Vec<&String> = candidates
        .iter()
        .filter(|c| {
            let Some(rest) = c.strip_prefix(base) else {
                return false;
            };
            let mut chars = rest.chars();
            matches!(chars.next(), Some('-' | '.')) && chars.as_str().ends_with(suffix)
        })
        .filter(|c| !excluded(c, suffix.is_empty()))
        .collect();
    matches.sort_by(|a, b| compare_keys(&sort_key(a), &sort_key(b)));
    matches.first().map(|c| c.to_string())
}

fn excluded(c: &str, no_t_suffix: bool) -> bool {
    if c.ends_with("-dev") || c.ends_with("-src") || c.ends_with("-latest") {
        return true;
    }
    let trimmed = c.trim_end_matches(|ch: char| ch.is_ascii_digit());
    if trimmed.len() < c.len()
        && (trimmed.ends_with('a') || trimmed.ends_with('b') || trimmed.ends_with("rc"))
    {
        return true;
    }
    no_t_suffix
        && c.strip_suffix('t')
            .is_some_and(|b| b.ends_with(|ch: char| ch.is_ascii_digit()))
}

fn sort_key(c: &str) -> String {
    let n = c.bytes().take_while(u8::is_ascii_alphanumeric).count();
    if n > 0 && c.as_bytes().get(n) == Some(&b'-') {
        format!("{}.{}..|{}", &c[..n], &c[n + 1..], c)
    } else {
        format!("{c}...|{c}")
    }
}

/// `sort -t. -k1,1r -k2,2nr -k3,3nr -k4,4nr`, then the whole key in byte order.
/// Ties use byte order rather than the locale (allowlist D-06).
fn compare_keys(a: &str, b: &str) -> Ordering {
    let fa: Vec<&str> = a.split('.').collect();
    let fb: Vec<&str> = b.split('.').collect();
    let f_a = |i: usize| fa.get(i).copied().unwrap_or("");
    let f_b = |i: usize| fb.get(i).copied().unwrap_or("");
    f_b(0)
        .cmp(f_a(0))
        .then_with(|| num_cmp(f_b(1), f_a(1)))
        .then_with(|| num_cmp(f_b(2), f_a(2)))
        .then_with(|| num_cmp(f_b(3), f_a(3)))
        .then_with(|| a.cmp(b))
}

/// The leading number of a field as GNU `sort -n` reads it: (negative, digits without leading zeros).
fn leading_number(s: &str) -> (bool, &str) {
    let s = s.trim_start_matches([' ', '\t']);
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let digits = &rest[..rest.bytes().take_while(u8::is_ascii_digit).count()];
    let digits = digits.trim_start_matches('0');
    (neg && !digits.is_empty(), digits)
}

fn num_cmp(a: &str, b: &str) -> Ordering {
    let (na, da) = leading_number(a);
    let (nb, db) = leading_number(b);
    let magnitude = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
    match (na, nb) {
        (false, false) => magnitude,
        (true, true) => magnitude.reverse(),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Vec<String> {
        [
            "3.12.1",
            "3.12.9",
            "3.12.10",
            "3.12.0rc1",
            "3.12-dev",
            "3.12.2t",
            "3.11.9",
            "3.1.4",
            "2.7.18",
            "pypy3.10-7.3.9",
            "pypy3.10-7.3.17",
            "miniconda3-4.7.12",
            "miniconda3-24.1.2-0",
            "miniconda3-latest",
        ]
        .map(String::from)
        .to_vec()
    }

    fn l(prefix: &str) -> Option<String> {
        latest(
            prefix,
            &installed(),
            std::path::Path::new("/nonexistent/versions"),
        )
    }

    #[test]
    fn probe_table_from_the_reference() {
        assert_eq!(l("3.12").as_deref(), Some("3.12.10"));
        assert_eq!(l("3").as_deref(), Some("3.12.10"));
        assert_eq!(l("3.1").as_deref(), Some("3.1.4"));
        assert_eq!(l("3.12t").as_deref(), Some("3.12.2t"));
        assert_eq!(l("pypy3.10").as_deref(), Some("pypy3.10-7.3.17"));
        assert_eq!(l("miniconda3").as_deref(), Some("miniconda3-24.1.2-0"));
        assert_eq!(l("3.12.1").as_deref(), Some("3.12.1"));
        assert_eq!(l("4"), None);
        assert_eq!(l("pypy"), None);
    }

    #[test]
    fn prereleases_are_not_candidates() {
        let v = vec!["3.14.0rc1".to_string()];
        assert_eq!(latest("3.14", &v, std::path::Path::new("/none")), None);
    }

    #[test]
    fn t_suffix_keeps_prerelease_t_builds() {
        let v = vec!["3.13.0a1t".to_string()];
        assert_eq!(
            latest("3.13t", &v, std::path::Path::new("/none")).as_deref(),
            Some("3.13.0a1t")
        );
    }

    #[test]
    fn newest_patch_wins() {
        let v: Vec<String> = ["3.5.6", "3.10.8", "3.10.6"].map(String::from).to_vec();
        assert_eq!(
            latest("3", &v, std::path::Path::new("/none")).as_deref(),
            Some("3.10.8")
        );
    }

    #[test]
    fn an_existing_directory_is_returned_as_is() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("3.12-dev")).unwrap();
        assert_eq!(
            latest("3.12-dev", &[], tmp.path()).as_deref(),
            Some("3.12-dev")
        );
    }
}
