//! Which standard-library extension modules each CPython 3 minor version builds, the
//! library each needs, how to probe it, and its package per distribution (M8 design §4.2).
//! Package columns: apt, dnf, zypper, pacman, apk.

pub struct Lib {
    pub key: &'static str,
    /// Any one of these headers is enough.
    pub headers: &'static [&'static str],
    /// Alternatives for the link probe, each a list of `-l` flags.
    pub links: &'static [&'static [&'static str]],
    /// When set, `pkg-config --cflags --libs <it>` is tried too (Tk).
    pub pkg_config: Option<&'static str>,
    /// One package per distribution: apt, dnf, zypper, pacman, apk. An entry may hold several
    /// names separated by spaces; callers split on whitespace.
    pub packages: [&'static str; 5],
}

pub struct Module {
    /// What the user loses, as in the prompt: `ssl`, `tkinter`.
    pub name: &'static str,
    pub lib: &'static str,
    /// What the post-build check imports (M8 design §4.4).
    pub imports: &'static [&'static str],
    pub since: u32,
    /// The last 3.x minor that has the module.
    pub until: Option<u32>,
}

pub struct MinVersion {
    pub header: &'static str,
    /// A macro whose preprocessed value is a number (`0x30000020L`, `3045001`).
    pub expr: &'static str,
    pub need: u64,
    pub shown: &'static str,
    /// What shows the installed version as text: one string macro (`OPENSSL_VERSION_TEXT`,
    /// `SQLITE_VERSION`), or three space-separated integer macros joined with dots (zstd has
    /// no string macro). Used only to word the message (M8 design §4.2).
    pub text: &'static str,
}

pub const CA_KEY: &str = "ca-certificates";

