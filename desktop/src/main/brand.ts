// The Crescendo mark, rasterised at runtime for the tray and the
// dev-build dock icon, so no binary assets ship. Packaged builds take their app
// icon from build/ (see scripts/gen-app-icons.mjs).
import { encodePng, type Rgb } from "./png";

type Point = readonly [number, number];
type Cubic = readonly [Point, Point, Point, Point];

/** The four notes on the mark's 100-unit tile, in drawing order; voices alternate 1, 2, 1, 2. */
const NOTES: readonly Cubic[] = [
  [[16, 48.5], [23, 37.81], [26, 37.81], [33, 48.5]],
  [[33, 48.5], [40, 63.2], [43, 63.2], [50, 48.5]],
  [[50, 48.5], [57, 29.8], [60, 29.8], [67, 48.5]],
  [[67, 48.5], [74, 71.21], [77, 71.21], [84, 48.5]],
];
const STROKE = 12;
/** The wave's extent on the tile, round caps included. */
const BOUNDS = { x0: 10, y0: 28.4, x1: 90, y1: 71.6 };

const hex = (h: string): Rgb => ({ r: parseInt(h.slice(1, 3), 16), g: parseInt(h.slice(3, 5), 16), b: parseInt(h.slice(5, 7), 16) });
/** Tonal Violet, dark tile. */
const VOICE: readonly [Rgb, Rgb] = [hex("#E4DFFF"), hex("#8F80F5")];
const BODY: readonly [Rgb, Rgb] = [hex("#2A2447"), hex("#1B1730")];

/** Flatten a cubic into a polyline (32 segments keeps it within a hundredth of a unit). */
function flatten([p0, p1, p2, p3]: Cubic, n = 32): Point[] {
  const out: Point[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const u = 1 - t;
    const a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t;
    out.push([a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]]);
  }
  return out;
}
const POLYLINES = NOTES.map((n) => flatten(n));

function distToPolyline(x: number, y: number, pts: readonly Point[]): number {
  let best = Infinity;
  for (let i = 1; i < pts.length; i++) {
    const [ax, ay] = pts[i - 1] as Point;
    const [bx, by] = pts[i] as Point;
    const dx = bx - ax, dy = by - ay;
    const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)));
    best = Math.min(best, Math.hypot(x - ax - t * dx, y - ay - t * dy));
  }
  return best;
}

class Canvas {
  readonly px: Uint8Array;
  constructor(readonly w: number, readonly h: number) {
    this.px = new Uint8Array(w * h * 4);
  }

  /** Source-over blend of `c` at coverage `a` (0..1). */
  blend(x: number, y: number, c: Rgb, a: number): void {
    if (a <= 0) return;
    const i = (y * this.w + x) * 4;
    const da = (this.px[i + 3] as number) / 255;
    const oa = a + da * (1 - a);
    const mix = (s: number, d: number) => (s * a + d * da * (1 - a)) / oa;
    this.px[i] = mix(c.r, this.px[i] as number);
    this.px[i + 1] = mix(c.g, this.px[i + 1] as number);
    this.px[i + 2] = mix(c.b, this.px[i + 2] as number);
    this.px[i + 3] = oa * 255;
  }

  /**
   * The wave, mapped from tile units to pixels by `scale` and offset (ox, oy).
   * Notes are painted in order so each overlaps the one before it.
   */
  wave(ox: number, oy: number, scale: number, colour: (voice: 0 | 1) => Rgb): void {
    const half = (STROKE / 2) * scale;
    POLYLINES.forEach((pts, n) => {
      const scaled = pts.map(([x, y]): Point => [ox + x * scale, oy + y * scale]);
      const xs = scaled.map((p) => p[0]), ys = scaled.map((p) => p[1]);
      const x0 = Math.max(0, Math.floor(Math.min(...xs) - half - 1)), x1 = Math.min(this.w - 1, Math.ceil(Math.max(...xs) + half + 1));
      const y0 = Math.max(0, Math.floor(Math.min(...ys) - half - 1)), y1 = Math.min(this.h - 1, Math.ceil(Math.max(...ys) + half + 1));
      const c = colour((n % 2) as 0 | 1);
      for (let y = y0; y <= y1; y++)
        for (let x = x0; x <= x1; x++) this.blend(x, y, c, Math.min(1, half + 0.5 - distToPolyline(x + 0.5, y + 0.5, scaled)));
    });
  }

  /** A rounded square with the vertical body gradient. */
  tile(x0: number, size: number, radius: number): void {
    for (let y = 0; y < this.h; y++) {
      const t = Math.min(1, Math.max(0, (y - x0) / size));
      const c = { r: BODY[0].r + (BODY[1].r - BODY[0].r) * t, g: BODY[0].g + (BODY[1].g - BODY[0].g) * t, b: BODY[0].b + (BODY[1].b - BODY[0].b) * t };
      for (let x = 0; x < this.w; x++) {
        // signed distance to the rounded rect, sampled at the pixel centre
        const qx = Math.abs(x + 0.5 - (x0 + size / 2)) - (size / 2 - radius);
        const qy = Math.abs(y + 0.5 - (x0 + size / 2)) - (size / 2 - radius);
        const d = Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - radius;
        this.blend(x, y, c, Math.min(1, 0.5 - d));
      }
    }
  }

  png(): Buffer {
    return encodePng(this.w, this.h, this.px);
  }
}

/**
 * The app icon: the mark on its dark tile. `inset` is the margin around the tile as a
 * fraction of the size: Apple's grid (an 824 px body on 1024) for the Dock, nearly full
 * bleed for small tray icons.
 */
export function appIconPng(size: number, inset = 100 / 1024): Buffer {
  const c = new Canvas(size, size);
  const x0 = size * inset;
  const body = size - 2 * x0;
  c.tile(x0, body, body * (185 / 824));
  c.wave(x0, x0, body / 100, (v) => VOICE[v]);
  return c.png();
}

/**
 * The bare wave in black (the macOS menu-bar template image): `height` pt tall with a
 * 1 pt margin, as wide as the wave needs, rendered at `scaleFactor`. `width` is in points,
 * so every scale factor of one icon has the same shape.
 */
export function waveGlyphPng(height: number, scaleFactor = 1): { png: Buffer; width: number } {
  const scale = ((height - 2) / (BOUNDS.y1 - BOUNDS.y0)) * scaleFactor;
  const width = Math.ceil(((BOUNDS.x1 - BOUNDS.x0) * scale) / scaleFactor + 2);
  const c = new Canvas(width * scaleFactor, height * scaleFactor);
  const ox = (c.w - (BOUNDS.x1 - BOUNDS.x0) * scale) / 2 - BOUNDS.x0 * scale;
  const oy = (c.h - (BOUNDS.y1 - BOUNDS.y0) * scale) / 2 - BOUNDS.y0 * scale;
  c.wave(ox, oy, scale, () => ({ r: 0, g: 0, b: 0 }));
  return { png: c.png(), width };
}
