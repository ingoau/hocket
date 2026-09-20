// Selection model for virtualised lists: keyed by id, with an "all matching"
// predicate mode so Ctrl+A over 50,000 rows never materialises ids
// (design.md "Library and bulk edits → Lists").
export interface Selection {
  /** Explicit ids when `all` is false; exclusions when `all` is true. */
  ids: ReadonlySet<string>;
  all: boolean;
  /** Count of the filtered list, for `all` mode. */
  total: number;
  anchor: string | undefined;
  focus: string | undefined;
}

export const EMPTY_SELECTION: Selection = { ids: new Set(), all: false, total: 0, anchor: undefined, focus: undefined };

export function isSelected(sel: Selection, id: string): boolean {
  return sel.all ? !sel.ids.has(id) : sel.ids.has(id);
}

export function selectionCount(sel: Selection): number {
  return sel.all ? Math.max(0, sel.total - sel.ids.size) : sel.ids.size;
}

export function selectOnly(id: string): Selection {
  return { ids: new Set([id]), all: false, total: 0, anchor: id, focus: id };
}

export function toggle(sel: Selection, id: string): Selection {
  const ids = new Set(sel.ids);
  if (ids.has(id)) ids.delete(id);
  else ids.add(id);
  return { ...sel, ids, anchor: id, focus: id };
}

/** Shift-click: select the contiguous range between anchor and `id` in `order`. */
export function selectRange(sel: Selection, id: string, order: readonly string[], additive: boolean): Selection {
  const anchor = sel.anchor ?? id;
  const a = order.indexOf(anchor);
  const b = order.indexOf(id);
  if (a < 0 || b < 0) return selectOnly(id);
  const [lo, hi] = a < b ? [a, b] : [b, a];
  const ids = additive && !sel.all ? new Set(sel.ids) : new Set<string>();
  for (let i = lo; i <= hi; i++) ids.add(order[i] as string);
  return { ids, all: false, total: sel.total, anchor, focus: id };
}

export function selectAll(total: number): Selection {
  return { ids: new Set(), all: true, total, anchor: undefined, focus: undefined };
}

export function clearSelection(): Selection {
  return EMPTY_SELECTION;
}

/** Materialise ids for a command when not in `all` mode. Returns undefined in `all` mode. */
export function explicitIds(sel: Selection): string[] | undefined {
  return sel.all ? undefined : [...sel.ids];
}

/** Keyboard navigation over a list: returns the new focus index. */
export function navigate(key: string, focusIndex: number, count: number, pageSize: number): number | undefined {
  if (count === 0) return undefined;
  switch (key) {
    case "ArrowDown":
      return Math.min(count - 1, focusIndex + 1);
    case "ArrowUp":
      return Math.max(0, focusIndex - 1);
    case "Home":
      return 0;
    case "End":
      return count - 1;
    case "PageDown":
      return Math.min(count - 1, focusIndex + pageSize);
    case "PageUp":
      return Math.max(0, focusIndex - pageSize);
    default:
      return undefined;
  }
}

/** Type-ahead: find the next index whose label starts with the buffer, wrapping. */
export function typeAhead(buffer: string, labels: readonly string[], from: number): number | undefined {
  const b = buffer.toLowerCase();
  if (!b) return undefined;
  const n = labels.length;
  for (let i = 1; i <= n; i++) {
    const idx = (from + i) % n;
    if ((labels[idx] ?? "").toLowerCase().startsWith(b)) return idx;
  }
  return undefined;
}
