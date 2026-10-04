//! python.org as rpyenv reads it on Windows (spec §9.1, plan M2b Decisions 1–3): the base URL,
//! nginx folder listings, the Install Manager index, and the catalog `pyenv update` builds.

use super::fetch::{Fetcher, TextError};
use super::wincatalog::{parse_code, sort_rows, Arch, Row};

pub const PYTHON_ORG: &str = "https://www.python.org/ftp/python";

/// The base installs read from: python.org. Debug builds honor `RPYENV_TEST_PYTHON_ORG` (the
/// tier-1 fake server); release builds ignore it, so the trust anchor can't be redirected.
pub fn base() -> String {
    #[cfg(debug_assertions)]
    if let Some(v) = std::env::var("RPYENV_TEST_PYTHON_ORG")
        .ok()
        .filter(|v| !v.is_empty())
    {
        return v.trim_end_matches('/').to_string();
    }
    PYTHON_ORG.to_string()
}

/// `PYTHON_BUILD_MIRROR_URL`, when set and non-empty.
pub fn mirror() -> Option<String> {
    std::env::var("PYTHON_BUILD_MIRROR_URL")
        .ok()
        .filter(|v| !v.is_empty())
}

/// pyenv-win's banner, printed by `install` and `update` before anything else (reference,
/// "The mirror banner"): one line per mirror, two spaces after `::`.
pub fn banner() -> String {
    match mirror() {
        Some(m) => format!(":: [Info] ::  Mirror: {m}\n"),
        None => ":: [Info] ::  Mirror: https://www.python.org/ftp/python\n:: [Info] ::  Mirror: https://downloads.python.org/pypy/versions.json\n:: [Info] ::  Mirror: https://api.github.com/repos/oracle/graalpython/releases\n".to_string(),
    }
}

/// `href` targets of an nginx autoindex page, in page order, without `../`, sort links,
/// absolute links or other hosts.
pub fn listing_names(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        let Some(end) = rest.find('"') else { break };
        let h = &rest[..end];
        rest = &rest[end..];
        if h.is_empty() || h.starts_with(['?', '/', '#']) || h == "../" || h.contains("://") {
            continue;
        }
        out.push(h.to_string());
    }
    out
}

fn numeric_folder(name: &str) -> Option<[u64; 3]> {
    let parts: Vec<&str> = name.split('.').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut n = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        n[i] = p.parse().ok()?;
    }
    Some(n)
}

/// The version folders of the root listing that can hold a Windows package: 2.4 and later.
pub fn version_folders(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter_map(|n| n.strip_suffix('/'))
        .filter(|n| numeric_folder(n).is_some_and(|v| v >= [2, 4, 0]))
        .map(String::from)
        .collect()
}

/// The cache rows one version folder's listing offers (Decision 3): single MSIs below 3.5,
/// one code per `<arch><pre>/` component folder from 3.5 on.
pub fn folder_rows(base: &str, folder: &str, names: &[String]) -> Vec<Row> {
    let Some(v) = numeric_folder(folder) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if v < [3, 5, 0] {
        for n in names {
            let Some(stem) = n
                .strip_prefix("python-")
                .and_then(|s| s.strip_suffix(".msi"))
            else {
                continue;
            };
            let (ver, arch) = match stem.strip_suffix(".amd64") {
                Some(v) => (v, Arch::Amd64),
                None => (stem, Arch::Win32),
            };
            let code = format!("{ver}{}", arch.suffix());
            // The file's version must be this folder's (or one of its pre-releases).
            let ok =
                parse_code(&code).is_some_and(|c| !c.ft && c.numeric == folder && c.arch == arch);
            if ok {
                rows.push(Row {
                    code,
                    file: n.clone(),
                    url: format!("{base}/{folder}/{n}"),
                    x64: arch.x64(),
                    web_install: false,
                    msi: true,
                    zip_root_dir: None,
                });
            }
        }
    } else {
        for n in names {
            let Some(dir) = n.strip_suffix('/') else {
                continue;
            };
            let (word, pre) = split_arch_dir(dir);
            let Some(arch) = word.and_then(Arch::from_word) else {
                continue;
            };
            let code = format!("{folder}{pre}{}", arch.suffix());
            if parse_code(&code).is_none() {
                continue;
            }
            let file = match arch {
                Arch::Amd64 => format!("python-{folder}{pre}-amd64.exe"),
                Arch::Arm64 => format!("python-{folder}{pre}-arm64.exe"),
                Arch::Win32 => format!("python-{folder}{pre}.exe"),
            };
            rows.push(Row {
                code,
                url: format!("{base}/{folder}/{file}"),
                file,
                x64: arch.x64(),
                web_install: false,
                msi: false,
                zip_root_dir: None,
            });
        }
    }
    rows
}

