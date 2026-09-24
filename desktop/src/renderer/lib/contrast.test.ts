import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { BLACK, THEME_COLOURS, WHITE, accentTokens, bestTextOn, brightestPixel, composite, contrast, mix, parseColor, scrimAlpha, themeSurfaces, toHex, type Rgb, type ThemeName } from "./contrast";

const P = (s: string) => parseColor(s) as Rgb;
const css = readFileSync(fileURLToPath(new URL("../styles/global.css", import.meta.url)), "utf8");

/** Custom properties declared in the first block whose selector starts with `selector`. */
function tokens(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  const block = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  return Object.fromEntries([...block.matchAll(/--([\w-]+):\s*([^;]+);/g)].map((m) => [m[1] as string, (m[2] as string).trim()]));
}
const CSS_THEMES: Record<ThemeName, Record<string, string>> = { light: tokens(':root, :root[data-theme="light"]'), dark: tokens(':root[data-theme="dark"]') };

function hsl(h: number, s: number, l: number): string {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return toHex({ r: f(0) * 255, g: f(8) * 255, b: f(4) * 255 });
}
/** A sweep of artwork-like accents: every 15° of hue, dull to vivid, near-black to near-white. */
const ACCENTS = [...Array.from({ length: 24 }, (_, i) => i * 15).flatMap((h) => [0.35, 1].flatMap((s) => [0.05, 0.2, 0.35, 0.5, 0.65, 0.8, 0.95].map((l) => hsl(h, s, l)))), "#000000", "#ffffff", "#808080", "#6f5cff"];

describe("WCAG contrast maths", () => {
  it("matches the reference ratios", () => {
    expect(contrast(WHITE, BLACK)).toBeCloseTo(21, 5);
    expect(contrast(P("#777777"), WHITE)).toBeCloseTo(4.48, 2);
    expect(contrast(P("#767676"), WHITE)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(P("#6f5cff"), P("#6f5cff"))).toBe(1);
  });

  it("parses hex and rgb() colours and composites alpha in sRGB", () => {
    expect(P("#abc")).toEqual({ r: 0xaa, g: 0xbb, b: 0xcc });
    expect(P("#6f5cffcc")).toEqual({ r: 0x6f, g: 0x5c, b: 0xff });
    expect(P("rgba(10, 20, 30, 0.5)")).toEqual({ r: 10, g: 20, b: 30 });
    expect(parseColor("nonsense")).toBeUndefined();
    expect(toHex(composite(BLACK, 0.5, WHITE))).toBe("#808080");
    expect(toHex(mix(P("#000000"), P("#ffffff"), 0.25))).toBe("#404040");
  });

  it("puts black or white on any fill at 4.5:1, preferring white", () => {
    for (const a of ACCENTS) expect(contrast(bestTextOn(P(a)), P(a)), a).toBeGreaterThanOrEqual(4.5);
    expect(bestTextOn(P("#6f5cff"))).toEqual(WHITE);
    expect(bestTextOn(P("#ffff00"))).toEqual(BLACK);
  });
});

