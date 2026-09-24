// User media preferences as React state: reduced motion, more contrast,
// forced colours (Windows high contrast), and viewport breakpoints.
import { useSyncExternalStore } from "react";
import { useLyricsAnimated } from "./lyrics-size";

function query(q: string): MediaQueryList | undefined {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" ? window.matchMedia(q) : undefined;
}

export function useMediaQuery(q: string): boolean {
  return useSyncExternalStore(
    (cb) => {
      const mq = query(q);
      mq?.addEventListener("change", cb);
      return () => mq?.removeEventListener("change", cb);
    },
    () => query(q)?.matches ?? false,
    () => false,
  );
}

export const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";
export const MORE_CONTRAST = "(prefers-contrast: more)";
export const FORCED_COLORS = "(forced-colors: active)";
/** Below this the shell switches to the narrow layout (icon rail, side panel as a drawer, two-row player bar). */
export const NARROW = "(max-width: 900px)";

export function usePrefersReducedMotion(): boolean {
  return useMediaQuery(REDUCED_MOTION);
}

/**
 * Lyrics render as a plain, statically highlighted list (no sweep, blur or
 * springs): with reduced motion, more contrast or forced colours, or when the
 * user turned the animation off in Settings → Appearance.
 */
export function usePlainLyrics(): boolean {
  const reduced = useMediaQuery(REDUCED_MOTION);
  const contrast = useMediaQuery(MORE_CONTRAST);
  const forced = useMediaQuery(FORCED_COLORS);
  const animated = useLyricsAnimated();
  return reduced || contrast || forced || !animated;
}
