// Virtualised, keyboard-navigable, multi-select track table. Selection is
// keyed by id; Ctrl+A selects the predicate ("all matching") using `total`.
import { Fragment, type CSSProperties, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import type { ActionTarget, SortOrder, Track, TrackRef } from "@core/api";
import { t } from "@shared/strings";
import { useApp, useOfflineDimming, useSetting } from "../store/app";
import { useLibraryVersion } from "../store/queries";
import { settledIds, type LibraryDeps } from "../store/library-changes";
import { SK } from "@shared/settings-keys";
import { useSurfaceTuning } from "../lib/gesture-prefs";
import { useSettle, useSwipe } from "../lib/swipe-hook";
import { isRtl, rowSwipeAction, runRowSwipe, SWIPE_DEFAULTS, SWIPE_KEYS, type RowSwipeAction } from "../lib/swipe-actions";
import { usePrefersReducedMotion } from "../lib/media";
import { SwipeReveal } from "./SwipeReveal";
import { EMPTY_SELECTION, clearSelection, isSelected, navigate, selectAll, selectOnly, selectRange, selectionCount, toggle, typeAhead, type Selection } from "../store/selection";
import { bridge } from "../core/bridge";
import { openContextMenu } from "./ContextMenu";
import { executeAction, type ActionContext } from "../store/actions";
import { fmtDate, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { TRACK_HEAD_H, TRACK_ROW_H } from "../lib/track-row";
import { scrollParent } from "../lib/scroll-parent";
import { Heart, Stars } from "./Stars";
import { Icon } from "./Icon";
import { OfflineBadge } from "./OfflineBadge";
import type { Primer } from "../lib/use-prime";
import { evictPages, materializePages } from "../lib/paged-window";
import { beginDrag, currentDrag, dragFromControl, finishDrag, insertIndex, isAppDrag, onDragEnded, type DragPayload } from "../lib/drag";
import { primeKey, ratingTarget, refOf } from "../lib/track-ref";
import { unavailableOffline } from "../lib/offline";

export type ColumnId = "index" | "position" | "art" | "title" | "artist" | "album" | "duration" | "year" | "genre" | "rating" | "love" | "plays" | "added" | "bpm" | "offline";

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
  /** A list's own order (a playlist's position), where "index" is the album track number. */
  position: { id: "position", label: t("col.track"), width: "40px", num: true },
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
const COLUMN_PRIORITY: ColumnId[] = ["title", "duration", "artist", "index", "position", "art", "album", "rating", "love", "offline", "year", "plays", "genre", "added", "bpm"];

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

/** Whether a track's artist is its album's: the same name, ignoring case and spacing (a track with none counts as the album's). */
export function sameArtist(track: string | null | undefined, album: string | null | undefined): boolean {
  return !track?.trim() || track.trim().toLowerCase() === (album ?? "").trim().toLowerCase();
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
  /** Songs, albums or queue rows dragged in from elsewhere (accepted by `acceptsDrop`) land at the gap before `atIndex`. */
  onDropItems?: (p: DragPayload, atIndex: number) => void;
  acceptsDrop?: (p: DragPayload) => boolean;
  /** Delete removes from the containing list (playlist) when provided. */
  onDelete?: (indices: number[], ids: string[]) => void;
  emptyMessage?: string;
  playingTrackId?: string;
  /** The track a resume ring replaces the number of, and how far through it playback got. */
  resumeTrackId?: string;
  resumeFraction?: number;
  testId?: string;
  /** Accessible name of the grid (defaults to "Tracks"). */
  label?: string;
  /** Play intent (hover on a row's play button, or keyboard focus resting on a row) primes the track. */
  primer?: Primer;
  /** An album's artist: the artist column leaves it out, naming only the tracks by someone else. */
  omitArtist?: string;
  /**
   * The table is part of a page that scrolls as a whole (album, playlist): it never scrolls itself,
   * it is as tall as its rows and virtualises against the page's scroller, so there is one scrollbar
   * however long the list.
   */
  pageScroll?: boolean;
  /** The rows are still loading: no empty message yet (`total` may already hold placeholders). */
  loading?: boolean;
}

/**
 * Where `body` starts in `scroller`'s content, kept current as anything above it (a header, an
 * image loading in) or around it resizes.
 */
function useScrollMargin(body: HTMLElement | null, scroller: HTMLElement | null): number {
  const [margin, setMargin] = useState(0);
  useLayoutEffect(() => {
    if (!body || !scroller) return;
    const measure = () => {
      const m = Math.max(0, Math.round(body.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop));
      setMargin((cur) => (cur === m ? cur : m));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(scroller);
    for (let el: Element | null = body; el && el !== scroller; el = el.parentElement) {
      ro.observe(el);
      for (let s = el.previousElementSibling; s; s = s.previousElementSibling) ro.observe(s);
    }
    return () => ro.disconnect();
  }, [body, scroller]);
  return margin;
}

export function TrackTable(props: TrackTableProps) {
  const { tracks, total, scope, sort, descending, onSort, onNeedRange, onPlay, context, onReorder, onDelete, playingTrackId, primer, pageScroll = false } = props;
  const parentRef = useRef<HTMLDivElement>(null);
  const tableRef = useRef<HTMLDivElement>(null);
  // In a page that scrolls as a whole, the rows are windowed against the page's scroller, offset by
  // everything above the table.
  const [pageScroller, setPageScroller] = useState<HTMLElement | null>(null);
  const [body, setBody] = useState<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    setBody(parentRef.current);
    setPageScroller(pageScroll && parentRef.current ? scrollParent(parentRef.current) : null);
  }, [pageScroll]);
  const scrollMargin = useScrollMargin(body, pageScroller);
  const [width, setWidth] = useState(0);
  // The body's own scrollbar, when it scrolls itself: the header stops short of it too, so the
  // column labels stay over their columns.
  const [gutter, setGutter] = useState(0);
  useLayoutEffect(() => {
    const el = tableRef.current;
    const body = parentRef.current;
    if (!el || !body) return;
    const measure = () => { setWidth(el.clientWidth); setGutter(body.offsetWidth - body.clientWidth); };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    ro.observe(body);
    return () => ro.disconnect();
  }, []);
  const columns = useMemo(() => fitColumns(props.columns, width), [props.columns, width]);
  /** Row to move DOM focus to once it is rendered (keyboard navigation). */
  const pendingFocus = useRef<number | undefined>(undefined);
  const selection = useApp((s) => (s.selectionScope === scope ? s.selection : EMPTY_SELECTION));
  const setSelection = useApp((s) => s.setSelection);
  const serverId = useApp((s) => s.accounts[0]?.id ?? "");
  const snoozedIds = useApp((s) => s.snoozes.map((x) => x.trackRef.id).join("\n"));
  const snoozed = useMemo(() => new Set(snoozedIds ? snoozedIds.split("\n") : []), [snoozedIds]);
  // Offline, what isn't on this device is skipped when this device plays it: dimmed, still selectable.
  const offline = useOfflineDimming();
  const [focusIdx, setFocusIdx] = useState(0);
  const typeBuf = useRef({ text: "", at: 0 });
  const dragFrom = useRef<number | undefined>(undefined);
  // The insertion line while something is dragged over the rows: the gap before this index.
  const [dropGap, setDropGap] = useState<number | undefined>(undefined);
  useEffect(() => onDragEnded(() => { dragFrom.current = undefined; setDropGap(undefined); }), []);
  const { onDropItems, acceptsDrop } = props;
  /** An outside drag this list takes; `true` when it is from another window and can only be read on drop. */
  const external = (e: React.DragEvent): DragPayload | true | undefined => {
    if (!onDropItems || dragFrom.current !== undefined || !isAppDrag(e.dataTransfer)) return undefined;
    const p = currentDrag(e.dataTransfer);
    if (!p) return true;
    return acceptsDrop?.(p) ?? true ? p : undefined;
  };
  const rowDragOver = (e: React.DragEvent<HTMLElement>, i: number) => {
    if (e.defaultPrevented) return;
    const from = dragFrom.current;
    if (onReorder && from !== undefined) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      setDropGap(i > from ? i + 1 : i);
      return;
    }
    if (!external(e)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    setDropGap(insertIndex(i, e.clientY, e.currentTarget.getBoundingClientRect()));
  };
  const rowDrop = (e: React.DragEvent<HTMLElement>, at: number | "end", i = 0) => {
    if (e.defaultPrevented) return;
    const from = dragFrom.current;
    if (onReorder && from !== undefined && at !== "end") {
      e.preventDefault();
      if (from !== i) onReorder(from, i);
      finishDrag();
      return;
    }
    if (!external(e)) return;
    const p = currentDrag(e.dataTransfer);
    if (!p || !onDropItems || !(acceptsDrop?.(p) ?? true)) return;
    e.preventDefault();
    onDropItems(p, at === "end" ? total : insertIndex(i, e.clientY, e.currentTarget.getBoundingClientRect()));
    finishDrag();
  };

  // The focused row is always rendered so the grid keeps its one Tab stop while scrolled away.
  const focusRef = useRef(focusIdx);
  focusRef.current = focusIdx;
  const rangeExtractor = useCallback((range: Range) => {
    const base = defaultRangeExtractor(range);
    const f = focusRef.current;
    return f < range.count && !base.includes(f) ? [...base, f].sort((a, b) => a - b) : base;
  }, []);
  const rowVirtualizer = useVirtualizer({ count: total, getScrollElement: () => (pageScroll ? pageScroller : parentRef.current), estimateSize: () => TRACK_ROW_H, overscan: pageScroll ? 10 : 6, rangeExtractor, scrollMargin: pageScroll ? scrollMargin : 0 });
  /** Where the virtualiser's offsets (counted from the scroller's top) start in the body. */
  const rowOffset = pageScroll ? scrollMargin : 0;
  const items = rowVirtualizer.getVirtualItems();

  // Two-finger swipes on a row run the swipe.list.* settings' actions on that one song (the
  // trackpad's version of Android's swipe actions; the same actions are in the row's menu).
  const swipeTuning = useSurfaceTuning("rows");
  const reducedMotion = usePrefersReducedMotion();
  const settle = useSettle(swipeTuning, reducedMotion);
  const queueSection = useSetting<boolean>(SK.queueSection, true);
  const swipeStart = useSetting<string>(SWIPE_KEYS.list.startToEnd, SWIPE_DEFAULTS.list.startToEnd);
  const swipeEnd = useSetting<string>(SWIPE_KEYS.list.endToStart, SWIPE_DEFAULTS.list.endToStart);
  /** The actions a row's swipe right and left run (right is start-to-end in left-to-right text). */
  const swipeActions = (tr: Track | undefined): { right: RowSwipeAction | null; left: RowSwipeAction | null } => {
    if (!tr) return { right: null, left: null };
    const facts = { loved: !!tr.loved, downloaded: tr.offline === "downloaded", snoozed: snoozed.has(tr.id), queueSection };
    const rtl = isRtl();
    return { right: rowSwipeAction(rtl ? swipeEnd : swipeStart, facts), left: rowSwipeAction(rtl ? swipeStart : swipeEnd, facts) };
  };
  /** The swiped row, by index and song: a row that changes under the index (the song left the list) is not it. */
  type SwipeKey = { i: number; id: string };
  const [flash, setFlash] = useState<SwipeKey | undefined>(undefined);
  const swipe = useSwipe<SwipeKey>(parentRef, {
    tuning: swipeTuning,
    begin: (e) => {
      const row = (e.target as Element | null)?.closest<HTMLElement>("[data-testid=track-row]");
      const i = row ? Number(row.dataset.index) : NaN;
      const tr = tracks[i];
      const a = swipeActions(tr);
      return tr && Number.isFinite(i) && (a.right || a.left) ? { key: { i, id: tr.id }, sides: { start: !!a.right, end: !!a.left } } : null;
    },
    onCommit: (side, _how, key) => {
      const tr = key && tracks[key.i]?.id === key.id ? tracks[key.i] : undefined;
      const a = tr ? (side > 0 ? swipeActions(tr).right : swipeActions(tr).left) : null;
      if (!tr || !a || !key) return;
      setFlash(key);
      setTimeout(() => setFlash((f) => (f === key ? undefined : f)), 450);
      // The reveal sits where the row rests; the row itself is still pushed aside.
      const body = parentRef.current;
      const anchor = body?.querySelector<HTMLElement>("[data-testid=swipe-reveal]") ?? body?.querySelector<HTMLElement>(`[data-testid=track-row][data-index="${key.i}"]`) ?? null;
      const ref = refOf(tr);
      runRowSwipe(a, { type: "tracks", data: { tracks: [ref] } }, ref, anchor, { ...context, indices: [key.i] });
    },
  });
  useEffect(() => {
    if (!onNeedRange || !items.length) return;
    const first = items[0]?.index ?? 0;
    const last = items[items.length - 1]?.index ?? 0;
    onNeedRange(first, last);
  }, [items, onNeedRange]);

  const template = useMemo(() => columns.map((c) => COLUMNS[c].width).join(" "), [columns]);
  const idAt = useCallback((i: number) => tracks[i]?.id, [tracks]);
  const loadedIds = useMemo(() => tracks.map((x) => x?.id ?? ""), [tracks]);
  // The selection is by row id; commands name songs by reference (their account too).
  const refsFor = useCallback((ids: Iterable<string>): TrackRef[] => {
    const byId = new Map<string, Track>();
    for (const x of tracks) if (x && !byId.has(x.id)) byId.set(x.id, x);
    return [...ids].map((id) => byId.get(id)).filter((x): x is Track => !!x).map(refOf);
  }, [tracks]);

  const publish = useCallback((sel: Selection) => {
    setSelection(sel, scope, "tracks", sel.all ? undefined : refsFor(sel.ids));
    // Tell the core what the selection is so commands can snapshot it (undo restores it).
    const target: ActionTarget = sel.all ? { type: "none" } : { type: "tracks", data: { tracks: refsFor(sel.ids) } };
    bridge().dispatch({ type: "setSelection", data: { target } });
  }, [setSelection, scope, refsFor]);

  // A table shown in full on its page (album, playlist) doesn't scroll itself: the page does. Keeping
  // the focused row in view and sizing Page Up/Down then go by the page's scroller instead.
  const ownScroll = () => { const el = parentRef.current; return !pageScroll && !!el && el.scrollHeight > el.clientHeight + 1; };
  const visibleHeight = () => {
    const el = parentRef.current;
    if (!el) return 300;
    if (ownScroll()) return el.clientHeight;
    let up = el.parentElement;
    while (up && !(up.scrollHeight > up.clientHeight + 1 && /(auto|scroll)/.test(getComputedStyle(up).overflowY))) up = up.parentElement;
    return Math.min(el.clientHeight, (up ?? document.documentElement).clientHeight - TRACK_HEAD_H);
  };
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
    if (!ownScroll()) el.scrollIntoView({ block: "nearest" });
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
    if (id && isSelected(selection, id) && selectionCount(selection) > 1 && !selection.all) return { type: "tracks", data: { tracks: refsFor(selection.ids) } };
    if (selection.all) return { type: "tracks", data: { tracks: tracks.filter((x): x is Track => !!x && !selection.ids.has(x.id)).map(refOf) } };
    const tr = tracks[i];
    return { type: "tracks", data: { tracks: tr ? [refOf(tr)] : [] } };
  };
  const selectedIndices = (): number[] => tracks.map((x, i) => (x && isSelected(selection, x.id) ? i : -1)).filter((i) => i >= 0);

  const onContext = (e: React.MouseEvent, i: number) => {
    const id = idAt(i);
    if (id && !isSelected(selection, id)) {
      setFocusIdx(i);
      publish(selectOnly(id));
      const tr = tracks[i];
      void openContextMenu(e, { type: "tracks", data: { tracks: tr ? [refOf(tr)] : [] } }, { ...context, indices: [i] });
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
    const next = navigate(e.key, focusIdx, total, Math.max(1, Math.floor(visibleHeight() / TRACK_ROW_H) - 1));
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
    <div ref={tableRef} className={`table${pageScroll ? " in-page" : ""}`} style={gutter ? { "--table-gutter": `${gutter}px` } as CSSProperties : undefined} role="grid" aria-label={count ? `${label}, ${t("selection.count", { count })}` : label} aria-rowcount={total + 1} aria-colcount={columns.length} aria-multiselectable="true" data-testid={props.testId ?? "track-table"}>
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
      <div ref={parentRef} className="table-body" role="rowgroup" onKeyDown={onKey}
        // Below the last row: the end of the list.
        onDragOver={onDropItems ? (e) => { if (e.defaultPrevented || !external(e)) return; e.preventDefault(); e.dataTransfer.dropEffect = "copy"; setDropGap(total); } : undefined}
        onDrop={onDropItems ? (e) => rowDrop(e, "end") : undefined}
        onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropGap(undefined); }}>
        {total === 0 && !props.loading ? <div className="empty" role="row"><div role="gridcell" className="empty-cell"><span className="empty-icon" aria-hidden="true"><Icon name="song" size={32} /></span>{props.emptyMessage ?? t("songs.empty")}</div></div> : null}
        <div style={{ height: rowVirtualizer.getTotalSize(), position: "relative" }}>
          {items.map((vi) => {
            const tr = tracks[vi.index];
            const selected = tr ? isSelected(selection, tr.id) : false;
            const focused = vi.index === focusIdx;
            const playing = !!tr && tr.id === playingTrackId;
            const faded = !!tr && snoozed.has(tr.id);
            const away = !!tr && unavailableOffline(offline, tr.offline);
            const swiped = !!tr && swipe.key?.i === vi.index && swipe.key.id === tr.id && (swipe.stage === "tracking" || swipe.settling);
            const tracking = swiped && swipe.stage === "tracking";
            const reveal = swiped && swipe.side !== 0 ? (swipe.side > 0 ? swipeActions(tr).right : swipeActions(tr).left) : null;
            return (
              <Fragment key={vi.key}>
              {reveal && swipe.side !== 0 ? <SwipeReveal action={reveal} side={swipe.side} armed={swipe.armed !== 0} style={{ top: 0, left: "var(--table-inset)", right: "var(--table-inset)", height: vi.size, transform: `translateY(${vi.start - rowOffset}px)` }} /> : null}
              <div
                className={`tr ${selected ? "selected" : ""} ${focused ? "focused" : ""} ${playing ? "playing" : ""} ${faded ? "snoozed" : ""}${away ? " offline-unavailable" : ""}${swiped ? " swiping" : ""}${flash && flash.i === vi.index && flash.id === tr?.id ? " swipe-flash" : ""}${dropGap === vi.index ? " drop-before" : ""}${dropGap === vi.index + 1 && vi.index === total - 1 ? " drop-after" : ""}`}
                role="row"
                aria-selected={selected}
                aria-rowindex={vi.index + 2}
                aria-current={playing ? "true" : undefined}
                aria-description={away ? t("misc.notAvailableOffline") : undefined}
                title={away ? t("misc.notAvailableOffline") : undefined}
                tabIndex={focused ? 0 : -1}
                data-index={vi.index}
                style={{ gridTemplateColumns: template, transform: `translateY(${vi.start - rowOffset}px)`, height: vi.size, ...(swiped ? { translate: `${tracking ? swipe.offset : 0}px 0`, transition: tracking ? "none" : settle } : null) }}
                onFocus={(e) => { if (e.target !== e.currentTarget) return; if (vi.index !== focusIdx) setFocusIdx(vi.index); primer?.enter(primeKey(tr)); }}
                onBlur={(e) => { if (e.target === e.currentTarget) primer?.leave(primeKey(tr)); }}
                onClick={(e) => click(e, vi.index)}
                onDoubleClick={() => tr && onPlay(vi.index, tr)}
                onContextMenu={(e) => onContext(e, vi.index)}
                draggable={!!tr}
                onDragStart={tr ? (e) => {
                  if (dragFromControl(e)) { e.preventDefault(); return; }
                  const target = targetFor(vi.index);
                  const n = target.type === "tracks" ? target.data.tracks.length : 0;
                  if (target.type !== "tracks" || !n) { e.preventDefault(); return; }
                  beginDrag(e, { target, label: n === 1 ? tr.title : undefined, origin: scope });
                  // A selection of several goes elsewhere as songs; reorder here is one row at a time.
                  dragFrom.current = onReorder && n === 1 ? vi.index : undefined;
                } : undefined}
                onDragEnd={finishDrag}
                onDragOver={onReorder || onDropItems ? (e) => rowDragOver(e, vi.index) : undefined}
                onDrop={onReorder || onDropItems ? (e) => rowDrop(e, vi.index, vi.index) : undefined}
                data-testid="track-row"
                data-track-id={tr?.id}
              >
                {columns.map((c) => (
                  <Cell key={c} col={c} track={tr} index={vi.index} playing={playing} serverId={serverId} onPlay={onPlay} primer={primer} omitArtist={props.omitArtist} resume={props.resumeTrackId && tr?.id === props.resumeTrackId ? (props.resumeFraction ?? 0) : undefined} />
                ))}
              </div>
              </Fragment>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function ResumeRing({ fraction }: { fraction: number }) {
  const r = 6;
  const c = 2 * Math.PI * r;
  const dash = Math.max(0, Math.min(1, fraction)) * c;
  return (
    <svg className="track-resume-ring" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="8" cy="8" r={r} fill="none" stroke="var(--border)" strokeWidth="2" />
      <circle cx="8" cy="8" r={r} fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" strokeDasharray={`${dash} ${c}`} transform="rotate(-90 8 8)" />
    </svg>
  );
}

function Cell({ col, track, index, playing, serverId, onPlay, primer, omitArtist, resume }: { col: ColumnId; track: Track | undefined; index: number; playing: boolean; serverId: string; onPlay: (index: number, track: Track) => void; primer?: Primer; omitArtist?: string; resume?: number }) {
  const navigate = useApp((s) => s.navigate);
  const cls = COLUMNS[col].num ? "td num" : "td";
  // Controls inside rows are not Tab stops: the row is (roving), and its
  // actions are on the keyboard (Enter plays, 0–5 rate, Shift+F10 menu).
  if (!track) return <div className={cls} role="gridcell">{col === "title" ? <span className="faint">…</span> : ""}</div>;
  switch (col) {
    case "index":
    case "position":
      return (
        <div className={`${cls} index`} role="gridcell">
          {playing ? <><Icon name="play" size={12} filled style={{ color: "var(--accent-text)" }} /><span className="sr-only">{t("a11y.playingTrack")}</span></> : resume !== undefined ? <><ResumeRing fraction={resume} /><span className="sr-only">{col === "position" ? index + 1 : (track.trackNumber ?? index + 1)}</span></> : <span className="n">{col === "position" ? index + 1 : (track.trackNumber ?? index + 1)}</span>}
          {/* Pointer shortcut (Enter plays the focused row); resting on it primes the track. */}
          {!playing ? <button type="button" className="row-play" tabIndex={-1} aria-label={t("a11y.playItem", { title: track.title })} onClick={(e) => { e.stopPropagation(); onPlay(index, track); }} onDoubleClick={(e) => e.stopPropagation()} {...primer?.bind(primeKey(track))} data-testid="row-play"><Icon name="play" size={11} filled /></button> : null}
        </div>
      );
    case "art":
      return <div className="td" role="gridcell"><Artwork id={track.coverArt} size={64} className="art" dwell /></div>;
    case "title":
      return <div className={`${cls} title`} role="gridcell" title={track.title}>{track.title}{track.explicit ? <span className="badge" style={{ marginLeft: 6 }}><span aria-hidden="true">E</span><span className="sr-only">{t("a11y.explicit")}</span></span> : null}</div>;
    case "artist":
      if (omitArtist !== undefined && sameArtist(track.artist, omitArtist)) return <div className={`${cls} sub`} role="gridcell" />;
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
      return <div className="td" role="gridcell"><Stars value={track.rating} tabbable={false} label={t("a11y.ratingOf", { title: track.title })} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [ratingTarget(track)], rating: r } })} /></div>;
    case "love":
      return <div className="td" role="gridcell"><Heart on={track.loved} size={13} tabbable={false} onToggle={() => bridge().dispatch({ type: "setLoved", data: { targets: [ratingTarget(track)], loved: !track.loved } })} /></div>;
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
export function usePagedTracks(fetchPage: ((offset: number, limit: number) => Promise<{ items: Track[]; total: number }>) | null, deps: unknown[], library?: LibraryDeps, pageSize = 200) {
  const [rows, setRows] = useState<(Track | undefined)[]>([]);
  const [total, setTotal] = useState(0);
  // Whether this list's first answer is in (until then `total` 0 is "not known yet", not "empty").
  const [loaded, setLoaded] = useState(false);
  const loading = useRef(new Set<number>());
  const generation = useRef(0);
  const pages = useRef(new Map<number, Track[]>());
  const around = useRef(0);
  // Refetched when a library change touches what the list reads (`library`, anything by default).
  const libraryVersion = useLibraryVersion(library, () => settledIds(loading.current.size > 0, () => [...pages.current.values()].flatMap((p) => p.map((t) => t.id))));
  const load = useCallback((page: number) => {
    if (!fetchPage || loading.current.has(page) || pages.current.has(page)) return;
    loading.current.add(page);
    const gen = generation.current;
    void fetchPage(page * pageSize, pageSize)
      .then((res) => {
        if (gen !== generation.current) return;
        pages.current.set(page, res.items);
        evictPages(pages.current, around.current);
        setTotal(res.total);
        setLoaded(true);
        setRows(materializePages(pages.current, res.total, pageSize));
      })
      .finally(() => {
        // A page from before a reload must not mark the reload's request for it done.
        if (gen === generation.current) loading.current.delete(page);
      });
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
      pages.current = new Map();
      around.current = 0;
      setRows([]);
      setTotal(0);
      setLoaded(false);
      load(0);
      return;
    }
    const keep = [...pages.current.keys()];
    pages.current = new Map();
    for (const p of keep.length ? keep : [0]) load(p);
  }, [depsKey, libraryVersion]);
  const onNeedRange = useCallback((start: number, end: number) => {
    around.current = Math.floor(((start + end) / 2) / pageSize);
    for (let p = Math.floor(start / pageSize); p <= Math.floor(end / pageSize); p++) {
      if (!pages.current.has(p)) load(p);
    }
  }, [load, pageSize]);
  return { rows, total, loaded, onNeedRange, reload: () => { generation.current += 1; loading.current.clear(); pages.current = new Map(); load(0); } };
}
