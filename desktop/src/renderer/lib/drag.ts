// One drag-and-drop payload for the whole desktop app (HTML5 drag). Every source
// — a song row, an album or playlist tile, a home, search or sidebar item, the
// song now playing, a queue row (history too) — carries the same DragPayload: an
// ActionTarget (what is dragged, as menus and actions name it), a label for the
// drag image, and the list it came from. Every drop target — the queue, a
// playlist (its sidebar entry, its tile, its open view) — reads it with
// currentDrag() and decides with a pure accepts check. The queue's own rows
// reorder with dnd-kit (QueuePanel), and hand the same payload to a playlist
// they are dropped on (dropTargetAt). Highlights are a `data-drop-over`
// attribute (React leaves it alone), cleared when any drag ends.
import type { DragEvent as ReactDragEvent } from "react";
import type { ActionTarget, TrackRef } from "@core/api";
import { t } from "@shared/strings";

export const DRAG_MIME = "application/x-hocket-library";

export const DRAG_KINDS = ["tracks", "albums", "artists", "playlists", "queueItems"] as const;
export type DragKind = (typeof DRAG_KINDS)[number];
export type DragTarget = Extract<ActionTarget, { type: DragKind }>;

export interface DragPayload {
  target: DragTarget;
  /** What the drag image says (a title); a count is used when absent. */
  label?: string;
  /** The list it was dragged out of (a TrackTable scope like `playlist:<id>`), so that list can reorder instead of add. */
  origin?: string;
}

/** How many items a target names. */
export function targetCount(target: DragTarget): number {
  switch (target.type) {
    case "tracks":
      return target.data.tracks.length;
    case "queueItems":
      return target.data.keys.length;
    default:
      return target.data.ids.length;
  }
}

function cleanIds(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((id): id is string => typeof id === "string" && id.length > 0) : [];
}

function cleanRefs(v: unknown): TrackRef[] {
  if (!Array.isArray(v)) return [];
  return v
    .filter((r): r is TrackRef => !!r && typeof r === "object" && typeof (r as TrackRef).id === "string" && (r as TrackRef).id.length > 0 && typeof (r as TrackRef).source === "string")
    .map((r) => ({ source: r.source, id: r.id }));
}

/** A target from untrusted JSON (another window, a test): only the kinds above, with at least one item. */
export function toDragTarget(v: unknown): DragTarget | undefined {
  if (!v || typeof v !== "object") return undefined;
  const o = v as { type?: unknown; data?: { ids?: unknown; keys?: unknown; tracks?: unknown } };
  if (typeof o.type !== "string" || !(DRAG_KINDS as readonly string[]).includes(o.type)) return undefined;
  if (o.type === "queueItems") {
    const keys = cleanIds(o.data?.keys);
    return keys.length ? { type: "queueItems", data: { keys } } : undefined;
  }
  if (o.type === "tracks") {
    const tracks = cleanRefs(o.data?.tracks);
    return tracks.length ? { type: "tracks", data: { tracks } } : undefined;
  }
  const ids = cleanIds(o.data?.ids);
  return ids.length ? ({ type: o.type, data: { ids } } as DragTarget) : undefined;
}

/** The payload from its wire form. Also reads a bare ActionTarget (the format before payloads had labels). */
export function parsePayload(raw: string): DragPayload | undefined {
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return undefined;
  }
  if (!v || typeof v !== "object") return undefined;
  const o = v as { target?: unknown; label?: unknown; origin?: unknown };
  const target = toDragTarget("target" in o ? o.target : v);
  if (!target) return undefined;
  const p: DragPayload = { target };
  if (typeof o.label === "string" && o.label) p.label = o.label;
  if (typeof o.origin === "string" && o.origin) p.origin = o.origin;
  return p;
}

