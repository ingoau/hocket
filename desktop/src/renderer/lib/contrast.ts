// WCAG contrast maths and the adaptive colour tokens built on it. The dynamic
// accent comes from arbitrary artwork, so every colour derived from it is
// checked here rather than trusted: text drawn in the accent is pushed until
// it reaches 4.5:1 on every surface it can sit on, the text on accent fills is
// black or white (whichever contrasts more; one of them always reaches 4.58:1),
// the focus ring reaches 3:1 (non-text contrast), and the tint behind selected
// and current rows is kept light (or dark) enough for muted text on top of it.
// The fullscreen player's scrim is sized from the artwork's brightest pixel so
// white text over the fluid background always reaches 4.5:1.

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export const WHITE: Rgb = { r: 255, g: 255, b: 255 };
export const BLACK: Rgb = { r: 0, g: 0, b: 0 };

/** `#rgb`, `#rrggbb`, `#rrggbbaa` (alpha ignored), `rgb()` / `rgba()`. */
export function parseColor(input: string): Rgb | undefined {
  const s = input.trim().toLowerCase();
  let m = /^#([0-9a-f]{3})$/.exec(s);
  if (m) {
    const [r, g, b] = (m[1] as string).split("").map((c) => parseInt(c + c, 16)) as [number, number, number];
    return { r, g, b };
  }
  m = /^#([0-9a-f]{6})(?:[0-9a-f]{2})?$/.exec(s);
  if (m) {
    const h = m[1] as string;
    return { r: parseInt(h.slice(0, 2), 16), g: parseInt(h.slice(2, 4), 16), b: parseInt(h.slice(4, 6), 16) };
  }
  m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/.exec(s);
  if (m) return { r: Number(m[1]), g: Number(m[2]), b: Number(m[3]) };
  return undefined;
}

/** Whole-number channels: what a hex token will actually be (checks run on this, not the float). */
export function roundRgb(c: Rgb): Rgb {
  const r = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  return { r: r(c.r), g: r(c.g), b: r(c.b) };
}

