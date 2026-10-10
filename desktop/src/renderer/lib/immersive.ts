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

/**
 * Draws the continuation into `canvas` (sized to the player, in CSS px × dpr): for
 * "bottom", everything below the artwork's square (the full width at the top); for
 * "right", everything right of it (the full height at the left). Mirror: the artwork
 * flipped, sharp at the seam and blurring with distance. Extend: the colours along
 * the edge carried on, softening sideways with distance. Both fade out (to the
 * fluid background beneath) except over a light continuation or one flat colour,
 * which carry on in their own colour. Then the scrim the core worked out for 4.5:1.
 */
export function drawContinuation(canvas: HTMLCanvasElement, img: HTMLImageElement | undefined, edge: ArtworkEdge, orientation: Orientation, dpr: number): void {
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
  const flat = new Set(edge.edgeColors).size <= 1;
  if (flat || edge.light) {
    ctx.fillStyle = hex(edge.baseColor);
    ctx.fillRect(0, S, W, region);
  }
  /** Draws `paint` into a band layer masked to `stops` ([fraction of S from the seam, alpha]). */
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
  if (edge.style === "mirror" && source) {
    const flipped = (blurPx: number) => (c: CanvasRenderingContext2D) => {
      c.filter = blurPx > 0 ? `blur(${blurPx * dpr}px)` : "none";
      c.save();
      c.scale(1, -1);
      c.drawImage(source, 0, -S, W, S);
      c.restore();
    };
    band([[0, 1], [0.45, 1], [0.85, 0]], flipped(40));
    band([[0, 1], [0.08, 1], [0.25, 0]], flipped(12));
    band([[0, 1], [0.04, 0]], flipped(0));
  } else if (edge.style === "extend" && !flat) {
    const across = (colors: number[]) => (c: CanvasRenderingContext2D) => {
      const g = c.createLinearGradient(0, 0, W, 0);
      colors.forEach((col, i) => g.addColorStop(colors.length > 1 ? i / (colors.length - 1) : 0, hex(col)));
      c.fillStyle = g;
      c.fillRect(0, 0, W, region);
    };
    band([[0, 1], [0.5, 1], [0.9, 0]], across(soften(edge.edgeColors, 6)));
    band([[0, 1], [0.12, 1], [0.4, 0]], across(soften(edge.edgeColors, 2)));
    band([[0, 1], [0.12, 0]], across(edge.edgeColors));
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
