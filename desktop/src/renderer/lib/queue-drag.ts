// HTML5 drag of library items onto the queue: a song, album, artist or playlist
// from a table, grid or tile. The payload is an ActionTarget so Play next can
// resolve it the same way the item's menu does. While that drag is over the
// player bar, the now-playing panel or the fullscreen player, those surfaces
// show the queue (without writing the remembered view) so the drop can pick a
// place; the player comes back when the drag ends.
import { useEffect, type DragEvent as ReactDragEvent } from "react";
import type { ActionTarget } from "@core/api";
import { useApp } from "../store/app";

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
        addToQueue(target, 0);
      }
    };
    const end = () => {
      window.clearTimeout(hide);
      endLibraryDrag();
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
