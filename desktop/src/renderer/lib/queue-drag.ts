// Drops on the queue, and the player surfaces that reveal it. Drags use the shared
// payload in drag.ts: library items (a song, album, artist or playlist from a table,
// grid, tile or the now-playing title) Play next, or land at a place in the
// timeline; queue rows dragged within the queue move there (copies when they can't
// move: history and the current item). While an app drag is over the player bar,
// the now-playing panel or the fullscreen player, those surfaces show the queue
// (without writing the remembered view) so the drop can pick a place; the player
// comes back when the drag ends.
import { useEffect, type DragEvent as ReactDragEvent } from "react";
import type { ActionTarget } from "@core/api";
import { useApp } from "../store/app";
import { DRAG_MIME, beginDrag, currentDrag, dragFromControl, dragSource, finishDrag, isAppDrag, onDragEnded, toDragTarget, type DragTarget } from "./drag";

export { dragFromControl };
export const LIBRARY_MIME = DRAG_MIME;

const KINDS = ["tracks", "albums", "artists", "playlists"] as const;

/** The player surfaces that reveal the queue while a library drag is over them. */
const PLAYER_ZONE = "[data-testid='player-bar'], [data-testid='right-panel'], [data-testid='fullscreen-player']";
/** Drops the queue itself handles (so the document listener does not also Play next). */
const QUEUE_ZONE = "[data-testid='queue-wrap'], [data-testid='queue-timeline'], [data-testid='queue-scroll'], [data-testid='queue-empty']";
/** Leaving the player for this long hides the queue again (the gutter between bar and panel). */
const HIDE_MS = 120;

onDragEnded(() => useApp.getState().setQueueDropReveal(false));

/** A library target (not queue items) from its wire form. */
export function parseLibraryTarget(raw: string): ActionTarget | undefined {
  try {
    const target = toDragTarget(JSON.parse(raw));
    return target && isLibraryTarget(target) ? target : undefined;
  } catch {
    return undefined;
  }
}

export function startLibraryDrag(e: { dataTransfer: DataTransfer | null; preventDefault: () => void }, target: ActionTarget, label?: string, origin?: string): void {
  const dt = toDragTarget(target);
  if (!dt) {
    e.preventDefault();
    return;
  }
  beginDrag(e, { target: dt, label, origin });
}

export function endLibraryDrag(): void {
  finishDrag();
}

export function isLibraryDrag(dt: DataTransfer | null | undefined): boolean {
  return isAppDrag(dt);
}

export function libraryDragTarget(dt: DataTransfer | null | undefined): ActionTarget | undefined {
  return currentDrag(dt)?.target;
}

export function isLibraryTarget(target: ActionTarget): boolean {
  return (KINDS as readonly string[]).includes(target.type) && Array.isArray((target as { data?: { ids?: unknown } }).data?.ids);
}

/** A library item as a drag source (tables, tiles, home and search rows). */
export function libraryDragProps(target: ActionTarget | undefined, label?: string): {
  draggable?: boolean;
  onDragStart?: (e: ReactDragEvent) => void;
  onDragEnd?: () => void;
} {
  const dt = target && isLibraryTarget(target) ? toDragTarget(target) : undefined;
  return dt ? dragSource({ target: dt, label }) : {};
}

/** arrayMove: take the item at `from` out, put it back at `to`. */
function arrayMove<T>(list: T[], from: number, to: number): void {
  const [x] = list.splice(from, 1);
  list.splice(to, 0, x as T);
}

/**
 * The `moveQueueItem` steps (each takes the key out, then puts it at `to`) that
 * move `keys` together to the gap before `insertAt` in `list`, in list order.
 * Fills the result from the front or from the back, whichever takes fewer steps
 * (a block moving up is cheapest from the front, one moving down from the back).
 */
export function reorderMoves(list: string[], keys: string[], insertAt: number): { key: string; to: number }[] {
  const moving = new Set(keys.filter((k) => list.includes(k)));
  if (!moving.size) return [];
  const block = list.filter((k) => moving.has(k));
  const rest = list.filter((k) => !moving.has(k));
  const at = Math.max(0, Math.min(insertAt, list.length));
  const before = list.slice(0, at).filter((k) => !moving.has(k)).length;
  const want = [...rest.slice(0, before), ...block, ...rest.slice(before)];
  const run = (fromFront: boolean) => {
    const cur = [...list];
    const out: { key: string; to: number }[] = [];
    for (let i = 0; i < want.length; i++) {
      const p = fromFront ? i : want.length - 1 - i;
      const k = want[p] as string;
      if (cur[p] === k) continue;
      arrayMove(cur, cur.indexOf(k), p);
      out.push({ key: k, to: p });
    }
    return out;
  };
  const a = run(true);
  const b = run(false);
  return b.length < a.length ? b : a;
}

