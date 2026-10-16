// A toast the renderer raises itself (Unpinned …, Hid …), through the same list the core's toasts use.
// [undo] is the command the toast's Undo button dispatches (a setting put back as it was).
import type { Command } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";

let seq = 0;

export function showToast(message: string, undo?: Command): void {
  useApp.getState().applyEvent({
    type: "toast",
    data: { toast: { id: `ui-${Date.now()}-${++seq}`, message, actionLabel: undo ? t("toast.undo") : undefined, actionCommand: undo ? JSON.stringify(undo) : undefined, durationMs: 5000 } },
  } as never);
}
