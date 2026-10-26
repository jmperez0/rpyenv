//! The libraries the build route compiles without root (M8 design §6.1): each library's
//! version, source and SHA-256 come from `deps/catalog.tsv`; how to build it is here. The
//! recipes were measured in Ubuntu 24.04 and Fedora 44 containers on 2026-10-17 (plan M8b,
//! "Measured").

use super::defs::{Definition, Fetch};
use super::stdlib;

/// One row of `deps/catalog.tsv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// Where a library's version came from: the catalog, or the definition's own pin, which
/// wins (M8 design §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Catalog,
    Definition,
}

impl Origin {
    pub fn word(self) -> &'static str {
        match self {
            Origin::Catalog => "catalog",
            Origin::Definition => "definition",
        }
    }

    pub fn from_word(w: &str) -> Option<Origin> {
        match w {
            "catalog" => Some(Origin::Catalog),
            "definition" => Some(Origin::Definition),
            _ => None,
        }
    }
}

/// How one library is built: shell command lines run with `sh -c`, in order, in the source
/// tree's `dir`. `store::expand` fills in `{prefix}` (the entry), `{destdir}` (where `make
/// install` stages it), `{stage}` (`{destdir}{prefix}`), `{jobs}`, `{triple}` (`cc
/// -dumpmachine`), `{openssldir}`, `{scripts}` (`<root>/.rpyenv/tcltk/<key>-<version>`, the
/// shared home of Tcl's and Tk's script folders), `{python}` (the bootstrap CPython, for recipes
/// with `python`) and `{dep:<key>}` (the entry of a library in `needs`).
pub struct Recipe {
    pub key: &'static str,
    /// The subdirectory of the source tree the commands run in (`unix` for Tcl and Tk).
    pub dir: &'static str,
    /// Built first, always together with this one (plan ruling P11).
    pub needs: &'static [&'static str],
    /// Its build runs Perl scripts that need Perl's core modules (ruling P4).
    pub perl: bool,
    /// It needs X11: the system's headers and libX11, or the store's (M8c, spec §6.4).
    pub x11: bool,
    /// Its steps run Python (xcb's code generator): `{python}` is the bootstrap CPython
    /// (`install::bootstrap`).
    pub python: bool,
    /// Added to the first step when the store built X11 in the same run (Tk only, ruling C4):
    /// Tk then uses the store's libX11, without Xft (D10).
    pub x11_args: &'static str,
    /// Include directories under the entry besides `include`, for CPython's `CPPFLAGS`.
    pub includes: &'static [&'static str],
    pub steps: &'static [&'static str],
}

const EMBEDDED: &str = include_str!("../../deps/catalog.tsv");

/// The X11 client libraries the store builds for Tk when the system has no X11 headers
/// (M8c, spec §6.4), in build order. libXext is not among them: Tk 8.6 doesn't use it
/// (measured, ruling C2).
pub const X11: [&str; 6] = [
    "xorgproto",
    "libXau",
    "xcb-proto",
    "libxcb",
    "xtrans",
    "libX11",
];

/// The order libraries are built in: each after the ones it needs.
pub const ORDER: [&str; 23] = [
    "pkgconf",
    "zlib",
    "bzip2",
    "xz",
    "ncurses",
    "readline",
    "gdbm",
    "sqlite",
    "libffi",
    "xorgproto",
    "libXau",
    "xcb-proto",
    "libxcb",
    "xtrans",
    "libX11",
    "tcl",
    "tk",
    "libuuid",
    "libxcrypt",
    "libtirpc",
    "libnsl",
    "zstd",
    "openssl",
];

/// `make install` with `DESTDIR` (three steps every autotools recipe ends with).
macro_rules! autotools {
    ($configure:expr) => {
        &[
            $configure,
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
        ]
    };
}

