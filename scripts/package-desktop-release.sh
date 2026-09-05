#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
  echo "usage: $0 <version> <app-path> [output-dir]" >&2
  exit 1
fi

version="$1"
app_path="$2"
output_dir="${3:-dist}"

if [ ! -d "$app_path" ]; then
  echo "app bundle not found: $app_path" >&2
  exit 1
fi

mkdir -p "$output_dir"

archive_name="plankton-v${version}-macos-aarch64.zip"
staging_dir="$(mktemp -d)"
trap 'rm -rf "$staging_dir"' EXIT

cli_path="$(dirname "$(dirname "$(dirname "$app_path")")")/plankton"
if [ ! -f "$cli_path" ]; then
  echo "CLI binary not found: $cli_path" >&2
  exit 1
fi
ditto "$app_path" "$staging_dir/Plankton.app"
cp "$cli_path" "$staging_dir/plankton"
chmod 0755 "$staging_dir/plankton"
ditto -c -k --sequesterRsrc "$staging_dir" "$output_dir/$archive_name"
