#!/usr/bin/env bash
# Evaluates one python-build conda definition (Anaconda, Miniconda, Miniforge) for one
# architecture without installing anything (M7b design §4.1): python-build's functions are
# stubs that record what the definition would install. ci/conda_tables.py runs it over the
# vendored definitions in a container with no network; rpyenv runs it
# (`bash -c "$script" conda_eval <file> <arch>`) for a user's own conda definition.
#
#   conda_eval.sh <definition file> <anaconda architecture>
#
# Prints one line: `ok <package> <url>[#<sha256>] <step> <verify> [<NAME>=<value>...]` (the
# variables the definition exported), `no-binary <what>` (the definition's own "binary
# distribution of <what> is not available" branch), or `unsupported <reason>`.
if [ $# -ne 2 ]; then
  echo "usage: conda_eval.sh <definition file> <arch>" >&2
  exit 2
fi
out=$(
  (
    anaconda_architecture() { echo "$CONDA_EVAL_ARCH"; }
    install_script() {
      local line="PKG" a e
      for a in "$@"; do line="$line $a"; done
      for e in $(compgen -e); do
        case " $CONDA_EVAL_BEFORE " in
          *" $e "*) ;;
          *) line="$line $e=${!e}" ;;
        esac
      done
      echo "$line" >&3
    }
    colorize() { printf '%s' "$2"; }
    command_not_found_handle() { echo "MISSING $1" >&3; return 127; }
    CONDA_EVAL_ARCH=$2
    CONDA_EVAL_BEFORE=$(compgen -e | tr '\n' ' ')
    # shellcheck disable=SC1090
    source "$1" 2>&3
  ) 3>&1 >/dev/null 2>&1
)
rc=$?
missing=$(printf '%s\n' "$out" | sed -n 's/^MISSING //p' | head -n 1)
pkgs=$(printf '%s\n' "$out" | grep -c '^PKG ')
what=$(printf '%s\n' "$out" | sed -n 's/^.*The binary distribution of \(.*\) is not available for .*$/\1/p' | head -n 1)
if [ -n "$missing" ]; then
  echo "unsupported calls $missing"
elif [ "$pkgs" -eq 1 ] && [ "$rc" -eq 0 ]; then
  set -f
  # shellcheck disable=SC2046
  set -- $(printf '%s\n' "$out" | sed -n 's/^PKG //p')
  if [ $# -lt 4 ]; then
    echo "unsupported install_script with $# arguments"
    exit 0
  fi
  pkg=$1 url=$2 step=$3 verify=$4
  shift 4
  hash=${url##*#}
  case "$step" in anaconda|miniconda) ;; *) echo "unsupported step $step"; exit 0 ;; esac
  case "$verify" in verify_py*) ;; *) echo "unsupported verify step $verify"; exit 0 ;; esac
  if [ "$hash" != "$url" ] && { [ ${#hash} -ne 64 ] || [ -n "${hash//[0-9a-f]/}" ]; }; then
    echo "unsupported malformed sha256 in $url"
  else
    echo "ok $pkg $url $step $verify${*:+ $*}"
  fi
elif [ "$pkgs" -eq 0 ] && [ "$rc" -ne 0 ] && [ -n "$what" ]; then
  echo "no-binary $what"
else
  echo "unsupported $pkgs packages, exit $rc"
fi
