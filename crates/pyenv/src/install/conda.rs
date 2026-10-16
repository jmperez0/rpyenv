//! Anaconda, Miniconda and Miniforge on Linux (M7b design, docs/superpowers/specs/
//! 2026-10-15-m7b-conda-linux-design.md): the table CI builds from upstream's definitions
//! (`crates/pyenv/conda/linux.tsv`), the publisher's own SHA-256 for a `-latest` installer, and
//! `bash`'s evaluation of a user's own conda definition.

use super::fetch::Fetcher;
#[cfg(unix)]
use std::path::Path;

pub(crate) static LINUX_TSV: &str = include_str!("../../conda/linux.tsv");
/// The stub evaluator ci/conda_tables.py runs over the vendored definitions.
pub const EVAL_SCRIPT: &str = include_str!("conda_eval.sh");
/// The only places an `ok` or `ok-latest` row's installer may come from (design D1).
pub const PREFIXES: [&str; 5] = [
    "https://repo.anaconda.com/miniconda/",
    "https://repo.anaconda.com/archive/",
    "https://repo.continuum.io/miniconda/",
    "https://repo.continuum.io/archive/",
    "https://github.com/conda-forge/miniforge/",
];
/// The definitions whose installer has no version in its name (`ok-latest` rows).
pub const LATEST: [&str; 4] = [
    "miniconda-latest",
    "miniconda2-latest",
    "miniconda3-latest",
    "miniforge3-latest",
];
pub const MINICONDA_LISTING: &str = "https://repo.anaconda.com/miniconda/";
pub const MINIFORGE: &str = "https://github.com/conda-forge/miniforge";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub package: String,
    /// `ok`: carries `#<sha256>`, as python-build writes it. `ok-latest`: no fragment yet.
    pub url: String,
    pub step: String,
    pub verify: String,
    /// The variables the definition exports, in order.
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ok(Install),
    OkLatest(Install),
    NoBinary(String),
    Unsupported(String),
}

/// A definition name of the conda family (`anaconda*`, `miniconda*`, `miniforge*`, `mambaforge*`).
pub fn is_conda_name(name: &str) -> bool {
    ["anaconda", "miniconda", "miniforge", "mambaforge"]
        .iter()
        .any(|p| name.starts_with(p))
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn plain_word(s: &str, extra: &[u8]) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || extra.contains(&b))
}

