// Swipe the now-playing artwork to skip: it follows the fingers, and letting go past the threshold
// slides it out the way it went while the next (or previous) song's cover comes in from the other
// side. Right goes back to the previous song, left on to the next, like turning pages. The player's
// buttons and the media keys do the same.
import { useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { bridge } from "../core/bridge";
import { useApp } from "../store/app";
import { useSurfaceTuning } from "./gesture-prefs";
import { usePrefersReducedMotion } from "./media";
import type { Side } from "./swipe";
import { useSettle, useSwipe } from "./swipe-hook";

const OUT_MS = 200;
const IN_MS = 260;

type Skip = { side: Side; phase: "out" | "snap" | "in" };

export function useCoverSwipe(ref: RefObject<HTMLElement | null>): { className: string; style: CSSProperties } {
  const tuning = useSurfaceTuning("cover");
  const reducedMotion = usePrefersReducedMotion();
  const settle = useSettle(tuning, reducedMotion);
  const playing = useApp((s) => !!s.nowPlaying);
  const [skip, setSkip] = useState<Skip | undefined>(undefined);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  const view = useSwipe(ref, {
    tuning,
    sides: { start: playing, end: playing },
    disabled: !!skip,
    onCommit: (side) => {
      bridge().dispatch(side > 0 ? { type: "previous" } : { type: "next" });
      if (reducedMotion) return;
      setSkip({ side, phase: "out" });
      timers.current.push(
        setTimeout(() => {
          setSkip({ side, phase: "snap" });
          // Two frames: the snap is drawn without a transition before the slide in starts.
          requestAnimationFrame(() => requestAnimationFrame(() => setSkip({ side, phase: "in" })));
        }, OUT_MS),
        setTimeout(() => setSkip(undefined), OUT_MS + IN_MS + 40),
      );
    },
  });
  const tracking = view.stage === "tracking";
  let style: CSSProperties;
  if (skip?.phase === "out") style = { translate: `calc(${skip.side} * (100% + 16px)) 0`, opacity: 0, transition: `translate ${OUT_MS}ms cubic-bezier(.3,0,.8,.15), opacity ${OUT_MS}ms ease` };
  else if (skip?.phase === "snap") style = { translate: `calc(${-skip.side} * 40%) 0`, opacity: 0, transition: "none" };
  else if (skip?.phase === "in") style = { translate: "0 0", opacity: 1, transition: `translate ${IN_MS}ms cubic-bezier(.2,.8,.2,1), opacity ${IN_MS}ms ease` };
  else if (tracking || view.settling) style = { translate: `${tracking ? view.offset : 0}px 0`, transition: tracking ? "none" : settle };
  else style = {};
  return { className: `cover-swipe${view.armed ? " armed" : ""}`, style };
}
