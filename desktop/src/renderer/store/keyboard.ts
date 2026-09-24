// Global keyboard handling. The map comes from Query.Shortcuts (rebindable)
// layered over DEFAULT_KEYMAP. Text fields keep Ctrl+Z and typing; lists
// handle navigation first (they stopPropagation); media keys never arrive here.
import { useEffect, useMemo } from "react";
import { DEFAULT_KEYMAP, canonicalActionId } from "@shared/keymap";
import { useApp } from "./app";
import { Keymap, chordFromEvent, isTextInput } from "./shortcuts";
import { executeAction } from "./actions";

export function useKeymap(): Keymap {
  const shortcuts = useApp((s) => s.shortcuts);
  return useMemo(() => {
    const merged = new Map<string, string | undefined>();
    for (const d of DEFAULT_KEYMAP) merged.set(d.actionId, d.shortcut);
    for (const s of shortcuts) merged.set(canonicalActionId(s.actionId), s.shortcut ?? undefined);
    return new Keymap([...merged.entries()].map(([actionId, shortcut]) => ({ actionId, shortcut })));
  }, [shortcuts]);
}

/**
 * The only actions that fire while an editable field has focus. Everything
 * else (including modifier chords: Alt+Arrow word navigation, Mod+Arrow line
 * navigation, Mod+L, Mod+Z…) belongs to the field.
 */
export const TEXT_ALLOWED = new Set(["openCommandPalette", "ui.escape"]);

/** Whether the global handler may run `id` when the event target is a text field. */
export function firesInTextField(id: string): boolean {
  return TEXT_ALLOWED.has(id);
}

export function useGlobalKeyboard(enabled = true): void {
  const keymap = useKeymap();
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const chord = chordFromEvent(e, platform);
      if (!chord) return;
      const ids = keymap.lookup(chord);
      if (!ids.length) return;
      const inText = isTextInput(e.target);
      const id = ids[0] as string;
      if (inText) {
        if (!firesInTextField(id)) return;
        if (id === "ui.escape") {
          (e.target as HTMLElement).blur();
          return;
        }
      }
      e.preventDefault();
      const app = useApp.getState();
      if (id === "redoAlt") { app.dispatch({ type: "redo" }); return; }
      if (id === "undo") { app.dispatch({ type: "undo" }); return; }
      if (id === "redo") { app.dispatch({ type: "redo" }); return; }
      if (/^rate[0-5]$/.test(id)) {
        // Rate the current selection if any; the list handles it when focused.
        if (app.selectionScope && !app.selection.all && app.selection.ids.size) {
          app.runAction(id, { type: "tracks", data: { ids: [...app.selection.ids] } });
        } else if (app.nowPlaying) {
          app.runAction(id, { type: "tracks", data: { ids: [app.nowPlaying.track.id] } });
        }
        return;
      }
      void executeAction(id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [keymap, platform, enabled]);
}