static LIBS: &[Lib] = &[
    Lib {
        key: "openssl",
        headers: &["openssl/ssl.h"],
        links: &[&["-lssl", "-lcrypto"]],
        pkg_config: None,
        packages: [
            "libssl-dev",
            "openssl-devel",
            "libopenssl-devel",
            "openssl",
            "openssl-dev",
        ],
    },
    Lib {
        key: "zlib",
        headers: &["zlib.h"],
        links: &[&["-lz"]],
        pkg_config: None,
        packages: ["zlib1g-dev", "zlib-devel", "zlib-devel", "zlib", "zlib-dev"],
    },
    Lib {
        key: "bzip2",
        headers: &["bzlib.h"],
        links: &[&["-lbz2"]],
        pkg_config: None,
        packages: [
            "libbz2-dev",
            "bzip2-devel",
            "libbz2-devel",
            "bzip2",
            "bzip2-dev",
        ],
    },
    Lib {
        key: "xz",
        headers: &["lzma.h"],
        links: &[&["-llzma"]],
        pkg_config: None,
        packages: ["liblzma-dev", "xz-devel", "xz-devel", "xz", "xz-dev"],
    },
    Lib {
        key: "readline",
        headers: &["readline/readline.h", "editline/readline.h"],
        links: &[&["-lreadline"], &["-ledit"]],
        pkg_config: None,
        packages: [
            "libreadline-dev",
            "readline-devel",
            "readline-devel",
            "readline",
            "readline-dev",
        ],
    },
    Lib {
        key: "sqlite",
        headers: &["sqlite3.h"],
        links: &[&["-lsqlite3"]],
        pkg_config: None,
        packages: [
            "libsqlite3-dev",
            "sqlite-devel",
            "sqlite3-devel",
            "sqlite",
            "sqlite-dev",
        ],
    },
    Lib {
        key: "libffi",
        headers: &["ffi.h"],
        links: &[&["-lffi"]],
        pkg_config: None,
        packages: [
            "libffi-dev",
            "libffi-devel",
            "libffi-devel",
            "libffi",
            "libffi-dev",
        ],
    },
    Lib {
        key: "ncurses",
        headers: &["ncursesw/ncurses.h", "ncurses.h", "curses.h"],
        links: &[&["-lncursesw", "-lpanelw"], &["-lncurses", "-lpanel"]],
        pkg_config: None,
        packages: [
            "libncurses-dev",
            "ncurses-devel",
            "ncurses-devel",
            "ncurses",
            "ncurses-dev",
        ],
    },
    Lib {
        key: "tk",
        headers: &["tk.h"],
        links: &[&["-ltk8.6", "-ltcl8.6"], &["-ltk", "-ltcl"]],
        pkg_config: Some("tk"),
        packages: ["tk-dev", "tk-devel", "tk-devel", "tk", "tk-dev"],
    },
    Lib {
        key: "gdbm",
        headers: &["gdbm.h"],
        links: &[&["-lgdbm", "-lgdbm_compat"]],
        pkg_config: None,
        packages: [
            "libgdbm-dev libgdbm-compat-dev",
            "gdbm-devel",
            "gdbm-devel",
            "gdbm",
            "gdbm-dev",
        ],
    },
    Lib {
        key: "libuuid",
        headers: &["uuid/uuid.h"],
        links: &[&["-luuid"]],
        pkg_config: None,
        packages: [
            "uuid-dev",
            "libuuid-devel",
            "libuuid-devel",
            "util-linux-libs",
            "util-linux-dev",
        ],
    },
    Lib {
        key: "libcrypt",
        headers: &["crypt.h"],
        links: &[&["-lcrypt"]],
        pkg_config: None,
        packages: [
            "libcrypt-dev",
            "libxcrypt-devel",
            "libxcrypt-devel",
            "libxcrypt",
            "libxcrypt-dev",
        ],
    },
    Lib {
        key: "libnsl",
        headers: &["rpcsvc/yp_prot.h"],
        links: &[&["-lnsl", "-ltirpc"]],
        // yp_prot.h includes <rpc/rpc.h>, which libtirpc keeps in /usr/include/tirpc.
        pkg_config: Some("libtirpc"),
        packages: [
            "libnsl-dev libtirpc-dev",
            "libnsl2-devel libtirpc-devel",
            "libnsl-devel libtirpc-devel",
            "libnsl libtirpc",
            "libnsl-dev libtirpc-dev",
        ],
    },
    Lib {
        key: "zstd",
        headers: &["zstd.h"],
        links: &[&["-lzstd"]],
        pkg_config: None,
        packages: [
            "libzstd-dev",
            "libzstd-devel",
            "libzstd-devel",
            "zstd",
            "zstd-dev",
        ],
    },
    Lib {
        key: CA_KEY,
        headers: &[],
        links: &[],
        pkg_config: None,
        packages: [
            "ca-certificates",
            "ca-certificates",
            "ca-certificates",
            "ca-certificates",
            "ca-certificates",
        ],
    },
];

static MODULES: &[Module] = &[
    Module {
        name: "ssl",
        lib: "openssl",
        imports: &["ssl", "_hashlib"],
        since: 0,
        until: None,
    },
    Module {
        name: "zlib",
        lib: "zlib",
        imports: &["zlib"],
        since: 0,
        until: None,
    },
    Module {
        name: "bz2",
        lib: "bzip2",
        imports: &["bz2"],
        since: 0,
        until: None,
    },
    Module {
        name: "lzma",
        lib: "xz",
        imports: &["lzma"],
        since: 3,
        until: None,
    },
    Module {
        name: "readline",
        lib: "readline",
        imports: &["readline"],
        since: 0,
        until: None,
    },
    Module {
        name: "sqlite3",
        lib: "sqlite",
        imports: &["sqlite3"],
        since: 0,
        until: None,
    },
    Module {
        name: "ctypes",
        lib: "libffi",
        imports: &["ctypes"],
        since: 7,
        until: None,
    },
    Module {
        name: "curses",
        lib: "ncurses",
        imports: &["curses", "curses.panel"],
        since: 0,
        until: None,
    },
    Module {
        name: "tkinter",
        lib: "tk",
        imports: &["tkinter"],
        since: 0,
        until: None,
    },
    Module {
        name: "dbm",
        lib: "gdbm",
        imports: &["dbm.gnu", "dbm.ndbm"],
        since: 0,
        until: None,
    },
    Module {
        name: "uuid",
        lib: "libuuid",
        imports: &["_uuid"],
        since: 7,
        until: None,
    },
    Module {
        name: "crypt",
        lib: "libcrypt",
        imports: &["crypt"],
        since: 0,
        until: Some(12),
    },
    Module {
        name: "nis",
        lib: "libnsl",
        imports: &["nis"],
        since: 0,
        until: Some(12),
    },
    Module {
        name: "zstd",
        lib: "zstd",
        imports: &["compression.zstd"],
        since: 14,
        until: None,
    },
];

