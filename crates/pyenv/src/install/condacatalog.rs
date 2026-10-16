//! The Windows conda table (M7c design §4): built from the publishers' own pages, by CI
//! (`examples/conda_windows_table.rs`) and by `pyenv update`. Every hash is the publisher's:
//! repo.anaconda.com's SHA256 column or Miniforge's `<asset>.sha256` file.

use super::conda::{self, Install, LinuxRow, Verdict, WIN_ARCH};
use super::fetch::Fetcher;
use std::collections::BTreeMap;

/// Installer URL → the publisher's SHA-256 (`None`: listed without one).
pub type Published = BTreeMap<String, Option<String>>;

pub struct Sources {
    pub miniconda: String,
    pub archive: String,
    /// `https://github.com/conda-forge/miniforge` (download URLs are built from it).
    pub miniforge: String,
    /// The releases API list, read with `?per_page=100&page=N`.
    pub releases: String,
}

/// Debug builds honor `RPYENV_TEST_CONDA` (as `conda::sources`): `<it>/miniconda/`,
/// `<it>/archive/`, `<it>/miniforge`, `<it>/miniforge-api/releases`; with `RPYENV_TEST_PYTHON_ORG`
/// set and it unset they name a closed local port, so a test never reaches the network. Release
/// builds ignore both.
pub fn sources() -> Sources {
    #[cfg(debug_assertions)]
    {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let test = var("RPYENV_TEST_CONDA")
            .or_else(|| var("RPYENV_TEST_PYTHON_ORG").map(|_| "http://127.0.0.1:9".to_string()));
        if let Some(b) = test {
            let b = b.trim_end_matches('/');
            return Sources {
                miniconda: format!("{b}/miniconda/"),
                archive: format!("{b}/archive/"),
                miniforge: format!("{b}/miniforge"),
                releases: format!("{b}/miniforge-api/releases"),
            };
        }
    }
    Sources {
        miniconda: "https://repo.anaconda.com/miniconda/".into(),
        archive: "https://repo.anaconda.com/archive/".into(),
        miniforge: "https://github.com/conda-forge/miniforge".into(),
        releases: "https://api.github.com/repos/conda-forge/miniforge/releases".into(),
    }
}

const WIN_SUFFIX: &str = "-Windows-x86_64.exe";

fn hrefs(html: &str) -> Vec<&str> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .collect()
}

/// The Windows x64 installers a repo.anaconda.com listing names, with the SHA-256 its row
/// gives (`conda::listing_sha`), in page order, once each.
pub fn listing_files(html: &str, folder: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for f in hrefs(html) {
        if f.ends_with(WIN_SUFFIX)
            && !f.contains('/')
            && !out.iter().any(|(u, _)| u.ends_with(&format!("/{f}")))
        {
            out.push((format!("{folder}{f}"), conda::listing_sha(html, f)));
        }
    }
    out
}

/// One page of Miniforge's releases API: each Windows x64 installer's download URL and
/// whether a `<asset>.sha256` sits beside it. A page that isn't a JSON array is an error
/// (GitHub's rate-limit answer is an object).
pub fn release_assets(json: &str, miniforge: &str) -> Result<Vec<(String, bool)>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("releases: {e}"))?;
    let releases = v.as_array().ok_or_else(|| {
        format!(
            "releases: not a list: {}",
            json.chars().take(120).collect::<String>()
        )
    })?;
    let mut out = Vec::new();
    for r in releases {
        let tag = r["tag_name"].as_str().unwrap_or_default();
        let names: Vec<&str> = r["assets"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect())
            .unwrap_or_default();
        for n in &names {
            if n.ends_with(WIN_SUFFIX) && !tag.is_empty() {
                let sidecar = names.contains(&format!("{n}.sha256").as_str());
                out.push((format!("{miniforge}/releases/download/{tag}/{n}"), sidecar));
            }
        }
    }
    Ok(out)
}

