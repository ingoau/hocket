# Hocket — brand

The mark, the word mark, and how they go together. The name comes from the technique (see the README): one line, two voices taking alternate notes. The mark draws that, and everything else stays quiet around it.


## The Crescendo mark  
*Status: decided*

Four notes on a 100-unit tile, one continuous wave, the voices alternating 1, 2, 1, 2. Each note rises or falls further than the one before.

| Note | Path (100-unit tile) | Voice |
| --- | --- | --- |
| 1 | `M16 48.5 C23 37.81 26 37.81 33 48.5` | 1 |
| 2 | `M33 48.5 C40 63.2 43 63.2 50 48.5` | 2 |
| 3 | `M50 48.5 C57 29.8 60 29.8 67 48.5` | 1 |
| 4 | `M67 48.5 C74 71.21 77 71.21 84 48.5` | 2 |

- **Stroke** 12, round caps, no fill. Each note overlaps the one before it, so the caps hide the hand-offs.
- **Extent** x 10–90, y 28.4–71.6, caps included.
- **The tile** is a rounded square, corner radius 22.45% of its side (185 on Apple's 824 px body), with a vertical gradient.
- **Never** redraw the wave by eye, recolour one note, add a third voice, or set the wave on a background lighter than the voices without swapping to the light-ground colours below.

Where it lives (the geometry is the same in each, and changes go to all of them):

- `desktop/build/icon.svg` — the flat master on Apple's grid.
- `desktop/build/icon.icon/` — the shipped macOS icon (Icon Composer); `desktop/scripts/gen-app-icons.mjs` renders the note layers, `.ico` and Linux PNGs from it.
- `desktop/src/main/brand.ts` — rasterised at runtime for the tray and the dev-build dock icon.
- `android/app/src/main/res/drawable/ic_launcher_foreground.xml` — the adaptive icon's foreground and monochrome layer.


## Colour: Tonal Violet  
*Status: decided*

| Role | Hex | Used for |
| --- | --- | --- |
| Voice 1 | `#E4DFFF` | Notes 1 and 3 |
| Voice 2 | `#8F80F5` | Notes 2 and 4 |
| Tile, top | `#2A2447` | Icon gradient |
| Tile, bottom | `#1B1730` | Icon gradient; ink of the word mark on light grounds |
| Launcher background | `#221D3A` | Android adaptive icon (one colour, so the midpoint of the tile) |
| Word mark on dark | `#ECE9FF` | The word mark on dark grounds |

The two voices are the only colour in the brand. The word mark is always one colour.


## The word mark  
*Status: decided*

**Unbounded**, weight 500, lowercase: `hocket`.

- **Tracking** −0.03em at display sizes, −0.015em at 24 px and under.
- **Case** always lowercase in the word mark, including at the start of a sentence-like layout. In running text the name is still written *Hocket*.
- **Colour** one colour: `#ECE9FF` on dark, `#1B1730` on light. Never the voice colours, never split by letter or syllable; the mark carries the colour.
- **No effects** — no gradients, outlines, shadows, or baseline shifts.
- **Why Unbounded:** its wide, round letters match the mark's heavy stroke and the tile's rounded corners, and it sits with the Material 3 Expressive shapes in the app. It's wide, so the lockup needs horizontal room.
- **Licence** SIL Open Font License 1.1 (Google Fonts), so it can ship in both apps.


## The lockup  
*Status: decided*

The app icon (the tile, not the bare wave) to the left of the word mark, centred vertically on the x-height.

- **Icon size** 1.15× the word mark's font size.
- **Gap** 0.3× the font size between the tile and the first letter.
- **Clear space** at least half the icon's height on every side.
- **Minimum size** 18 px font size (about a 21 px icon). Below that, use the icon alone.
- **On a dark ground** the tile still shows; keep the ground darker than `#1B1730` or lighter than mid-grey so its edge reads.
- The icon alone is fine anywhere. The word mark alone is fine where the icon is already on screen (the setup hero beside the M3 shapes, an About dialog under the icon).


## Not done yet

- **Outlined SVGs.** The word mark and lockup should ship as outlined paths beside `desktop/build/icon.svg`, so no surface has to load Unbounded just to draw the name. Until then, anywhere that sets it as live text has to bundle the font.
- **The desktop setup screen** still sets the name in the system font at weight 800 (`.setup .brand` in `desktop/src/renderer/styles/m3-pages.css`), so it looks different on each platform. It should move to the word mark.
- **Android** has no word mark use yet.
