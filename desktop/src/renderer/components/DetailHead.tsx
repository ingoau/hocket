// The header of the album, artist and playlist pages. Artwork is never
// stretched past its own pixels: a cover big enough to fill the header edge to
// edge is shown that way (Large, the immersive one); a smaller one sits at its
// size on a wash of its own colour that fades into the page under the actions
// (Small); with none, an accent gradient (None). Every mode is the same height
// (--hero-h), so the page never jumps when the cover turns out to be big.
//
// Once the actions have scrolled away, a compact bar (Play and the title, on
// the hero's colour) fades in at the top of the page, as a native player's does.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { resolveArtwork } from "./Artwork";
import { useLargeArtworkSize } from "../lib/artwork-size";
import { Icon } from "./Icon";
import { extractAccent } from "../lib/accent";
import { heroWash } from "../lib/contrast";
import { scrollParent } from "../lib/scroll-parent";

export type HeroMode = "large" | "small" | "none";

/**
 * Large only when the image has the pixels to fill `width` × `height` CSS px at `dpr` without being
 * scaled up. `url` undefined means no artwork; null means it is still being resolved (shown as Small,
 * so a page with a cover never flashes the no-artwork gradient first).
 */
export function heroMode(natural: { width: number; height: number } | undefined, url: string | null | undefined, width: number, height: number, dpr: number): HeroMode {
  if (url === undefined) return "none";
  if (url === null) return "small";
  if (!natural || !width || !height) return "small";
  // Cropped to cover: the scale is set by the tighter side.
  const scale = Math.max((width * dpr) / natural.width, (height * dpr) / natural.height);
  return scale <= 1 ? "large" : "small";
}

export interface DetailHeadProps {
  coverArt: string | undefined;
  /** The page's item is still loading: its cover is not known to be missing yet (no None flash). */
  loading?: boolean;
  round?: boolean;
  icon?: string;
  onContextMenu?: (e: React.MouseEvent) => void;
  /** The hero's contents: an eyebrow, the title, its lines. */
  children: ReactNode;
  /** Under the hero, on the page. */
  actions?: ReactNode;
  /** The compact bar's title and its Play (shown once the actions have scrolled away). */
  title: string;
  barAction?: ReactNode;
  testId?: string;
}

export function DetailHead({ coverArt, loading = false, round = false, icon = "music", onContextMenu, children, actions, title, barAction, testId }: DetailHeadProps) {
  // null while resolving; undefined when there is no cover (or it could not be fetched).
  const [resolved, setResolved] = useState<{ id: string; url: string | undefined } | undefined>(undefined);
  const size = useLargeArtworkSize();
  useEffect(() => {
    let alive = true;
    if (coverArt) void resolveArtwork(coverArt, size).then((u) => alive && setResolved({ id: coverArt, url: u }));
    return () => { alive = false; };
  }, [coverArt, size]);
  const url = loading ? null : !coverArt ? undefined : resolved?.id === coverArt ? resolved.url : null;
  const [natural, setNatural] = useState<{ width: number; height: number } | undefined>(undefined);
  const [box, setBox] = useState({ width: 0, height: 0 });
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setBox((b) => (b.width === el.clientWidth && b.height === el.offsetHeight ? b : { width: el.clientWidth, height: el.offsetHeight }));
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => setNatural(undefined), [url]);
  const mode = heroMode(natural, url, box.width, box.height, window.devicePixelRatio || 1);
  // The wash: the cover's accent, darkened until white text holds on it (the blurred cover shows until then).
  const [wash, setWash] = useState<{ url: string; colour: string } | undefined>(undefined);
  useEffect(() => {
    if (!url) return;
    let alive = true;
    void extractAccent(url).then((c) => { if (alive && c) setWash({ url, colour: heroWash(c) }); });
    return () => { alive = false; };
  }, [url]);
  const colour = wash && wash.url === url ? wash.colour : undefined;
  const washed = mode === "small" ? colour : undefined;
  const washStyle = washed ? ({ "--hero-wash": washed } as CSSProperties) : undefined;

  // The compact bar shows once the actions (or the hero, without any) are under it.
  const bar = useRef<HTMLDivElement>(null);
  const actionsRef = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(false);
  useEffect(() => {
    const target = actionsRef.current ?? ref.current;
    const el = bar.current;
    const root = el ? scrollParent(el) : null;
    if (!target || !el || !root) return;
    const io = new IntersectionObserver(([e]) => {
      if (!e) return;
      setStuck(!e.isIntersecting && e.boundingClientRect.top < (e.rootBounds?.top ?? 0));
    }, { root, rootMargin: `-${el.offsetHeight}px 0px 0px 0px` });
    io.observe(target);
    return () => io.disconnect();
  }, [!!actions]); // eslint-disable-line react-hooks/exhaustive-deps
  const barStyle = colour ? ({ "--hero-wash": colour } as CSSProperties) : undefined;
  return (
    <>
    <div ref={bar} className={`detail-bar bar-${mode}${stuck ? " stuck" : ""}`} style={barStyle} inert={!stuck} aria-hidden={!stuck} data-testid="detail-bar">
      {barAction}
      <span className="detail-bar-title">{title}</span>
    </div>
    <div ref={ref} className={`detail-head hero-${mode} ${washed ? "washed" : ""}`} style={washStyle} data-hero={mode} onContextMenu={onContextMenu} data-testid={testId}>
      {url ? (
        <div className="hero-backdrop" aria-hidden="true">
          <img src={url} alt="" draggable={false} onLoad={(e) => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })} />
        </div>
      ) : null}
      {mode === "small" ? (url ? <img className={`art${round ? " round" : ""}`} src={url} alt="" draggable={false} /> : <div className={`art pending${round ? " round" : ""}`} aria-hidden="true" />) : null}
      {mode === "none" ? <div className={`art placeholder${round ? " round" : ""}`} aria-hidden="true"><Icon name={icon} size={64} /></div> : null}
      <div className="meta">{children}</div>
    </div>
    {actions ? <div ref={actionsRef} className={`detail-actions ${washed ? "washed" : ""}`} style={washStyle}>{actions}</div> : null}
    </>
  );
}
