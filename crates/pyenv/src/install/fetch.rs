//! Downloads with python-build's mirror and cache rules (docs/parity/pyenv-m2-reference.md,
//! "Download" and "Cache"), done in-process (plan Decision 6).

use super::checksum::sha256_file;
use super::{interrupted, InstallError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Large enough for any CPython tarball.
const MAX_DOWNLOAD: u64 = 4 << 30;
const DEFAULT_MIRROR: &str = "https://pyenv.github.io/pythons";

pub struct FetchRequest {
    /// `<package name><extension>`, as python-build names the file.
    pub file_name: String,
    /// Without the `#` fragment.
    pub url: String,
    pub sha256: String,
    pub dest_dir: PathBuf,
}

struct Mirror {
    base: String,
    default: bool,
    /// `PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM`: the mirror mimics python.org's layout.
    skip_checksum: bool,
}

pub struct Fetcher {
    agent: ureq::Agent,
    mirror: Option<Mirror>,
    cache: Option<PathBuf>,
    /// Pause before attempts 2 and 3 (zero in tests).
    pub retry_delay: Duration,
}

enum Attempt {
    Ok,
    /// Worth another try: connection errors, timeouts, HTTP 5xx, short bodies.
    Transient(String),
    Final(String),
}

impl Fetcher {
    /// `env` reads a variable. `cache` is `PYTHON_BUILD_CACHE_PATH` as `pyenv install` resolved
    /// it (an existing directory, or None).
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>, cache: Option<PathBuf>) -> Fetcher {
        let set = |k: &str| env(k).filter(|v| !v.is_empty());
        let mirror = if set("PYTHON_BUILD_SKIP_MIRROR").is_some() {
            None
        } else {
            Some(match set("PYTHON_BUILD_MIRROR_URL") {
                Some(m) => Mirror {
                    base: m.strip_suffix('/').unwrap_or(&m).to_string(),
                    default: false,
                    skip_checksum: set("PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM").is_some(),
                },
                None => Mirror {
                    base: DEFAULT_MIRROR.into(),
                    default: true,
                    skip_checksum: false,
                },
            })
        };
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(ureq::Proxy::try_from_env())
            .timeout_connect(Some(Duration::from_secs(30)))
            .build()
            .into();
        Fetcher {
            agent,
            mirror,
            cache,
            retry_delay: Duration::from_secs(1),
        }
    }

    /// The mirror URL for `url`, if any (reference "Download", step 1).
    pub fn mirror_url(&self, url: &str, sha256: &str) -> Option<String> {
        let m = self.mirror.as_ref()?;
        if m.default && url.contains("/www.python.org/") {
            return None;
        }
        if m.skip_checksum {
            let rest = url.split_once("//www.python.org/ftp/python")?.1;
            return Some(format!("{}{rest}", m.base));
        }
        Some(format!("{}/{sha256}", m.base))
    }

    /// Puts a verified `<dest_dir>/<file_name>` in place, from the cache, the mirror or `url`.
    /// Progress lines go to `say` (stderr); checksum and HTTP detail go to `log`.
    pub fn fetch(
        &self,
        req: &FetchRequest,
        log: &mut dyn Write,
        say: &mut dyn FnMut(&str),
    ) -> Result<PathBuf, InstallError> {
        let dest = req.dest_dir.join(&req.file_name);
        if let Some(cache) = &self.cache {
            let cached = cache.join(&req.file_name);
            if sha256_file(&cached).ok().as_deref() == Some(req.sha256.as_str()) {
                std::fs::copy(&cached, &dest).map_err(|e| {
                    InstallError::Message(format!(
                        "pyenv: cannot copy {} from the cache: {e}",
                        req.file_name
                    ))
                })?;
                return Ok(dest);
            }
        }
        say(&format!("Downloading {}...", req.file_name));
        let mut fetched = false;
        if let Some(m) = self.mirror_url(&req.url, &req.sha256) {
            if self.agent.head(&m).call().is_ok() {
                say(&format!("-> {m}"));
                fetched = self.download(&m, &dest, &req.sha256, &req.file_name, log)?;
            } else {
                let _ = writeln!(log, "mirror HEAD failed: {m}");
            }
        }
        if !fetched {
            say(&format!("-> {}", req.url));
            if !self.download(&req.url, &dest, &req.sha256, &req.file_name, log)? {
                say(&format!("error: failed to download {}", req.file_name));
                return Err(InstallError::Failed);
            }
        }
        if let Some(cache) = &self.cache {
            let _ = std::fs::copy(&dest, cache.join(&req.file_name));
        }
        Ok(dest)
    }

    /// Up to 3 attempts. Ok(false) when the file couldn't be fetched or didn't verify.
    fn download(
        &self,
        url: &str,
        dest: &Path,
        sha256: &str,
        file_name: &str,
        log: &mut dyn Write,
    ) -> Result<bool, InstallError> {
        let part = dest.with_file_name(format!("{file_name}.part"));
        for attempt in 1..=3 {
            if attempt > 1 {
                std::thread::sleep(self.retry_delay * (attempt - 1));
            }
            let outcome = self.attempt(url, &part);
            let _ = std::fs::remove_file(dest);
            if interrupted() {
                let _ = std::fs::remove_file(&part);
                return Err(InstallError::Interrupted);
            }
            match outcome {
                Attempt::Ok => {
                    let got = sha256_file(&part).unwrap_or_default();
                    if got == sha256 {
                        std::fs::rename(&part, dest).map_err(|e| {
                            InstallError::Message(format!(
                                "pyenv: cannot write {}: {e}",
                                dest.display()
                            ))
                        })?;
                        return Ok(true);
                    }
                    let _ = std::fs::remove_file(&part);
                    let _ = write!(
                        log,
                        "\nchecksum mismatch: {file_name} (file is corrupt)\nexpected {sha256}, got {got}\n\n"
                    );
                    return Ok(false);
                }
                Attempt::Transient(e) => {
                    let _ = writeln!(log, "{url}: {e} (attempt {attempt} of 3)");
                }
                Attempt::Final(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = writeln!(log, "{url}: {e}");
                    return Ok(false);
                }
            }
        }
        let _ = std::fs::remove_file(&part);
        Ok(false)
    }

    fn attempt(&self, url: &str, part: &Path) -> Attempt {
        if let Some(path) = url.strip_prefix("file://") {
            return match std::fs::copy(path, part) {
                Ok(_) => Attempt::Ok,
                Err(e) => Attempt::Final(e.to_string()),
            };
        }
        let mut resp = match self.agent.get(url).call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(code)) if code >= 500 => {
                return Attempt::Transient(format!("HTTP {code}"))
            }
            Err(ureq::Error::StatusCode(code)) => return Attempt::Final(format!("HTTP {code}")),
            Err(e) => return Attempt::Transient(e.to_string()),
        };
        let Ok(mut out) = std::fs::File::create(part) else {
            return Attempt::Final(format!("cannot create {}", part.display()));
        };
        let mut reader = resp.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            if interrupted() {
                return Attempt::Final("interrupted".into());
            }
            match reader.read(&mut buf) {
                Ok(0) => return Attempt::Ok,
                Ok(n) => {
                    if let Err(e) = out.write_all(&buf[..n]) {
                        return Attempt::Final(e.to_string());
                    }
                }
                Err(e) => return Attempt::Transient(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(vars: &[(&str, &str)]) -> Fetcher {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        Fetcher::from_env(
            &|k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()),
            None,
        )
    }

    #[test]
    fn the_default_mirror_skips_python_org() {
        let f = with(&[]);
        assert_eq!(
            f.mirror_url("https://www.python.org/ftp/python/3.12.0/P.tgz", "ab"),
            None
        );
        assert_eq!(
            f.mirror_url("https://ftpmirror.gnu.org/r.tgz", "ab"),
            Some("https://pyenv.github.io/pythons/ab".into())
        );
    }

    #[test]
    fn a_custom_mirror_is_used_for_python_org_too_and_loses_one_trailing_slash() {
        let f = with(&[("PYTHON_BUILD_MIRROR_URL", "https://m.example/x/")]);
        assert_eq!(
            f.mirror_url("https://www.python.org/ftp/python/3.12.0/P.tgz", "ab"),
            Some("https://m.example/x/ab".into())
        );
    }

    #[test]
    fn skip_checksum_mirrors_the_python_org_layout() {
        let f = with(&[
            ("PYTHON_BUILD_MIRROR_URL", "https://m.example/py"),
            ("PYTHON_BUILD_MIRROR_URL_SKIP_CHECKSUM", "1"),
        ]);
        assert_eq!(
            f.mirror_url("https://www.python.org/ftp/python/3.12.0/P.tgz", "ab"),
            Some("https://m.example/py/3.12.0/P.tgz".into())
        );
    }

    #[test]
    fn skip_mirror_disables_it() {
        assert_eq!(
            with(&[("PYTHON_BUILD_SKIP_MIRROR", "1")]).mirror_url("https://a/b", "ab"),
            None
        );
    }
}
