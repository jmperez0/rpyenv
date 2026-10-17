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
| D13 | Scripts that set nothing (user, after a side-agent note) | **Phased.** Until M8b ships, a run with no terminal and neither `--deps` nor `RPYENV_BUILD_DEPS` keeps today's rule: the compiler, `make`, OpenSSL and zlib stop the build; every other missing library only warns, and the build goes on. The run ends with a summary naming what is missing, the command that adds it, and the notice that a later release makes this stop the build. **M8b** makes the script default strict (`system`), because from then on a script without root can answer with `--deps=build`. Chosen over strict everywhere (breaks today's Dockerfiles on upgrade, while a script without root could only fail), lenient for good (scripts never get every module), a policy per pyenv root (Dockerfiles make fresh roots), automatic installs as root (installs packages nobody approved) and splitting libraries by weight (still breaks on the newly checked gdbm, uuid and crypt). |

## 3. Parts

M8 ships in four parts. Each has its own plan and pull request and is useful on its own.

| Part | Delivers | Needs |
|---|---|---|
| **M8a** detect and install | The complete detection (§4); the prompt and the `system` and `install` routes (§5); the check after the build (§4.4) | M2's pre-flight check |
| **M8b** build from source | The catalog, the store and static builds of every library except X11 (§6) | M8a |
| **M8c** X11 from source | The X11 client libraries in the catalog, for the no-root case (§6.4) | M8b |
| **M8d** prebuilt CPython | The `prebuilt` route (§7) | M8a |
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
- `import tkinter` and `tkinter.Tcl()` work without a display, and Tcl's `info library`
  lies inside the version (when the build route supplied Tcl/Tk).

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
```

- **`p`** prints the commands and exits as today. Enter chooses it.
- **`i`** shows the exact command, runs it through `sudo` (which asks for the password
  itself), then runs detection again. Anything still missing gets the prompt again,
  without `i`. rpyenv never adds `-y` to a command it has not shown.
- **`i` is left out**, with the reason stated, when there is no `sudo`, when
  `sudo -n true` reports the user may not use it, or when the distribution is not
  recognized. Running as root (most containers), `i` runs the command without `sudo`.
- **`b`** appears from M8b on, **`s`** from M8d on.

### 5.2 Without a terminal

`pyenv install --deps=system|install|build|prebuilt` or `RPYENV_BUILD_DEPS` with the
same values; then every module is required, as at a terminal. `install` uses `sudo -n`
and fails with a clear message when `sudo` needs a password. With neither set, D13's
phased rule applies: until M8b, only the compiler, `make`, OpenSSL and zlib stop the
build, and the run ends with this summary when anything else is missing:

```
pyenv: 3.12.14 was built without: tkinter, uuid
  Add them: sudo apt-get install tk-dev uuid-dev, then pyenv install -f 3.12.14
  In a later rpyenv release this stops the build; choose now with --deps=install or
  --deps=system (or RPYENV_BUILD_DEPS).
```

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
  Tk, ncurses before readline).
- **Only missing libraries are built.** Anything the system provides is used as it is,
  so it keeps getting the distribution's security updates.
- **It is a cache.** CPython links the libraries statically, so deleting `cache/deps`
  breaks no installed version; the next build compiles again.
- **Reuse:** another CPython version that needs the same library, version and options
  uses the same entry.

### 6.3 Building CPython against the store

`CPPFLAGS`, `LDFLAGS` and `PKG_CONFIG_PATH` point at the store entries used,
`--with-openssl=<entry>` for OpenSSL, and the Tcl/Tk options for the version's
`configure`. Each version records the libraries it was built with, and their versions,
in `versions/<v>/.rpyenv-deps`. Linux has no `pyenv update` (it is pyenv-win's command),
so the check runs during `pyenv install`: when an rpyenv upgrade brings a catalog with a
newer pin for a library an installed version was built with, `pyenv install` (of any
version) prints one warning line per affected version, naming the library and both
versions. `pyenv install -f <v>` rebuilds, reusing the store.

**Tcl/Tk:** 8.6 (D11). The script folders `tcl8.6/` and `tk8.6/` are copied into
`versions/<v>/lib/`, where Tcl looks relative to the executable. §4.4's `info library`
check confirms it.

### 6.4 X11 (M8c)

Only when the user can't install packages (D9): xorgproto, libXau, libxcb, xtrans,
libX11 and libXext join the catalog, all static. Tk is then built without Xft (D10),
which saves fontconfig, freetype and libXft. With system X11, Tk is built as the
distribution's Tk is.

### 6.5 CA certificates for a self-built OpenSSL

OpenSSL is configured to look where the system keeps certificates (`/etc/ssl` or
`/etc/pki/tls`), so the distribution's certificate updates apply. When the system has no
certificates and the user has no root: a pinned Mozilla bundle (curl.se's `cacert.pem`,
SHA-256 checked) in `<root>/cache/deps/ca/`. The pin ships with rpyenv; when an rpyenv
upgrade brings a newer one, the next `pyenv install` replaces the file, and every
version built against it uses the new bundle at once (the path, not the content, is
compiled into OpenSSL).

## 7. The prebuilt route (M8d)

Downloads a python-build-standalone build of the requested version instead of compiling.

- **Checks:** the SHA-256 the release publishes and its GitHub attestation.
- **The same check after install as §4.4.**
- **It is never the default** (D8), and the prompt says it is a third-party build.
- **Tk 9:** these builds ship Tk 9.0 (measured below); the route says so, because some
  older Tk programs behave differently under Tk 9.
- **Coverage:** only versions the project publishes; for others, `s` is left out.

**Measured, 2026-10-17:** release `20261013`, CPython 3.13.16 for Linux x86_64
(`install_only` tarball; SHA-256 matched the published digest). Method: `readelf -d`
on the binaries, which reads ELF headers and runs nothing; a dependency on the system's
X11 would show as `libX11.so.6`.

| File | System libraries it needs |
|---|---|
| `_tkinter` module | libc, libm, libdl, libpthread, plus `libtcl9.0.so` and `libtcl9tk9.0.so` from the tarball |
| `libtcl9tk9.0.so` | libc, libm, libdl, libpthread; no libX11, libxcb, libXft or fontconfig |
| `python3.13` | libc, libm, libdl, libpthread, librt, libutil |

So `import tkinter` needs no X11 packages; showing a window still needs a display. That
the X11 client code is linked into the Tk library is an inference (it contains the
string `XOpenDisplay` but imports no such symbol).

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
| `prebuilt` (M8d): hash and attestation, §4.4 check | Ubuntu | every PR |
| `install` and `build` on all six containers | all six | weekly |

**The check must be able to fail:** for each route, a deliberately incomplete build (one
library held back) must fail §4.4 before a passing run is trusted.

## 10. Not decided here

- Alpine/musl (a later milestone, D3).
- Whether to share the store between users (M9's machine-wide root, if it happens).
