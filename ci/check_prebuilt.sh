#!/usr/bin/env bash
# M8d: a real `RPYENV_BUILD_DEPS=prebuilt pyenv install` of a python-build-standalone build,
# run on the runner itself (x86_64 or aarch64), then three checks of the result.
#   ci/check_prebuilt.sh <directory holding the pyenv binary>
# Environment:
#   PREBUILT_MUSTFAIL=1   also run the must-fail case in a container (needs docker or podman)
#   CONTAINER_ENGINE      the engine to use; default: docker, else `podman --cgroup-manager=cgroupfs`
#   PREBUILT_IMAGE        the container image; default ubuntu:24.04 (it has no ca-certificates)
#
# The must-fail case (spec §9: a check that can't fail proves nothing). Measured 2026-10-21 in
# ubuntu:24.04 and ubuntu:26.04 under podman, real network: rpyenv's own download works in an
# image with no CA certificates (it falls back to the pinned bundle, allowlist D-128), so the
# build is fetched, unpacked, and then refused by the check after install with
# "doesn't work here without: ssl certificates", exit 1 and nothing left in versions/. So the
# case needs no pre-filled cache: the only difference from the passing install is the missing
# ca-certificates package, and the install itself must fail on that.
set -euo pipefail

bin="${1:?usage: check_prebuilt.sh <directory holding the pyenv binary>}"
bin=$(cd "$bin" && pwd)
repo=$(cd "$(dirname "$0")/.." && pwd)
table="$repo/crates/pyenv/prebuilt/linux.tsv"
arch=$(uname -m)

# 1. The newest 3.13.* (not a free-threaded "t" build) with a row for this architecture.
v=$(awk -F'\t' -v a="$arch" '$1 !~ /^#/ && $2 == a && $1 ~ /^3\.13\.[0-9]+$/ { print $1 }' "$table" | sort -V | tail -1)
[ -n "$v" ] || { echo "::error::no 3.13.* row for $arch in $table"; exit 1; }
release=$(awk -F'\t' -v a="$arch" -v v="$v" '$1 == v && $2 == a { print $3 }' "$table")
echo "== prebuilt $v ($arch, release $release)"

# 2. The install.
root=$(mktemp -d)
work=$(mktemp -d)
trap 'rm -rf "$root" "$work"' EXIT
# Nothing of the developer's home or pyenv settings may steer or be touched by this run
# (pip's cache and config live under HOME).
# The container engine keeps its images under the real HOME (rootless podman can't delete its
# storage as a plain `rm`), so it runs with the HOME the script started with.
real_home="$HOME"
mkdir -p "$work/home" "$work/pyenv-home"
for name in $(compgen -e | grep '^PYENV_' || true); do unset "$name"; done
export HOME="$work/home" PYENV_HOME="$work/pyenv-home" PYENV_ROOT="$root"
out="$work/install.log"
start=$(date +%s)
RPYENV_BUILD_DEPS=prebuilt "$bin/pyenv" install "$v" 2>&1 | tee "$out"
echo "== installed in $(( $(date +%s) - start )) s"
grep -q "third-party build" "$out" || { echo "::error::no 'third-party build' line in the install output"; exit 1; }
p="$root/versions/$v"
py="$p/bin/python"

# 3. The result.
"$py" -c "import ssl, sqlite3, ctypes, lzma, bz2, readline, curses, uuid, tkinter; tkinter.Tcl(); print('modules ok')"
cc=$("$py" -c "import sysconfig; print(sysconfig.get_config_var('CC'))")
case "$cc" in
  cc*) echo "CC: $cc" ;;
  *) echo "::error::sysconfig CC is '$cc', want one that starts with cc"; exit 1 ;;
