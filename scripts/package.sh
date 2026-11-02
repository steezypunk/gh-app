#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 VERSION TARGET" >&2
  echo "example: $0 v0.1.0 x86_64-unknown-linux-musl" >&2
  exit 2
fi

version="$1"
target="$2"
binary="target/${target}/release/gh-app"
dist="dist"
archive="${dist}/gh-app-${version}-${target}.tar.gz"

if [ ! -f "$binary" ]; then
  echo "missing binary: $binary" >&2
  echo "build it first: cross build --release --locked --target $target" >&2
  exit 1
fi

mkdir -p "$dist"
cp "$binary" "$dist/gh-app"
tar -C "$dist" -czf "$archive" gh-app
sha256sum "$archive" > "${archive}.sha256"
rm -f "$dist/gh-app"

echo "created $archive"
echo "created ${archive}.sha256"
