// Immersive artwork (docs/design.md, "Immersive artwork"): the fullscreen
// player's artwork edge to edge, carried on past its edge by a reflection or by
// its own colours, or the usual card. The core decides which
// (hocket_core::artwork, over IPC); this side reads the cover's pixels from a
// canvas and draws the continuation, once per track and window size, as a
// still on a canvas: nothing redraws per frame.
import { useEffect, useState } from "react";
import type { ArtworkEdge, ArtworkLayout, ImmersiveArtwork } from "@core/api";
import { bridge } from "../core/bridge";
import { useArtwork } from "../components/Artwork";

/** What the classifier gets: the cover at this side (it resamples to 128 itself). */
const SIDE = 256;
const cache = new Map<string, ArtworkLayout | undefined>();

export function loadImage(url: string): Promise<HTMLImageElement | undefined> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.decoding = "async";
  img.src = url;
  return img.decode().then(() => (img.width ? img : undefined), () => undefined);
}

/** The layout for the cover at `url` (cached per URL and preference). */
export async function classify(url: string, preference: ImmersiveArtwork): Promise<ArtworkLayout | undefined> {
  const key = `${url}|${preference}`;
  if (cache.has(key)) return cache.get(key);
  const img = await loadImage(url);
  let layout: ArtworkLayout | undefined;
  if (img) {
    const scale = Math.min(1, SIDE / Math.max(img.width, img.height));
    const w = Math.max(1, Math.round(img.width * scale));
    const h = Math.max(1, Math.round(img.height * scale));
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (ctx) {
      ctx.drawImage(img, 0, 0, w, h);
      const data = ctx.getImageData(0, 0, w, h).data;
      // No face detection on the desktop yet: faces only rule a mirror out, so a portrait may mirror.
      layout = await bridge().artworkLayout(new Uint8Array(data.buffer, data.byteOffset, data.byteLength), w, h, { faces: [], preference }).catch(() => undefined);
    }
  }
  cache.set(key, layout);
  return layout;
}

/** The layout for `coverArt`: undefined while it is worked out, without a cover, or without the native core. */
export function useArtworkLayout(coverArt: string | undefined, preference: ImmersiveArtwork): ArtworkLayout | undefined {
  const url = useArtwork(coverArt, 300);
  const [layout, setLayout] = useState<ArtworkLayout | undefined>(undefined);
  useEffect(() => {
    setLayout(undefined);
    if (!url) return;
    let alive = true;
    void classify(url, preference).then((l) => alive && setLayout(l));
    return () => {
      alive = false;
    };
  }, [url, preference]);
  return layout;
}

export type Orientation = "bottom" | "right";

const hex = (c: number) => `#${(c & 0xffffff).toString(16).padStart(6, "0")}`;

/** `colors` averaged over a window of 2·radius + 1: the same edge, blurred sideways. */
export function soften(colors: number[], radius: number): number[] {
  return colors.map((_, i) => {
    const span = colors.slice(Math.max(0, i - radius), i + radius + 1);
    const ch = (shift: number) => Math.round(span.reduce((a, c) => a + ((c >> shift) & 255), 0) / span.length);
    return (ch(16) << 16) | (ch(8) << 8) | ch(0);
  });
}

/** One colour carried on: the artwork already ends in it, so no fade and no blur at the seam. */
export function isFlat(edge: ArtworkEdge): boolean {
  return edge.style === "extend" && new Set(edge.edgeColors).size <= 1;
}

/** The share of the artwork that fades into the continuation (the CSS mask on .np-art matches it). */
export const FEATHER = 0.18;
/** Blur at the seam and far from it, as shares of the artwork's side. */
const BLUR_NEAR = 0.025;
const BLUR_FAR = 0.067;

/**
 * The artwork with its reflection below it, padded on every side by reflection too, blurred by
 * `blur` px: what both sides of the seam are cut from. Blurring the two together keeps the seam
 * continuous, and the padding keeps the canvas edges from blurring into transparency.
 */
function mirrored(source: CanvasImageSource, W: number, S: number, blur: number): { canvas: HTMLCanvasElement; pad: number } {
  const pad = Math.ceil(blur * 3);
  const c = document.createElement("canvas");
  c.width = W + 2 * pad;
  c.height = 2 * S + pad;
  const ctx = c.getContext("2d");
  if (!ctx) return { canvas: c, pad };
  const tile = document.createElement("canvas");
  tile.width = c.width;
  tile.height = c.height;
  const t = tile.getContext("2d");
  if (t) {
    for (const [sx, dx] of [[1, pad], [-1, pad], [-1, pad + 2 * W]] as const) {
      // The artwork, then flipped below it; the side copies are mirrored horizontally.
      t.save();
      t.translate(dx, 0);
      t.scale(sx, 1);
      t.drawImage(source, 0, 0, W, S);
      t.translate(0, 2 * S);
      t.scale(1, -1);
      t.drawImage(source, 0, 0, W, S);
      t.restore();
    }
  }
  ctx.filter = blur > 0 ? `blur(${blur}px)` : "none";
  ctx.drawImage(tile, 0, 0);
  return { canvas: c, pad };
}

