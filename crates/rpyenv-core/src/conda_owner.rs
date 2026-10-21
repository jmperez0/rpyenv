//! Which conda executables get shims on Windows (parent spec §15.1 resolution 3, M7c design
//! §6.3): by the package that owns each file, read from `conda-meta\*.json`; upstream's
//! `default.list` names never.
//!
//! `rpyenv-core` is linked by the shims, which carry no runtime dependency beyond the OS
//! bindings (M7c design §3), so `conda-meta` is read by a small JSON reader below rather than
//! `serde_json`.

use std::collections::HashMap;
use std::path::Path;

pub struct Owners {
    /// Lowercased `/`-path → whether its package is Python-related.
    by_path: HashMap<String, bool>,
}

impl Owners {
    pub fn read(prefix: &Path) -> Owners {
        let mut by_path = HashMap::new();
        let Ok(rd) = std::fs::read_dir(prefix.join("conda-meta")) else {
            return Owners { by_path };
        };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Some((name, files)) = parse_meta(&text) else {
                continue;
            };
            let related = name == "python"
                || name == "conda"
                || files
                    .iter()
                    .any(|f| f.to_ascii_lowercase().contains("site-packages/"));
            for f in files {
                by_path.insert(f.replace('\\', "/").to_ascii_lowercase(), related);
            }
        }
        Owners { by_path }
    }

    /// `rel`: the file under the version (`Scripts/pip.exe`); `stem`: its name without
    /// extension.
    pub fn shim(&self, rel: &str, stem: &str) -> bool {
        if crate::conda_list::names().contains(stem.to_ascii_lowercase().as_str()) {
            return false;
        }
        let key = rel.replace('\\', "/").to_ascii_lowercase();
        match self.by_path.get(&key) {
            Some(related) => *related,
            None => key.starts_with("scripts/") && !key[8..].contains('/'),
        }
    }
}

