# Plankton logo design archive

The approved direction is a minimal red/white, left/right symmetric organism
with a long central spine between two curved antennae. The v2 concept adds that
spine to the initial v1 proposal.

- `plankton-logo-concept-v1.png` and `plankton-logo-concept-v2.png`: original
  generated design concepts, with the corresponding generation prompts.
- `plankton-logo-master.svg`: a snapshot of the deterministic production SVG.
- `plankton-macos-icon.png`: the macOS icon preview with transparent Dock margins.
- `plankton-logo-adaptations.html`: a standalone preview using embedded production
  assets, including light/dark menu-bar simulations at several display sizes.

The maintained source of truth is
`apps/desktop/src-tauri/assets/tray/plankton-mark.svg`, not this archive.
Run `npm run icons:generate` from `apps/desktop` to regenerate the frontend mark,
favicon, platform icons, and transparent tray assets. macOS uses black template
pixels with alpha and loads the Retina asset at runtime; AppKit supplies its color.
