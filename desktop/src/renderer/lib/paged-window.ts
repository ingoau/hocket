// Sparse paged rows for virtualised lists: only the pages around the viewport
// stay in memory. A 50k-track library no longer holds 50k slots of copies on
// every page merge.

const KEEP_PAGES = 12;

export function materializePages<T>(pages: Map<number, T[]>, total: number, pageSize: number): (T | undefined)[] {
  const next = new Array<T | undefined>(total);
  for (const [p, items] of pages) {
    const off = p * pageSize;
    for (let i = 0; i < items.length; i++) next[off + i] = items[i];
  }
  return next;
}

export function evictPages<T>(pages: Map<number, T[]>, around: number): void {
  if (pages.size <= KEEP_PAGES) return;
  const keep = new Set<number>();
  if (pages.has(around)) keep.add(around);
  for (let d = 1; keep.size < KEEP_PAGES && d < pages.size + KEEP_PAGES; d++) {
    if (pages.has(around - d)) keep.add(around - d);
    if (keep.size >= KEEP_PAGES) break;
    if (pages.has(around + d)) keep.add(around + d);
  }
  for (const k of [...pages.keys()]) if (!keep.has(k)) pages.delete(k);
}
