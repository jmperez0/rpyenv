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

/// How a download is checked before it is used (spec §9.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// A SHA-256 its publisher published (python-build, python.org's index).
    Sha256(String),
    /// The caller verifies the file (a python.org `.asc`) before using it. No cache, no mirror.
    Caller,
}

pub struct FetchRequest {
    /// `<package name><extension>`, as python-build names the file.
    pub file_name: String,
    /// Without the `#` fragment.
    pub url: String,
    /// What makes the bytes trustworthy.
    pub check: Check,
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
    /// Whole-request deadline for `get_text`.
    pub text_timeout: Duration,
}

/// Why `get_text` failed: the HTTP status, when there was one, and a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError {
    pub status: Option<u16>,
    pub message: String,
}

enum Attempt {
    Ok,
    /// Worth another try: connection errors, timeouts, HTTP 5xx, short bodies.
    Transient(String),
    Final(String),
}

/// The certificates downloads trust on Linux.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Roots {
    /// The system's, through its platform verifier.
    Platform,
    /// Mozilla's root list compiled into rpyenv (Linux without CA certificates).
    Mozilla,
}

/// Linux (allowlist D-128): the system's certificates when it has any, where its OpenSSL looks
/// (`openssldir`, from `openssl version -d`), in the distributions' bundle files, or in
/// `SSL_CERT_FILE`/`SSL_CERT_DIR` (the check `pyenv install` reports missing certificates
/// with); else Mozilla's roots, so that a machine without them can still download (and get them
/// through the build route, M8 design §6.5). Every download is SHA-256 checked either way.
#[cfg(target_os = "linux")]
pub(crate) fn linux_roots(
    openssldir: Option<&str>,
    cert_file: Option<&str>,
    cert_dir: Option<&str>,
    exists: &dyn Fn(&str) -> bool,
    has_files: &dyn Fn(&str) -> bool,
) -> Roots {
    if super::preflight::ca_present(openssldir, cert_file, cert_dir, exists, has_files) {
        Roots::Platform
    } else {
        Roots::Mozilla
    }
}

/// The roots for this process, decided once.
#[cfg(target_os = "linux")]
fn roots() -> Roots {
    static ROOTS: std::sync::OnceLock<Roots> = std::sync::OnceLock::new();
    *ROOTS.get_or_init(|| {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let dir = std::process::Command::new("openssl")
            .args(["version", "-d"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split('"')
                    .nth(1)
                    .map(str::to_string)
            });
        let has_files = |d: &str| std::fs::read_dir(d).is_ok_and(|mut r| r.next().is_some());
        linux_roots(
            dir.as_deref(),
            var("SSL_CERT_FILE").as_deref(),
            var("SSL_CERT_DIR").as_deref(),
            &|p| Path::new(p).is_file(),
            &has_files,
        )
    })
}

