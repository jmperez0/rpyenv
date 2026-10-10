#!/bin/sh
# Installs rpyenv on Linux (M6b design §6.2):
#   curl -fsSL https://github.com/jmperez0/rpyenv/releases/latest/download/install.sh | sh
# Options, after `sh -s --`:
#   --version vX.Y.Z    install that release instead of the latest
#   --init | --no-init  add pyenv to your shell's startup file, or don't (default: ask)
#   --take-over         move an upstream pyenv in PYENV_ROOT aside without asking
#   --restore-upstream  remove rpyenv and put a moved-aside upstream pyenv back
#   --uninstall         remove rpyenv's files (Python versions stay)
set -eu

REPO_URL=https://github.com/jmperez0/rpyenv
RPYENV_FILES="bin/pyenv bin/pyenv-shim completions/pyenv.bash completions/pyenv.zsh completions/pyenv.fish"

say() { printf 'rpyenv: %s\n' "$*"; }
die() { printf 'rpyenv: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage: install.sh [--version vX.Y.Z] [--init|--no-init] [--take-over]
                  [--restore-upstream] [--uninstall]
EOF
}

version=
init=ask
take_over=no
action=install
while [ $# -gt 0 ]; do
  case $1 in
    --version)
      [ $# -ge 2 ] || die "--version needs a release, such as v0.1.0"
      version=$2
      shift 2
      ;;
    --version=*) version=${1#--version=}; shift ;;
    --init) init=yes; shift ;;
    --no-init) init=no; shift ;;
    --take-over) take_over=yes; shift ;;
    --restore-upstream) action=restore-upstream; shift ;;
    --uninstall) action=uninstall; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
done

root=${PYENV_ROOT:-${HOME:?HOME is not set}/.pyenv}
saved=$root/.upstream-pyenv
tty_in=${RPYENV_INSTALL_TTY:-/dev/tty}

# A terminal to ask on, even under `curl | sh` (stdin is the script there).
interactive() { ( exec <"$tty_in" ) 2>/dev/null; }

# Asks a yes/no question; an empty answer is yes.
ask() {
  printf 'rpyenv: %s [Y/n] ' "$1" >&2
  reply=
  IFS= read -r reply <"$tty_in" || reply=n
  case $reply in
    ''|[Yy]*) return 0 ;;
    *) return 1 ;;
  esac
}

check_platform() {
  os=$(uname -s)
  [ "$os" = Linux ] || die "this installer is for Linux (this is $os); on Windows, use the MSI from $REPO_URL/releases"
  machine=$(uname -m)
  case $machine in
    x86_64|amd64) arch=x64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) die "no rpyenv build for $machine; there are builds for x86_64 and aarch64" ;;
  esac
}

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$2" "$1"
  else
    die "needs curl or wget to download rpyenv"
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  else
    die "needs sha256sum or shasum to check the download"
  fi
}

download() {
  if [ -n "${RPYENV_INSTALL_BASE_URL:-}" ]; then
    base=$RPYENV_INSTALL_BASE_URL
  elif [ -n "$version" ]; then
    base=$REPO_URL/releases/download/$version
  else
    base=$REPO_URL/releases/latest/download
  fi
  fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "cannot download $base/SHA256SUMS"
  line=$(grep "  rpyenv-[^ ]*-linux-$arch\.tar\.gz\$" "$tmp/SHA256SUMS" | head -n 1)
  [ -n "$line" ] || die "no linux-$arch build is listed in $base/SHA256SUMS"
  want=${line%% *}
  name=${line##* }
  say "downloading $name"
  fetch "$base/$name" "$tmp/$name" || die "cannot download $base/$name"
  got=$(sha256 "$tmp/$name")
  [ "$got" = "$want" ] || die "checksum mismatch for $name (expected $want, got $got); nothing was installed"
}

is_upstream() { [ -e "$root/libexec/pyenv" ] || [ -d "$root/.git" ]; }

confirm_take_over() {
  [ ! -e "$saved" ] || die "$saved already exists (an earlier take-over?); run with --restore-upstream first, or move it away"
  if [ "$take_over" = no ] && interactive &&
    ask "An upstream pyenv is installed in $root. Move it aside to $saved and install rpyenv there? Your Python versions stay."; then
    take_over=yes
  fi
  [ "$take_over" = yes ] || die "an upstream pyenv is installed in $root; rerun with --take-over to move it aside (your Python versions stay), or set PYENV_ROOT to another folder"
}

# Moves everything but the user's data into $saved, recording each name (design §6.2).
take_over() {
  mkdir "$saved"
  : >"$saved/moved.txt"
  for entry in "$root"/* "$root"/.[!.]* "$root"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    base=${entry##*/}
    case $base in
      versions|version|shims|cache|plugins|.upstream-pyenv) continue ;;
    esac
    mv "$entry" "$saved/$base"
    printf '%s\n' "$base" >>"$saved/moved.txt"
  done
  if [ -e "$root/plugins/python-build" ]; then
    mkdir -p "$saved/plugins"
    mv "$root/plugins/python-build" "$saved/plugins/python-build"
    printf '%s\n' plugins/python-build >>"$saved/moved.txt"
  fi
  say "moved the upstream pyenv to $saved (undo: --restore-upstream)"
}

