// Where a context menu's submenu (Add to playlist, Share, Snooze) goes: beside the menu, level with
// the row it opens from, on the right when there's room and else on the left, and never past the
// window's top or bottom. The side it lands on picks the way its entrance slides.

/** The menu's left and right edges and the top of the row the submenu opens from. */
export interface FlyoutAnchor {
  left: number;
  right: number;
  top: number;
}

export type FlyoutSide = "right" | "left";

export interface FlyoutPlace {
  x: number;
  y: number;
  side: FlyoutSide;
}

/** How far the submenu tucks over the menu's edge, and the margin it keeps from the window's. */
const OVERLAP = 4;
const MARGIN = 6;

/** The submenu's place for its layout size (`w`×`h`) in a `vw`×`vh` window. */
export function placeFlyout(anchor: FlyoutAnchor, w: number, h: number, vw: number, vh: number): FlyoutPlace {
  const right = anchor.right - OVERLAP + w <= vw - MARGIN;
  const x = right ? anchor.right - OVERLAP : Math.max(4, anchor.left - w + OVERLAP);
  const y = Math.max(4, Math.min(anchor.top - MARGIN, vh - h - MARGIN));
  return { x, y, side: right ? "right" : "left" };
}
