//! A Windows `PATH` value as entries (spec §9.4): pure, so its tests run on every OS.

/// The entries of `value`, in order, without empty ones.
pub fn split(value: &str) -> Vec<String> {
    value
        .split(';')
        .filter(|e| !e.trim().is_empty())
        .map(str::to_string)
        .collect()
}

pub fn join(entries: &[String]) -> String {
    entries.join(";")
}

/// The same folder: compared without case and without a trailing `\` or `/`.
pub fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

/// `value` with `entry` first and only there; `expand` turns an entry as written
/// (`%USERPROFILE%\…`) into the folder it names. `None` when it's already so.
pub fn put_first(value: &str, entry: &str, expand: &dyn Fn(&str) -> String) -> Option<String> {
    let entries = split(value);
    let rest: Vec<String> = entries
        .iter()
        .filter(|e| !same(&expand(e), entry))
        .cloned()
        .collect();
    let already =
        entries.first().is_some_and(|e| same(&expand(e), entry)) && rest.len() + 1 == entries.len();
    if already {
        return None;
    }
    let mut out = vec![entry.to_string()];
    out.extend(rest);
    Some(join(&out))
}

/// `value` without the entries `drop` matches, and those entries as written.
pub fn without(value: &str, drop: &dyn Fn(&str) -> bool) -> (String, Vec<String>) {
    let (gone, kept): (Vec<String>, Vec<String>) = split(value).into_iter().partition(|e| drop(e));
    (join(&kept), gone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn split_and_join_drop_empty_entries() {
        assert_eq!(split(r"a;;b;"), vec!["a", "b"]);
        assert_eq!(join(&split(r"a;b")), "a;b");
        assert!(split("").is_empty());
    }

    #[test]
    fn entries_compare_without_case_or_a_trailing_separator() {
        assert!(same(r"C:\Root\Shims\", r"c:\root\shims"));
        assert!(same("C:/x/", r"C:/x"));
        assert!(!same(r"C:\root\shims2", r"C:\root\shims"));
    }

    /// Review focus 1: however the entry is spelled, it ends up first and only once.
    #[test]
    fn put_first_moves_the_entry_and_removes_its_other_spellings() {
        let expand = |s: &str| s.replace("%USERPROFILE%", r"C:\Users\me");
        let shims = r"C:\Users\me\.pyenv\pyenv-win\shims";
        let v = r"C:\a;%USERPROFILE%\.pyenv\pyenv-win\Shims\;C:\b";
        assert_eq!(
            put_first(v, shims, &expand).unwrap(),
            format!(r"{shims};C:\a;C:\b")
        );
        assert_eq!(put_first(&format!(r"{shims};C:\a"), shims, &expand), None);
        assert_eq!(put_first("", shims, &id).unwrap(), shims);
    }

    #[test]
    fn without_returns_what_it_removed_as_written() {
        let (v, gone) = without(r"C:\x\bin;C:\a;c:\X\BIN\", &|e| same(e, r"C:\x\bin"));
        assert_eq!(v, r"C:\a");
        assert_eq!(
            gone,
            vec![r"C:\x\bin".to_string(), r"c:\X\BIN\".to_string()]
        );
    }
}
