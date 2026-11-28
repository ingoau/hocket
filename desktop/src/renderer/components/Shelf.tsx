// A shelf: one row of tiles that scrolls sideways (Home's sections, search results). Its tiles are
// sized so a whole number of them fills the row, growing with the window. It runs out to the
// pane's edges, so tiles slide under them rather than being cut at the page's gutter, and scrolls
// freely under a trackpad or Shift + wheel (no CSS snapping: on Linux Chromium snaps after every
// wheel event, so a small scroll springs back). Back and next buttons over its ends, shown while
// the pointer is on the row and there is more that way, page it a rowful at a time, each page
// starting on a whole tile. The keyboard moves tile to tile (TileList) and scrolls with it.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { TileList } from "./Tile";
import { Icon } from "./Icon";
import { usePrefersReducedMotion } from "../lib/media";

/**
 * Where paging a row lands (its scrollLeft), so the page starts on a whole tile at the row's start
 * ([inset] in from its edge). Forward: the first tile not wholly in view leads the next page. Back:
 * a rowful (the row and one [gap]) back, moved on to the next tile start.
 */
export function pageTarget(starts: readonly number[], widths: readonly number[], scrollLeft: number, viewport: number, inset: number, gap: number, dir: 1 | -1): number {
  const span = viewport - 2 * inset;
  if (dir > 0) {
    const end = scrollLeft + inset + span;
    const i = starts.findIndex((s, k) => s + (widths[k] ?? 0) > end + 1);
    return i < 0 ? scrollLeft : Math.max(0, (starts[i] ?? 0) - inset);
  }
  const want = scrollLeft - span - gap;
  if (want <= 1) return 0;
  const i = starts.findIndex((s) => s - inset >= want - 1);
  return i < 0 ? 0 : Math.max(0, (starts[i] ?? 0) - inset);
}

/**
 * How wide a shelf's tiles are, so a whole number of them (at least [min] wide, two or more)
 * fills [width]: the row never ends on a sliver of a tile, and they grow with the window.
 */
export function tileWidth(width: number, gap: number, min: number): number {
  const count = Math.max(2, Math.floor((width + gap) / (min + gap)));
  return Math.floor(((width - (count - 1) * gap) / count) * 100) / 100;
}

/** The narrowest a shelf's tile gets (its padding included) before one fewer fits. */
const MIN_TILE = 156;

export function Shelf({ label, children, testId }: { label: string; children: ReactNode; testId?: string }) {
  const box = useRef<HTMLDivElement>(null);
  const [ends, setEnds] = useState({ back: false, next: false });
  const reducedMotion = usePrefersReducedMotion();
  const list = () => box.current?.querySelector<HTMLElement>(":scope > ul");
  const measure = useCallback(() => {
    const el = box.current?.querySelector<HTMLElement>(":scope > ul");
    if (!el) return;
    const max = el.scrollWidth - el.clientWidth;
    const back = el.scrollLeft > 1;
    const next = el.scrollLeft < max - 1;
    setEnds((e) => (e.back === back && e.next === next ? e : { back, next }));
  }, []);
  // Before the first paint, and again on every resize: the tiles fit the row.
  useLayoutEffect(() => {
    const el = list();
    const shelf = box.current;
    if (!el || !shelf) return;
    const fit = () => {
      const cs = getComputedStyle(el);
      const inset = Number.parseFloat(cs.paddingLeft) || 0;
      const gap = Number.parseFloat(cs.columnGap) || 0;
      shelf.style.setProperty("--shelf-tile", `${tileWidth(el.clientWidth - 2 * inset, gap, MIN_TILE)}px`);
      measure();
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [measure]);
  useEffect(() => {
    const el = list();
    if (!el) return;
    measure();
    el.addEventListener("scroll", measure, { passive: true });
    // Tiles arriving (or leaving) change how far it scrolls without resizing it.
    const mo = new MutationObserver(measure);
    mo.observe(el, { childList: true });
    return () => { el.removeEventListener("scroll", measure); mo.disconnect(); };
  }, [measure]);
  const page = (dir: 1 | -1) => {
    const el = list();
    if (!el) return;
    const tiles = [...el.children] as HTMLElement[];
    const cs = getComputedStyle(el);
    const left = pageTarget(tiles.map((x) => x.offsetLeft), tiles.map((x) => x.offsetWidth), el.scrollLeft, el.clientWidth, Number.parseFloat(cs.paddingLeft) || 0, Number.parseFloat(cs.columnGap) || 0, dir);
    el.scrollTo({ left, behavior: reducedMotion ? "instant" : "smooth" });
  };
  return (
    <div ref={box} className="shelf" data-testid={testId}>
      <TileList label={label} className="scroller">{children}</TileList>
      {/* For the pointer: the keyboard already moves through the tiles (and scrolls with them). */}
      <button type="button" className={`shelf-arrow back ${ends.back ? "" : "spent"}`} tabIndex={-1} aria-hidden="true" onMouseDown={(e) => e.preventDefault()} onClick={() => page(-1)} data-testid="shelf-back"><Icon name="chevronLeft" size={20} /></button>
      <button type="button" className={`shelf-arrow next ${ends.next ? "" : "spent"}`} tabIndex={-1} aria-hidden="true" onMouseDown={(e) => e.preventDefault()} onClick={() => page(1)} data-testid="shelf-next"><Icon name="chevronRight" size={20} /></button>
    </div>
  );
}
