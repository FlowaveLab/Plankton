#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
  echo "usage: $0 <version> <target> <binary-path> [output-dir]" >&2
  exit 1
fi

version="$1"
target="$2"
binary_path="$3"
output_dir="${4:-dist}"

if [ ! -f "$binary_path" ]; then
  echo "binary not found: $binary_path" >&2
  exit 1
fi

mkdir -p "$output_dir"

archive_name="plankton-v${version}-${target}.tar.gz"
staging_dir="$(mktemp -d)"
trap 'rm -rf "$staging_dir"' EXIT

cp "$binary_path" "$staging_dir/plankton"
chmod 0755 "$staging_dir/plankton"
cp LICENSE "$staging_dir/LICENSE"
cp README.md "$staging_dir/README.md"

tar -C "$staging_dir" -czf "$output_dir/$archive_name" plankton LICENSE README.md