/// One line of `conda_eval.sh`'s output (or one table verdict).
pub fn parse_verdict(line: &str) -> Result<Verdict, String> {
    let bad = || format!("unreadable conda verdict: {line}");
    let mut w = line.split_whitespace();
    let kind = w.next();
    match kind {
        Some(k @ ("ok" | "ok-latest")) => {
            let (Some(package), Some(url), Some(step), Some(verify)) =
                (w.next(), w.next(), w.next(), w.next())
            else {
                return Err(bad());
            };
            let has_hash = url.rsplit_once('#').is_some_and(|(_, s)| is_sha256(s));
            let hash_ok = if k == "ok" {
                has_hash
            } else {
                !url.contains('#')
            };
            let mut env = Vec::new();
            for kv in w {
                match kv.split_once('=') {
                    Some((n, v))
                        if plain_word(n, b"")
                            && !n.as_bytes()[0].is_ascii_digit()
                            && (v.is_empty() || plain_word(v, b".,:/+@%-")) =>
                    {
                        env.push((n.to_string(), v.to_string()))
                    }
                    _ => return Err(bad()),
                }
            }
            if !hash_ok
                || !super::is_plain_name(package)
                || !plain_word(package, b".-+")
                || url.contains('"')
                || !matches!(step, "anaconda" | "miniconda")
                || !verify.starts_with("verify_py")
                || !plain_word(verify, b"")
            {
                return Err(bad());
            }
            let i = Install {
                package: package.to_string(),
                url: url.to_string(),
                step: step.to_string(),
                verify: verify.to_string(),
                env,
            };
            Ok(if k == "ok" {
                Verdict::Ok(i)
            } else {
                Verdict::OkLatest(i)
            })
        }
        Some("no-binary") => Ok(Verdict::NoBinary(
            line.trim_start()["no-binary".len()..].trim().to_string(),
        )),
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

/// `linux.tsv`: `#` comments, then `<name>\t<arch>\t<verdict>` lines. An `ok` or `ok-latest`
/// row's URL must start with one of `PREFIXES` (the trust rule).
pub fn parse_linux(text: &str) -> Result<Vec<LinuxRow>, String> {
    parse_linux_with(text, true)
}

fn parse_linux_with(text: &str, trusted_only: bool) -> Result<Vec<LinuxRow>, String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut f = l.splitn(3, '\t');
            match (f.next(), f.next(), f.next()) {
                (Some(name), Some(arch), Some(v)) if !name.is_empty() && !arch.is_empty() => {
                    let verdict = parse_verdict(v)?;
                    if let Verdict::Ok(i) | Verdict::OkLatest(i) = &verdict {
                        if trusted_only && !PREFIXES.iter().any(|p| i.url.starts_with(p)) {
                            return Err(format!(
                                "linux.tsv: {name} {arch} names a file outside the five conda download folders"
                            ));
                        }
                    }
                    Ok(LinuxRow {
                        name: name.to_string(),
                        arch: arch.to_string(),
                        verdict,
                    })
                }
                _ => Err(format!("unreadable linux.tsv line: {l}")),
            }
        })
        .collect()
}

/// Debug builds read the table from `<folder>/linux.tsv`, the folder named by
/// `RPYENV_TEST_CONDA_TABLES`, when it is
/// set; release builds always use the embedded one.
fn table_text(embedded: &'static str) -> Result<String, String> {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("RPYENV_TEST_CONDA_TABLES").filter(|v| !v.is_empty()) {
        let p = std::path::PathBuf::from(p).join("linux.tsv");
        return std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()));
    }
    Ok(embedded.to_string())
}

pub fn linux_table() -> Result<Vec<LinuxRow>, String> {
    // A test table points rows at a local server, so the prefix rule is for the embedded one.
    let overridden = cfg!(debug_assertions)
        && std::env::var_os("RPYENV_TEST_CONDA_TABLES").is_some_and(|v| !v.is_empty());
    parse_linux_with(&table_text(LINUX_TSV)?, !overridden)
}

/// M7c design §4.1: the Windows table's arch, file names and download folders.
pub const WIN_ARCH: &str = "Windows-x86_64";
pub const WIN_TABLE: &str = ".conda_versions.tsv";
pub const WIN_PREFIXES: [&str; 3] = [
    "https://repo.anaconda.com/miniconda/",
    "https://repo.anaconda.com/archive/",
    "https://github.com/conda-forge/miniforge/",
];

/// `# generated <YYYY-MM-DDTHH:MM:SSZ> by <who>` on the first line: (time, who).
pub fn generated(text: &str) -> Option<(String, String)> {
    let line = text.lines().next()?.strip_prefix("# generated ")?;
    let (time, by) = line.split_once(" by ")?;
    let b = time.as_bytes();
    let shape = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && b.iter()
            .enumerate()
            .all(|(i, c)| [4, 7, 10, 13, 16, 19].contains(&i) || c.is_ascii_digit());
    (shape && !by.is_empty()).then(|| (time.to_string(), by.to_string()))
}