describe("theme tokens in global.css", () => {
  for (const theme of ["light", "dark"] as ThemeName[]) {
    const c = CSS_THEMES[theme];
    it(`${theme}: THEME_COLOURS mirrors the stylesheet`, () => {
      const t = THEME_COLOURS[theme];
      expect({ bg: t.bg, elev: t.elev, sunken: t.sunken, fg: t.fg, muted: t.muted, faint: t.faint }).toEqual({ bg: c.bg, elev: c["bg-elev"], sunken: c["bg-sunken"], fg: c.fg, muted: c["fg-muted"], faint: c["fg-faint"] });
      expect(c.selection).toContain(`${Math.round(t.selectionPct * 100)}%`);
    });

    it(`${theme}: body text, muted text and faint text reach 4.5:1 on every surface`, () => {
      for (const fg of ["fg", "fg-muted", "fg-faint"]) {
        for (const bg of ["bg", "bg-elev", "bg-sunken"]) expect(contrast(P(c[fg] as string), P(c[bg] as string)), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    });

    it(`${theme}: status colours read at 4.5:1 as text on the surfaces and on their own 14% badge tint`, () => {
      for (const status of ["danger", "warn", "ok"]) {
        const col = P(c[status] as string);
        for (const bg of ["bg", "bg-elev", "bg-sunken"]) {
          const surface = P(c[bg] as string);
          expect(contrast(col, surface), `${status} on ${bg}`).toBeGreaterThanOrEqual(4.5);
          expect(contrast(col, mix(surface, col, 0.14)), `${status} badge on ${bg}`).toBeGreaterThanOrEqual(4.5);
        }
        expect(contrast(P(c["danger-fg"] as string), P(c.danger as string)), "text on a danger fill").toBeGreaterThanOrEqual(4.5);
      }
    });

    it(`${theme}: the stylesheet's default accent tokens are what accentTokens computes`, () => {
      const tk = accentTokens(c.accent ?? "#6f5cff", theme);
      expect({ text: c["accent-text"], inverse: c["accent-inverse"], tint: c["accent-tint"], ring: c["focus-ring"] }).toEqual({ text: tk.accentText, inverse: tk.accentInverse, tint: tk.accentTint, ring: tk.focusRing });
    });
  }
});

describe("adaptive accent tokens for any artwork colour", () => {
  for (const theme of ["light", "dark"] as ThemeName[]) {
    it(`${theme}: every derived colour meets its contrast target for ${ACCENTS.length} accents`, () => {
      const t = THEME_COLOURS[theme];
      for (const raw of ACCENTS) {
        const tk = accentTokens(raw, theme);
        const surfaces = themeSurfaces(theme, P(tk.accentTint));
        const plain = themeSurfaces(theme);
        expect(contrast(P(tk.accentFg), P(tk.accent)), `${raw} fill text`).toBeGreaterThanOrEqual(4.5);
        for (const s of surfaces) {
          expect(contrast(P(tk.accentText), s), `${raw} accent text`).toBeGreaterThanOrEqual(4.5);
          expect(contrast(P(t.faint), s), `${raw} faint text on tint`).toBeGreaterThanOrEqual(4.5);
          expect(contrast(P(t.muted), s), `${raw} muted text on tint`).toBeGreaterThanOrEqual(4.5);
          expect(contrast(P(t.fg), s), `${raw} text on tint`).toBeGreaterThanOrEqual(4.5);
        }
        for (const s of plain) expect(contrast(P(tk.focusRing), s), `${raw} focus ring`).toBeGreaterThanOrEqual(3);
        expect(contrast(P(tk.accentInverse), P(t.fg)), `${raw} toast action`).toBeGreaterThanOrEqual(4.5);
      }
    });
  }

  it("leaves an accent that already works untouched", () => {
    expect(accentTokens("#6f5cff", "light").accent).toBe("#6f5cff");
    expect(accentTokens("#6f5cff", "light").focusRing).toBe("#6f5cff");
  });
});

describe("fullscreen scrim", () => {
  it("keeps white and 80% white text at 4.5:1 over the brightest cover colour", () => {
    for (const c of [WHITE, P("#ffff00"), P("#00ffff"), P("#ff8080"), P("#808080"), P("#202030"), BLACK]) {
      for (const alpha of [1, 0.8]) {
        const a = scrimAlpha(c, alpha);
        const bg = composite(BLACK, a, c);
        expect(contrast(composite(WHITE, alpha, bg), bg), `${toHex(c)} @ ${alpha}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("stays at the light 0.28 look on dark covers and darkens bright ones", () => {
    expect(scrimAlpha(P("#202030"), 0.8)).toBe(0.28);
    expect(scrimAlpha(WHITE, 0.8)).toBeGreaterThan(0.55);
    expect(scrimAlpha(WHITE, 0.8)).toBeLessThan(0.9);
  });

  it("finds the brightest pixel of RGBA data", () => {
    const px = new Uint8ClampedArray([10, 10, 10, 255, 250, 240, 10, 255, 0, 0, 255, 255]);
    expect(brightestPixel(px)).toEqual({ r: 250, g: 240, b: 10 });
  });
});
