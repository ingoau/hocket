// The header of the album, artist and playlist pages. Artwork is never
// stretched past its own pixels: a cover big enough to fill the header edge to
// edge is shown that way (Large); a smaller one sits at its size over a
// blurred wash of itself (Small); with none, an accent gradient (None).
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ARTWORK_SIZES } from "@shared/constants";
import { resolveArtwork } from "./Artwork";
import { Icon } from "./Icon";

export type HeroMode = "large" | "small" | "none";

/** The Large header's height by default; a cover must cover it (and the header's width) at the screen's pixel density. */
export const LARGE_HERO_HEIGHT = 470;

/**
 * Large only when the image has the pixels to fill `width` × `height` CSS px at `dpr` without being
 * scaled up. `url` undefined means no artwork; null means it is still being resolved (shown as Small,
 * so a page with a cover never flashes the no-artwork gradient first).
 */
export function heroMode(natural: { width: number; height: number } | undefined, url: string | null | undefined, width: number, height: number, dpr: number): HeroMode {
  if (url === undefined) return "none";
  if (url === null) return "small";
  if (!natural || !width) return "small";
  // Cropped to cover: the scale is set by the tighter side.
  const scale = Math.max((width * dpr) / natural.width, (height * dpr) / natural.height);
  return scale <= 1 ? "large" : "small";
}

/** The hero holds `children` (an eyebrow, the title, its lines); `actions` sit under it, on the page. */
export function DetailHead({ coverArt, round = false, icon = "music", onContextMenu, children, actions, testId }: { coverArt: string | undefined; round?: boolean; icon?: string; onContextMenu?: (e: React.MouseEvent) => void; children: ReactNode; actions?: ReactNode; testId?: string }) {
  // null while resolving; undefined when there is no cover (or it could not be fetched).
  const [resolved, setResolved] = useState<{ id: string; url: string | undefined } | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    if (coverArt) void resolveArtwork(coverArt, ARTWORK_SIZES.full).then((u) => alive && setResolved({ id: coverArt, url: u }));
    return () => { alive = false; };
  }, [coverArt]);
  const url = !coverArt ? undefined : resolved?.id === coverArt ? resolved.url : null;
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
  const mode = heroMode(natural, url, width, LARGE_HERO_HEIGHT, window.devicePixelRatio || 1);
  return (
    <>
    <div ref={ref} className={`detail-head hero-${mode}`} data-hero={mode} onContextMenu={onContextMenu} data-testid={testId}>
      {url ? (
        <div className="hero-backdrop" aria-hidden="true">
          <img src={url} alt="" draggable={false} onLoad={(e) => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })} />
        </div>
      ) : null}
      {mode === "small" ? (url ? <img className={`art${round ? " round" : ""}`} src={url} alt="" draggable={false} /> : <div className={`art pending${round ? " round" : ""}`} aria-hidden="true" />) : null}
      {mode === "none" ? <div className={`art placeholder${round ? " round" : ""}`} aria-hidden="true"><Icon name={icon} size={64} /></div> : null}
      <div className="meta">{children}</div>
    </div>
    {actions ? <div className="detail-actions">{actions}</div> : null}
    </>
  );
}
