#!/usr/bin/env bash
# Tier 3 (spec §12.5): checks a real `pyenv install` result.
#   ci/check_install.sh <PYENV_ROOT> <version>
set -euo pipefail
root="$1"
v="$2"
p="$root/versions/$v"
xy=$(echo "$v" | sed -E 's/^([0-9]+\.[0-9]+).*/\1/')
py="$p/bin/python$xy"
"$py" -c "import ssl, sqlite3, lzma, ctypes, bz2, zlib, readline, curses; print('modules ok')"
want="#!$py"
for s in pip3 idle3; do
  got=$(head -1 "$p/bin/$s")
  [ "$got" = "$want" ] || { echo "::error::$s shebang is '$got', want '$want'"; exit 1; }
done
"$py" -m pip --version
left=$(find "$root/versions" -maxdepth 1 -name '.tmp-*' -o -maxdepth 1 -name '.old-*' -o -maxdepth 1 -name '.lock-*')
[ -z "$left" ] || { echo "::error::staging left behind: $left"; exit 1; }
[ ! -e "$p/.rpyenv-incomplete" ] || { echo "::error::marker left in $p"; exit 1; }
[ -e "$root/shims/python$xy" ] || { echo "::error::no shim for python$xy"; exit 1; }
echo "install check ok: $v"
