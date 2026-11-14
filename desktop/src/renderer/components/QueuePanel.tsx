// The queue view: its header (the queue selector and Autoplay, QueueSelector.tsx),
// then one scrollable timeline over the flat list (design.md "Queue model"). It
// rests with the current item at the top: Now playing, "Next in queue · N" (the
// group, with Stash and Clear), then "Next from: <context>" (or "Later") for the
// rest; scrolling up reveals History (with Clear). The core sends a window around
// the pointer; scrolling near either end pages in more with QueueRange. Follows
// the current item when it changes unless the user is scrolling or working in
// the list. Drag-and-drop reorder (dnd-kit, the whole row; a row of a
// multi-selection carries the whole selection): above the "Next from" heading is
// the group, below it is not. Library items drop in the same way. Delete
// removes, double-click jumps (history too). Selection keyed by queue key.
// Keyboard: a listbox with aria-activedescendant (one Tab stop); arrows move,
// Enter plays from the item, Delete removes, Alt+Up/Down reorders (the
// keyboard equivalent of dragging), Shift+F10 opens the item's menu.
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { DndContext, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent, type DragStartEvent } from "@dnd-kit/core";
import { SortableContext, arrayMove, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { QueueEntry, QueueView } from "@core/api";
import { SK } from "@shared/settings-keys";
import { t } from "@shared/strings";
import { useApp, useSetting } from "../store/app";
import { bridge } from "../core/bridge";
import { EMPTY_SELECTION, isSelected, selectOnly, selectRange, toggle } from "../store/selection";
import { openContextMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { OfflineBadge } from "./OfflineBadge";
import { fmtTime } from "../lib/format";
import { usePrefersReducedMotion } from "../lib/media";
import { GROUP_END, addToQueue, dropSlot, endLibraryDrag, isLibraryDrag, libraryDragTarget, placeBlock, previewMoves, queueDragLayout, sameOrder, stepMove, type QueueMove } from "../lib/queue-drag";
import { queueSections, rangeChunks, restLabel, rowMarker, upcomingEntries, wantedRange, windowEnd, mergePages, pagedFits, type QueueRowKind } from "../lib/queue-view";
import { EmptyState } from "./EmptyState";
import { QueueHeader } from "./QueueSelector";

/** How long after the last scroll, wheel or pointer activity the list may follow the current item again. */
const IDLE_MS = 4000;
/** Rows paged in at a time, and how close (px) to an end of the loaded rows scrolling asks for more. */
const PAGE = 200;
const EDGE_PX = 800;

const SCOPE = "queue";

/**
 * The event's window, widened by QueueRange pages once the user scrolls past it. The wider
 * window is refetched whenever the queue changes (until then the last one stays up).
 */
function usePagedQueue(base: QueueView): { queue: QueueView; loadBefore: () => void; loadAfter: () => void } {
  const [more, setMore] = useState({ before: 0, after: 0 });
  const [paged, setPaged] = useState<QueueView | undefined>(undefined);
  const want = wantedRange(base, more.before, more.after);
  const widened = want.start < base.windowStart || want.end > windowEnd(base);
  useEffect(() => {
    if (!widened) return;
    let live = true;
    void Promise.all(rangeChunks(want.start, want.end).map((c) => bridge().query({ type: "queueRange", data: c }))).then((rs) => {
      if (!live) return;
      const merged = mergePages(base, rs.flatMap((r) => (r.type === "queue" ? [r.data] : [])));
      if (merged) setPaged(merged);
    }, () => undefined);
    return () => {
      live = false;
    };
  }, [base, widened, want.start, want.end]);
  // Until the first pages arrive the event's window is shown. Between a change and its
  // refetch the last wider window stays up only while it still describes the same list
  // around the same current entry; after a track change or a new context it would put
  // history rows where upcoming ones are, so the event's window shows until then.
  const queue = widened && paged && pagedFits(paged, base) ? paged : base;
  useEffect(() => {
    if (!base.total) setMore({ before: 0, after: 0 });
  }, [base.total]);
  const loadBefore = useCallback(() => {
    if (queue.windowStart <= 0 || queue.windowStart > want.start) return; // at the top, or still loading
    setMore((m) => ({ ...m, before: m.before + PAGE }));
  }, [queue.windowStart, want.start]);
  const loadAfter = useCallback(() => {
    const end = windowEnd(queue);
    if (end >= queue.total || end < want.end) return;
    setMore((m) => ({ ...m, after: m.after + PAGE }));
  }, [queue, want.end]);
  return { queue, loadBefore, loadAfter };
}

/** Every history key, fetching the rows the window doesn't hold. */
async function historyKeys(q: QueueView): Promise<string[]> {
  if (q.windowStart === 0) return q.entries.slice(0, q.historyCount).map((e) => e.item.key);
  const rs = await Promise.all(rangeChunks(0, q.historyCount).map((c) => bridge().query({ type: "queueRange", data: c })));
  return rs.flatMap((r) => (r.type === "queue" ? r.data.entries : [])).slice(0, q.historyCount).map((e) => e.item.key);
}

/** How long sent moves are shown ahead of the core if no queue update answers them. */
const PENDING_MS = 2000;

export function QueuePanel({ large = false }: { large?: boolean }) {
  const base = useApp((s) => s.queue);
  const { queue: paged, loadBefore, loadAfter } = usePagedQueue(base);
  const section = useSetting<boolean>(SK.queueSection, true);
  // Reorders show at once, until the queue shown here has caught up with all of them:
  // otherwise a dropped row snaps back and animates a second time (R-20261104-74r7). The
  // core answers each move with its own queue update, and a widened window refetches its
  // pages after that, so the moves stay applied on top of each newer queue (a no-op once
  // it holds them). A new current entry or length means something else changed it.
  const [pending, setPending] = useState<{ current: string | undefined; total: number; moves: QueueMove[] } | undefined>(undefined);
  const live = pending && pending.current === paged.current?.item.key && pending.total === paged.total ? pending : undefined;
  const queue = useMemo(() => (live ? previewMoves(paged, live.moves, section) : paged), [live, paged, section]);
  const caughtUp = !live || sameOrder(queue, paged);
  useEffect(() => {
    if (!pending) return;
    if (caughtUp) {
      setPending(undefined);
      return;
    }
    const id = window.setTimeout(() => setPending(undefined), PENDING_MS);
    return () => window.clearTimeout(id);
  }, [pending, caughtUp]);
  const selection = useApp((s) => (s.selectionScope === SCOPE ? s.selection : EMPTY_SELECTION));
  const setSelection = useApp((s) => s.setSelection);
  const [focusKey, setFocusKey] = useState<string | undefined>(undefined);
  const listRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  // Pointer drag the whole row (the keyboard reorders with Alt+Up/Down on the listbox).
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 8 } }));
  // Where a library drop would land: before this index of `order` (the upcoming rows and the
  // "Next from" heading). The ref is the latest dragover's answer, which the drop reads even
  // before React has rendered it.
  const [dropIndex, setDropIndexState] = useState<number | undefined>(undefined);
  const dropRef = useRef<number | undefined>(undefined);
  const setDropIndex = useCallback((i: number | undefined) => {
    dropRef.current = i;
    setDropIndexState(i);
  }, []);
  // Any library drag ending, here or anywhere else, takes the insertion line away.
  useEffect(() => {
    const clear = () => setDropIndex(undefined);
    document.addEventListener("dragend", clear);
    document.addEventListener("drop", clear);
    return () => {
      document.removeEventListener("dragend", clear);
      document.removeEventListener("drop", clear);
    };
  }, [setDropIndex]);
  // The keys travelling with a pointer reorder (more than one when it started on a selected row).
  const [dragBlock, setDragBlock] = useState<ReadonlySet<string> | undefined>(undefined);
  const d = bridge().dispatch;

  const sections = useMemo(() => queueSections(queue), [queue]);
  const currentKey = queue.current?.item.key;
  const keys = useMemo(() => queue.entries.map((e) => e.item.key), [queue]);
  const reorderable = useMemo(() => (currentKey ? upcomingEntries(queue).map((e) => e.item.key) : []), [queue, currentKey]);
  const groupKeys = useMemo(() => sections.group.map((e) => e.item.key), [sections]);
  const restKeys = useMemo(() => sections.rest.map((e) => e.item.key), [sections]);
  // The "Next from" heading is the group's border, so it sorts with the rows (when it is
  // shown: with the section on, or off while a group still exists).
  const { order, noHeading: noHeadingInGroup } = useMemo(() => (currentKey ? queueDragLayout(groupKeys, restKeys, section, queue.groupCount) : { order: [], noHeading: false }), [groupKeys, restKeys, section, queue.groupCount, currentKey]);

  const publish = useCallback((sel: typeof selection) => {
    setSelection(sel, SCOPE, "queueItems");
    d({ type: "setSelection", data: { target: { type: "queueItems", data: { keys: [...sel.ids] } } } });
  }, [setSelection, d]);

  const sendMoves = (moves: QueueMove[]) => {
    if (!moves.length) return;
    setPending((p) => ({ current: paged.current?.item.key, total: paged.total, moves: [...(p?.moves ?? []), ...moves] }));
    for (const m of moves) d({ type: "moveQueueItem", data: m });
  };

  const click = (e: React.MouseEvent, key: string) => {
    setFocusKey(key);
    if (e.shiftKey) publish(selectRange(selection, key, keys, e.ctrlKey || e.metaKey));
    else if (e.ctrlKey || e.metaKey) publish(toggle(selection, key));
    else publish(selectOnly(key));
  };
  const selectedKeys = () => keys.filter((k) => isSelected(selection, k));
  const onContext = (e: React.MouseEvent, key: string) => {
    if (!isSelected(selection, key)) publish(selectOnly(key));
    const ks = isSelected(selection, key) && selection.ids.size > 1 ? selectedKeys() : [key];
    void openContextMenu(e, { type: "queueItems", data: { keys: ks } });
  };
  const onKey = (e: React.KeyboardEvent) => {
    const idx = focusKey ? keys.indexOf(focusKey) : -1;
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown") && focusKey) {
      e.preventDefault();
      e.stopPropagation();
      if (currentKey) sendMoves(stepMove(order, focusKey, e.key === "ArrowDown" ? 1 : -1, currentKey, noHeadingInGroup));
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      const ks = selectedKeys();
      if (ks.length) { e.stopPropagation(); d({ type: "removeQueueItems", data: { keys: ks } }); publish(EMPTY_SELECTION); }
      return;
    }
    if (e.key === "Enter" && focusKey) { e.preventDefault(); e.stopPropagation(); d({ type: "jumpToQueueItem", data: { key: focusKey } }); return; }
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End") {
      e.preventDefault();
      e.stopPropagation();
      const next = e.key === "Home" ? 0 : e.key === "End" ? keys.length - 1 : Math.max(0, Math.min(keys.length - 1, idx + (e.key === "ArrowDown" ? 1 : -1)));
      const k = keys[next];
      if (k) { setFocusKey(k); publish(e.shiftKey ? selectRange(selection, k, keys, true) : selectOnly(k)); listRef.current?.querySelector<HTMLElement>(`[data-key="${k}"]`)?.scrollIntoView({ block: "nearest" }); }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") { e.preventDefault(); e.stopPropagation(); publish({ ids: new Set(reorderable), all: false, total: reorderable.length, anchor: undefined, focus: undefined }); }
  };
  /** The selected rows to move, in queue order, when `key` is one of several; else just `key`. */
  const blockFor = (key: string): string[] => {
    if (!isSelected(selection, key)) return [key];
    const block = reorderable.filter((k) => isSelected(selection, k));
    return block.length > 1 ? block : [key];
  };
  const onDragStart = (ev: DragStartEvent) => {
    const block = blockFor(String(ev.active.id));
    setDragBlock(block.length > 1 ? new Set(block) : undefined);
  };
  const onDragEnd = (ev: DragEndEvent) => {
    setDragBlock(undefined);
    const { active, over } = ev;
    if (!over || !currentKey) return;
    const key = String(active.id);
    const from = order.indexOf(key);
    const to = order.indexOf(String(over.id));
    if (from < 0 || to < 0) return;
    // Where dnd-kit leaves the dragged row decides the place and, by the side of the
    // "Next from" heading it ends up on, whether it is in the group. A selection lands
    // together there, in queue order.
    sendMoves(placeBlock(order, arrayMove(order, from, to), key, blockFor(key), currentKey, noHeadingInGroup));
  };

  // Rest with the current item at the top; follow it when it changes, unless
  // the user scrolled, pointed or focused into the list in the last few seconds.
  const reducedMotion = usePrefersReducedMotion();
  const lastTouch = useRef(0);
  const programmatic = useRef(false);
  const touched = useCallback(() => {
    if (programmatic.current) return;
    lastTouch.current = Date.now();
  }, []);
  const settled = useRef(false);
  useLayoutEffect(() => {
    const list = scrollRef.current;
    const head = listRef.current?.querySelector<HTMLElement>('[data-group="current"]');
    if (!list || !head) return;
    const first = !settled.current;
    settled.current = true;
    if (!first && (Date.now() - lastTouch.current < IDLE_MS || list.contains(document.activeElement) && document.activeElement !== list)) return;
    const top = head.getBoundingClientRect().top - list.getBoundingClientRect().top + list.scrollTop;
    if (Math.abs(list.scrollTop - top) < 2) return;
    programmatic.current = true;
    list.scrollTo({ top, behavior: first || reducedMotion ? "auto" : "smooth" });
    window.setTimeout(() => { programmatic.current = false; }, first || reducedMotion ? 50 : 600);
  }, [currentKey, reducedMotion]);
  // Page in more rows near either end of the loaded ones.
  const onScroll = () => {
    touched();
    const el = scrollRef.current;
    if (!el || programmatic.current) return;
    if (el.scrollTop < EDGE_PX) loadBefore();
    if (el.scrollHeight - el.scrollTop - el.clientHeight < EDGE_PX) loadAfter();
  };

  const onLibraryOver = (e: React.DragEvent, kind?: QueueRowKind, key?: string) => {
    if (!isLibraryDrag(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    if (kind === "history" || kind === "current") {
      setDropIndex(0);
      return;
    }
    if (!kind || !key) {
      // Between rows, over the header or below the list: below the last row is the end;
      // elsewhere the line stays where it was rather than jumping to the front.
      const rows = listRef.current?.querySelectorAll<HTMLElement>(".qrow.group, .qrow.rest");
      const last = rows?.[rows.length - 1];
      if (last && e.clientY > last.getBoundingClientRect().bottom) setDropIndex(order.length);
      else if (dropRef.current === undefined) setDropIndex(0);
      return;
    }
    const i = order.indexOf(key);
    const r = e.currentTarget.getBoundingClientRect();
    const below = e.clientY > r.top + r.height / 2;
    setDropIndex(i < 0 ? 0 : below ? i + 1 : i);
  };
  /** A section heading: History, Now playing and Next in queue mean the top; the "Next from" one is the group's end above its middle, the rest's start below. */
  const onHeadingOver = (e: React.DragEvent, kind: QueueRowKind) => {
    if (!isLibraryDrag(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    if (kind !== "rest") {
      setDropIndex(0);
      return;
    }
    const h = order.indexOf(GROUP_END);
    if (h < 0) {
      setDropIndex(groupKeys.length);
      return;
    }
    const r = e.currentTarget.getBoundingClientRect();
    setDropIndex(e.clientY > r.top + r.height / 2 ? h + 1 : h);
  };
  const onLibraryDrop = (e: React.DragEvent) => {
    const target = libraryDragTarget(e.dataTransfer);
    if (!target) return;
    e.preventDefault();
    e.stopPropagation();
    addToQueue(target, currentKey ? dropSlot(order, dropRef.current ?? 0, currentKey, undefined, noHeadingInGroup) : undefined);
    setDropIndex(undefined);
    endLibraryDrag();
  };
  const onLibraryLeave = (e: React.DragEvent) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropIndex(undefined);
  };
  /** The one insertion line: before the row at the drop index, or after the row just above the heading or the end. */
  const dropAt = (kind: QueueRowKind, key: string): "before" | "after" | undefined => {
    if (dropIndex === undefined) return undefined;
    if (kind === "current") return dropIndex === 0 && (order[0] === GROUP_END || !order.length) ? "after" : undefined;
    if (kind === "history") return undefined;
    const i = order.indexOf(key);
    if (i < 0) return undefined;
    if (dropIndex === i) return "before";
    if (dropIndex === i + 1 && (i + 1 === order.length || order[i + 1] === GROUP_END)) return "after";
    return undefined;
  };

  if (!queue.current && !queue.entries.length) {
    return (
      <div className={`queue-wrap ${large ? "large" : ""}`} data-testid="queue-wrap" onDragOver={(e) => onLibraryOver(e)} onDrop={onLibraryDrop} onDragLeave={onLibraryLeave}>
        <QueueHeader large={large} />
        <EmptyState message={t("queue.empty")} icon="queue" testId="queue-empty" />
      </div>
    );
  }

  const idPrefix = large ? "fsq" : "q";
  const header = <QueueHeader large={large} />;
  const optionId = (key: string) => `${idPrefix}-${key.replace(/[^A-Za-z0-9_-]/g, "_")}`;
  const row = (e: QueueEntry, kind: QueueRowKind) => (
    <QueueRow key={e.item.key} id={optionId(e.item.key)} entry={e} kind={kind} large={large} selected={isSelected(selection, e.item.key)} focused={focusKey === e.item.key} sortable={kind === "group" || kind === "rest"} carried={!!dragBlock?.has(e.item.key)}
      dropAt={dropAt(kind, e.item.key)} onLibraryOver={(ev) => onLibraryOver(ev, kind, e.item.key)}
      onClick={(ev) => click(ev, e.item.key)} onDoubleClick={() => d({ type: "jumpToQueueItem", data: { key: e.item.key } })} onContextMenu={(ev) => onContext(ev, e.item.key)} />
  );
  // Section headings label their group. A section's buttons can't live in the
  // listbox (it holds options only): they follow the list and are anchored
  // onto their (sticky) heading with CSS anchor positioning.
  const anchor = (key: string) => `--${idPrefix}-sec-${key}`;
  const group = (key: string, label: string, entries: QueueEntry[], kind: QueueRowKind) =>
    entries.length ? (
      <div role="group" aria-labelledby={`${idPrefix}-grp-${key}`} key={key} className={`queue-group ${kind}`} data-group={kind}>
        <SectionHeading labelId={`${idPrefix}-grp-${key}`} label={label} anchorName={anchor(key)} sortId={kind === "rest" && order.includes(GROUP_END) ? GROUP_END : undefined} onDragOver={(e) => onHeadingOver(e, kind)} />
        {entries.map((e) => row(e, kind))}
      </div>
    ) : null;
  const sectionButtons = (key: string, buttons: { label: string; short: string; testId: string; onClick: () => void }[]) => (
    <div className="queue-actions" style={{ positionAnchor: anchor(key) } as React.CSSProperties}>
      {buttons.map((b) => (
        <button key={b.testId} type="button" className="btn sm ghost queue-clear" aria-label={b.label} title={b.label} onClick={b.onClick} data-testid={b.testId}>{b.short}</button>
      ))}
    </div>
  );
  // Focus with no cursor yet starts on the current item.
  const onFocus = (e: React.FocusEvent) => {
    if (e.target !== e.currentTarget || focusKey) return;
    const k = currentKey ?? keys[0];
    if (k) setFocusKey(k);
  };
  const activeId = focusKey && keys.includes(focusKey) ? optionId(focusKey) : undefined;
  const notLoaded = queue.total - windowEnd(queue);
  return (
    <div className={`queue-wrap ${large ? "large" : ""}`} data-testid="queue-wrap" onDragOver={(e) => onLibraryOver(e)} onDrop={onLibraryDrop} onDragLeave={onLibraryLeave}>
      {header}
      <div ref={scrollRef} className="pane-body queue-scroll" onScroll={onScroll} onWheel={touched} onPointerDown={touched} data-testid="queue-scroll">
        <div ref={listRef} className="queue-list" role="listbox" aria-multiselectable="true" aria-label={t("queue.title")} aria-describedby={`${idPrefix}-hint`} aria-activedescendant={activeId} tabIndex={0} onKeyDown={onKey} onFocus={onFocus} data-testid="queue-timeline">
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragStart={onDragStart} onDragEnd={onDragEnd} onDragCancel={() => setDragBlock(undefined)} accessibility={{ container: document.body }}>
            {group("history", t("queue.history"), sections.history, "history")}
            {sections.current ? group("current", t("queue.nowPlaying"), [sections.current], "current") : null}
            <SortableContext items={order} strategy={verticalListSortingStrategy}>
              {group("next", t("queue.nextInQueueCount", { count: queue.groupCount }), sections.group, "group")}
              {group("rest", restLabel(queue), sections.rest, "rest")}
            </SortableContext>
          </DndContext>
        </div>
        {queue.historyCount ? sectionButtons("history", [{ label: t("queue.clearHistory"), short: t("queue.clearShort"), testId: "queue-clear-history", onClick: () => { void historyKeys(queue).then((ks) => { if (ks.length) d({ type: "removeQueueItems", data: { keys: ks } }); }); publish(EMPTY_SELECTION); } }]) : null}
        {queue.groupCount ? sectionButtons("next", [
          { label: t("queue.stashGroup"), short: t("queue.stashShort"), testId: "queue-stash-group", onClick: () => d({ type: "stashQueueGroup" }) },
          { label: t("queue.clearGroup"), short: t("queue.clearShort"), testId: "queue-clear-group", onClick: () => d({ type: "clearQueueGroup" }) },
        ]) : null}
        {notLoaded > 0 ? <div className="queue-more">{t("queue.more", { count: notLoaded })}</div> : null}
        {queue.autoplay ? <div className="queue-autoplay-note" data-testid="queue-autoplay-note"><Icon name="autoplay" size={18} />{t("queue.autoplayFooter")}</div> : null}
      </div>
      <span id={`${idPrefix}-hint`} className="sr-only">{t("a11y.queueHint")}</span>
    </div>
  );
}

/** A section's sticky heading. The "Next from" one sorts with the rows (`sortId`), so a drag shows which side of it a row lands on; it is never dragged itself. */
function SectionHeading({ labelId, label, anchorName, sortId, onDragOver }: { labelId: string; label: string; anchorName: string; sortId?: string; onDragOver: (e: React.DragEvent) => void }) {
  const { setNodeRef, transform, transition } = useSortable({ id: sortId ?? labelId, disabled: sortId ? { draggable: true, droppable: false } : true });
  return (
    <div ref={sortId ? setNodeRef : undefined} className="queue-section" style={{ anchorName, transform: sortId ? CSS.Transform.toString(transform) : undefined, transition: sortId ? transition : undefined } as React.CSSProperties} onDragOver={onDragOver} data-testid={sortId ? "queue-heading-rest" : undefined}>
      <div className="queue-section-text">
        <span id={labelId}>{label}</span>
      </div>
    </div>
  );
}

const QueueRow = memo(function QueueRow({ id, entry, kind, large, selected, focused, sortable, carried, dropAt, onLibraryOver, onClick, onDoubleClick, onContextMenu }: { id: string; entry: QueueEntry; kind: QueueRowKind; large: boolean; selected: boolean; focused: boolean; sortable: boolean; carried: boolean; dropAt?: "before" | "after"; onLibraryOver: (e: React.DragEvent) => void; onClick: (e: React.MouseEvent) => void; onDoubleClick: () => void; onContextMenu: (e: React.MouseEvent) => void }) {
  const { listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: entry.item.key, disabled: !sortable });
  const snoozed = useApp((s) => s.snoozes.some((x) => x.trackRef.id === entry.track.id));
  const src = entry.item.source;
  const sub = src.type === "autoplay" ? t("queue.autoplayFrom", { reason: src.data.reason }) : entry.track.artist ?? "";
  const marker = rowMarker(entry, kind);
  return (
    <div ref={setNodeRef} style={{ transform: CSS.Transform.toString(transform), transition }} className={`qrow ${kind} ${selected ? "selected" : ""} ${isDragging || carried ? "dragging" : ""} ${dropAt ? `drop-${dropAt}` : ""} ${entry.item.unavailable ? "unavailable" : ""} ${snoozed ? "snoozed" : ""}`} id={id} role="option" aria-selected={selected} aria-current={kind === "current" ? "true" : undefined} aria-disabled={entry.item.unavailable || undefined} data-key={entry.item.key} data-drop={dropAt} data-testid={`queue-row-${kind}`}
      onClick={onClick} onDoubleClick={onDoubleClick} onContextMenu={onContextMenu} onDragOver={onLibraryOver} title={entry.item.unavailable ? t("queue.unavailable") : undefined}
      {...(sortable ? listeners : {})}
      {...(focused ? { "data-focused": true } : {})}>
      <Artwork id={entry.track.coverArt} size={large ? 100 : 64} className="art" />
      <div className="text">
        <div className="t1">{entry.track.title}</div>
        <div className="t2">{sub}</div>
      </div>
      <OfflineBadge state={entry.track.offline} size={12} />
      {marker ? <span className={`mark ${marker}`} title={t(marker === "added" ? "queue.added" : "queue.autoplayed")} data-testid={`queue-mark-${marker}`}><Icon name={marker === "added" ? "queue" : "autoplay"} size={12} /><span className="sr-only">{t(marker === "added" ? "queue.added" : "queue.autoplayed")}</span></span> : null}
      <span className="dur">{fmtTime(entry.track.durationMs)}</span>
      {/* Visual hint; the whole row is the pointer drag (the keyboard uses Alt+Up/Down). */}
      {sortable ? <span className="grip" aria-hidden="true"><Icon name={large ? "dragHandle" : "grip"} size={large ? 18 : 14} /></span> : <span className="grip-space" aria-hidden="true" />}
      {entry.item.unavailable ? <span className="sr-only">{t("queue.unavailable")}</span> : null}
    </div>
  );
});