fn agent() -> ureq::Agent {
    #[cfg(target_os = "linux")]
    let certs = match roots() {
        Roots::Platform => ureq::tls::RootCerts::PlatformVerifier,
        Roots::Mozilla => ureq::tls::RootCerts::WebPki,
    };
    #[cfg(not(target_os = "linux"))]
    let certs = ureq::tls::RootCerts::PlatformVerifier;
    let tls = ureq::tls::TlsConfig::builder().root_certs(certs).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(ureq::Proxy::try_from_env())
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .build()
        .into()
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
        Fetcher {
            agent: agent(),
            mirror,
            cache,
            retry_delay: Duration::from_secs(1),
            stall_timeout: Duration::from_secs(60),
            text_timeout: Duration::from_secs(60),
        }
    }

    /// No mirror and no python-build cache: pyenv-win's installs fetch from python.org only,
    /// and keep their own `install_cache` (plan M2b Decisions 8 and the base-URL constraint).
    pub fn direct() -> Fetcher {
        Fetcher {
            agent: agent(),
            mirror: None,
            cache: None,
            retry_delay: Duration::from_secs(1),
            stall_timeout: Duration::from_secs(60),
            text_timeout: Duration::from_secs(60),
        }
    }

    /// A small text resource (a folder listing, an index page): up to 3 attempts for transient
    /// failures (including a body that stalls or breaks), each bounded by `text_timeout` as a
    /// whole, at most 16 MiB, decoded as UTF-8.
    pub fn get_text(&self, url: &str) -> Result<String, TextError> {
        let mut last = TextError {
            status: None,
            message: String::new(),
        };
        for attempt in 1..=3u32 {
            if attempt > 1 {
                std::thread::sleep(self.retry_delay * (attempt - 1));
            }
            if interrupted() {
                return Err(TextError {
                    status: None,
                    message: "interrupted".into(),
                });
            }
            let called = self
                .agent
                .get(url)
                .config()
                .timeout_global(Some(self.text_timeout))
                .build()
                .call();
            let err = match called {
                Ok(mut r) => match r.body_mut().with_config().limit(16 << 20).read_to_string() {
                    Ok(text) => return Ok(text),
                    Err(e) => e,
                },
                Err(e) => e,
            };
            match err {
                ureq::Error::StatusCode(code) if code < 500 => {
                    return Err(TextError {
                        status: Some(code),
                        message: format!("HTTP {code}"),
                    })
                }
                ureq::Error::StatusCode(code) => {
                    last = TextError {
                        status: Some(code),
                        message: format!("HTTP {code}"),
                    }
                }
                e if is_transient(&e) => {
                    last = TextError {
                        status: None,
                        message: e.to_string(),
                    }
                }
                e => {
                    return Err(TextError {
                        status: None,
                        message: e.to_string(),
                    })
                }
            }
        }
        Err(last)
    }

    /// One GET with redirects off: the `Location` of a 3xx answer. Anything else is an error;
    /// there are no retries (the caller reports the failure).
    pub fn redirect_target(&self, url: &str) -> Result<String, TextError> {
        let fail = |status: Option<u16>, message: String| TextError { status, message };
        let r = self
            .agent
            .get(url)
            .config()
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .timeout_global(Some(self.text_timeout))
            .build()
            .call()
            .map_err(|e| fail(None, e.to_string()))?;
        let code = r.status().as_u16();
        if !r.status().is_redirection() {
            return Err(fail(Some(code), format!("HTTP {code}")));
        }
        r.headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| fail(Some(code), format!("HTTP {code} without a Location")))
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
        if let Check::Sha256(h) = &req.check {
            if h.len() != 64 || !h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(InstallError::Message(format!(
                    "pyenv: invalid SHA-256 for {}: {h}",
                    req.file_name
                )));
            }
        }
        let Some(dest) = super::child_of(&req.dest_dir, &req.file_name) else {
            return Err(InstallError::Message(format!(
                "pyenv: invalid file name: {}",
                req.file_name
            )));
        };
        if let (Some(cache), Check::Sha256(h)) = (&self.cache, &req.check) {
            let cached = cache.join(&req.file_name);
            if sha256_file(&cached).ok().as_deref() == Some(h.as_str()) {
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
        let mirror = match &req.check {
            Check::Sha256(h) => self.mirror_url(&req.url, h),
            Check::Caller => None,
        };
        if let Some(m) = mirror {
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
        if let (Some(cache), Check::Sha256(_)) = (&self.cache, &req.check) {
            // The temporary name is checked like `dest` (review I2).
            let tmp_name = format!("{}.tmp-{}", req.file_name, std::process::id());
            let tmp = super::child_of(cache, &tmp_name).filter(|_| cache != &req.dest_dir);
            if let Some(tmp) = tmp {
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

    /// Up to 3 attempts at one URL. When the GET finally fails, prints the reason (final review
    /// I2: a TLS failure must not hide in the log), then `error: failed to download <file>`.
    fn download(
        &self,
        url: &str,
        dest: &Path,
        req: &FetchRequest,
        log: &mut dyn Write,
        say: &mut dyn FnMut(&str),
    ) -> Result<Outcome, InstallError> {
        let file_name = &req.file_name;
        let mut last = String::new();
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
                    if let Check::Sha256(sha256) = &req.check {
                        let got = sha256_file(&part).unwrap_or_default();
                        if &got != sha256 {
                            let _ = std::fs::remove_file(&part);
                            let _ = write!(
                                log,
                                "\nchecksum mismatch: {file_name} (file is corrupt)\nexpected {sha256}, got {got}\n\n"
                            );
                            return Ok(Outcome::Mismatch);
                        }
                    }
                    if let Err(e) = std::fs::rename(&part, dest) {
                        let _ = std::fs::remove_file(&part);
                        return Err(InstallError::Message(format!(
                            "pyenv: cannot write {}: {e}",
                            dest.display()
                        )));
                    }
                    return Ok(Outcome::Done);
                }
                Attempt::Transient(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = writeln!(log, "{url}: {e} (attempt {attempt} of 3)");
                    last = e;
                }
                Attempt::Final(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = writeln!(log, "{url}: {e}");
                    say(&format!("pyenv: cannot download {url}: {e}"));
                    say(&format!("error: failed to download {file_name}"));
                    return Ok(Outcome::GetFailed);
                }
            }
        }
        say(&format!("pyenv: cannot download {url}: {last}"));
        say(&format!("error: failed to download {file_name}"));
        Ok(Outcome::GetFailed)
    }

    /// The GET and the body copy run on a worker thread, so that Ctrl+C and a stalled
    /// connection are noticed here even while a read blocks.
    fn attempt(&self, url: &str, part: &Path) -> Attempt {
        if url.starts_with("file:") {
            return match file_url_path(url) {
                Some(path) => match std::fs::copy(&path, part) {
                    Ok(_) => Attempt::Ok,
                    Err(e) => Attempt::Final(format!("{}: {e}", path.display())),
                },
                None => Attempt::Final(format!("unsupported file URL: {url}")),
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

/// The local path of a `file:` URL: `file:///p`, `file://localhost/p`, and on Windows
/// `file:///C:/p` (the leading `/` before the drive dropped), percent-decoded. Other hosts and
/// invalid escapes give `None`. Escaped separators (`%2F`, `%5C`) and NUL (`%00`) are refused,
/// and so is a raw `\` anywhere in the path, a decoded path starting `//` (UNC) or containing NUL. On Linux, `file:` URLs
/// are now percent-decoded and other hosts refused (intended); the drive-letter strip is
/// Windows-only.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') || rest.contains('\\') {
        return None;
    }
    let mut bytes = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            let v = u8::from_str_radix(hex, 16).ok()?;
            if matches!(v, b'/' | b'\\' | 0) {
                return None;
            }
            bytes.push(v);
            i += 3;
        } else {
            bytes.push(b[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(bytes).ok()?;
    if s.starts_with("//") || s.contains('\0') {
        return None;
    }
    #[cfg(windows)]
    let s = {
        let d = s.as_bytes();
        if d.len() >= 3 && d[0] == b'/' && d[1].is_ascii_alphabetic() && d[2] == b':' {
            s[1..].to_string()
        } else {
            s
        }
    };
    Some(PathBuf::from(s))
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
    let Ok(mut out) = std::fs::File::create(part) else {
        return Attempt::Final(format!("cannot create {}", part.display()));
    };
    // After the file exists, so the stall clock measures only network silence.
    shared.headers.store(true, Ordering::SeqCst);
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
    fn file_urls_are_percent_decoded_and_drive_letters_kept() {
        assert_eq!(
            file_url_path("file:///tmp/a%20b"),
            Some(PathBuf::from("/tmp/a b"))
        );
        assert_eq!(
            file_url_path("file://localhost/x"),
            Some(PathBuf::from("/x"))
        );
        #[cfg(windows)]
        assert_eq!(
            file_url_path("file:///C:/py%C3%B1/x.zip"),
            Some(PathBuf::from("C:/pyñ/x.zip"))
        );
        #[cfg(not(windows))]
        assert_eq!(
            file_url_path("file:///C:/py%C3%B1/x.zip"),
            Some(PathBuf::from("/C:/pyñ/x.zip"))
        );
        assert_eq!(file_url_path("file://host/x"), None);
        assert_eq!(file_url_path("file:///bad%zz"), None);
    }

    #[test]
    fn file_urls_cannot_name_unc_paths_or_smuggle_separators() {
        for bad in [
            "file:////h/s",
            "file:///%5C%5Ch%5Cs",
            "file:///a%2F..%2Fb",
            "file:///a%2f..%2fb",
            "file:///a%5cb",
            r"file:///\h\s",
            r"file:///\\?\C:\x",
            r"file:///C:\x",
            "file:///a%00b",
        ] {
            assert_eq!(file_url_path(bad), None, "{bad}");
        }
    }

    // allowlist D-128
    #[cfg(target_os = "linux")]
    #[test]
    fn downloads_trust_mozillas_roots_only_when_the_system_has_no_certificates() {
        let none = |_: &str| false;
        assert_eq!(linux_roots(None, None, None, &none, &none), Roots::Mozilla);
        // ca-certificates' bundle on Debian and Ubuntu.
        let debian = |p: &str| p == "/etc/ssl/certs/ca-certificates.crt";
        assert_eq!(
            linux_roots(None, None, None, &debian, &none),
            Roots::Platform
        );
        // The folder the system's OpenSSL names (`openssl version -d`).
        let dir = |d: &str| d == "/opt/ssl/certs";
        assert_eq!(
            linux_roots(Some("/opt/ssl"), None, None, &none, &dir),
            Roots::Platform
        );
        // SSL_CERT_FILE and SSL_CERT_DIR replace the system's places, as for OpenSSL.
        let mine = |p: &str| p == "/home/u/ca.pem";
        assert_eq!(
            linux_roots(None, Some("/home/u/ca.pem"), None, &mine, &none),
            Roots::Platform
        );
        assert_eq!(
            linux_roots(None, Some("/home/u/missing.pem"), None, &debian, &none),
            Roots::Mozilla
        );
        let certs = |d: &str| d == "/home/u/certs";
        assert_eq!(
            linux_roots(None, None, Some("/x:/home/u/certs"), &none, &certs),
            Roots::Platform
        );
    }

    #[test]
    fn direct_has_no_mirror() {
        assert_eq!(
            Fetcher::direct().mirror_url("https://ftpmirror.gnu.org/x", "ab"),
            None
        );
    }

    #[test]
    fn a_caller_checked_request_neither_reads_nor_writes_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(cache.join("out.bin"), b"stale").unwrap();
        std::fs::write(cache.join("other.bin"), b"x").unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"fresh").unwrap();
        let f = Fetcher::from_env(
            &|k| (k == "PYTHON_BUILD_SKIP_MIRROR").then(|| "1".to_string()),
            Some(cache.clone()),
        );
        let dest = dir.path().join("dest");
        std::fs::create_dir(&dest).unwrap();
        let url = format!(
            "file:///{}",
            src.display()
                .to_string()
                .replace('\\', "/")
                .trim_start_matches('/')
        );
        let req = FetchRequest {
            file_name: "out.bin".into(),
            url,
            check: Check::Caller,
            dest_dir: dest,
        };
        let got = f.fetch(&req, &mut Vec::new(), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(got).unwrap(), b"fresh");
        assert_eq!(std::fs::read(cache.join("out.bin")).unwrap(), b"stale");
        let mut names: Vec<_> = std::fs::read_dir(&cache)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["other.bin", "out.bin"]);
    }

    #[test]
    fn a_caller_checked_request_never_uses_the_mirror() {
        let f = with(&[("PYTHON_BUILD_MIRROR_URL", "https://m.example")]);
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"payload").unwrap();
        let url = format!(
            "file:///{}",
            src.display()
                .to_string()
                .replace('\\', "/")
                .trim_start_matches('/')
        );
        let req = FetchRequest {
            file_name: "out.bin".into(),
            url,
            check: Check::Caller,
            dest_dir: dir.path().to_path_buf(),
        };
        let mut log = Vec::new();
        let got = f.fetch(&req, &mut log, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(got).unwrap(), b"payload");
        assert!(!String::from_utf8_lossy(&log).contains("mirror"));
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

    #[test]
    fn an_abandoned_worker_removes_its_own_part_file() {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/f",
            listener.local_addr().unwrap().port()
        );
        std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut r = BufReader::new(c.try_clone().unwrap());
            loop {
                let mut l = String::new();
                if r.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
                    break;
                }
            }
            let _ = c.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd",
            );
        });
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("f.part1");
        let shared = Shared::default();
        // Abandoned before it starts: it still creates the file, then must stop and clean up.
        shared.abandoned.store(true, Ordering::SeqCst);
        let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
        assert!(matches!(
            transfer(&agent, &url, &part, &shared),
            Attempt::Final(_)
        ));
        assert!(!part.exists(), "the abandoned worker left its part file");
    }
}
