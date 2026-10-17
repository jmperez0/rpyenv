#!/usr/bin/env bash
# M8a CI (M8 design §9): in a container with only a compiler and make, `pyenv install`
# without a terminal prints a command. Detection-only legs run that command and require the
# next detection to come back empty. Full legs instead let `pyenv install --deps=install` run
# the package manager itself, then build a real CPython whose every module imports.
#   ci/check_deps.sh <distro> [<version>]       run as root, with static ./pyenv and ./pyenv-shim
# <distro>: debian, ubuntu, fedora, rocky, tumbleweed or arch. <version>: a full version, or
# X.Y for the newest X.Y.Z (default 3.12.14).
# DETECT_ONLY=1 stops after detection comes back empty (no build).
# DISCRIMINATE=1 (Ubuntu) removes uuid-dev after the build and requires `--deps=system` to
# name uuid.
# WEEKLY=1 (Ubuntu, full) also builds the newest 3.14, and builds once more without uuid-dev
# under RPYENV_SKIP_PREFLIGHT=1, where the check after the build must warn naming uuid.
# BUILD=1 (M8b, M8 design §9 row 3): the container gets a compiler, make, patch, Perl, the X11
# headers and CA certificates, and no -dev package of any library and no pkg-config.
# `pyenv install --deps=build` compiles every missing library into the store, builds a CPython
# whose every module imports, and records them; a second build reuses the store; the version
# keeps working without the store; and (Ubuntu) a store entry with its library removed makes
# the check after the build fail naming the module. WEEKLY=1 adds the newest 3.14 (OpenSSL 4
# and upstream's patch) and 3.8 (OpenSSL 1.1.1).
# NO_CA=1 (BUILD, Debian/Ubuntu; allowlist D-128): no CA certificates at all. rpyenv downloads
# with Mozilla's roots compiled in, builds OpenSSL against its pinned bundle, and the built
# Python must complete a TLS handshake through that bundle.
set -euo pipefail
distro="$1"
bin="$PWD/pyenv"
[ -x "$bin" ] && [ -x "$PWD/pyenv-shim" ] || { echo "::error::$distro: no ./pyenv and ./pyenv-shim to test"; exit 1; }

# Stage 0: the container gets only a compiler and make (and patch, pkg-config), as root, with
# its own manager. Debian-family images then lose their package lists again, as the official
# images ship, so the printed command must start with `apt-get update` (design §5.3).
if [ -n "${BUILD:-}" ]; then
  case "$distro" in
    debian|ubuntu)
      apt-get update -q
      if [ -n "${NO_CA:-}" ]; then
        # Without recommends: several packages recommend ca-certificates.
        apt-get install -yq --no-install-recommends gcc libc6-dev make patch perl libx11-dev
        if dpkg -s ca-certificates >/dev/null 2>&1 || [ -e /etc/ssl/certs/ca-certificates.crt ]; then
          echo "::error::$distro: NO_CA=1, but CA certificates are installed"; exit 1
        fi
      else
        apt-get install -yq gcc libc6-dev make patch perl libx11-dev ca-certificates
      fi
      rm -rf /var/lib/apt/lists/* ;;
    fedora|rocky) dnf install -y gcc make patch perl-core libX11-devel ;;
    tumbleweed) zypper --non-interactive install gcc make patch perl libX11-devel ca-certificates ;;
    arch) pacman -Syu --noconfirm gcc make patch perl libx11 ;;
    *) echo "::error::unknown distro $distro"; exit 1 ;;
  esac
else
  case "$distro" in
    debian|ubuntu)
      apt-get update -q
      apt-get install -yq gcc libc6-dev make patch pkg-config
      rm -rf /var/lib/apt/lists/* ;;
    fedora|rocky) dnf install -y gcc make patch pkgconf-pkg-config ;;
    tumbleweed) zypper --non-interactive install gcc make patch pkg-config ;;
    arch) pacman -Syu --noconfirm gcc make patch pkgconf ;;
    *) echo "::error::unknown distro $distro"; exit 1 ;;
  esac
fi

root="$(mktemp -d)/root"
mkdir -p "$root"
export PYENV_ROOT="$root"

# X.Y means the newest X.Y.Z rpyenv knows.
newest() { "$bin" latest -k "$1"; }
v="${2:-3.12.14}"
case "$v" in *.*.*) ;; *) v=$(newest "$v") ;; esac
echo "== $distro: Python $v"

# The install command as `pyenv install` prints it, plus the manager's yes flag.
with_yes() {
  sed -E 's/apt-get install/apt-get install -y/; s/dnf install/dnf install -y/;
          s/zypper install/zypper --non-interactive install/; s/pacman -S /pacman -S --noconfirm /' <<<"$1"
}

# True when strict detection finds nothing missing for version $1: `--deps=system` then goes
# on to download, so the run is cut off after 20 seconds (a fresh root: the cut-off leaves a
# staging folder).
detect_empty() {
  local out
  out=$(PYENV_ROOT="$(mktemp -d)" timeout 20 "$bin" install --deps=system "${1:-$v}" 2>&1 </dev/null) || true
  head -20 <<<"$out"
  ! grep -q 'missing build dependencies' <<<"$out"
}

# The check from outside the build: every module of version $1 imports.
imports() {
  local mods minor
  mods="ssl, tkinter, sqlite3, ctypes, lzma, bz2, zlib, readline, curses, curses.panel, dbm.gnu, dbm.ndbm, uuid, _uuid, _hashlib"
  minor=$(echo "$1" | sed -E 's/^[0-9]+\.([0-9]+).*/\1/')
  if [ "$minor" -ge 14 ]; then mods="$mods, compression.zstd"; fi
  if [ "$minor" -le 12 ]; then mods="$mods, crypt"; fi
  "$root/versions/$1/bin/python3" -c "import $mods; print('every module imports')"
}

