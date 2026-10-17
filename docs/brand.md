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
- **The tile** is a rounded square, corner radius 22.45% of its side (185 on Apple's 824 px body), in the tile colour (see Colour). Platforms that mask their own icon shape (Android) use the colour alone.
- **Never** redraw the wave by eye, recolour one note, or add a third voice. The bare wave in its two voices only goes on dark grounds; the pale voice disappears on light ones, so on a light ground use the tile, or the bare wave in one colour, the word mark's ink (only beside the word mark: see The header lockup).

Where it lives (the geometry is the same in each, and changes go to all of them):

- `desktop/build/icon.svg` — the flat master on Apple's grid.
- `desktop/build/icon.icon/` — the shipped macOS icon, edited in Icon Composer.
- `desktop/scripts/gen-app-icons.mjs` — renders `icon.icon`'s note layers, the Windows `.ico` and the Linux PNGs; the output is committed.
- `desktop/src/main/brand.ts` — rasterised at runtime for the tray and the dev-build dock icon.
- `android/app/src/main/res/drawable/ic_launcher_foreground.xml` — the adaptive icon's foreground and monochrome layer.
- `desktop/src/renderer/components/BrandMark.tsx` and `android/app/src/main/java/app/hocket/ui/brand/Wordmark.kt` — the bare wave, cropped to x 10–90, y 28.4–71.6, for the lockups.


## Colour: Tonal Violet  
*Status: decided*

| Role | Hex | Used for |
| --- | --- | --- |
| Voice 1 | `#E4DFFF` | Notes 1 and 3 |
| Voice 2 | `#8F80F5` | Notes 2 and 4 |
| Tile | `#221D3A` | The tile wherever it is one flat colour: the macOS icon's fill (`icon.icon`, which adds its own gradient), the Android launcher, splash and shortcut backgrounds |
| Tile gradient, top | `#2A2447` | Flat icons drawn with a gradient (`icon.svg`, `brand.ts`, `gen-app-icons.mjs`) |
| Tile gradient, bottom | `#1B1730` | As above |
| Shortcut glyph | `#D7CCFF` | Android launcher shortcut icons, on the tile |

The gradient stops sit either side of `#221D3A`, so a flat tile and a gradient one read as the same colour.

The word mark adds two inks, chosen with the word mark rather than taken from existing assets: `#ECE9FF` on dark grounds and `#1B1730` (the gradient's bottom stop) on light ones. It's always one colour. The voices colour the mark and nothing else; on light grounds the bare wave beside the word mark takes the ink too (see The header lockup).


## The word mark  
*Status: decided*

**Unbounded**, weight 500, lowercase: `hocket`.

- **Tracking** −0.03em at display sizes, −0.015em at 24 px and under.
- **Case** always lowercase in the word mark. In running text the name is still written *Hocket*.
- **Colour** one colour: `#ECE9FF` on dark, `#1B1730` on light. Never the voice colours, never split by letter or syllable; the mark carries the colour.
- **No effects** — no gradients, outlines, shadows, or baseline shifts.
- **Why Unbounded:** its wide, round letters match the mark's heavy stroke and the tile's rounded corners, and it sits with the Material 3 Expressive shapes in the app. It's wide, so the lockup needs horizontal room.
- **Licence** SIL Open Font License 1.1 (Google Fonts), so it can ship in both apps.
- **Bundled** in both, as live text, until the outlined SVGs exist (see Not done yet). Each ships the whole variable font with its licence beside it:
  - Desktop: `desktop/src/renderer/assets/fonts/unbounded.ttf`, licence `unbounded-OFL.txt` in the same folder, loaded by the `@font-face` in `desktop/src/renderer/styles/m3-pages.css` (served from the app, so the CSP's `font-src 'self'` holds).
  - Android: `android/app/src/main/res/font/unbounded.ttf`, licence in `android/app/src/main/assets/licenses/unbounded-OFL.txt`, with the weights mapped onto its axis.
- **Page titles** may be set in the word mark's type without being the word mark: Unbounded 500 in the title's own size and case (Library's title, "Library", on both apps). Android tracks these −0.015em, as a top-bar title is at most display size briefly; desktop −0.03em, at its large page-title size.


## The header lockup  
*Status: decided*

The app's name as a page header, inside the app: the bare wave (no tile), then the word mark, centred vertically on the line. The tile would be a picture of the app icon on a page that is already the app; the bare wave keeps the mark and drops the frame.

- **Wave height** 0.8× the word mark's font size (about the x-height and a little more), from cap to cap.
- **Gap** 0.3× the font size between the wave and the first letter.
- **Size** whatever the page title around it is: the lockup takes the title's font size, and the wave and gap follow.
- **Colour, dark grounds** the wave in its two voices (`#E4DFFF`, `#8F80F5`), the word in `#ECE9FF`.
- **Colour, light grounds** the wave and the word both in `#1B1730`, one colour. The pale voice vanishes on a light ground, and a wave with one note missing reads as a broken mark; one ink keeps all four notes and stays legible at title sizes. The voices never go on a light ground.
- **Accessible name** the app's name once, *Hocket*: the wave is decorative and the lowercase word is hidden in favour of the name in running-text case.

Where it's used:

- **Android Home** — the top bar's title (`HeaderLockup` in `android/app/src/main/java/app/hocket/ui/brand/Wordmark.kt`, used by `ui/screens/home/HomeScreen.kt`), at the collapsing bar's title size.
- **Desktop Home** — the page header (`h1.brand-lockup` in `desktop/src/renderer/views/Home.tsx`, the wave from `components/BrandMark.tsx`, styles in `styles/m3-pages.css`), at the Library title's size.
- **Desktop first-run setup** — the hero (`.setup .brand` in `desktop/src/renderer/views/Setup.tsx` and `m3-pages.css`), the same lockup at hero size, above the setup heading.
- **Android lyrics share card** — the footer (`Lockup` in `Wordmark.kt`) sets the bare wave and `hocket` in `#ECE9FF` alone, one colour, on the card's dark ground.


## The app-icon lockup  
*Status: open*

For outside the app (a website, a README, a store listing): the app icon (the tile, not the bare wave) to the left of the word mark, centred vertically on the line. The proportions below come from the lockup mockups and haven't been tested yet; neither app uses this lockup.

- **Icon size** 1.15× the word mark's font size.
- **Gap** 0.3× the font size between the tile and the first letter.
- **Clear space** at least half the icon's height on every side.
- **Minimum size** 18 px font size (about a 21 px icon). Below that, use the icon alone.
- **Ground** the tile's edge has to read: on dark, use a ground darker than `#1B1730`; any light ground works.
- The icon alone is fine anywhere. The word mark alone is fine where the icon is already nearby, such as an About dialog with the icon above the name.


## Not done yet

- **Outlined SVGs.** The word mark and both lockups should ship as outlined paths beside `desktop/build/icon.svg`, so no surface has to load Unbounded just to draw the name. Until then, anywhere that sets it as live text has to bundle the font (both apps do).
- **Subsetting.** Both apps bundle the whole variable Unbounded for a handful of weights and letters; a subset (Latin, weight 500) would be a fraction of the size.
- **The app-icon lockup** is untested; it waits for a surface outside the apps that needs it.
