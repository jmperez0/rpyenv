# M8: every standard-library module works after `pyenv install` on Linux

Parent spec: [docs/specs/2026-09-27-rpyenv-design.md](../../specs/2026-09-27-rpyenv-design.md)
§15.2. This document replaces §15.2's open questions with decisions and adds what the
user asked for on 2026-10-17. It also changes two of §15.2's plans: the "optional"
libraries (Tcl/Tk, libuuid, gdbm) are now resolved like the required ones (D1), and the
build route links libraries statically from a store instead of installing shared
libraries into each version with an `$ORIGIN` path (D7). Status: design approved in
conversation, 2026-10-17.

## 1. Goal

On Linux, `pyenv install <cpython version>` produces a Python in which **every
standard-library extension module that this version of CPython builds works**: `ssl`
(with trusted CA certificates), `tkinter`, `sqlite3`, `ctypes`, `lzma`, `bz2`, `zlib`,
`readline`, `curses`, `dbm`, `uuid` and the rest. A missing library is either installed
through the package manager or compiled from source. It is never silently skipped.

**Out of scope:** third-party packages that need system libraries (`psycopg2`, `lxml`,
...). pip installs those, and most ship prebuilt wheels. Windows is unaffected: it uses
python.org's builds.

## 2. Decisions

| # | Topic | Decision (who) |
|---|---|---|
| D1 | Scope | The standard library only, `tkinter` included (user). |
| D2 | Certificates | `ssl` counts as working only when trusted CA certificates are present (user). |
| D3 | Distributions | glibc distributions in M8: Debian/Ubuntu (apt), Fedora/RHEL/Rocky/Alma (dnf), openSUSE (zypper), Arch (pacman), including WSL and containers based on them. Alpine/musl in a later milestone (user). |
| D4 | Choosing a route at a terminal | Ask each time; Enter means "print the commands and stop" (user). |
| D5 | Without a terminal | `--deps=<route>` or `RPYENV_BUILD_DEPS` chooses, and then every module is required. With neither set, D13 applies. |
| D6 | Reusing compiled libraries | Yes, when library, version and build options match (user). |
| D7 | How a compiled library is reused | Static linking from a store that is only a build cache (user chose option D of six; see §8). |
| D8 | Prebuilt CPython | python-build-standalone as a route the user can select, never the default, after the build route (user). |
| D9 | X11 for `tkinter` | From the package manager when the user can install packages; compiled from source when they can't (user). |
| D10 | Fonts in a no-root Tk | Tk built without Xft in the no-root case: fewer libraries, plainer fonts (accepted in review of §6). |
| D11 | Tcl/Tk version | 8.6 for every source build, as CPython's own builds. Tk 9 only through the prebuilt route. |
| D12 | Parity | Upstream pyenv has none of this. Each user-visible difference gets an allowlist entry. |
| D13 | Scripts that set nothing (user, after a side-agent note) | **Phased.** Until M8b ships, a run with no terminal and neither `--deps` nor `RPYENV_BUILD_DEPS` keeps today's rule: the compiler, `make`, `patch`, `awk`, OpenSSL and zlib stop the build (`awk` added after the first real runs: Tumbleweed's base image has no `awk`, and CPython's configure needs it); every other missing library only warns, and the build goes on. The run ends with a summary naming what is missing, the command that adds it, and the notice that a later release makes this stop the build. **M8c** makes the script default strict (`system`), because from then on, for CPython 3.7 and later, a script without root can usually answer with `--deps=build` (not without Perl for OpenSSL and libxcrypt, nor for a definition fetched with git on a machine without X11 headers; `RPYENV_SKIP_PREFLIGHT=1` keeps the old behaviour) (moved from M8b by the user on 2026-10-17: M8b can't build X11, so a script without root on a machine without X11 headers could not pass). Chosen over strict everywhere (breaks today's Dockerfiles on upgrade, while a script without root could only fail), lenient for good (scripts never get every module), a policy per pyenv root (Dockerfiles make fresh roots), automatic installs as root (installs packages nobody approved) and splitting libraries by weight (still breaks on the newly checked gdbm, uuid and crypt). Done in M8c (2026-10-20). |
| D14 | Python for xcb's code generator (user, 2026-10-19) | A minimal CPython built from the tarball being installed, used from its build tree and removed afterwards (measured on 3.7.17, 3.8.20, 3.12.15 and 3.14.8: libxcb's generated C is byte-identical). Chosen over the system's `python3` first (two paths), requiring `python3` (fails in minimal containers), pregenerated C published by rpyenv (modified sources), a pinned bootstrap CPython (an extra download; kept as the fallback had 3.7/3.8 failed) and others. |
| D15 | Prebuilt: where trust comes from (user, 2026-10-21) | A table built into rpyenv (`crates/pyenv/prebuilt/linux.tsv`), like the PyPy and conda tables. The weekly sync downloads each new asset, checks its SHA-256 against the release's, and runs `gh attestation verify` (repository and signing workflow pinned) before it writes the row; `pyenv install` checks only the SHA-256 against the table. Chosen over `gh` at install time (needs `gh` logged in), both (a check that runs only on some machines) and Sigstore verification in Rust (a large dependency). The table lags a new build by up to a week, as the vendored definitions already do. |
| D16 | Prebuilt: which builds (user, 2026-10-21) | Only releases with attestations (20250311 and later), and for each version, arch and variant the newest release that built it. Measured: no release before 20250311 has attestations; the 2022 to 2024 builds bundle outdated OpenSSL (3.10.4 of 20220528 has 1.1.1o, unsupported since 2023-09), the 2022 one doesn't start on Fedora 44 (needs `libcrypt.so.1`), and Tcl doesn't start in their virtual environments. The user first chose every published build, then this, after the measurements. |
| D17 | Prebuilt: dbm.gnu and nis (user, 2026-10-21) | python-build-standalone ships neither (gdbm is GPL; nis is not built). §4.4 skips exactly these two for a prebuilt build, and the prompt and messages say so. |
| D18 | Prebuilt: compiler settings (user, 2026-10-21) | After unpacking, rpyenv rewrites the build's compiler settings (`clang` to `cc`, `clang++` to `c++`, `/install` to the version's folder) in `_sysconfigdata_*.py`, `bin/python3.X-config` and `config-3.X-*/Makefile`, as uv does. Measured: without it, pip can't build a C extension on a system with gcc only ("No such file or directory: 'clang'"); with it, the extension built and imported. |
| D19 | Prebuilt: what the user is told (user, 2026-10-21) | The build's date (its release tag) and its bundled OpenSSL version, on the `[s]` line and when it installs. The bundled OpenSSL can't be swapped: `_ssl` and `_hashlib` are built into the interpreter with OpenSSL linked statically (measured on 3.8.20, 3.9.25 and 3.13.16). |