static RECIPES: &[Recipe] = &[
    Recipe {
        key: "pkgconf",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            "./configure --prefix={prefix} --disable-shared --with-system-libdir=/lib:/usr/lib:/lib64:/usr/lib64 --with-system-includedir=/usr/include --with-pkg-config-dir=/usr/local/lib/pkgconfig:/usr/local/share/pkgconfig:/usr/lib/{triple}/pkgconfig:/usr/lib64/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig",
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            "ln -s pkgconf {stage}/bin/pkg-config",
        ],
    },
    Recipe {
        key: "zlib",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("./configure --static --prefix={prefix}"),
    },
    Recipe {
        key: "bzip2",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            r#"make -j{jobs} libbz2.a CC="${CC:-cc}" "CFLAGS=-O2 -fPIC -D_FILE_OFFSET_BITS=64""#,
            "install -D -m644 bzlib.h {stage}/include/bzlib.h",
            "install -D -m644 libbz2.a {stage}/lib/libbz2.a",
        ],
    },
    Recipe {
        key: "xz",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-xz --disable-xzdec --disable-lzmadec --disable-lzmainfo --disable-scripts --disable-doc --disable-nls"),
    },
    Recipe {
        key: "ncurses",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        // `term.h` (`setupterm`, for `_curses`) is only there (measured).
        includes: &["include/ncursesw"],
        steps: autotools!("./configure --prefix={prefix} --without-shared --with-normal --enable-widec --without-debug --without-ada --without-cxx-binding --without-manpages --without-tests --enable-pc-files --with-pkg-config-libdir={prefix}/lib/pkgconfig --with-default-terminfo-dir=/usr/share/terminfo --with-terminfo-dirs=/etc/terminfo:/lib/terminfo:/usr/share/terminfo"),
    },
    Recipe {
        key: "readline",
        dir: "",
        needs: &["ncurses"],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            // `-std=gnu11`: readline before 8.2 (3.8's pin, 8.1) declares `()` prototypes that
            // C23, GCC 15's default, reads as `(void)` (measured: Fedora 44, Tumbleweed).
            r#"CFLAGS="${CFLAGS--g -O2} -std=gnu11" CPPFLAGS='-I{dep:ncurses}/include -I{dep:ncurses}/include/ncursesw' ./configure --prefix={prefix} --disable-shared --enable-static --with-curses"#,
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            // ncurses joins the archive: `-lreadline` then stands alone (ruling P11).
            r"cd {stage}/lib && mv libreadline.a in-libreadline.a && printf 'create libreadline.a\naddlib in-libreadline.a\naddlib {dep:ncurses}/lib/libncursesw.a\nsave\nend\n' | ar -M && rm in-libreadline.a",
        ],
    },
    Recipe {
        key: "gdbm",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            "./configure --prefix={prefix} --disable-shared --enable-static --with-pic --enable-libgdbm-compat --without-readline --disable-nls",
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            // `_dbm` links `-lgdbm_compat` alone (ruling P11).
            r"cd {stage}/lib && mv libgdbm_compat.a in-libgdbm_compat.a && printf 'create libgdbm_compat.a\naddlib in-libgdbm_compat.a\naddlib libgdbm.a\nsave\nend\n' | ar -M && rm in-libgdbm_compat.a",
        ],
    },
    Recipe {
        key: "sqlite",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static"),
    },
    Recipe {
        key: "libffi",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-docs --disable-multi-os-directory"),
    },
    Recipe {
        key: "xorgproto",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("./configure --prefix={prefix}"),
    },
    Recipe {
        key: "libXau",
        dir: "",
        needs: &["xorgproto"],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("PKG_CONFIG_PATH={dep:xorgproto}/share/pkgconfig ./configure --prefix={prefix} --disable-shared --enable-static --with-pic"),
    },
    Recipe {
        key: "xcb-proto",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: true,
        x11_args: "",
        includes: &[],
        // Installs xcbgen (pure Python) under the bootstrap's `lib/python3.X` (ruling C9).
        steps: autotools!("PYTHON={python} ./configure --prefix={prefix}"),
    },
    Recipe {
        key: "libxcb",
        dir: "",
        needs: &["xorgproto", "libXau", "xcb-proto"],
        perl: false,
        x11: false,
        python: true,
        x11_args: "",
        includes: &[],
        steps: autotools!("PYTHON={python} PKG_CONFIG_PATH={dep:xcb-proto}/share/pkgconfig:{dep:libXau}/lib/pkgconfig:{dep:xorgproto}/share/pkgconfig ./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-devel-docs --without-doxygen"),
    },
    Recipe {
        key: "xtrans",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: autotools!("./configure --prefix={prefix}"),
    },
    Recipe {
        key: "libX11",
        dir: "",
        needs: &["xorgproto", "libXau", "libxcb", "xtrans"],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            // `--datadir=/usr/share`: locale data and XErrorDB come from the system's path, so
            // the store stays a cache (ruling C3, measured).
            "PKG_CONFIG_PATH={dep:libxcb}/lib/pkgconfig:{dep:libXau}/lib/pkgconfig:{dep:xtrans}/share/pkgconfig:{dep:xorgproto}/share/pkgconfig ./configure --prefix={prefix} --datadir=/usr/share --disable-shared --enable-static --with-pic --disable-specs --without-xmlto --without-fop --disable-xf86bigfont",
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            // libxcb and libXau join the archive: `-lX11` then stands alone (ruling P11).
            r"cd {stage}/lib && mv libX11.a in-libX11.a && printf 'create libX11.a\naddlib in-libX11.a\naddlib {dep:libxcb}/lib/libxcb.a\naddlib {dep:libXau}/lib/libXau.a\nsave\nend\n' | ar -M && rm in-libX11.a",
        ],
    },
    Recipe {
        key: "tcl",
        dir: "unix",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        // The compiled-in library path is the shared script home outside the store, so Tcl
        // finds it from a venv too, and never the store's copy; without it, Tcl falls back to
        // the copy in the version (`<prefix>/lib/tcl8.6`, measured; final review I1).
        steps: &[
            "./configure --prefix={prefix} --disable-shared --enable-threads",
            "make -j{jobs} TCL_LIBRARY={scripts}/tcl8.6 binaries libraries",
            "make install-binaries install-libraries install-headers INSTALL_ROOT={destdir}",
        ],
    },
    Recipe {
        key: "tk",
        dir: "unix",
        needs: &["tcl"],
        perl: false,
        x11: true,
        python: false,
        x11_args: " --disable-xft --disable-xss --x-includes={dep:libX11}/include --x-libraries={dep:libX11}/lib CPPFLAGS=-I{dep:xorgproto}/include",
        includes: &[],
        steps: &[
            "./configure --prefix={prefix} --disable-shared --with-tcl={dep:tcl}/lib",
            "make -j{jobs} TK_LIBRARY={scripts}/tk8.6 TCL_GENERIC_DIR={dep:tcl}/include TCL_PLATFORM_DIR={dep:tcl}/include binaries libraries",
            "make install-binaries install-libraries install-headers INSTALL_ROOT={destdir}",
        ],
    },
    Recipe {
        key: "libuuid",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        // CPython 3.8's setup.py looks for `uuid.h` itself, not `uuid/uuid.h` (measured).
        includes: &["include/uuid"],
        steps: autotools!("./configure --prefix={prefix} --disable-all-programs --enable-libuuid --disable-shared --enable-static --with-pic --without-python --disable-nls --disable-bash-completion --without-systemd --without-udev --disable-liblastlog2"),
    },
    Recipe {
        key: "libxcrypt",
        dir: "",
        needs: &[],
        perl: true,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        // Without `--disable-symvers`, `_crypt` doesn't link (measured).
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-symvers --disable-failure-tokens"),
    },
    Recipe {
        key: "libtirpc",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &["include/tirpc"],
        steps: autotools!("./configure --prefix={prefix} --disable-shared --enable-static --with-pic --disable-gssapi"),
    },
    Recipe {
        key: "libnsl",
        dir: "",
        needs: &["libtirpc"],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            r#"TIRPC_CFLAGS=-I{dep:libtirpc}/include/tirpc TIRPC_LIBS="-L{dep:libtirpc}/lib -ltirpc" ./configure --prefix={prefix} --disable-shared --enable-static --with-pic"#,
            "make -j{jobs}",
            "make install DESTDIR={destdir}",
            r"cd {stage}/lib && mv libnsl.a in-libnsl.a && printf 'create libnsl.a\naddlib in-libnsl.a\naddlib {dep:libtirpc}/lib/libtirpc.a\nsave\nend\n' | ar -M && rm in-libnsl.a",
        ],
    },
    Recipe {
        key: "zstd",
        dir: "",
        needs: &[],
        perl: false,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        steps: &[
            "make -C lib -j{jobs} libzstd.a",
            "make -C lib install-static install-includes install-pc PREFIX={prefix} DESTDIR={destdir}",
        ],
    },
    Recipe {
        key: "openssl",
        dir: "",
        needs: &[],
        perl: true,
        x11: false,
        python: false,
        x11_args: "",
        includes: &[],
        // `./config` and `build_libs`/`install_dev` exist in 1.1.1, 3.x and 4.x alike (1.1.1 has
        // no `build_sw`); CPython needs the libraries and headers, not the `openssl` program
        // (measured on 1.1.1u, 3.0.16, 3.6.5 and 4.0.3). `no-tests` is left out: OpenSSL 1.1.0
        // (pinned by 3.7.0-3.7.7 and 3.8.0-3.8.3) rejects it, and `build_libs` builds no tests
        // (measured on 1.1.0j).
        steps: &[
            "./config --prefix={prefix} --libdir=lib --openssldir={openssldir} no-shared -fPIC",
            "make -j{jobs} build_libs",
            "make install_dev DESTDIR={destdir}",
        ],
    },
];

