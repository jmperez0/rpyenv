# M8c: X11 from source without root, and strict scripts, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On Linux without X11 headers, `pyenv install --deps=build` (or `[b]`) builds the X11
client libraries Tk needs into the store too, so `tkinter` works for users without root; and a
script that sets neither `--deps` nor `RPYENV_BUILD_DEPS` becomes strict (spec D13).

**Architecture:** Six catalog rows and recipes (xorgproto, libXau, xcb-proto, libxcb, xtrans,
libX11) join the store, all static; libX11's archive absorbs libxcb's and libXau's (as M8b's
merges do). xcb-proto and libxcb run Python to generate C, so a new `install/bootstrap.rs`
builds a minimal CPython from the tarball being installed, once per store run and only when one
of those two must be built, and removes it afterwards. When the store built libX11, Tk's
`configure` gets extra arguments (without Xft, D10) and a distinct entry; with the system's X11
headers, Tk and every entry name stay as in M8b. `deps::build_plan` plans X11 instead of
refusing `tkinter`. Then `commands/install.rs` treats a run with no terminal and no route as
`--deps=system`, and the builder drops the lenient summary.

**Tech Stack:** Rust (crate `pyenv`, Linux code paths only), POSIX `sh` recipes run by the
store, GitHub Actions with distribution containers, Python 3 for the weekly catalog check.

**Spec:** `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md` (M8c = §3 row 3,
§6.4, D9, D10, D13, §9 row 4). Earlier plans: `2026-10-17-m8a-detect-and-install.md`,
`2026-10-17-m8b-build-store.md`.

## Global Constraints

- Linux flavor only. No Windows code path changes; Windows tests must stay green.
- The build route is for CPython **3.7 and later** (M8b ruling P2). Python 2.x, 3.0-3.6, PyPy
  and conda keep today's behavior.
- Tcl/Tk **8.6** for every source build (spec D11). Tk without Xft when the store builds X11 (D10).
- X11 is built **only when the system has no X11 headers** (D9): with them, Tk is built as in
  M8b, and no store entry name changes (an rpyenv upgrade rebuilds nothing).
- Static libraries only; the store stays a cache: deleting `cache/deps` breaks no installed
  version (spec §6.2, D7).
- Never run `sudo` or a package manager on a development machine, and never build the real
  libraries there: unit and CLI tests use fakes (`RPYENV_TEST_PREFLIGHT`,
  `RPYENV_TEST_BUILD_TOOLS`, `RPYENV_TEST_CATALOG` with `file://` URLs). Real builds run only in
  throwaway podman containers (`podman --cgroup-manager=cgroupfs`) and CI (Task 6).
- Never run rpyenv against the real roots (`C:\Users\JM\.pyenv\pyenv-win`, `/home/jm/.pyenv`):
  tests use their fixtures; container runs use their own `PYENV_ROOT`.
- Test hooks are honored in debug builds only (`cfg!(debug_assertions)`).
- Every user-visible difference from upstream pyenv gets an allowlist entry (spec D12), cited
  by a test (`// allowlist D-NNN` directly above a `#[test]`); `python parity/coverage.py` must
  cover every row. The next free ID is **D-130**.
- Tests run in WSL through a script file (`wsl -d Debian --exec /usr/bin/bash -l
  /mnt/c/tmp/<file>.sh`, `CARGO_TARGET_DIR=/home/jm/rpyenv-m8-target`); never put `$VARS` in a
  `wsl … bash -c "…"` string. Python: `encoding='utf-8'` on every `open()`.
- Commit messages: one line, no attribution lines of any kind. No pushes.

## Plan rulings (made while planning; the user confirms them at plan review)

- **C1 (user, 2026-10-19):** xcb's Python is a minimal CPython built from the tarball being
  installed (option B of six: A PATH python3 first, C require python3, D pregenerated C
  published by rpyenv, L a pinned bootstrap CPython, plus rejected E-H). Measured below on 3.7.17,
  3.8.20, 3.12.15 and 3.14.8, so option L (the fallback for 3.7/3.8) is not needed.
- **C2:** libXext is left out of spec §6.4's list. Measured: Tk 8.6.18 builds without it,
  `TK_LIBS` names only `-lX11`, `tkinter.Tcl()` works, and `tkinter.Tk()` reaches the display
  check. The symbols Tk uses that sound like extensions (`XSync`) are libX11's own. Task 7
  updates the spec.
- **C3:** libX11 is configured with `--datadir=/usr/share`, so its locale data and `XErrorDB`
  are read from the system's path, not from the store: the store stays a cache. Measured: a Tk
  window opens under Xvfb with the store moved aside, and ASCII typing works with and without
  the system's locale data. Non-ASCII input needs the system's locale data (measured by hand
  on WSLg, below).
- **C4:** Tk's X11 arguments (`Recipe::x11_args`) are added to its first step only when the
  store built libX11 in the same run; `xorgproto` and `libX11` then join Tk's fingerprint. With
  the system's X11, Tk's steps and fingerprint are unchanged, so M8b store entries stay valid.
- **C5 (amended at Task 3 review):** the X11 entries reach CPython only through Tk's flags:
  libX11 through `TK_LIBS` (`-L<libX11>/lib -lX11`, read from `tkConfig.sh` as in M8b), and
  `-I<libX11>/include -I<xorgproto>/include` appended to the Tcl/Tk cflags (`TCLTK_CFLAGS`, or
  `--with-tcltk-includes=` for 3.10 and older), because `tk.h` includes `<X11/Xlib.h>`.
  `store::link` gives them no `CPPFLAGS`, `LDFLAGS` or `pkg-config` folder.
- **C6:** the store's downloads go through the download cache: the store closure in `pyenv
  install` creates `cache/deps` (which turns on `$PYENV_ROOT/cache`, as M8b documents) before
  it builds its fetcher, and the builder's fetcher is made after the store ran. The bootstrap
  saves its CPython tarball under the name the main build fetches, so it is downloaded once.
  With `PYTHON_BUILD_CACHE_PATH` naming a missing folder there is no cache, and the tarball is
  downloaded twice (accepted).
- **C7:** building X11 needs a CPython source tarball. A definition that fetches CPython with
  `git` (the `-dev` ones) keeps M8b's refusal for `tkinter` ("needs the X11 headers
  (libx11-dev)").
- **C8 (spec D13):** a run with no terminal and neither `--deps` nor `RPYENV_BUILD_DEPS` is
  `--deps=system`: anything missing stops the build with the report. `RPYENV_DEPS_MODE=lenient`
  and the "In a later rpyenv release" notice go away; `[c]` keeps a short summary.
- **C9:** xcb-proto's entry installs its generator (`xcbgen`, pure Python) under the
  bootstrap's `lib/python3.X/site-packages`. A later libxcb build with another version's
  bootstrap reads it from there (xcb-proto.pc's `pythondir`). Task 6's weekly Ubuntu leg forces
  that case and checks it.

## Measured (2026-10-19/20, throwaway `ubuntu:24.04` podman containers with only `build-essential`; scripts in `C:\tmp\m8c_spike\`)

| Fact | Evidence |
|---|---|
| A minimal CPython (`./configure --without-ensurepip --disable-test-modules && make -j python sharedmods`) builds and imports `xml.etree.ElementTree` and `xml.sax` from 3.7.17 (61 s), 3.8.20 (38 s), 3.12.15 (38 s), 3.14.8 (39 s); 3.7 warns about the unknown `--disable-test-modules` and goes on. `sharedmods` is what builds `pyexpat` | `boot.sh`, `boot314.sh`; python-build's patches for each version applied first |
| With each of those bootstraps as `PYTHON`, xcb-proto 1.17.0 and libxcb 1.17.0 configure and build, and libxcb's 39 generated C files are byte-identical (SHA-256 of their concatenation `21303b2fd2e81001…` all four times) | `boot.sh`, `boot314.sh` (`out/boot.txt`, `out/boot314.txt`) |
| xorgproto 2026.1, libXau 1.0.12, xcb-proto 1.17.0, libxcb 1.17.0, xtrans 1.6.0, libX11 1.8.14 build static; `ar -M` merges libxcb.a and libXau.a into libX11.a | `x11b.sh` (`out/round2.txt`) |
| Tk 8.6.18 with `--disable-xft --disable-xss --x-includes=<libX11>/include --x-libraries=<libX11>/lib CPPFLAGS=-I<xorgproto>/include` gives `TK_LIBS=-L<libX11>/lib -lX11 -lpthread -ldl -lpthread -lm`, with or without libXext | `x11b.sh`, `x11c.sh` (`out/noxext.txt`) |
| CPython 3.12.15 against that Tk: `tkinter.Tcl()` works, `tkinter.Tk()` stops at "no display name"; `_tkinter` links only libc and libm (`ldd`) | `x11b.sh`, `x11c.sh` |
| libX11 with `--datadir=/usr/share` has `/usr/share/X11/locale` and `/usr/share/X11/XErrorDB` compiled in (`strings`); with the store moved aside, under `xvfb-run`, a Tk window opens (`:99.0`) and an `Entry` works; with `/usr/share/X11/locale` also moved aside, `LANG=C.UTF-8`, the window and an `Entry` with text still work | `x11d.sh`, `x11e.sh` (`out/datadir.txt`, `out/nolocale.txt`) |
| Typed input via XTEST (`xdotool`) with the store moved aside: ASCII keys reach a Tk `Entry` with and without `/usr/share/X11/locale` (absence checked with `ls`), in `en_US.UTF-8` and `C.UTF-8`. On a Spanish keymap (`setxkbmap es`), `ñ` and dead `´` + `e` give the same text (`'hi  e'`) from the store's Tk 8.6.18 and from Ubuntu's own Tk 8.6.14 (`python3-tk`, system libX11) in all three cases. Neither Tk produced `ñ` or `é`, not even with the locale data present, so these runs can't show what missing locale data does to non-ASCII input | `x11f.sh`, `x11g.sh` (`out/typed.txt`, `out/keymap.txt`) |
| By hand on a real X server (WSLg, 2026-10-20; Task 6 Step 8 item 4): the 3.12.14 built in Task 6's Ubuntu run, in a fresh `ubuntu:24.04` container, `LANG=C.UTF-8`, the user typing `ñ`, `é`, then dead `´` + `e` on a Spanish keyboard. Without `/usr/share/X11/locale` (absence checked): the Entry holds `ee'e` (U+0065 U+0065 U+0027 U+0065), so `ñ` and `é` are lost. Control, same container with `libx11-data` installed: `ñé'e` (U+00F1 U+00E9 U+0027 U+0065). The dead key gave `'e` in both, so dead-key composition can't be judged on WSLg | `C:\tmp\m8c_wslg_typing.sh`, `.py` (`C:\tmp\m8c_wslg_out.txt`) |
| Each X11 source matches its SHA-256 below, computed from a download on 2026-10-19; every one has an upstream `.sig` | `latest2.sh`, `sha256sum` |