export function toHex(c: Rgb): string {
  const h = (v: number) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

function channel(v: number): number {
  const c = v / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/** WCAG 2 relative luminance (0 black … 1 white). */
export function luminance(c: Rgb): number {
  return 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
}

/** WCAG 2 contrast ratio (1 … 21). */
export function contrast(a: Rgb, b: Rgb): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** `color-mix(in srgb, a, b t)`: `t` is the fraction of `b`. */
export function mix(a: Rgb, b: Rgb, t: number): Rgb {
  return { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t };
}

/** `fg` at `alpha` painted over an opaque `bg` (what the compositor does, in sRGB). */
export function composite(fg: Rgb, alpha: number, bg: Rgb): Rgb {
  return mix(bg, fg, alpha);
}

/**
 * Text colour for an accent fill: white while it reaches 4.5:1 (the design's
 * default), otherwise black or white, whichever contrasts more (for any colour
 * one of the two reaches at least 4.58:1).
 */
export function bestTextOn(bg: Rgb): Rgb {
  if (contrast(WHITE, bg) >= 4.5) return WHITE;
  return contrast(WHITE, bg) >= contrast(BLACK, bg) ? WHITE : BLACK;
}

function minContrast(c: Rgb, backgrounds: readonly Rgb[]): number {
  return Math.min(...backgrounds.map((b) => contrast(c, b)));
}

/**
 * Moves `color` toward `toward` in small steps until it reaches `target`
 * against every background. Returns the first colour that does (or `toward`
 * itself, which callers pick as the theme's text colour, so it always does).
 */
export function ensureContrast(color: Rgb, backgrounds: readonly Rgb[], target: number, toward: Rgb): Rgb {
  for (let t = 0; t <= 1.0001; t += 0.02) {
    const c = roundRgb(mix(color, toward, t));
    if (minContrast(c, backgrounds) >= target) return c;
  }
  return toward;
}

export type ThemeName = "light" | "dark";

/** The theme surfaces and text colours; mirrored from global.css (contrast.test.ts checks they match). */
export const THEME_COLOURS: Record<ThemeName, { bg: string; elev: string; sunken: string; fg: string; muted: string; faint: string; softPct: number; selectionPct: number }> = {
  light: { bg: "#f4f4f6", elev: "#ffffff", sunken: "#ebebef", fg: "#1b1b1f", muted: "#51515a", faint: "#5b5b64", softPct: 0.18, selectionPct: 0.22 },
  dark: { bg: "#121214", elev: "#1c1c20", sunken: "#0d0d0f", fg: "#ececf1", muted: "#acacb5", faint: "#9f9fa8", softPct: 0.18, selectionPct: 0.3 },
};

export interface AccentTokens {
  /** Fills (primary buttons, the active menu item, the seek fill): the accent as extracted. */
  accent: string;
  /** Text and icons drawn on an accent fill. */
  accentFg: string;
  /** Text drawn in the accent colour on the theme's surfaces and tints. */
  accentText: string;
  /** Text drawn in the accent colour on the inverse surface (toasts: `--fg` background). */
  accentInverse: string;
  /** Base of the selection / current-row tints (kept light in light theme, dark in dark). */
  accentTint: string;
  /** Keyboard focus ring: at least 3:1 against every surface. */
  focusRing: string;
}

/** Every surface a token may sit on: the three theme surfaces and the accent tints over each. */
export function themeSurfaces(theme: ThemeName, tint?: Rgb): Rgb[] {
  const c = THEME_COLOURS[theme];
  const base = [c.bg, c.elev, c.sunken].map((x) => parseColor(x) as Rgb);
  if (!tint) return base;
  const pcts = [c.softPct, c.selectionPct];
  return [...base, ...base.flatMap((s) => pcts.map((p) => mix(s, tint, p)))];
}

export function accentTokens(raw: string, theme: ThemeName): AccentTokens {
  const c = THEME_COLOURS[theme];
  const accent = parseColor(raw) ?? (parseColor("#6f5cff") as Rgb);
  const fg = parseColor(c.fg) as Rgb;
  const faint = parseColor(c.faint) as Rgb;
  const bg = parseColor(c.bg) as Rgb;
  const plain = themeSurfaces(theme);
  // The tint base: move toward the theme background until the faintest text
  // colour still reads at 4.5:1 on the strongest tint over every surface.
  let tint = accent;
  for (let t = 0; t <= 1.0001; t += 0.02) {
    tint = roundRgb(mix(accent, bg, t));
    // Checked against the tints as the browser will blend them, with a hair of margin for its rounding.
    if (minContrast(faint, themeSurfaces(theme, tint).map(roundRgb)) >= 4.55) break;
  }
  const surfaces = themeSurfaces(theme, tint).map(roundRgb);
  return {
    accent: toHex(accent),
    accentFg: toHex(bestTextOn(accent)),
    // A hair above 4.5 / 3 so the browser's own rounding of the color-mix() tints can't tip it under.
    accentText: toHex(ensureContrast(accent, surfaces, 4.55, fg)),
    accentInverse: toHex(ensureContrast(accent, [fg], 4.55, bg)),
    accentTint: toHex(tint),
    focusRing: toHex(ensureContrast(accent, plain, 3.05, fg)),
  };
}

/**
 * Opacity of the black scrim over the fullscreen background so text in
 * `textAlpha` white reaches `target` even over the artwork's brightest colour.
 * Never below `min` (the look on dark covers), never above 0.9.
 */
export function scrimAlpha(brightest: Rgb, textAlpha: number, target = 4.5, min = 0.28): number {
  for (let a = min; a < 0.9; a += 0.01) {
    const bg = composite(BLACK, a, brightest);
    if (contrast(composite(WHITE, textAlpha, bg), bg) >= target) return Math.round(a * 100) / 100;
  }
  return 0.9;
}

/** The highest-luminance pixel of RGBA data (the worst case for white text). */
export function brightestPixel(data: Uint8ClampedArray | Uint8Array): Rgb {
  let best: Rgb = BLACK;
  let bestL = -1;
  for (let i = 0; i + 2 < data.length; i += 4) {
    const c = { r: data[i] as number, g: data[i + 1] as number, b: data[i + 2] as number };
    const l = luminance(c);
    if (l > bestL) {
      bestL = l;
      best = c;
    }
  }
  return best;
}
