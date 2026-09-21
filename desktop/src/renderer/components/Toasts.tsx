import { useEffect } from "react";
import type { Command } from "@core/api";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  useEffect(() => {
    const timers = toasts.map((t) => setTimeout(() => dismiss(t.id), Math.max(1500, t.durationMs)));
    return () => timers.forEach(clearTimeout);
  }, [toasts, dismiss]);
  if (!toasts.length) return null;
  return (
    <div className="toasts" role="status" aria-live="polite">
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