pub fn lib(key: &str) -> Option<&'static Lib> {
    LIBS.iter().find(|l| l.key == key)
}

/// The modules CPython 3.`minor` builds that need an outside library.
pub fn modules(minor: u32) -> Vec<&'static Module> {
    MODULES
        .iter()
        .filter(|m| minor >= m.since && m.until.is_none_or(|u| minor <= u))
        .collect()
}

/// CPython's `configure` minimums (PEP 644 for OpenSSL; `configure.ac` for SQLite).
pub fn min_version(key: &str, minor: u32) -> Option<MinVersion> {
    match key {
        "openssl" if minor >= 10 => Some(MinVersion {
            header: "openssl/opensslv.h",
            expr: "OPENSSL_VERSION_NUMBER",
            need: 0x1010_100f,
            shown: "1.1.1",
            text: "OPENSSL_VERSION_TEXT",
        }),
        "openssl" if minor >= 7 => Some(MinVersion {
            header: "openssl/opensslv.h",
            expr: "OPENSSL_VERSION_NUMBER",
            need: 0x1000_2000,
            shown: "1.0.2",
            text: "OPENSSL_VERSION_TEXT",
        }),
        "sqlite" if minor >= 13 => Some(MinVersion {
            header: "sqlite3.h",
            expr: "SQLITE_VERSION_NUMBER",
            need: 3_015_002,
            shown: "3.15.2",
            text: "SQLITE_VERSION",
        }),
        "sqlite" if minor >= 11 => Some(MinVersion {
            header: "sqlite3.h",
            expr: "SQLITE_VERSION_NUMBER",
            need: 3_007_015,
            shown: "3.7.15",
            text: "SQLITE_VERSION",
        }),
        "zstd" if minor >= 14 => Some(MinVersion {
            header: "zstd.h",
            expr: "ZSTD_VERSION_NUMBER",
            need: 10405,
            shown: "1.4.5",
            text: "ZSTD_VERSION_MAJOR ZSTD_VERSION_MINOR ZSTD_VERSION_RELEASE",
        }),
        _ => None,
    }
}

/// The first library version CPython 3.`minor` can't build against (final review M4): `need`
/// is that version's value of `expr`, and `shown` what the version needs instead. CPython 3.12
/// and older don't build `_tkinter` against Tk 9 (Fedora's `tk-devel` is Tk 9 from Fedora 42).
pub fn too_new(key: &str, minor: u32) -> Option<MinVersion> {
    match key {
        "tk" if minor <= 12 => Some(MinVersion {
            header: "tk.h",
            expr: "TK_MAJOR_VERSION",
            need: 9,
            shown: "8.5 or 8.6",
            text: "TK_PATCH_LEVEL",
        }),
        _ => None,
    }
}

