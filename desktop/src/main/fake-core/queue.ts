// The queue as one flat list with a pointer: a TypeScript mirror of the
// Rust reducer's `Flat` and its ops (crates/hocket-core/src/session/reducer.rs).
// See docs/design.md "Queue model". Pure functions over a `FlatQueue`; the
// fake core wraps them with undo, playback and events.
import type { QueueItem, RepeatMode } from "@core/api";
import { Rng } from "./random";

/** `ptr === list.length` means nothing is current (the list is all history). */
export interface FlatQueue {
  list: QueueItem[];
  ptr: number;
  shuffle: boolean;
  repeat: RepeatMode;
}

export type QueuePlacement = "next" | "group" | "later";

export const hasCurrent = (q: FlatQueue): boolean => q.ptr < q.list.length;
export const currentItem = (q: FlatQueue): QueueItem | undefined => q.list[q.ptr];
export const indexOfKey = (q: FlatQueue, key: string): number => q.list.findIndex((i) => i.key === key);

/** Where "directly after the current entry" is. */
export function afterCurrent(q: FlatQueue): number {
  return hasCurrent(q) ? q.ptr + 1 : q.ptr;
}

/** One past the "Next in queue" group: the run of `queued` entries right after the current one. */
export function groupEnd(q: FlatQueue): number {
  let end = afterCurrent(q);
  if (hasCurrent(q)) while (end < q.list.length && q.list[end]!.queued) end++;
  return end;
}

/** Nearest playable entry after the pointer, wrapping under Repeat All (the current one last). */
export function forward(q: FlatQueue, wrap: boolean): { to: number; passed: number[] } | undefined {
  const n = q.list.length;
  const passed: number[] = [];
  const order: number[] = [];
  for (let i = afterCurrent(q); i < n; i++) order.push(i);
  if (wrap) for (let i = 0; i < (hasCurrent(q) ? q.ptr + 1 : n); i++) order.push(i);
  for (const i of order) {
    if (q.list[i]!.unavailable) passed.push(i);
    else return { to: i, passed };
  }
  return undefined;
}

/** Nearest playable entry before the pointer, wrapping under Repeat All to the last one (the current one last). */
export function backward(q: FlatQueue, wrap: boolean): number | undefined {
  const n = q.list.length;
  for (let i = Math.min(q.ptr, n) - 1; i >= 0; i--) if (!q.list[i]!.unavailable) return i;
  if (!wrap) return undefined;
  for (let i = n - 1; i >= (hasCurrent(q) ? q.ptr : n); i--) if (!q.list[i]!.unavailable) return i;
  return undefined;
}

/** Next: move the pointer one playable entry on. `restart` when Repeat All wraps a one-song list onto itself. */
export function advance(q: FlatQueue): { moved: boolean; restart: boolean; skipped: string[] } {
  const f = forward(q, q.repeat === "all");
  if (!f) return { moved: false, restart: false, skipped: [] };
  const restart = f.to === q.ptr;
  const skipped = f.passed.map((i) => q.list[i]!.key);
  q.ptr = f.to;
  return { moved: true, restart, skipped };
}

/** Previous as a step (the 3 s restart rule is the caller's). Returns false when it restarts instead. */
export function stepBack(q: FlatQueue): boolean {
  const i = backward(q, q.repeat === "all");
  if (i === undefined || i === q.ptr) return false;
  q.ptr = i;
  return true;
}

/** Move the pointer onto an entry; nothing in the list changes. */
export function jump(q: FlatQueue, key: string): boolean {
  const i = indexOfKey(q, key);
  if (i < 0) return false;
  q.list[i]!.unavailable = false;
  q.ptr = i;
  return true;
}

export function trimHistory(q: FlatQueue, cap: number): void {
  const c = Math.max(1, cap);
  if (q.ptr > c) {
    const excess = q.ptr - c;
    q.list.splice(0, excess);
    q.ptr -= excess;
  }
}

/** While shuffle is off, the list order is the unshuffled order. */
export function renumber(q: FlatQueue): void {
  q.list.forEach((item, i) => (item.ordinal = i));
}

/** After every op, as the reducer: trim history, renumber with shuffle off. */
export function settle(q: FlatQueue, historyCap: number): void {
  trimHistory(q, historyCap);
  if (!q.shuffle) renumber(q);
}

