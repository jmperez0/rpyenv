//! PyPy (M7a design, docs/superpowers/specs/2026-10-14-m7a-pypy-design.md): the tables CI
//! builds from upstream's definitions and pypy.org (`crates/pyenv/pypy/`), pypy.org's checksum
//! page for a Windows release newer than the table, and `bash`'s evaluation of a user's own
//! PyPy definition.

use super::fetch::Fetcher;
use std::collections::HashMap;
#[cfg(unix)]
use std::path::Path;

static LINUX_TSV: &str = include_str!("../../pypy/linux.tsv");
static WINDOWS_TSV: &str = include_str!("../../pypy/windows.tsv");
/// The stub evaluator ci/pypy_tables.py runs over the vendored definitions.
pub const EVAL_SCRIPT: &str = include_str!("pypy_eval.sh");
pub const DOWNLOADS: &str = "https://downloads.python.org/pypy";
pub const CHECKSUMS: &str = "https://www.pypy.org/checksums.html";
/// Fewer pairs than this on the checksum page means its layout changed (1021 on 2026-10-14);
/// ci/pypy_tables.py uses the same number.
pub const MIN_PAGE_HASHES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// `url` carries `#<sha256>`, as python-build writes it.
    Ok {
        package: String,
        url: String,
        steps: Vec<String>,
    },
    NoBinary,
    Unsupported(String),
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// One line of `pypy_eval.sh`'s output.
pub fn parse_verdict(line: &str) -> Result<Verdict, String> {
    let bad = || format!("unreadable PyPy verdict: {line}");
    let mut w = line.split_whitespace();
    match w.next() {
        Some("ok") => {
            let (Some(package), Some(url)) = (w.next(), w.next()) else {
                return Err(bad());
            };
            let steps: Vec<String> = w.map(str::to_string).collect();
            let sha = url.rsplit_once('#').map(|(_, s)| s).unwrap_or("");
            if !is_sha256(sha) || !super::is_plain_name(package) || steps.is_empty() {
                return Err(bad());
            }
            Ok(Verdict::Ok {
                package: package.to_string(),
                url: url.to_string(),
                steps,
            })
        }
        Some("no-binary") if w.next().is_none() => Ok(Verdict::NoBinary),
        Some("unsupported") => Ok(Verdict::Unsupported(
            line.trim_start()["unsupported".len()..].trim().to_string(),
        )),
        _ => Err(bad()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxRow {
    pub name: String,
    pub arch: String,
    pub verdict: Verdict,
}

/// `linux.tsv`: `#` comments, then `<name>\t<arch>\t<verdict>` lines.
pub fn parse_linux(text: &str) -> Result<Vec<LinuxRow>, String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut f = l.splitn(3, '\t');
            match (f.next(), f.next(), f.next()) {
                (Some(name), Some(arch), Some(v)) if !name.is_empty() && !arch.is_empty() => {
                    Ok(LinuxRow {
                        name: name.to_string(),
                        arch: arch.to_string(),
                        verdict: parse_verdict(v)?,
                    })
                }
                _ => Err(format!("unreadable linux.tsv line: {l}")),
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinRow {
    pub code: String,
    pub url: String,
    pub sha256: String,
}

/// `windows.tsv`: `#` comments, then `<code>\t<url>\t<sha256>` lines; the URL must be the
/// code's zip in pypy.org's downloads folder.
pub fn parse_windows(text: &str) -> Result<Vec<WinRow>, String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            match f.as_slice() {
                [code, url, sha]
                    if win_code(code).is_some()
                        && is_sha256(sha)
                        && *url == format!("{DOWNLOADS}/{code}.zip") =>
                {
                    Ok(WinRow {
                        code: code.to_string(),
                        url: url.to_string(),
                        sha256: sha.to_string(),
                    })
                }
                _ => Err(format!("unreadable windows.tsv line: {l}")),
            }
        })
        .collect()
}

/// Debug builds read the tables from `RPYENV_TEST_PYPY_TABLES` (a folder with `linux.tsv` and
/// `windows.tsv`) when it is set; release builds always use the embedded ones.
fn table_text(file: &str, embedded: &'static str) -> Result<String, String> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("RPYENV_TEST_PYPY_TABLES").filter(|v| !v.is_empty()) {
        let p = std::path::PathBuf::from(dir).join(file);
        return std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()));
    }
    let _ = file;
    Ok(embedded.to_string())
}