/** What the drag image reads: the label for one item, else a count of what is dragged. */
export function dragLabel(p: DragPayload): string {
  const n = targetCount(p.target);
  if (p.label && n <= 1) return p.label;
  switch (p.target.type) {
    case "albums":
      return n === 1 ? t("drag.album") : t("drag.albums", { count: n });
    case "artists":
      return n === 1 ? t("drag.artist") : t("drag.artists", { count: n });
    case "playlists":
      return n === 1 ? t("drag.playlist") : t("drag.playlists", { count: n });
    default:
      return n === 1 ? t("drag.song") : t("drag.songs", { count: n });
  }
}

// ---- The drag in flight ----------------------------------------------------

let dragging: DragPayload | undefined;
const beginListeners = new Set<(p: DragPayload) => void>();
const endListeners = new Set<() => void>();

/** The payload being dragged: the one this window started, else what the DataTransfer carries (readable on drop only). */
export function currentDrag(dt?: DataTransfer | null): DragPayload | undefined {
  if (dragging) return dragging;
  const raw = dt?.getData(DRAG_MIME);
  return raw ? parsePayload(raw) : undefined;
}

/** Whether an app drag is over us (on dragover another window's data can't be read, only its type). */
export function isAppDrag(dt?: DataTransfer | null): boolean {
  return !!dragging || (!!dt && Array.from(dt.types ?? []).includes(DRAG_MIME));
}

/** Called whenever a drag starts in this window. */
export function onDragBegan(fn: (p: DragPayload) => void): () => void {
  beginListeners.add(fn);
  return () => beginListeners.delete(fn);
}

/** Called when the current drag ends (dropped or not). */
export function onDragEnded(fn: () => void): () => void {
  endListeners.add(fn);
  return () => endListeners.delete(fn);
}

export function beginDrag(e: { dataTransfer: DataTransfer | null; preventDefault: () => void; currentTarget?: EventTarget | null }, p: DragPayload): void {
  if (!e.dataTransfer) {
    e.preventDefault();
    return;
  }
  dragging = p;
  e.dataTransfer.setData(DRAG_MIME, JSON.stringify(p));
  e.dataTransfer.setData("text/plain", dragLabel(p));
  e.dataTransfer.effectAllowed = "copyMove";
  setDragImage(e.dataTransfer, p);
  // dragend fires on the source even once it has left the DOM (the now-playing
  // panel turns into the queue under the pointer, a virtual row scrolls away),
  // where it no longer bubbles to React or the document: listen on the node itself.
  const src = e.currentTarget;
  if (src && typeof (src as Node).addEventListener === "function") (src as Node).addEventListener("dragend", finishDrag, { once: true });
  for (const fn of [...beginListeners]) fn(p);
}

// A safety net: no pointer events reach the page during an HTML5 drag, so one that does
// means any drag still on record is over (its dragend went somewhere we couldn't hear).
if (typeof document !== "undefined") {
  document.addEventListener("pointerdown", () => { if (dragging) finishDrag(); }, true);
}

/** Ends the drag (idempotent): the payload, highlights and every listener's state. */
export function finishDrag(): void {
  dragging = undefined;
  clearDropHighlights();
  for (const fn of [...endListeners]) fn();
}

/** The drag image: a chip naming what is dragged (the browser's default is a ghost of the whole row). */
export function dragChip(p: DragPayload): HTMLElement {
  const chip = document.createElement("div");
  chip.className = "drag-chip";
  chip.textContent = dragLabel(p);
  const n = targetCount(p.target);
  if (n > 1) {
    const badge = document.createElement("span");
    badge.className = "drag-chip-count";
    badge.textContent = String(n);
    chip.prepend(badge);
  }
  return chip;
}

function setDragImage(dt: DataTransfer, p: DragPayload): void {
  if (typeof document === "undefined" || typeof dt.setDragImage !== "function") return;
  const chip = dragChip(p);
  chip.classList.add("offscreen");
  document.body.appendChild(chip);
  dt.setDragImage(chip, -12, -12);
  // The browser snapshots it synchronously; it can go on the next task.
  window.setTimeout(() => chip.remove(), 0);
}

// ---- Sources -----------------------------------------------------------------

