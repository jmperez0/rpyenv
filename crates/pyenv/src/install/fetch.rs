//! Downloads with python-build's mirror and cache rules (docs/parity/pyenv-m2-reference.md,
//! "Download" and "Cache"), done in-process (plan Decision 6).

use super::checksum::sha256_file;
use super::{interrupted, InstallError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

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
    /// An attempt with no new bytes for this long is abandoned and retried.
    pub stall_timeout: Duration,
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
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .build()
            .into();
        Fetcher {
            agent,
            mirror,
            cache,
            retry_delay: Duration::from_secs(1),
            stall_timeout: Duration::from_secs(60),
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
        if req.sha256.len() != 64
            || !req
                .sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(InstallError::Message(format!(
                "pyenv: invalid SHA-256 for {}: {}",
                req.file_name, req.sha256
            )));
        }
        let dest = req.dest_dir.join(&req.file_name);
        if let Some(cache) = &self.cache {
            let cached = cache.join(&req.file_name);
            if sha256_file(&cached).ok().as_deref() == Some(req.sha256.as_str()) {
                let part = dest.with_file_name(format!("{}.part", req.file_name));
                let placed =
                    std::fs::copy(&cached, &part).and_then(|_| std::fs::rename(&part, &dest));
                match placed {
                    Ok(()) => return Ok(dest),
                    Err(e) => {
                        let _ = std::fs::remove_file(&part);
                        let _ = writeln!(log, "cannot use the cached {}: {e}", req.file_name);
                    }
                }
            }
        }
        say(&format!("Downloading {}...", req.file_name));
        let mut fetched = false;
        if let Some(m) = self.mirror_url(&req.url, &req.sha256) {
            if self.agent.head(&m).call().is_ok() {
                let _ = writeln!(log, "mirror HEAD ok: {m}");
                say(&format!("-> {m}"));
                fetched = self.download(&m, &dest, req, log, say)? == Outcome::Done;
            } else {
                let _ = writeln!(log, "mirror HEAD failed: {m}");
            }
        }
        if !fetched {
            say(&format!("-> {}", req.url));
            if self.download(&req.url, &dest, req, log, say)? != Outcome::Done {
                return Err(InstallError::Failed);
            }
        }
        if let Some(cache) = &self.cache {
            if cache != &req.dest_dir {
                let tmp = cache.join(format!("{}.tmp-{}", req.file_name, std::process::id()));
                let stored = std::fs::copy(&dest, &tmp)
                    .and_then(|_| std::fs::rename(&tmp, cache.join(&req.file_name)));
                if let Err(e) = stored {
                    let _ = std::fs::remove_file(&tmp);
                    let _ = writeln!(log, "cannot store {} in the cache: {e}", req.file_name);
                }
            }
        }
        Ok(dest)
    }

    /// Up to 3 attempts at one URL. Prints `error: failed to download` when the GET finally fails.
    fn download(
        &self,
        url: &str,
        dest: &Path,
        req: &FetchRequest,
        log: &mut dyn Write,
        say: &mut dyn FnMut(&str),
    ) -> Result<Outcome, InstallError> {
        let file_name = &req.file_name;
        let sha256 = &req.sha256;
        for attempt in 1..=3u32 {
            if attempt > 1 {
                std::thread::sleep(self.retry_delay * (attempt - 1));
            }
            // A fresh name per attempt: an abandoned (stalled) worker may still hold the old one.
            let part = dest.with_file_name(format!("{file_name}.part{attempt}"));
            let outcome = self.attempt(url, &part);
            let _ = std::fs::remove_file(dest);
            if interrupted() {
                let _ = std::fs::remove_file(&part);
                return Err(InstallError::Interrupted);
            }
            match outcome {
                Attempt::Ok => {
                    let got = sha256_file(&part).unwrap_or_default();
                    if &got == sha256 {
                        if let Err(e) = std::fs::rename(&part, dest) {
                            let _ = std::fs::remove_file(&part);
                            return Err(InstallError::Message(format!(
                                "pyenv: cannot write {}: {e}",
                                dest.display()
                            )));
                        }
                        return Ok(Outcome::Done);
                    }
                    let _ = std::fs::remove_file(&part);
                    let _ = write!(
                        log,
                        "\nchecksum mismatch: {file_name} (file is corrupt)\nexpected {sha256}, got {got}\n\n"
                    );
                    return Ok(Outcome::Mismatch);
                }
                Attempt::Transient(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = writeln!(log, "{url}: {e} (attempt {attempt} of 3)");
                }
                Attempt::Final(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = writeln!(log, "{url}: {e}");
                    say(&format!("error: failed to download {file_name}"));
                    return Ok(Outcome::GetFailed);
                }
            }
        }
        say(&format!("error: failed to download {file_name}"));
        Ok(Outcome::GetFailed)
    }

    /// The GET and the body copy run on a worker thread, so that Ctrl+C and a stalled
    /// connection are noticed here even while a read blocks.
    fn attempt(&self, url: &str, part: &Path) -> Attempt {
        if let Some(path) = url.strip_prefix("file://") {
            return match std::fs::copy(path, part) {
                Ok(_) => Attempt::Ok,
                Err(e) => Attempt::Final(e.to_string()),
            };
        }
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        {
            let (agent, url, part, shared) = (
                self.agent.clone(),
                url.to_string(),
                part.to_path_buf(),
                shared.clone(),
            );
            std::thread::spawn(move || {
                let _ = tx.send(transfer(&agent, &url, &part, &shared));
            });
        }
        // The stall clock starts when the response headers arrive; before that the attempt is
        // bounded by the agent's connect and response timeouts.
        let mut last: Option<(u64, Instant)> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(done) => return done,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Attempt::Transient("download thread failed".into())
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if interrupted() {
                shared.abandoned.store(true, Ordering::SeqCst);
                return Attempt::Final("interrupted".into());
            }
            if !shared.headers.load(Ordering::SeqCst) {
                continue;
            }
            let n = shared.progress.load(Ordering::SeqCst);
            match &mut last {
                Some((seen, at)) if *seen == n => {
                    if at.elapsed() >= self.stall_timeout {
                        shared.abandoned.store(true, Ordering::SeqCst);
                        return Attempt::Transient(format!(
                            "no data for {} s",
                            self.stall_timeout.as_secs_f32()
                        ));
                    }
                }
                _ => last = Some((n, Instant::now())),
            }
        }
    }
}

