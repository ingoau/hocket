// Two-finger swipe over the page to go back (fingers right) or forward (left), like a browser:
// an arrow comes in at the edge and fills once letting go would go there, and the page leans a
// little the way it is pulled. The top bar's arrows and the keyboard do the same. Song rows and
// anything that scrolls sideways keep their own swipes (lib/swipe-hook.ts).
import { useEffect, type RefObject } from "react";
import { useApp } from "../store/app";
import { useSurfaceTuning } from "../lib/gesture-prefs";
import { usePrefersReducedMotion } from "../lib/media";
import { useSettle, useSwipe } from "../lib/swipe-hook";
import { isRtl } from "../lib/swipe-actions";
import { Icon } from "./Icon";

/** How much of the pull the page itself follows. */
const LEAN = 0.25;

export function PageSwipe({ target }: { target: RefObject<HTMLElement | null> }) {
  const tuning = useSurfaceTuning("page");
  const reducedMotion = usePrefersReducedMotion();
  const settle = useSettle(tuning, reducedMotion);
  const canBack = useApp((s) => s.history.length > 0);
  const canForward = useApp((s) => s.future.length > 0);
  const rtl = isRtl();
  // Back is towards the end of the line: right in left-to-right text.
  const view = useSwipe(target, {
    tuning,
    sides: rtl ? { start: canForward, end: canBack } : { start: canBack, end: canForward },
    onCommit: (side) => {
      const app = useApp.getState();
      if ((side > 0) !== rtl) app.back();
      else app.forward();
    },
  });
  const tracking = view.stage === "tracking";
  const shift = tracking ? view.offset * LEAN : 0;
  useEffect(() => {
    const el = target.current;
    if (!el) return;
    if (tracking || view.settling) {
      el.dataset.pageSwipe = "";
      el.style.setProperty("--page-swipe-x", `${reducedMotion ? 0 : shift}px`);
      el.style.setProperty("--page-swipe-transition", tracking ? "none" : settle);
    } else {
      delete el.dataset.pageSwipe;
      el.style.removeProperty("--page-swipe-x");
      el.style.removeProperty("--page-swipe-transition");
    }
  }, [target, tracking, view.settling, shift, settle, reducedMotion]);
  const width = target.current?.clientWidth ?? 1;
  const progress = tuning ? Math.min(1, Math.abs(view.offset) / (tuning.threshold * width)) : 0;
  const ease = tracking || reducedMotion ? "none" : "opacity 200ms ease, transform 200ms ease, background-color 120ms";
  const arrow = (dir: "back" | "fwd", shown: boolean, armed: boolean) => {
    const p = shown ? progress : 0;
    const from = dir === "back" ? -48 : 48;
    const x = dir === "back" ? p * 56 - 48 : 48 - p * 56;
    return (
      <div className={`page-arrow ${dir} ${armed ? "armed" : ""}`} aria-hidden="true" data-testid={`page-swipe-${dir}`} data-armed={armed || undefined}
        style={{ opacity: p, transform: `translate(${shown ? x : from}px, -50%) scale(${0.6 + 0.4 * p})`, transition: ease }}>
        <Icon name={dir === "back" ? "chevronLeft" : "chevronRight"} size={22} />
      </div>
    );
  };
  if (!tuning) return null;
  const towardsEnd = tracking && view.offset > 0;
  const towardsStart = tracking && view.offset < 0;
  // The arrow on the side the page is pulled from: back on the left in left-to-right text.
  return (
    <>
      {arrow("back", rtl ? towardsStart : towardsEnd, rtl ? view.armed < 0 : view.armed > 0)}
      {arrow("fwd", rtl ? towardsEnd : towardsStart, rtl ? view.armed > 0 : view.armed < 0)}
    </>
  );
}