/** True when the drag started on a control inside the row (rating, play, a link): those keep their own pointer. `allowLinks`: links and buttons may start it. */
export function dragFromControl(e: { target: EventTarget | null; currentTarget?: EventTarget | null }, allowLinks = false): boolean {
  const el = e.target as HTMLElement | null;
  const hit = el?.closest?.(allowLinks ? "input, textarea, select, [role='slider'], [role='radiogroup']" : "button, a, input, textarea, select, [role='slider'], [role='radiogroup']");
  // The source itself may be a button or link (a tile, a sidebar entry): that is the drag, not a control inside it.
  return !!hit && hit !== e.currentTarget;
}

export interface SourceProps {
  draggable?: boolean;
  onDragStart?: (e: ReactDragEvent) => void;
  onDragEnd?: () => void;
}

/**
 * Props that make an element a drag source. `payload` may be a function so a list
 * can build it from the selection at drag start. `allowControls`: a link or button
 * may start the drag too (a sidebar entry, a now-playing title linking to its album);
 * fields, sliders and ratings still keep their own pointer.
 */
export function dragSource(payload: DragPayload | (() => DragPayload | undefined) | undefined, opts: { allowControls?: boolean } = {}): SourceProps {
  if (!payload) return {};
  return {
    draggable: true,
    onDragStart: (e) => {
      if (dragFromControl(e, opts.allowControls)) {
        e.preventDefault();
        return;
      }
      const p = typeof payload === "function" ? payload() : payload;
      if (!p) {
        e.preventDefault();
        return;
      }
      e.stopPropagation();
      beginDrag(e, p);
    },
    onDragEnd: finishDrag,
  };
}

// ---- Targets -----------------------------------------------------------------

export interface DropSpec {
  accepts: (p: DragPayload) => boolean;
  onDrop: (p: DragPayload, e: ReactDragEvent<HTMLElement>) => void;
  effect?: "copy" | "move";
}

export interface DropProps {
  onDragOver: (e: ReactDragEvent<HTMLElement>) => void;
  onDragLeave: (e: ReactDragEvent<HTMLElement>) => void;
  onDrop: (e: ReactDragEvent<HTMLElement>) => void;
}

export function clearDropHighlights(): void {
  if (typeof document === "undefined") return;
  document.querySelectorAll("[data-drop-over]").forEach((el) => el.removeAttribute("data-drop-over"));
}

/** Marks `el` as the drop target under the pointer (only one at a time). */
export function highlightDrop(el: Element | null): void {
  if (el?.hasAttribute("data-drop-over")) return;
  clearDropHighlights();
  el?.setAttribute("data-drop-over", "");
}

/**
 * Props that make an element a drop target for app drags it accepts; it shows
 * `data-drop-over` while one is over it. Zones nest (a playlist's rows inside its
 * view): the innermost that takes the event calls preventDefault, and the ones
 * around it leave a handled event alone. Events still bubble, so the document-level
 * queue reveal keeps seeing them. A drag from another window can't be read until
 * it is dropped, so it is let in on dragover and checked on drop.
 */
export function dropZone(spec: DropSpec): DropProps {
  return {
    onDragOver: (e) => {
      if (e.defaultPrevented || !isAppDrag(e.dataTransfer)) return;
      const p = currentDrag(e.dataTransfer);
      if (p && !spec.accepts(p)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = spec.effect ?? "copy";
      highlightDrop(e.currentTarget);
    },
    onDragLeave: (e) => {
      if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
      e.currentTarget.removeAttribute("data-drop-over");
    },
    onDrop: (e) => {
      e.currentTarget.removeAttribute("data-drop-over");
      if (e.defaultPrevented) return;
      const p = currentDrag(e.dataTransfer);
      if (!p || !spec.accepts(p)) return;
      e.preventDefault();
      spec.onDrop(p, e);
      finishDrag();
    },
  };
}

/** Where a drop over a row inserts: before it, or after it in its lower half. */
export function insertIndex(index: number, clientY: number, rect: { top: number; height: number }): number {
  return clientY > rect.top + rect.height / 2 ? index + 1 : index;
}
