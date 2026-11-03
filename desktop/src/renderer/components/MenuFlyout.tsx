// A context menu's list submenu (Share, Snooze): beside the menu, one row per choice, each closing
// the menu and running. The Add to playlist submenu (PlaylistSubmenu) has its own search and rows
// but places, animates and goes back the same way.
//
// Keyboard: ↑↓ move (wrapping), Home/End jump, a letter jumps to the next row starting with it,
// Enter/Space run, ← or Escape go back to the menu, Tab closes both. The mouse's back button goes
// back too.
import { useEffect, useLayoutEffect, useState } from "react";
import type { ExitProps } from "../lib/presence";
import { placeFlyout, type FlyoutAnchor, type FlyoutPlace } from "../lib/flyout";
import { Icon } from "./Icon";

export interface FlyoutRow {
  id: string;
  label: string;
  icon: string;
  run: () => void;
}

interface Props {
  rows: FlyoutRow[];
  /** The submenu's accessible name (the row it opens from). */
  label: string;
  testId: string;
  anchor: FlyoutAnchor;
  /** The submenu's motion class: `fade-in` while open, `closing` on its way out (lib/presence). */
  motion: string;
  exitProps: ExitProps<HTMLDivElement>;
  /** Back to the menu. */
  onBack: () => void;
  /** Close the menu and the submenu, then run [then]. */
  onClose: (then?: () => void) => void;
}

/** The mouse's back button (the thumb button, "X1"). */
export const MOUSE_BACK = 3;

/** Placed once its size is known, and focused once placed (a hidden menu can't take focus). */
export function useFlyoutPlace(ref: React.RefObject<HTMLElement | null>, anchor: FlyoutAnchor, ready = true): FlyoutPlace | undefined {
  const [place, setPlace] = useState<FlyoutPlace | undefined>(undefined);
  useLayoutEffect(() => {
    if (!ready || place || !ref.current) return;
    // Layout size, not getBoundingClientRect(): the entrance animation scales it.
    setPlace(placeFlyout(anchor, ref.current.offsetWidth, ref.current.offsetHeight, window.innerWidth, window.innerHeight));
  }, [ready, place, anchor, ref]);
  return place;
}

export function MenuFlyout({ rows, label, testId, anchor, motion, exitProps, onBack, onClose }: Props) {
  const [active, setActive] = useState(0);
  const ref = exitProps.ref;
  const place = useFlyoutPlace(ref, anchor);
  useEffect(() => {
    if (place) ref.current?.focus({ preventScroll: true });
  }, [place, ref]);

  const run = (i: number) => {
    const row = rows[i];
    if (row) onClose(row.run);
  };
  const n = rows.length;
  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    const go = (i: number) => { e.preventDefault(); setActive(i); };
    switch (e.key) {
      case "Tab": e.preventDefault(); onClose(); return;
      case "Escape": case "ArrowLeft": e.preventDefault(); onBack(); return;
      case "ArrowDown": go((active + 1) % n); return;
      case "ArrowUp": go((active + n - 1) % n); return;
      case "Home": go(0); return;
      case "End": go(n - 1); return;
      case "Enter": case " ": e.preventDefault(); run(active); return;
    }
    // Type-ahead: the next row whose label starts with the key.
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const k = e.key.toLowerCase();
      const i = [...rows.keys()].map((j) => (active + 1 + j) % n).find((j) => rows[j]?.label.toLowerCase().startsWith(k));
      if (i !== undefined) go(i);
    }
  };

  return (
    <div
      {...exitProps}
      className={`menu cm cm-sub ${motion}`}
      role="menu"
      aria-label={label}
      aria-activedescendant={rows[active] ? `${testId}-${rows[active].id}` : undefined}
      tabIndex={-1}
      style={{ left: place?.x ?? -9999, top: place?.y ?? 0, visibility: place ? undefined : "hidden" }}
      data-side={place?.side}
      data-context-layer=""
      data-testid={testId}
      onKeyDown={onKey}
      onMouseUp={(e) => { if (e.button === MOUSE_BACK) { e.preventDefault(); onBack(); } }}
    >
      {rows.map((row, i) => (
        <div
          key={row.id}
          id={`${testId}-${row.id}`}
          role="menuitem"
          className={`mi ${active === i ? "active" : ""}`}
          data-action={row.id}
          onMouseMove={() => setActive(i)}
          onClick={() => run(i)}
        >
          <span className="cm-lead cm-icon" aria-hidden="true"><Icon name={row.icon} size={15} /></span>
          <span className="grow truncate">{row.label}</span>
        </div>
      ))}
    </div>
  );
}
