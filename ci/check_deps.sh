#!/usr/bin/env bash
# M8a CI (M8 design §9): in a container with only a compiler and make, `pyenv install`
# without a terminal prints a command; running it makes detection come back empty; then
# --deps=install builds a real CPython whose every module imports.
#   ci/check_deps.sh <distro> [<version>]       run as root, with static ./pyenv and ./pyenv-shim
# <distro>: debian, ubuntu, fedora, rocky, tumbleweed or arch.
# DETECT_ONLY=1 stops after detection comes back empty (no build).
# DISCRIMINATE=1 (Ubuntu) first removes uuid-dev and requires `--deps=system` to name uuid.
set -euo pipefail
distro="$1"
v="${2:-3.12.14}"
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

# The install command as `pyenv install` prints it, plus the manager's yes flag.
with_yes() {
  sed -E 's/apt-get install/apt-get install -y/; s/dnf install/dnf install -y/;
          s/zypper install/zypper --non-interactive install/; s/pacman -S /pacman -S --noconfirm /' <<<"$1"
}

# True when strict detection finds nothing missing: `--deps=system` then goes on to download,
# so the run is cut off after 20 seconds (a fresh root: the cut-off leaves a staging folder).
detect_empty() {
  local out
  out=$(PYENV_ROOT="$(mktemp -d)" timeout 20 "$bin" install --deps=system "$v" 2>&1 </dev/null) || true
  head -20 <<<"$out"
  ! grep -q 'missing build dependencies' <<<"$out"
}

root="$(mktemp -d)/root"
mkdir -p "$root"
export PYENV_ROOT="$root"

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

# Stage 2: run the printed command as printed (plus -y).
echo "== running: $(with_yes "$cmd")"
if ! eval "$(with_yes "$cmd")"; then
  if [ "$distro" != rocky ]; then
    echo "::error::$distro: the printed command failed"; exit 1
  fi
  # Rocky 9: -devel packages live in CRB. `--deps=install` fails the same way and prints the
  # hint once; run its command (dnf-plugins-core provides config-manager), then try again.
  hint_out=$(PYENV_ROOT="$(mktemp -d)" "$bin" install --deps=install "$v" 2>&1 </dev/null) && {
    echo "::error::rocky: --deps=install worked where the printed command failed"; exit 1; }
  echo "$hint_out"
  hint=$(sed -n 's/^pyenv: some packages live in the CRB repository: //p;T;q' <<<"$hint_out")
  [ -n "$hint" ] || { echo "::error::rocky: no CRB hint after a failed install"; exit 1; }
  echo "== CRB hint seen: $hint"
  dnf install -y dnf-plugins-core
  eval "${hint#sudo }"
  eval "$(with_yes "$cmd")"
else
  [ "$distro" != rocky ] || echo "::warning::rocky: the printed command worked without CRB"
fi

# Stage 3: detection must now be empty.
detect_empty || { echo "::error::$distro: still missing after the printed command"; exit 1; }
echo "== detection is empty on $distro"
[ -z "${DETECT_ONLY:-}" ] || exit 0

# Stage 4 (Ubuntu): the check can fail. Without uuid-dev, `--deps=system` fails naming uuid.
if [ -n "${DISCRIMINATE:-}" ]; then
  apt-get remove -y uuid-dev
  rc=0
  out=$(PYENV_ROOT="$(mktemp -d)" timeout 20 "$bin" install --deps=system "$v" 2>&1 </dev/null) || rc=$?
  echo "$out"
  { [ "$rc" -ne 0 ] && grep -q '^  uuid' <<<"$out"; } ||
    { echo "::error::$distro: removing uuid-dev did not make --deps=system fail naming uuid"; exit 1; }
  echo "== discriminating run: failed naming uuid, as it must"
  # Removing uuid-dev took tk-dev with it (a dependency): the printed command puts both back.
  apt-get install -y uuid-dev
  eval "$(with_yes "$cmd")"
  detect_empty || { echo "::error::$distro: still missing after reinstalling uuid-dev"; exit 1; }
fi

# Stage 5: the real build. It checks every module itself (strict mode fails on a miss); the
# imports below are an independent look from outside.
"$bin" install --deps=install "$v"
mods="ssl, tkinter, sqlite3, ctypes, lzma, bz2, zlib, readline, curses, curses.panel, dbm.gnu, dbm.ndbm, uuid, _uuid, _hashlib"
minor=$(echo "$v" | sed -E 's/^[0-9]+\.([0-9]+).*/\1/')
if [ "$minor" -ge 14 ]; then mods="$mods, compression.zstd"; fi
if [ "$minor" -le 12 ]; then mods="$mods, crypt"; fi
"$root/versions/$v/bin/python3" -c "import $mods; print('every module imports')"