/// `amd64rc2` → (`amd64`, `rc2`); `win32` → (`win32`, ``); anything else → (None, ``).
fn split_arch_dir(dir: &str) -> (Option<&str>, &str) {
    for word in ["amd64", "win32", "arm64"] {
        if let Some(pre) = dir.strip_prefix(word) {
            let tag_end = pre.find(|c: char| c.is_ascii_digit()).unwrap_or(pre.len());
            let (w, n) = pre.split_at(tag_end);
            let pre_ok = pre.is_empty()
                || (matches!(w, "a" | "b" | "rc")
                    && !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit()));
            return if pre_ok {
                (Some(word), pre)
            } else {
                (None, "")
            };
        }
    }
    (None, "")
}

/// One `PythonCore` zip of the Install Manager index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexZip {
    /// `sort-version`: `3.12.1`, `3.15.0rc3`.
    pub version: String,
    pub ft: bool,
    pub arch: Arch,
    pub url: String,
    pub file: String,
    pub sha256: String,
}

/// The `PythonCore` zips on one index page (rows whose URL is under `base`, named
/// `python-<sort-version>[t]-<arch>.zip`, with a 64-hex SHA-256), and the next page's URL,
/// resolved against this page's folder.
pub fn index_page(
    json: &str,
    page_url: &str,
    base: &str,
) -> Result<(Vec<IndexZip>, Option<String>), String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let entries = v["versions"].as_array().ok_or("no \"versions\" array")?;
    let mut zips = Vec::new();
    for e in entries {
        if e["company"].as_str() != Some("PythonCore") {
            continue;
        }
        let (Some(url), Some(sort), Some(sha)) = (
            e["url"].as_str(),
            e["sort-version"].as_str(),
            e["hash"]["sha256"].as_str(),
        ) else {
            continue;
        };
        if sha.len() != 64 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            continue;
        }
        let Some(rest) = url.strip_prefix(base).and_then(|r| r.strip_prefix('/')) else {
            continue;
        };
        let file = rest.rsplit('/').next().unwrap_or("");
        let Some(stem) = file
            .strip_prefix("python-")
            .and_then(|s| s.strip_suffix(".zip"))
        else {
            continue;
        };
        let Some((ver, word)) = stem.rsplit_once('-') else {
            continue;
        };
        let Some(arch) = Arch::from_word(word) else {
            continue;
        };
        let (ver, ft) = match ver.strip_suffix('t') {
            Some(v) if v.ends_with(|c: char| c.is_ascii_digit()) => (v, true),
            _ => (ver, false),
        };
        if ver != sort || parse_code(ver).is_none() {
            continue;
        }
        zips.push(IndexZip {
            version: ver.to_string(),
            ft,
            arch,
            url: url.to_string(),
            file: file.to_string(),
            sha256: sha.to_string(),
        });
    }
    let next = v["next"].as_str().filter(|n| !n.is_empty()).map(|n| {
        if n.contains("://") {
            n.to_string()
        } else {
            let dir = page_url.rsplit_once('/').map_or(page_url, |(d, _)| d);
            format!("{dir}/{n}")
        }
    });
    Ok((zips, next))
}

/// Every zip on the index chain from `<base>/index-windows.json` (at most 10 pages), and
/// how many pages were read. An error names the URL that failed.
pub fn index_zips(f: &Fetcher, base: &str) -> Result<(Vec<IndexZip>, usize), (String, TextError)> {
    let mut url = format!("{base}/index-windows.json");
    let mut all = Vec::new();
    let mut seen = Vec::new();
    for _ in 0..10 {
        if seen.contains(&url) {
            break;
        }
        seen.push(url.clone());
        let text = f.get_text(&url).map_err(|e| (url.clone(), e))?;
        let (zips, next) = index_page(&text, &url, base).map_err(|m| {
            (
                url.clone(),
                TextError {
                    status: None,
                    message: m,
                },
            )
        })?;
        all.extend(zips);
        match next {
            Some(n) => url = n,
            None => break,
        }
    }
    Ok((all, seen.len()))
}

