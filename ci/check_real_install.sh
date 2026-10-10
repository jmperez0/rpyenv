#!/bin/sh
# Installs the real tarball with install.sh into a throwaway HOME, with no terminal, then
# uninstalls it (M6b design §7.4). Usage: sh ci/check_real_install.sh <dist-dir>
set -eu
dist=$(cd "${1:?usage: sh ci/check_real_install.sh <dist-dir>}" && pwd)
(cd "$dist" && sha256sum rpyenv-*.tar.gz >SHA256SUMS)
port=8765
python3 -m http.server --bind 127.0.0.1 --directory "$dist" "$port" >/dev/null 2>&1 &
server=$!
trap 'kill "$server"' EXIT
i=0
until curl -fs "http://127.0.0.1:$port/SHA256SUMS" >/dev/null; do
  i=$((i + 1))
  [ "$i" -lt 50 ] || { echo "the test server didn't start" >&2; exit 1; }
  sleep 0.1
done
home=$(mktemp -d)
HOME=$home RPYENV_INSTALL_BASE_URL=http://127.0.0.1:$port RPYENV_INSTALL_TTY=/nonexistent \
  sh install.sh --no-init </dev/null
"$home/.pyenv/bin/pyenv" --version
"$home/.pyenv/bin/pyenv" --version | grep -qi rpyenv
test -x "$home/.pyenv/bin/pyenv-shim"
HOME=$home sh install.sh --uninstall
test ! -e "$home/.pyenv/bin/pyenv"
echo "real install: ok"
