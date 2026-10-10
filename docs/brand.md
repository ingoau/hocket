# Brand

The Hocket logo: what it is, what it means, how it was chosen, and how to use it.
Decisions are recorded with the reasons behind them, in the spirit of `design.md`.

## The idea

*Hocket* is a medieval technique where one melody is split between two or more
voices, each supplying alternate notes. The line is continuous; no single
performer plays all of it. The word comes from the Old French *hoquet*,
"hiccup", after the stop-start sound.

The app works the same way: playback moves between devices, and the music
should keep going when the network doesn't. So the logo is **one continuous line
played by two alternating voices**.

## The mark: Crescendo

A wave of four half-cycles. The voices alternate: the 1st and 3rd humps are
voice one, the 2nd and 4th dips are voice two. The wave swells from left to
right, like a phrase building, which makes it read as music rather than a
signal or waveform.

- **Geometry.** Drawn on a 100-unit tile. Each half-cycle is 17 units wide; the
  amplitudes grow 8 → 11 → 14 → 17; the baseline sits at y = 48.5 so the whole
  wave is optically centred. Each half-cycle is a cubic Bézier fitted to a sine
  arc (max error 0.3% of the amplitude).

  ```svg
  <g fill="none" stroke-width="12" stroke-linecap="round">
    <path stroke="VOICE_1" d="M16 48.5 C23 37.81 26 37.81 33 48.5"/>
    <path stroke="VOICE_2" d="M33 48.5 C40 63.2 43 63.2 50 48.5"/>
    <path stroke="VOICE_1" d="M50 48.5 C57 29.8 60 29.8 67 48.5"/>
    <path stroke="VOICE_2" d="M67 48.5 C74 71.21 77 71.21 84 48.5"/>
  </g>
  ```

- **Stroke 12, at every size.** Six weights (6, 7.5, 9, 10.5, 12, 14) were
  compared from 160 px down to 16 px. 12 is the lightest that still reads at
  16 px and on the light tile; 14 clogs the humps. One weight everywhere means
  one master. At very large sizes 12 is slightly heavy and the first hump
  narrows; if that ever matters, raise only the first hump's amplitude.
- **Round joins.** Every segment has round caps and is drawn in order, so each
  voice overlaps the one before it with a rounded end. Butt-capped segments left
  hairline anti-aliasing seams at the hand-offs; overlapping removes them.

## Colour: Tonal Violet

Two tones of one violet. The voices differ in **lightness**, not just hue, so
the hand-off survives at small sizes, in greyscale and for colour-blind viewers.

| Use | Background | Voice 1 | Voice 2 |
|---|---|---|---|
| Dark tile (default) | `#221D3A` | `#E4DFFF` | `#8F80F5` |
| Light tile | `#E4DFFF` | `#2E2470` | `#7F6FF0` |

Lightness contrast between the two voices (WCAG ratio; 1.0 = identical):

| Palette | Voice contrast |
|---|---|
| **Tonal Violet, dark** | **2.48** |
| **Tonal Violet, light** | **3.39** |
| Lavender & Mint (the old placeholder) | 1.22 |
| Night & Cyan | 1.15 |
| Material Violet | 1.00 |
| Violet & Amber | 1.00 |

Every other palette tried separates its voices by hue alone.

The app's default accent is `#6F5CFF`; Tonal Violet sits in the same hue.

## Wordmark

Lowercase **hocket** set in **Nunito Extra Bold (800)**, letter-spacing −0.025em,
in the mark's ink colour: `#2E2470` on light, `#E4DFFF` on dark.

Nunito's rounded terminals match the mark's round caps, and its stroke weight is
closest to the stroke-12 wave, so mark and word read as one unit. Nunito is OFL;
for shipped artwork the wordmark is converted to outlines so nothing depends on
the font being installed.

Lockups: horizontal (mark left of the word, mark about 1.6× the cap height) and
stacked (mark centred over the word).

## Material You

The Android app uses dynamic colour on Android 12+. When the icon or mark is
themed from a wallpaper seed (`SchemeTonalSpot`), the two voices map to
**primary** and **tertiary**: tertiary is Material's built-in contrasting
accent, so the two-voice idea falls straight out of the scheme. Dark tile =
`surfaceContainerHigh`, light tile = `primaryContainer`.

