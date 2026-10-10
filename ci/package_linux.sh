#!/bin/sh
# Builds a static (musl) rpyenv for this machine's architecture and packs the release
# tarball (M6b design §6.1, §7.1). Needs musl-tools and the musl rustup target.
# Usage: sh ci/package_linux.sh <out-dir>
set -eu
out=${1:?usage: sh ci/package_linux.sh <out-dir>}
case $(uname -m) in
  x86_64)
    arch=x64 target=x86_64-unknown-linux-musl
    export CC_x86_64_unknown_linux_musl=musl-gcc
    ;;
  aarch64)
    arch=arm64 target=aarch64-unknown-linux-musl
    export CC_aarch64_unknown_linux_musl=musl-gcc
    ;;
  *) echo "no rpyenv build for $(uname -m)" >&2; exit 1 ;;
esac
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
cargo build --release --locked --target "$target" -p pyenv -p pyenv-shim
for b in pyenv pyenv-shim; do
  if readelf -l "target/$target/release/$b" | grep -q INTERP; then
    echo "$b is dynamically linked; the release must be static" >&2
    exit 1
  fi
done
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/bin" "$stage/completions" "$out"
cp "target/$target/release/pyenv" "target/$target/release/pyenv-shim" "$stage/bin/"
cp completions/pyenv.bash completions/pyenv.zsh completions/pyenv.fish "$stage/completions/"
cp LICENSE README.md "$stage/"
name=rpyenv-$version-linux-$arch.tar.gz
tar -C "$stage" -czf "$out/$name" bin completions LICENSE README.md
echo "$out/$name"