if [ -n "${BUILD:-}" ]; then
  # Stage B1: the build route, without a terminal.
  "$bin" install --deps=build "$v" </dev/null 2>&1 | tee /tmp/build1.log
  [ "${PIPESTATUS[0]}" -eq 0 ] || { echo "::error::$distro: --deps=build failed"; exit 1; }
  imports "$v"
  rec="$root/versions/$v/.rpyenv-deps"
  [ -f "$rec" ] || { echo "::error::$distro: no $rec"; exit 1; }
  cat "$rec"
  # No libssl-dev was installed: OpenSSL must have come from the store.
  grep -q '^openssl ' "$rec" || { echo "::error::$distro: OpenSSL is not in the record"; exit 1; }
  # NO_CA (§6.5, D-128): the downloads worked without system certificates; the store's OpenSSL
  # reads rpyenv's pinned bundle, and a real TLS handshake goes through it.
  if [ -n "${NO_CA:-}" ]; then
    bundle="$root/.rpyenv/ca/cert.pem"
    [ -f "$bundle" ] || { echo "::error::$distro: no CA bundle at $bundle"; exit 1; }
    echo "== the CA bundle is $bundle"
    "$root/versions/$v/bin/python3" -c "import ssl; print(ssl.get_default_verify_paths())"
    "$root/versions/$v/bin/python3" -c "import urllib.request; urllib.request.urlopen('https://pypi.org/simple/', timeout=30)" ||
      { echo "::error::$distro: no TLS handshake through $bundle"; exit 1; }
    echo "== a TLS handshake to pypi.org through the bundle works"
  fi
  # Tcl finds its library inside the version, not in the store (§4.4).
  lib=$("$root/versions/$v/bin/python3" -c "import tkinter; print(tkinter.Tcl().eval('info library'))")
  case "$lib" in "$root/versions/$v/"*) ;; *) echo "::error::$distro: Tcl's library is $lib"; exit 1 ;; esac
  # The system itself is unchanged: strict detection still finds what is missing.
  if detect_empty; then echo "::error::$distro: detection is empty without any -dev package"; exit 1; fi
  # Stage B2: reuse (D6). A second build compiles no library.
  "$bin" install -f --deps=build "$v" </dev/null > /tmp/build2.log 2>&1 ||
    { tail -30 /tmp/build2.log; echo "::error::$distro: the second --deps=build failed"; exit 1; }
  if grep -q '^Building ' /tmp/build2.log; then
    grep '^Building ' /tmp/build2.log; echo "::error::$distro: a library was built again"; exit 1
  fi
  grep -q '^Using openssl ' /tmp/build2.log || { echo "::error::$distro: OpenSSL was not reused"; exit 1; }
  # Stage B3 (Review Focus 3): the store is only a cache.
  mv "$root/cache/deps" "$root/cache/deps.off"
  imports "$v"
  mv "$root/cache/deps.off" "$root/cache/deps"
  # Stage B4 (Ubuntu): the check can fail. A store entry that no longer holds its library (the
  # folder stays, so the store reuses it) makes the check after the build fail naming uuid; the
  # rollback keeps the version from stage B2.
  if [ "$distro" = ubuntu ]; then
    rm -rf "$root"/cache/deps/libuuid-*/*
    rc=0
    out=$("$bin" install -f --deps=build "$v" 2>&1 </dev/null) || rc=$?
    tail -5 <<<"$out"
    { [ "$rc" -ne 0 ] && grep -q 'uuid (import _uuid failed' <<<"$out"; } ||
      { echo "::error::$distro: a store without libuuid.a did not fail the check naming uuid"; exit 1; }
    imports "$v"
    echo "== the check after the build failed naming uuid, as it must"
    # The next build compiles libuuid again.
    rm -rf "$root"/cache/deps/libuuid-*
  fi
  if [ -n "${WEEKLY:-}" ]; then
    for w in "$(newest 3.14)" "$(newest 3.8)"; do
      echo "== weekly: Python $w through the build route"
      "$bin" install --deps=build "$w" </dev/null
      imports "$w"
    done
  fi
  echo "== the build route works on $distro"
  exit 0
fi

# Stage 1: no terminal, no flags (lenient mode, D13): the hard set (compiler, make, patch,
# OpenSSL, zlib) is missing, so it fails. Arch's base image already holds OpenSSL and zlib, so
# there a flagless run would build; `--deps=system` (strict, installs nothing) stands in.
flag=""
[ "$distro" != arch ] || flag="--deps=system"
rc=0
out=$("$bin" install $flag "$v" 2>&1 </dev/null) || rc=$?
echo "$out"
if [ "$rc" -eq 0 ]; then
  echo "::error::$distro: install succeeded with nothing installed"; exit 1
fi
grep -q '^pyenv: cannot build Python: missing build dependencies:' <<<"$out" ||
  { echo "::error::$distro: no missing-dependencies report"; exit 1; }
cmd=$(sed -n '/^Install them with:/{n;s/^  //;p;q}' <<<"$out")
[ -n "$cmd" ] || { echo "::error::$distro: no install command printed"; exit 1; }
case "$cmd" in sudo*) echo "::error::$distro: root was told to use sudo: $cmd"; exit 1 ;; esac
if [ "$distro" = debian ] || [ "$distro" = ubuntu ]; then
  case "$cmd" in "apt-get update && apt-get install"*) ;;
    *) echo "::error::$distro: empty apt lists, but the command has no update: $cmd"; exit 1 ;; esac
fi
# Rocky 9: some -devel packages live in CRB, and the report says so before the command
# (ruling R15), without sudo for root.
crb=$(sed -n 's/^Some packages live in the CRB repository; enable it first: //p' <<<"$out")
if [ "$distro" = rocky ]; then
  [ -n "$crb" ] || { echo "::error::rocky: the report does not name the CRB repository"; exit 1; }
  case "$crb" in *sudo*) echo "::error::rocky: root was told to use sudo: $crb"; exit 1 ;; esac
elif [ -n "$crb" ]; then
  echo "::error::$distro: a CRB line on a system without CRB: $crb"; exit 1
fi

if [ -n "${DETECT_ONLY:-}" ]; then
  # Stage 2: run the printed commands as printed (plus -y): CRB first where the report says so.
  if [ -n "$crb" ]; then
    echo "== running: $(with_yes "$crb")"
    eval "$(with_yes "$crb")"
  fi
  echo "== running: $(with_yes "$cmd")"
  eval "$(with_yes "$cmd")" || { echo "::error::$distro: the printed command failed"; exit 1; }
  # Stage 3: detection must now be empty.
  detect_empty || { echo "::error::$distro: still missing after the printed command"; exit 1; }
  echo "== detection is empty on $distro"
  exit 0
fi

# Stage 2 (full legs): `pyenv install --deps=install` runs the package manager itself (spec §9
# row 2), from the container as stage 1 left it, then builds. It checks every module itself
# (strict: a miss fails it); the imports below are an independent look from outside.
"$bin" install --deps=install "$v" </dev/null
detect_empty || { echo "::error::$distro: still missing after --deps=install"; exit 1; }
echo "== detection is empty on $distro"
imports "$v"

# Stage 3 (Ubuntu): the check can fail. Without uuid-dev, `--deps=system` fails naming uuid.
if [ -n "${DISCRIMINATE:-}" ]; then
  apt-get remove -y uuid-dev
  rc=0
  out=$(PYENV_ROOT="$(mktemp -d)" timeout 20 "$bin" install --deps=system "$v" 2>&1 </dev/null) || rc=$?
  echo "$out"
  { [ "$rc" -ne 0 ] && grep -q '^  uuid' <<<"$out"; } ||
    { echo "::error::$distro: removing uuid-dev did not make --deps=system fail naming uuid"; exit 1; }
  echo "== discriminating run: failed naming uuid, as it must"
  # Removing uuid-dev took tk-dev with it (a dependency): stage 1's command puts both back.
  eval "$(with_yes "$cmd")"
  detect_empty || { echo "::error::$distro: still missing after reinstalling uuid-dev"; exit 1; }
fi

[ -n "${WEEKLY:-}" ] || exit 0

# Stage 4 (weekly): the newest 3.14, so compression.zstd and the 3.13 SQLite minimum are
# probed and checked on a real build.
v14=$(newest 3.14)
echo "== weekly: Python $v14"
"$bin" install --deps=install "$v14" </dev/null
imports "$v14"

# Stage 5 (weekly): the check after the build on a real build that lacks a module (§4.4).
# Without uuid-dev and with RPYENV_SKIP_PREFLIGHT=1, detection is skipped, the build goes on
# without _uuid, and the check must warn naming uuid; the build stays (ruling P1).
apt-get remove -y uuid-dev
rc=0
out=$(RPYENV_SKIP_PREFLIGHT=1 "$bin" install -f "$v" 2>&1 </dev/null) || rc=$?
tail -20 <<<"$out"
[ "$rc" -eq 0 ] || { echo "::error::$distro: the build without uuid-dev failed (exit $rc)"; exit 1; }
grep -q "^pyenv: warning: $v was built without: .*uuid (import _uuid failed" <<<"$out" ||
  { echo "::error::$distro: no warning naming uuid after a build without uuid-dev"; exit 1; }
echo "== weekly: the check after the build named uuid"
