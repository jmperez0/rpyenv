#!/usr/bin/env bash
# Evaluates one python-build PyPy definition for one architecture without installing anything
# (M7a design §4): python-build's functions are stubs that record what the definition would
# install. ci/pypy_tables.py runs it over the vendored definitions in a container with no
# network; rpyenv runs it (`bash -c "$script" pypy_eval <file> <arch>`) for a user's own PyPy
# definition, which upstream would source with bash too.
#
#   pypy_eval.sh <definition file> <pypy architecture>
#
# Prints one line: `ok <package> <url>#<sha256> <step>...`, `no-binary` (the definition stopped
# without installing, as upstream's "binary distribution … not available" error does), or
# `unsupported <reason>`. No temporary files, so it runs on a read-only file system.
if [ $# -ne 2 ]; then
  echo "usage: pypy_eval.sh <definition file> <arch>" >&2
  exit 2
fi
out=$(
  (
    pypy_architecture() { echo "$PYPY_EVAL_ARCH"; }
    # The template shape's `declare -a pd="$(...)"` makes pd a plain string, so `${pd[0]}` word-splits
    # into the three words and "${pd[1]}" "${pd[2]}" arrive as two empty arguments: drop empties.
    install_package() {
      local line="PKG" a
      for a in "$@"; do [ -n "$a" ] && line="$line $a"; done
      echo "$line" >&3
    }
    install_zip() { install_package "$@"; }
    colorize() { printf '%s' "$2"; }
    require_osx_version() { return 1; }
    command_not_found_handle() { echo "MISSING $1" >&3; return 127; }
    PYPY_EVAL_ARCH=$2
    # shellcheck disable=SC1090
    source "$1"
  ) 3>&1 >/dev/null 2>&1
)
rc=$?
missing=$(printf '%s\n' "$out" | sed -n 's/^MISSING //p' | head -n 1)
pkgs=$(printf '%s\n' "$out" | grep -c '^PKG ')
if [ -n "$missing" ]; then
  echo "unsupported calls $missing"
elif [ "$pkgs" -eq 1 ] && [ "$rc" -eq 0 ]; then
  line=$(printf '%s\n' "$out" | sed -n 's/^PKG //p')
  pkg=${line%% *}
  rest=${line#* }
  url=${rest%% *}
  steps=${rest#"$url"}
  steps=${steps# }
  hash=${url##*#}
  if [ "$hash" = "$url" ] || [ ${#hash} -ne 64 ] || [ -n "${hash//[0-9a-f]/}" ]; then
    echo "unsupported no sha256 in $url"
  elif [ -z "$steps" ]; then
    echo "unsupported no build steps in $pkg"
  else
    echo "ok $pkg $url $steps"
  fi
elif [ "$pkgs" -eq 0 ] && [ "$rc" -ne 0 ]; then
  echo "no-binary"
else
  echo "unsupported $pkgs packages, exit $rc"
fi