/// `verify_py312` → 12; `verify_py3_latest` → the newest (`u32::MAX`); 2.x and others → `None`.
pub fn minor_of_step(step: &str) -> Option<u32> {
    let tag = step.strip_prefix("verify_py3")?;
    if tag == "_latest" {
        return Some(u32::MAX);
    }
    tag.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(minor: u32) -> Vec<&'static str> {
        modules(minor).iter().map(|m| m.name).collect()
    }

    #[test]
    fn modules_follow_cpythons_releases() {
        let m312 = names(12);
        for n in [
            "ssl", "zlib", "bz2", "lzma", "readline", "sqlite3", "ctypes", "curses", "tkinter",
            "dbm", "uuid", "crypt", "nis",
        ] {
            assert!(m312.contains(&n), "3.12 lacks {n}");
        }
        let m313 = names(13);
        assert!(
            !m313.contains(&"crypt") && !m313.contains(&"nis"),
            "{m313:?}"
        );
        assert!(!names(13).contains(&"zstd"), "compression.zstd is 3.14+");
        assert!(names(14).contains(&"zstd") && names(u32::MAX).contains(&"zstd"));
        assert!(!names(6).contains(&"uuid"), "_uuid is 3.7+");
        assert!(!names(6).contains(&"ctypes"), "3.6 bundles libffi");
        assert!(names(u32::MAX).contains(&"tkinter"));
    }

    #[test]
    fn every_module_names_a_known_library_with_five_packages() {
        for minor in [7, 12, 13] {
            for m in modules(minor) {
                let l = lib(m.lib).unwrap_or_else(|| panic!("{}: no lib {}", m.name, m.lib));
                assert!(l.packages.iter().all(|p| !p.is_empty()), "{}", l.key);
                assert!(!m.imports.is_empty(), "{}", m.name);
            }
        }
        assert!(lib(CA_KEY).is_some());
    }

    #[test]
    fn minimum_versions_come_from_cpythons_configure() {
        let ssl310 = min_version("openssl", 10).unwrap();
        assert_eq!((ssl310.need, ssl310.shown), (0x1010_100f, "1.1.1"));
        assert_eq!(min_version("openssl", 9).unwrap().shown, "1.0.2");
        assert!(min_version("openssl", 6).is_none());
        assert_eq!(min_version("sqlite", 13).unwrap().need, 3_015_002);
        assert_eq!(min_version("sqlite", 11).unwrap().shown, "3.7.15");
        assert!(min_version("zlib", 13).is_none());
        assert_eq!(min_version("zstd", 14).unwrap().shown, "1.4.5");
        assert!(min_version("zstd", 13).is_none());
        assert_eq!(
            min_version("openssl", 13).unwrap().text,
            "OPENSSL_VERSION_TEXT"
        );
        assert_eq!(min_version("sqlite", 13).unwrap().text, "SQLITE_VERSION");
        assert_eq!(min_version("zstd", 14).unwrap().text.split(' ').count(), 3);
    }

    #[test]
    fn tk_9_is_too_new_up_to_3_12() {
        let tk = too_new("tk", 12).unwrap();
        assert_eq!(
            (tk.expr, tk.need, tk.shown),
            ("TK_MAJOR_VERSION", 9, "8.5 or 8.6")
        );
        assert!(too_new("tk", 13).is_none());
        assert!(too_new("openssl", 12).is_none());
    }

    #[test]
    fn the_minor_comes_from_the_verify_step() {
        assert_eq!(minor_of_step("verify_py312"), Some(12));
        assert_eq!(minor_of_step("verify_py39"), Some(9));
        assert_eq!(minor_of_step("verify_py3_latest"), Some(u32::MAX));
        assert_eq!(minor_of_step("verify_py27"), None);
        assert_eq!(minor_of_step("standard"), None);
    }

    #[test]
    fn libnsl_probe_uses_libtirpc_pkg_config() {
        // yp_prot.h includes <rpc/rpc.h>, found only through libtirpc's include path.
        assert_eq!(lib("libnsl").unwrap().pkg_config, Some("libtirpc"));
    }
}
