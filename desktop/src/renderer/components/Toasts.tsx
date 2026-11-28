import { useEffect, useRef, useState } from "react";
import type { Command } from "@core/api";
import { useApp } from "../store/app";
import { usePresenceList } from "../lib/presence";
import { type BusyToast, UI_UNDO, runUiUndo } from "../lib/ui-toast";
import { Icon } from "./Icon";

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  // Hovering or focusing a toast holds every toast (WCAG 2.2.1): an Undo must stay reachable.
  const [held, setHeld] = useState(false);
  // A dismissed toast stays mounted, inert, while it sinks away.
  const entries = usePresenceList(toasts, (t) => t.id);
  // When each toast's time started: its arrival (a toast pushed again under its id is a new one),
  // or the end of a hold. Another toast arriving doesn't restart the others' time, so a stack
  // clears from the oldest.
  const since = useRef(new WeakMap<object, number>());
  useEffect(() => {
    if (held) {
      // Letting go gives every toast its full time again.
      since.current = new WeakMap();
      return;
    }
    const now = Date.now();
    const timers = toasts.map((t) => {
      const from = since.current.get(t) ?? now;
      since.current.set(t, from);
      return setTimeout(() => dismiss(t.id), Math.max(0, from + Math.max(1500, t.durationMs) - now));
    });
    return () => timers.forEach(clearTimeout);
  }, [toasts, dismiss, held]);
  // Always mounted: a live region must exist before its content changes to be announced.
  return (
    <div className="toasts" role="status" aria-live="polite" data-testid="toasts" onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)} onFocus={() => setHeld(true)} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setHeld(false); }}>
      {entries.map(({ item: t, key, motion, exitProps }) => (
        <div key={key} {...exitProps} className={`toast ${motion}`} data-testid="toast">
          {(t as BusyToast).busy ? <Icon name="spinner" size={18} className="spin" /> : null}
          <span>{t.message}</span>
          {t.actionLabel && t.actionCommand ? (
            <button
              type="button"
              className="btn sm"
              data-testid="toast-action"
              onClick={() => {
                try {
                  if (t.actionCommand === UI_UNDO) runUiUndo(t.id);
                  else useApp.getState().dispatch(JSON.parse(t.actionCommand as string) as Command);
                } catch (err) {
                  console.error("bad toast command", err);
                }
                dismiss(t.id);
              }}
            >
              {t.actionLabel}
            </button>
          ) : null}
        </div>
      ))}
    </div>
  );
}