/** Ordinals right after the entry before `at` for `count` entries inserted there, shifting later ones (shuffle on). */
function ordinalsAfterPrevious(q: FlatQueue, at: number, count: number): void {
  const prev = at > 0 ? q.list[at - 1] : undefined;
  const base = prev ? (prev.ordinal ?? 0) + 1 : 0;
  q.list.forEach((item, i) => {
    if ((i < at || i >= at + count) && (item.ordinal ?? 0) >= base) item.ordinal = (item.ordinal ?? 0) + count;
  });
  for (let k = 0; k < count; k++) {
    const item = q.list[at + k];
    if (item) item.ordinal = base + k;
  }
}

/** Ordinals at the end of the unshuffled order for `count` entries at `at`. */
export function ordinalsAtEnd(q: FlatQueue, at: number, count: number): void {
  let max = -1;
  q.list.forEach((item, i) => {
    if (i < at || i >= at + count) max = Math.max(max, item.ordinal ?? 0);
  });
  for (let k = 0; k < count; k++) {
    const item = q.list[at + k];
    if (item) item.ordinal = max + 1 + k;
  }
}

/**
 * Play next / Add to queue / Play later. `keys` name entries: ahead of the pointer they
 * move, in history or current they are copied under a new key. `fresh` makes a new
 * `added` entry for a track. `queued` is the Queue section setting for Play next.
 */
export function queueItems(
  q: FlatQueue,
  trackIds: string[],
  keys: string[],
  placement: QueuePlacement,
  queued: boolean,
  fresh: (trackId: string) => QueueItem,
): boolean {
  const items: QueueItem[] = [];
  for (const key of keys) {
    const i = indexOfKey(q, key);
    if (i < 0) continue;
    if (i > q.ptr && hasCurrent(q)) items.push(q.list.splice(i, 1)[0]!);
    else items.push(fresh(q.list[i]!.trackId));
  }
  for (const t of trackIds) items.push(fresh(t));
  if (!items.length) return false;
  const hasGroup = groupEnd(q) > afterCurrent(q);
  // A group already after the current entry stays attached to it.
  const flag = placement === "next" ? queued || hasGroup : placement === "group";
  for (const item of items) item.queued = flag;
  const at = placement === "next" ? afterCurrent(q) : placement === "group" ? groupEnd(q) : q.list.length;
  q.list.splice(at, 0, ...items);
  if (q.shuffle) {
    if (placement === "later") ordinalsAtEnd(q, at, items.length);
    else ordinalsAfterPrevious(q, at, items.length);
  }
  // Queued onto a list with nothing current: the first of them is current now.
  if (!hasCurrent(q)) q.ptr = Math.min(at, q.ptr);
  return true;
}

/** Remove entries. Returns whether the current entry went (the next playable one takes its place) and whether that stopped playback. */
export function removeKeys(q: FlatQueue, keys: string[]): { removed: boolean; currentRemoved: boolean; stopped: boolean } {
  const wanted = new Set(keys);
  const hadCurrent = hasCurrent(q);
  const currentRemoved = !!currentItem(q) && wanted.has(currentItem(q)!.key);
  const beforePtr = q.list.slice(0, Math.min(q.ptr, q.list.length)).filter((i) => wanted.has(i.key)).length;
  const before = q.list.length;
  q.list = q.list.filter((i) => !wanted.has(i.key));
  if (q.list.length === before) return { removed: false, currentRemoved: false, stopped: false };
  q.ptr -= beforePtr;
  let stopped = false;
  if (currentRemoved) {
    let next = -1;
    for (let i = q.ptr; i < q.list.length; i++) {
      if (!q.list[i]!.unavailable) {
        next = i;
        break;
      }
    }
    if (next >= 0) q.ptr = next;
    else {
      q.ptr = q.list.length;
      stopped = hadCurrent;
    }
  }
  return { removed: true, currentRemoved, stopped };
}

/**
 * Drag an entry ahead of the pointer to directly after `after` (the current entry or one
 * ahead), in or out of the group: an in-group drop clamps to the group's end, an
 * out-of-group drop to just after the group. False for a bad move.
 */