/// The folder rows plus a row for each zip code no folder offered (free-threaded builds,
/// mostly), de-duplicated by code and sorted (Decision 2).
pub fn catalog(mut rows: Vec<Row>, zips: &[IndexZip]) -> Vec<Row> {
    for z in zips {
        let code = format!(
            "{}{}{}",
            z.version,
            if z.ft { "t" } else { "" },
            z.arch.suffix()
        );
        if rows.iter().any(|r| r.code == code) {
            continue;
        }
        rows.push(Row {
            code,
            file: z.file.clone(),
            url: z.url.clone(),
            x64: z.arch.x64(),
            web_install: false,
            msi: false,
            zip_root_dir: None,
        });
    }
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| seen.insert(r.code.clone()));
    sort_rows(&mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::wincatalog::Arch;

    const NGINX: &str = "<html><body><h1>Index of /ftp/python/3.10.0/</h1><hr><pre><a href=\"../\">../</a>\n<a href=\"amd64/\">amd64/</a>   01-Oct-2021 15:16    -\n<a href=\"amd64rc2/\">amd64rc2/</a>\n<a href=\"win32/\">win32/</a>\n<a href=\"win32rc2/\">win32rc2/</a>\n<a href=\"respun/\">respun/</a>\n<a href=\"?C=M;O=A\">sort</a>\n<a href=\"python-3.10.0-amd64.exe\">python-3.10.0-amd64.exe</a>\n</pre></body></html>";

    #[test]
    fn listing_names_skip_parent_query_and_absolute_links() {
        assert_eq!(
            listing_names(NGINX),
            [
                "amd64/",
                "amd64rc2/",
                "win32/",
                "win32rc2/",
                "respun/",
                "python-3.10.0-amd64.exe"
            ]
        );
    }

    #[test]
    fn version_folders_start_at_2_4() {
        let names: Vec<String> = [
            "2.3.7/",
            "2.4/",
            "2.7.18/",
            "3.10.0/",
            "3.15.0/",
            "doc/",
            "3.x/",
            "index-windows.json",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            version_folders(&names),
            ["2.4", "2.7.18", "3.10.0", "3.15.0"]
        );
    }

    fn codes(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.code.as_str()).collect()
    }

    #[test]
    fn a_component_folder_gives_one_code_per_arch_folder() {
        let rows = folder_rows("B", "3.10.0", &listing_names(NGINX));
        assert_eq!(
            codes(&rows),
            ["3.10.0", "3.10.0rc2", "3.10.0-win32", "3.10.0rc2-win32"]
        );
        let rc = &rows[1];
        assert_eq!(
            (rc.file.as_str(), rc.url.as_str()),
            (
                "python-3.10.0rc2-amd64.exe",
                "B/3.10.0/python-3.10.0rc2-amd64.exe"
            )
        );
        assert!(rc.x64 && !rc.msi);
        let arm = folder_rows("B", "3.12.5", &["arm64/".to_string()]);
        assert_eq!(
            (arm[0].code.as_str(), arm[0].file.as_str()),
            ("3.12.5-arm", "python-3.12.5-arm64.exe")
        );
    }

    #[test]
    fn a_single_msi_folder_gives_its_msis_and_skips_odd_names() {
        let names: Vec<String> = [
            "python-2.7.18.msi",
            "python-2.7.18.msi.asc",
            "python-2.7.18.amd64.msi",
            "python-2.7.18rc1.msi",
            "python-2.4.ia64.msi",
            "python-3.0a2.x86.msi",
            "Python-2.7.18.tgz",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let rows = folder_rows("B", "2.7.18", &names);
        assert_eq!(codes(&rows), ["2.7.18-win32", "2.7.18", "2.7.18rc1-win32"]);
        assert!(rows[1].x64 && rows[1].msi);
        assert_eq!(rows[1].url, "B/2.7.18/python-2.7.18.amd64.msi");
        // A component-era folder never yields single-MSI rows, and the reverse.
        assert!(folder_rows("B", "3.5.0", &["python-3.5.0.msi".to_string()]).is_empty());
        assert!(folder_rows("B", "3.4.4", &["amd64/".to_string()]).is_empty());
    }

    fn index(next: Option<&str>, entries: &[(&str, &str, &str)]) -> String {
        let v: Vec<String> = entries
            .iter()
            .map(|(company, url, sha)| format!(
                "{{\"schema\":1,\"id\":\"x\",\"sort-version\":\"{}\",\"company\":\"{company}\",\"url\":\"{url}\",\"hash\":{{\"sha256\":\"{sha}\"}}}}",
                url.rsplit('/').next().unwrap().trim_start_matches("python-").split('-').next().unwrap().trim_end_matches('t')
            ))
            .collect();
        let next = next
            .map(|n| format!(",\"next\":\"{n}\""))
            .unwrap_or_default();
        format!("{{\"versions\":[{}]{next}}}", v.join(","))
    }

    const SHA: &str = "11a906a2f36cacaee938c048968d99aa68ec0db592693b5a0fe3b161bb280ec5";

    #[test]
    fn index_pages_yield_pythoncore_zips_and_the_next_page() {
        let b = "https://www.python.org/ftp/python";
        let json = index(
            Some("index-windows-recent.json"),
            &[
                (
                    "PythonCore",
                    "https://www.python.org/ftp/python/3.12.1/python-3.12.1-amd64.zip",
                    SHA,
                ),
                (
                    "PythonCore",
                    "https://www.python.org/ftp/python/3.13.0/python-3.13.0t-arm64.zip",
                    SHA,
                ),
                (
                    "PythonEmbed",
                    "https://www.python.org/ftp/python/3.12.1/python-3.12.1-embeddable-amd64.zip",
                    SHA,
                ),
                (
                    "PythonCore",
                    "https://api.nuget.org/v3-flatcontainer/python/3.10.11/python.3.10.11.nupkg",
                    SHA,
                ),
                (
                    "PythonCore",
                    "https://evil.example/ftp/python/3.12.1/python-3.12.1-win32.zip",
                    SHA,
                ),
                (
                    "PythonCore",
                    "https://www.python.org/ftp/python/3.12.2/python-3.12.2-win32.zip",
                    "abc",
                ),
            ],
        );
        let (zips, next) = index_page(&json, &format!("{b}/index-windows.json"), b).unwrap();
        assert_eq!(
            next.as_deref(),
            Some("https://www.python.org/ftp/python/index-windows-recent.json")
        );
        assert_eq!(zips.len(), 2, "{zips:?}");
        assert_eq!(
            (zips[0].version.as_str(), zips[0].ft, zips[0].arch),
            ("3.12.1", false, Arch::Amd64)
        );
        assert_eq!(
            (zips[1].version.as_str(), zips[1].ft, zips[1].arch),
            ("3.13.0", true, Arch::Arm64)
        );
        assert_eq!(zips[0].file, "python-3.12.1-amd64.zip");
        assert!(index_page("not json", "u", b).is_err());
    }

    #[test]
    fn the_catalog_adds_zip_only_codes_once_and_sorts() {
        let rows = folder_rows("B", "3.13.0", &["amd64/".to_string(), "win32/".to_string()]);
        let zip = |v: &str, ft: bool, arch: Arch| IndexZip {
            version: v.into(),
            ft,
            arch,
            url: format!("B/{v}/z.zip"),
            file: "z.zip".into(),
            sha256: SHA.into(),
        };
        let cat = catalog(
            rows,
            &[
                zip("3.13.0", false, Arch::Amd64),
                zip("3.13.0", true, Arch::Amd64),
                zip("3.13.0", true, Arch::Win32),
            ],
        );
        assert_eq!(
            codes(&cat),
            ["3.13.0-win32", "3.13.0", "3.13.0t-win32", "3.13.0t"]
        );
        let t = cat.iter().find(|r| r.code == "3.13.0t").unwrap();
        assert_eq!((t.url.as_str(), t.msi), ("B/3.13.0/z.zip", false));
    }
}
