//! pyenv-win's version codes and its version cache `<root>\.versions_cache.xml` (spec §9.1,
//! plan M2b Decision 2): the codes `install --list` prints, `install` checks against and
//! `latest -k` resolves among. It is pyenv-win's own format, so a cache pyenv-win wrote works
//! unchanged; `pyenv update` writes it as pyenv-win's VBScript writer does (CRLF, tabs, no
//! final newline).

use std::path::Path;

/// A build's architecture, in the order pyenv-win lists one version's builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Arch {
    Win32,
    Arm64,
    Amd64,
}

impl Arch {
    /// The code suffix: `-win32`, `-arm` (pyenv-win's name for ARM64 builds), or none.
    pub fn suffix(self) -> &'static str {
        match self {
            Arch::Win32 => "-win32",
            Arch::Arm64 => "-arm",
            Arch::Amd64 => "",
        }
    }

    /// python.org's word for it, in folder and file names.
    pub fn word(self) -> &'static str {
        match self {
            Arch::Win32 => "win32",
            Arch::Arm64 => "arm64",
            Arch::Amd64 => "amd64",
        }
    }

    pub fn x64(self) -> bool {
        self != Arch::Win32
    }

    pub fn from_word(w: &str) -> Option<Arch> {
        match w {
            "win32" => Some(Arch::Win32),
            "arm64" => Some(Arch::Arm64),
            "amd64" => Some(Arch::Amd64),
            _ => None,
        }
    }
}

/// A CPython version code: `X.Y[.Z][pre][t][-win32|-arm]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// The code as written.
    pub text: String,
    /// The version as python.org writes it: `3.10.0rc2`, `2.4.3c1`, `2.7`.
    pub version: String,
    /// The numeric part, which is also the python.org folder: `3.10.0`, `2.7`.
    pub numeric: String,
    /// Major, minor, patch (a missing patch is 0).
    pub nums: [u64; 3],
    /// `a`, `b`, `c` or `rc`, and its number.
    pub pre: Option<(String, u64)>,
    /// A free-threaded build (`t`).
    pub ft: bool,
    pub arch: Arch,
}

impl Code {
    /// pyenv-win's order: version, pre-releases before the final, then regular before
    /// free-threaded builds, then win32, arm, amd64.
    pub fn sort_key(&self) -> ([u64; 3], u8, u64, bool, Arch) {
        let (rank, n) = match &self.pre {
            None => (3, 0),
            Some((w, n)) => (
                match w.as_str() {
                    "a" => 0,
                    "b" => 1,
                    _ => 2,
                },
                *n,
            ),
        };
        (self.nums, rank, n, self.ft, self.arch)
    }

    /// The pre-release tag as python.org writes it in folder names (`rc2`), or "".
    pub fn pre_tag(&self) -> String {
        self.pre
            .as_ref()
            .map(|(w, n)| format!("{w}{n}"))
            .unwrap_or_default()
    }
}

pub fn parse_code(s: &str) -> Option<Code> {
    let (rest, arch) = if let Some(r) = s.strip_suffix("-win32") {
        (r, Arch::Win32)
    } else if let Some(r) = s.strip_suffix("-arm") {
        (r, Arch::Arm64)
    } else {
        (s, Arch::Amd64)
    };
    let (rest, ft) = match rest.strip_suffix('t') {
        Some(r) if r.ends_with(|c: char| c.is_ascii_digit()) => (r, true),
        _ => (rest, false),
    };
    let num_end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let numeric = &rest[..num_end];
    let parts: Vec<&str> = numeric.split('.').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut nums = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        nums[i] = p.parse().ok()?;
    }
    let tail = &rest[num_end..];
    let pre = if tail.is_empty() {
        None
    } else {
        let w_end = tail.find(|c: char| c.is_ascii_digit())?;
        let (w, n) = tail.split_at(w_end);
        if !matches!(w, "a" | "b" | "c" | "rc") || !n.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((w.to_string(), n.parse().ok()?))
    };
    Some(Code {
        text: s.to_string(),
        version: rest.to_string(),
        numeric: numeric.to_string(),
        nums,
        pre,
        ft,
        arch,
    })
}

/// One `<version>` row of the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub code: String,
    pub file: String,
    pub url: String,
    pub x64: bool,
    pub web_install: bool,
    pub msi: bool,
    pub zip_root_dir: Option<String>,
}

pub const DB_NAME: &str = ".versions_cache.xml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    Missing,
    /// The file has no `<version>` rows.
    Empty,
    Malformed(String),
}

