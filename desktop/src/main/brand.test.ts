import { inflateSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { appIconPng, waveGlyphPng } from "./brand";

/** Decode the encoder's own output: one IDAT, filter 0 on every row. */
function decode(png: Buffer): { w: number; h: number; at: (x: number, y: number) => number[] } {
  const w = png.readUInt32BE(16);
  const h = png.readUInt32BE(20);
  const raw = inflateSync(png.subarray(41, 41 + png.readUInt32BE(33)));
  return { w, h, at: (x, y) => [...raw.subarray(y * (w * 4 + 1) + 1 + x * 4, y * (w * 4 + 1) + 5 + x * 4)] };
}

describe("brand mark", () => {
  it("draws the app icon on Apple's grid", () => {
    const img = decode(appIconPng(100));
    expect([img.w, img.h]).toEqual([100, 100]);
    expect(img.at(2, 2)[3]).toBe(0); // outside the tile
    expect(img.at(50, 85)).toEqual([expect.any(Number), expect.any(Number), expect.any(Number), 255]); // tile body
    // Voice one on the first hump's crest (tile 24.5, 40.5), voice two in the last dip (75.5, 65.5).
    const at = (u: number, v: number) => img.at(Math.floor(9.77 + u * 0.805), Math.floor(9.77 + v * 0.805));
    expect(at(24.5, 40.5).slice(0, 3)).toEqual([0xe4, 0xdf, 0xff]);
    expect(at(75.5, 65.5).slice(0, 3)).toEqual([0x8f, 0x80, 0xf5]);
  });

  it("renders the tray glyph at the same shape for every scale factor", () => {
    const one = waveGlyphPng(14, 1);
    const two = waveGlyphPng(14, 2);
    expect(one.width).toBe(two.width);
    const a = decode(one.png);
    const b = decode(two.png);
    expect([a.w, a.h]).toEqual([one.width, 14]);
    expect([b.w, b.h]).toEqual([one.width * 2, 28]);
    // A template image: black, shaped by alpha alone.
    let ink = 0;
    for (let y = 0; y < b.h; y++)
      for (let x = 0; x < b.w; x++) {
        const [r, g, bl, al] = b.at(x, y) as [number, number, number, number];
        if (al > 0) {
          ink++;
          expect([r, g, bl]).toEqual([0, 0, 0]);
        }
      }
    expect(ink).toBeGreaterThan(b.w * b.h * 0.2);
  });
});
