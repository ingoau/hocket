// HTML5 drag of library items onto the queue: a song, album, artist or playlist
// from a table, grid or tile. The payload is an ActionTarget so Play next can
// resolve it the same way the item's menu does. While that drag is over the
// player bar, the now-playing panel or the fullscreen player, those surfaces
// show the queue (without writing the remembered view) so the drop can pick a
// place; the player comes back when the drag ends.
import { useEffect, type DragEvent as ReactDragEvent } from "react";
import type { ActionTarget } from "@core/api";
import { SK } from "@shared/settings-keys";
import { useApp } from "../store/app";
import { settingValue } from "../store/reducer";
import { queueSections, upcomingEntries } from "./queue-view";

export const LIBRARY_MIME = "application/x-hocket-library";

const KINDS = ["tracks", "albums", "artists", "playlists"] as const;
type LibraryKind = (typeof KINDS)[number];

/** The player surfaces that reveal the queue while a library drag is over them. */
const PLAYER_ZONE = "[data-testid='player-bar'], [data-testid='right-panel'], [data-testid='fullscreen-player']";
/** Drops the queue itself handles (so the document listener does not also Play next). */
const QUEUE_ZONE = "[data-testid='queue-wrap'], [data-testid='queue-timeline'], [data-testid='queue-scroll'], [data-testid='queue-empty']";
/** Leaving the player for this long hides the queue again (the gutter between bar and panel). */
const HIDE_MS = 120;

let dragging: ActionTarget | undefined;

export function parseLibraryTarget(raw: string): ActionTarget | undefined {
  try {
    const v = JSON.parse(raw) as { type?: string; data?: { ids?: unknown } };
    if (!(KINDS as readonly string[]).includes(v.type ?? "") || !Array.isArray(v.data?.ids)) return undefined;
    const ids = v.data.ids.filter((id): id is string => typeof id === "string" && id.length > 0);
    if (!ids.length) return undefined;
    return { type: v.type as LibraryKind, data: { ids } };
  } catch {
    return undefined;
  }
}

export function startLibraryDrag(e: { dataTransfer: DataTransfer | null; preventDefault: () => void }, target: ActionTarget): void {
  if (!isLibraryTarget(target) || !e.dataTransfer) {
    e.preventDefault();
    return;
  }
  dragging = target;
  e.dataTransfer.setData(LIBRARY_MIME, JSON.stringify(target));
  e.dataTransfer.effectAllowed = "copyMove";
}

export function endLibraryDrag(): void {
  dragging = undefined;
  useApp.getState().setQueueDropReveal(false);
}

export function isLibraryDrag(dt: DataTransfer | null | undefined): boolean {
  if (dragging) return true;
  if (!dt) return false;
  return Array.from(dt.types).includes(LIBRARY_MIME);
}

export function libraryDragTarget(dt: DataTransfer | null | undefined): ActionTarget | undefined {
  if (dt) {
    const raw = dt.getData(LIBRARY_MIME);
    if (raw) return parseLibraryTarget(raw);
  }
  return dragging;
}

export function isLibraryTarget(target: ActionTarget): boolean {
  return (KINDS as readonly string[]).includes(target.type) && Array.isArray((target as { data?: { ids?: unknown } }).data?.ids);
}

/** True when the drag started on a control inside the row (rating, play, a link): those keep their own pointer. */
export function dragFromControl(e: { target: EventTarget | null }): boolean {
  const el = e.target as HTMLElement | null;
  return !!el?.closest("button, a, input, textarea, select, [role='slider'], [role='radiogroup']");
}

export function libraryDragProps(target: ActionTarget | undefined): {
  draggable?: boolean;
  onDragStart?: (e: ReactDragEvent) => void;
  onDragEnd?: () => void;
} {
  if (!target || !isLibraryTarget(target)) return {};
  return {
    draggable: true,
    onDragStart: (e) => {
      if (dragFromControl(e)) {
        e.preventDefault();
        return;
      }
      startLibraryDrag(e, target);
    },
    onDragEnd: endLibraryDrag,
  };
}

