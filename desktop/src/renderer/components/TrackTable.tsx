// Virtualised, keyboard-navigable, multi-select track table. Selection is
// keyed by id; Ctrl+A selects the predicate ("all matching") using `total`.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import type { ActionTarget, SortOrder, Track } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { EMPTY_SELECTION, clearSelection, isSelected, navigate, selectAll, selectOnly, selectRange, selectionCount, toggle, typeAhead, type Selection } from "../store/selection";
import { bridge } from "../core/bridge";
import { openContextMenu } from "./ContextMenu";
import { executeAction, type ActionContext } from "../store/actions";
import { fmtDate, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Heart, Stars } from "./Stars";
import { Icon } from "./Icon";
import { OfflineBadge } from "./OfflineBadge";
import type { Primer } from "../lib/use-prime";

export type ColumnId = "index" | "art" | "title" | "artist" | "album" | "duration" | "year" | "genre" | "rating" | "love" | "plays" | "added" | "bpm" | "offline";

interface Column {
  id: ColumnId;
  label: string;
  /** Header text for screen readers when the visible header is empty (icon columns). */
  sr?: string;
  width: string;
  sort?: SortOrder;
  num?: boolean;
}

const COLUMNS: Record<ColumnId, Column> = {
  index: { id: "index", label: t("col.track"), width: "40px", num: true },
  art: { id: "art", label: "", sr: t("col.art"), width: "34px" },
  title: { id: "title", label: t("col.title"), width: "minmax(140px, 3fr)", sort: "title" },
  artist: { id: "artist", label: t("col.artist"), width: "minmax(100px, 2fr)", sort: "artist" },
  album: { id: "album", label: t("col.album"), width: "minmax(100px, 2fr)", sort: "album" },
  duration: { id: "duration", label: t("col.duration"), width: "56px", sort: "duration", num: true },
  year: { id: "year", label: t("col.year"), width: "52px", sort: "year", num: true },
  genre: { id: "genre", label: t("col.genre"), width: "minmax(70px, 1fr)" },
  rating: { id: "rating", label: t("col.rating"), width: "92px", sort: "rating" },
  love: { id: "love", label: "", sr: t("col.love"), width: "30px" },
  plays: { id: "plays", label: t("col.plays"), width: "52px", sort: "playCount", num: true },
  added: { id: "added", label: t("col.added"), width: "96px", sort: "dateAdded" },
  bpm: { id: "bpm", label: t("col.bpm"), width: "52px", sort: "bpm", num: true },
  offline: { id: "offline", label: "", sr: t("col.offline"), width: "28px" },
};

/** Columns in the order they give way when the table is narrow (last goes first); the title never does. */
const COLUMN_PRIORITY: ColumnId[] = ["title", "duration", "artist", "index", "art", "album", "rating", "love", "offline", "year", "plays", "genre", "added", "bpm"];

function minWidth(c: ColumnId): number {
  return Number(/(\d+)px/.exec(COLUMNS[c].width)?.[1] ?? 40);
}

/**
 * The columns that fit `width` px (row padding and a scrollbar included):
 * the lowest-priority columns are dropped until the minimum widths fit, so
 * the table never scrolls sideways at narrow windows or 200% zoom.
 */
export function fitColumns(columns: readonly ColumnId[], width: number): ColumnId[] {
  if (!width) return [...columns];
  const kept = new Set(columns);
  // Grid track minimums include the cells' padding; 24 px row padding, 10 px scrollbar.
  const need = () => [...kept].reduce((sum, c) => sum + minWidth(c), 24 + 10);
  const dropOrder = [...columns].sort((a, b) => COLUMN_PRIORITY.indexOf(b) - COLUMN_PRIORITY.indexOf(a));
  for (const c of dropOrder) {
    if (need() <= width || kept.size <= 1) break;
    if (c !== "title") kept.delete(c);
  }
  return columns.filter((c) => kept.has(c));
}