/// The top-level `"name"` string and `"files"` string array of a `conda-meta` file. Only
/// depth-1 keys count: `paths_data.paths[]` and others nest keys of the same names. `None`
/// when the text isn't well-formed JSON object syntax, or lacks either key.
fn parse_meta(text: &str) -> Option<(String, Vec<String>)> {
    let mut r = Reader {
        s: text.as_bytes(),
        i: 0,
    };
    let mut name = None;
    let mut files = None;
    r.ws();
    r.eat(b'{')?;
    r.ws();
    if r.peek()? == b'}' {
        return None;
    }
    loop {
        r.ws();
        let key = r.string()?;
        r.ws();
        r.eat(b':')?;
        r.ws();
        match key.as_str() {
            "name" if r.peek()? == b'"' => name = Some(r.string()?),
            "files" if r.peek()? == b'[' => files = Some(r.string_array()?),
            _ => r.skip_value()?,
        }
        r.ws();
        match r.next()? {
            b',' => {}
            b'}' => break,
            _ => return None,
        }
    }
    Some((name?, files?))
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.i += 1;
        Some(b)
    }

    fn eat(&mut self, b: u8) -> Option<()> {
        (self.next()? == b).then_some(())
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0;
        for _ in 0..4 {
            v = v * 16 + (self.next()? as char).to_digit(16)?;
        }
        Some(v)
    }

    /// A string literal, escapes decoded (`\uXXXX` with surrogate pairs included).
    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.next()? {
                b'"' => break,
                b'\\' => {
                    let c = match self.next()? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                self.eat(b'\\')?;
                                self.eat(b'u')?;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return None;
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(cp)?
                        }
                        _ => return None,
                    };
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                b if b < 0x20 => return None,
                b => out.push(b),
            }
        }
        String::from_utf8(out).ok()
    }

    fn string_array(&mut self) -> Option<Vec<String>> {
        self.eat(b'[')?;
        let mut v = Vec::new();
        self.ws();
        if self.peek()? == b']' {
            self.i += 1;
            return Some(v);
        }
        loop {
            self.ws();
            v.push(self.string()?);
            self.ws();
            match self.next()? {
                b',' => {}
                b']' => return Some(v),
                _ => return None,
            }
        }
    }

    /// Skips one value of any kind, nesting tracked.
    fn skip_value(&mut self) -> Option<()> {
        match self.peek()? {
            b'"' => {
                self.string()?;
            }
            b'{' | b'[' => {
                let mut depth = 0usize;
                loop {
                    match self.peek()? {
                        b'"' => {
                            self.string()?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => depth -= 1,
                        _ => {}
                    }
                    self.i += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            _ => {
                // number, true, false, null: up to the next delimiter
                let start = self.i;
                while !matches!(
                    self.peek(),
                    None | Some(b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
                ) {
                    self.i += 1;
                }
                if self.i == start {
                    return None;
                }
            }
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(meta: &[(&str, &[&str])]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("conda-meta")).unwrap();
        for (i, (name, files)) in meta.iter().enumerate() {
            let json = serde_json::json!({"name": name, "version": "1", "files": files});
            std::fs::write(
                t.path().join("conda-meta").join(format!("{name}-{i}.json")),
                json.to_string(),
            )
            .unwrap();
        }
        t
    }

    #[test]
    fn python_related_owners_get_shims_and_tools_do_not() {
        let t = tree(&[
            ("python", &["python.exe", "Lib/site.py"]),
            (
                "conda",
                &["Scripts/conda.exe", "Lib/site-packages/conda/__init__.py"],
            ),
            (
                "requests",
                &["Lib/site-packages/requests/__init__.py", "Scripts/req.exe"],
            ),
            ("m2-cygpath", &["Library/usr/bin/cygpath.exe"]),
            ("libcurl", &["Library/bin/curl.exe"]),
            (
                "menuinst",
                &[
                    "Lib/site-packages/menuinst/__init__.py",
                    "Library/bin/menu.exe",
                ],
            ),
        ]);
        let o = Owners::read(t.path());
        assert!(o.shim("python.exe", "python"));
        assert!(o.shim("Scripts/conda.exe", "conda"));
        assert!(o.shim("Scripts/req.exe", "req"));
        assert!(o.shim("Library/bin/menu.exe", "menu"));
        assert!(!o.shim("Library/bin/curl.exe", "curl"));
        // Unowned: only in Scripts (where pip writes).
        assert!(o.shim("Scripts/mytool.exe", "mytool"));
        assert!(!o.shim("pre_uninstall.bat", "pre_uninstall"));
        assert!(!o.shim("Library/bin/x.exe", "x"));
    }

    #[test]
    fn default_list_names_never_get_shims_even_when_python_related() {
        let t = tree(&[(
            "pkg",
            &["Lib/site-packages/pkg/__init__.py", "Scripts/curl.exe"],
        )]);
        assert!(!Owners::read(t.path()).shim("Scripts/curl.exe", "curl"));
    }

    #[test]
    fn paths_compare_without_case() {
        let t = tree(&[("python", &["python.exe"])]);
        assert!(Owners::read(t.path()).shim("PYTHON.EXE", "PYTHON"));
    }

    fn meta(text: &str) -> Option<(String, Vec<String>)> {
        parse_meta(text)
    }

    #[test]
    fn reader_decodes_escapes() {
        let (n, f) = meta(r#"{"name":"a\"b\\c\/d","files":["x\ty\n","\b\f\r"]}"#).unwrap();
        assert_eq!(n, "a\"b\\c/d");
        assert_eq!(f, ["x\ty\n", "\u{8}\u{c}\r"]);
    }

    #[test]
    fn reader_decodes_unicode_escapes_and_surrogate_pairs() {
        let (_, f) = meta(r#"{"name":"n","files":["café","😀.exe","é"]}"#).unwrap();
        assert_eq!(f, ["caf\u{e9}", "\u{1F600}.exe", "\u{e9}"]);
        // A lone high surrogate is not a string.
        assert!(meta(r#"{"name":"n","files":["\ud83d"]}"#).is_none());
        // Raw UTF-8 passes through.
        let (_, f) = meta("{\"name\":\"n\",\"files\":[\"caf\u{e9}\"]}").unwrap();
        assert_eq!(f, ["caf\u{e9}"]);
    }

    #[test]
    fn reader_counts_only_depth_one_keys() {
        let text = r#"{"paths_data":{"name":"inner","files":["nested.exe"],
            "paths":[{"files":["x"],"_path":"y"}]},
            "n":-1.5e3,"b":true,"z":null,
            "extra":{"files":["bad"]},"name":"outer","files":["real.exe"]}"#;
        let (n, f) = meta(text).unwrap();
        assert_eq!(n, "outer");
        assert_eq!(f, ["real.exe"]);
        // A nested-only `files` doesn't count.
        assert!(meta(r#"{"name":"n","x":{"files":["a"]}}"#).is_none());
    }

    #[test]
    fn reader_rejects_truncated_and_malformed_text() {
        assert!(meta(r#"{"name":"n","files":["a","#).is_none());
        assert!(meta(r#"{"name":"n","files":["a"]"#).is_none());
        assert!(meta(r#"{"name":"n","files":["a"]} "#).is_some());
        assert!(meta(r#"{"name":"n","files":["a" "b"]}"#).is_none());
        assert!(meta("").is_none());
        assert!(meta("[]").is_none());
        assert!(meta(r#"{"name":"n","files":[]}"#).is_some_and(|(_, f)| f.is_empty()));
    }

    /// The reader agrees with `serde_json` on every real `conda-meta` file Task 1 measured.
    #[test]
    fn reader_matches_serde_json_on_the_miniforge_fixtures() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/miniforge3-conda-meta");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let text = std::fs::read_to_string(&p).unwrap();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            let want_name = v["name"].as_str().unwrap().to_string();
            let want_files: Vec<String> = v["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap().to_string())
                .collect();
            let (name, files) = parse_meta(&text).unwrap_or_else(|| panic!("{p:?}"));
            assert_eq!((name, files), (want_name, want_files), "{p:?}");
            n += 1;
        }
        assert_eq!(n, 111);
    }
}
