// Edit pins: the side panel's draft of `library.pins`. Reordering, unpinning (removePin, and
// restorePin for its Undo) and choosing what a click does all change the draft; Done saves it, Cancel
// drops it. Pins that aren't shown (their album or filter is gone) stay in the draft where they were,
// as the pin menu's moves leave them.
import { pinTap, setTap, type LibraryPin, type PinTap } from "./library-pins";

export const pinKey = (p: Pick<LibraryPin, "kind" | "id">): string => `${p.kind}:${p.id}`;
const same = (a: LibraryPin, b: LibraryPin) => a.kind === b.kind && a.id === b.id;

/** The chip's order: Opens, Plays, Shuffles, round again. */
export function nextTap(tap: PinTap): PinTap {
  return tap === "open" ? "play" : tap === "play" ? "shuffle" : "open";
}

/** [pin]'s click moved on to the next choice. */
export function cycleTap(draft: readonly LibraryPin[], pin: LibraryPin): LibraryPin[] {
  return setTap(draft, pin, nextTap(pinTap(pin)));
}

/**
 * [pin] moved [by] places among [shown] (clamped at either end), to just past the pin it lands on;
 * pins that aren't shown keep their place. In the two-column grid ±1 is a step along, ±2 a row.
 */
export function moveShownBy(draft: readonly LibraryPin[], pin: LibraryPin, by: number, shown: readonly LibraryPin[] = draft): LibraryPin[] {
  const i = shown.findIndex((p) => same(p, pin));
  if (i < 0 || by === 0) return [...draft];
  const j = Math.max(0, Math.min(shown.length - 1, i + by));
  const target = shown[j];
  return target && j !== i ? placeBeside(draft, pin, target, j > i) : [...draft];
}

/** [pin] placed next to [target]: before it, or after it when [after] (a drag landing on a tile). */
export function placeBeside(draft: readonly LibraryPin[], pin: LibraryPin, target: LibraryPin, after: boolean): LibraryPin[] {
  if (same(pin, target)) return [...draft];
  const moved = draft.find((p) => same(p, pin));
  if (!moved) return [...draft];
  const list = draft.filter((p) => !same(p, pin));
  const to = list.findIndex((p) => same(p, target));
  if (to < 0) return [...draft];
  list.splice(after ? to + 1 : to, 0, moved);
  return list;
}

/** Whether the draft differs from what was stored when editing began (order, pins or clicks). */
export function draftChanged(original: readonly LibraryPin[], draft: readonly LibraryPin[]): boolean {
  return original.length !== draft.length || original.some((p, i) => {
    const d = draft[i];
    return !d || !same(p, d) || p.tap !== d.tap;
  });
}

/**
 * What Done stores: a three-way merge of [original] (when editing began), the [draft] and [current]
 * (stored now; the panel isn't modal, so a pin's own menu, or another device, may have changed it).
 * The draft's order wins; a pin unpinned elsewhere stays unpinned, one pinned elsewhere goes at the
 * end, and a click changed elsewhere is kept unless the draft changed it too. Unknown kinds and
 * fields are carried through as they are.
 */
export function draftToSave(original: readonly LibraryPin[], draft: readonly LibraryPin[], current: readonly LibraryPin[]): LibraryPin[] {
  const find = (list: readonly LibraryPin[], p: LibraryPin) => list.find((x) => same(x, p));
  const kept = draft.flatMap((d) => {
    const o = find(original, d);
    const c = find(current, d);
    if (o && !c) return [];
    return [c && o && d.tap === o.tap && c.tap !== d.tap ? { ...d, tap: c.tap } : d];
  });
  const added = current.filter((c) => !find(original, c) && !find(draft, c));
  return [...kept, ...added];
}

/** How large each of a tile's corners is: large on the group's outer edges, small inside. */
export interface Corners { tl: boolean; tr: boolean; br: boolean; bl: boolean }

/**
 * Tile [i] of [n] in a two-column connected group (the board's radiusFor). A short last row leaves
 * the tile above its gap with an outer bottom-right corner.
 */
export function tileCorners(i: number, n: number): Corners {
  const col = i % 2;
  const row = Math.floor(i / 2);
  const lastRow = Math.floor((n - 1) / 2);
  const hasRight = col === 1 || i + 1 < n;
  return {
    tl: row === 0 && col === 0,
    tr: row === 0 && (col === 1 || n === 1),
    br: (row === lastRow && (col === 1 || !hasRight)) || (row === lastRow - 1 && col === 1 && n % 2 === 1),
    bl: row === lastRow && col === 0,
  };
}

/** [tileCorners] as a CSS border-radius. */
export function tileRadius(i: number, n: number, large = 24, small = 6): string {
  const c = tileCorners(i, n);
  const r = (big: boolean) => `${big ? large : small}px`;
  return `${r(c.tl)} ${r(c.tr)} ${r(c.br)} ${r(c.bl)}`;
}
