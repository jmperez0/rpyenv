#!/usr/bin/env bash
# Runs upstream pyenv's bats suite against rpyenv (spec §12.3, plan decision 2). Run it as
# root, in a Debian container on CI or on a Debian host:
#   bash parity/bats_run.sh <rpyenv-bin-dir> <pyenv-src-dir> <bats-dir> <tap-out>
# Each test file runs as the unprivileged user `tester`, against root-owned copies of rpyenv's
# binaries that no test can change. <tap-out> gets one `<file>\t<TAP line>` line per TAP line.
set -euo pipefail
for a in "$1" "$2" "$3"; do
  [ -e "$a" ] || { echo "bats_run.sh: no such file or directory: $a" >&2; exit 2; }
done
bin=$(realpath "$1") up=$(realpath "$2") bats=$(realpath "$3") out=$(realpath -m "$4")
work=$(mktemp -d /tmp/rpyenv-bats.XXXXXX)
chmod 755 "$work"
install -d -m 755 "$work/bin"
install -m 755 "$bin/pyenv" "$bin/pyenv-shim" "$work/bin/"
# The suite puts `<test>/../libexec` on PATH: there, `pyenv` is rpyenv and each `pyenv-<cmd>`
# the suite calls directly is a wrapper for `pyenv <cmd>`. The wrappers use the absolute path: a
# test may stub `pyenv` on PATH, and upstream's `pyenv-<cmd>` scripts never go through it.
mkdir -p "$work/run/libexec"
cp -r "$up/test" "$work/run/test"
cp -r "$up/pyenv.d" "$work/run/pyenv.d"
ln -s "$work/bin/pyenv" "$work/run/libexec/pyenv"
# When a milestone delivers a command (latest, init, shell, completions, sh-*), add it to this
# list (latest: M2a), or its upstream tests keep failing through the missing wrapper.
for c in root prefix version version-name version-origin version-file version-file-read \
         version-file-write versions which whence exec rehash shims commands help global local latest; do
  printf '#!/bin/sh\nexec "%s" %s "$@"\n' "$work/bin/pyenv" "$c" > "$work/run/libexec/pyenv-$c"
done
printf '#!/bin/sh\nexec "%s" --version "$@"\n' "$work/bin/pyenv" > "$work/run/libexec/pyenv---version"
chmod 755 "$work/run/libexec/"pyenv-*
id tester >/dev/null 2>&1 || useradd --create-home tester
chown -R tester "$work/run/test" "$work/run/pyenv.d"
: > "$out"
cd "$work/run/test"
for f in $(ls -- *.bats); do
  runuser -u tester -- "$bats/bin/bats" --tap "./$f" 2>&1 | sed "s|^|$f\t|" >> "$out" || true
done
echo "bats: $(grep -c -P '\tok ' "$out") ok, $(grep -c -P '\tnot ok ' "$out") not ok"
