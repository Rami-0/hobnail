# Hobnail branding

**Hobnail** is the product name. Use lowercase `hobnail` for the executable,
package, Docker image, Compose service and container-label prefix.

**Tagline:** A tiny home for your homelab.

A hobnail is the short nail hammered into a boot sole. The mark is that nail,
forged in Rust orange, drawn on a diagonal with the head upper right and the
point lower left.

## The mark

One nail, two cuts. They share an outline exactly, so one can replace the other
without anything moving.

| Cut | File | Use |
|---|---|---|
| Glyph | `docs/brand/mark-glyph.svg` | One colour. Favicon, sidebar, anywhere small or single-ink. |
| Facets | `docs/brand/mark-facets.svg` | Full colour: five flat tones, no gradients. App icons, banners, the wordmark. |

At 64 px and below a long thin nail is a sliver, so small sizes use a close-up:
the head and upper shank fill the tile and the point runs off the corner. The
favicon and the app icons are that crop.

The nail is straight: the head sits square on the shank. Where the mark is
tilted, the whole nail is rotated as one piece (49.5° from upright); the head is
never angled on its own.

Do not add gradients, outlines, shadows or a 3D render, do not change the tilt,
and do not redraw the head larger: earlier rounds that did read as a pushpin or
a mallet.

## The wordmark

`HOBNAIL` in Big Shoulders Display Black, with the faceted nail standing upright
as the I. `docs/brand/wordmark.svg` is for dark grounds and `wordmark-light.svg`
for light ones. The letters are outlines, so the files need no font.

## Colour

| Role | Dark | Light |
|---|---|---|
| Surface | `#1D1210` | `#FFF8F3` |
| Raised surface | `#35231D` | `#F4E8E0` |
| Border | `#795245` | `#AD8877` |
| Text | `#FFF0E5` | `#2B1912` |
| Muted text | `#CDB4A8` | `#725448` |
| Accent (Rust orange) | `#F77848` | `#AC3712` |
| Accent, hover | `#FF956B` | `#922B0A` |
| Text on accent | `#26120B` | `#FFFFFF` |
| OK | `#83CCA7` | `#216648` |
| Warning | `#F4D05E` | `#806000` |
| Error | `#FF678B` | `#AD1747` |

Text and accent pass 4.5:1 on their surface in both themes. Because the accent
is orange, warnings are yellow and errors are crimson so the three never blur.

The mark's five tones are `#FF956B`, `#F77848`, `#AC3712`, `#7A2A12` and a
`#FFF0E5` glint.

These colours are the brand. The app's own themes still use their earlier
palette; moving the UI onto these tokens is a separate change.

## Type

| Face | Use |
|---|---|
| Big Shoulders Display Black | The wordmark and display headings |
| Instrument Serif | The Pulse banner |
| Fraunces | The Plate banner |
| JetBrains Mono | Small technical captions in banners |

All four are under the SIL Open Font License.

## Assets

| File | Size | Used for |
|---|---|---|
| `static/brand/hobnail.svg` | vector | `/icon.svg`: favicon, sidebar tile, offline page |
| `static/brand/icon-1024.png` | 1024 px | App icon master |
| `static/pwa/icon-512.png`, `icon-192.png` | 512 / 192 px | Web app manifest (`any maskable`) |
| `static/pwa/apple-touch-icon.png` | 180 px | iOS home screen |
| `docs/brand/social-preview.png` | 1280×640 | The wordmark banner: README header, GitHub social preview |
| `docs/brand/banner-pulse.png` | 2560×1280 | README: what it does |
| `docs/brand/banner-plate.png` | 2560×1280 | README: anatomy of the nail |

The three PWA PNGs are compiled into the binary, so they must stay small: a
test in `src/ui/pwa.rs` fails above 8 KB (192 px, 180 px) or 24 KB (512 px).

## Rebuilding

```sh
python3 docs/brand/src/build.py            # everything
python3 docs/brand/src/build.py icons      # or one of: vectors icons struck pulse plate
```

It needs Pillow, fontTools and a Chromium headless shell (the Playwright one is
found automatically; set `CH` to use another). The wordmark font is fetched into
`docs/brand/src/fonts/` on first run, and the banners load their text faces from
Google Fonts while rendering.

`docs/brand/src/mark.json` is the mark itself, as SVG path data. It was traced
once with `trace.py` from two rasters drawn with Codex image generation and
should not need regenerating; edit the paths directly for small corrections.

## The old name

The old name appears only in compatibility handling, migration guidance,
actual checkout paths and historical records.
Licensing and domain registration are separate decisions.
