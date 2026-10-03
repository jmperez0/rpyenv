//! python-build definitions (docs/parity/pyenv-m2-reference.md, "The definitions DSL"):
//! where they are found, the order `--list` prints them in, and an interpreter for exactly
//! the constructs the CPython definitions use (plan Decision 3).

use std::io::Read;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/defs.rs"));
static PATCHES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/patches.tar.gz"));

/// The python-build release the vendored definitions come from (UPSTREAM).
pub const UPSTREAM_VERSION: &str = "2.8.6";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    /// A directory from `PYTHON_BUILD_DEFINITIONS` or a plugin.
    Dir(PathBuf),
    /// A definition file named on the command line.
    Path(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Found {
    /// The definition's basename: the version name it installs as.
    pub name: String,
    pub text: String,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Tarball {
        url: String,
        fragment: Option<String>,
    },
    Git {
        url: String,
        reference: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub fetch: Fetch,
    pub steps: Vec<String>,
    /// `--if <function>`: the package is skipped when it is false (all Linux ones are).
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Definition {
    pub packages: Vec<Package>,
    /// Exported and plain assignments, in order, already expanded.
    pub vars: Vec<(String, String)>,
    pub require_gcc: bool,
}

/// `PYTHON_BUILD_DEFINITIONS` (colon-separated), then, for `pyenv install` (`plugins`), each
/// `$PYENV_ROOT/plugins/*/share/python-build`.
fn dirs(root: &Path, env: &dyn Fn(&str) -> Option<String>, plugins: bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = env("PYTHON_BUILD_DEFINITIONS")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if plugins {
        let mut found: Vec<PathBuf> = std::fs::read_dir(root.join("plugins"))
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.path().join("share").join("python-build"))
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        found.sort();
        out.extend(found);
    }
    out
}

/// `python-build --definitions`: every definition dir's files except `patches`, then the
/// built-in names, sorted with `sort_versions`, adjacent duplicates removed.
pub fn names(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    list(dirs(root, env, true))
}

/// Standalone `pyenv latest -k`: the same list without plugin directories, which only
/// `pyenv install` adds (reference "Plugin definition directories").
pub fn known(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    list(dirs(Path::new(""), env, false))
}

fn list(dirs: Vec<PathBuf>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            v.extend(
                rd.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n != "patches"),
            );
        }
    }
    v.extend(DEFINITIONS.iter().map(|(n, _)| n.to_string()));
    sort_versions(&mut v);
    v.dedup();
    v
}

/// python-build's `sort_versions`: the key replaces `+`/`-` with `.`, `.p<digit>` with
/// `.z.<digit>`, appends `.z`, then sorts `-t. -k1,1 -k2,2n -k3,3n -k4,4n -k5,5n` in C order.
pub fn sort_versions(v: &mut [String]) {
    fn key(name: &str) -> String {
        let mut k = name.replace(['+', '-'], ".");
        if let Some(i) = k
            .find(".p")
            .filter(|&i| k[i + 2..].starts_with(|c: char| c.is_ascii_digit()))
        {
            k.replace_range(i..i + 2, ".z.");
        }
        k.push_str(".z");
        k
    }
    // `sort -n` reads a leading number; text without one compares as 0.
    fn num(field: &str) -> u64 {
        let digits: String = field.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().unwrap_or(0)
    }
    fn f<'a>(v: &[&'a str], i: usize) -> &'a str {
        v.get(i).copied().unwrap_or("")
    }
    v.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        let fa: Vec<&str> = ka.split('.').collect();
        let fb: Vec<&str> = kb.split('.').collect();
        f(&fa, 0)
            .cmp(f(&fb, 0))
            .then_with(|| {
                (1..5)
                    .map(|i| num(f(&fa, i)).cmp(&num(f(&fb, i))))
                    .find(|o| o.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| format!("{ka} {a}").cmp(&format!("{kb} {b}")))
    });
}