## 3. Parts

M8 ships in four parts. Each has its own plan and pull request and is useful on its own.

| Part | Delivers | Needs |
|---|---|---|
| **M8a** detect and install | The complete detection (§4); the prompt and the `system` and `install` routes (§5); the check after the build (§4.4) | M2's pre-flight check |
| **M8b** build from source | The catalog, the store and static builds of every library except X11 (§6); scripts stay lenient (D13) | M8a |
| **M8c** X11 from source | The X11 client libraries in the catalog, for the no-root case (§6.4); scripts become strict (D13) | M8b |
| **M8d** prebuilt CPython | The `prebuilt` route (§7); done in M8d (2026-10-21) | M8a |
| later | Alpine/musl | M8b |

## 4. Detection (M8a)

### 4.1 What exists

`crates/pyenv/src/install/preflight.rs` (M2) checks for a compiler, `make`, `patch`, and
the headers for `ssl`, `zlib`, `bz2`, `readline`, `sqlite3`, `ctypes`, `curses`, `lzma`
and `tkinter`. Each probe already has package names for apt, dnf, zypper, pacman and
apk, and the result is a structured list (`Dep`, `Missing`).

### 4.2 What M8a adds

1. **A module list per CPython version.** For each extension module: the versions that
   build it (`_crypt` and `nis` stop at 3.12), its library, its probe, its minimum
   library version from CPython's `configure` (for example OpenSSL 1.1.1 from 3.10 on),
   and its package per distribution.