/** The sortable id of the "Next from" heading: rows above it are in the "Next in queue" group. */
export const GROUP_END = "\u0000next-from";

export interface QueueMove {
  key: string;
  /** The entry it goes directly after: the current entry's key for the top of the list. */
  after: string;
  in_group: boolean;
}

/**
 * The upcoming rows as the panel sorts them: the group's keys, the "Next from" heading
 * (only with the Queue section on, else nothing can join a group), then the rest.
 */
export function sortableOrder(group: readonly string[], rest: readonly string[], section: boolean): string[] {
  return section ? [...group, GROUP_END, ...rest] : [...group, ...rest];
}

/**
 * Where a drop before `order[index]` lands: the entry it follows, and whether it joins the
 * group. Without the heading in `order`, membership is `noHeading` (true when everything
 * upcoming is the group, false with the Queue section off).
 */
export function dropSlot(order: readonly string[], index: number, currentKey: string, skip: ReadonlySet<string> = new Set(), noHeading = false): { after: string; inGroup: boolean } {
  const head = order.indexOf(GROUP_END);
  const i = Math.max(0, Math.min(order.length, index));
  let after = currentKey;
  for (let j = i - 1; j >= 0; j--) {
    const k = order[j]!;
    if (k !== GROUP_END && !skip.has(k)) {
      after = k;
      break;
    }
  }
  return { after, inGroup: head >= 0 ? i <= head : noHeading };
}

/**
 * MoveQueueItem as the core applies it, on a sortable order (with GROUP_END marking the
 * group's end): out of place, then directly after `after`; an in-group drop past the
 * group's end lands at its end, an out-of-group drop inside the group just after it.
 */
export function applyMove(order: readonly string[], move: QueueMove): string[] {
  const out = order.filter((k) => k !== move.key);
  let at = out.indexOf(move.after) + 1; // the current key isn't in the order: 0
  const head = out.indexOf(GROUP_END);
  if (head >= 0) {
    if (move.in_group && at > head) at = head;
    if (!move.in_group && at <= head) at = head + 1;
  }
  out.splice(at, 0, move.key);
  return out;
}

/**
 * The moves that put `block` (keys in queue order) together where the dragged row `active`
 * sits in `dropped` (the order with only it moved, as dnd-kit leaves it). Moves that would
 * change nothing are left out.
 */
export function placeBlock(order: readonly string[], dropped: readonly string[], active: string, block: readonly string[], currentKey: string, noHeading = false): QueueMove[] {
  const at = dropped.indexOf(active);
  if (at < 0) return [];
  const { after, inGroup } = dropSlot(dropped, at, currentKey, new Set(block), noHeading);
  const moves: QueueMove[] = [];
  let work = [...order];
  let prev = after;
  for (const key of block) {
    const m = { key, after: prev, in_group: inGroup };
    const next = applyMove(work, m);
    if (next.some((k, i) => k !== work[i])) {
      moves.push(m);
      work = next;
    }
    prev = key;
  }
  return moves;
}

/** Alt+Up/Down: one place up or down, crossing the "Next from" heading counts as a step. */
export function stepMove(order: readonly string[], key: string, dir: 1 | -1, currentKey: string, noHeading = false): QueueMove[] {
  const from = order.indexOf(key);
  const to = from + dir;
  if (from < 0 || to < 0 || to >= order.length) return [];
  const dropped = [...order];
  dropped.splice(from, 1);
  dropped.splice(to, 0, key);
  return placeBlock(order, dropped, key, [key], currentKey, noHeading);
}

function queueSectionOn(): boolean {
  return settingValue(useApp.getState(), SK.queueSection, true);
}

/**
 * A library item dropped on the player or the queue. Without a place it is added to the
 * queue (Add to queue with the Queue section on, else Play next). With one (`after`, the
 * key it follows, and whether it joins the group) the closest command does it: Play next
 * for the top, Add to queue for the group's end; anywhere else it is Played next and then
 * moved into place.
 */
