// Global keyboard handling. The map comes from Query.Shortcuts (rebindable)
// layered over DEFAULT_KEYMAP. Text fields keep Ctrl+Z and typing; lists
// handle navigation first (they stopPropagation); media keys never arrive here.
import { useEffect, useMemo } from "react";
import { DEFAULT_KEYMAP, canonicalActionId } from "@shared/keymap";
import { useApp } from "./app";
import { Keymap, chordFromEvent, isTextInput } from "./shortcuts";
import { executeAction } from "./actions";
import { openContextMenuFromKeyboard } from "../components/ContextMenu";

export function useKeymap(): Keymap {
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  return useMemo(() => {
    const merged = new Map<string, string | undefined>();
    for (const d of DEFAULT_KEYMAP) merged.set(d.actionId, d.shortcut);
    for (const s of shortcuts) merged.set(canonicalActionId(s.actionId), s.shortcut ?? undefined);
    return new Keymap([...merged.entries()].map(([actionId, shortcut]) => ({ actionId, shortcut })), platform);
  }, [shortcuts, platform]);
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

/** What the global handler needs to know about the focused element. */
export interface KeyTarget {
  tag: string;
  type?: string;
  role?: string | null;
  href?: boolean;
}

export function describeTarget(target: EventTarget | null): KeyTarget | undefined {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return undefined;
  return { tag: el.tagName.toLowerCase(), type: (el as HTMLInputElement).type?.toLowerCase(), role: el.getAttribute("role"), href: el.hasAttribute("href") };
}

const ACTIVATES = new Set(["button", "checkbox", "radio", "switch", "tab", "menuitem", "menuitemcheckbox", "menuitemradio", "link"]);
const ARROW_ROLES = new Set(["slider", "spinbutton", "tab", "tablist", "radio", "radiogroup", "menu", "menuitem", "menuitemcheckbox", "menuitemradio", "separator", "scrollbar"]);
const NAV_KEYS = new Set(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"]);

/**
 * Whether an unmodified key belongs to the focused control rather than the
 * global keymap: Space/Enter activate buttons, links, checkboxes, tabs and
 * menu items (so Space on a focused button presses it instead of toggling
 * playback), and arrows move sliders, radios, tabs, menus and splitters (so
 * ArrowLeft on the volume slider lowers the volume instead of seeking).
 * Lists and grids handle their own arrows and stop them before this runs.
 */
export function widgetOwnsKey(key: string, mods: { ctrlKey: boolean; metaKey: boolean; altKey: boolean }, target: KeyTarget | undefined): boolean {
  if (!target || mods.ctrlKey || mods.metaKey || mods.altKey) return false;
  const role = target.role ?? "";
  if (key === " " || key === "Enter") {
    if (target.tag === "button" || target.tag === "summary" || target.tag === "select") return true;
    if (target.tag === "a" && target.href) return true;
    if (target.tag === "input" && ["checkbox", "radio", "button", "submit", "reset", "color", "file"].includes(target.type ?? "")) return true;
    return ACTIVATES.has(role);
  }
  if (NAV_KEYS.has(key)) {
    if (target.tag === "select") return true;
    if (target.tag === "input" && ["range", "radio", "number"].includes(target.type ?? "")) return true;
    return ARROW_ROLES.has(role);
  }
  return false;
}

export function useGlobalKeyboard(enabled = true): void {
  const keymap = useKeymap();
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      // The context-menu key and Shift+F10 open the focused item's context menu.
      if ((e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey)) && !isTextInput(e.target)) {
        if (openContextMenuFromKeyboard()) e.preventDefault();
        return;
      }
      if (widgetOwnsKey(e.key, e, describeTarget(e.target))) return;
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
