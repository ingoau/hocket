// Roving tabindex for simple lists and wrapping tile grids: one Tab stop per
// list, arrow keys move between items by geometry (so the same code serves a
// horizontal shelf, a vertical list and a CSS auto-fill grid), Home/End jump
// to the ends. The virtualised album grid and track table have their own
// index-based versions because most of their items are not in the DOM.
import { useLayoutEffect, useRef, type RefObject } from "react";

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Index of the item an arrow key moves to from `from`, or undefined to stay. */
export function nextByGeometry(key: string, from: number, boxes: readonly Box[]): number | undefined {
  const n = boxes.length;
  if (!n) return undefined;
  if (key === "Home") return 0;
  if (key === "End") return n - 1;
  const cur = boxes[from];
  if (!cur) return 0;
  const cx = cur.left + cur.width / 2;
  const cy = cur.top + cur.height / 2;
  let best: number | undefined;
  let bestScore = Infinity;
  boxes.forEach((b, i) => {
    if (i === from) return;
    const bx = b.left + b.width / 2;
    const by = b.top + b.height / 2;
    const sameRow = Math.abs(by - cy) < Math.max(4, cur.height / 2);
    const sameCol = Math.abs(bx - cx) < Math.max(4, cur.width / 2);
    let score: number;
    switch (key) {
      case "ArrowRight":
        if (!sameRow || bx <= cx) return;
        score = bx - cx;
        break;
      case "ArrowLeft":
        if (!sameRow || bx >= cx) return;
        score = cx - bx;
        break;
      case "ArrowDown":
        if (by <= cy + 1 || sameRow) return;
        // Nearest row first, then the nearest column in it.
        score = (by - cy) * 10_000 + Math.abs(bx - cx) + (sameCol ? 0 : 1);
        break;
      case "ArrowUp":
        if (by >= cy - 1 || sameRow) return;
        score = (cy - by) * 10_000 + Math.abs(bx - cx) + (sameCol ? 0 : 1);
        break;
      default:
        return;
    }
    if (score < bestScore) {
      bestScore = score;
      best = i;
    }
  });
  return best;
}

export interface RovingHandlers {
  onKeyDown: (e: React.KeyboardEvent) => void;
  onFocus: (e: React.FocusEvent) => void;
}

/**
 * Wires roving tabindex on `container`'s items (`selector`); spread the
 * returned handlers on the container. Exactly one item is tabbable: the last
 * one focused (by keyboard or pointer), else the first.
 */
export function useRoving(container: RefObject<HTMLElement | null>, selector = "[data-roving]"): RovingHandlers {
  const active = useRef(0);
  // Runs after every render: the item list may have changed.
  useLayoutEffect(() => {
    const items = Array.from(container.current?.querySelectorAll<HTMLElement>(selector) ?? []);
    if (!items.length) return;
    if (active.current >= items.length) active.current = items.length - 1;
    items.forEach((el, i) => { el.tabIndex = i === active.current ? 0 : -1; });
  });
  const itemsOf = () => Array.from(container.current?.querySelectorAll<HTMLElement>(selector) ?? []);
  const onFocus = (e: React.FocusEvent) => {
    const items = itemsOf();
    const i = items.indexOf((e.target as HTMLElement).closest(selector) as HTMLElement);
    if (i < 0 || i === active.current) return;
    active.current = i;
    items.forEach((el, j) => { el.tabIndex = j === i ? 0 : -1; });
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(e.key)) return;
    const items = itemsOf();
    const from = items.indexOf((e.target as HTMLElement).closest(selector) as HTMLElement);
    if (from < 0) return;
    const next = nextByGeometry(e.key, from, items.map((el) => el.getBoundingClientRect()));
    // Arrows never leak to the global keymap (seek) from inside a list, even at an edge.
    e.preventDefault();
    e.stopPropagation();
    if (next === undefined) return;
    items.forEach((el, i) => { el.tabIndex = i === next ? 0 : -1; });
    active.current = next;
    const el = items[next] as HTMLElement;
    el.focus();
    el.scrollIntoView({ block: "nearest", inline: "nearest" });
  };
  return { onKeyDown, onFocus };
}
