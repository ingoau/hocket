// One scrollable timeline: history above, current, "Playing next", then
// "Continuing from <context>". Drag-and-drop reorder (dnd-kit), Delete removes,
// double-click jumps. Selection keyed by queue key.
// Keyboard: a listbox with aria-activedescendant (one Tab stop); arrows move,
// Enter plays from the item, Delete removes, Alt+Up/Down reorders (the
// keyboard equivalent of dragging), Shift+F10 opens the item's menu.
import { useCallback, useMemo, useRef, useState } from "react";
import { DndContext, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import { SortableContext, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { QueueEntry } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { EMPTY_SELECTION, isSelected, selectOnly, selectRange, toggle } from "../store/selection";
import { openContextMenu } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { fmtTime } from "../lib/format";
import { EmptyState } from "./EmptyState";

const SCOPE = "queue";

export function QueuePanel({ large = false }: { large?: boolean }) {
  const queue = useApp((s) => s.queue);
  const selection = useApp((s) => (s.selectionScope === SCOPE ? s.selection : EMPTY_SELECTION));
  const setSelection = useApp((s) => s.setSelection);
  const [focusKey, setFocusKey] = useState<string | undefined>(undefined);
  const listRef = useRef<HTMLDivElement>(null);
  // Pointer drag only: the keyboard reorders with Alt+Up/Down on the listbox.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));
  const d = bridge().dispatch;

  const all = useMemo(() => [...queue.history, ...(queue.current ? [queue.current] : []), ...queue.playingNext, ...queue.upcoming], [queue]);
  const keys = useMemo(() => all.map((e) => e.item.key), [all]);
  const reorderable = useMemo(() => [...queue.playingNext, ...queue.upcoming].map((e) => e.item.key), [queue]);

  const publish = useCallback((sel: typeof selection) => {
    setSelection(sel, SCOPE);
    d({ type: "setSelection", data: { target: { type: "queueItems", data: { keys: [...sel.ids] } } } });
  }, [setSelection, d]);

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
      const from = reorderable.indexOf(focusKey);
      const to = from + (e.key === "ArrowDown" ? 1 : -1);
      if (from >= 0 && to >= 0 && to < reorderable.length) d({ type: "moveQueueItem", data: { key: focusKey, to_index: to } });
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
  const onDragEnd = (ev: DragEndEvent) => {
    const { active, over } = ev;
    if (!over || active.id === over.id) return;
    const to = reorderable.indexOf(String(over.id));
    if (to >= 0) d({ type: "moveQueueItem", data: { key: String(active.id), to_index: to } });
  };

  if (!queue.current && !all.length) return <EmptyState message={t("queue.empty")} testId="queue-empty" />;

  const idPrefix = large ? "fsq" : "q";
  const optionId = (key: string) => `${idPrefix}-${key.replace(/[^A-Za-z0-9_-]/g, "_")}`;
  const row = (e: QueueEntry, kind: "history" | "current" | "next" | "upcoming") => (
    <QueueRow key={e.item.key} id={optionId(e.item.key)} entry={e} kind={kind} large={large} selected={isSelected(selection, e.item.key)} focused={focusKey === e.item.key} sortable={kind === "next" || kind === "upcoming"}
      onClick={(ev) => click(ev, e.item.key)} onDoubleClick={() => d({ type: "jumpToQueueItem", data: { key: e.item.key } })} onContextMenu={(ev) => onContext(ev, e.item.key)} />
  );
  const group = (key: string, label: React.ReactNode, entries: QueueEntry[], kind: "history" | "current" | "next" | "upcoming") =>
    entries.length ? (
      <div role="group" aria-labelledby={`${idPrefix}-grp-${key}`} key={key}>
        <div className="queue-section" id={`${idPrefix}-grp-${key}`}>{label}</div>
        {entries.map((e) => row(e, kind))}
      </div>
    ) : null;
  const upcomingLabel = queue.contextLabel ? t("queue.continuingFrom", { context: queue.contextLabel }) : t("queue.upcoming");
  // Focus with no cursor yet starts on the current item.
  const onFocus = (e: React.FocusEvent) => {
    if (e.target !== e.currentTarget || focusKey) return;
    const k = queue.current?.item.key ?? keys[0];
    if (k) setFocusKey(k);
  };
  const activeId = focusKey && keys.includes(focusKey) ? optionId(focusKey) : undefined;
  return (
    <div className="queue-wrap">
      {/* "Clear playing next" lives outside the listbox: a listbox holds options only. */}
      {queue.playingNext.length ? <div className="queue-tools"><button type="button" className="btn sm ghost" onClick={() => d({ type: "clearInsertions" })}>{t("queue.clearInsertions")}</button></div> : null}
      <div ref={listRef} className="pane-body" role="listbox" aria-multiselectable="true" aria-label={t("queue.title")} aria-describedby={`${idPrefix}-hint`} aria-activedescendant={activeId} tabIndex={0} onKeyDown={onKey} onFocus={onFocus} data-testid="queue-timeline">
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd} accessibility={{ container: document.body }}>
          {group("history", t("queue.history"), queue.history, "history")}
          {queue.current ? group("current", t("queue.nowPlaying"), [queue.current], "current") : null}
          <SortableContext items={reorderable} strategy={verticalListSortingStrategy}>
            {group("next", t("queue.playingNext"), queue.playingNext, "next")}
            {group("upcoming", upcomingLabel, queue.upcoming, "upcoming")}
          </SortableContext>
        </DndContext>
      </div>
      {queue.totalUpcoming > queue.upcoming.length ? <div className="queue-section" style={{ textTransform: "none", fontWeight: 400 }}>{t("queue.more", { count: queue.totalUpcoming - queue.upcoming.length })}</div> : null}
      <span id={`${idPrefix}-hint`} className="sr-only">{t("a11y.queueHint")}</span>
    </div>
  );
}

function QueueRow({ id, entry, kind, large, selected, focused, sortable, onClick, onDoubleClick, onContextMenu }: { id: string; entry: QueueEntry; kind: string; large: boolean; selected: boolean; focused: boolean; sortable: boolean; onClick: (e: React.MouseEvent) => void; onDoubleClick: () => void; onContextMenu: (e: React.MouseEvent) => void }) {
  const { listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: entry.item.key, disabled: !sortable });
  const src = entry.item.source;
  const sub = src.type === "autoplay" ? t("queue.autoplayFrom", { reason: src.data.reason }) : entry.track.artist ?? "";
  return (
    <div ref={setNodeRef} style={{ transform: CSS.Transform.toString(transform), transition }} className={`qrow ${kind} ${selected ? "selected" : ""} ${isDragging ? "dragging" : ""} ${entry.item.unavailable ? "unavailable" : ""}`} id={id} role="option" aria-selected={selected} aria-current={kind === "current" ? "true" : undefined} aria-disabled={entry.item.unavailable || undefined} data-key={entry.item.key} data-testid={`queue-row-${kind}`}
      onClick={onClick} onDoubleClick={onDoubleClick} onContextMenu={onContextMenu} title={entry.item.unavailable ? t("queue.unavailable") : undefined}
      {...(focused ? { "data-focused": true } : {})}>
      {/* Pointer drag handle; the keyboard reorders with Alt+Up/Down on the list. */}
      {sortable ? <span className="grip" {...listeners} aria-hidden="true" style={{ display: "inline-flex" }}><Icon name="grip" size={14} /></span> : <span style={{ width: 14 }} aria-hidden="true" />}
      <Artwork id={entry.track.coverArt} size={64} className="art" />
      <div className="text">
        <div className="t1" style={large ? { fontSize: 15 } : undefined}>{entry.track.title}</div>
        <div className="t2">{sub}</div>
      </div>
      <span className="dur">{fmtTime(entry.track.durationMs)}</span>
      {entry.item.unavailable ? <span className="sr-only">{t("queue.unavailable")}</span> : null}
    </div>
  );
}
