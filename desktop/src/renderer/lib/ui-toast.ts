// A toast the renderer raises itself (Unpinned …, Hid …), through the same list the core's toasts use.
// [undo] runs when its Undo button is pressed. It re-applies just that one change to the current
// value, so edits made since (here or on another device) are kept.
import { t } from "@shared/strings";
import { useApp } from "../store/app";

let seq = 0;
const undos = new Map<string, () => void>();

/** The marker a renderer toast carries in place of a core command. */
export const UI_UNDO = "ui-undo";

/** Returns the toast's id, for taking it down early (the store's dismissToast). */
export function showToast(message: string, undo?: () => void): string {
  const id = `ui-${Date.now()}-${++seq}`;
  if (undo) undos.set(id, undo);
  // Forget it after the toast could have gone (held toasts last longer, so be generous).
  setTimeout(() => undos.delete(id), 120_000);
  useApp.getState().applyEvent({
    type: "toast",
    data: { toast: { id, message, actionLabel: undo ? t("toast.undo") : undefined, actionCommand: undo ? UI_UNDO : undefined, durationMs: 5000 } },
  } as never);
  return id;
}

/** Runs a renderer toast's Undo; false when [id] is not one (its action is a core command). */
export function runUiUndo(id: string): boolean {
  const undo = undos.get(id);
  if (!undo) return false;
  undos.delete(id);
  undo();
  return true;
}

/**
 * A toast for a change the core records on its undo stack (a rating, a playlist edit): [apply] makes
 * the change, and the toast's Undo undoes that entry. Most of the time it is still the newest one
 * (the core's plain Undo); when something was done since, the core's UndoEntry walks back to it.
 */
export function toastUndoable(message: string, apply: () => void): void {
  const before = useApp.getState().undo.history[0]?.id;
  let mine: string | undefined;
  const stop = useApp.subscribe((s) => {
    const top = s.undo.history[0]?.id;
    if (top && top !== before) {
      mine = top;
      stop();
    }
  });
  setTimeout(stop, 10_000);
  apply();
  showToast(message, () => {
    stop();
    const top = useApp.getState().undo.history[0]?.id;
    useApp.getState().dispatch(mine && top !== mine ? { type: "undoEntry", data: { id: mine } } : { type: "undo" });
  });
}