/// The rows of a catalog file: key, version, URL and lowercase SHA-256, separated by tabs;
/// `#` lines and blank lines are skipped. A malformed row is an error naming its line.
pub fn parse(text: &str) -> Result<Vec<Row>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let hex =
            |s: &str| s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if f.len() != 4 || f[0].is_empty() || f[1].is_empty() || f[2].is_empty() || !hex(f[3]) {
            return Err(format!("the build catalog, line {}: {line}", i + 1));
        }
        out.push(Row {
            key: f[0].into(),
            version: f[1].into(),
            url: f[2].into(),
            sha256: f[3].into(),
        });
    }
    Ok(out)
}

/// The catalog rpyenv ships. Debug builds read the file `RPYENV_TEST_CATALOG` names instead.
pub fn table(env: &dyn Fn(&str) -> Option<String>) -> Result<Vec<Row>, String> {
    if cfg!(debug_assertions) {
        if let Some(p) = env("RPYENV_TEST_CATALOG").filter(|p| !p.is_empty()) {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
            return parse(&text);
        }
    }
    parse(EMBEDDED)
}

pub fn recipe(key: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.key == key)
}

/// The catalog keys that supply a `Dep::lib` (a `stdlib` key, `ca-certificates` or
/// `pkg-config`). Empty for what only the system provides: the compiler, `make`, `patch`.
pub fn keys_for(lib: &str) -> &'static [&'static str] {
    match lib {
        // A self-built OpenSSL reads the pinned bundle when the system has no certificates.
        "openssl" | stdlib::CA_KEY => &["openssl"],
        "zlib" => &["zlib"],
        "bzip2" => &["bzip2"],
        "xz" => &["xz"],
        "readline" => &["ncurses", "readline"],
        "sqlite" => &["sqlite"],
        "libffi" => &["libffi"],
        "ncurses" => &["ncurses"],
        "tk" => &["tcl", "tk"],
        "gdbm" => &["gdbm"],
        "libuuid" => &["libuuid"],
        "libcrypt" => &["libxcrypt"],
        "libnsl" => &["libtirpc", "libnsl"],
        "zstd" => &["zstd"],
        "pkg-config" => &["pkgconf"],
        // No X11 headers: the store builds the X11 libraries Tk needs (M8c).
        "x11" => &X11,
        _ => &[],
    }
}