/// A Windows table (§4.1): `linux.tsv`'s grammar, every row `Windows-x86_64`, and, when
/// `trusted_only`, every installer from one of `WIN_PREFIXES`.
pub fn parse_windows(text: &str, trusted_only: bool) -> Result<Vec<LinuxRow>, String> {
    let rows = parse_linux_with(text, false)?;
    for r in &rows {
        if r.arch != WIN_ARCH {
            return Err(format!("{}: arch {} in a Windows table", r.name, r.arch));
        }
        if let Verdict::Ok(i) | Verdict::OkLatest(i) = &r.verdict {
            if trusted_only && !WIN_PREFIXES.iter().any(|p| i.url.starts_with(p)) {
                return Err(format!(
                    "{}: {} is outside the three conda download folders",
                    r.name, i.url
                ));
            }
        }
    }
    Ok(rows)
}

#[derive(Debug)]
pub struct WinTable {
    pub rows: Vec<LinuxRow>,
    /// Copies that exist but couldn't be read, one line each.
    pub warnings: Vec<String>,
}

/// The shipped copy (§4.4): `<exe dir>\..\share\conda\windows.tsv`. Debug builds take
/// `<RPYENV_TEST_CONDA_TABLES>\windows.tsv` when that is set.
pub fn shipped_table_path() -> Option<std::path::PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("RPYENV_TEST_CONDA_TABLES").filter(|v| !v.is_empty()) {
        return Some(std::path::PathBuf::from(p).join("windows.tsv"));
    }
    let exe = std::env::current_exe().ok()?;
    Some(
        exe.parent()?
            .parent()?
            .join("share")
            .join("conda")
            .join("windows.tsv"),
    )
}

/// Both copies merged by name (§4.4, D8): every shipped row; the root copy's rows for names
/// the shipped copy lacks, or whose shipped row isn't installable. A reviewed row is never
/// replaced by one `pyenv update` read.
pub fn windows_table(root: &std::path::Path) -> Result<WinTable, String> {
    let overridden = cfg!(debug_assertions)
        && std::env::var_os("RPYENV_TEST_CONDA_TABLES").is_some_and(|v| !v.is_empty());
    let mut warnings = Vec::new();
    let mut read = |path: Option<std::path::PathBuf>| -> Option<Vec<LinuxRow>> {
        let path = path?;
        let text = std::fs::read_to_string(&path).ok()?;
        match generated(&text)
            .ok_or_else(|| "no `# generated` line".to_string())
            .and_then(|_| parse_windows(&text, !overridden))
        {
            Ok(rows) => Some(rows),
            Err(e) => {
                warnings.push(format!("rpyenv: ignored {}: {e}", path.display()));
                None
            }
        }
    };
    let shipped = read(shipped_table_path());
    let updated = read(Some(root.join(WIN_TABLE)));
    if shipped.is_none() && updated.is_none() {
        return Err("rpyenv has no conda version table; run pyenv update".to_string());
    }
    let mut rows = shipped.unwrap_or_default();
    for r in updated.unwrap_or_default() {
        match rows.iter_mut().find(|s| s.name == r.name) {
            Some(s) if matches!(s.verdict, Verdict::Ok(_) | Verdict::OkLatest(_)) => {}
            Some(s) => *s = r,
            None => rows.push(r),
        }
    }
    Ok(WinTable { rows, warnings })
}

pub fn linux_verdict<'a>(rows: &'a [LinuxRow], name: &str, arch: &str) -> Option<&'a Verdict> {
    rows.iter()
        .find(|r| r.name == name && r.arch == arch)
        .map(|r| &r.verdict)
}