**Not measured:** the bootstrap on 3.9-3.11 and 3.13 (between measured versions; the weekly
legs build 3.14 and 3.8), libX11 built against a store pkgconf (the spike used the store
pkgconf, CI's Ubuntu leg has no system `pkg-config`), and distributions other than Ubuntu
(Debian's weekly leg gets the X11 build too, Task 6).
Dead-key composition is also not measured: on WSLg the dead key gave `'e` even with the locale
data present (the hand-check row above). Non-ASCII input without locale data was measured by
that hand check: `ñ` and `é` are lost, and they work once `libx11-data` is installed.

## Review Focus

1. **A CPython definition fetched with git** (`3.14-dev`) on a machine without X11 headers:
   `--deps=build` must refuse `tkinter` before building anything, naming `libx11-dev`, as in M8b
   (C7). Pinned in Task 4 (`a_git_definition_without_x11_headers_refuses_tkinter`).
2. **An rpyenv upgrade on a machine with X11 headers** must reuse every M8b store entry: Tk's
   entry name must not change. Pinned in Task 3 (`tk_without_store_x11_keeps_its_m8b_entry_name`).
3. **A second X11 build** (another CPython version, same store) builds no library and no
   bootstrap. Pinned in Task 3 (`a_python_recipe_reused_from_the_store_needs_no_bootstrap`) and
   Task 6 (stage B2 finds no `Building` line).
4. **A bootstrap that fails** (an old CPython that won't build minimal) must name its log and
   leave no bootstrap folder and no xcb-proto entry. Pinned in Task 2
   (`a_failed_bootstrap_names_its_log_and_leaves_no_folder`).
5. **A Dockerfile that used to build with warnings** (no flag, a missing optional library) now
   stops before downloading, with the report, the install command and the `--deps=build` hint.
   Pinned in Task 5 (`without_a_terminal_or_flag_a_missing_optional_library_stops_the_build`).

---

### Task 1: The X11 rows and recipes (`deps/catalog.tsv`, `install/catalog.rs`, `ci/deps_catalog.py`)

**Files:**
- Modify: `crates/pyenv/deps/catalog.tsv`
- Modify: `crates/pyenv/src/install/catalog.rs`
- Modify: `ci/deps_catalog.py`, `ci/test_deps_catalog.py`

**Interfaces:**
- Produces: `Recipe::python: bool` (its steps use `{python}`), `Recipe::x11_args: &'static str`
  (Tk only); `catalog::X11: [&str; 6]` (the X11 keys in build order); `keys_for("x11")`
  returns `&X11`; `ORDER` has 23 keys.

- [ ] **Step 1: Write the failing tests** (in `catalog.rs`'s `mod tests`)

```rust
    #[test]
    fn x11_is_planned_before_tk_in_build_order() {
        assert_eq!(
            plan(&["tk", "x11"]),
            ["xorgproto", "libXau", "xcb-proto", "libxcb", "xtrans", "libX11", "tcl", "tk"]
        );
        assert_eq!(plan(&["tk"]), ["tcl", "tk"], "with the system's X11, no X11 key");
        assert_eq!(keys_for("x11"), X11);
    }

    #[test]
    fn only_tk_has_x11_args_and_only_xcb_runs_python() {
        for r in RECIPES {
            assert_eq!(r.x11_args.is_empty(), r.key != "tk", "{}", r.key);
            let uses = r.steps.iter().any(|s| s.contains("{python}"));
            assert_eq!(uses, r.python, "{}: {{python}} and `python` disagree", r.key);
            assert_eq!(r.python, matches!(r.key, "xcb-proto" | "libxcb"), "{}", r.key);
        }
        let tk = recipe("tk").unwrap().x11_args;
        for want in ["--disable-xft", "--x-includes={dep:libX11}/include", "--x-libraries={dep:libX11}/lib", "-I{dep:xorgproto}/include"] {
            assert!(tk.contains(want), "{tk}");
        }
    }
```

Extend `every_recipe_names_only_dependencies_built_before_it`: after its `for s in r.steps`
loop, check `r.x11_args` the same way, except that a `{dep:k}` there must be in `X11` (not in
`needs`) and built before `r.key`:

```rust
            let mut rest = r.x11_args;
            while let Some(i) = rest.find("{dep:") {
                let tail = &rest[i + 5..];
                let k = &tail[..tail.find('}').unwrap()];
                assert!(X11.contains(&k), "{}'s x11_args use {{dep:{k}}}", r.key);
                assert!(ORDER.iter().position(|o| *o == k).unwrap() < at, "{k} after {}", r.key);
                rest = &tail[k.len()..];
            }
```

- [ ] **Step 2: Run them to see them fail**

Run (WSL script): `cargo test -q -p pyenv --lib install::catalog`
Expected: compile errors (`X11`, `x11_args`, `python` don't exist).

- [ ] **Step 3: Add the rows** to `crates/pyenv/deps/catalog.tsv` after the `libffi` row, and add
  `# X11 rows (M8c): computed from a download on 2026-10-19.` to the header comment:

```
xorgproto	2026.1	https://www.x.org/releases/individual/proto/xorgproto-2026.1.tar.xz	f9bfe4a9ed8c8ab9d2a3b0d49797f046052dadd06b7a8b45dbffaffb137e8290
libXau	1.0.12	https://www.x.org/releases/individual/lib/libXau-1.0.12.tar.xz	74d0e4dfa3d39ad8939e99bda37f5967aba528211076828464d2777d477fc0fb
xcb-proto	1.17.0	https://xorg.freedesktop.org/archive/individual/xcb/xcb-proto-1.17.0.tar.xz	2c1bacd2110f4799f74de6ebb714b94cf6f80fb112316b1219480fd22562148c
libxcb	1.17.0	https://xorg.freedesktop.org/archive/individual/xcb/libxcb-1.17.0.tar.xz	599ebf9996710fea71622e6e184f3a8ad5b43d0e5fa8c4e407123c88a59a6d55
xtrans	1.6.0	https://www.x.org/releases/individual/lib/xtrans-1.6.0.tar.xz	faafea166bf2451a173d9d593352940ec6404145c5d1da5c213423ce4d359e92
libX11	1.8.14	https://www.x.org/releases/individual/lib/libX11-1.8.14.tar.xz	bbc78c46217174655c2b32b138ad119f3ac15f50808b4d0b3a2677a4b1afc950
```

(Tabs between fields, as the other rows.)

- [ ] **Step 4: Add the fields, the keys and the recipes** in `catalog.rs`

In `struct Recipe`, change the `x11` doc and add two fields after it:

```rust
    /// It needs X11: the system's headers and libX11, or the store's (M8c, spec §6.4).
    pub x11: bool,
    /// Its steps run Python (xcb's code generator): `{python}` is the bootstrap CPython
    /// (`install::bootstrap`).
    pub python: bool,
    /// Added to the first step when the store built X11 in the same run (Tk only, ruling C4):
    /// Tk then uses the store's libX11, without Xft (D10).
    pub x11_args: &'static str,
```

Add `python: false, x11_args: "",` to every existing recipe except Tk, which gets:

```rust
        python: false,
        x11_args: " --disable-xft --disable-xss --x-includes={dep:libX11}/include --x-libraries={dep:libX11}/lib CPPFLAGS=-I{dep:xorgproto}/include",
```

Update the `Recipe` doc's placeholder list with `{python}` (the bootstrap CPython, for recipes
with `python`). Add the key list and the new `ORDER`:

```rust
/// The X11 client libraries the store builds for Tk when the system has no X11 headers
/// (M8c, spec §6.4), in build order. libXext is not among them: Tk 8.6 doesn't use it
/// (measured, ruling C2).
pub const X11: [&str; 6] = ["xorgproto", "libXau", "xcb-proto", "libxcb", "xtrans", "libX11"];

/// The order libraries are built in: each after the ones it needs.
pub const ORDER: [&str; 23] = [
    "pkgconf", "zlib", "bzip2", "xz", "ncurses", "readline", "gdbm", "sqlite", "libffi",
    "xorgproto", "libXau", "xcb-proto", "libxcb", "xtrans", "libX11",
    "tcl", "tk", "libuuid", "libxcrypt", "libtirpc", "libnsl", "zstd", "openssl",
];
```

(rustfmt will put one key per line; keep that.) In `keys_for`, add before `_ => &[]`:

```rust
        // No X11 headers: the store builds the X11 libraries Tk needs (M8c).
        "x11" => &X11,
```

and extend its doc: "…, `pkg-config`, or `x11` (the X11 libraries for Tk)". Add the six
recipes after `libffi`'s (store paths are plain, ruling P5 of M8b, so the inline
`PKG_CONFIG_PATH` needs no quotes):

```rust
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
```

- [ ] **Step 5: The weekly check knows the new rows.** In `ci/deps_catalog.py`'s `SOURCES`, add:

```python
    "xorgproto": ("listing", "https://www.x.org/releases/individual/proto/", r"xorgproto-(\d+\.\d+)\.tar\.xz", "https://www.x.org/releases/individual/proto/xorgproto-{v}.tar.xz"),
    "libXau": ("listing", "https://www.x.org/releases/individual/lib/", r"libXau-(\d+\.\d+\.\d+)\.tar\.xz", "https://www.x.org/releases/individual/lib/libXau-{v}.tar.xz"),
    "xcb-proto": ("listing", "https://xorg.freedesktop.org/archive/individual/xcb/", r"xcb-proto-(\d+\.\d+\.\d+)\.tar\.xz", "https://xorg.freedesktop.org/archive/individual/xcb/xcb-proto-{v}.tar.xz"),
    "libxcb": ("listing", "https://xorg.freedesktop.org/archive/individual/xcb/", r"libxcb-(\d+\.\d+\.\d+)\.tar\.xz", "https://xorg.freedesktop.org/archive/individual/xcb/libxcb-{v}.tar.xz"),
    "xtrans": ("listing", "https://www.x.org/releases/individual/lib/", r"xtrans-(\d+\.\d+\.\d+)\.tar\.xz", "https://www.x.org/releases/individual/lib/xtrans-{v}.tar.xz"),
    "libX11": ("listing", "https://www.x.org/releases/individual/lib/", r"libX11-(\d+\.\d+\.\d+)\.tar\.xz", "https://www.x.org/releases/individual/lib/libX11-{v}.tar.xz"),
```

and in `ci/test_deps_catalog.py`'s `Versions` class:

```python
    def test_x_org_listings_take_the_newest(self):
        page = '<a href="libX11-1.8.13.tar.xz">a</a> <a href="libX11-1.8.14.tar.xz">b</a> libX11-1.8.14.tar.xz.sig libXau-1.0.12.tar.xz'
        src = dc.SOURCES["libX11"]
        self.assertEqual(dc.newest(dc.matches(page, src[2])), "1.8.14")
        page = "xcb-proto-1.16.0.tar.xz xcb-proto-1.17.0.tar.xz libxcb-1.17.0.tar.xz"
        self.assertEqual(dc.newest(dc.matches(page, dc.SOURCES["xcb-proto"][2])), "1.17.0")

    def test_every_catalog_row_has_a_source(self):
        here = os.path.dirname(os.path.abspath(__file__))
        with open(os.path.join(here, "..", "crates", "pyenv", "deps", "catalog.tsv"), encoding="utf-8") as f:
            keys = [l.split("\t")[0] for l in f if l.strip() and not l.startswith("#")]
        self.assertEqual(sorted(k for k in keys if k not in dc.SOURCES), [])
```

(If `read_rows` already gives the keys, use it instead of the inline parse.)

- [ ] **Step 6: Run the tests**

Run (WSL): `cargo test -q -p pyenv --lib install::catalog`; then on Windows
`python -m unittest discover -s ci -p "test_deps_catalog.py"`.
Expected: all pass (`the_shipped_catalog_has_a_row_and_a_recipe_for_each_library` covers the
six new rows; `every_library_the_check_names_has_catalog_keys` still passes).

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv/deps/catalog.tsv crates/pyenv/src/install/catalog.rs ci/deps_catalog.py ci/test_deps_catalog.py
git commit -m "Catalog: the X11 client libraries for Tk (xorgproto, libXau, xcb-proto, libxcb, xtrans, libX11) with their recipes and weekly sources"
```

---

### Task 2: The bootstrap CPython (`install/bootstrap.rs`)

**Files:**
- Create: `crates/pyenv/src/install/bootstrap.rs`
- Modify: `crates/pyenv/src/install/mod.rs` (add `#[cfg(unix)] pub mod bootstrap;`)
- Modify: `crates/pyenv/src/install/builder.rs:438` (`fn apply_patch` becomes `pub(crate) fn apply_patch`)

**Interfaces:**
- Consumes: `builder::{get, logged, apply_patch}`, `archive::{extract, Kind}`,
  `fetch::{Check, FetchRequest, Fetcher}`, `log::BuildLog`.
- Produces:
  - `pub struct Source { pub name: String, pub url: String, pub sha256: String, pub patches: Vec<(String, Vec<u8>)> }`
  - `pub struct Python { pub exe: PathBuf, .. }` (dropping it removes its folder)
  - `pub fn build(src: &Source, env: &[(OsString, OsString)], fetcher: &Fetcher, jobs: &str, verbose: bool, say: &mut dyn FnMut(&str)) -> Result<Python, InstallError>`

- [ ] **Step 1: Write the failing tests** (`bootstrap.rs`'s `mod tests`). The fake CPython's
  `configure` refuses to run unless the patch was applied; its Makefile's `python` target writes
  a `python` script that exits 1 under `FAKE_NO_EXPAT` (the `import xml.etree…` probe), and
  `sharedmods` must have run first.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::Write;

    // `r##`: the text holds `"#`, which would end an `r#"…"#` string.
    const CONFIGURE: &str = r##"#!/bin/sh
[ -f patched.txt ] || { echo 'not patched'; exit 1; }
printf 'python:\n\tprintf "#!/bin/sh\\n[ -f sharedmods.txt ] || exit 1\\n[ -z \\"$$FAKE_NO_EXPAT\\" ]\\n" > python && chmod +x python\nsharedmods: python\n\ttouch sharedmods.txt\n' > Makefile
"##;

    const PATCH: &str = "diff --git a/patched.txt b/patched.txt\nnew file mode 100644\n--- /dev/null\n+++ b/patched.txt\n@@ -0,0 +1 @@\n+yes\n";

    struct Fake {
        tmp: tempfile::TempDir,
        src: Source,
        env: Vec<(OsString, OsString)>,
    }

    fn fake() -> Fake {
        let tmp = tempfile::tempdir().unwrap();
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut h = tar::Header::new_gnu();
        h.set_size(CONFIGURE.len() as u64);
        h.set_mode(0o755);
        b.append_data(&mut h, "Python-9.9.9/configure", CONFIGURE.as_bytes())
            .unwrap();
        let mut gz = b.into_inner().unwrap().finish().unwrap();
        gz.flush().unwrap();
        let tarball = tmp.path().join("Python-9.9.9.tar.gz");
        std::fs::write(&tarball, &gz).unwrap();
        let sha256 = crate::install::checksum::sha256_file(&tarball).unwrap();
        std::fs::create_dir_all(tmp.path().join("tmp")).unwrap();
        let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(k, _)| k == "PATH")
            .collect();
        env.push(("TMPDIR".into(), tmp.path().join("tmp").into()));
        Fake {
            src: Source {
                name: "Python-9.9.9".into(),
                url: format!("file://{}", tarball.display()),
                sha256,
                patches: vec![("0001-fake.patch".into(), PATCH.as_bytes().to_vec())],
            },
            env,
            tmp,
        }
    }

    fn leftovers(f: &Fake) -> Vec<String> {
        std::fs::read_dir(f.tmp.path().join("tmp"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn builds_from_the_patched_tarball_and_removes_its_folder_when_dropped() {
        let f = fake();
        let mut said = Vec::new();
        let py = build(&f.src, &f.env, &Fetcher::direct(), "2", false, &mut |s: &str| {
            said.push(s.to_string())
        })
        .unwrap();
        assert!(py.exe.is_file(), "{:?}", py.exe);
        assert!(said[0].starts_with("Building a bootstrap Python from Python-9.9.9"), "{said:?}");
        assert_eq!(leftovers(&f).len(), 1, "only the bootstrap folder, no log: {:?}", leftovers(&f));
        drop(py);
        assert!(leftovers(&f).is_empty(), "{:?}", leftovers(&f));
    }

    // Review Focus 4
    #[test]
    fn a_failed_bootstrap_names_its_log_and_leaves_no_folder() {
        let mut f = fake();
        f.env.push(("FAKE_NO_EXPAT".into(), "1".into()));
        let mut said = Vec::new();
        let r = build(&f.src, &f.env, &Fetcher::direct(), "2", false, &mut |s: &str| {
            said.push(s.to_string())
        });
        assert!(matches!(r, Err(InstallError::Failed)));
        let last = said.last().unwrap();
        assert!(last.starts_with("pyenv: building the bootstrap Python from Python-9.9.9 failed; the log is "), "{last}");
        let left = leftovers(&f);
        assert_eq!(left.len(), 1, "{left:?}");
        assert!(left[0].ends_with(".log"), "only the log stays: {left:?}");
    }

    #[test]
    fn the_download_is_cached_under_the_name_the_main_build_fetches() {
        let f = fake();
        let cache = f.tmp.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let env = |k: &str| (k == "PYTHON_BUILD_SKIP_MIRROR").then(|| "1".to_string());
        let fetcher = Fetcher::from_env(&env, Some(cache.clone()));
        let py = build(&f.src, &f.env, &fetcher, "2", false, &mut |_: &str| {}).unwrap();
        drop(py);
        assert!(cache.join("Python-9.9.9.tar.gz").is_file());
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run (WSL): `cargo test -q -p pyenv --lib install::bootstrap`
Expected: compile error (no `bootstrap` module / `build`).

- [ ] **Step 3: Write the module**

```rust
//! A bootstrap CPython for xcb's code generator (M8 design §6.4, ruling C1): xcb-proto's and
//! libxcb's builds run Python, which a machine without root may not have. It is built minimal
//! from the CPython tarball being installed (`make python sharedmods`, about 40 s; measured on
//! 3.7.17, 3.8.20, 3.12.15 and 3.14.8, whose generated C is byte-identical), run from its build
//! tree, and removed when the store run ends.

use super::archive::{extract, Kind};
use super::builder::{apply_patch, get, logged};
use super::fetch::{Check, FetchRequest, Fetcher};
use super::log::BuildLog;
use super::InstallError;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

/// The CPython source a bootstrap is built from: the definition's tarball, its SHA-256 and
/// python-build's patches for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The package name (`Python-3.12.14`). The download is saved as `<name><ext>`, the name
    /// the main build fetches, so a download cache serves both (ruling C6).
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub patches: Vec<(String, Vec<u8>)>,
}

/// A built bootstrap. Dropping it removes its folder.
pub struct Python {
    work: PathBuf,
    pub exe: PathBuf,
}

impl Drop for Python {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.work);
    }
}

/// The two commands, run with `sh -c` in the source tree (`--disable-test-modules` is unknown
/// to 3.7, whose configure only warns; measured).
const STEPS: [&str; 2] = [
    "./configure --without-ensurepip --disable-test-modules",
    "make -j{jobs} python sharedmods",
];

/// Downloads, patches and builds `src` minimal in `$TMPDIR/rpyenv-bootstrap.<pid>`; the result
/// imports `xml.etree.ElementTree` (xcbgen parses XML; `sharedmods` builds `pyexpat`). A
/// failure keeps the log and names it; a failure or Ctrl+C leaves no folder.
pub fn build(
    src: &Source,
    env: &[(OsString, OsString)],
    fetcher: &Fetcher,
    jobs: &str,
    verbose: bool,
    say: &mut dyn FnMut(&str),
) -> Result<Python, InstallError> {
    say(&format!(
        "Building a bootstrap Python from {} (xcb's code generator needs one)...",
        src.name
    ));
    let tmp = get(env, "TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let pid = std::process::id();
    let work = tmp.join(format!("rpyenv-bootstrap.{pid}"));
    let log_path = tmp.join(format!("rpyenv-bootstrap.{pid}.log"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| {
        InstallError::Message(format!("pyenv: cannot create {}: {e}", work.display()))
    })?;
    // From here on, dropping `py` removes the folder, on every path out.
    let mut py = Python {
        work: work.clone(),
        exe: PathBuf::new(),
    };
    let log = BuildLog::open(&log_path, verbose).map_err(|e| {
        InstallError::Message(format!("pyenv: cannot write {}: {e}", log_path.display()))
    })?;
    let result = (|| -> Result<PathBuf, InstallError> {
        let kind = Kind::of_url(&src.url);
        let req = FetchRequest {
            file_name: format!("{}{}", src.name, kind.ext()),
            url: src.url.clone(),
            check: Check::Sha256(src.sha256.clone()),
            dest_dir: work.clone(),
        };
        let mut w: &BuildLog = &log;
        let archive = fetcher.fetch(&req, &mut w, say)?;
        let dir = extract(&archive, kind, &work, &src.name).map_err(|e| {
            log.line(&e);
            InstallError::Failed
        })?;
        let _ = std::fs::remove_file(&archive);
        for (name, text) in &src.patches {
            log.line(&format!("patch {name}"));
            apply_patch(&log, &dir, &work, text, env)?;
        }
        for step in STEPS {
            let line = step.replace("{jobs}", jobs);
            log.line(&format!("+ {line}"));
            let mut c = Command::new("sh");
            c.arg("-c")
                .arg(&line)
                .current_dir(&dir)
                .env_clear()
                .envs(env.iter().map(|(k, v)| (k, v)));
            logged(&log, &mut c)?;
        }
        let exe = dir.join("python");
        log.line("+ ./python -c 'import xml.etree.ElementTree'");
        let mut c = Command::new(&exe);
        c.args(["-c", "import xml.etree.ElementTree"])
            .current_dir(&dir)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)));
        logged(&log, &mut c)?;
        Ok(exe)
    })();
    match result {
        Ok(exe) => {
            drop(log);
            let _ = std::fs::remove_file(&log_path);
            py.exe = exe;
            Ok(py)
        }
        Err(InstallError::Interrupted) => {
            drop(log);
            let _ = std::fs::remove_file(&log_path);
            Err(InstallError::Interrupted)
        }
        Err(e) => {
            if let InstallError::Message(m) = &e {
                say(m);
            }
            say(&format!(
                "pyenv: building the bootstrap Python from {} failed; the log is {}",
                src.name,
                log_path.display()
            ));
            Err(InstallError::Failed)
        }
    }
}
```

Make `builder::apply_patch` `pub(crate)`. Check that `apply_patch`'s `-p1` detection works on
`PATCH` (it starts with `diff --git a/`).

- [ ] **Step 4: Run the tests**

Run (WSL): `cargo test -q -p pyenv --lib install::bootstrap`
Expected: 3 pass. If `the_download_is_cached_…` fails because `Fetcher` skips the cache for
`file://` URLs, read `fetch.rs`'s `fetch` and say so in the report. Don't change the fetcher;
change the test to an HTTP fake (`crate`'s test server, as `fetch.rs`'s tests use).

- [ ] **Step 5: Mutation check.** Remove the `apply_patch` loop: the first test must fail
  ("not patched"). Restore it.

- [ ] **Step 6: Commit**

```bash
git add crates/pyenv/src/install/bootstrap.rs crates/pyenv/src/install/mod.rs crates/pyenv/src/install/builder.rs
git commit -m "Bootstrap CPython for xcb's code generator: built minimal from the tarball being installed, removed afterwards"
```

---

### Task 3: The store builds X11 and Tk against it (`install/store.rs`)

**Files:**
- Modify: `crates/pyenv/src/install/store.rs` (`Build`, `fingerprint`, `ensure`, `build_one`, `link`, tests)

**Interfaces:**
- Consumes: Task 1's `Recipe::{python, x11_args}`, `catalog::X11`; Task 2's `bootstrap::{Source, Python, build}`.
- Produces:
  - `Build { …, pub bootstrap: Option<&'a bootstrap::Source> }`
  - `pub fn fingerprint(r: &Recipe, row: &Row, cc: &str, triple: &str, openssldir: &str, scripts: &str, deps: &[String], store_x11: bool) -> String`
  - `pub fn steps_of(r: &Recipe, store_x11: bool) -> Vec<String>`

- [ ] **Step 1: Pin today's Tk entry name** (Review Focus 2). Add this test **before changing
  anything**, and run it on the current code; it must pass:

```rust
    // Review Focus 2: with the system's X11, an rpyenv upgrade keeps M8b's entries.
    #[test]
    fn tk_without_store_x11_keeps_its_m8b_entry_name() {
        let rows = catalog::parse(include_str!("../../deps/catalog.tsv")).unwrap();
        let row = rows.iter().find(|r| r.key == "tk").unwrap();
        assert_eq!(row.version, "8.6.18", "update this pin when the catalog moves Tk");
        let fp = fingerprint(
            catalog::recipe("tk").unwrap(),
            row,
            "cc",
            "x86_64-linux-gnu",
            "/r/.rpyenv/ssl",
            "/r/.rpyenv/tcltk/tk-8.6.18",
            &["tcl-8.6.18-aaaaaaaaaaaa".to_string()],
        );
        assert_eq!(entry_name("tk", "8.6.18", &fp), "tk-8.6.18-c8a45ea4f16c");
    }
```

(The expected name was computed outside Rust from M8b's fingerprint format. If the test fails on
the unchanged code, replace the constant with the name the test prints, and say so in the
report. It pins today's behavior.) Then add the `false` argument to the call once Step 3
changes the signature.

- [ ] **Step 2: Write the failing tests**

```rust
    #[test]
    fn tk_with_the_stores_x11_gets_the_x11_args_and_another_entry() {
        let tk = catalog::recipe("tk").unwrap();
        assert_eq!(steps_of(tk, false), tk.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let with = steps_of(tk, true);
        assert!(with[0].ends_with(tk.x11_args), "{}", with[0]);
        assert_eq!(&with[1..], &steps_of(tk, false)[1..]);
        let row = Row { key: "tk".into(), version: "1".into(), url: "u".into(), sha256: "s".into() };
        let deps = vec!["tcl-1-x".to_string()];
        assert_ne!(
            fingerprint(tk, &row, "cc", "t", "o", "s", &deps, true),
            fingerprint(tk, &row, "cc", "t", "o", "s", &deps, false)
        );
    }

    // Review Focus 3
    #[test]
    fn a_python_recipe_reused_from_the_store_needs_no_bootstrap() {
        let f = fixture(false);
        let row = Row {
            key: "xcb-proto".into(),
            version: "1.0".into(),
            url: "file:///nonexistent/xcb-proto-1.0.tar.xz".into(),
            sha256: "a".repeat(64),
        };
        let rows = vec![row.clone()];
        let cc = get(&f.env, "CC").filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "cc".into());
        let triple = triple_of(&cc, &f.env);
        let name = entry_name(
            "xcb-proto",
            "1.0",
            &fingerprint(
                catalog::recipe("xcb-proto").unwrap(),
                &row,
                &cc,
                &triple,
                &ssl_dir(&f.root).display().to_string(),
                &scripts_home(&f.root, "xcb-proto", "1.0").display().to_string(),
                &[],
                false,
            ),
        );
        std::fs::create_dir_all(deps_dir(&f.root).join(&name)).unwrap();
        let boot = bootstrap::Source {
            name: "Python-9.9.9".into(),
            url: "file:///nonexistent/Python-9.9.9.tar.xz".into(),
            sha256: "b".repeat(64),
            patches: vec![],
        };
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: ssl_dir(&f.root),
            certs: SystemCerts::default(),
            verbose: false,
            bootstrap: Some(&boot),
        };
        let mut said = Vec::new();
        ensure(&b, &["xcb-proto"], &mut |s: &str| said.push(s.to_string())).unwrap();
        assert!(said.iter().any(|s| s.starts_with("Using xcb-proto 1.0 from ")), "{said:?}");
        assert!(!said.iter().any(|s| s.contains("bootstrap")), "{said:?}");
    }

    #[test]
    fn a_python_recipe_without_a_cpython_tarball_fails_before_downloading() {
        let f = fixture(false);
        let rows = vec![Row {
            key: "xcb-proto".into(),
            version: "1.0".into(),
            url: "file:///nonexistent/xcb-proto-1.0.tar.xz".into(),
            sha256: "a".repeat(64),
        }];
        let def = Definition::default();
        let fetcher = Fetcher::direct();
        let b = Build {
            root: &f.root,
            rows: &rows,
            definition: &def,
            env: &f.env,
            fetcher: &fetcher,
            openssldir: ssl_dir(&f.root),
            certs: SystemCerts::default(),
            verbose: false,
            bootstrap: None,
        };
        let r = ensure(&b, &["xcb-proto"], &mut |_: &str| {});
        match r {
            Err(InstallError::Message(m)) => assert_eq!(
                m,
                "pyenv: building xcb-proto needs Python, and this definition has no CPython source tarball"
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn link_leaves_the_x11_entries_to_tks_flags() {
        let e = |key: &'static str, dir: &str| Entry {
            key,
            version: "1".into(),
            origin: Origin::Catalog,
            dir: PathBuf::from(dir),
        };
        let entries = vec![
            e("xorgproto", "/s/xorgproto-1-a"),
            e("libX11", "/s/libX11-1-b"),
            e("tcl", "/s/tcl-1-c"),
            e("tk", "/s/tk-1-d"),
        ];
        let l = link(entries, 12, &|p: &Path| {
            p.ends_with("tkConfig.sh")
                .then(|| "TK_LIBS='-L/s/libX11-1-b/lib -lX11   -lpthread -lm'\n".to_string())
        });
        let all = format!("{:?} {:?} {:?}", l.cppflags, l.ldflags, l.pkg_config_path);
        assert!(!all.contains("libX11") && !all.contains("xorgproto"), "{all}");
        let libs = &l.configure_env.iter().find(|(k, _)| k == "TCLTK_LIBS").unwrap().1;
        assert!(libs.ends_with("-L/s/libX11-1-b/lib -lX11 -lpthread -lm"), "{libs}");
        assert_eq!(l.entries.len(), 4, "the record still names every entry");
    }
```

Add `use super::super::bootstrap;` (or `use crate::install::bootstrap;`) to the tests' imports.

- [ ] **Step 3: Run them to see them fail**

Run (WSL): `cargo test -q -p pyenv --lib install::store`
Expected: compile errors (`steps_of`, the `bootstrap` field, `fingerprint`'s eighth argument).

- [ ] **Step 4: Implement**

1. `Build` gets, after `verbose`:

```rust
    /// The CPython tarball a bootstrap Python is built from when xcb-proto or libxcb must be
    /// built (M8c); `None` when the definition has none (it fetches CPython with git).
    pub bootstrap: Option<&'a super::bootstrap::Source>,
```

2. `fingerprint` gets `store_x11: bool` as its last parameter; at its end, before returning:

```rust
    // Tk against the store's X11 (ruling C4): another build, another entry. Without it the
    // text is M8b's, so existing entries keep their names.
    if store_x11 {
        s.push_str(r.x11_args);
        s.push('\n');
    }
```

   and its doc gains: "and, for Tk built against the store's X11, its X11 arguments".

3. Add:

```rust
/// A recipe's steps, with its `x11_args` added to the first when the store supplies X11.
pub fn steps_of(r: &Recipe, store_x11: bool) -> Vec<String> {
    let mut s: Vec<String> = r.steps.iter().map(|x| x.to_string()).collect();
    if store_x11 {
        if let Some(first) = s.first_mut() {
            first.push_str(r.x11_args);
        }
    }
    s
}
```

4. In `ensure`: before the `for &key in keys` loop, `let mut boot: Option<super::bootstrap::Python> = None;`
   (dropping it at the end of `ensure`, on every path, removes the bootstrap). In the loop, replace
   `for n in recipe.needs {` with:

```rust
        // Tk against the store's X11, when this run built libX11 (M8c, ruling C4).
        let store_x11 = recipe.x11 && done.iter().any(|e| e.key == "libX11");
        let extra: &[&str] = if store_x11 { &["xorgproto", "libX11"] } else { &[] };
        for n in recipe.needs.iter().chain(extra) {
```

   pass `store_x11` to `fingerprint(...)`, and in the build branch (after `let pkgconf = …;`,
   before `build_one`):

```rust
            if recipe.python {
                let src = b.bootstrap.ok_or_else(|| {
                    message(format!(
                        "pyenv: building {key} needs Python, and this definition has no CPython source tarball"
                    ))
                })?;
                if boot.is_none() {
                    boot = Some(super::bootstrap::build(
                        src,
                        &step_env(b.env, pkgconf.as_deref()),
                        b.fetcher,
                        &jobs,
                        b.verbose,
                        say,
                    )?);
                }
                if let Some(p) = &boot {
                    vars.push(("python".to_string(), p.exe.display().to_string()));
                }
            }
```

   and pass `&steps_of(recipe, store_x11)` to `build_one`.

   The error test expects the refusal before any download, so the `recipe.python` check must sit
   before `build_one`. It must also sit after the lock, as written. An entry that exists never
   reaches it (Review Focus 3).

5. `build_one` takes `steps: &[String]` after `recipe`, and loops `for step in steps` instead of
   `recipe.steps`.

6. In `link`, add an arm before `k => {`:

```rust
            // The X11 entries reach CPython only through Tk's `TK_LIBS` (ruling C5).
            k if catalog::X11.contains(&k) => {}
```

7. Every other `Build { … }` in the tests gets `bootstrap: None,` and every other
   `fingerprint(…)` call gets `false`. Find them with `grep -n "Build {\|fingerprint(" crates/pyenv/src`.
   `commands/install.rs` is wired in Task 4; for now give it `bootstrap: None` so it compiles.

- [ ] **Step 5: Run the tests**

Run (WSL): `cargo test -q -p pyenv --lib install::store` and `cargo clippy -p pyenv --all-targets -- -D warnings`
Expected: all pass, including the pin from Step 1.

- [ ] **Step 6: Mutation checks.** (a) Make `fingerprint` push `x11_args` unconditionally: the
  Step 1 pin fails. (b) Remove the `catalog::X11` arm in `link`: the link test fails. Restore both.

- [ ] **Step 7: Commit**

```bash
git add crates/pyenv/src/install/store.rs crates/pyenv/src/commands/install.rs
git commit -m "Store: xcb-proto and libxcb run a bootstrap Python built once per run; Tk builds against the store's X11 without Xft as a separate entry; X11 entries reach CPython only through TK_LIBS"
```

---

### Task 4: Plan X11 instead of refusing Tk, and wire the bootstrap into `pyenv install`

**Files:**
- Modify: `crates/pyenv/src/install/deps.rs` (`build_plan`, `BuildPlan` doc, tests)
- Modify: `crates/pyenv/src/commands/install.rs` (`resolve_deps`, `install`)
- Test: `crates/pyenv/tests/cli_install_deps.rs`

**Interfaces:**
- Consumes: Task 3's `store::Build::bootstrap`, Task 2's `bootstrap::Source`, Task 1's `keys_for("x11")`.
- Produces: `deps::build_plan(missing: &[Dep], minor: u32, tools: &Tools, x11_source: bool, root_ok: Result<(), String>, m: Option<Manager>) -> BuildPlan`; `BuildPlan::libs` may hold `"x11"`.

- [ ] **Step 1: Write the failing unit tests** (in `deps.rs`'s `mod tests`; use the existing
  `dep` helper and the `Tools` literal style of the tests near line 1054)

```rust
    #[test]
    fn without_x11_headers_the_plan_builds_x11_for_tk() {
        let perl_only = Tools { perl: true, x11: false };
        let p = build_plan(&[dep("tkinter", "tk")], 12, &perl_only, true, Ok(()), Some(Manager::Apt));
        assert_eq!(p.keys, ["xorgproto", "libXau", "xcb-proto", "libxcb", "xtrans", "libX11", "tcl", "tk"]);
        assert!(p.not.is_empty() && p.blocked.is_none(), "{p:?}");
        let both = Tools { perl: true, x11: true };
        let p = build_plan(&[dep("tkinter", "tk")], 12, &both, true, Ok(()), Some(Manager::Apt));
        assert_eq!(p.keys, ["tcl", "tk"], "the system's X11 headers are used");
    }

    #[test]
    fn without_a_cpython_tarball_tk_still_needs_the_x11_headers() {
        let perl_only = Tools { perl: true, x11: false };
        let p = build_plan(&[dep("tkinter", "tk")], 12, &perl_only, false, Ok(()), Some(Manager::Apt));
        assert_eq!(p.not, [("tkinter", "needs the X11 headers (libx11-dev)".to_string())]);
    }
```

Add the new `x11_source` argument to every existing `build_plan(` call in the tests. Pass
`false` in the P3 test near line 1054, which checks the X11-headers reason, so it keeps
checking it. Pass `true` elsewhere.

- [ ] **Step 2: Write the failing CLI tests** (in `cli_install_deps.rs`)

Replace `without_x11_headers_deps_build_refuses_tkinter_before_building_anything` with:

```rust
/// A definition that fetches CPython with git has no tarball for the bootstrap Python, so
/// the store can't build X11 (ruling C7).
#[test]
fn a_git_definition_without_x11_headers_refuses_tkinter() {
    let f = Fixture::plain();
    stubs(&f, 0);
    f.file(
        &f.root.join("plugins/fake/share/python-build/3.12.99-dev"),
        "install_git \"Python-3.12-dev\" \"https://example.invalid/cpython\" \"3.12\" standard verify_py312 ensurepip\n",
    );
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "libuuid,tk"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
    ];
    let r = run(&f, &["install", "--deps=build", "3.12.99-dev"], &p);
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(
        r.stderr.contains(
            "pyenv: --deps=build can't build tkinter here: needs the X11 headers (libx11-dev)\n"
        ),
        "{}",
        r.stderr
    );
    assert_eq!(uuid_runs(&f), 0, "nothing was built");
    assert!(store_entries(&f).is_empty());
}

/// M8c: without X11 headers, `[b]` covers tkinter too (the store builds X11).
#[test]
fn at_a_terminal_b_offers_tkinter_without_x11_headers() {
    let f = Fixture::plain();
    plugin_def(&f);
    stubs(&f, 0);
    let cat = fake_catalog(&f).display().to_string();
    let p = [
        ("RPYENV_TEST_PREFLIGHT", "tk"),
        ("RPYENV_TEST_BUILD_TOOLS", "perl"),
        ("RPYENV_TEST_CATALOG", cat.as_str()),
        ("RPYENV_TEST_TTY", "1"),
    ];
    let r = run_input(&f, &["install", "3.12.99"], &p, "p\n");
    assert!(
        r.stderr.contains("  [b] build them from source for rpyenv, no root\n"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("not tkinter"), "{}", r.stderr);
    assert!(store_entries(&f).is_empty(), "p builds nothing");
}
```

`at_a_terminal_b_builds_what_it_can_and_the_rest_comes_back_to_the_prompt` relied on Tk being
left out. Make Perl the missing tool instead: `RPYENV_TEST_PREFLIGHT` = `"libuuid,libcrypt"`,
`RPYENV_TEST_BUILD_TOOLS` = `"x11"`. Expect the prompt to name the crypt module. Take its exact
name and text from the prompt the test prints (e.g. `(not crypt: needs Perl 5 with its core
modules, perl)`). Keep the rest of the test as is.

- [ ] **Step 3: Run them to see them fail**

Run (WSL): `cargo test -q -p pyenv --lib install::deps` and `cargo test -q -p pyenv --test cli_install_deps`
Expected: compile error for the new argument; once it compiles,
`at_a_terminal_b_offers_tkinter_without_x11_headers` fails (`not tkinter` is printed).

- [ ] **Step 4: Implement `build_plan`.** Add the `x11_source: bool` parameter after `tools`,
  and replace the X11 branch:

```rust
        } else if needs_x11 && !tools.x11 && !x11_source {
            not.push((d.name, with_package("needs the X11 headers", false)));
        } else {
            libs.push(d.lib);
            // No X11 headers: the store builds the X11 libraries too (M8c, spec §6.4).
            if needs_x11 && !tools.x11 && !libs.contains(&"x11") {
                libs.push("x11");
            }
        }
```

Document the parameter on `build_plan`. `x11_source` means the definition has a CPython source
tarball, from which the store's bootstrap Python is built (ruling C7). Extend `BuildPlan::libs`'
doc: "…, and `x11` when the store builds X11 for Tk".

- [ ] **Step 5: Wire it in `commands/install.rs`.**

1. `resolve_deps` gets `x11_source: bool` after `root`, and passes it to `deps::build_plan`.
2. In `install`, after `minor` is computed (around line 907), derive the bootstrap's source:

```rust
        // The CPython tarball a bootstrap Python for xcb's generator is built from (M8c); none
        // when the definition fetches CPython with git (ruling C7).
        let boot_src: Option<crate::install::bootstrap::Source> = parsed
            .packages
            .iter()
            .find(|p| p.steps.iter().any(|s| s == "standard" || s == "python"))
            .and_then(|p| match &p.fetch {
                crate::install::defs::Fetch::Tarball { url, fragment } => {
                    crate::install::checksum::sha256_of_fragment(url, fragment.as_deref())
                        .ok()
                        .map(|sha256| crate::install::bootstrap::Source {
                            name: p.name.clone(),
                            url: url.clone(),
                            sha256,
                            patches: builder::patch_files(&found, &p.name),
                        })
                }
                _ => None,
            });
```

3. Move the download-cache computation into a helper next to `install`:

```rust
/// The download cache (reference "Cache"): `PYTHON_BUILD_CACHE_PATH`, else `$PYENV_ROOT/cache`,
/// when it is a directory.
fn download_cache(env: &[(OsString, OsString)], root: &Path) -> Option<PathBuf> {
    match get_os(env, "PYTHON_BUILD_CACHE_PATH") {
        Some(p) => Some(PathBuf::from(p)),
        None => Some(root.join("cache")),
    }
    .filter(|c| c.is_dir())
}
```

4. In the store closure, make the store's own fetcher after creating the store folder (ruling C6),
   and pass the bootstrap:

```rust
                // `cache/deps` turns on `$PYENV_ROOT/cache`: made first, so the store's
                // downloads (the bootstrap's CPython tarball among them) are cached and the main
                // build finds that tarball there (ruling C6).
                let _ = std::fs::create_dir_all(store::deps_dir(&abs_root));
                let fetcher = Fetcher::from_env(
                    &|k: &str| builder::get(&snapshot, k),
                    download_cache(&snapshot, &abs_root),
                );
                …
                        bootstrap: boot_src.as_ref(),
```

5. Make the builder's `fetcher` after the dependency block instead of before it:
   `let fetcher = Fetcher::from_env(&job_get, download_cache(&job_env, &ctx.root));` placed just
   before `let linked = …`. Remove the old `let cache = …; let fetcher = …;` lines.
6. Pass `boot_src.is_some()` to `resolve_deps`.

- [ ] **Step 6: Run the tests**

Run (WSL): `cargo test -q -p pyenv` (whole crate) and `cargo clippy -p pyenv --all-targets -- -D warnings`
Expected: all pass.

- [ ] **Step 7: Mutation check.** In `build_plan`, drop `&& !x11_source` from the refusal
  condition: `at_a_terminal_b_offers_tkinter_without_x11_headers` fails. Restore it.

- [ ] **Step 8: Commit**

```bash
git add crates/pyenv/src/install/deps.rs crates/pyenv/src/commands/install.rs crates/pyenv/tests/cli_install_deps.rs
git commit -m "Without X11 headers, [b] and --deps=build build X11 for tkinter from the definition's CPython tarball; the store's downloads go through the download cache"
```

---

### Task 5: Scripts become strict (spec D13)

**Files:**
- Modify: `crates/pyenv/src/commands/install.rs` (the lenient branch and `RPYENV_DEPS_MODE=lenient`)
- Modify: `crates/pyenv/src/install/builder.rs` (`stdlib_check`, `lenient_summary` → `continue_summary`, tests)
- Modify: `crates/pyenv/src/install/preflight.rs`, `crates/pyenv/src/install/deps.rs` (comments; `report_deps_with`'s `refuse` if unused)
- Test: `crates/pyenv/tests/cli_install_deps.rs`

**Interfaces:**
- Produces: no new API. `RPYENV_DEPS_MODE` is `continue` or unset.

- [ ] **Step 1: Rewrite the D-117 test to the new behavior** (replace
  `without_a_terminal_or_flag_other_libraries_only_warn_and_the_build_goes_on`):

```rust
// allowlist D-117
// Review Focus 5
#[test]
fn without_a_terminal_or_flag_a_missing_optional_library_stops_the_build() {
    let f = Fixture::new();
    plugin_def(&f);
    stubs(&f, 0);
    let r = run(
        &f,
        &["install", "3.12.99"],
        &[("RPYENV_TEST_PREFLIGHT", "tk,libuuid")],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    for want in [
        "pyenv: cannot build Python: missing build dependencies:\n",
        "\n  sudo apt-get install tk-dev uuid-dev\n",
        "Without root, pyenv install --deps=build compiles them from source.\n",
        "To build anyway, set RPYENV_SKIP_PREFLIGHT=1.",
    ] {
        assert!(r.stderr.contains(want), "{want:?} in {}", r.stderr);
    }
    assert!(!r.stderr.contains("In a later rpyenv release"), "{}", r.stderr);
    assert!(!installed(&f));
    assert_eq!(log(&f), "", "no package manager ran");
}
```

Change `a_users_deps_mode_does_not_make_a_strict_run_lenient` to
`a_users_deps_mode_does_not_make_a_run_continue`: no `--deps=system` (a flagless run is strict
now), `("RPYENV_DEPS_MODE", "continue")` instead of `lenient`, the same assertions plus
`assert!(!r.stderr.contains("Add them:"), "{}", r.stderr);`. Rename
`skip_preflight_in_lenient_mode_warns_without_the_summary` to
`skip_preflight_without_a_terminal_warns_and_keeps_the_build` (body unchanged; doc: "D13 is
strict since M8c; RPYENV_SKIP_PREFLIGHT=1 still gives P1's warning line"). In the comments of
`without_a_terminal_or_flag_openssl_still_stops_the_build` and `…awk…`, replace "Lenient mode
(D13): … is in the hard set" with "Without a terminal or flag (D13, strict)". Update the
file's top doc ("…and the strict default for scripts (D13)").

- [ ] **Step 2: Run it to see it fail**

Run (WSL): `cargo test -q -p pyenv --test cli_install_deps without_a_terminal_or_flag_a_missing_optional_library_stops_the_build`
Expected: FAIL (code 0: lenient mode builds).

- [ ] **Step 3: Implement.**

1. `resolve_deps`: replace

```rust
        } else if let Some(r) = chosen {
            r
        } else {
            // D13, lenient mode: …
            …
        };
```

   with

```rust
        } else {
            // D13 (strict since M8c): without a terminal and without `--deps` or
            // RPYENV_BUILD_DEPS, every module is required, as with `--deps=system`.
            chosen.unwrap_or(Route::System)
        };
```

2. `install`: delete the `strict` computation and the `lenient` push; keep the `retain`:

```rust
        // RPYENV_DEPS_MODE is rpyenv's own (`continue` after `[c]`): one set by hand must not
        // turn a failed check into a summary (review M8).
        job_env.retain(|(k, _)| k != "RPYENV_DEPS_MODE");
```

3. `builder.rs`: in `stdlib_check`, the summary branch becomes
   `if !skip && mode.as_deref() == Some("continue") {` and says `continue_summary(…)` directly
   (no split). Rename `lenient_summary` to `continue_summary`. Its doc: "The summary after `[c]`
   (M8 design §5.1): what the build lacks and the command that adds it". Its `format!` ends after
   `{crb}`; the two "In a later rpyenv release…" lines go. Update `stdlib_check`'s doc
   ("(strict), a warning (RPYENV_SKIP_PREFLIGHT=1, ruling P1) or the `[c]` summary") and its
   comments. In the builder tests, rename `the_lenient_summary_…` to `the_continue_summary_…`
   and drop the two notice lines from their expected text.
4. `deps.rs` line 15 and 22, `preflight.rs` line 309 and 1766: replace "lenient mode" wording with
   what is true now ("`[c]`" for `is_hard`; the strict default for the report). Grep for callers of
   `report_deps_with` and `report_deps`. If every caller now passes `refuse: true`, remove the
   parameter and the "the build will lack these optional modules:" header, and update the unit
   tests that pass `false`. Otherwise leave it, and say which caller still passes `false`.
5. `grep -rn "lenient\|later rpyenv release" crates/` must show only unrelated hits
   (`winconda.rs`'s publisher line, the prebuilt route's "comes in a later rpyenv release").

- [ ] **Step 4: Run the whole suite**

Run (WSL): `cargo test -q --workspace`. Some tests expect success from a run that has no flag,
no terminal and a missing optional library. They relied on lenient mode. For each one: if the
missing library is the point of the test, expect the refusal; if it is incidental, set
`RPYENV_TEST_PREFLIGHT` to `""`. List each change in the report.
Expected: all pass. `cargo clippy --workspace --all-targets -- -D warnings` clean.

- [ ] **Step 5: Mutation check.** Put back `chosen.unwrap_or(Route::Continue)` in place of
  `Route::System`: the D-117 test fails. Restore.

- [ ] **Step 6: Commit**

```bash
git add crates/pyenv/src crates/pyenv/tests
git commit -m "D13: scripts that set neither --deps nor RPYENV_BUILD_DEPS are strict, as --deps=system; the lenient summary and its later-release notice go"
```

---

### Task 6: CI and the container runs

**Files:**
- Modify: `ci/check_deps.sh`
- Modify: `.github/workflows/install-linux.yml` (comments only)

- [ ] **Step 1: Debian and Ubuntu build legs lose the X11 headers.** In `ci/check_deps.sh` stage
  0, both `apt-get install` lines of the BUILD branch drop `libx11-dev`. Update the header
  comment: "BUILD=1 … the container gets a compiler, make, patch, Perl and CA certificates (the
  X11 headers too, except on Debian and Ubuntu, where the store builds X11: M8c) …".

- [ ] **Step 2: Assert who supplied X11.** After `cat "$rec"` in stage B1:

```bash
  # M8c: Debian and Ubuntu have no X11 headers, so the store built X11 and Tk uses it; the
  # others use the system's X11 headers (D9).
  tkso=$(ls "$root/versions/$v"/lib/python3*/lib-dynload/_tkinter*.so)
  case "$distro" in
    debian|ubuntu)
      grep -q '^libX11 ' "$rec" || { echo "::error::$distro: libX11 is not in the record"; exit 1; }
      grep -q '^Building a bootstrap Python from Python-' /tmp/build1.log ||
        { echo "::error::$distro: no bootstrap Python was built"; exit 1; }
      if ldd "$tkso" | grep -q 'libX11'; then
        ldd "$tkso"; echo "::error::$distro: _tkinter links a shared libX11"; exit 1
      fi ;;
    *)
      if grep -q '^libX11 ' "$rec"; then echo "::error::$distro: X11 was built despite the system's headers"; exit 1; fi ;;
  esac
```

- [ ] **Step 3: A real window, without the store** (Ubuntu, BUILD, not NO_CA): after stage B4,
  before the WEEKLY block:

```bash
  # Stage B5 (M8c, Ubuntu): Tk built against the store's X11 opens a window under Xvfb with the
  # store moved aside (ruling C3: libX11's data comes from the system's path), and again without
  # any X11 locale data.
  if [ "$distro" = ubuntu ] && [ -z "${NO_CA:-}" ]; then
    apt-get update -q && apt-get install -yq --no-install-recommends xvfb xauth >/dev/null
    rm -rf /var/lib/apt/lists/*
    mv "$root/cache/deps" "$root/cache/deps.off"
    win='import tkinter; r = tkinter.Tk(); e = tkinter.Entry(r); e.pack(); e.insert(0, "x"); r.update(); print("a window opened on", r.winfo_screen()); r.destroy()'
    xvfb-run -a "$root/versions/$v/bin/python3" -c "$win" ||
      { echo "::error::$distro: no Tk window"; exit 1; }
    mv /usr/share/X11/locale /usr/share/X11/locale.off
    LANG=C.UTF-8 xvfb-run -a "$root/versions/$v/bin/python3" -c "$win" ||
      { echo "::error::$distro: no Tk window without X11 locale data"; exit 1; }
    mv /usr/share/X11/locale.off /usr/share/X11/locale
    mv "$root/cache/deps.off" "$root/cache/deps"
  fi
```

- [ ] **Step 4: Weekly, 3.8's bootstrap builds libxcb against the xcb-proto an earlier bootstrap
  made** (ruling C9). In the WEEKLY loop, before `"$bin" install --deps=build "$w"`:

```bash
      # Ubuntu (M8c, ruling C9): 3.8's bootstrap rebuilds libxcb with the xcb-proto entry that
      # 3.12's bootstrap installed.
      if [ "$distro" = ubuntu ] && [ "${w%.*}" = 3.8 ]; then rm -rf "$root"/cache/deps/libxcb-*; fi
```

- [ ] **Step 5: Stage 1 is strict without a flag now.** Replace the Arch special case:

```bash
# Stage 1: no terminal, no flags (D13, strict since M8c): what is missing stops the build before
# anything is downloaded, with the report and its install command.
rc=0
out=$("$bin" install "$v" 2>&1 </dev/null) || rc=$?
```

  (drop the `flag=…` lines and `$flag`).

- [ ] **Step 6: Workflow comments.** In `.github/workflows/install-linux.yml`'s comment above
  `deps:`, say the Ubuntu build leg (every PR) and the Debian one (weekly) have no X11 headers,
  so they build X11 (M8c, spec §9 row 4).

- [ ] **Step 7: Syntax check.** Run `bash -n ci/check_deps.sh` → no output.

- [ ] **Step 8: Container runs** (WSL scripts as in M8b's Task 6: build the static musl
  `pyenv`/`pyenv-shim` in a `rust:latest` container into `/home/jm/rpyenv-m8-musl`, then run
  `ci/check_deps.sh` in a fresh container per leg, logs to `/home/jm/m8c_logs/`):
  1. `ubuntu:24.04`, `BUILD=1 WEEKLY=1`, version `3.12.14`, to `== the build route works on ubuntu`.
  2. `fedora:44`, `BUILD=1`, version `3.13` (system X11 path), to its success line.
  3. `ubuntu:24.04`, no BUILD, `DETECT_ONLY=1`, to `== detection is empty on ubuntu` (stage 1's
     strict flagless run).
  Record the wall-clock and every `== ` line of each run in the report.
  4. **By hand, on a real X server (WSLg):** keep run 1's container root (`-v` a WSL folder as
     `PYENV_ROOT`). Start a fresh `ubuntu:24.04` container with only that folder,
     `-v /tmp/.X11-unix:/tmp/.X11-unix -e DISPLAY=:0`, and no X11 packages, so
     `/usr/share/X11/locale` is absent (check it with `ls`). Run a Tk `Entry` in that container's 3.12.14
     with `LANG=C.UTF-8`. The user types `ñ`, `é` and a dead-key `´ e` on their keyboard. Record
     what appears, then repeat once with `libx11-data` installed in the container as the control.
     The result replaces the "Not measured" line about non-ASCII input in Measured.

- [ ] **Step 9: Commit**

```bash
git add ci/check_deps.sh .github/workflows/install-linux.yml
git commit -m "CI: Debian and Ubuntu build legs have no X11 headers, so the store builds X11; a Tk window opens under Xvfb without the store; weekly, 3.8's bootstrap rebuilds libxcb; stage 1 runs flagless"
```

---

### Task 7: Docs, allowlist, spec, README

**Files:**
- Modify: `docs/parity/allowlist.md` (D-117, D-119, D-122, D-123)
- Modify: `docs/distributions.md`
- Modify: `README.md` (the `RPYENV_BUILD_DEPS` row)
- Modify: `docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md`

- [ ] **Step 1: Allowlist.**
  - D-117: replace the "A script that sets neither … (lenient mode) keeps upstream's
    behavior: …" sentence with: "A script that sets neither `--deps=` nor `RPYENV_BUILD_DEPS` and
    has no terminal is treated as `--deps=system`: anything missing stops the build, with the
    report." Change the reason: "…; since M8c for CPython 3.7 and later a script without root can usually answer with
    `--deps=build` (M8 design D13, §4, §5.2)".
  - D-119: delete "In lenient mode … as other optional libraries".
  - D-122: "With `RPYENV_SKIP_PREFLIGHT=1` the failures are a one-line warning; after `[c]` they
    are a short summary; in both the build stays."
  - D-123: replace "(no X11 headers for Tk, no Perl modules for OpenSSL)" with "(no Perl modules
    for OpenSSL; for Tk without X11 headers, a definition that fetches CPython with git)". Add:
    "Without X11 headers, it also builds the X11 client libraries Tk needs (Tk without Xft),
    running xcb's code generator with a minimal CPython built from the version's own tarball and
    removed afterwards."
- [ ] **Step 2: `docs/distributions.md`.**
  - "Scripts that set neither": "Without a terminal, `--deps` or `RPYENV_BUILD_DEPS`, rpyenv
    treats the run as `--deps=system`: anything missing stops the build, and the report names the
    packages and, where it can run, `--deps=build`. (Before M8c rpyenv only warned, as upstream
    does.)"
  - "After the build": drop "In lenient mode this is the three-line summary, and the build
    stays." and say "After `[c]` this is a short summary, and the build stays."
  - "What the system still needs": "…; Tk uses the system's X11 headers when it has them. Without
    them rpyenv builds the X11 client libraries too (about a minute, plus about 40 s for a minimal
    Python from the same tarball that runs xcb's code generator). A version whose definition
    fetches CPython with git (`3.14-dev`) can't build X11 that way and needs the headers."
  - Add: "A self-built X11 reads its locale data from `/usr/share/X11/locale` when the system has
    it. Without that folder (package `libx11-data` or `libX11-common`), Tk windows open and
    plain typing works; dead keys and Compose sequences may not."
- [ ] **Step 3: README.** The `RPYENV_BUILD_DEPS` row: "…With it set, rpyenv doesn't ask.
  Without it, `--deps` or a terminal, a missing library stops the build."
- [ ] **Step 4: Spec.** In the M8 design:
  - D13: append "Done in M8c (2026-10-20)."
  - Add D14: "| D14 | Python for xcb's code generator (user, 2026-10-19) | A minimal CPython built
    from the tarball being installed, used from its build tree and removed afterwards (measured on
    3.7.17, 3.8.20, 3.12.15 and 3.14.8: libxcb's generated C is byte-identical). Chosen over the
    system's `python3` first (two paths), requiring `python3` (fails in minimal containers),
    pregenerated C published by rpyenv (modified sources), a pinned bootstrap CPython (an extra
    download; kept as the fallback had 3.7/3.8 failed) and others. |"
  - §5.2: replace "With neither set, D13's phased rule applies: until M8c, …" and its summary
    block with the strict rule: "With neither set, the run is `--deps=system` (D13)."
  - §6.4: "xorgproto, libXau, xcb-proto, libxcb, xtrans and libX11 join the catalog, all static
    (libXext is not needed: measured in M8c, Tk 8.6 links only libX11). libX11 absorbs libxcb and
    libXau, and reads its locale data from `/usr/share/X11/locale`. xcb-proto and libxcb need
    Python (D14)."
  - §6.5's last sentence: drop "and (until M8c) the system's X11 headers for Tk".
- [ ] **Step 5: Coverage.** Run `python parity/coverage.py` → every row covered (D-117 is cited
  by the rewritten test).
- [ ] **Step 6: Commit**

```bash
git add docs/parity/allowlist.md docs/distributions.md README.md docs/superpowers/specs/2026-10-17-m8-linux-build-deps-design.md
git commit -m "Docs: X11 from source (D-123, distributions.md, spec §6.4 and D14) and strict scripts (D-117, D-119, D-122, README, spec D13 and §5.2)"
```

---

## Running the tests

- WSL (`/mnt/c/tmp/<name>.sh`, run with `wsl -d Debian --exec /usr/bin/bash -l <script>`):
  `cd /mnt/c/Users/JM/repos/OWN/rpyenv && CARGO_TARGET_DIR=/home/jm/rpyenv-m8-target cargo test -q --workspace`,
  then `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`. A stale
  build cache once made panel tests fail: `cargo clean -p pyenv` first if a result looks impossible.
- Windows: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- `python parity/coverage.py`; `python -m unittest discover -s ci -p "test_deps_catalog.py"`;
  `bash -n ci/check_deps.sh`; control characters: `grep -nP "[\x00-\x08\x0b\x0c\x0e-\x1f]"` on
  changed files.
