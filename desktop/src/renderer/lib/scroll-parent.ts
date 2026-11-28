/** The nearest ancestor of `el` that scrolls vertically (a page's scroller), or null. */
export function scrollParent(el: Element): HTMLElement | null {
  for (let up = el.parentElement; up; up = up.parentElement) if (/(auto|scroll)/.test(getComputedStyle(up).overflowY)) return up;
  return null;
}