pub fn read_db(root: &Path) -> Result<Vec<Row>, DbError> {
    let bytes = match std::fs::read(root.join(DB_NAME)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(DbError::Missing),
        Err(e) => return Err(DbError::Malformed(e.to_string())),
    };
    let text = String::from_utf8(bytes).map_err(|_| DbError::Malformed("not UTF-8".into()))?;
    parse_db(&text)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The text of `<tag>…</tag>` inside `block`, if present.
fn element(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(unescape(&block[start..end]))
}

fn attr(tag: &str, name: &str, default: bool) -> Result<bool, DbError> {
    let key = format!("{name}=\"");
    let Some(i) = tag.find(&key) else {
        return Ok(default);
    };
    let v = &tag[i + key.len()..];
    let v = &v[..v
        .find('"')
        .ok_or_else(|| DbError::Malformed(format!("bad {name}")))?];
    match v {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(DbError::Malformed(format!("{name}=\"{v}\""))),
    }
}

/// The schema's defaults are `x64=false`, `webInstall=false` and `msi=true`
/// (docs/parity/pyenv-win-m2-reference.md, "Format of .versions_cache.xml").
pub fn parse_db(text: &str) -> Result<Vec<Row>, DbError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(root) = text.find("<versions") else {
        return Err(DbError::Malformed("no <versions> element".into()));
    };
    let mut rest = &text[root + "<versions".len()..];
    let mut rows = Vec::new();
    while let Some(i) = rest.find("<version") {
        rest = &rest[i..];
        // `<version ` or `<version>`; `</versions>` never matches `<version`.
        let after = rest.as_bytes().get("<version".len()).copied();
        if !matches!(after, Some(b' ' | b'>' | b'\t' | b'\r' | b'\n')) {
            rest = &rest["<version".len()..];
            continue;
        }
        let tag_end = rest
            .find('>')
            .ok_or_else(|| DbError::Malformed("unclosed <version".into()))?;
        let tag = &rest[..tag_end];
        let close = rest[tag_end..]
            .find("</version>")
            .map(|c| c + tag_end)
            .ok_or_else(|| DbError::Malformed("<version> without </version>".into()))?;
        let block = &rest[tag_end + 1..close];
        let need = |t: &str| {
            element(block, t)
                .ok_or_else(|| DbError::Malformed(format!("a <version> without <{t}>")))
        };
        rows.push(Row {
            code: need("code")?,
            file: need("file")?,
            url: need("URL")?,
            x64: attr(tag, "x64", false)?,
            web_install: attr(tag, "webInstall", false)?,
            msi: attr(tag, "msi", true)?,
            zip_root_dir: element(block, "zipRootDir"),
        });
        rest = &rest[close + "</version>".len()..];
    }
    if !text.contains("</versions>") {
        return Err(DbError::Malformed("no </versions>".into()));
    }
    if rows.is_empty() {
        return Err(DbError::Empty);
    }
    Ok(rows)
}

/// The file pyenv-win's `SaveVersionsXML` writes: CRLF, tab indentation, no final newline
/// (reference, "Format of .versions_cache.xml").
pub fn render_db(rows: &[Row]) -> String {
    let b = |v: bool| if v { "true" } else { "false" };
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"no\"?>\r\n<versions>\r\n",
    );
    for r in rows {
        s.push_str(&format!(
            "\t<version x64=\"{}\" webInstall=\"{}\" msi=\"{}\">\r\n",
            b(r.x64),
            b(r.web_install),
            b(r.msi)
        ));
        s.push_str(&format!("\t\t<code>{}</code>\r\n", escape(&r.code)));
        s.push_str(&format!("\t\t<file>{}</file>\r\n", escape(&r.file)));
        s.push_str(&format!("\t\t<URL>{}</URL>\r\n", escape(&r.url)));
        if let Some(z) = &r.zip_root_dir {
            s.push_str(&format!("\t\t<zipRootDir>{}</zipRootDir>\r\n", escape(z)));
        }
        s.push_str("\t</version>\r\n");
    }
    s.push_str("</versions>");
    s
}

/// Replaces the cache whole, through a temporary file in the same folder.
pub fn write_db(root: &Path, rows: &[Row]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let tmp = root.join(format!("{DB_NAME}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, render_db(rows))?;
    std::fs::rename(&tmp, root.join(DB_NAME)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// CPython rows in pyenv-win's order; other rows (PyPy, GraalPy) after them, in their order.
pub fn sort_rows(rows: &mut Vec<Row>) {
    let (mut cpython, others): (Vec<Row>, Vec<Row>) = rows
        .drain(..)
        .partition(|r| r.zip_root_dir.is_none() && parse_code(&r.code).is_some());
    cpython.sort_by_key(|r| parse_code(&r.code).map(|c| c.sort_key()));
    rows.extend(cpython);
    rows.extend(others);
}
