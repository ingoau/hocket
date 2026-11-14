// HTML5 drag of library items onto the queue: a song, album, artist or playlist
// from a table, grid or tile. The payload is an ActionTarget so Play next can
// resolve it the same way the item's menu does. While that drag is over the
// player bar, the now-playing panel or the fullscreen player, those surfaces
// show the queue (without writing the remembered view) so the drop can pick a
// place; the player comes back when the drag ends.
import { useEffect, type DragEvent as ReactDragEvent } from "react";
import type { ActionTarget, Command, QueueEntry, QueueView } from "@core/api";
import { SK } from "@shared/settings-keys";
import { useApp } from "../store/app";
import { settingValue } from "../store/reducer";
import { resolveTrackIds } from "../store/actions";
import { pointerIndex, queueSections, upcomingEntries, windowEnd } from "./queue-view";

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
 * (when `heading`: the Queue section is on or a group exists, else nothing can join one),
 * then the rest.
 */
export function sortableOrder(group: readonly string[], rest: readonly string[], heading: boolean): string[] {
  return heading ? [...group, GROUP_END, ...rest] : [...group, ...rest];
}

/**
 * The panel's sortable order and the membership of a drop when it has no heading. There is
 * a group to drop into with the Queue section on, and with it off while one still exists
 * (a Connect peer's, or one from before it was turned off): then the "Next from" heading
 * marks its end (if anything follows it), else everything upcoming is the group.
 */
export function queueDragLayout(group: readonly string[], rest: readonly string[], section: boolean, groupCount: number): { order: string[]; noHeading: boolean } {
  const hasGroup = section || groupCount > 0;
  return { order: sortableOrder(group, rest, hasGroup && rest.length > 0), noHeading: hasGroup && !rest.length };
}

/**
 * Where a drop before `order[index]` lands: the entry it follows, and whether it joins the
 * group. Without the heading in `order`, membership is `noHeading` (true when everything
 * upcoming is the group, false when there is no group and the Queue section is off).
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

/**
 * The queue as it will be once the core has applied `moves` (sent for a pointer reorder),
 * shown until its answer arrives. The new order only comes back with the next queue
 * update, a round trip later, while dnd-kit drops its drag transforms at once: without
 * this the rows snap back to the old order, then animate a second time into the new
 * one (R-20261104-74r7). Left as it is when the window doesn't start by the current
 * entry, since the group's border could then lie outside it.
 */
export function previewMoves(q: QueueView, moves: readonly QueueMove[], section: boolean): QueueView {
  if (!moves.length) return q;
  const from = pointerIndex(q) + (q.currentIndex === undefined ? 0 : 1) - q.windowStart;
  if (from < 0) return q;
  const { group, rest } = queueSections(q);
  const byKey = new Map(q.entries.slice(from).map((e) => [e.item.key, e]));
  const { order } = queueDragLayout(group.map((e) => e.item.key), rest.map((e) => e.item.key), section, q.groupCount);
  const work = moves.reduce((o, m) => (o.includes(m.key) ? applyMove(o, m) : o), order);
  const upcoming = work.filter((k) => k !== GROUP_END).map((k) => byKey.get(k));
  if (upcoming.some((e) => !e) || upcoming.length !== byKey.size) return q;
  const head = work.indexOf(GROUP_END);
  const groupCount = head >= 0 ? head : q.groupCount;
  // A moved row's membership follows where it lands (its marker reads it).
  const moved = new Set(moves.map((m) => m.key));
  const entries = (upcoming as QueueEntry[]).map((e, i) => (moved.has(e.item.key) ? { ...e, inGroup: i < groupCount } : e));
  return { ...q, entries: [...q.entries.slice(0, from), ...entries], groupCount };
}

/** Whether two views list the same entries in the same order with the same group. */
export function sameOrder(a: QueueView, b: QueueView): boolean {
  return a === b || (a.groupCount === b.groupCount && a.entries.length === b.entries.length && a.entries.every((e, i) => e.item.key === b.entries[i]!.item.key));
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

/** Where a library drop lands: the entry it follows and whether it joins the group. */
export interface DropSlot {
  after: string;
  inGroup: boolean;
}

/** How a library drop is done: a queue action that lands exactly there, else one InsertTracks. */
export type LibraryDropPlan = { action: "playNext" | "addToQueue" | "playLater" } | { insert: { after: string; in_group: boolean } };

/**
 * The command for a library drop on `queue`. Without a place (or nothing playing) it is
 * added to the queue (Add to queue with the Queue section on, else Play next). With one,
 * Play next, Add to queue or Play later when it lands exactly where that action puts
 * songs; anywhere else InsertTracks after `slot.after`.
 */
export function libraryDropPlan(queue: QueueView, section: boolean, slot?: DropSlot): LibraryDropPlan {
  const currentKey = queue.current?.item.key;
  if (!slot || !currentKey) return { action: section ? "addToQueue" : "playNext" };
  // Play next lands right after the current song, in the group with the section on or
  // when a group already exists (it joins at its head), else out of it.
  if (slot.after === currentKey && slot.inGroup === (section || queue.groupCount > 0)) return { action: "playNext" };
  const { group } = queueSections(queue);
  const groupLoaded = group.length === queue.groupCount;
  if (section && slot.inGroup && queue.groupCount > 0 && groupLoaded && slot.after === group[group.length - 1]!.item.key) return { action: "addToQueue" };
  const upcoming = upcomingEntries(queue);
  const last = upcoming[upcoming.length - 1]?.item.key ?? currentKey;
  if (!slot.inGroup && windowEnd(queue) >= queue.total && slot.after === last) return { action: "playLater" };
  return { insert: { after: slot.after, in_group: slot.inGroup } };
}

/**
 * The entry to insert after on `queue` (the latest view, maybe newer than the drop's): if
 * `after` has since gone behind the pointer (a track change), the top of the list instead,
 * since InsertTracks refuses a history entry.
 */
export function insertAnchor(queue: QueueView, after: string): string {
  const currentKey = queue.current?.item.key;
  if (!currentKey || after === currentKey) return after;
  const i = queue.entries.findIndex((e) => e.item.key === after);
  if (i >= 0 && queue.windowStart + i < pointerIndex(queue)) return currentKey;
  return after;
}

/** The InsertTracks command for `trackIds`, or nothing when there is nothing to insert or nowhere to put it. */
export function insertTracksCommand(queue: QueueView, serverId: string, trackIds: readonly string[], at: { after: string; in_group: boolean }): Command | undefined {
  if (!trackIds.length || !queue.current) return undefined;
  return { type: "insertTracks", data: { server_id: serverId, track_ids: [...trackIds], after: insertAnchor(queue, at.after), in_group: at.in_group } };
}

/** A library item dropped on the player or the queue, at `slot` when it was dropped on a place. */
export function addToQueue(target: ActionTarget, slot?: DropSlot): void {
  if (!isLibraryTarget(target)) return;
  const app = useApp.getState();
  const plan = libraryDropPlan(app.queue, queueSectionOn(), slot);
  if ("action" in plan) {
    app.runAction(plan.action, target);
    return;
  }
  void resolveTrackIds(target).then((ids) => {
    const s = useApp.getState();
    const cmd = insertTracksCommand(s.queue, s.accounts[0]?.id ?? "", ids, plan.insert);
    if (cmd) s.dispatch(cmd);
  });
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
