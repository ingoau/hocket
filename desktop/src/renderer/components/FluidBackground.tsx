// Kawarp fluid background fed from the cached artwork via a CORS-clean <img>. Tint and
// saturation tuned to sit behind AMLL's white lyric styling. Honours the
// performance budget: stopped when hidden, ~24 fps unfocused, static blurred
// still in battery saver, when the animated background is switched off, or
// when the user prefers reduced motion. Kawarp is loaded on demand so the
// main shell does not parse WebGL until the fullscreen player opens.
//
// Contrast: a black scrim sits over the background, sized from the cover's
// brightest colour so the fullscreen text colours (white and the 80 % white
// muted text) reach 4.5:1 over any part of it. Until the cover is measured
// the scrim assumes a white cover.
import { useEffect, useRef, useState } from "react";
import { useApp, useSetting } from "../store/app";
import { useArtwork } from "./Artwork";
import { SK } from "@shared/settings-keys";
import { usePrefersReducedMotion } from "../lib/media";
import { extractBrightest } from "../lib/accent";
import { WHITE, scrimAlpha, type Rgb } from "../lib/contrast";

/** Alpha of the fullscreen player's muted text (global.css `.fullscreen --fg-muted`). */
export const FULLSCREEN_MUTED_ALPHA = 0.8;
/** A little above 4.5:1: the fluid background is resampled and saturated, not the exact cover. */
const TARGET = 4.8;

type KawarpEngine = {
  resize(): void;
  dispose(): void;
  start(): void;
  stop(): void;
  renderFrame(now: number): void;
  loadGradient(colors: string[], angle: number): void;
  loadImageElement(img: HTMLImageElement): void;
};

export function FluidBackground({ coverArt }: { coverArt: string | undefined }) {
  const perf = useApp((s) => s.perf);
  const playing = useApp((s) => s.transport.position.isPlaying);
  const batterySaver = useApp((s) => s.batterySaver);
  const animatedSetting = useSetting(SK.displayAnimatedBackground, true);
  const reducedMotion = usePrefersReducedMotion();
  const url = useArtwork(coverArt, 300);
  const canvas = useRef<HTMLCanvasElement>(null);
  const kawarp = useRef<KawarpEngine | undefined>(undefined);
  const animated = animatedSetting && !batterySaver && !reducedMotion;
  const [engineGen, setEngineGen] = useState(0);
  const [brightest, setBrightest] = useState<Rgb | undefined>(undefined);
  useEffect(() => {
    setBrightest(undefined);
    if (!url) return;
    let alive = true;
    void extractBrightest(url).then((c) => alive && setBrightest(c));
    return () => { alive = false; };
  }, [url]);
  const scrim = scrimAlpha(brightest ?? WHITE, FULLSCREEN_MUTED_ALPHA, TARGET);
  const [source, setSource] = useState<"none" | "artwork" | "gradient">("none");

  useEffect(() => {
    if (!animated || !canvas.current) return;
    let disposed = false;
    let k: KawarpEngine | undefined;
    let ro: ResizeObserver | undefined;
    void import("@kawarp/core").then(({ Kawarp }) => {
      if (disposed || !canvas.current) return;
      try {
        k = new Kawarp(canvas.current, {
          warpIntensity: 0.85,
          blurPasses: 10,
          animationSpeed: 0.7,
          transitionDuration: 1400,
          saturation: 1.35,
          tintColor: [0.08, 0.08, 0.12],
          tintIntensity: 0.22,
          dithering: 0.008,
          scale: 1.15,
        }) as KawarpEngine;
      } catch (err) {
        console.warn("[background] WebGL unavailable, falling back to a still", err);
        return;
      }
      kawarp.current = k;
      ro = new ResizeObserver(() => k?.resize());
      ro.observe(canvas.current);
      setEngineGen((n) => n + 1);
    });
    return () => {
      disposed = true;
      ro?.disconnect();
      k?.dispose();
      kawarp.current = undefined;
    };
  }, [animated]);

  useEffect(() => {
    const k = kawarp.current;
    if (!k || !animated) return;
    if (!url) {
      k.loadGradient(["#1c1c28", "#2a2540"], 30);
      setSource("gradient");
      return;
    }
    // Through an <img> (CORS-clean, like the accent picker), not fetch():
    // the CSP deliberately keeps hocket-art: out of connect-src.
    let alive = true;
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.decoding = "async";
    img.src = url;
    img
      .decode()
      .then(() => {
        if (!alive) return;
        k.loadImageElement(img);
        setSource("artwork");
      })
      .catch(() => {
        if (!alive) return;
        k.loadGradient(["#1c1c28", "#2a2540"], 30);
        setSource("gradient");
      });
    return () => {
      alive = false;
    };
  }, [url, animated, engineGen]);

  // Frame budget: explicit start/stop plus a throttled render in the background.
  // While playback is paused the last frame stays on the canvas (no GPU loop).
  useEffect(() => {
    const k = kawarp.current;
    if (!k || !animated) return;
    if (perf === "stopped" || !playing) {
      try {
        k.renderFrame(performance.now());
      } catch {
        /* image may not have loaded yet */
      }
      k.stop();
      return;
    }
    if (perf === "full") {
      k.start();
      return () => k.stop();
    }
    // background: ~24 fps via our own interval instead of Kawarp's rAF loop
    k.stop();
    const id = setInterval(() => k.renderFrame(performance.now()), 1000 / 24);
    return () => clearInterval(id);
  }, [perf, animated, playing, engineGen]);

  const shade = <div className="bg-shade" style={{ background: `rgba(0, 0, 0, ${scrim})` }} data-scrim={scrim} data-measured={brightest ? "true" : "false"} data-testid="fs-scrim" aria-hidden="true" />;
  if (!animated) return <>{url ? <img className="bg-still" src={url} alt="" data-testid="fs-still" /> : <div className="bg-still" style={{ background: "#1c1c28" }} data-testid="fs-still" />}{shade}</>;
  return <><canvas ref={canvas} className="bg" data-testid="fluid-bg" data-source={source} aria-hidden="true" />{shade}</>;
}