Caveat: in light schemes primary and tertiary land at the same tone (T40), so
they separate by hue only. Use primary T30 and tertiary T50–T60 there.

Android 13+ themed icons are single-colour; the mark still reads as a wave, but
the voice split is lost. That is accepted.

## Platform assets

| Platform | Asset | Status |
|---|---|---|
| macOS | `desktop/build/icon.svg` — master on Apple's grid (824 px body on a 1024 px canvas, radius 185, soft drop shadow, subtle vertical gradient around `#221D3A`) | Done |
| macOS | `desktop/build/icon.icns` — flat icon, rendered from the master | To do |
| macOS 26+ | `desktop/build/icon.icon` — Liquid Glass icon from Icon Composer | To do (needs a Mac) |
| Windows | `.ico` at 16–256 | To do |
| Linux | PNGs for AppImage/deb, MPRIS | To do |
| Desktop | Tray icon, single colour | To do |
| Android | Adaptive icon foreground/background, monochrome layer, notification icon, splash icon | To do |
| Web | Favicon, README header | To do |

### macOS and Liquid Glass

An `.icns` is a flat bitmap: macOS 26 shows it as-is, with no glass,
highlights or dark/clear/tinted variants. Liquid Glass needs an `.icon` file
made in **Icon Composer**, which is layered so the system can render the glass
live. It can only be authored and previewed there.

electron-builder (26.15.3, as locked) supports `mac.icon: build/icon.icon`. It
compiles the file into `Assets.car` with `actool`, and still bundles
`build/icon.icns` for macOS 15 and earlier. The catch: release builds for macOS
then need Xcode 26 on the build machine.

Plan: export the two voices as separate layer SVGs (so each voice becomes its
own glass layer), assemble them in Icon Composer with a `#221D3A` background,
save as `desktop/build/icon.icon`, then set `mac.icon` in `electron-builder.yml`.

## How we got here

The full exploration lives on a design canvas (private; ask the owner for
access): <https://claude.ai/artifact/QBnzNvD83HpFQ7LFUP7Zi8>

1. **Three directions**, all in the original lavender `#B5A7F5` and mint
   `#7DD3C0` on `#1B1B22` (from the placeholder Android icon):
   - **A, Relay:** a wave whose humps and dips alternate colour.
   - **B, Handoff h:** a lowercase h whose stem and shoulder are different
     voices, with a gap where they hand off.
   - **C, Shared beam:** two beamed eighth notes, one per voice.
2. **Does it say "hocket"?** Only loosely: all three leaned on the colour split.
   C was musically wrong (beamed notes are one voice). Stronger readings would
   be interlocking voices (each fills the other's rests), the *hoquet* hiccup,
   or playback handing between devices.
3. **Material You.** Each direction recoloured from real dynamic schemes for
   five wallpaper seeds. **A in Violet** was the favourite.
4. **Eight Relay variations:** Original, Heavy, Broad, Crescendo, Hiccup (gaps
   at the hand-offs), Vibrato, Bleed (runs off the tile edges), Loop (closed
   ring). Bleed and Crescendo were the strongest; Hiccup's gaps vanish by 32 px;
   Loop read as a blob. The shortlist was Original, Broad and Crescendo, and
   **Crescendo** won: it keeps the alternation and adds a sense of a phrase.
5. **Eight colourways for Crescendo:** Material Violet, Lavender & Mint,
   Violet & Amber, Tonal Violet, Accent Tile, Coral & Violet, Night & Cyan, Ink.
   **Tonal Violet** was chosen; it also turned out to have the widest lightness
   gap between voices.
6. **Stroke weight:** 12, everywhere.
7. **Wordmark:** nine faces tried (Bricolage Grotesque, Fraunces Soft, Nunito,
   Fredoka, Quicksand, Outfit, Manrope, Young Serif, Instrument Serif italic).
   **Nunito** was chosen; Outfit was the runner-up, Instrument Serif the
   characterful alternative.