export type QueueDropPlan = { kind: "move"; moves: { key: string; to: number }[] } | { kind: "add"; target: ActionTarget };

/**
 * What a drop on the queue does. Library items Play next (then slide to the
 * place). Queue rows that can move (Playing next and Continue playing) move
 * there; history or the current item can't, so their songs are added again.
 */
export function queueDropPlan(target: DragTarget, reorderable: string[], insertAt: number, trackIdOf: (key: string) => string | undefined): QueueDropPlan | undefined {
  if (target.type !== "queueItems") return { kind: "add", target };
  const keys = target.data.keys;
  if (keys.every((k) => reorderable.includes(k))) return { kind: "move", moves: reorderMoves(reorderable, keys, insertAt) };
  const ids = keys.map(trackIdOf).filter((id): id is string => !!id);
  return ids.length ? { kind: "add", target: { type: "tracks", data: { ids } } } : undefined;
}

/** Drop `target` on the queue at `insertAt` (the gap in Playing next + Continue playing). */
export function dropOnQueue(target: DragTarget, insertAt = 0): void {
  const q = useApp.getState().queue;
  const reorderable = [...q.playingNext, ...q.upcoming].map((e) => e.item.key);
  const all = [...q.history, ...(q.current ? [q.current] : []), ...q.playingNext, ...q.upcoming];
  const plan = queueDropPlan(target, reorderable, insertAt, (k) => all.find((e) => e.item.key === k)?.track.id);
  if (!plan) return;
  if (plan.kind === "add") {
    addToQueue(plan.target, insertAt);
    return;
  }
  const d = useApp.getState().dispatch;
  for (const m of plan.moves) d({ type: "moveQueueItem", data: { key: m.key, to_index: m.to } });
}

/**
 * After Play next the new items sit at the front of playing-next. These `to_index`
 * values (last of the block first) slide that block to `toIndex` in the combined
 * playing-next + upcoming list.
 */
export function movesToInsertBlock(addedCount: number, toIndex: number): number[] {
  if (addedCount <= 0 || toIndex <= 0) return [];
  const to: number[] = [];
  for (let i = addedCount - 1; i >= 0; i--) to.push(toIndex + i);
  return to;
}

/** Play next, then move the inserted block to `toIndex` when that is not already the front. */
export function addToQueue(target: ActionTarget, toIndex = 0): void {
  if (!isLibraryTarget(target)) return;
  const before = new Set(useApp.getState().queue.playingNext.map((e) => e.item.key));
  useApp.getState().runAction("playNext", target);
  if (toIndex <= 0) return;
  const apply = (): boolean => {
    const added = useApp.getState().queue.playingNext.filter((e) => !before.has(e.item.key));
    if (!added.length) return false;
    const dest = movesToInsertBlock(added.length, toIndex);
    const d = useApp.getState().dispatch;
    dest.forEach((to, i) => {
      const key = added[added.length - 1 - i]?.item.key;
      if (key) d({ type: "moveQueueItem", data: { key, to_index: to } });
    });
    return true;
  };
  if (apply()) return;
  const unsub = useApp.subscribe(() => {
    if (apply()) unsub();
  });
  window.setTimeout(unsub, 2000);
}

function overSelector(x: number, y: number, selector: string): boolean {
  return !!document.elementFromPoint(x, y)?.closest(selector);
}

/**
 * While a library drag is over the player, show the queue there (the side panel
 * or the fullscreen view) without writing the remembered view. Dropping on the
 * player itself (not a row) Plays next; dropping on the timeline is the panel's.
 */
export function useQueueDropReveal(): void {
  useEffect(() => {
    let hide = 0;
    const over = (e: DragEvent) => {
      if (!isAppDrag(e.dataTransfer)) return;
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
      const target = currentDrag(e.dataTransfer)?.target;
      const onQueue = overSelector(e.clientX, e.clientY, QUEUE_ZONE);
      useApp.getState().setQueueDropReveal(false);
      // A drop zone (the queue, a playlist) already took it.
      if (!target || onQueue || e.defaultPrevented) return;
      if (overSelector(e.clientX, e.clientY, PLAYER_ZONE)) {
        e.preventDefault();
        dropOnQueue(target, 0);
      }
    };
    const end = () => {
      window.clearTimeout(hide);
      finishDrag();
    };
    document.addEventListener("dragover", over);
    document.addEventListener("drop", drop);
    document.addEventListener("dragend", end);
    return () => {
      window.clearTimeout(hide);
      document.removeEventListener("dragover", over);
      document.removeEventListener("drop", drop);
      document.removeEventListener("dragend", end);
    };
  }, []);
}
