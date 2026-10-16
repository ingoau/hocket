// A toast the renderer raises itself (Unpinned …, Hid …), through the same list the core's toasts use.
// [undo] runs when its Undo button is pressed. It re-applies just that one change to the current
// value, so edits made since (here or on another device) are kept.
import { t } from "@shared/strings";
import { useApp } from "../store/app";

let seq = 0;
const undos = new Map<string, () => void>();

/** The marker a renderer toast carries in place of a core command. */
export const UI_UNDO = "ui-undo";

export function showToast(message: string, undo?: () => void): void {
  const id = `ui-${Date.now()}-${++seq}`;
  if (undo) undos.set(id, undo);
  // Forget it after the toast could have gone (held toasts last longer, so be generous).
  setTimeout(() => undos.delete(id), 120_000);
  useApp.getState().applyEvent({
    type: "toast",
    data: { toast: { id, message, actionLabel: undo ? t("toast.undo") : undefined, actionCommand: undo ? UI_UNDO : undefined, durationMs: 5000 } },
  } as never);
}

/** Runs a renderer toast's Undo; false when [id] is not one (its action is a core command). */
export function runUiUndo(id: string): boolean {
  const undo = undos.get(id);
  if (!undo) return false;
  undos.delete(id);
  undo();
  return true;
}
