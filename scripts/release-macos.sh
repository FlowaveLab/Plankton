#!/usr/bin/env bash
set -euo pipefail

# Build from a clean, reviewed checkout. Credentials remain in the login keychain.
: "${APPLE_SIGNING_IDENTITY:?Set the Developer ID Application certificate identity}"
: "${NOTARY_KEYCHAIN_PROFILE:?Set an existing notarytool keychain profile}"
if [ "$(uname -sm)" != "Darwin arm64" ]; then
  echo "This release script requires Apple Silicon macOS" >&2
  exit 1
fi
cd "$(git rev-parse --show-toplevel)"
if [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
  echo "Release checkout must be clean" >&2
  exit 1
fi
source_commit="$(git rev-parse HEAD)"
version="$(sed -n '/\[workspace.package\]/,/^\[/s/^version = "\([^"]*\)"/\1/p' Cargo.toml)"
test -n "$version"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${HOME}=/Users/zqqqqz2000"
export CARGO_PROFILE_RELEASE_STRIP=symbols
npm --prefix apps/desktop ci
cargo build --locked --release -p plankton
env -u APPLE_SIGNING_IDENTITY npm --prefix apps/desktop run tauri build -- --bundles app

test "$(git rev-parse HEAD)" = "$source_commit"
git diff --exit-code HEAD
app="target/release/bundle/macos/Plankton.app"
cli="target/release/plankton"
# Preserve the upstream KeePassXC signature and the engine's pinned CLI digest.
codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$cli"
codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$app"
codesign --verify --strict "$cli"
codesign --verify --deep --strict "$app"

mkdir -p dist
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
ditto "$app" "$staging/Plankton.app"
cp "$cli" "$staging/plankton"
ditto -c -k --keepParent "$staging" dist/notarization-upload.zip
xcrun notarytool submit dist/notarization-upload.zip \
  --keychain-profile "$NOTARY_KEYCHAIN_PROFILE" --wait --output-format json > dist/notarization.json
python3 -c 'import json; assert json.load(open("dist/notarization.json"))["status"] == "Accepted"'
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose=2 "$app"
bash scripts/package-cli-release.sh "$version" aarch64-apple-darwin "$cli" dist
bash scripts/package-desktop-release.sh "$version" "$app" dist
git archive --format=tar.gz --prefix="plankton-v${version}/" HEAD > "dist/plankton-v${version}-source.tar.gz"
(cd dist && shasum -a 256 "plankton-v${version}"*.tar.gz "plankton-v${version}"*.zip > checksums.txt)
bash scripts/render-homebrew-formula.sh "$version" FlowaveLab/Plankton dist/checksums.txt > dist/plankton-helper.rb
bash scripts/render-homebrew-cask.sh "$version" FlowaveLab/Plankton dist/checksums.txt > dist/plankton.rb
printf '%s\n' "$source_commit" > dist/source-commit.txt
echo "Signed and notarized release files are ready in dist. Review before publishing."