pub fn linux_table() -> Result<Vec<LinuxRow>, String> {
    parse_linux(&table_text("linux.tsv", LINUX_TSV)?)
}

pub fn windows_table() -> Result<Vec<WinRow>, String> {
    parse_windows(&table_text("windows.tsv", WINDOWS_TSV)?)
}

pub fn linux_verdict<'a>(rows: &'a [LinuxRow], name: &str, arch: &str) -> Option<&'a Verdict> {
    rows.iter()
        .find(|r| r.name == name && r.arch == arch)
        .map(|r| &r.verdict)
}

/// python-build's `pypy_architecture` for the machine rpyenv was built for; PyPy's Linux
/// tables cover x64 and aarch64.
pub fn host_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("linux64"),
        "aarch64" => Some("linux-aarch64"),
        _ => None,
    }
}

/// The one-line definition the interpreter runs for an `Ok` verdict.
pub fn definition_text(package: &str, url: &str, steps: &[String]) -> String {
    format!(
        "install_package \"{package}\" \"{url}\" {}\n",
        steps.join(" ")
    )
}

/// pypy.org's downloads folder and checksum page. Debug builds honor `RPYENV_TEST_PYPY` (the
/// fake server: `<it>/versions.json`, `<it>/checksums.html`, `<it>/<file>`); with
/// `RPYENV_TEST_PYTHON_ORG` set and it unset they name a closed local port, so a test never
/// reaches the network. Release builds ignore both.
pub fn urls() -> (String, String) {
    #[cfg(debug_assertions)]
    {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let test = var("RPYENV_TEST_PYPY").or_else(|| {
            var("RPYENV_TEST_PYTHON_ORG").map(|_| "http://127.0.0.1:9/pypy".to_string())
        });
        if let Some(b) = test {
            let b = b.trim_end_matches('/').to_string();
            let page = format!("{b}/checksums.html");
            return (b, page);
        }
    }
    (DOWNLOADS.to_string(), CHECKSUMS.to_string())
}

fn is_pypy_file(f: &str) -> bool {
    f.starts_with("pypy")
        && [".zip", ".tar.bz2", ".tar.gz"]
            .iter()
            .any(|e| f.ends_with(e))
        && f.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// `{file: sha256}` from pypy.org's checksum page: tags removed, then each `<64 hex> <pypy
/// file>` pair of words (ci/pypy_tables.py does the same). Fewer than `MIN_PAGE_HASHES` pairs,
/// or two values for one file, is an error: the layout changed.
pub fn page_hashes(html: &str) -> Result<HashMap<String, String>, String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                text.push(' ');
            }
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if in_tag => {}
            _ => text.push(c),
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: HashMap<String, String> = HashMap::new();
    for pair in words.windows(2) {
        let (h, f) = (pair[0], pair[1]);
        if is_sha256(h) && is_pypy_file(f) {
            if let Some(prev) = out.insert(f.to_string(), h.to_string()) {
                if prev != h {
                    return Err(format!(
                        "pypy.org's checksum page gives two SHA-256 values for {f}"
                    ));
                }
            }
        }
    }
    if out.len() < MIN_PAGE_HASHES {
        return Err(format!(
            "pypy.org's checksum page has {} SHA-256 lines, fewer than {MIN_PAGE_HASHES}",
            out.len()
        ));
    }
    Ok(out)
}