export function moveItem(q: FlatQueue, key: string, after: string, inGroup: boolean): boolean {
  const i = indexOfKey(q, key);
  const a = indexOfKey(q, after);
  if (i < 0 || a < 0 || !hasCurrent(q) || i <= q.ptr || a < q.ptr) return false;
  const item = q.list.splice(i, 1)[0]!;
  // Where `after` is now; dropping onto itself keeps its place.
  const target = a === i ? i : a > i ? a : a + 1;
  const end = groupEnd(q);
  const at = inGroup ? Math.min(target, end) : Math.max(target, end);
  item.queued = inGroup;
  q.list.splice(at, 0, item);
  if (q.shuffle) ordinalsAfterPrevious(q, at, 1);
  return true;
}

/** Remove the "Next in queue" group; returns its entries. */
export function clearGroup(q: FlatQueue): QueueItem[] {
  const start = afterCurrent(q);
  return q.list.splice(start, groupEnd(q) - start);
}

/** Deterministic Fisher-Yates over a slice. */
export function shuffleInPlace<T>(items: T[], seed: number): void {
  new Rng(seed).shuffle(items);
}

/** Shuffle the entries after the group. */
export function shuffleTail(q: FlatQueue, seed: number): void {
  const from = groupEnd(q);
  const tail = q.list.slice(from);
  shuffleInPlace(tail, seed);
  q.list.splice(from, tail.length, ...tail);
  q.shuffle = true;
}

/** Put the entries after the group back in unshuffled order, continuing from the current entry's place in it. */
export function unshuffleTail(q: FlatQueue): void {
  const from = groupEnd(q);
  const pivot = currentItem(q)?.ordinal;
  const key = (i: QueueItem): [number, number] => [pivot !== undefined && (i.ordinal ?? 0) < pivot ? 1 : 0, i.ordinal ?? 0];
  const tail = q.list.slice(from).sort((x, y) => {
    const [a0, a1] = key(x);
    const [b0, b1] = key(y);
    return a0 - b0 || a1 - b1;
  });
  q.list.splice(from, tail.length, ...tail);
  q.shuffle = false;
}

export function setShuffle(q: FlatQueue, enabled: boolean, seed: number): void {
  if (enabled && !q.shuffle) shuffleTail(q, seed);
  else if (!enabled && q.shuffle) unshuffleTail(q);
}

/** A window of rows from the list, with exact totals (the reducer's `derive_range`). */
export interface DerivedQueue {
  entries: { item: QueueItem; inGroup: boolean }[];
  windowStart: number;
  total: number;
  historyCount: number;
  currentIndex: number | undefined;
  groupCount: number;
  totalUpcoming: number;
  current: QueueItem | undefined;
  wrapNext: QueueItem | undefined;
  previous: QueueItem | undefined;
}

export function deriveRange(q: FlatQueue, start: number, count: number): DerivedQueue {
  const has = hasCurrent(q);
  const h = Math.min(q.ptr, q.list.length);
  const total = q.list.length;
  const groupCount = has ? groupEnd(q) - afterCurrent(q) : 0;
  const s = Math.min(Math.max(0, start), total);
  const e = Math.min(s + Math.max(0, count), total);
  const entries = q.list.slice(s, e).map((item, k) => {
    const i = s + k;
    return { item, inGroup: has && i > h && i <= h + groupCount };
  });
  const wrap = q.repeat === "all";
  const upcoming = q.list.slice(afterCurrent(q));
  let wrapNext: QueueItem | undefined;
  if (wrap && !upcoming.some((i) => !i.unavailable)) {
    wrapNext = q.list.slice(0, h).find((i) => !i.unavailable) ?? (currentItem(q)?.unavailable ? undefined : currentItem(q));
  }
  let previous: QueueItem | undefined;
  for (let i = h - 1; i >= 0 && !previous; i--) if (!q.list[i]!.unavailable) previous = q.list[i];
  if (!previous && wrap) for (let i = upcoming.length - 1; i >= 0 && !previous; i--) if (!upcoming[i]!.unavailable) previous = upcoming[i];
  return {
    entries,
    windowStart: s,
    total,
    historyCount: h,
    currentIndex: has ? h : undefined,
    groupCount,
    totalUpcoming: upcoming.length,
    current: currentItem(q),
    wrapNext,
    previous,
  };
}

/** At most `before` rows of history and `after` rows past the current entry: what the UI is sent on every change. */
export function deriveWindow(q: FlatQueue, before: number, after: number): DerivedQueue {
  const ptr = Math.min(q.ptr, q.list.length);
  const start = Math.max(0, ptr - before);
  return deriveRange(q, start, ptr - start + 1 + after);
}
