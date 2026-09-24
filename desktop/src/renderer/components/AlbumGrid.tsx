// Virtualised album/artist/playlist grid with keyboard navigation and selection.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { EMPTY_SELECTION, clearSelection, isSelected, selectAll, selectOnly, selectRange, selectionCount, toggle, typeAhead } from "../store/selection";
import { openContextMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { usePlayIntent } from "../lib/use-prime";

export interface GridItem {
  id: string;
  title: string;
  subtitle?: string;
  coverArt?: string;
  round?: boolean;
  /** Icon name shown in the tile corner (e.g. "download" for offline items). */
  badge?: string;
}

export interface AlbumGridProps {
  items: (GridItem | undefined)[];
  total: number;
  scope: string;
  targetKind: "albums" | "artists" | "playlists";
  onOpen: (item: GridItem) => void;
  onPlay?: (item: GridItem) => void;
  onNeedRange?: (start: number, end: number) => void;
  tileWidth?: number;
  emptyMessage: string;
  testId?: string;
  /** Accessible name of the grid. */
  label: string;
}

export function AlbumGrid({ items, total, scope, targetKind, onOpen, onPlay, onNeedRange, tileWidth = 160, emptyMessage, testId, label }: AlbumGridProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [cols, setCols] = useState(4);
  const [colW, setColW] = useState(tileWidth);
  const selection = useApp((s) => (s.selectionScope === scope ? s.selection : EMPTY_SELECTION));
  const setSelection = useApp((s) => s.setSelection);
  const [focusIdx, setFocusIdx] = useState(0);
  const typeBuf = useRef({ text: "", at: 0 });
  // An album tile's play button primes the album when the pointer rests on it
  // (never when the grid scrolls under a resting pointer: see lib/prime.ts).
  const primer = usePlayIntent(scope, "album");
  const intent = (id: string) => (targetKind === "albums" ? primer.bind(id) : undefined);

  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const cs = getComputedStyle(el);
      const inner = el.clientWidth - (Number.parseFloat(cs.paddingLeft) || 0) - (Number.parseFloat(cs.paddingRight) || 0);
      const n = Math.max(1, Math.floor((el.clientWidth - 40) / (tileWidth + 14)));
      setCols(n);
      // Tiles stretch to fill the row, so the row height follows the real column width.
      setColW(Math.max(tileWidth, Math.floor((inner - (n - 1) * 14) / n)));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [tileWidth]);

  const rowCount = Math.ceil(total / cols);
  // Tile: 6px padding, square art, 6px gap, title, 6px gap, subtitle, 6px padding; plus the row's 4px padding and breathing room.
  const rowH = colW + 70;
  // The focused tile's row is always rendered so the grid keeps its one Tab stop.
  const focusRow = useRef(0);
  focusRow.current = Math.floor(focusIdx / cols);
  const rangeExtractor = useCallback((range: Range) => {
    const base = defaultRangeExtractor(range);
    const f = focusRow.current;
    return f < range.count && !base.includes(f) ? [...base, f].sort((a, b) => a - b) : base;
  }, []);
  const virt = useVirtualizer({ count: rowCount, getScrollElement: () => parentRef.current, estimateSize: () => rowH, overscan: 3, rangeExtractor });
  const pendingFocus = useRef<number | undefined>(undefined);
  useEffect(() => {
    const i = pendingFocus.current;
    if (i === undefined) return;
    const el = parentRef.current?.querySelector<HTMLElement>(`[data-index="${i}"]`);
    if (!el) return;
    pendingFocus.current = undefined;
    el.focus({ preventScroll: true });
  });
  useEffect(() => virt.measure(), [rowH, virt]);
  const vrows = virt.getVirtualItems();
  useEffect(() => {
    if (!onNeedRange || !vrows.length) return;
    onNeedRange((vrows[0]?.index ?? 0) * cols, ((vrows[vrows.length - 1]?.index ?? 0) + 1) * cols - 1);
  }, [vrows, cols, onNeedRange]);
  const ids = useMemo(() => items.map((x) => x?.id ?? ""), [items]);

  const publish = useCallback((sel: typeof selection) => setSelection(sel, scope), [setSelection, scope]);
  const click = (e: React.MouseEvent, i: number, it: GridItem) => {
    setFocusIdx(i);
    if (e.shiftKey) publish(selectRange(selection, it.id, ids, e.ctrlKey || e.metaKey));
    else if (e.ctrlKey || e.metaKey) publish(toggle(selection, it.id));
    else publish(selectOnly(it.id));
  };
  const targetFor = (it: GridItem): ActionTarget => {
    if (isSelected(selection, it.id) && selectionCount(selection) > 1 && !selection.all) return { type: targetKind, data: { ids: [...selection.ids] } } as ActionTarget;
    return { type: targetKind, data: { ids: [it.id] } } as ActionTarget;
  };
  const onKey = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "a") { e.preventDefault(); e.stopPropagation(); publish(selectAll(total)); return; }
    if (e.key === "Escape" && selectionCount(selection)) { e.stopPropagation(); publish(clearSelection()); return; }
    let next: number | undefined;
    if (e.key === "ArrowRight") next = Math.min(total - 1, focusIdx + 1);
    else if (e.key === "ArrowLeft") next = Math.max(0, focusIdx - 1);
    else if (e.key === "ArrowDown") next = Math.min(total - 1, focusIdx + cols);
    else if (e.key === "ArrowUp") next = Math.max(0, focusIdx - cols);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = total - 1;
    else if (e.key === "PageDown") next = Math.min(total - 1, focusIdx + cols * 3);
    else if (e.key === "PageUp") next = Math.max(0, focusIdx - cols * 3);
    if (next !== undefined) {
      e.preventDefault();
      e.stopPropagation();
      setFocusIdx(next);
      pendingFocus.current = next;
      virt.scrollToIndex(Math.floor(next / cols), { align: "auto" });
      const it = items[next];
      if (it) publish(e.shiftKey ? selectRange(selection, it.id, ids, true) : selectOnly(it.id));
      return;
    }
    if (e.key === "Enter") { const it = items[focusIdx]; if (it) { e.stopPropagation(); onOpen(it); } return; }
    if (e.key.length === 1 && !mod && !e.altKey && e.key !== " ") {
      const now = Date.now();
      const buf = now - typeBuf.current.at < 800 ? typeBuf.current.text + e.key : e.key;
      typeBuf.current = { text: buf, at: now };
      const idx = typeAhead(buf, items.map((x) => x?.title ?? ""), buf.length === 1 ? focusIdx : focusIdx - 1);
      if (idx !== undefined) { e.stopPropagation(); setFocusIdx(idx); pendingFocus.current = idx; virt.scrollToIndex(Math.floor(idx / cols)); const it = items[idx]; if (it) publish(selectOnly(it.id)); }
    }
  };

  return (
    <div ref={parentRef} className="view-body" role="grid" aria-label={label} aria-multiselectable="true" aria-rowcount={rowCount} aria-colcount={cols} onKeyDown={onKey} data-testid={testId ?? "grid"} style={{ "--tile-w": `${tileWidth}px` } as React.CSSProperties}>
      {total === 0 ? <div className="empty" role="row"><div role="gridcell">{emptyMessage}</div></div> : null}
      <div style={{ height: virt.getTotalSize(), position: "relative" }}>
        {vrows.map((vr) => (
          <div key={vr.key} role="row" aria-rowindex={vr.index + 1} style={{ position: "absolute", top: 0, left: 0, right: 0, transform: `translateY(${vr.start}px)`, height: vr.size, display: "grid", gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, gap: "16px 14px", padding: "4px 0" }}>
            {Array.from({ length: cols }, (_, c) => {
              const i = vr.index * cols + c;
              if (i >= total) return <div key={c} role="presentation" />;
              const it = items[i];
              if (!it) return <div key={c} className="tile" role="gridcell" aria-busy="true" tabIndex={focusIdx === i ? 0 : -1} data-index={i}><div className="art" /><div className="t1 faint">…</div></div>;
              const selected = isSelected(selection, it.id);
              return (
                <div
                  // Keyed by column, like the placeholder it replaces, so a tile that loads while focused keeps focus.
                  key={c}
                  className={`tile ${selected ? "selected" : ""} ${focusIdx === i ? "focused" : ""}`}
                  role="gridcell"
                  aria-selected={selected}
                  aria-label={it.subtitle ? `${it.title}, ${it.subtitle}` : it.title}
                  aria-colindex={c + 1}
                  tabIndex={focusIdx === i ? 0 : -1}
                  data-index={i}
                  onFocus={(e) => { if (e.target === e.currentTarget && focusIdx !== i) setFocusIdx(i); }}
                  onClick={(e) => click(e, i, it)}
                  onDoubleClick={() => onOpen(it)}
                  onContextMenu={(e) => { if (!isSelected(selection, it.id)) publish(selectOnly(it.id)); void openContextMenu(e, targetFor(it)); }}
                  data-testid="grid-tile"
                  data-id={it.id}
                >
                  <Artwork id={it.coverArt} size={300} round={it.round} alt="" />
                  <div className="t1" title={it.title}>{it.title}</div>
                  {it.subtitle ? <div className="t2" title={it.subtitle}>{it.subtitle}</div> : null}
                  {it.badge ? <span className="badge tile-badge" style={{ position: "absolute", top: 10, left: 10 }}><Icon name={it.badge} size={11} title={t("a11y.downloaded")} /></span> : null}
                  {/* Not a Tab stop: Enter opens, Shift+F10 has Play; the pointer gets the shortcut button. */}
                  {onPlay ? <button type="button" className="play" tabIndex={-1} aria-label={t("a11y.playItem", { title: it.title })} onClick={(e) => { e.stopPropagation(); onPlay(it); }} {...intent(it.id)}><Icon name="play" size={16} filled /></button> : null}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
