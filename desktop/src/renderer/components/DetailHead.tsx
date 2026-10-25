// The header of the album, artist and playlist pages. Artwork is never
// stretched past its own pixels: a cover big enough to fill the header edge to
// edge is shown that way (Large); a smaller one sits at its size over a
// blurred wash of itself (Small); with none, an accent gradient (None).
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ARTWORK_SIZES } from "@shared/constants";
import { useArtwork } from "./Artwork";
import { Icon } from "./Icon";

export type HeroMode = "large" | "small" | "none";

/** The Large header's height by default; a cover must cover it (and the header's width) at the screen's pixel density. */
export const LARGE_HERO_HEIGHT = 420;
export const COMPACT_HERO_HEIGHT = 320;

/** Large only when the image has the pixels to fill `width` × `height` CSS px at `dpr` without being scaled up. */
export function heroMode(natural: { width: number; height: number } | undefined, url: string | undefined, width: number, height: number, dpr: number): HeroMode {
  if (!url) return "none";
  if (!natural || !width) return "small";
  // Cropped to cover: the scale is set by the tighter side.
  const scale = Math.max((width * dpr) / natural.width, (height * dpr) / natural.height);
  return scale <= 1 ? "large" : "small";
}

/** `compact`: the album and playlist pages, whose track list scrolls under a fixed header, keep it shorter. */
export function DetailHead({ coverArt, round = false, compact = false, icon = "music", onContextMenu, children, testId }: { coverArt: string | undefined; round?: boolean; compact?: boolean; icon?: string; onContextMenu?: (e: React.MouseEvent) => void; children: ReactNode; testId?: string }) {
  const url = useArtwork(coverArt, ARTWORK_SIZES.full);
  const [natural, setNatural] = useState<{ width: number; height: number } | undefined>(undefined);
  const [width, setWidth] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => setNatural(undefined), [url]);
  const mode = heroMode(natural, url, width, compact ? COMPACT_HERO_HEIGHT : LARGE_HERO_HEIGHT, window.devicePixelRatio || 1);
  return (
    <div ref={ref} className={`detail-head hero-${mode}${compact ? " compact" : ""}`} data-hero={mode} onContextMenu={onContextMenu} data-testid={testId}>
      {url ? (
        <div className="hero-backdrop" aria-hidden="true">
          <img src={url} alt="" draggable={false} onLoad={(e) => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })} />
        </div>
      ) : null}
      {mode === "small" && url ? <img className={`art${round ? " round" : ""}`} src={url} alt="" draggable={false} /> : null}
      {mode === "none" ? <div className={`art placeholder${round ? " round" : ""}`} aria-hidden="true"><Icon name={icon} size={64} /></div> : null}
      <div className="meta">{children}</div>
    </div>
  );
}
