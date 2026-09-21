// One scrollable timeline: history above, current, "Playing next", then
// "Continuing from <context>". Drag-and-drop reorder (dnd-kit), Delete removes,
// double-click jumps. Selection keyed by queue key.
import { useCallback, useMemo, useRef, useState } from "react";
import { DndContext, KeyboardSensor, PointerSensor, closestCenter, useSensor, useSensors, type DragEndEvent } from "@dnd-kit/core";
import { SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from "@dnd-kit/sortable";
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
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }), useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }));
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
    if (e.key === "Delete" || e.key === "Backspace") {
      const ks = selectedKeys();
      if (ks.length) { e.stopPropagation(); d({ type: "removeQueueItems", data: { keys: ks } }); publish(EMPTY_SELECTION); }
      return;
    }
    if (e.key === "Enter" && focusKey) { e.stopPropagation(); d({ type: "jumpToQueueItem", data: { key: focusKey } }); return; }
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

  const row = (e: QueueEntry, kind: "history" | "current" | "next" | "upcoming") => (
    <QueueRow key={e.item.key} entry={e} kind={kind} large={large} selected={isSelected(selection, e.item.key)} focused={focusKey === e.item.key} sortable={kind === "next" || kind === "upcoming"}
      onClick={(ev) => click(ev, e.item.key)} onDoubleClick={() => d({ type: "jumpToQueueItem", data: { key: e.item.key } })} onContextMenu={(ev) => onContext(ev, e.item.key)} />
  );
  return (
    <div ref={listRef} className="pane-body" role="listbox" aria-multiselectable="true" aria-label={t("queue.title")} tabIndex={0} onKeyDown={onKey} data-testid="queue-timeline">
      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
        {queue.history.length ? <div className="queue-section">{t("queue.history")}</div> : null}
        {queue.history.map((e) => row(e, "history"))}
        {queue.current ? <div className="queue-section">{t("queue.nowPlaying")}</div> : null}
        {queue.current ? row(queue.current, "current") : null}
        <SortableContext items={reorderable} strategy={verticalListSortingStrategy}>
          {queue.playingNext.length ? <div className="queue-section row"><span className="grow">{t("queue.playingNext")}</span><button type="button" className="btn sm ghost" onClick={() => d({ type: "clearInsertions" })}>{t("queue.clearInsertions")}</button></div> : null}
          {queue.playingNext.map((e) => row(e, "next"))}
          {queue.upcoming.length ? <div className="queue-section">{queue.contextLabel ? t("queue.continuingFrom", { context: queue.contextLabel }) : t("queue.upcoming")}</div> : null}
          {queue.upcoming.map((e) => row(e, "upcoming"))}
        </SortableContext>
        {queue.totalUpcoming > queue.upcoming.length ? <div className="queue-section" style={{ textTransform: "none", fontWeight: 400 }}>{t("queue.more", { count: queue.totalUpcoming - queue.upcoming.length })}</div> : null}
      </DndContext>
    </div>
  );
}

function QueueRow({ entry, kind, large, selected, focused, sortable, onClick, onDoubleClick, onContextMenu }: { entry: QueueEntry; kind: string; large: boolean; selected: boolean; focused: boolean; sortable: boolean; onClick: (e: React.MouseEvent) => void; onDoubleClick: () => void; onContextMenu: (e: React.MouseEvent) => void }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: entry.item.key, disabled: !sortable });
  const src = entry.item.source;
  const sub = src.type === "autoplay" ? t("queue.autoplayFrom", { reason: src.data.reason }) : entry.track.artist ?? "";
  return (
    <div ref={setNodeRef} style={{ transform: CSS.Transform.toString(transform), transition }} className={`qrow ${kind} ${selected ? "selected" : ""} ${isDragging ? "dragging" : ""} ${entry.item.unavailable ? "unavailable" : ""}`} role="option" aria-selected={selected} data-key={entry.item.key} data-testid={`queue-row-${kind}`} tabIndex={-1}
      onClick={onClick} onDoubleClick={onDoubleClick} onContextMenu={onContextMenu} title={entry.item.unavailable ? t("queue.unavailable") : undefined}
      {...(focused ? { "data-focused": true } : {})}>
      {sortable ? <span className="grip" {...attributes} {...listeners} aria-label="Drag to reorder" style={{ display: "inline-flex" }}><Icon name="grip" size={14} /></span> : <span style={{ width: 14 }} />}
      <Artwork id={entry.track.coverArt} size={64} className="art" />
      <div className="text">
        <div className="t1" style={large ? { fontSize: 15 } : undefined}>{entry.track.title}</div>
        <div className="t2">{sub}</div>
      </div>
      <span className="dur">{fmtTime(entry.track.durationMs)}</span>
    </div>
  );
}