/**
 * Draws the continuation into `canvas` (sized to the player, in CSS px × dpr): for
 * "bottom", from the artwork's lower part down (the full width at the top); for
 * "right", from its right part rightwards (the full height at the left). The seam
 * never cuts over: the artwork fades out over its last FEATHER (a CSS mask) onto a
 * blurred copy of itself drawn here, and the continuation starts at that same blur.
 * Except a flat extension ([isFlat]): the artwork already ends in that one colour, so
 * it stays crisp to its edge and the colour simply carries on.
 * Mirror: the reflection, blurring further with distance. Extend: a short blurred
 * reflection bridging into the colours along the edge, softening sideways with
 * distance. Both fade out (to the fluid background beneath) except over a light
 * continuation or one flat colour, which carry on in their own colour. Then the
 * scrim the core worked out for 4.5:1.
 */
export function drawContinuation(canvas: HTMLCanvasElement, img: HTMLImageElement | undefined, edge: ArtworkEdge, orientation: Orientation): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  // Work in "bottom" terms: for "right", transpose the canvas (x↔y) and the image.
  const right = orientation === "right";
  const W = right ? canvas.height : canvas.width;
  const H = right ? canvas.width : canvas.height;
  const S = W; // the artwork's side: the full width (or, for "right", the full height)
  if (H <= S) return;
  let source: CanvasImageSource | undefined = img;
  if (img && right) {
    const t = document.createElement("canvas");
    t.width = img.width;
    t.height = img.height;
    const tc = t.getContext("2d");
    if (tc) {
      tc.setTransform(0, 1, 1, 0, 0, 0);
      tc.drawImage(img, 0, 0);
      source = t;
    }
  }
  if (right) ctx.setTransform(0, 1, 1, 0, 0, 0);
  const region = H - S;
  const flat = isFlat(edge);
  const band0 = flat ? S : Math.round(S * (1 - FEATHER));
  if (flat || edge.light) {
    ctx.fillStyle = hex(edge.baseColor);
    ctx.fillRect(0, band0, W, H - band0);
  }
  /** Draws `paint` into a layer below the seam, masked to `stops` ([fraction of S from the seam, alpha]). */
  const band = (stops: [number, number][], paint: (c: CanvasRenderingContext2D) => void) => {
    const layer = document.createElement("canvas");
    layer.width = W;
    layer.height = region;
    const lc = layer.getContext("2d");
    if (!lc) return;
    paint(lc);
    lc.globalCompositeOperation = "destination-in";
    const g = lc.createLinearGradient(0, 0, 0, region);
    for (const [y, a] of stops) g.addColorStop(Math.min(1, (y * S) / region), `rgba(0,0,0,${a})`);
    lc.fillStyle = g;
    lc.fillRect(0, 0, W, region);
    ctx.drawImage(layer, 0, S);
  };
  const near = source && !flat ? mirrored(source, W, S, BLUR_NEAR * S) : undefined;
  const reflection = (m: { canvas: HTMLCanvasElement; pad: number }) => (c: CanvasRenderingContext2D) => c.drawImage(m.canvas, m.pad, S, W, region, 0, 0, W, region);
  // Under the artwork's fading edge: the same artwork, blurred.
  if (near) ctx.drawImage(near.canvas, near.pad, band0, W, S - band0, 0, band0, W, S - band0);
  if (edge.style === "mirror" && near && source) {
    band([[0, 1], [0.5, 1], [0.9, 0]], reflection(mirrored(source, W, S, BLUR_FAR * S)));
    band([[0, 1], [0.12, 1], [0.4, 0]], reflection(near));
  } else if (edge.style === "extend") {
    const across = (colors: number[]) => (c: CanvasRenderingContext2D) => {
      const g = c.createLinearGradient(0, 0, W, 0);
      colors.forEach((col, i) => g.addColorStop(colors.length > 1 ? i / (colors.length - 1) : 0, hex(col)));
      c.fillStyle = g;
      c.fillRect(0, 0, W, region);
    };
    if (!flat) {
      band([[0, 1], [0.5, 1], [0.9, 0]], across(soften(edge.edgeColors, 6)));
      band([[0, 1], [0.15, 1], [0.45, 0]], across(soften(edge.edgeColors, 2)));
    }
    if (near) band([[0, 1], [0.1, 0]], reflection(near));
  }
  if (edge.scrim > 0) {
    const rgb = edge.light ? "255,255,255" : "0,0,0";
    const g = ctx.createLinearGradient(0, S, 0, H);
    g.addColorStop(0, `rgba(${rgb},0)`);
    g.addColorStop(Math.min(1, (0.2 * S) / region), `rgba(${rgb},${edge.scrim})`);
    g.addColorStop(1, `rgba(${rgb},${edge.scrim})`);
    ctx.fillStyle = g;
    ctx.fillRect(0, S, W, region);
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}