2. **New probes:** `dbm` (gdbm or ndbm), `uuid` (libuuid), `_crypt` (libcrypt, older
   versions only), `_curses_panel` (panel), `pkg-config`, and CA certificates.
3. **Stronger probes.** Each probe compiles **and links** a small program, so a header
   without its library counts as missing. Where a library has a version macro
   (`OPENSSL_VERSION_NUMBER`, `SQLITE_VERSION_NUMBER`), the probe reads it, and a
   version that is too old counts as missing.
4. **The result** names, for every missing item: the module the user would lose, the
   catalog key (§6.1), why it is missing (no program, no header, can't link, too old:
   "found 1.0.2, needs 1.1.1 or later"), and the package for the distribution found in
   `/etc/os-release`.

### 4.3 CA certificates

Present when OpenSSL's default CA file exists, or its default CA directory holds
certificates. With the system's OpenSSL, that is `/etc/ssl` (Debian, Arch, SUSE) or
`/etc/pki/tls` (Fedora, RHEL). If neither has certificates, the missing package is
`ca-certificates`.

### 4.4 The check after the build

After CPython is built, before the version counts as installed:

- every module on the list for this version imports;
- `ssl.get_default_verify_paths()` names a CA file that exists or a CA directory that
  holds certificates (no network);
- `import tkinter` and `tkinter.Tcl()` work without a display; when the build route supplied
  Tcl/Tk, Tcl's `info library` is outside the build cache, and Tcl also starts in a virtual
  environment made from the version.

A failure names the module and the library. It fails the install as any build error
does.

## 5. Routes and how the user chooses (M8a)

### 5.1 At a terminal

```
pyenv: 3.13.1 can't build these modules: ssl, tkinter, uuid
  ssl      OpenSSL 1.0.2 found, needs 1.1.1+   (libssl-dev)
  tkinter  no tk.h                              (tk-dev)
  uuid     no uuid/uuid.h                       (uuid-dev)
How should rpyenv get them?
  [p] print the commands and stop (default)
  [i] install with sudo: sudo apt-get install libssl-dev tk-dev uuid-dev
  [b] build them from source for rpyenv, no root
  [s] use a prebuilt CPython instead
  [c] continue without them (the build warns about them at the end)
```

- **`p`** prints the commands and exits as today. Enter chooses it.
- **`i`** shows the exact command, runs it through `sudo` (which asks for the password
  itself), then runs detection again. Anything still missing gets the prompt again,
  without `i`. rpyenv never adds `-y` to a command it has not shown.
- **`i` is left out**, with the reason stated, when there is no `sudo`, when
  `sudo -n true` reports the user may not use it, or when the distribution is not
  recognized. Running as root (most containers), `i` runs the command without `sudo`.
- **`b`** appears from M8b on, **`s`** from M8d on.
- **`c`** (added after M8a, user decision) builds without the missing libraries and ends
  with a short summary (since M8c; D13's earlier three-line summary had a "later release"
  notice). It is offered only when nothing
  in D13's hard set (compiler, `make`, `patch`, `awk`, OpenSSL, zlib) is missing, so a user
  without sudo is never stuck before M8b brings `b`.

### 5.2 Without a terminal

`pyenv install --deps=system|install|build|prebuilt` or `RPYENV_BUILD_DEPS` with the
same values; then every module is required, as at a terminal. `install` uses `sudo -n`
and fails with a clear message when `sudo` needs a password. With neither set, the run is `--deps=system` (D13).

