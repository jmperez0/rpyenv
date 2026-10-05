#!/usr/bin/env bash
# Runs pyenv-virtualenv's bats suite against rpyenv's built-in virtualenv commands (spec §12.3,
# plan M4b Task 1; docs/parity/pyenv-virtualenv-m4-harness-and-windows.md §1.3). Run as root,
# in a Debian container on CI or on a Debian host:
#   bash parity/bats_venv_run.sh <rpyenv-bin-dir> <pyenv-virtualenv-src> <bats-dir> <tap-out>
# The suite calls `pyenv-<cmd>` names on a PATH it builds from `<test>/../bin`, so rpyenv and its
# multicall links take the place of the plugin's own bin/. Each file runs as `tester`, against
# root-owned binaries. installer.bats is left out: it tests the plugin's install.sh, which
# rpyenv doesn't have.
set -euo pipefail
for a in "$1" "$2" "$3"; do
  [ -e "$a" ] || { echo "bats_venv_run.sh: no such file or directory: $a" >&2; exit 2; }
done
bin=$(realpath "$1") up=$(realpath "$2") bats=$(realpath "$3") out=$(realpath -m "$4")
work=$(mktemp -d /tmp/rpyenv-bats-venv.XXXXXX)
chmod 755 "$work"
install -d -m 755 "$work/run" "$work/run/bin"
install -m 755 "$bin/pyenv" "$bin/pyenv-shim" "$work/run/bin/"
for c in activate deactivate sh-activate sh-deactivate virtualenv virtualenv-delete \
         virtualenv-init virtualenv-prefix virtualenvs; do
  ln -s pyenv "$work/run/bin/pyenv-$c"
done
cp -r "$up/test" "$work/run/test"
rm -f "$work/run/test/installer.bats"
# The lenient overlay (user decision 2026-10-05; harness doc §1.3 point 2): rpyenv answers these
# in-process (allowlist D-52), so a test's stub of one is never run, and unstub only removes it.
# Every other unstub verifies as upstream's does.
cat >> "$work/run/test/test_helper.bash" <<'OVERLAY'
# --- rpyenv overlay, appended by parity/bats_venv_run.sh ---
_RPY_INPROC=" pyenv-version-name pyenv-version-origin pyenv-prefix pyenv-hooks pyenv-rehash pyenv-exec pyenv-which pyenv-whence pyenv-latest python-build curl pyenv-virtualenv-prefix pyenv-sh-deactivate "
eval "_rpy_up_unstub() $(declare -f unstub | tail -n +2)"
unstub() {
  if [[ $_RPY_INPROC == *" $1 "* ]]; then
    rm -f "$TMP/bin/$1" "$TMP/$1-stub-plan" "$TMP/$1-stub-run" "$TMP/$1-stub-log"
    return 0
  fi
  _rpy_up_unstub "$@"
}
OVERLAY
id tester >/dev/null 2>&1 || useradd --create-home tester
chown -R tester "$work/run/test"
: > "$out"
cd "$work/run/test"
for f in $(ls -- *.bats); do
  # Each file starts with an empty `test/tmp`: a test that fails by design (it expects a stub
  # rpyenv never calls) dies before its unstub, and the stub left in `tmp/bin`, first on the
  # suite's PATH, would answer for the next file's command instead of rpyenv.
  if [ -d "$work/run/test/tmp" ]; then find "$work/run/test/tmp" -mindepth 1 -delete; fi
  runuser -u tester -- "$bats/bin/bats" --tap "./$f" 2>&1 | sed "s|^|$f\t|" >> "$out" || true
done
echo "bats: $(grep -c -P '\tok ' "$out") ok, $(grep -c -P '\tnot ok ' "$out") not ok"
