import { describe, expect, it } from "vitest";
import type { ArtworkEdge } from "@core/api";
import { composite, contrast, WHITE } from "./contrast";
import { scrimFor, toneForText, withTone } from "./immersive";

const edge = (colors: number[], light: boolean, scrim = 0): ArtworkEdge => ({
  style: "extend",
  edgeColors: colors,
  baseColor: colors[0]!,
  light,
  scrim,
  reason: "extend:plain",
  metrics: { flat: 1, drift: 0, rough: 0, marks: 0, busy: 0, faces: 0 },
});
const rgb = (c: number) => ({ r: (c >> 16) & 255, g: (c >> 8) & 255, b: c & 255 });

describe("the scrim for the other tone", () => {
  it("is none where the text already reaches 4.5:1", () => {
    expect(scrimFor(edge([0x101418], false), false)).toBe(0);
    expect(scrimFor(edge([0xf4f0e8], true), true)).toBe(0);
  });

  it("is enough for white text over a mid-tone, and more for muted text", () => {
    const mid = 0x8a8f96;
    const a = scrimFor(edge([mid], false), false);
    expect(a).toBeGreaterThan(0);
    expect(contrast(WHITE, composite({ r: 0, g: 0, b: 0 }, a, rgb(mid)))).toBeGreaterThanOrEqual(4.5);
    expect(scrimFor(edge([mid], false), false, 0.8)).toBeGreaterThan(a);
  });

  it("keeps the core's own scrim when the tone is unchanged, and the same object for the same choice", () => {
    const e = edge([0x8a8f96], false, 0.9);
    expect(withTone(e, false)).toBe(e);
    const muted = withTone(e, false, 0.8);
    expect(muted.scrim).toBe(0.9);
    expect(withTone(e, false, 0.8)).toBe(muted);
    const flipped = withTone(e, true);
    expect(flipped.light).toBe(true);
    expect(flipped).toBe(withTone(e, true));
  });
});

describe("a reflection's end colour, toned for the text over it", () => {
  const dark = { r: 0x1c, g: 0x1b, b: 0x1f };
  it("darkens under light text and lightens under dark text until muted text reaches 4.5:1, keeping the hue", () => {
    const purple = { r: 150, g: 110, b: 190 };
    const under = toneForText(purple, false, 0.8);
    expect(contrast(composite(WHITE, 0.8, under), under)).toBeGreaterThanOrEqual(4.5);
    expect(under.b).toBeGreaterThan(under.r);
    expect(under.r).toBeGreaterThan(under.g);
    const over = toneForText(purple, true, 0.8);
    expect(contrast(composite(dark, 0.8, over), over)).toBeGreaterThanOrEqual(4.5);
    expect(over.b).toBeGreaterThan(over.g);
  });

  it("leaves a colour that already reads alone", () => {
    expect(toneForText({ r: 20, g: 24, b: 40 }, false, 0.8)).toEqual({ r: 20, g: 24, b: 40 });
  });
});