export interface TrackTableProps {
  /** Loaded tracks (may be a window of the total when paging). */
  tracks: (Track | undefined)[];
  /** Total row count for virtualisation and select-all. */
  total: number;
  columns: ColumnId[];
  /** Unique id for this list; the selection is scoped to it. */
  scope: string;
  sort?: SortOrder;
  descending?: boolean;
  onSort?: (sort: SortOrder, descending: boolean) => void;
  /** Called with the first row index needed when the virtualiser scrolls into unloaded rows. */
  onNeedRange?: (start: number, end: number) => void;
  /** Play: from this row index within the list's context. */
  onPlay: (index: number, track: Track) => void;
  context?: ActionContext;
  /** Playlist rows can be reordered by drag. */
  onReorder?: (from: number, to: number) => void;
  /** Delete removes from the containing list (playlist) when provided. */
  onDelete?: (indices: number[], ids: string[]) => void;
  emptyMessage?: string;
  playingTrackId?: string;
  testId?: string;
  /** Accessible name of the grid (defaults to "Tracks"). */
  label?: string;
  /** Play intent (hover on a row's play button, or keyboard focus resting on a row) primes the track. */
  primer?: Primer;
}

export function TrackTable(props: TrackTableProps) {
  const { tracks, total, scope, sort, descending, onSort, onNeedRange, onPlay, context, onReorder, onDelete, playingTrackId, primer } = props;
  const parentRef = useRef<HTMLDivElement>(null);
  const tableRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = tableRef.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const columns = useMemo(() => fitColumns(props.columns, width), [props.columns, width]);
  /** Row to move DOM focus to once it is rendered (keyboard navigation). */
  const pendingFocus = useRef<number | undefined>(undefined);
  const selection = useApp((s) => (s.selectionScope === scope ? s.selection : EMPTY_SELECTION));
  const setSelection = useApp((s) => s.setSelection);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const [focusIdx, setFocusIdx] = useState(0);
  const typeBuf = useRef({ text: "", at: 0 });
  const dragFrom = useRef<number | undefined>(undefined);

  // The focused row is always rendered so the grid keeps its one Tab stop while scrolled away.
  const focusRef = useRef(focusIdx);
  focusRef.current = focusIdx;
  const rangeExtractor = useCallback((range: Range) => {
    const base = defaultRangeExtractor(range);
    const f = focusRef.current;
    return f < range.count && !base.includes(f) ? [...base, f].sort((a, b) => a - b) : base;
  }, []);
  const rowVirtualizer = useVirtualizer({ count: total, getScrollElement: () => parentRef.current, estimateSize: () => 30, overscan: 12, rangeExtractor });
  const items = rowVirtualizer.getVirtualItems();
  useEffect(() => {
    if (!onNeedRange || !items.length) return;
    const first = items[0]?.index ?? 0;
    const last = items[items.length - 1]?.index ?? 0;
    onNeedRange(first, last);
  }, [items, onNeedRange]);

  const template = useMemo(() => columns.map((c) => COLUMNS[c].width).join(" "), [columns]);
  const idAt = useCallback((i: number) => tracks[i]?.id, [tracks]);
  const loadedIds = useMemo(() => tracks.map((x) => x?.id ?? ""), [tracks]);

  const publish = useCallback((sel: Selection) => {
    setSelection(sel, scope);
    // Tell the core what the selection is so commands can snapshot it (undo restores it).
    const target: ActionTarget = sel.all ? { type: "none" } : { type: "tracks", data: { ids: [...sel.ids] } };
    bridge().dispatch({ type: "setSelection", data: { target } });
  }, [setSelection, scope]);

  const moveFocus = (i: number) => {
    setFocusIdx(i);
    pendingFocus.current = i;
  };
  useEffect(() => {
    const i = pendingFocus.current;
    if (i === undefined) return;
    const el = parentRef.current?.querySelector<HTMLElement>(`[data-index="${i}"]`);
    if (!el) return;
    pendingFocus.current = undefined;
    el.focus({ preventScroll: true });
  });

  const click = (e: React.MouseEvent, i: number) => {
    const id = idAt(i);
    if (!id) return;
    setFocusIdx(i);
    if (e.shiftKey) publish(selectRange(selection, id, loadedIds, e.ctrlKey || e.metaKey));
    else if (e.ctrlKey || e.metaKey) publish(toggle(selection.all ? { ...clearSelection(), total } : selection, id));
    else publish(selectOnly(id));
  };

  const targetFor = (i: number): ActionTarget => {
    const id = idAt(i);
    if (id && isSelected(selection, id) && selectionCount(selection) > 1 && !selection.all) return { type: "tracks", data: { ids: [...selection.ids] } };
    if (selection.all) return { type: "tracks", data: { ids: tracks.filter((x): x is Track => !!x && !selection.ids.has(x.id)).map((x) => x.id) } };
    return { type: "tracks", data: { ids: id ? [id] : [] } };
  };
  const selectedIndices = (): number[] => tracks.map((x, i) => (x && isSelected(selection, x.id) ? i : -1)).filter((i) => i >= 0);

  const onContext = (e: React.MouseEvent, i: number) => {
    const id = idAt(i);
    if (id && !isSelected(selection, id)) {
      setFocusIdx(i);
      publish(selectOnly(id));
      void openContextMenu(e, { type: "tracks", data: { ids: [id] } }, { ...context, indices: [i] });
      return;
    }
    void openContextMenu(e, targetFor(i), { ...context, indices: selectedIndices() });
  };

  const onKey = (e: React.KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      e.stopPropagation();
      publish(selectAll(total));
      // Report the count, not ids: SetSelection with a predicate is expressed as "none" + count via TrackCount by the view.
      return;
    }
    if (e.key === "Escape" && selectionCount(selection)) {
      e.stopPropagation();
      publish(clearSelection());
      return;
    }
    const next = navigate(e.key, focusIdx, total, Math.max(1, Math.floor((parentRef.current?.clientHeight ?? 300) / 30) - 1));
    if (next !== undefined) {
      e.preventDefault();
      e.stopPropagation();
      moveFocus(next);
      rowVirtualizer.scrollToIndex(next, { align: "auto" });
      const id = idAt(next);
      if (id) publish(e.shiftKey ? selectRange(selection, id, loadedIds, true) : selectOnly(id));
      return;
    }
    if (e.key === "Enter") {
      e.stopPropagation();
      const tr = tracks[focusIdx];
      if (tr) onPlay(focusIdx, tr);
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      if (onDelete && selectionCount(selection)) {
        e.stopPropagation();
        const idx = selectedIndices();
        onDelete(idx, idx.map((i) => tracks[i]?.id ?? "").filter(Boolean));
      }
      return;
    }
    if (e.key === " " && !mod) {
      // Space is play/pause globally; don't type-ahead on it.
      return;
    }
    if (/^[0-5]$/.test(e.key) && !mod && selectionCount(selection)) {
      e.stopPropagation();
      const tgt = targetFor(focusIdx);
      void executeAction(`rate.${e.key}`, tgt, context);
      return;
    }
    if (e.key.length === 1 && !mod && !e.altKey) {
      const now = Date.now();
      const buf = now - typeBuf.current.at < 800 ? typeBuf.current.text + e.key : e.key;
      typeBuf.current = { text: buf, at: now };
      const labels = tracks.map((x) => x?.title ?? "");
      const idx = typeAhead(buf, labels, buf.length === 1 ? focusIdx : focusIdx - 1);
      if (idx !== undefined) {
        e.stopPropagation();
        moveFocus(idx);
        rowVirtualizer.scrollToIndex(idx, { align: "auto" });
        const id = idAt(idx);
        if (id) publish(selectOnly(id));
      }
    }
  };

  const count = selectionCount(selection);
  const label = props.label ?? t("a11y.tracksTable");
  return (
    <div ref={tableRef} className="table" role="grid" aria-label={count ? `${label}, ${t("selection.count", { count })}` : label} aria-rowcount={total + 1} aria-colcount={columns.length} aria-multiselectable="true" data-testid={props.testId ?? "track-table"}>
      <div role="rowgroup" style={{ display: "contents" }}>
        <div className="table-head" style={{ gridTemplateColumns: template }} role="row" aria-rowindex={1}>
          {columns.map((c) => {
            const col = COLUMNS[c];
            const sortable = !!col.sort && !!onSort;
            const sorted = sortable && sort === col.sort;
            const text = col.label || (col.sr ? <span className="sr-only">{col.sr}</span> : null);
            return (
              <div key={c} className={`th ${col.num ? "num" : ""} ${sortable ? "sortable" : ""} ${sorted ? "sorted" : ""}`} role="columnheader" aria-sort={sorted ? (descending ? "descending" : "ascending") : undefined}>
                {sortable ? (
                  <button type="button" className="th-btn" onClick={() => onSort?.(col.sort as SortOrder, sorted ? !descending : false)}>
                    {text}
                    {sorted ? <Icon name={descending ? "chevronDown" : "chevronUp"} size={12} /> : null}
                  </button>
                ) : text}
              </div>
            );
          })}
        </div>
      </div>
      <div ref={parentRef} className="table-body" role="rowgroup" onKeyDown={onKey}>
        {total === 0 ? <div className="empty" role="row"><div role="gridcell" className="empty-cell"><span className="empty-icon" aria-hidden="true"><Icon name="song" size={32} /></span>{props.emptyMessage ?? t("songs.empty")}</div></div> : null}
        <div style={{ height: rowVirtualizer.getTotalSize(), position: "relative" }}>
          {items.map((vi) => {
            const tr = tracks[vi.index];
            const selected = tr ? isSelected(selection, tr.id) : false;
            const focused = vi.index === focusIdx;
            const playing = !!tr && tr.id === playingTrackId;
            return (
              <div
                key={vi.key}
                className={`tr ${selected ? "selected" : ""} ${focused ? "focused" : ""} ${playing ? "playing" : ""}`}
                role="row"
                aria-selected={selected}
                aria-rowindex={vi.index + 2}
                aria-current={playing ? "true" : undefined}
                tabIndex={focused ? 0 : -1}
                data-index={vi.index}
                style={{ gridTemplateColumns: template, transform: `translateY(${vi.start}px)`, height: vi.size }}
                onFocus={(e) => { if (e.target !== e.currentTarget) return; if (vi.index !== focusIdx) setFocusIdx(vi.index); primer?.enter(tr?.id); }}
                onBlur={(e) => { if (e.target === e.currentTarget) primer?.leave(tr?.id); }}
                onClick={(e) => click(e, vi.index)}
                onDoubleClick={() => tr && onPlay(vi.index, tr)}
                onContextMenu={(e) => onContext(e, vi.index)}
                draggable={!!onReorder}
                onDragStart={onReorder ? (e) => { dragFrom.current = vi.index; e.dataTransfer.effectAllowed = "move"; } : undefined}
                onDragOver={onReorder ? (e) => e.preventDefault() : undefined}
                onDrop={onReorder ? (e) => { e.preventDefault(); if (dragFrom.current !== undefined && dragFrom.current !== vi.index) onReorder(dragFrom.current, vi.index); dragFrom.current = undefined; } : undefined}
                data-testid="track-row"
                data-track-id={tr?.id}
              >
                {columns.map((c) => (
                  <Cell key={c} col={c} track={tr} index={vi.index} playing={playing} serverId={serverId} onPlay={onPlay} primer={primer} />
                ))}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function Cell({ col, track, index, playing, serverId, onPlay, primer }: { col: ColumnId; track: Track | undefined; index: number; playing: boolean; serverId: string; onPlay: (index: number, track: Track) => void; primer?: Primer }) {
  const navigate = useApp((s) => s.navigate);
  const cls = COLUMNS[col].num ? "td num" : "td";
  // Controls inside rows are not Tab stops: the row is (roving), and its
  // actions are on the keyboard (Enter plays, 0–5 rate, Shift+F10 menu).
  if (!track) return <div className={cls} role="gridcell">{col === "title" ? <span className="faint">…</span> : ""}</div>;
  switch (col) {
    case "index":
      return (
        <div className={`${cls} index`} role="gridcell">
          {playing ? <><Icon name="play" size={12} filled style={{ color: "var(--accent-text)" }} /><span className="sr-only">{t("a11y.playingTrack")}</span></> : <span className="n">{track.trackNumber ?? index + 1}</span>}
          {/* Pointer shortcut (Enter plays the focused row); resting on it primes the track. */}
          {!playing ? <button type="button" className="row-play" tabIndex={-1} aria-label={t("a11y.playItem", { title: track.title })} onClick={(e) => { e.stopPropagation(); onPlay(index, track); }} onDoubleClick={(e) => e.stopPropagation()} {...primer?.bind(track.id)} data-testid="row-play"><Icon name="play" size={11} filled /></button> : null}
        </div>
      );
    case "art":
      return <div className="td" role="gridcell"><Artwork id={track.coverArt} size={64} className="art" /></div>;
    case "title":
      return <div className={`${cls} title`} role="gridcell" title={track.title}>{track.title}{track.explicit ? <span className="badge" style={{ marginLeft: 6 }}><span aria-hidden="true">E</span><span className="sr-only">{t("a11y.explicit")}</span></span> : null}</div>;
    case "artist":
      return <div className={`${cls} sub`} role="gridcell"><a href="#" tabIndex={-1} onClick={(e) => { e.preventDefault(); e.stopPropagation(); if (track.artistId) navigate({ view: "artist", id: track.artistId }); }} style={{ color: "inherit", textDecoration: "none" }}>{track.artist ?? t("misc.unknownArtist")}</a></div>;
    case "album":
      return <div className={`${cls} sub`} role="gridcell"><a href="#" tabIndex={-1} onClick={(e) => { e.preventDefault(); e.stopPropagation(); if (track.albumId) navigate({ view: "album", id: track.albumId }); }} style={{ color: "inherit", textDecoration: "none" }}>{track.album ?? t("misc.unknownAlbum")}</a></div>;
    case "duration":
      return <div className={cls} role="gridcell">{fmtTime(track.durationMs)}</div>;
    case "year":
      return <div className={cls} role="gridcell">{track.year ?? ""}</div>;
    case "genre":
      return <div className={`${cls} sub`} role="gridcell">{track.genre ?? ""}</div>;
    case "rating":
      return <div className="td" role="gridcell"><Stars value={track.rating} tabbable={false} label={t("a11y.ratingOf", { title: track.title })} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [{ type: "track", data: { id: track.id } }], rating: r } })} /></div>;
    case "love":
      return <div className="td" role="gridcell"><Heart on={track.loved} size={13} tabbable={false} onToggle={() => bridge().dispatch({ type: "setLoved", data: { targets: [{ type: "track", data: { id: track.id } }], loved: !track.loved } })} /></div>;
    case "plays":
      return <div className={cls} role="gridcell">{track.playCount || ""}</div>;
    case "added":
      return <div className={`${cls} sub`} role="gridcell">{fmtDate(track.created)}</div>;
    case "bpm":
      return <div className={cls} role="gridcell">{track.sonic?.bpm ? Math.round(track.sonic.bpm) : ""}</div>;
    case "offline":
      return <div className="td" role="gridcell"><OfflineBadge state={track.offline} size={12} /></div>;
  }
  void serverId;
  return null;
}

/** Hook to keep a paged window of tracks loaded for the virtualiser. */
export function usePagedTracks(fetchPage: ((offset: number, limit: number) => Promise<{ items: Track[]; total: number }>) | null, deps: unknown[], pageSize = 200) {
  const [rows, setRows] = useState<(Track | undefined)[]>([]);
  const [total, setTotal] = useState(0);
  const loading = useRef(new Set<number>());
  const generation = useRef(0);
  const libraryVersion = useApp((s) => s.libraryVersion);
  const load = useCallback((page: number) => {
    if (!fetchPage || loading.current.has(page)) return;
    loading.current.add(page);
    const gen = generation.current;
    void fetchPage(page * pageSize, pageSize)
      .then((res) => {
        if (gen !== generation.current) return;
        setTotal(res.total);
        setRows((prev) => {
          const next = prev.length === res.total ? [...prev] : new Array<Track | undefined>(res.total).fill(undefined).map((_, i) => prev[i]);
          res.items.forEach((tr, i) => { next[page * pageSize + i] = tr; });
          return next;
        });
      })
      .finally(() => loading.current.delete(page));
  }, [fetchPage, pageSize]);
  // A new list (deps) starts empty; a library refresh keeps the rows on screen
  // and reloads the pages that were loaded, so nothing flashes or loses focus.
  const depsKey = JSON.stringify(deps);
  const lastDeps = useRef<string | undefined>(undefined);
  useEffect(() => {
    generation.current += 1;
    loading.current.clear();
    const changed = lastDeps.current !== depsKey;
    lastDeps.current = depsKey;
    if (changed) {
      setRows([]);
      setTotal(0);
      load(0);
      return;
    }
    const pages = new Set<number>([0]);
    rows.forEach((r, i) => { if (r) pages.add(Math.floor(i / pageSize)); });
    for (const p of pages) load(p);
  }, [depsKey, libraryVersion]);
  const onNeedRange = useCallback((start: number, end: number) => {
    for (let p = Math.floor(start / pageSize); p <= Math.floor(end / pageSize); p++) {
      if (rows[p * pageSize] === undefined) load(p);
    }
  }, [rows, load, pageSize]);
  return { rows, total, onNeedRange, reload: () => { generation.current += 1; loading.current.clear(); load(0); } };
}
