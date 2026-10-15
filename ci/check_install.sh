#!/usr/bin/env bash
# Tier 3 (spec §12.5): checks a real `pyenv install` result.
#   ci/check_install.sh <PYENV_ROOT> <version> [<pyenv binary>]
set -euo pipefail
root="$1"
v="$2"
PYENV_BIN="${3:-pyenv}"
p="$root/versions/$v"
conda=0
case "$v" in
  anaconda*|miniconda*|miniforge*|mambaforge*)
    # Conda (M7b design §8): python3 is the interpreter, the install is in place (no staging to
    # check), and conda's own scripts carry shebangs this check doesn't model.
    name=python3
    mods="ssl, sqlite3, ctypes, zlib, bz2, lzma"
    conda=1 ;;
  pypy*)
    # PyPy (M7a design §8): bin/python is upstream's link to bin/pypy*, the shim checked is
    # `python`, and its module set is smaller.
    name=python
    mods="ssl, sqlite3, ctypes, zlib, bz2" ;;
  *)
    xy=$(echo "$v" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')
    major=${xy%%.*}
    minor=${xy#*.}
    # A free-threaded build ("3.14.7t") installs python3.14t, not python3.14.
    name="python$xy"
    case "$v" in *t) name="python${xy}t" ;; esac
    mods="ssl, sqlite3, lzma, ctypes, bz2, zlib, readline, curses, tkinter, uuid, _decimal, pyexpat"
    if [ "$major" -eq 3 ] && [ "$minor" -ge 14 ]; then mods="$mods, compression.zstd"; fi ;;
esac
py="$p/bin/$name"
"$py" -c "import $mods; print('modules ok')"

# Every script in bin/ with a shebang must name an interpreter that this install provides;
# nothing may point at a staging (.tmp-*) or build directory. Two upstream scripts are not
# meant to run from here and keep their fixed shebang: *-config (#!/bin/sh) and *-gdb.py.
if [ "$conda" = 0 ]; then
for f in "$p"/bin/*; do
  [ -f "$f" ] && [ ! -L "$f" ] || continue
  first=$(head -c 2 "$f" | od -An -c | tr -d ' ')
  [ "$first" = '#!' ] || continue
  got=$(head -1 "$f")
  base=$(basename "$f")
  case "$got" in
    *.tmp-*|*.old-*) echo "::error::$base shebang points into staging: '$got'"; exit 1 ;;
  esac
  ok=0
  case "$got" in
    "#!$p/bin/"*)
      target="${got#"#!$p/bin/"}"
      case "$target" in */*|"") ;; *) [ -e "$p/bin/$target" ] && ok=1 ;; esac ;;
  esac
  case "$base:$got" in
    *-config:'#!/bin/sh') ok=1 ;;
    *-gdb.py:'#!/usr/bin/python') ok=1 ;;
  esac
  [ "$ok" = 1 ] || { echo "::error::$base shebang is '$got', want '#!$p/bin/<interpreter present in bin>'"; exit 1; }
done
fi

"$py" -m pip --version
left=$(find "$root/versions" -maxdepth 1 \( -name '.tmp-*' -o -name '.old-*' \))
[ -z "$left" ] || { echo "::error::staging left behind: $left"; exit 1; }
[ ! -e "$p/.rpyenv-incomplete" ] || { echo "::error::marker left in $p"; exit 1; }
[ -e "$root/shims/$name" ] || { echo "::error::no shim for $name"; exit 1; }
prefix=$(PYENV_ROOT="$root" PYENV_VERSION="$v" "$root/shims/$name" -c 'import sys; print(sys.prefix)')
[ "$prefix" = "$p" ] || { echo "::error::shim $name reports prefix '$prefix', want '$p'"; exit 1; }
if [ "$conda" = 1 ]; then
  [ ! -e "$root/shims/curl" ] || { echo "::error::conda's curl got a shim"; exit 1; }
  PYENV_ROOT="$root" "$PYENV_BIN" virtualenv "$v" ci-env
  [ -d "$p/envs/ci-env/conda-meta" ] || { echo "::error::no conda env at $p/envs/ci-env"; exit 1; }
fi
echo "install check ok: $v"
