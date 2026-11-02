// The song menu's Snooze flyout: 1 day, 3 days, 1 week, 1 month, indefinitely.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { SnoozeFor } from "@core/api";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { Icon } from "./Icon";

const DURATIONS: { id: SnoozeFor; label: Parameters<typeof t>[0] }[] = [
  { id: "day", label: "snooze.day" },
  { id: "threeDays", label: "snooze.threeDays" },
  { id: "week", label: "snooze.week" },
  { id: "month", label: "snooze.month" },
  { id: "indefinite", label: "snooze.indefinite" },
];

interface Props {
  trackIds: string[];
  anchor: { left: number; right: number; top: number };
  closing: boolean;
  motion: string;
  onBack: () => void;
  onClose: (then?: () => void) => void;
}

export function SnoozeSubmenu({ trackIds, anchor, closing, motion, onBack, onClose }: Props) {
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ x: number; y: number } | undefined>(undefined);
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!ref.current || pos) return;
    const w = ref.current.offsetWidth;
    const h = ref.current.offsetHeight;
    const x = anchor.right - 4 + w <= window.innerWidth - 6 ? anchor.right - 4 : Math.max(4, anchor.left - w + 4);
    setPos({ x, y: Math.max(4, Math.min(anchor.top - 6, window.innerHeight - h - 6)) });
  }, [pos, anchor]);
  useEffect(() => { if (pos) ref.current?.focus({ preventScroll: true }); }, [pos]);
  const choose = (duration: SnoozeFor) => onClose(() => bridge().dispatch({ type: "snooze", data: { track_ids: trackIds, duration } }));
  return (
    <div
      ref={ref}
      className={`menu cm cm-sub ${motion} ${closing ? "closing" : ""}`}
      role="menu"
      aria-label={t("action.snooze")}
      tabIndex={-1}
      style={pos ? { left: pos.x, top: pos.y } : { left: -9999, top: 0 }}
      data-context-layer=""
      data-testid="snooze-submenu"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape" || e.key === "ArrowLeft") { e.preventDefault(); onBack(); return; }
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          setActive((i) => (i + (e.key === "ArrowDown" ? 1 : DURATIONS.length - 1)) % DURATIONS.length);
          return;
        }
        if (e.key === "Enter" || e.key === " ") { e.preventDefault(); const row = DURATIONS[active]; if (row) choose(row.id); }
      }}
    >
      {DURATIONS.map((row, i) => (
        <div
          key={row.id}
          role="menuitem"
          className={`mi ${active === i ? "active" : ""}`}
          data-action={row.id}
          onMouseMove={() => setActive(i)}
          onClick={() => choose(row.id)}
        >
          <span className="cm-lead cm-icon" aria-hidden="true"><Icon name="sleep" size={15} /></span>
          <span className="grow truncate">{t(row.label)}</span>
        </div>
      ))}
    </div>
  );
}
