// Dynamic accent from artwork: average of the most saturated bucket of a
// downscaled image, drawn on a canvas. Cheap (32x32) and cached per URL.
const cache = new Map<string, string | undefined>();

export async function extractAccent(url: string): Promise<string | undefined> {
  if (cache.has(url)) return cache.get(url);
  const p = (async () => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.decoding = "async";
    img.src = url;
    await img.decode().catch(() => undefined);
    if (!img.width) return undefined;
    const c = document.createElement("canvas");
    c.width = 32;
    c.height = 32;
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (!ctx) return undefined;
    ctx.drawImage(img, 0, 0, 32, 32);
    const { data } = ctx.getImageData(0, 0, 32, 32);
    return pickAccent(data);
  })();
  const v = await p;
  cache.set(url, v);
  return v;
}

/** Exposed for tests: choose a vivid, mid-lightness colour from RGBA pixels. */
export function pickAccent(data: Uint8ClampedArray | Uint8Array): string | undefined {
  const buckets = new Map<number, { r: number; g: number; b: number; n: number; w: number }>();
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i] as number;
    const g = data[i + 1] as number;
    const b = data[i + 2] as number;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const l = (max + min) / 510;
    const s = max === min ? 0 : (max - min) / (1 - Math.abs(2 * l - 1)) / 255;
    if (l < 0.15 || l > 0.9) continue;
    const key = ((r >> 5) << 6) | ((g >> 5) << 3) | (b >> 5);
    const w = s * s + 0.02;
    const bk = buckets.get(key) ?? { r: 0, g: 0, b: 0, n: 0, w: 0 };
    bk.r += r * w;
    bk.g += g * w;
    bk.b += b * w;
    bk.n += 1;
    bk.w += w;
    buckets.set(key, bk);
  }
  let best: { r: number; g: number; b: number; score: number } | undefined;
  for (const bk of buckets.values()) {
    const score = bk.w * Math.sqrt(bk.n);
    if (!best || score > best.score) best = { r: bk.r / bk.w, g: bk.g / bk.w, b: bk.b / bk.w, score };
  }
  if (!best) return undefined;
  // Lift very dark accents so text on them stays legible.
  const { r, g, b } = best;
  const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const k = lum < 70 ? 70 / Math.max(lum, 1) : 1;
  const hex = (v: number) => Math.min(255, Math.round(v * k)).toString(16).padStart(2, "0");
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}