### 5.3 Distribution details

- **apt:** when a package can't be found, the command shown starts with
  `sudo apt-get update` (minimal images have no package lists).
- **RHEL, Rocky, Alma:** some `-devel` packages live in the CRB (PowerTools) repository.
  The message names it and prints the command that enables it.
- **An unrecognized distribution:** `p` lists the libraries without a command.

## 6. Building from source without root (M8b, M8c)

### 6.1 The catalog

A checked-in table, one row per library: version, source URL, SHA-256, build options,
and the libraries it depends on. Where upstream `python-build` pins a library for a
CPython version (OpenSSL 1.1 for old releases), that pin wins. The weekly sync job opens
a pull request when a newer upstream release of a catalog library appears, as it does
for the conda table.

### 6.2 The store

- **Where:** `<root>/cache/deps/<lib>-<version>-<options hash>/`, with `include/`,
  `lib/*.a`, `lib/pkgconfig/` and, for Tcl/Tk, the script folders.
- **How:** static only (`--disable-shared`, `-fPIC`), built into a temporary folder and
  renamed into place, one lock per entry. Libraries build in dependency order (Tcl before
  Tk, ncurses before readline). Libraries that need each other are built together and their
  archives merged, so each `-l` stands alone: readline with ncurses, Tk with Tcl, libnsl with
  libtirpc, and `libgdbm_compat` with `libgdbm` (measured in M8b: CPython's extension modules
  don't get `LIBS` on their link line). `Libs.private` is folded into `Libs` in the store's
  `.pc` files. A symbolic link in a library's source archive that points outside the source
  tree is skipped and logged (libtirpc 1.3.8 ships one); python-build's own archives still
  refuse such links.
- **Only missing libraries are built.** Anything the system provides is used as it is,
  so it keeps getting the distribution's security updates.
- **It is a cache.** CPython links the libraries statically, so deleting `cache/deps`
  breaks no installed version; the next build compiles again.
- **Reuse:** another CPython version that needs the same library, version and options
  uses the same entry.

### 6.3 Building CPython against the store

`CPPFLAGS`, `LDFLAGS` and `PKG_CONFIG_PATH` point at the store entries used (the X11
entries are the exception: §6.4),
`--with-openssl=<entry>` for OpenSSL, and the Tcl/Tk options for the version's
`configure`. Each version records the libraries it was built with, and their versions,
in `versions/<v>/.rpyenv-deps`. Linux has no `pyenv update` (it is pyenv-win's command),
so the check runs during `pyenv install`: when an rpyenv upgrade brings a catalog with a
newer pin for a library an installed version was built with, `pyenv install` (of any
version) prints one warning line per affected version, naming the library and both
versions. `pyenv install -f --deps=build <v>` rebuilds, reusing the store.

**Tcl/Tk:** 8.6 (D11). The script folders `tcl8.6/` and `tk8.6/` are copied into
`versions/<v>/lib/`, where Tcl looks relative to the executable, and into a shared
`<root>/.rpyenv/tcltk/<release>/` (`tcl-8.6.x/tcl8.6`, `tk-8.6.x/tk8.6`), which Tcl and Tk have
compiled in so that virtual environments find them (a venv's executable is not beside the
version's copy). The shared folder is outside `cache/deps`, so deleting the cache breaks no
venv; when it is deleted, the version itself falls back to its own copy (measured in M8b),
`pyenv install` and `pyenv rehash` re-create it from an installed version's copy, and
`pyenv uninstall` removes it once no installed version's record names that release. §4.4's
checks confirm both. (User decision 2026-10-17, after the final review found venvs broken
with a compiled-in path that never exists.)

### 6.4 X11 (M8c)

Only when the user can't install packages (D9): xorgproto, libXau, xcb-proto, libxcb, xtrans and
libX11 join the catalog, all static (libXext is not needed: measured in M8c, Tk 8.6 links only
libX11). libX11 absorbs libxcb and libXau, and reads its locale data from `/usr/share/X11/locale`.
xcb-proto and libxcb need Python (D14). Tk is then built without Xft (D10), which saves
fontconfig, freetype and libXft. With system X11, Tk is built as the distribution's Tk is.

The X11 entries reach CPython only through Tk. The store's libX11 and xorgproto include folders
go into Tk's compiler flags (`TCLTK_CFLAGS`, `--with-tcltk-includes`), because `tk.h` includes
`<X11/Xlib.h>`; libX11 links only through Tk's `TK_LIBS`. They get no `CPPFLAGS`, `LDFLAGS` or
`pkg-config` folder. A definition that fetches CPython with git has no tarball for D14's Python,
so it keeps the refusal for `tkinter` and needs the X11 headers.

### 6.5 CA certificates for a self-built OpenSSL

OpenSSL is configured with a private `OPENSSLDIR`, `<root>/.rpyenv/ssl`, outside the cache,
so deleting `cache/deps` never breaks HTTPS (user decision 2026-10-17). The folder holds
no `openssl.cnf`: a distribution's config is written for its own OpenSSL and a vanilla one
can fail on it (measured on Tumbleweed 2026-10-18: its vendor `openssl.cnf` made `ssl` raise
`MODULE_INITIALIZATION_ERROR`). Its `cert.pem` and `certs` link to the system's
certificates, so the distribution's updates still apply: `cert.pem` to the first existing of
`/etc/ssl/certs/ca-certificates.crt`, `/etc/pki/tls/certs/ca-bundle.crt`,
`/etc/ssl/ca-bundle.pem`, `/etc/ssl/cert.pem`, and only last `<openssl version -d>/cert.pem` (an
`openssl` first on `PATH` can be conda's or Linuxbrew's, not the system's); `certs` to
`/etc/ssl/certs` or `/etc/pki/tls/certs`, whichever is not empty, else
`<openssl version -d>/certs`. A link names the path as found, not its resolved target. When the system has no
certificates and the user has no root, `cert.pem` is a regular file with a pinned Mozilla
bundle (curl.se's `cacert.pem`, SHA-256 checked). The links are made again (under a
temporary name, then renamed) whenever the store uses OpenSSL and at the start of every
`pyenv install` that finds the folder, so a system that gains certificates turns `cert.pem`
into a link and one that loses them turns it back into the pinned file. The pin ships with
rpyenv; when an rpyenv upgrade brings a newer one, the next `pyenv install` replaces the
file (only a regular file with another hash), and every version built against it uses the
new bundle at once (the path, not the content, is compiled into OpenSSL). The build route needs CPython 3.7 or later, a `PYENV_ROOT`
without spaces, Perl's core modules for OpenSSL and libxcrypt.

## 7. The prebuilt route (M8d)

Downloads a python-build-standalone build of the requested version instead of compiling.

- **Checks:** the SHA-256 the release publishes and its GitHub attestation, both by the weekly
  sync before a row enters rpyenv's table (D15); `pyenv install` checks the SHA-256 against the
  table.
- **The same check after install as §4.4**, without `dbm.gnu` and `nis` (D17), and with Tcl
  started in a virtual environment as well.
- **It is never the default** (D8), and the prompt says it is a third-party build, its date and
  its bundled OpenSSL version (D19).
- **Tk 9:** builds from 2026 ship Tk 9.0; the route says so, because some older Tk programs
  behave differently under Tk 9. The table records each build's Tk. Method: the table's tk
  column, counted by release year (`grep -v '^#' linux.tsv | cut -f3,6 | cut -c1-4,9- | sort
  | uniq -c`, 2026-10-21): 80 of 80 rows from 2026 have 9.0, 50 of 50 rows from 2025 have 8.6.
- **Coverage:** rpyenv's built-in CPython definitions that an attested release built for this
  machine's architecture (x86_64 or aarch64, glibc), free-threaded (`t`) ones included (D16);
  for others, `s` is left out.
- **glibc** (final review A2): the builds are linked against glibc, while rpyenv's own Linux
  binary is static musl and also runs where glibc is missing (Alpine, NixOS without nix-ld).
  The route needs the glibc loader for the arch (`/lib64/ld-linux-x86-64.so.2` or
  `/lib/ld-linux-aarch64.so.1`); without it, `--deps=prebuilt` refuses with `pyenv: no
  prebuilt builds for this system: python-build-standalone's builds need glibc (<loader> not
  found)` and the prompt leaves `s` out.
- **Compiler settings** are rewritten for the system's compiler (D18).
- **`--deps=prebuilt`** always installs the prebuilt build (it doesn't depend on what is
  missing), and fails when the table has none for this version. `-g` and `-p` can't be used
  with it. Ruling R1: `--deps=prebuilt` and `RPYENV_BUILD_DEPS=prebuilt` act only for
  definitions that build CPython 3 from source; PyPy, conda and CPython 2.x ignore them, as
  they ignore the other routes, so "fails when the table has none" applies only to those.
- **The record:** `versions/<v>/.rpyenv-deps` names the release, the arch and the bundled
  OpenSSL (`prebuilt <release> <arch> <openssl>`; records without the OpenSSL word are still
  read); when rpyenv's table later offers a newer build of that version, `pyenv install` says
  so, as §6.3 does for libraries.
- **The newest patch** (final review B1, user ruling "extend", 2026-10-21):
  python-build-standalone rebuilds only the newest patch of each minor, so an installed older
  patch never gets the newer-build line. When the version's own row has no newer release but
  the table has a later patch of the same `X.Y` and variant (`t` or not) for the recorded
  arch, `pyenv install` prints instead `pyenv: <name> is the python-build-standalone build of
  <date> (OpenSSL <x>); rpyenv offers <newer>, built <date> (OpenSSL <y>): pyenv install
  --deps=prebuilt <newer>`. `<newer>` is the highest such version, compared numerically; final
  releases only, unless the installed version is itself a prerelease. Table only, no network;
  one line per version (the newer-build line wins).

**Measured in M8d, 2026-10-21.**

- **The table:** 130 rows, 65 for x86_64 and 65 for aarch64, from 23 releases between
  20250317 and 20261020. Method: counting the lines of `crates/pyenv/prebuilt/linux.tsv`
  that do not start with `#`, and their second and third columns (`sort | uniq -c`). The
  `ci/prebuilt_table.py` wrote it; on the refresh after the rebase onto pyenv 2.8.10 it added
  the four rows of 3.16.0a1 and 3.16.0a1t (release 20261020, OpenSSL 3.5.9, Tk 9.0).
- **The check script:** `ci/check_prebuilt.sh` ran locally (WSL, with podman for the must-fail
  case) and exited 0 in 21 to 23 s, the must-fail run included. That run installs a real
  prebuilt 3.13 and checks it; the must-fail run installs in an image without
  `ca-certificates` and must (and did) fail with nothing left in `versions/`.
- **Not measured yet:** the time of the CI job (`install-linux.yml`) on GitHub's runners, on
  x86_64 and aarch64. The first run of the pull request will give it; this section does not
  guess.

**Measured, 2026-10-17:** release `20261013`, CPython 3.13.16 for Linux x86_64
(`install_only` tarball; SHA-256 matched the published digest). Method: `readelf -d`
on the binaries, which reads ELF headers and runs nothing; a dependency on the system's
X11 would show as `libX11.so.6`.

| File | System libraries it needs |
|---|---|
| `_tkinter` module | libc, libm, libdl, libpthread, plus `libtcl9.0.so` and `libtcl9tk9.0.so` from the tarball |
| `libtcl9tk9.0.so` | libc, libm, libdl, libpthread; no libX11, libxcb, libXft or fontconfig |
| `python3.13` | libc, libm, libdl, libpthread, librt, libutil |

So, for this one build, `import tkinter` needs no X11 packages; showing a window still needs a
display. That the X11 client code is linked into the Tk library is an inference (it contains
the string `XOpenDisplay` but imports no such symbol). Indirectly, the must-fail run of
`ci/check_prebuilt.sh` (3.13.16 x86_64, release 20261020, in `ubuntu:24.04`, which has no
libX11 or libxcb files and no X11 packages: checked with `ls` and `dpkg -l` in that image,
2026-10-21) failed only for "ssl certificates", so `tkinter` and Tcl worked there. Other builds
were not examined; the check after install runs `import tkinter` and `tkinter.Tcl()` on every
install, so a build that needs X11 fails visibly.

**Method notes (2026-10-21).** D16's facts were measured with the GitHub API (attestation
lookups per release) and in Ubuntu 24.04 and Fedora 44 containers (OpenSSL via
`ssl.OPENSSL_VERSION`; 3.10.4 of 20220528 not starting on Fedora 44: the loader's error for
`libcrypt.so.1`). D19's static linking was established in containers by
`sys.builtin_module_names` containing `_ssl` and `_hashlib`, no `_ssl` or libssl file in
`lib-dynload` or `lib/`, and the OpenSSL init symbol string in libpython.

## 8. Options considered for reusing compiled libraries (D7)

| Option | Main reason not chosen |
|---|---|
| A. Store and hardlinks of shared libraries | Needs link-count bookkeeping; versions depend on runtime library paths |
| B. Versions link to a shared prefix | Versions stop being self-contained; one bad library update breaks all of them |
| C. Cached build copied into each version | 30-40 MB more per version |
| **D. Static linking from a store (chosen)** | Most self-contained; store prunable at any time; about 10-15 MB more per version |
| E. Prebuilt CPython | Not "compiled from source, as pyenv does"; kept as an opt-in route (D8) |
| F. A user-level package manager (micromamba, Nix) | Installs a large extra tool; another set of pins |

Common to A, C and D: a library rpyenv compiled needs a rebuild of the version for a
security fix. §6.3's record and warning cover it.

## 9. Testing

**Unit tests** (every PR, no network): reading `/etc/os-release` (fixtures per
distribution); the install command per package manager; the prompt's choices (`i` left
out without `sudo`; the default without a terminal); the module list per version; the
catalog, store keys and locking; reading version macros.

**CI in containers** (packages are only installed inside throwaway containers, never on
a development machine):

| Test | Containers | When |
|---|---|---|
| Detection and `system`: only a compiler installed; running the printed command makes the next detection come back empty | Debian slim, Ubuntu, Fedora, Rocky 9, openSUSE, Arch | every PR |
| `install` as root: `--deps=install`, full build, §4.4 check | Ubuntu, Fedora | every PR |
| `build` (M8b): only a compiler, no `-dev` packages; §4.4 check | Ubuntu, Fedora | every PR |
| X11 build (M8c): no X11 packages; `_tkinter` and `tkinter.Tcl()` work | Ubuntu | every PR |
| `prebuilt` (M8d): a real install checked against the table's SHA-256, §4.4 check, a C extension built with the runner's compiler; on x86_64 also a must-fail install without CA certificates (in `ubuntu:24.04`). The attestation is checked by the weekly sync before a row enters the table (D15), not here | Ubuntu runners, x86_64 and aarch64 | every PR |
| `install` and `build` on all six containers | all six | weekly |

**The check must be able to fail:** for each route, a deliberately incomplete build (one
library held back) must fail §4.4 before a passing run is trusted.

## 10. Not decided here

- Alpine/musl (a later milestone, D3).
- Whether to share the store between users (M9's machine-wide root, if it happens).