esac
# The build prefix is /install as a whole path: "/usr/bin/install", "./install", "/opt/install"
# and "$(srcdir)/install-sh" don't count. Fails when no file matches or the file lacks "prefix"
# (so the grep below has something to read), and when a leftover is found.
no_build_prefix() { # <file>...
  [ "$#" -gt 0 ] || { echo "::error::no lib/python3.*/_sysconfigdata_*.py in the install"; return 1; }
  local f rc
  for f in "$@"; do
    [ -f "$f" ] && grep -q '"prefix"' "$f" || { echo "::error::$f is not a sysconfig data file ("prefix" missing)"; return 1; }
    rc=0
    grep -nE "(^|[^A-Za-z0-9_.]|-[IL])/install([/\"' ),]|$)" "$f" || rc=$?
    case "$rc" in
      0) echo "::error::$f still names /install"; return 1 ;;
      1) ;;
      *) echo "::error::grep failed on $f (status $rc)"; return 1 ;;
    esac
  done
}
shopt -s nullglob
sysconfig_files=("$p"/lib/python3.*/_sysconfigdata_*.py)
shopt -u nullglob
no_build_prefix "${sysconfig_files[@]}"
want="prebuilt $release $arch"
# The record has comment and "definition" lines too; the route's own line is the last one.
grep -qxF "$want" "$p/.rpyenv-deps" || { echo "::error::.rpyenv-deps has no line '$want':"; cat "$p/.rpyenv-deps"; exit 1; }
echo ".rpyenv-deps has: $want"

# 4. A C extension built with the runner's compiler (the build's CC was clang before the rewrite).
ext="$work/ext"
mkdir -p "$ext"
cat > "$ext/hello.c" <<'EOF'
#include <Python.h>
static struct PyModuleDef m = { PyModuleDef_HEAD_INIT, "hello", NULL, -1, NULL };
PyMODINIT_FUNC PyInit_hello(void) { return PyModule_Create(&m); }
EOF
cat > "$ext/setup.py" <<'EOF'
from setuptools import Extension, setup
setup(name="hello", version="0", ext_modules=[Extension("hello", ["hello.c"])])
EOF
"$py" -m venv "$work/venv"
"$work/venv/bin/python" -m pip install -q setuptools
(cd "$ext" && "$work/venv/bin/python" -m pip install -q --no-build-isolation .)
(cd "$work" && "$work/venv/bin/python" -c "import hello; print('extension ok', hello.__file__)")

# 5. The must-fail case, in a container without ca-certificates.
if [ "${PREBUILT_MUSTFAIL:-}" = 1 ]; then
  engine="${CONTAINER_ENGINE:-}"
  if [ -z "$engine" ]; then
    if command -v docker >/dev/null 2>&1; then engine=docker
    elif command -v podman >/dev/null 2>&1; then engine="podman --cgroup-manager=cgroupfs"
    else echo "::error::PREBUILT_MUSTFAIL=1 needs docker or podman"; exit 1; fi
  fi
  image="${PREBUILT_IMAGE:-ubuntu:24.04}"
  echo "== must fail: $image without ca-certificates ($engine)"
  rc=0
  # shellcheck disable=SC2086
  HOME="$real_home" $engine run --rm -e "V=$v" -v "$bin:/in:ro" "$image" bash -c '
    set -u
    if [ -e /etc/ssl/certs/ca-certificates.crt ]; then echo "image has CA certificates"; exit 90; fi
    mkdir -p /r/root && cd /r
    PYENV_ROOT=/r/root RPYENV_BUILD_DEPS=prebuilt /in/pyenv install "$V"
    rc=$?
    echo "listing: [$(ls -A /r/root/versions 2>/dev/null)]"
    exit $rc' > "$work/mustfail.log" 2>&1 || rc=$?
  cat "$work/mustfail.log"
  [ "$rc" = 1 ] || { echo "::error::the install without CA certificates exited $rc, want 1"; exit 1; }
  grep -q "ssl certificates" "$work/mustfail.log" || { echo "::error::no 'ssl certificates' in the failed install's output"; exit 1; }
  grep -q '^listing: \[\]$' "$work/mustfail.log" || { echo "::error::versions/ was not empty after the failed install"; exit 1; }
  echo "== must fail: ok"
fi
echo "== prebuilt $v: ok"
