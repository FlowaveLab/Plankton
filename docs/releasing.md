# macOS releases

Public source commits use the `zqqqqz2000` GitHub identity. Do not commit local
credentials, runtime transcripts, screenshots containing account information, or
private signing material. Scan both the release tree and its Git history before
publishing. Apple Developer ID signatures carry the certificate holder's legal
identity; this is separate from Git authorship.

Use the Node and Rust versions pinned in `.nvmrc` and `rust-toolchain.toml`.
Run `make check` from a clean checkout. Set `APPLE_SIGNING_IDENTITY` to the
available Developer ID Application identity and `NOTARY_KEYCHAIN_PROFILE` to an
existing `notarytool` profile, then run:

```bash
bash scripts/release-macos.sh
```

The script builds the CLI and desktop from the same commit, signs them, submits
both to Apple, requires an Accepted result, staples the app, verifies Gatekeeper,
and creates checksummed release archives and Homebrew definitions. It preserves
the bundled KeePassXC signature. Build paths are remapped to the public identity.

After inspecting the archives, create the matching `v<version>` tag and a GitHub
release in `FlowaveLab/Plankton`. Upload the versioned archives, `checksums.txt`,
`source-commit.txt`, `plankton.rb`, and `plankton-helper.rb`. Do not upload the
temporary notarization submission archive or private diagnostic logs.

Copy the generated cask and formula into `Casks/plankton.rb` and
`Formula/plankton-helper.rb` in `FlowaveLab/homebrew-tap`. Publish the tap only
after the release downloads are publicly available and their hashes match.
Verify `brew install --cask flowavelab/tap/plankton`, the installed CLI, app
signature, and both README and embedded skill installation paths.

The tag workflow builds additional CI artifacts. It does not publish a release
or update Homebrew; public macOS assets must pass the signing and notarization
checks above.