/// The family of a conda name: everything before its first `-<digit>` (`miniconda3-4.7.12` →
/// `miniconda3`; `mambaforge-pypy3` has no version, so it is its own family).
pub fn family(name: &str) -> &str {
    name.match_indices('-')
        .find(|(i, _)| name[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
        .map_or(name, |(i, _)| &name[..i])
}

/// A conda name's release and Python as numbers, for ranking: `miniconda3-3.14-26.7.1-1` is
/// release 26.7.1-1 for Python 3.14, `miniconda3-4.7.12` and `anaconda3-2026.07-1` name only a
/// release. None for a name with a part that isn't a number.
pub(crate) fn release_key(name: &str) -> Option<(Vec<u64>, Vec<u64>)> {
    let rest = name.strip_prefix(family(name))?.strip_prefix('-')?;
    let nums =
        |s: &str| -> Option<Vec<u64>> { s.split(['.', '-']).map(|p| p.parse().ok()).collect() };
    match rest.split_once('-') {
        Some((py, release))
            if py.split_once('.').is_some_and(|(m, n)| {
                matches!(m, "2" | "3") && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
            }) =>
        {
            Some((nums(release)?, nums(py)?))
        }
        _ => Some((nums(rest)?, Vec::new())),
    }
}

/// The newest name of `name`'s family that installs on `arch` (an `ok` row), for a refusal's
/// hint. Ranked by the conda release, then the Python version: upstream's `latest` sort would
/// rank `miniconda3-4.3.30` (2017) above `miniconda3-3.14-26.7.1-1`.
pub fn newest_installable(rows: &[LinuxRow], name: &str, arch: &str) -> Option<String> {
    let fam = family(name);
    rows.iter()
        .filter(|r| r.arch == arch && matches!(r.verdict, Verdict::Ok(_)) && family(&r.name) == fam)
        .filter_map(|r| release_key(&r.name).map(|k| (k, &r.name)))
        .max()
        .map(|(_, n)| n.clone())
}

/// python-build's `anaconda_architecture` for the machine rpyenv was built for.
pub fn host_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("Linux-x86_64"),
        "aarch64" => Some("Linux-aarch64"),
        _ => None,
    }
}

/// The definition text the interpreter runs for an install: the exported variables, then the
/// `install_script` call.
pub fn definition_text(i: &Install) -> String {
    let mut s = String::new();
    for (k, v) in &i.env {
        s.push_str(&format!("export {k}={v}\n"));
    }
    s.push_str(&format!(
        "install_script \"{}\" \"{}\" {} {}\n",
        i.package, i.url, i.step, i.verify
    ));
    s
}

/// Tags replaced by spaces, then split on whitespace (as `pypy::page_hashes`).
fn page_words(html: &str) -> Vec<String> {
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
    text.split_whitespace().map(str::to_string).collect()
}

/// The SHA-256 repo.anaconda.com's listing gives for `file`: the first 64-hex word within the
/// eight after the file's name. Two different values, or none, give `None`.
pub fn listing_sha(html: &str, file: &str) -> Option<String> {
    let words = page_words(html);
    let mut found: Option<&str> = None;
    for (i, w) in words.iter().enumerate() {
        if w != file {
            continue;
        }
        // Stop at the next listed file (or the next row of this one): a row without a hash
        // cell must not take its neighbour's.
        let row = words[i + 1..].iter().take(8).take_while(|x| {
            *x != file && !(x.ends_with(".sh") || x.ends_with(".exe") || x.ends_with(".pkg"))
        });
        if let Some(h) = row.into_iter().find(|x| is_sha256(x)) {
            match found {
                Some(prev) if prev != h => return None,
                _ => found = Some(h),
            }
        }
    }
    found.map(str::to_string)
}

/// A `.sha256` file: its first word, when that is 64 lowercase hex digits.
pub fn sha_file(text: &str) -> Option<String> {
    text.split_whitespace()
        .next()
        .filter(|w| is_sha256(w))
        .map(str::to_string)
}

/// The listing page and Miniforge's release base. Debug builds honor `RPYENV_TEST_CONDA` (the
/// fake server: `<it>/miniconda/`, `<it>/miniforge/...`); with `RPYENV_TEST_PYTHON_ORG` set and
/// it unset they name a closed local port, so a test never reaches the network. Release
/// builds ignore both.
pub fn sources() -> (String, String) {
    #[cfg(debug_assertions)]
    {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let test = var("RPYENV_TEST_CONDA")
            .or_else(|| var("RPYENV_TEST_PYTHON_ORG").map(|_| "http://127.0.0.1:9".to_string()));
        if let Some(b) = test {
            let b = b.trim_end_matches('/');
            return (format!("{b}/miniconda/"), format!("{b}/miniforge"));
        }
    }
    (MINICONDA_LISTING.to_string(), MINIFORGE.to_string())
}