remove_rpyenv_files() {
  # shellcheck disable=SC2086 # RPYENV_FILES is a list of words
  for f in $RPYENV_FILES; do
    rm -f "$root/$f"
  done
  rmdir "$root/bin" "$root/completions" 2>/dev/null || true
}

restore_upstream() {
  [ -f "$saved/moved.txt" ] || die "nothing to restore: $saved/moved.txt not found"
  remove_rpyenv_files
  while IFS= read -r entry; do
    [ -z "$entry" ] || [ ! -e "$root/$entry" ] || die "cannot restore $entry: $root/$entry exists; move it away and rerun"
  done <"$saved/moved.txt"
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    mkdir -p "$(dirname "$root/$entry")"
    mv "$saved/$entry" "$root/$entry"
  done <"$saved/moved.txt"
  rm "$saved/moved.txt"
  rmdir "$saved/plugins" 2>/dev/null || true
  rmdir "$saved" 2>/dev/null || say "kept $saved: it holds files the take-over didn't put there"
  say "removed rpyenv and put the upstream pyenv back in $root"
}

install_files() {
  mkdir -p "$tmp/stage"
  tar -xzf "$tmp/$name" -C "$tmp/stage"
  [ -f "$tmp/stage/bin/pyenv" ] || die "$name has no bin/pyenv"
  mkdir -p "$root/bin" "$root/completions"
  # shellcheck disable=SC2086 # RPYENV_FILES is a list of words
  for f in $RPYENV_FILES; do
    [ -f "$tmp/stage/$f" ] || continue
    # A rename replaces even a running binary; the old install stays until this point.
    cp "$tmp/stage/$f" "$root/$f.new"
    mv -f "$root/$f.new" "$root/$f"
  done
  chmod 755 "$root/bin/pyenv" "$root/bin/pyenv-shim"
}

shell_setup() {
  shell_name=${SHELL:-sh}
  shell_name=${shell_name##*/}
  if [ "$init" = ask ]; then
    if interactive && ask "Add pyenv to your $shell_name startup file?"; then
      init=yes
    else
      init=no
    fi
  fi
  if [ "$init" = yes ]; then
    PYENV_ROOT=$root "$root/bin/pyenv" init --install "$shell_name" ||
      say "could not edit your $shell_name startup file; see: $root/bin/pyenv init $shell_name"
  else
    say "to finish, add pyenv to your shell's startup file:"
    PYENV_ROOT=$root "$root/bin/pyenv" init "$shell_name" || true
  fi
}

uninstall() {
  if [ -L "$root/bin/pyenv" ] || [ -e "$root/libexec/pyenv" ]; then
    die "$root holds an upstream pyenv, not rpyenv; nothing was removed"
  fi
  [ -e "$root/bin/pyenv" ] || [ -e "$root/bin/pyenv-shim" ] || die "rpyenv isn't installed in $root"
  remove_rpyenv_files
  say "removed rpyenv from $root; your Python versions stay in $root/versions"
  say "remove the pyenv lines (PYENV_ROOT, PATH, pyenv init) from your shell's startup file"
  if [ -d "$saved" ]; then
    say "to bring back the upstream pyenv: rerun with --restore-upstream"
  fi
}

case $action in
  uninstall) uninstall; exit 0 ;;
  restore-upstream) restore_upstream; exit 0 ;;
esac
check_platform
if is_upstream; then
  confirm_take_over
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 130' INT TERM
download
if is_upstream; then
  take_over
fi
install_files
shell_setup
say "installed $(PYENV_ROOT=$root "$root/bin/pyenv" --version 2>/dev/null || echo rpyenv) in $root"