/// Every catalog key `libs` (`Dep::lib` values) need, each once, in `ORDER`.
pub fn plan(libs: &[&str]) -> Vec<&'static str> {
    ORDER
        .iter()
        .copied()
        .filter(|k| libs.iter().any(|l| keys_for(l).contains(k)))
        .collect()
}

/// A definition's own pin of `key`: python-build's macOS-only `openssl-*` and `readline-*`
/// packages (`--if has_broken_mac_openssl`, `--if has_broken_mac_readline`), which Linux
/// otherwise skips.
pub fn pinned(key: &str, def: &Definition) -> Option<Row> {
    let (prefix, condition) = match key {
        "openssl" => ("openssl-", "has_broken_mac_openssl"),
        "readline" => ("readline-", "has_broken_mac_readline"),
        _ => return None,
    };
    def.packages.iter().rev().find_map(|p| {
        let version = p.name.strip_prefix(prefix)?;
        if p.condition.as_deref() != Some(condition) {
            return None;
        }
        match &p.fetch {
            Fetch::Tarball {
                url,
                fragment: Some(sha),
            } => Some(Row {
                key: key.into(),
                version: version.into(),
                url: url.clone(),
                sha256: sha.to_ascii_lowercase(),
            }),
            _ => None,
        }
    })
}

