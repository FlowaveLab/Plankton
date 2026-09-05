#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <version> <github-repository> <checksums-file>" >&2
  exit 1
fi

version="$1"
github_repository="$2"
checksums_file="$3"
formula_class="PlanktonHelper"
binary_name="plankton"
homepage_url="https://github.com/${github_repository}"

lookup_sha() {
  local archive_name="$1"
  awk -v file="$archive_name" '$2 == file { print $1; exit }' "$checksums_file"
}

archive_name="plankton-v${version}-aarch64-apple-darwin.tar.gz"
archive_sha="$(lookup_sha "$archive_name")"

if [ -z "$archive_sha" ]; then
  echo "missing checksum for ${archive_name} in ${checksums_file}" >&2
  exit 1
fi

cat <<EOF
class ${formula_class} < Formula
  desc "Command-line companion installed by the Plankton desktop cask"
  homepage "${homepage_url}"
  license "MIT"
  version "${version}"
  depends_on :macos
  depends_on arch: :arm64
  url "${homepage_url}/releases/download/v${version}/plankton-v${version}-aarch64-apple-darwin.tar.gz"
  sha256 "${archive_sha}"

  def install
    bin.install "${binary_name}"
    prefix.install_metafiles
  end

  test do
    help = shell_output("#{bin}/${binary_name} --help")
    assert_match "read-only", help
  end
end
EOF
