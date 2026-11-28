// The columns of a tile grid, as CSS's `repeat(auto-fill, minmax(min, 1fr))` lays them out: as many
// as fit at the minimum width, then stretched to fill the row exactly (no gap left at the end).
import { useCallback, useLayoutEffect, useState, type CSSProperties, type RefObject } from "react";

export interface GridColumns {
  cols: number;
  /** Each column's width (fractional, as the grid's 1fr tracks are). */
  colW: number;
}

export function gridColumns(inner: number, minWidth: number, gap: number): GridColumns {
  const width = Math.max(0, inner);
  const cols = Math.max(1, Math.floor((width + gap) / (minWidth + gap)));
  return { cols, colW: Math.max(0, (width - (cols - 1) * gap) / cols) };
}

/** The columns a laid-out CSS grid resolved to, from its computed `grid-template-columns`. */
export function resolvedColumns(template: string): GridColumns | undefined {
  const widths = template.split(/\s+/).map((w) => Number.parseFloat(w)).filter((w) => Number.isFinite(w) && w > 0);
  return widths.length ? { cols: widths.length, colW: widths[0]! } : undefined;
}

/** A grid's columns as the CSS variables the `sized-cols` rules in m3-library.css read. */
export function gridVars(cols: number, colW: number, gap: number): CSSProperties {
  return { "--grid-cols": cols, "--grid-col-w": `${colW}px`, "--grid-gap": `${gap}px` } as CSSProperties;
}

/**
 * The grid [selector] in [scroller] as gridVars for the scroller, kept as the scroller resizes: for
 * a page whose grid CSS lays out, so the groups above it take its column size.
 */
export function useGridVars(scroller: RefObject<HTMLElement | null>, selector: string): CSSProperties | undefined {
  const [vars, setVars] = useState<{ cols: number; colW: number; gap: number } | undefined>(undefined);
  const read = useCallback(() => {
    const grid = scroller.current?.querySelector<HTMLElement>(selector);
    if (!grid) return;
    const cs = getComputedStyle(grid);
    const c = resolvedColumns(cs.gridTemplateColumns);
    if (!c) return;
    const gap = Number.parseFloat(cs.columnGap) || 0;
    setVars((cur) => (cur && cur.cols === c.cols && Math.abs(cur.colW - c.colW) < 0.5 && cur.gap === gap ? cur : { ...c, gap }));
  }, [scroller, selector]);
  // After each render (the grid may only now be drawn), and as the scroller resizes.
  useLayoutEffect(read);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(read);
    ro.observe(el);
    return () => ro.disconnect();
  }, [scroller, read]);
  return vars ? gridVars(vars.cols, vars.colW, vars.gap) : undefined;
}