/// pyenv-win's PyPy code `pypy<X>.<Y>-v<version>-win64`: (`[X, Y]`, the version's numbers).
pub fn win_code(s: &str) -> Option<([u64; 2], Vec<u64>)> {
    let rest = s.strip_prefix("pypy")?.strip_suffix("-win64")?;
    let (py, ver) = rest.split_once("-v")?;
    let (x, y) = py.split_once('.')?;
    let num = |p: &str| -> Option<u64> {
        (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    };
    let parts: Vec<u64> = ver.split('.').map(num).collect::<Option<_>>()?;
    (1..=3)
        .contains(&parts.len())
        .then_some(([num(x)?, num(y)?], parts))
}

/// Where `code`'s zip comes from and the SHA-256 it must have (spec §5.2): the table's row, or
/// for a release newer than the table, pypy.org's checksum page.
pub fn win_source(code: &str, f: &Fetcher) -> Result<(String, String), String> {
    if let Some(r) = windows_table()?.into_iter().find(|r| r.code == code) {
        let (base, _) = urls();
        // A test table names the fake server's files by their real URL; serve them from it.
        let url = r
            .url
            .strip_prefix(DOWNLOADS)
            .map(|rest| format!("{base}{rest}"))
            .unwrap_or(r.url);
        return Ok((url, r.sha256));
    }
    let (base, page) = urls();
    let newer = || {
        format!("{code} is newer than this rpyenv's PyPy table and pypy.org's checksum page couldn't be read: update rpyenv")
    };
    let html = f.get_text(&page).map_err(|_| newer())?;
    let sums = page_hashes(&html).map_err(|_| newer())?;
    let file = format!("{code}.zip");
    let sha = sums.get(&file).ok_or_else(|| {
        format!("pypy.org publishes no SHA-256 for {file}, so rpyenv won't install it")
    })?;
    Ok((format!("{base}/{file}"), sha.clone()))
}

/// A user's own PyPy definition, evaluated by `bash` with the stubs CI uses (spec §4.2).
#[cfg(unix)]
pub fn evaluate(def: &Path, arch: &str) -> Result<Verdict, String> {
    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(EVAL_SCRIPT)
        .arg("pypy_eval")
        .arg(def)
        .arg(arch)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    let name = def
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match out {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{name} needs bash to be read"))
        }
        Err(e) => Err(format!("{name}: cannot run bash: {e}")),
        Ok(o) => parse_verdict(String::from_utf8_lossy(&o.stdout).trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn verdict_lines() {
        let ok = format!(
            "ok pypy3.11-v7.3.99-linux64 https://x/p.tar.bz2#{SHA} pypy verify_py311 ensurepip"
        );
        assert_eq!(
            parse_verdict(&ok),
            Ok(Verdict::Ok {
                package: "pypy3.11-v7.3.99-linux64".into(),
                url: format!("https://x/p.tar.bz2#{SHA}"),
                steps: vec!["pypy".into(), "verify_py311".into(), "ensurepip".into()],
            })
        );
        assert_eq!(parse_verdict("no-binary"), Ok(Verdict::NoBinary));
        assert_eq!(
            parse_verdict("unsupported source build"),
            Ok(Verdict::Unsupported("source build".into()))
        );
        for bad in [
            "ok p https://x/p.tar.bz2 pypy".to_string(),
            format!("ok ../p https://x/p.tar.bz2#{SHA} pypy"),
            format!("ok p https://x/p.tar.bz2#{SHA}"),
            "no-binary extra".to_string(),
            "maybe".to_string(),
        ] {
            assert!(parse_verdict(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tables_parse_and_refuse_malformed_lines() {
        let linux = format!("# c\npypy3.11-7.3.99\tlinux64\tok p https://x/p.tar.bz2#{SHA} pypy\npypy3.11-7.3.99\tlinux-aarch64\tno-binary\n");
        let rows = parse_linux(&linux).unwrap();
        assert_eq!(
            linux_verdict(&rows, "pypy3.11-7.3.99", "linux-aarch64"),
            Some(&Verdict::NoBinary)
        );
        assert_eq!(linux_verdict(&rows, "pypy3.11-7.3.99", "s390x"), None);
        assert!(parse_linux("pypy3.11-7.3.99\tlinux64\n").is_err());
        let code = "pypy3.11-v7.3.99-win64";
        let win = format!("{code}\t{DOWNLOADS}/{code}.zip\t{SHA}\n");
        assert_eq!(parse_windows(&win).unwrap()[0].code, code);
        assert!(
            parse_windows(&format!("{code}\thttps://evil.example/{code}.zip\t{SHA}\n")).is_err()
        );
        assert!(parse_windows(&format!("{code}\t{DOWNLOADS}/{code}.zip\tabc\n")).is_err());
    }

    /// The embedded tables parse, and every vendored binary PyPy definition has a row for both
    /// architectures: the tables and the definitions can't drift apart (spec §8).
    #[test]
    fn the_embedded_tables_cover_every_vendored_pypy_definition() {
        let rows = parse_linux(LINUX_TSV).unwrap();
        let win = parse_windows(WINDOWS_TSV).unwrap();
        assert!(!win.is_empty());
        let names: Vec<&str> = super::super::defs::DEFINITIONS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| n.starts_with("pypy"))
            .collect();
        assert!(names.len() > 100, "{}", names.len());
        for n in names {
            for a in ["linux64", "linux-aarch64"] {
                assert!(linux_verdict(&rows, n, a).is_some(), "{n} {a}");
            }
        }
        assert_eq!(rows.len() % 2, 0);
    }

    #[test]
    fn the_saved_checksum_page_gives_the_same_pairs_as_the_ci_script() {
        let html = include_str!("../../tests/fixtures/pypy/checksums.html");
        // ci/test_pypy_tables.py pins the same number for the same file.
        assert_eq!(page_hashes(html).unwrap().len(), 1021);
        let few = format!("<pre>{SHA}  pypy3.11-v7.3.99-win64.zip</pre>");
        assert!(page_hashes(&few).unwrap_err().contains("fewer than 500"));
    }

    #[test]
    fn pyenv_win_pypy_codes() {
        assert_eq!(
            win_code("pypy3.11-v7.3.20-win64"),
            Some(([3, 11], vec![7, 3, 20]))
        );
        assert_eq!(
            win_code("pypy2.7-v8.0.0-win64"),
            Some(([2, 7], vec![8, 0, 0]))
        );
        for bad in [
            "pypy3.11-7.3.20",
            "pypy-c-jit-latest-win64",
            "graalpy-24.2.1-windows-amd64",
            "pypy3.11-v7.3.20-win32",
            "pypy3-v7.3.20-win64",
            "pypy3.11-v-win64",
        ] {
            assert_eq!(win_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_definition_line_reads_back_through_the_interpreter() {
        let text = definition_text(
            "pypy3.11-v7.3.99-linux64",
            &format!("https://x/p.tar.bz2#{SHA}"),
            &["pypy".into(), "verify_py311".into(), "ensurepip".into()],
        );
        let found = super::super::defs::Found {
            name: "pypy3.11-7.3.99".into(),
            text,
            origin: super::super::defs::Origin::Builtin,
        };
        let def = super::super::defs::parse(&found, &|_| None, &|_| None).unwrap();
        assert_eq!(def.packages.len(), 1);
        assert_eq!(def.packages[0].steps, ["pypy", "verify_py311", "ensurepip"]);
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_runs_the_embedded_script_with_bash() {
        let d = std::env::temp_dir().join(format!("rpyenv-pypy-eval-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let def = d.join("pypy3.11-7.3.99");
        std::fs::write(
            &def,
            format!("case \"$(pypy_architecture)\" in linux64) install_package p \"https://x/p.tar.bz2#{SHA}\" pypy ;; *) exit 1 ;; esac\n"),
        )
        .unwrap();
        assert!(matches!(evaluate(&def, "linux64"), Ok(Verdict::Ok { .. })));
        assert_eq!(evaluate(&def, "linux-aarch64"), Ok(Verdict::NoBinary));
        let _ = std::fs::remove_dir_all(&d);
    }
}
