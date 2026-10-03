//! python-build's `verify_pyXY` post-build checks (bin/python-build:2192-2390): which
//! modules are fatal and which only warn, in upstream's order.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub module: &'static str,
    /// The "Missing the …?" text.
    pub lib: &'static str,
    /// Extra words after "was not compiled".
    pub extra: Option<&'static str>,
    pub fatal: bool,
    /// tkinter is checked only when `$DISPLAY` is non-empty.
    pub needs_display: bool,
}

const fn warn(module: &'static str, lib: &'static str) -> Check {
    Check {
        module,
        lib,
        extra: None,
        fatal: false,
        needs_display: false,
    }
}
const fn fatal(module: &'static str, lib: &'static str) -> Check {
    Check {
        module,
        lib,
        extra: None,
        fatal: true,
        needs_display: false,
    }
}

/// For a `verify_pyXY` step: the `X.Y` whose `bin/python<X.Y>` must exist, and the checks.
pub fn plan(step: &str) -> Option<(String, Vec<Check>)> {
    let tag = step.strip_prefix("verify_py")?;
    if tag == "3_latest" {
        return Some(("3".into(), py3(11)));
    }
    let (major, minor) = tag.split_at(1);
    let minor: u32 = minor.parse().ok()?;
    let xy = format!("{major}.{minor}");
    let checks = match (major, minor) {
        ("2", 1..=3) => vec![
            warn("readline", "GNU readline lib"),
            fatal("binascii", "binascii"),
            warn("zlib", "zlib"),
            warn("bz2", "bzip2 lib"),
        ],
        ("2", 4..=7) => {
            let mut v = vec![
                warn("readline", "GNU readline lib"),
                fatal("zlib", "zlib"),
                warn("bz2", "bzip2 lib"),
            ];
            if minor >= 5 {
                v.push(warn("sqlite3", "SQLite3 lib"));
            }
            if minor >= 6 {
                v.push(fatal("ssl", "OpenSSL lib"));
            }
            v
        }
        ("3", 0..=16) => py3(minor),
        _ => return None,
    };
    Some((xy, checks))
}

fn py3(minor: u32) -> Vec<Check> {
    let mut v = vec![
        warn("bz2", "bzip2 lib"),
        warn("curses", "ncurses lib"),
        warn("ctypes", "libffi lib"),
        warn("readline", "GNU readline lib"),
        fatal("ssl", "OpenSSL lib"),
        warn("sqlite3", "SQLite3 lib"),
        Check {
            module: "tkinter",
            lib: "Tk toolkit",
            extra: Some("and GUI subsystem has been detected"),
            fatal: false,
            needs_display: true,
        },
        fatal("zlib", "zlib"),
    ];
    if minor >= 3 {
        v.push(warn("lzma", "lzma lib"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn py312_checks_in_upstreams_order() {
        let (xy, c) = plan("verify_py312").unwrap();
        assert_eq!(xy, "3.12");
        let order: Vec<&str> = c.iter().map(|c| c.module).collect();
        assert_eq!(
            order,
            ["bz2", "curses", "ctypes", "readline", "ssl", "sqlite3", "tkinter", "zlib", "lzma"]
        );
        assert_eq!(
            c.iter()
                .filter(|c| c.fatal)
                .map(|c| c.module)
                .collect::<Vec<_>>(),
            ["ssl", "zlib"]
        );
    }

    #[test]
    fn py27_has_fatal_zlib_and_ssl() {
        let (xy, c) = plan("verify_py27").unwrap();
        assert_eq!(xy, "2.7");
        assert_eq!(
            c.iter()
                .filter(|c| c.fatal)
                .map(|c| c.module)
                .collect::<Vec<_>>(),
            ["zlib", "ssl"]
        );
    }

    #[test]
    fn rolling_and_unknown_steps() {
        assert_eq!(plan("verify_py3_latest").unwrap().0, "3");
        assert!(plan("verify_py99").is_none());
        assert!(plan("standard").is_none());
    }
}
