import { useEffect, useState } from "react";
import type { Command } from "@core/api";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  // Hovering or focusing a toast holds every toast (WCAG 2.2.1): an Undo must stay reachable.
  const [held, setHeld] = useState(false);
  useEffect(() => {
    if (held) return;
    const timers = toasts.map((t) => setTimeout(() => dismiss(t.id), Math.max(1500, t.durationMs)));
    return () => timers.forEach(clearTimeout);
  }, [toasts, dismiss, held]);
  // Always mounted: a live region must exist before its content changes to be announced.
  return (
    <div className="toasts" role="status" aria-live="polite" data-testid="toasts" onMouseEnter={() => setHeld(true)} onMouseLeave={() => setHeld(false)} onFocus={() => setHeld(true)} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setHeld(false); }}>
      {toasts.map((t) => (
        <div key={t.id} className="toast fade-in" data-testid="toast">
          <span>{t.message}</span>
          {t.actionLabel && t.actionCommand ? (
            <button
              type="button"
              className="btn sm"
              data-testid="toast-action"
              onClick={() => {
                try {
                  bridge().dispatch(JSON.parse(t.actionCommand as string) as Command);
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