/// The publisher's own SHA-256 for a `-latest` installer (design §4.3 step 3, D2, D6): it
/// catches a broken download, not a compromised publisher.
pub fn latest_source(name: &str, i: &Install, arch: &str, f: &Fetcher) -> Result<Install, String> {
    let (listing, miniforge) = sources();
    let refuse = |file: &str, source: &str| {
        format!("python-build: {name}: the publisher's SHA-256 for {file} couldn't be read from {source}, so rpyenv won't install it")
    };
    let ext = if arch.starts_with("Windows") {
        "exe"
    } else {
        "sh"
    };
    let mut out = i.clone();
    if name.starts_with("miniconda") {
        let file = i.url.rsplit('/').next().unwrap_or_default().to_string();
        let html = f.get_text(&listing).map_err(|_| refuse(&file, &listing))?;
        let sha = listing_sha(&html, &file).ok_or_else(|| refuse(&file, &listing))?;
        // A test table points the installer at the fake server; the real URL stays as it is.
        out.url = format!("{}#{sha}", i.url);
        return Ok(out);
    }
    // miniforge3-latest: the release `latest` redirects to, and its versioned installer, whose
    // `.sha256` file conda-forge publishes (the unversioned asset has none).
    let latest = format!("{miniforge}/releases/latest");
    let tag = f
        .redirect_target(&latest)
        .ok()
        .and_then(|loc| {
            loc.rsplit_once("/releases/tag/")
                .map(|(_, t)| t.to_string())
        })
        .filter(|t| {
            !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
        .ok_or_else(|| refuse(&format!("Miniforge3-{arch}.{ext}"), &latest))?;
    let file = format!("Miniforge3-{tag}-{arch}.{ext}");
    let base = format!("{miniforge}/releases/download/{tag}");
    let sums = format!("{base}/{file}.sha256");
    let sha = f
        .get_text(&sums)
        .ok()
        .and_then(|t| sha_file(&t))
        .ok_or_else(|| refuse(&file, &sums))?;
    out.url = format!("{base}/{file}#{sha}");
    Ok(out)
}

/// A user's own conda definition, evaluated by `bash` with the stubs CI uses. A user's
/// definition has no publisher source for a hash, so an `ok` without one is unsupported.
#[cfg(unix)]
pub fn evaluate(def: &Path, arch: &str) -> Result<Verdict, String> {
    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(EVAL_SCRIPT)
        .arg("conda_eval")
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
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let line = text.trim();
            let mut w = line.split_whitespace();
            if w.next() == Some("ok") {
                if let Some(url) = w.nth(1).filter(|u| !u.contains('#')) {
                    return Ok(Verdict::Unsupported(format!("no sha256 in {url}")));
                }
            }
            parse_verdict(line)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const URL: &str =
        "https://repo.anaconda.com/miniconda/Miniconda3-py312_25.1.1-0-Linux-x86_64.sh";

    #[test]
    fn families() {
        assert_eq!(family("miniconda3-4.7.12"), "miniconda3");
        assert_eq!(family("miniconda3-3.14-26.7.1-1"), "miniconda3");
        assert_eq!(family("anaconda3-2026.07-1"), "anaconda3");
        assert_eq!(family("mambaforge-pypy3"), "mambaforge-pypy3");
        assert_eq!(family("mambaforge"), "mambaforge");
    }

    #[test]
    fn the_newest_installable_ranks_by_release_not_upstreams_sort() {
        let ok = || format!("ok P {URL}#{SHA} miniconda verify_py3");
        let row = |name: &str, arch: &str, verdict: String| LinuxRow {
            name: name.to_string(),
            arch: arch.to_string(),
            verdict: parse_verdict(&verdict).unwrap(),
        };
        let x = "Linux-x86_64";
        let rows = vec![
            row(
                "miniconda3-4.7.12",
                x,
                "unsupported md5 checksum only".into(),
            ),
            row("miniconda3-4.3.30", x, ok()),
            row("miniconda3-3.13-26.7.1-1", x, ok()),
            row("miniconda3-3.14-26.7.1-1", x, ok()),
            row("miniconda3-3.14-26.5.3-2", x, ok()),
            row("miniconda3-3.14-26.9.0-0", "Linux-aarch64", ok()),
            row("miniconda3-latest", x, ok()),
            row("miniconda2-4.7.12", x, ok()),
            row("anaconda3-5.3.1", x, ok()),
            row("anaconda3-2026.07-1", x, ok()),
            row("mambaforge", x, "unsupported no published hash".into()),
        ];
        let newest = |n: &str| newest_installable(&rows, n, x);
        assert_eq!(
            newest("miniconda3-4.7.12").as_deref(),
            Some("miniconda3-3.14-26.7.1-1")
        );
        assert_eq!(
            newest("anaconda3-2022.05").as_deref(),
            Some("anaconda3-2026.07-1")
        );
        assert_eq!(newest("mambaforge"), None);
        assert_eq!(
            newest_installable(&rows, "miniconda3-4.7.12", "Linux-aarch64").as_deref(),
            Some("miniconda3-3.14-26.9.0-0")
        );
    }

    #[test]
    fn verdict_lines() {
        let ok = format!(
            "ok Miniconda3-x {URL}#{SHA} miniconda verify_py312 CONDA_PLUGINS_AUTO_ACCEPT_TOS=true"
        );
        let Ok(Verdict::Ok(i)) = parse_verdict(&ok) else {
            panic!()
        };
        assert_eq!(
            (i.step.as_str(), i.verify.as_str()),
            ("miniconda", "verify_py312")
        );
        assert_eq!(
            i.env,
            vec![(
                "CONDA_PLUGINS_AUTO_ACCEPT_TOS".to_string(),
                "true".to_string()
            )]
        );
        assert!(matches!(
            parse_verdict(&format!("ok-latest P {URL} miniconda verify_py3_latest")),
            Ok(Verdict::OkLatest(_))
        ));
        assert_eq!(
            parse_verdict("no-binary Anaconda"),
            Ok(Verdict::NoBinary("Anaconda".into()))
        );
        assert_eq!(
            parse_verdict("unsupported no published hash"),
            Ok(Verdict::Unsupported("no published hash".into()))
        );
        // `ok` needs a hash, `ok-latest` must not have one; steps and verify are checked.
        assert!(parse_verdict(&format!("ok P {URL} miniconda verify_py312")).is_err());
        assert!(parse_verdict(&format!("ok-latest P {URL}#{SHA} miniconda verify_py312")).is_err());
        assert!(parse_verdict(&format!("ok P {URL}#{SHA} pypy verify_py312")).is_err());
        assert!(parse_verdict(&format!("ok P {URL}#{SHA} miniconda ensurepip")).is_err());
        assert!(parse_verdict(&format!("ok ../P {URL}#{SHA} miniconda verify_py312")).is_err());
    }

    #[test]
    fn the_table_refuses_a_row_outside_the_five_prefixes() {
        let good = format!("miniconda3-x\tLinux-x86_64\tok P {URL}#{SHA} miniconda verify_py312\n");
        assert_eq!(parse_linux(&good).unwrap().len(), 1);
        let bad = good.replace(
            "https://repo.anaconda.com/miniconda/",
            "https://example.com/",
        );
        assert!(parse_linux(&bad).unwrap_err().contains("outside"));
    }

    #[test]
    fn definition_text_exports_then_installs() {
        let i = Install {
            package: "P".into(),
            url: format!("{URL}#{SHA}"),
            step: "miniconda".into(),
            verify: "verify_py312".into(),
            env: vec![("CONDA_PLUGINS_AUTO_ACCEPT_TOS".into(), "true".into())],
        };
        assert_eq!(definition_text(&i),
                   format!("export CONDA_PLUGINS_AUTO_ACCEPT_TOS=true\ninstall_script \"P\" \"{URL}#{SHA}\" miniconda verify_py312\n"));
    }

    #[test]
    fn the_listing_page_gives_the_hash_of_one_file() {
        let html = include_str!("../../tests/fixtures/conda/miniconda.html");
        let x = listing_sha(html, "Miniconda3-latest-Linux-x86_64.sh").expect("row");
        let a = listing_sha(html, "Miniconda3-latest-Linux-aarch64.sh").expect("row");
        assert_eq!(x.len(), 64);
        assert_ne!(x, a);
        assert_eq!(listing_sha(html, "Miniconda3-latest-Linux-s390x.sh"), None);
    }

    #[test]
    fn a_hashless_row_does_not_take_the_next_rows_hash() {
        let html = format!(
            "<tr><td>A.sh</td><td>1M</td><td>2026-01-01</td><td>10:00:00</td></tr>
<tr><td>B.sh</td><td>1M</td><td>2026-01-01</td><td>10:00:00</td><td>{SHA}</td></tr>"
        );
        assert_eq!(listing_sha(&html, "A.sh"), None);
        assert_eq!(listing_sha(&html, "B.sh"), Some(SHA.to_string()));
    }

    #[test]
    fn a_sha256_file_is_its_first_word() {
        assert_eq!(
            sha_file(&format!("{SHA}  Miniforge3-26.7.2-0-Linux-x86_64.sh\n")),
            Some(SHA.to_string())
        );
        assert_eq!(sha_file("Not Found"), None);
    }

    #[test]
    fn conda_names() {
        for n in [
            "anaconda3-2024.10-1",
            "miniconda3-latest",
            "miniforge3-24.1.2-0",
            "mambaforge",
            "anaconda-2.4.0",
        ] {
            assert!(is_conda_name(n), "{n}");
        }
        for n in ["3.12.1", "pypy3.11-7.3.20", "ana"] {
            assert!(!is_conda_name(n), "{n}");
        }
    }

    /// The embedded table parses under the prefix rule, and its `ok-latest` rows are exactly
    /// the `LATEST` names.
    #[test]
    fn the_embedded_table_parses_under_the_trust_rule() {
        let rows = parse_linux(LINUX_TSV).unwrap();
        assert_eq!(rows.len(), 890);
        let mut latest: Vec<&str> = rows
            .iter()
            .filter(|r| matches!(r.verdict, Verdict::OkLatest(_)))
            .map(|r| r.name.as_str())
            .collect();
        latest.sort();
        latest.dedup();
        assert_eq!(latest, LATEST);
        assert!(rows.iter().all(|r| is_conda_name(&r.name)));
    }

    #[cfg(unix)]
    #[test]
    fn evaluate_runs_the_embedded_script_with_bash() {
        let d = std::env::temp_dir().join(format!("rpyenv-conda-eval-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let def = d.join("miniconda3-x");
        std::fs::write(
            &def,
            format!("install_script P \"{URL}#{SHA}\" miniconda verify_py312\n"),
        )
        .unwrap();
        assert!(matches!(evaluate(&def, "Linux-x86_64"), Ok(Verdict::Ok(_))));
        let nohash = d.join("miniconda3-y");
        std::fs::write(
            &nohash,
            format!("install_script P \"{URL}\" miniconda verify_py312\n"),
        )
        .unwrap();
        assert_eq!(
            evaluate(&nohash, "Linux-x86_64"),
            Ok(Verdict::Unsupported(format!("no sha256 in {URL}")))
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
