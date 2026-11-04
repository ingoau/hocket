// Reading the windowed QueueView (design.md "Queue model"): one flat list with a pointer.
// Rows `0..historyCount` are history, `currentIndex` is the current entry, the next
// `groupCount` are the "Next in queue" group and the rest follow. `entries` is a window
// onto that list starting at `windowStart`; these helpers split it into the panel's
// sections, and merge pages fetched with `Query::QueueRange` into one wider window.
import type { QueueEntry, QueueView } from "@core/api";
import { t } from "@shared/strings";

export type QueueRowKind = "history" | "current" | "group" | "rest";

export interface QueueSections {
  history: QueueEntry[];
  current: QueueEntry | undefined;
  /** The "Next in queue" group. */
  group: QueueEntry[];
  /** Everything after the group. */
  rest: QueueEntry[];
}

/** The list index of the pointer: the current entry, or where it would be. */
export function pointerIndex(q: QueueView): number {
  return q.currentIndex ?? q.historyCount;
}

/** One past the list index of the window's last row. */
export function windowEnd(q: QueueView): number {
  return q.windowStart + q.entries.length;
}

export function rowKind(q: QueueView, index: number): QueueRowKind {
  if (index < q.historyCount) return "history";
  if (q.currentIndex !== undefined && index === q.currentIndex) return "current";
  const p = pointerIndex(q);
  return index > p && index <= p + q.groupCount ? "group" : "rest";
}

export function queueSections(q: QueueView): QueueSections {
  const out: QueueSections = { history: [], current: undefined, group: [], rest: [] };
  q.entries.forEach((e, i) => {
    const kind = rowKind(q, q.windowStart + i);
    if (kind === "current") out.current = e;
    else out[kind].push(e);
  });
  if (!out.current && q.current) out.current = q.current;
  return out;
}

/** The loaded entries after the current one, group first. */
export function upcomingEntries(q: QueueView): QueueEntry[] {
  const from = Math.max(0, pointerIndex(q) + (q.currentIndex === undefined ? 0 : 1) - q.windowStart);
  return q.entries.slice(from);
}

/** The heading over the entries after the group: where they come from, or just "Later". */
export function restLabel(q: QueueView): string {
  return q.contextLabel ? t("queue.nextFrom", { context: q.contextLabel }) : t("queue.later");
}

/**
 * The small icon a row carries before its duration: autoplay entries have their own;
 * entries the user added show the queue icon unless they are in the group (its
 * heading already says so).
 */
export function rowMarker(e: QueueEntry, kind: QueueRowKind): "autoplay" | "added" | undefined {
  if (e.autoplay || e.item.source.type === "autoplay") return "autoplay";
  if (kind === "group" || e.inGroup) return undefined;
  return e.item.added ? "added" : undefined;
}

/** The rows `start..end` to show when `before` / `after` more than the event's window are wanted. */
export function wantedRange(q: QueueView, before: number, after: number): { start: number; end: number } {
  return { start: Math.max(0, q.windowStart - before), end: Math.min(q.total, windowEnd(q) + after) };
}

/** `QueueRange` queries covering `start..end`, at most `max` rows each (the core's cap is 201). */
export function rangeChunks(start: number, end: number, max = 200): { start: number; count: number }[] {
  const out: { start: number; count: number }[] = [];
  for (let s = start; s < end; s += max) out.push({ start: s, count: Math.min(max, end - s) });
  return out;
}

/**
 * Pages from `QueueRange` (any order) joined into one window with `base`'s totals.
 * Undefined when they don't describe the same list as `base` (it changed meanwhile)
 * or leave a gap.
 */
export function mergePages(base: QueueView, pages: readonly QueueView[]): QueueView | undefined {
  if (!pages.length) return undefined;
  const sorted = [...pages].sort((a, b) => a.windowStart - b.windowStart);
  const entries: QueueEntry[] = [];
  let at = sorted[0]!.windowStart;
  for (const p of sorted) {
    if (p.total !== base.total || p.historyCount !== base.historyCount || p.currentIndex !== base.currentIndex || p.groupCount !== base.groupCount) return undefined;
    if (p.windowStart !== at) return undefined;
    entries.push(...p.entries);
    at += p.entries.length;
  }
  // The rows the base window holds must be the same entries (keys), else the list moved.
  for (let i = 0; i < base.entries.length; i++) {
    const j = base.windowStart + i - sorted[0]!.windowStart;
    const e = entries[j];
    if (e && e.item.key !== base.entries[i]!.item.key) return undefined;
  }
  return { ...base, entries, windowStart: sorted[0]!.windowStart };
}

/**
 * Whether a wider window fetched for an earlier view can still stand in for `base` until
 * its refetch: the same list size, history and current entry. Otherwise (a track change,
 * a new context) it would show and target rows that have since moved behind the pointer.
 */
export function pagedFits(paged: QueueView, base: QueueView): boolean {
  if (paged.total !== base.total || paged.historyCount !== base.historyCount || paged.currentIndex !== base.currentIndex) return false;
  if (paged.current?.item.key !== base.current?.item.key) return false;
  if (base.currentIndex === undefined) return true;
  const at = paged.entries[base.currentIndex - paged.windowStart];
  return !at || at.item.key === base.current?.item.key;
}

/** Find a loaded entry by key. */
export function entryByKey(q: QueueView, key: string): QueueEntry | undefined {
  return q.entries.find((e) => e.item.key === key) ?? (q.current?.item.key === key ? q.current : undefined);
}