/// Like python-build's argument 1: an existing file path, else the first definitions dir
/// that has the name, else the built-in copy.
pub fn find(root: &Path, arg: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<Found> {
    let p = Path::new(arg);
    if p.is_file() {
        let text = std::fs::read_to_string(p).ok()?;
        let name = p.file_name()?.to_string_lossy().into_owned();
        return Some(Found {
            name,
            text,
            origin: Origin::Path(p.to_path_buf()),
        });
    }
    if arg.is_empty() || arg.contains('/') {
        return None;
    }
    for d in dirs(root, env, true) {
        let f = d.join(arg);
        if f.is_file() {
            let text = std::fs::read_to_string(&f).ok()?;
            return Some(Found {
                name: arg.to_string(),
                text,
                origin: Origin::Dir(d),
            });
        }
    }
    DEFINITIONS
        .iter()
        .find(|(n, _)| *n == arg)
        .map(|(n, t)| Found {
            name: n.to_string(),
            text: t.to_string(),
            origin: Origin::Builtin,
        })
}

/// The built-in patches for `<def>/<package>/`, sorted by file name (python-build `sort -z`).
pub fn patches_for(def: &str, package: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(PATCHES));
    let Ok(entries) = ar.entries() else {
        return out;
    };
    for mut e in entries.filter_map(Result::ok) {
        let Ok(path) = e.path().map(|p| p.into_owned()) else {
            continue;
        };
        let comps: Vec<String> = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if comps.len() == 3
            && comps[0] == def
            && comps[1] == package
            && e.header().entry_type().is_file()
        {
            let mut data = Vec::new();
            if e.read_to_end(&mut data).is_ok() {
                out.push((comps[2].clone(), data));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Splits a shell line into words, honoring double and single quotes and expanding
/// `$VAR`, `${VAR}`, `${VAR:+word}` and `${VAR:-word}` (in double quotes and bare words).
fn words(line: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Vec<String>, ()> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut any = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match (quote, c) {
            (None, ' ' | '\t') => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
                i += 1;
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
                i += 1;
            }
            (Some(q), c) if c == q => {
                quote = None;
                i += 1;
            }
            (Some('\''), c) => {
                cur.push(c);
                i += 1;
            }
            (_, '$') => {
                let (text, used) = expand(&chars[i..], lookup)?;
                cur.push_str(&text);
                any = true;
                i += used;
            }
            (_, c) => {
                cur.push(c);
                any = true;
                i += 1;
            }
        }
    }
    if quote.is_some() {
        return Err(());
    }
    if any {
        out.push(cur);
    }
    Ok(out)
}

/// Expands one `$…` at the start of `s`; returns the text and the chars consumed.
fn expand(s: &[char], lookup: &dyn Fn(&str) -> Option<String>) -> Result<(String, usize), ()> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    if s.get(1) == Some(&'{') {
        let close = s.iter().position(|&c| c == '}').ok_or(())?;
        // A nested `${…}` inside the word part closes later.
        let mut depth = 0;
        let mut end = 0;
        for (j, &c) in s.iter().enumerate().skip(1) {
            if c == '{' {
                depth += 1;
            }
            if c == '}' {
                depth -= 1;
                if depth == 0 {
                    end = j;
                    break;
                }
            }
        }
        let end = if end == 0 { close } else { end };
        let body: String = s[2..end].iter().collect();
        let value = if let Some((name, word)) = body.split_once(":+") {
            match lookup(name).filter(|v| !v.is_empty()) {
                Some(_) => words(&format!("\"{word}\""), lookup)?.concat(),
                None => String::new(),
            }
        } else if let Some((name, word)) = body.split_once(":-") {
            match lookup(name).filter(|v| !v.is_empty()) {
                Some(v) => v,
                None => words(&format!("\"{word}\""), lookup)?.concat(),
            }
        } else if body.chars().all(ident) {
            lookup(&body).unwrap_or_default()
        } else {
            return Err(());
        };
        return Ok((value, end + 1));
    }
    let n = s[1..].iter().take_while(|&&c| ident(c)).count();
    if n == 0 {
        return Err(());
    }
    let name: String = s[1..1 + n].iter().collect();
    Ok((lookup(&name).unwrap_or_default(), 1 + n))
}

const DARWIN_IF: &str = r#"if [[ "Darwin" == "$(uname -s)" ]]; then"#;
const SOURCE_TWIN: &str = r#"source "${BASH_SOURCE[0]%t}""#;
const SOURCE_SIBLING: &str = r#"source "$(dirname "${BASH_SOURCE[0]}")"/"#;

/// Interprets a definition. `env` is the process environment; `sibling` finds a definition
/// named by `source` (in the same place as `found`).
pub fn parse(
    found: &Found,
    env: &dyn Fn(&str) -> Option<String>,
    sibling: &dyn Fn(&str) -> Option<Found>,
) -> Result<Definition, String> {
    let mut def = Definition::default();
    run(found, env, sibling, &mut def, 0)?;
    Ok(def)
}

fn run(
    found: &Found,
    env: &dyn Fn(&str) -> Option<String>,
    sibling: &dyn Fn(&str) -> Option<Found>,
    def: &mut Definition,
    depth: u32,
) -> Result<(), String> {
    if depth > 4 {
        return Err(format!("{}: `source` nests too deeply", found.name));
    }
    // Join backslash continuations, remembering each logical line's first physical line.
    let mut lines: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (i, raw) in found.text.lines().enumerate() {
        let (n, mut acc) = pending.take().unwrap_or((i + 1, String::new()));
        let t = raw.trim_end();
        if let Some(head) = t.strip_suffix('\\') {
            acc.push_str(head);
            acc.push(' ');
            pending = Some((n, acc));
        } else {
            acc.push_str(t);
            lines.push((n, acc));
        }
    }
    if let Some(p) = pending {
        lines.push(p);
    }
    // Branch state for `if … then … else … fi`: whether the current branch runs.
    let mut stack: Vec<(bool, bool)> = Vec::new(); // (condition, in_else)
    let active = |stack: &Vec<(bool, bool)>| stack.iter().all(|&(c, e)| c != e);
    for (n, line) in &lines {
        let t = line.trim();
        let bad = || {
            format!(
                "{}: line {n}: rpyenv cannot interpret this definition line: {t}",
                found.name
            )
        };
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        match t {
            "if has_tar_xz_support; then" => {
                stack.push((true, false));
                continue;
            }
            _ if t == DARWIN_IF => {
                stack.push((false, false));
                continue;
            }
            "else" => {
                let top = stack.last_mut().ok_or_else(bad)?;
                top.1 = true;
                continue;
            }
            "fi" => {
                stack.pop().ok_or_else(bad)?;
                continue;
            }
            _ => {}
        }
        if !active(&stack) {
            continue;
        }
        let lookup = |k: &str| {
            def.vars
                .iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .or_else(|| env(k))
        };
        if t == SOURCE_TWIN || t.starts_with(SOURCE_SIBLING) {
            let target = if t == SOURCE_TWIN {
                found.name.strip_suffix('t').ok_or_else(bad)?.to_string()
            } else {
                t[SOURCE_SIBLING.len()..].to_string()
            };
            let f = sibling(&target).ok_or_else(|| {
                format!("{}: line {n}: cannot find `{target}' to source", found.name)
            })?;
            run(&f, env, sibling, def, depth + 1)?;
            continue;
        }
        // `has_tar_xz_support && A || B` (3.4.10): rpyenv always reads xz, so A runs.
        let stmt = match t.strip_prefix("has_tar_xz_support") {
            Some(rest) => {
                let rest = rest.trim_start().strip_prefix("&&").ok_or_else(bad)?;
                rest.split(" || ")
                    .next()
                    .ok_or_else(bad)?
                    .trim()
                    .to_string()
            }
            None => t.to_string(),
        };
        let w = words(&stmt, &lookup).map_err(|_| bad())?;
        let Some(first) = w.first() else { continue };
        // An `export NAME=value` line or a plain `NAME=value` line is an assignment; anything
        // else on such a line is an error.
        if first == "export" || first.contains('=') {
            let export = first == "export";
            let assign = if export {
                w.get(1).ok_or_else(bad)?
            } else {
                first
            };
            let (k, v) = assign.split_once('=').ok_or_else(bad)?;
            if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                || w.len() > if export { 2 } else { 1 }
            {
                return Err(bad());
            }
            def.vars.push((k.to_string(), v.to_string()));
            continue;
        }
        match first.as_str() {
            "prefer_openssl3" | "prefer_openssl11" | "prefer_openssl3_to_4" => {}
            "require_gcc" => def.require_gcc = true,
            "install_package" | "install_git" => {
                let git = first == "install_git";
                let fixed = if git { 4 } else { 3 };
                if w.len() < fixed {
                    return Err(bad());
                }
                let mut rest: Vec<String> = w[fixed..].to_vec();
                let condition = match rest.iter().position(|s| s == "--if") {
                    Some(i) => {
                        let c = rest.get(i + 1).cloned().ok_or_else(bad)?;
                        rest.truncate(i);
                        Some(c)
                    }
                    None => None,
                };
                let steps = if rest.is_empty() {
                    vec!["standard".to_string()]
                } else {
                    rest
                };
                let fetch = if git {
                    Fetch::Git {
                        url: w[2].clone(),
                        reference: w[3].clone(),
                    }
                } else {
                    let (u, f) = super::checksum::split_url(&w[2]);
                    Fetch::Tarball {
                        url: u.to_string(),
                        fragment: f.map(str::to_string),
                    }
                };
                if !super::is_plain_name(&w[1]) {
                    return Err(format!(
                        "{}: line {n}: invalid package name: {}",
                        found.name, w[1]
                    ));
                }
                def.packages.push(Package {
                    name: w[1].clone(),
                    fetch,
                    steps,
                    condition,
                });
            }
            _ => return Err(bad()),
        }
    }
    if !stack.is_empty() {
        return Err(format!("{}: `if` without `fi`", found.name));
    }
    Ok(())
}
