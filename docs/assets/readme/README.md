# README diagrams

Ten standalone SVGs provide English and Chinese variants of the access flow,
approval routing, exposure policy, feature overview, and planned
remote-server architecture.

The palette follows `apps/desktop/src/components/desktop/workspace.css`:

| Role      | Color     |
| --------- | --------- |
| Ink       | `#171716` |
| Paper     | `#f4f1ea` |
| Surface   | `#fffefb` |
| Accent    | `#f2381e` |
| Rule      | `#cfcac1` |
| Muted ink | `#706d67` |

Diagrams use square panels, thin rules, and native SVG line icons from the same
Lucide dependency used by the desktop app. They contain no emoji, embedded raster
images, scripts, external font dependencies, or live credential data. Each file
has a title, description, and language attribute for accessibility.

Lucide icon paths retain their [upstream license](./LUCIDE-LICENSE). The diagram
layouts and text follow the repository's [MIT license](../../../LICENSE).
