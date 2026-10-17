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
set -euo pipefail
distro="$1"
bin="$PWD/pyenv"
[ -x "$bin" ] && [ -x "$PWD/pyenv-shim" ] || { echo "::error::$distro: no ./pyenv and ./pyenv-shim to test"; exit 1; }

# Stage 0: the container gets only a compiler and make (and patch, pkg-config), as root, with
# its own manager. Debian-family images then lose their package lists again, as the official
# images ship, so the printed command must start with `apt-get update` (design §5.3).
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
