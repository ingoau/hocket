// React side of the album primer (lib/prime.ts): dispatches PrimeAlbum /
// PrimeTrack, fire and forget. The core routes each prime to the device that
// owns playback and decides there whether to fetch anything.
import { useEffect, useMemo, useRef } from "react";
import { bridge } from "../core/bridge";
import { useApp } from "../store/app";
import { IntentPrimer, VisitDwell, type Point } from "./prime";

/** The window is on screen, focused, and nothing modal covers the page. */
function useActivePage(): boolean {
  return useApp((s) => s.pageVisible && s.windowState.visible && s.windowState.focused && !s.dialog && !s.paletteOpen && !s.fullscreen);
}

/** PrimeAlbum once per visit, after the album page has been active for 2 s. */
export function useAlbumDwellPrime(albumId: string | undefined): void {
  const active = useActivePage();
  const dwell = useRef<VisitDwell | undefined>(undefined);
  useEffect(() => {
    if (!albumId) return;
    const d = new VisitDwell(() => bridge().dispatch({ type: "primeAlbum", data: { album_id: albumId } }));
    dwell.current = d;
    return () => {
      d.dispose();
      if (dwell.current === d) dwell.current = undefined;
    };
  }, [albumId]);
  useEffect(() => {
    dwell.current?.setActive(active && !!albumId);
  }, [active, albumId]);
}

export interface IntentHandlers {
  onPointerEnter: (e: React.PointerEvent) => void;
  onPointerMove: (e: React.PointerEvent) => void;
  onPointerLeave: () => void;
  onFocus: () => void;
  onBlur: () => void;
}

export interface Primer {
  /** Handlers for a play control that plays `id` (a track, or an album with `kind: "album"`). */
  bind(id: string | undefined): IntentHandlers;
  enter(id: string | undefined): void;
  leave(id: string | undefined): void;
}

/**
 * Hover/focus intent on play controls: PrimeTrack (or PrimeAlbum) after ~300 ms,
 * once per id per `visitKey`. Any scroll in the window cancels pending intents,
 * so content scrolling under a resting pointer never primes.
 */
export function usePlayIntent(visitKey: string, kind: "track" | "album" = "track"): Primer {
  const primer = useMemo(() => {
    void visitKey; // a new visit is a new primer: every id may prime again
    return new IntentPrimer((id) => bridge().dispatch(kind === "album" ? { type: "primeAlbum", data: { album_id: id } } : { type: "primeTrack", data: { track_id: id } }));
  }, [visitKey, kind]);
  useEffect(() => {
    let pointer: Point | undefined;
    const onMove = (e: PointerEvent) => { pointer = { x: e.clientX, y: e.clientY }; };
    const onScroll = () => primer.scrolled(pointer);
    // Capture: scroll events don't bubble, and every list scrolls its own box.
    document.addEventListener("pointermove", onMove, { capture: true, passive: true });
    document.addEventListener("scroll", onScroll, { capture: true, passive: true });
    document.addEventListener("wheel", onScroll, { capture: true, passive: true });
    return () => {
      document.removeEventListener("pointermove", onMove, { capture: true });
      document.removeEventListener("scroll", onScroll, { capture: true });
      document.removeEventListener("wheel", onScroll, { capture: true });
      primer.dispose();
    };
  }, [primer]);
  return useMemo<Primer>(() => ({
    enter: (id) => { if (id) primer.enter(id); },
    leave: (id) => { if (id) primer.leave(id); },
    bind: (id) => ({
      onPointerEnter: (e) => { if (id) primer.enter(id, { x: e.clientX, y: e.clientY }); },
      onPointerMove: (e) => { if (id) primer.enter(id, { x: e.clientX, y: e.clientY }); },
      onPointerLeave: () => { if (id) primer.leave(id); },
      onFocus: () => { if (id) primer.enter(id); },
      onBlur: () => { if (id) primer.leave(id); },
    }),
  }), [primer]);
}
