#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <version> <github-repository> <checksums-file>" >&2
  exit 1
fi

version="$1"
github_repository="$2"
checksums_file="$3"
homepage_url="https://github.com/${github_repository}"

lookup_sha() {
  local archive_name="$1"
  awk -v file="$archive_name" '$2 == file { print $1; exit }' "$checksums_file"
}

archive_name="plankton-v${version}-macos-aarch64.zip"
archive_sha="$(lookup_sha "$archive_name")"

if [ -z "$archive_sha" ]; then
  echo "missing checksum for ${archive_name} in ${checksums_file}" >&2
  exit 1
fi

cat <<EOF
cask "plankton" do
  version "${version}"
  sha256 "${archive_sha}"

  url "${homepage_url}/releases/download/v#{version}/plankton-v#{version}-macos-aarch64.zip"
  name "Plankton"
  desc "Local-first approval console for sensitive resource access"
  homepage "${homepage_url}"

  depends_on arch: :arm64

  app "Plankton.app"
  binary "plankton"
end
EOF
