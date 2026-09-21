// Kawarp fluid background fed from the cached artwork via loadBlob. Tint and
// saturation tuned to sit behind AMLL's white lyric styling. Honours the
// performance budget: stopped when hidden, ~24 fps unfocused, static blurred
// still in battery saver or when the animated background is switched off.
import { useEffect, useRef } from "react";
import { Kawarp } from "@kawarp/core";
import { useApp, useSetting } from "../store/app";
import { useArtwork } from "./Artwork";
import { SK } from "@shared/settings-keys";

export function FluidBackground({ coverArt }: { coverArt: string | undefined }) {
  const perf = useApp((s) => s.perf);
  const batterySaver = useApp((s) => s.batterySaver);
  const animatedSetting = useSetting(SK.displayAnimatedBackground, true);
  const url = useArtwork(coverArt, 300);
  const canvas = useRef<HTMLCanvasElement>(null);
  const kawarp = useRef<Kawarp | undefined>(undefined);
  const animated = animatedSetting && !batterySaver;

  useEffect(() => {
    if (!animated || !canvas.current) return;
    let k: Kawarp;
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
      });
    } catch (err) {
      console.warn("[background] WebGL unavailable, falling back to a still", err);
      return;
    }
    kawarp.current = k;
    const ro = new ResizeObserver(() => k.resize());
    ro.observe(canvas.current);
    return () => {
      ro.disconnect();
      k.dispose();
      kawarp.current = undefined;
    };
  }, [animated]);

  useEffect(() => {
    const k = kawarp.current;
    if (!k || !animated) return;
    if (!url) {
      k.loadGradient(["#1c1c28", "#2a2540"], 30);
      return;
    }
    let alive = true;
    fetch(url)
      .then((r) => r.blob())
      .then((b) => { if (alive) return k.loadBlob(b); })
      .catch(() => alive && k.loadGradient(["#1c1c28", "#2a2540"], 30));
    return () => {
      alive = false;
    };
  }, [url, animated]);

  // Frame budget: explicit start/stop plus a throttled render in the background.
  useEffect(() => {
    const k = kawarp.current;
    if (!k || !animated) return;
    if (perf === "stopped") {
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
  }, [perf, animated]);

  if (!animated) return url ? <img className="bg-still" src={url} alt="" /> : <div className="bg-still" style={{ background: "#1c1c28" }} />;
  return <canvas ref={canvas} className="bg" data-testid="fluid-bg" />;
}
