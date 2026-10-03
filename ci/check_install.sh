#!/usr/bin/env bash
# Tier 3 (spec §12.5): checks a real `pyenv install` result.
#   ci/check_install.sh <PYENV_ROOT> <version>
set -euo pipefail
root="$1"
v="$2"
p="$root/versions/$v"
xy=$(echo "$v" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')
major=${xy%%.*}
minor=${xy#*.}
# A free-threaded build ("3.14.7t") installs python3.14t, not python3.14.
name="python$xy"
case "$v" in *t) name="python${xy}t" ;; esac
py="$p/bin/$name"

mods="ssl, sqlite3, lzma, ctypes, bz2, zlib, readline, curses, tkinter, uuid, _decimal, pyexpat"
if [ "$major" -eq 3 ] && [ "$minor" -ge 14 ]; then mods="$mods, compression.zstd"; fi
"$py" -c "import $mods; print('modules ok')"

# Every script in bin/ with a shebang must name an interpreter that this install provides;
# nothing may point at a staging (.tmp-*) or build directory. Two upstream scripts are not
# meant to run from here and keep their fixed shebang: *-config (#!/bin/sh) and *-gdb.py.
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

"$py" -m pip --version
left=$(find "$root/versions" -maxdepth 1 \( -name '.tmp-*' -o -name '.old-*' \))
[ -z "$left" ] || { echo "::error::staging left behind: $left"; exit 1; }
[ ! -e "$p/.rpyenv-incomplete" ] || { echo "::error::marker left in $p"; exit 1; }
[ -e "$root/shims/$name" ] || { echo "::error::no shim for $name"; exit 1; }
prefix=$(PYENV_ROOT="$root" PYENV_VERSION="$v" "$root/shims/$name" -c 'import sys; print(sys.prefix)')
[ "$prefix" = "$p" ] || { echo "::error::shim $name reports prefix '$prefix', want '$p'"; exit 1; }
echo "install check ok: $v"
