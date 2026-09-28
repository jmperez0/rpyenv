//! GNU `sort --version-sort` (gnulib `filevercmp`), which upstream uses to order `pyenv versions`.

use std::cmp::Ordering;

/// gnulib `order()`: the sort weight of the byte at `pos`.
fn order(s: &[u8], pos: usize) -> i32 {
    if pos >= s.len() {
        return -1;
    }
    let c = s[pos];
    if c.is_ascii_digit() {
        0
    } else if c.is_ascii_alphabetic() {
        i32::from(c)
    } else if c == b'~' {
        -2
    } else {
        i32::from(c) + 256
    }
}

/// gnulib `verrevcmp()` (the Debian version comparison).
fn verrevcmp(s1: &[u8], s2: &[u8]) -> i32 {
    let (mut i, mut j) = (0usize, 0usize);
    while i < s1.len() || j < s2.len() {
        let mut first_diff = 0i32;
        while (i < s1.len() && !s1[i].is_ascii_digit()) || (j < s2.len() && !s2[j].is_ascii_digit())
        {
            let (a, b) = (order(s1, i), order(s2, j));
            if a != b {
                return a - b;
            }
            i += 1;
            j += 1;
        }
        while i < s1.len() && s1[i] == b'0' {
            i += 1;
        }
        while j < s2.len() && s2[j] == b'0' {
            j += 1;
        }
        while i < s1.len() && j < s2.len() && s1[i].is_ascii_digit() && s2[j].is_ascii_digit() {
            if first_diff == 0 {
                first_diff = i32::from(s1[i]) - i32::from(s2[j]);
            }
            i += 1;
            j += 1;
        }
        if i < s1.len() && s1[i].is_ascii_digit() {
            return 1;
        }
        if j < s2.len() && s2[j].is_ascii_digit() {
            return -1;
        }
        if first_diff != 0 {
            return first_diff;
        }
    }
    0
}

/// gnulib `file_prefixlen()`: the length without a trailing `(\.[A-Za-z~][A-Za-z0-9~]*)*` suffix.
fn file_prefixlen(s: &[u8]) -> usize {
    let n = s.len();
    let mut prefixlen = 0;
    let mut i = 0;
    loop {
        if i == n {
            return prefixlen;
        }
        i += 1;
        prefixlen = i;
        while i + 1 < n && s[i] == b'.' && (s[i + 1].is_ascii_alphabetic() || s[i + 1] == b'~') {
            i += 2;
            while i < n && (s[i].is_ascii_alphanumeric() || s[i] == b'~') {
                i += 1;
            }
        }
    }
}

fn filenvercmp(a: &[u8], b: &[u8]) -> i32 {
    if a.is_empty() {
        return if b.is_empty() { 0 } else { -1 };
    }
    if b.is_empty() {
        return 1;
    }
    if a[0] == b'.' {
        if b[0] != b'.' {
            return -1;
        }
        let (adot, bdot) = (a.len() == 1, b.len() == 1);
        if adot {
            return if bdot { 0 } else { -1 };
        }
        if bdot {
            return 1;
        }
        let adotdot = a[1] == b'.' && a.len() == 2;
        let bdotdot = b[1] == b'.' && b.len() == 2;
        if adotdot {
            return if bdotdot { 0 } else { -1 };
        }
        if bdotdot {
            return 1;
        }
    } else if b[0] == b'.' {
        return 1;
    }
    let (ap, bp) = (file_prefixlen(a), file_prefixlen(b));
    let one_pass_only = ap == a.len() && bp == b.len();
    let result = verrevcmp(&a[..ap], &b[..bp]);
    if result != 0 || one_pass_only {
        result
    } else {
        verrevcmp(a, b)
    }
}

/// gnulib `filevercmp()`.
pub fn filevercmp(a: &[u8], b: &[u8]) -> Ordering {
    filenvercmp(a, b).cmp(&0)
}

/// Orders version directory names as upstream's `pyenv versions` does: version
/// sort of `<versions_dir>/<name>`, then byte order for names it finds equal.
pub fn sort_version_names(names: &mut [String], versions_dir: &str) {
    names.sort_by(|a, b| {
        let fa = format!("{versions_dir}/{a}");
        let fb = format!("{versions_dir}/{b}");
        filevercmp(fa.as_bytes(), fb.as_bytes()).then_with(|| fa.cmp(&fb))
    });
}

#[cfg(test)]
mod tests {
    use super::{filevercmp, sort_version_names};
    use std::cmp::Ordering::{Equal, Greater, Less};

    #[test]
    fn upstream_listing_order() {
        // The probe listing in docs/parity/pyenv-m1-reference.md (`pyenv versions`).
        let mut v: Vec<String> = [
            "with space",
            "pypy3.10-7.3.17",
            "3.12.10",
            "alpha",
            "3.12",
            "ext",
            "3.12.9",
            "2.7.18",
            "3.12-dev",
            "miniconda3-latest",
            ".venv",
            "3.10.0",
            "3.12.0rc1",
            "3.9.1",
        ]
        .map(String::from)
        .to_vec();
        sort_version_names(&mut v, "/home/tester/.pyenv/versions");
        assert_eq!(
            v,
            [
                ".venv",
                "2.7.18",
                "3.9.1",
                "3.10.0",
                "3.12",
                "3.12-dev",
                "3.12.0rc1",
                "3.12.9",
                "3.12.10",
                "alpha",
                "ext",
                "miniconda3-latest",
                "pypy3.10-7.3.17",
                "with space",
            ]
        );
    }

    #[test]
    fn numbers_compare_numerically() {
        assert_eq!(filevercmp(b"1.2", b"1.10"), Less);
        assert_eq!(filevercmp(b"1.10", b"1.9"), Greater);
        assert_eq!(filevercmp(b"1.02", b"1.2"), Equal);
    }

    #[test]
    fn dot_names_come_first() {
        assert_eq!(filevercmp(b".", b".."), Less);
        assert_eq!(filevercmp(b"..", b".x"), Less);
        assert_eq!(filevercmp(b".x", b"a"), Less);
        assert_eq!(filevercmp(b"", b"a"), Less);
    }

    #[test]
    fn tilde_sorts_before_the_end() {
        assert_eq!(filevercmp(b"1.0~rc1", b"1.0"), Less);
    }
}