/// Both listings, every releases page, and each sidecar `.sha256` (8 at a time).
pub fn read_published(f: &Fetcher, s: &Sources) -> Result<Published, String> {
    let mut p = Published::new();
    for folder in [&s.miniconda, &s.archive] {
        let html = f
            .get_text(folder)
            .map_err(|e| format!("{folder}: {}", e.message))?;
        let files = listing_files(&html, folder);
        if files.is_empty() {
            return Err(format!(
                "{folder}: lists no Windows installers: has its layout changed?"
            ));
        }
        p.extend(files);
    }
    let mut assets = Vec::new();
    for page in 1..=20 {
        let url = format!("{}?per_page=100&page={page}", s.releases);
        let text = f
            .get_text(&url)
            .map_err(|e| format!("{url}: {}", e.message))?;
        let got = release_assets(&text, &s.miniforge)?;
        let empty = text.trim() == "[]";
        assets.extend(got);
        if empty {
            break;
        }
    }
    if assets.is_empty() {
        return Err(format!("{}: lists no Windows installers", s.releases));
    }
    let shas = std::sync::Mutex::new(Vec::new());
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..8 {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some((url, sidecar)) = assets.get(i) else {
                    break;
                };
                let sha = sidecar
                    .then(|| {
                        f.get_text(&format!("{url}.sha256"))
                            .ok()
                            .and_then(|t| conda::sha_file(&t))
                    })
                    .flatten();
                shas.lock().unwrap().push((url.clone(), sha));
            });
        }
    });
    p.extend(shas.into_inner().unwrap());
    Ok(p)
}

/// A Linux x86_64 installer URL → its Windows twin (§4.2 step 2): fragment dropped,
/// `repo.continuum.io` → `repo.anaconda.com`. `None` for any other arch.
pub fn swap_url(linux_url: &str) -> Option<String> {
    let url = linux_url.split('#').next()?;
    let stem = url.strip_suffix("-Linux-x86_64.sh")?;
    Some(format!("{stem}{WIN_SUFFIX}").replacen(
        "https://repo.continuum.io/",
        "https://repo.anaconda.com/",
        1,
    ))
}