export function addToQueue(target: ActionTarget, slot?: { after: string; inGroup: boolean }): void {
  if (!isLibraryTarget(target)) return;
  const app = useApp.getState();
  const section = queueSectionOn();
  const queue = app.queue;
  const currentKey = queue.current?.item.key;
  if (!slot || !currentKey) {
    app.runAction(section ? "addToQueue" : "playNext", target);
    return;
  }
  const inGroup = section && slot.inGroup;
  const { group } = queueSections(queue);
  const groupEnd = group[group.length - 1]?.item.key ?? currentKey;
  if (slot.after === currentKey && inGroup === section) {
    app.runAction("playNext", target);
    return;
  }
  if (inGroup && slot.after === groupEnd) {
    app.runAction("addToQueue", target);
    return;
  }
  const before = new Set(upcomingEntries(queue).map((e) => e.item.key));
  app.runAction("playNext", target);
  const queueBefore = queue;
  const apply = (): boolean => {
    const q = useApp.getState().queue;
    // Only the first queue change after the drop is ours to place: if it added nothing
    // (our Play next failed, or something else changed first), stop rather than later
    // moving someone else's items.
    if (q === queueBefore) return false;
    const added = upcomingEntries(q).filter((e) => !before.has(e.item.key)).map((e) => e.item.key);
    let prev = slot.after;
    for (const key of added) {
      useApp.getState().dispatch({ type: "moveQueueItem", data: { key, after: prev, in_group: inGroup } });
      prev = key;
    }
    return true;
  };
  if (apply()) return;
  const unsub = useApp.subscribe(() => {
    if (apply()) unsub();
  });
  // The queue view arrives from the core asynchronously; a busy core can be slow.
  window.setTimeout(unsub, 5_000);
}

function overSelector(x: number, y: number, selector: string): boolean {
  return !!document.elementFromPoint(x, y)?.closest(selector);
}

/**
 * While a library drag is over the player, show the queue there (the side panel
 * or the fullscreen view) without writing the remembered view. Dropping on the
 * player itself (not a row) adds it to the queue; dropping on the timeline is the panel's.
 */
export function useQueueDropReveal(): void {
  useEffect(() => {
    let hide = 0;
    const over = (e: DragEvent) => {
      if (!isLibraryDrag(e.dataTransfer)) return;
      if (overSelector(e.clientX, e.clientY, PLAYER_ZONE)) {
        e.preventDefault();
        if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
        window.clearTimeout(hide);
        useApp.getState().setQueueDropReveal(true);
      } else if (useApp.getState().queueDropReveal) {
        window.clearTimeout(hide);
        hide = window.setTimeout(() => useApp.getState().setQueueDropReveal(false), HIDE_MS);
      }
    };
    const drop = (e: DragEvent) => {
      window.clearTimeout(hide);
      const target = libraryDragTarget(e.dataTransfer);
      const onQueue = overSelector(e.clientX, e.clientY, QUEUE_ZONE);
      useApp.getState().setQueueDropReveal(false);
      if (!target || onQueue) return;
      if (overSelector(e.clientX, e.clientY, PLAYER_ZONE)) {
        e.preventDefault();
        addToQueue(target);
      }
    };
    const end = () => {
      window.clearTimeout(hide);
      endLibraryDrag();
    };
    // Capture: the queue's rows stop dragover from bubbling, and without it here a hide
    // scheduled on the way in (the gutter) would hide the queue under the pointer.
    document.addEventListener("dragover", over, true);
    document.addEventListener("drop", drop);
    document.addEventListener("dragend", end);
    return () => {
      window.clearTimeout(hide);
      document.removeEventListener("dragover", over, true);
      document.removeEventListener("drop", drop);
      document.removeEventListener("dragend", end);
    };
  }, []);
}