/// `key`'s row for this definition: its pin when it has one, else the catalog's.
pub fn resolve(rows: &[Row], key: &str, def: &Definition) -> Option<(Row, Origin)> {
    pinned(key, def)
        .map(|r| (r, Origin::Definition))
        .or_else(|| {
            rows.iter()
                .find(|r| r.key == key)
                .cloned()
                .map(|r| (r, Origin::Catalog))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::defs::{self, Package};
    use std::path::Path;

    #[test]
    fn the_shipped_catalog_has_a_row_and_a_recipe_for_each_library() {
        let rows = parse(EMBEDDED).unwrap();
        for k in ORDER {
            assert!(rows.iter().any(|r| r.key == k), "{k} has no row");
            assert!(recipe(k).is_some(), "{k} has no recipe");
        }
        assert!(rows.iter().any(|r| r.key == "cacert"));
        assert_eq!(
            rows.len(),
            ORDER.len() + 1,
            "one row per library, plus cacert"
        );
        for r in &rows {
            assert!(r.url.starts_with("https://"), "{}", r.url);
        }
        let mut keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), rows.len(), "a key appears twice");
    }

    #[test]
    fn malformed_rows_are_refused_with_their_line() {
        let sha = "a".repeat(64);
        let ok = format!("# comment\n\nzlib\t1.3.2\thttps://x/z.tar.gz\t{sha}\n");
        assert_eq!(parse(&ok).unwrap().len(), 1);
        assert!(parse("zlib\t1.3.2\thttps://x/z.tar.gz\n")
            .unwrap_err()
            .contains("line 1"));
        let upper = format!("zlib\t1.3.2\thttps://x/z.tar.gz\t{}\n", "A".repeat(64));
        assert!(parse(&upper).is_err(), "SHA-256 must be lowercase hex");
        let short = "zlib\t1.3.2\thttps://x/z.tar.gz\tabc\n";
        assert!(parse(short).is_err());
    }

    #[test]
    fn every_recipe_names_only_dependencies_built_before_it() {
        for r in RECIPES {
            let at = ORDER.iter().position(|k| *k == r.key).unwrap();
            for n in r.needs {
                let dep = ORDER.iter().position(|k| k == n).expect(n);
                assert!(dep < at, "{} needs {n}, built later", r.key);
            }
            for s in r.steps {
                let mut rest = *s;
                while let Some(i) = rest.find("{dep:") {
                    let tail = &rest[i + 5..];
                    let k = &tail[..tail.find('}').unwrap()];
                    assert!(
                        r.needs.contains(&k),
                        "{} uses {{dep:{k}}} without needing it",
                        r.key
                    );
                    rest = &tail[k.len()..];
                }
            }
            let mut rest = r.x11_args;
            while let Some(i) = rest.find("{dep:") {
                let tail = &rest[i + 5..];
                let k = &tail[..tail.find('}').unwrap()];
                assert!(X11.contains(&k), "{}'s x11_args use {{dep:{k}}}", r.key);
                assert!(
                    ORDER.iter().position(|o| *o == k).unwrap() < at,
                    "{k} after {}",
                    r.key
                );
                rest = &tail[k.len()..];
            }
        }
    }

    #[test]
    fn x11_is_planned_before_tk_in_build_order() {
        assert_eq!(
            plan(&["tk", "x11"]),
            [
                "xorgproto",
                "libXau",
                "xcb-proto",
                "libxcb",
                "xtrans",
                "libX11",
                "tcl",
                "tk"
            ]
        );
        assert_eq!(
            plan(&["tk"]),
            ["tcl", "tk"],
            "with the system's X11, no X11 key"
        );
        assert_eq!(keys_for("x11"), X11);
    }

    #[test]
    fn only_tk_has_x11_args_and_only_xcb_runs_python() {
        for r in RECIPES {
            assert_eq!(r.x11_args.is_empty(), r.key != "tk", "{}", r.key);
            let uses = r.steps.iter().any(|s| s.contains("{python}"));
            assert_eq!(
                uses, r.python,
                "{}: {{python}} and `python` disagree",
                r.key
            );
            assert_eq!(
                r.python,
                matches!(r.key, "xcb-proto" | "libxcb"),
                "{}",
                r.key
            );
        }
        let tk = recipe("tk").unwrap().x11_args;
        for want in [
            "--disable-xft",
            "--x-includes={dep:libX11}/include",
            "--x-libraries={dep:libX11}/lib",
            "-I{dep:xorgproto}/include",
        ] {
            assert!(tk.contains(want), "{tk}");
        }
    }

    #[test]
    fn every_library_the_check_names_has_catalog_keys() {
        for minor in [7, 12, 13, 14] {
            for m in stdlib::modules(minor) {
                assert!(!keys_for(m.lib).is_empty(), "{} has no catalog key", m.lib);
            }
        }
        assert_eq!(keys_for(stdlib::CA_KEY), ["openssl"]);
        assert_eq!(keys_for("pkg-config"), ["pkgconf"]);
        for system_only in ["compiler", "make", "patch"] {
            assert!(keys_for(system_only).is_empty(), "{system_only}");
        }
    }

    #[test]
    fn the_plan_keeps_dependencies_together_and_in_order() {
        assert_eq!(
            plan(&["tk", "readline", "libnsl"]),
            ["ncurses", "readline", "tcl", "tk", "libtirpc", "libnsl"]
        );
        assert_eq!(plan(&["openssl", stdlib::CA_KEY]), ["openssl"]);
        assert_eq!(plan(&["zstd", "pkg-config"]), ["pkgconf", "zstd"]);
        assert!(plan(&["compiler"]).is_empty());
    }

    fn def_with(pkgs: &[(&str, &str, &str)]) -> Definition {
        let mut d = Definition::default();
        for (name, url, cond) in pkgs {
            d.packages.push(Package {
                name: name.to_string(),
                fetch: Fetch::Tarball {
                    url: url.to_string(),
                    fragment: Some("b".repeat(64)),
                },
                steps: vec![],
                condition: (!cond.is_empty()).then(|| cond.to_string()),
            });
        }
        d
    }

    #[test]
    fn a_definitions_pin_wins_for_openssl_and_readline() {
        let rows = parse(EMBEDDED).unwrap();
        let d = def_with(&[
            (
                "openssl-1.1.1u",
                "https://www.openssl.org/source/openssl-1.1.1u.tar.gz",
                "has_broken_mac_openssl",
            ),
            (
                "readline-8.1",
                "https://ftpmirror.gnu.org/readline/readline-8.1.tar.gz",
                "has_broken_mac_readline",
            ),
            (
                "Python-3.8.20",
                "https://www.python.org/ftp/python/3.8.20/Python-3.8.20.tar.xz",
                "",
            ),
        ]);
        let (r, o) = resolve(&rows, "openssl", &d).unwrap();
        assert_eq!((r.version.as_str(), o), ("1.1.1u", Origin::Definition));
        assert_eq!(
            r.url,
            "https://www.openssl.org/source/openssl-1.1.1u.tar.gz"
        );
        assert_eq!(r.sha256, "b".repeat(64));
        assert_eq!(resolve(&rows, "readline", &d).unwrap().0.version, "8.1");
        // Every other library comes from the catalog.
        let (z, o) = resolve(&rows, "zlib", &d).unwrap();
        assert_eq!((z.version.as_str(), o), ("1.3.2", Origin::Catalog));
    }

    // Review Focus 5
    #[test]
    fn a_definition_without_a_pin_gets_the_catalog_row() {
        let rows = parse(EMBEDDED).unwrap();
        // An unconditional `openssl-*` package is no pin: only the macOS-only ones are.
        let d = def_with(&[("openssl-9.9.9", "https://example.org/o.tar.gz", "")]);
        let (r, o) = resolve(&rows, "openssl", &d).unwrap();
        assert_eq!((r.version.as_str(), o), ("3.6.5", Origin::Catalog));
        assert_eq!(Origin::from_word(o.word()), Some(o));
    }

    /// Every vendored CPython 3.7+ definition that pins OpenSSL pins a series the recipe
    /// builds (1.1.0, 1.1.1, 3.x or 4.x), with a SHA-256.
    #[test]
    fn every_vendored_cpython_3_7_and_later_pins_an_openssl_the_recipe_builds() {
        let env = |_: &str| -> Option<String> { None };
        let root = Path::new("/nonexistent-rpyenv-root");
        let mut checked = 0;
        for name in defs::known(&env) {
            let Some(minor) = name
                .strip_prefix("3.")
                .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|m| m.parse::<u32>().ok())
            else {
                continue;
            };
            if minor < 7 {
                continue;
            }
            let Some(found) = defs::find(root, &name, &env) else {
                continue;
            };
            let Ok(d) = defs::parse(&found, &env, &|n| defs::find(root, n, &env)) else {
                continue;
            };
            if let Some(r) = pinned("openssl", &d) {
                assert!(
                    ["1.1.0", "1.1.1", "3.", "4."]
                        .iter()
                        .any(|p| r.version.starts_with(p)),
                    "{name}: openssl {}",
                    r.version
                );
                assert_eq!(r.sha256.len(), 64, "{name}");
                checked += 1;
            }
        }
        assert!(checked > 100, "only {checked} definitions checked");
    }
}
