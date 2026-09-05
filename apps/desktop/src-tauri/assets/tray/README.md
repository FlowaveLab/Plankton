# Plankton system icons

`plankton-mark.svg` is the single deterministic vector master: an upright,
left/right symmetric plankton with a long central spine, two curved antennae,
one pair of short side fins, and a rounded body with an oval cutout. The mark is
a single filled path with even-odd transparency, without fine anatomical detail.
The approved red/white concept is adapted to exact mirror symmetry in this SVG.

- macOS uses `plankton-trayTemplate.svg` as a template image. The source is
  black with alpha; AppKit renders it white on a dark menu bar.
  The runtime loads the 64px `@2x` mark and matching spinner frames; AppKit
  displays them at 18 points with sufficient resolution for Retina screens.
- Windows uses separate light/dark tray marks and the red taskbar mark.
- The desktop sidebar imports the generated path in `src/generated/planktonMark.ts`.
- `public/` contains the red and white transparent SVG marks and red favicon.
- App icons use white on red (`#e92339`); the macOS ICNS retains a transparent
  Dock safe area. PNG, Windows ICO/store, iOS, and Android assets are regenerated.
- Idle and attention always keep the Plankton mark static; attention
  adds its badge without transforming the mark.
- Reasoning uses eight dedicated circular spinner frames. macOS spinner frames
  are black template pixels with alpha, while Windows has separate light and
  dark themed frames. Reduced-motion mode keeps spinner frame 0 as a static
  partial ring.

Generated PNG/ICO/ICNS outputs are build artifacts and must be derived from
this master so alpha, dimensions, and checksums remain reproducible. Run
`npm run icons:generate` from `apps/desktop` after changing the master.