/// State shared between an attempt and its worker thread.
#[derive(Default)]
struct Shared {
    /// Bytes written so far.
    progress: AtomicU64,
    /// The response headers have arrived.
    headers: AtomicBool,
    /// The attempt gave up on this worker: it must stop and remove its own `.part`.
    abandoned: AtomicBool,
}

#[derive(PartialEq, Eq)]
enum Outcome {
    Done,
    /// The GET failed for good (already reported on `say`).
    GetFailed,
    /// Downloaded, but not the expected bytes (only in the log).
    Mismatch,
}

/// Which errors are worth another try: transport, I/O and timeouts. A bad URL, TLS
/// failure, redirect trouble or the size limit would fail the same way again.
fn is_transient(e: &ureq::Error) -> bool {
    use ureq::Error::*;
    matches!(e, Io(_) | Timeout(_) | ConnectionFailed | HostNotFound)
}

/// Removes the worker's `.part` on every exit if the attempt abandoned it.
struct PartGuard<'a> {
    part: &'a Path,
    shared: &'a Shared,
}

impl Drop for PartGuard<'_> {
    fn drop(&mut self) {
        if self.shared.abandoned.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(self.part);
        }
    }
}

fn transfer(agent: &ureq::Agent, url: &str, part: &Path, shared: &Shared) -> Attempt {
    let _guard = PartGuard { part, shared };
    let mut resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) if code >= 500 => {
            return Attempt::Transient(format!("HTTP {code}"))
        }
        Err(ureq::Error::StatusCode(code)) => return Attempt::Final(format!("HTTP {code}")),
        Err(e) if is_transient(&e) => return Attempt::Transient(e.to_string()),
        Err(e) => return Attempt::Final(e.to_string()),
    };
    shared.headers.store(true, Ordering::SeqCst);
    let Ok(mut out) = std::fs::File::create(part) else {
        return Attempt::Final(format!("cannot create {}", part.display()));
    };
    let mut reader = resp.body_mut().with_config().limit(MAX_DOWNLOAD).reader();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        if shared.abandoned.load(Ordering::SeqCst) {
            return Attempt::Final("abandoned".into());
        }
        match reader.read(&mut buf) {
            Ok(0) => return Attempt::Ok,
            Ok(n) => {
                if let Err(e) = out.write_all(&buf[..n]) {
                    return Attempt::Final(e.to_string());
                }
                shared.progress.fetch_add(n as u64, Ordering::SeqCst);
            }
            Err(e) => {
                let inner = e.get_ref().and_then(|i| i.downcast_ref::<ureq::Error>());
                return match inner {
                    Some(ue) if !is_transient(ue) => Attempt::Final(e.to_string()),
                    _ => Attempt::Transient(e.to_string()),
                };
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