fn release_shape(s: &str) -> bool {
    let mut parts = s.split('-');
    let (Some(v), build, None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let nums: Vec<&str> = v.split('.').collect();
    nums.len() >= 2
        && nums
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && build.is_none_or(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
}

/// Upstream's name for a Windows installer file (§4.3 step 2), or `None` when it isn't a
/// release: `Miniconda3-py313_26.1.0-1-…` → `miniconda3-3.13-26.1.0-1`;
/// `Miniforge3-27.1.0-0-…` → `miniforge3-27.1.0-0`.
pub fn name_of(file: &str) -> Option<String> {
    let stem = file.strip_suffix(WIN_SUFFIX)?;
    let (fam, rest) = stem.split_once('-')?;
    if let Some((py, release)) = rest.strip_prefix("py").and_then(|r| r.split_once('_')) {
        let (major, minor) = py.split_at_checked(1)?;
        return (major.bytes().all(|b| b.is_ascii_digit())
            && !minor.is_empty()
            && minor.bytes().all(|b| b.is_ascii_digit())
            && release_shape(release))
        .then(|| format!("{}-{major}.{minor}-{release}", fam.to_ascii_lowercase()));
    }
    release_shape(rest).then(|| format!("{}-{rest}", fam.to_ascii_lowercase()))
}

/// The rows that get `CONDA_PLUGINS_AUTO_ACCEPT_TOS=true` (M7c design §5 step 4): `miniconda3`
/// with a conda release of 25 or later, and `miniconda3-latest`.
pub fn tos(name: &str) -> bool {
    name == "miniconda3-latest"
        || (conda::family(name) == "miniconda3"
            && conda::release_key(name).is_some_and(|(r, _)| r.first().is_some_and(|m| *m >= 25)))
}

fn step_for(name: &str) -> &'static str {
    if conda::family(name).starts_with("anaconda") {
        "anaconda"
    } else {
        "miniconda"
    }
}

fn win_row(name: &str, verdict: Verdict) -> LinuxRow {
    LinuxRow {
        name: name.to_string(),
        arch: WIN_ARCH.to_string(),
        verdict,
    }
}

fn ok_row(name: &str, url: &str, sha: &str, step: &str, env: Vec<(String, String)>) -> LinuxRow {
    let file = url.rsplit('/').next().unwrap_or_default();
    win_row(
        name,
        Verdict::Ok(Install {
            package: file.trim_end_matches(".exe").to_string(),
            url: format!("{url}#{sha}"),
            step: step.to_string(),
            verify: "verify_py3".to_string(),
            env,
        }),
    )
}

fn verdict_for(
    name: &str,
    url: Option<&str>,
    p: &Published,
    step: &str,
    env: Vec<(String, String)>,
) -> LinuxRow {
    match url.and_then(|u| p.get(u).map(|s| (u, s))) {
        Some((u, Some(sha))) => ok_row(name, u, sha, step, env),
        Some(_) => win_row(name, Verdict::Unsupported("no published SHA-256".into())),
        None => win_row(name, Verdict::NoBinary(conda::family(name).to_string())),
    }
}

/// §4.2: one Windows row per Linux x86_64 row, in the same order.
pub fn from_linux(linux: &[LinuxRow], p: &Published) -> Vec<LinuxRow> {
    let by_name: BTreeMap<String, String> = p
        .keys()
        .filter_map(|u| Some((name_of(u.rsplit('/').next()?)?, u.clone())))
        .collect();
    let tos_env = |n: &str| {
        if tos(n) {
            vec![(
                "CONDA_PLUGINS_AUTO_ACCEPT_TOS".to_string(),
                "true".to_string(),
            )]
        } else {
            Vec::new()
        }
    };
    linux
        .iter()
        .filter(|r| r.arch == "Linux-x86_64")
        .map(|r| match &r.verdict {
            // The URL swap first (it keeps upstream's twins and re-releases); a miss falls back
            // to the file named backwards, as rows without a URL do.
            Verdict::Ok(i) => {
                let url = swap_url(&i.url)
                    .filter(|u| p.contains_key(u))
                    .or_else(|| by_name.get(&r.name).cloned());
                verdict_for(&r.name, url.as_deref(), p, &i.step, i.env.clone())
            }
            Verdict::OkLatest(i) => {
                let url = swap_url(&i.url);
                let unversioned = url
                    .as_deref()
                    .and_then(|u| u.rsplit('/').next())
                    .map(str::to_string);
                let served = url.as_deref().is_some_and(|u| p.contains_key(u))
                    || unversioned.as_deref().is_some_and(|f| {
                        p.keys().any(|k| {
                            k.contains("/releases/download/") && k.ends_with(&format!("/{f}"))
                        })
                    });
                match (url, served) {
                    (Some(u), true) => win_row(
                        &r.name,
                        Verdict::OkLatest(Install {
                            package: unversioned
                                .unwrap_or_default()
                                .trim_end_matches(".exe")
                                .to_string(),
                            url: u,
                            step: i.step.clone(),
                            verify: i.verify.clone(),
                            env: i.env.clone(),
                        }),
                    ),
                    _ => win_row(
                        &r.name,
                        Verdict::NoBinary(conda::family(&r.name).to_string()),
                    ),
                }
            }
            _ => verdict_for(
                &r.name,
                by_name.get(&r.name).map(String::as_str),
                p,
                step_for(&r.name),
                tos_env(&r.name),
            ),
        })
        .collect()
}

fn sha_of(r: &LinuxRow) -> Option<&str> {
    match &r.verdict {
        Verdict::Ok(i) => i.url.rsplit_once('#').map(|(_, s)| s),
        _ => None,
    }
}

/// §4.3 step 1: every `ok` row's hash read again from `p` (a row whose file `p` doesn't list
/// is kept as it was).
pub fn refresh(old: &[LinuxRow], p: &Published) -> Vec<LinuxRow> {
    old.iter()
        .map(|r| match &r.verdict {
            Verdict::Ok(i) => {
                let url = i.url.split('#').next().unwrap_or_default();
                match p.get(url) {
                    Some(Some(sha)) => ok_row(&r.name, url, sha, &i.step, i.env.clone()),
                    _ => r.clone(),
                }
            }
            _ => r.clone(),
        })
        .collect()
}

/// §4.2 steps 6-7, §4.3 step 1: a row whose hash differs from `previous` keeps the
/// previous row, unless its name is in `accept` (D8). Returns the kept names, so the caller
/// can report them.
pub fn pin_hashes(previous: &[LinuxRow], new: &mut [LinuxRow], accept: &[String]) -> Vec<String> {
    let mut changed = Vec::new();
    for r in new.iter_mut().filter(|r| !accept.contains(&r.name)) {
        if let (Some(now), Some(old)) = (sha_of(r), previous.iter().find(|o| o.name == r.name)) {
            if let Some(pinned) = sha_of(old) {
                if pinned != now {
                    changed.push(r.name.clone());
                    *r = old.clone();
                }
            }
        }
    }
    changed
}

/// §4.2 step 5: names `ok` in `previous` that are no longer `ok`.
pub fn lost(previous: &[LinuxRow], new: &[LinuxRow]) -> Vec<String> {
    previous
        .iter()
        .filter(|o| matches!(o.verdict, Verdict::Ok(_)))
        .filter(|o| {
            !new.iter()
                .any(|n| n.name == o.name && matches!(n.verdict, Verdict::Ok(_)))
        })
        .map(|o| o.name.clone())
        .collect()
}

/// §4.3 step 2: releases newer than their family's newest row, appended in name order.
pub fn add_newer(rows: &mut Vec<LinuxRow>, p: &Published) -> usize {
    let mut added = Vec::new();
    for (url, sha) in p {
        let (Some(sha), Some(name)) = (sha, url.rsplit('/').next().and_then(name_of)) else {
            continue;
        };
        if rows.iter().any(|r| r.name == name) {
            continue;
        }
        let fam = conda::family(&name);
        let newest = rows
            .iter()
            .filter(|r| conda::family(&r.name) == fam)
            .filter_map(|r| conda::release_key(&r.name))
            .max();
        let Some(key) = conda::release_key(&name) else {
            continue;
        };
        if newest.is_some_and(|n| key > n) {
            let env = if tos(&name) {
                vec![(
                    "CONDA_PLUGINS_AUTO_ACCEPT_TOS".to_string(),
                    "true".to_string(),
                )]
            } else {
                Vec::new()
            };
            added.push(ok_row(&name, url, sha, step_for(&name), env));
        }
    }
    added.sort_by(|a, b| a.name.cmp(&b.name));
    let n = added.len();
    rows.extend(added);
    n
}

fn verdict_text(v: &Verdict) -> String {
    let install = |k: &str, i: &Install| {
        let mut s = format!("{k} {} {} {} {}", i.package, i.url, i.step, i.verify);
        for (n, val) in &i.env {
            s.push_str(&format!(" {n}={val}"))
        }
        s
    };
    match v {
        Verdict::Ok(i) => install("ok", i),
        Verdict::OkLatest(i) => install("ok-latest", i),
        Verdict::NoBinary(w) => format!("no-binary {w}"),
        Verdict::Unsupported(w) => format!("unsupported {w}"),
    }
}

pub fn render(rows: &[LinuxRow], by: &str, time: &str) -> String {
    let mut s = format!("# generated {time} by {by}\n# <name>\\t<arch>\\t<verdict>\n");
    for r in rows {
        s.push_str(&format!(
            "{}\t{}\t{}\n",
            r.name,
            r.arch,
            verdict_text(&r.verdict)
        ))
    }
    s
}

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since 1970 (civil-from-days, H. Hinnant).
pub fn utc_stamp(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Now, as `utc_stamp`.
pub fn now() -> String {
    utc_stamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::conda::{parse_linux, Verdict, LINUX_TSV};

    fn linux() -> Vec<LinuxRow> {
        parse_linux(LINUX_TSV)
            .unwrap()
            .into_iter()
            .filter(|r| r.arch == "Linux-x86_64")
            .collect()
    }

    #[test]
    fn swap_url_turns_a_linux_installer_into_its_windows_twin() {
        assert_eq!(
            swap_url("https://repo.continuum.io/miniconda/Miniconda3-latest-Linux-x86_64.sh")
                .as_deref(),
            Some("https://repo.anaconda.com/miniconda/Miniconda3-latest-Windows-x86_64.exe")
        );
        assert_eq!(
            swap_url("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Linux-x86_64.sh#ab").as_deref(),
            Some("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe")
        );
        assert_eq!(
            swap_url("https://repo.anaconda.com/archive/Anaconda3-2023.03-Linux-aarch64.sh"),
            None
        );
    }

    /// Spec §3: 380 of upstream's 388 `ok` names come back from their file; the 8 below don't,
    /// and a new difference fails this test.
    #[test]
    fn naming_backwards_reproduces_upstreams_names_but_eight() {
        const KNOWN: [&str; 8] = [
            "anaconda-2.4.0",
            "anaconda-4.0.0",
            "anaconda2-5.0.0",
            "anaconda3-5.0.0",
            "miniconda-3.18.3",
            "miniconda2-4.3.27",
            "miniconda3-3.9-24.1.2-0",
            "miniconda3-4.3.27",
        ];
        let mut differ = Vec::new();
        let mut same = 0;
        for r in linux() {
            let Verdict::Ok(i) = &r.verdict else { continue };
            let file = i.url.split('#').next().unwrap().rsplit('/').next().unwrap();
            let file = file.replace("-Linux-x86_64.sh", "-Windows-x86_64.exe");
            if name_of(&file).as_deref() == Some(r.name.as_str()) {
                same += 1
            } else {
                differ.push(r.name.clone())
            }
        }
        differ.sort();
        assert_eq!(differ, KNOWN);
        assert_eq!(same, 380);
    }

    #[test]
    fn name_of_ignores_files_that_are_not_releases() {
        assert_eq!(
            name_of("Miniforge3-uninstaller-patch-Windows-x86_64.exe"),
            None
        );
        assert_eq!(name_of("Miniconda3-latest-Windows-x86_64.exe"), None);
        assert_eq!(
            name_of("Miniconda3-py313_26.1.0-1-Windows-x86_64.exe").as_deref(),
            Some("miniconda3-3.13-26.1.0-1")
        );
        assert_eq!(
            name_of("Anaconda3-2027.01-0-Windows-x86_64.exe").as_deref(),
            Some("anaconda3-2027.01-0")
        );
        assert_eq!(
            name_of("Miniforge3-27.1.0-0-Windows-x86_64.exe").as_deref(),
            Some("miniforge3-27.1.0-0")
        );
        assert_eq!(name_of("Miniforge3-27.1.0-0-Linux-x86_64.sh"), None);
    }

    /// Spec §3: the rule picks exactly the 75 Linux rows that carry the variable.
    #[test]
    fn the_tos_rule_picks_linuxs_75_names() {
        let mut by_rule = Vec::new();
        let mut by_row = Vec::new();
        for r in linux() {
            if tos(&r.name) {
                by_rule.push(r.name.clone())
            }
            if let Verdict::Ok(i) | Verdict::OkLatest(i) = &r.verdict {
                if i.env
                    .iter()
                    .any(|(k, v)| k == "CONDA_PLUGINS_AUTO_ACCEPT_TOS" && v == "true")
                {
                    by_row.push(r.name.clone())
                }
            }
        }
        assert_eq!(by_row.len(), 75);
        assert_eq!(by_rule, by_row);
    }

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn published(entries: &[(&str, Option<&str>)]) -> Published {
        entries
            .iter()
            .map(|(u, s)| (u.to_string(), s.map(str::to_string)))
            .collect()
    }

    #[test]
    fn from_linux_gives_ok_unsupported_and_no_binary() {
        let rows = parse_linux(&format!(
            "a3-1\tLinux-x86_64\tok A3-1-Linux-x86_64 https://repo.anaconda.com/archive/A3-1-Linux-x86_64.sh#{SHA_B} anaconda verify_py3\n\
             anaconda3-2022.10\tLinux-x86_64\tunsupported md5 checksum only\n\
             anaconda3-2023.03\tLinux-x86_64\tok A https://repo.anaconda.com/archive/Anaconda3-2023.03-Linux-x86_64.sh#{SHA_B} anaconda verify_py3\n"
        )).unwrap();
        let p = published(&[
            (
                "https://repo.anaconda.com/archive/A3-1-Windows-x86_64.exe",
                Some(SHA_A),
            ),
            (
                "https://repo.anaconda.com/archive/Anaconda3-2022.10-Windows-x86_64.exe",
                Some(SHA_A),
            ),
        ]);
        let w = from_linux(&rows, &p);
        assert!(
            matches!(&w[0].verdict, Verdict::Ok(i) if i.url.ends_with(&format!("A3-1-Windows-x86_64.exe#{SHA_A}")) && i.package == "A3-1-Windows-x86_64")
        );
        assert!(
            matches!(&w[1].verdict, Verdict::Ok(i) if i.step == "anaconda"),
            "{:?}",
            w[1]
        );
        assert!(matches!(&w[2].verdict, Verdict::NoBinary(_)));
        assert!(w.iter().all(|r| r.arch == "Windows-x86_64"));
        let p2 = published(&[(
            "https://repo.anaconda.com/archive/A3-1-Windows-x86_64.exe",
            None,
        )]);
        assert!(
            matches!(&from_linux(&rows[..1], &p2)[0].verdict, Verdict::Unsupported(w) if w == "no published SHA-256")
        );
    }

    /// Spec §4.2 step 6 and §4.3 step 1.
    #[test]
    fn a_changed_hash_keeps_the_pinned_one_and_is_named() {
        let row = |sha: &str| {
            format!("m3-1\tWindows-x86_64\tok M https://repo.anaconda.com/miniconda/M-Windows-x86_64.exe#{sha} miniconda verify_py3\n")
        };
        let prev = crate::install::conda::parse_windows(&row(SHA_A), true).unwrap();
        let mut new = crate::install::conda::parse_windows(&row(SHA_B), true).unwrap();
        assert_eq!(pin_hashes(&prev, &mut new, &[]), ["m3-1"]);
        assert_eq!(new, prev);
        // spec D8: an accepted name takes the publisher's new hash.
        let mut accepted = crate::install::conda::parse_windows(&row(SHA_B), true).unwrap();
        assert!(pin_hashes(&prev, &mut accepted, &["m3-1".to_string()]).is_empty());
        assert!(sha_of(&accepted[0]) == Some(SHA_B));
    }

    #[test]
    fn lost_names_rows_that_stopped_installing() {
        let prev = crate::install::conda::parse_windows(&format!("m3-1\tWindows-x86_64\tok M https://repo.anaconda.com/miniconda/M.exe#{SHA_A} miniconda verify_py3\n"), true).unwrap();
        let now = crate::install::conda::parse_windows("m3-1\tWindows-x86_64\tno-binary M\n", true)
            .unwrap();
        assert_eq!(lost(&prev, &now), ["m3-1"]);
        assert!(lost(&prev, &prev).is_empty());
    }

    #[test]
    fn add_newer_adds_only_releases_newer_than_the_family() {
        let mut rows = crate::install::conda::parse_windows(&format!(
            "miniforge3-26.7.2-0\tWindows-x86_64\tok Miniforge3-26.7.2-0-Windows-x86_64 https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe#{SHA_A} miniconda verify_py3\n"
        ), true).unwrap();
        let p = published(&[
            ("https://github.com/conda-forge/miniforge/releases/download/27.1.0-0/Miniforge3-27.1.0-0-Windows-x86_64.exe", Some(SHA_B)),
            ("https://github.com/conda-forge/miniforge/releases/download/4.9.2-7/Miniforge3-4.9.2-7-Windows-x86_64.exe", Some(SHA_B)),
            ("https://github.com/conda-forge/miniforge/releases/download/x/Miniforge3-uninstaller-patch-Windows-x86_64.exe", Some(SHA_B)),
        ]);
        assert_eq!(add_newer(&mut rows, &p), 1);
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["miniforge3-26.7.2-0", "miniforge3-27.1.0-0"]
        );
    }

    #[test]
    fn render_round_trips_through_the_parser() {
        let rows = from_linux(&linux(), &Published::new());
        let text = render(&rows, "ci", "2026-10-16T09:30:00Z");
        assert_eq!(
            crate::install::conda::generated(&text),
            Some(("2026-10-16T09:30:00Z".into(), "ci".into()))
        );
        assert_eq!(
            crate::install::conda::parse_windows(&text, true).unwrap(),
            rows
        );
    }

    #[test]
    fn utc_stamp_formats_seconds() {
        assert_eq!(utc_stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_stamp(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn listing_files_reads_windows_installers_and_their_hashes() {
        let html = format!(
            r#"<tr><td><a href="M-py313_25.9.1-3-Windows-x86_64.exe">M-py313_25.9.1-3-Windows-x86_64.exe</a></td><td class="s">88.8M</td><td>2025-11-11 12:41:47</td><td>{SHA_A}</td></tr>
<tr><td><a href="M-Linux-x86_64.sh">M-Linux-x86_64.sh</a></td><td>1M</td><td>x</td><td>{SHA_B}</td></tr>
<tr><td><a href="Old-Windows-x86_64.exe">Old-Windows-x86_64.exe</a></td><td>1M</td><td>x</td><td>9e9f</td></tr>"#
        );
        let got = listing_files(&html, "https://repo.anaconda.com/miniconda/");
        assert_eq!(
            got,
            vec![
                (
                    "https://repo.anaconda.com/miniconda/M-py313_25.9.1-3-Windows-x86_64.exe"
                        .to_string(),
                    Some(SHA_A.to_string())
                ),
                (
                    "https://repo.anaconda.com/miniconda/Old-Windows-x86_64.exe".to_string(),
                    None
                ),
            ]
        );
    }

    #[test]
    fn release_assets_reads_the_api_page() {
        let json = r#"[{"tag_name":"26.7.2-0","assets":[{"name":"Miniforge3-26.7.2-0-Windows-x86_64.exe"},{"name":"Miniforge3-26.7.2-0-Windows-x86_64.exe.sha256"},{"name":"Miniforge3-Windows-x86_64.exe"},{"name":"Miniforge3-26.7.2-0-Linux-x86_64.sh"}]}]"#;
        let got = release_assets(json, "https://github.com/conda-forge/miniforge").unwrap();
        assert_eq!(got, vec![
            ("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-26.7.2-0-Windows-x86_64.exe".to_string(), true),
            ("https://github.com/conda-forge/miniforge/releases/download/26.7.2-0/Miniforge3-Windows-x86_64.exe".to_string(), false),
        ]);
        assert!(release_assets("{\"message\":\"API rate limit exceeded\"}", "x").is_err());
    }

    #[test]
    fn tos_reads_the_release_not_the_python_part() {
        assert!(tos("miniconda3-latest"));
        assert!(tos("miniconda3-25.1.1-0"));
        assert!(!tos("miniconda3-3.9-24.1.2-0"));
        assert!(tos("miniconda3-3.13-26.1.0-1"));
        assert!(!tos("miniforge3-26.7.2-0"));
    }

    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Spec R2: with only `RPYENV_TEST_PYTHON_ORG` set, no source can reach the network.
    #[test]
    fn sources_name_a_closed_port_under_the_python_org_test_variable() {
        let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let saved = (
            std::env::var_os("RPYENV_TEST_CONDA"),
            std::env::var_os("RPYENV_TEST_PYTHON_ORG"),
        );
        std::env::remove_var("RPYENV_TEST_CONDA");
        std::env::set_var("RPYENV_TEST_PYTHON_ORG", "http://example.invalid");
        let s = sources();
        for (k, v) in [
            ("RPYENV_TEST_CONDA", saved.0),
            ("RPYENV_TEST_PYTHON_ORG", saved.1),
        ] {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
        for u in [&s.miniconda, &s.archive, &s.miniforge, &s.releases] {
            assert!(u.starts_with("http://127.0.0.1:9"), "{u}");
        }
    }
}
