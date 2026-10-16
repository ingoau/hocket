// Native-feeling context menu generated from Query.Actions(surface = contextMenu).
// Keyboard: the context-menu key or Shift+F10 opens it for the focused item
// (a synthetic `contextmenu` event at the item, so every surface's existing
// onContextMenu handler serves both), focus moves into the menu with the
// first item active, arrows/Home/End move, Enter/Space run, Escape/Tab close
// and focus returns to whatever opened it.
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ActionDescriptor, ActionTarget } from "@core/api";
import { formatChord, parseChord } from "../store/shortcuts";
import { fetchActions } from "../store/queries";
import { useApp, type MenuItem } from "../store/app";
import { executeAction, type ActionContext } from "../store/actions";
import { Icon, hasIcon } from "./Icon";
import { handOnFocus } from "../lib/focus";
import { usePresence } from "../lib/presence";

/** Where focus goes back to when the menu closes. */
let returnFocus: HTMLElement | null = null;
let openedByKeyboard = false;

/** Pin to Library and Unpin from Library: only the one that applies is shown (the other is disabled). */
const LIBRARY_PIN_IDS = new Set(["pinToLibrary", "unpinFromLibrary"]);

/** Open the menu for a target at the pointer. Call from an onContextMenu handler. */
export async function openContextMenu(e: { clientX: number; clientY: number; preventDefault(): void }, target: ActionTarget, context?: ActionContext, items?: MenuItem[]): Promise<void> {
  e.preventDefault();
  const keyboard = openedByKeyboard;
  openedByKeyboard = false;
  const active = document.activeElement;
  returnFocus = active instanceof HTMLElement && active !== document.body ? active : null;
  const actions = target.type === "none" ? [] : (await fetchActions("contextMenu", target)).filter((a) => !(LIBRARY_PIN_IDS.has(a.id) && !a.enabled));
  if (!actions.length && !items?.length) return;
  useApp.getState().openContextMenu({ x: e.clientX, y: e.clientY, target, actions, items, context, keyboard });
}

/** A menu of renderer-run items only (things the registry has no target for: genres, filters, Home sections). */
export function openItemsMenu(e: { clientX: number; clientY: number; preventDefault(): void }, items: MenuItem[]): void {
  void openContextMenu(e, { type: "none" }, undefined, items);
}

/** As [openItemsMenu], under a "More" button. */
export function openItemsMenuFromButton(e: React.MouseEvent<HTMLElement>, items: MenuItem[]): void {
  const r = e.currentTarget.getBoundingClientRect();
  openedByKeyboard = e.detail === 0;
  openItemsMenu({ clientX: Math.round(r.left), clientY: Math.round(r.bottom + 2), preventDefault: () => e.preventDefault() }, items);
}

/** A "More" button's menu: under the button, first item active when opened from the keyboard. */
export function openMenuFromButton(e: React.MouseEvent<HTMLElement>, target: ActionTarget, context?: ActionContext, items?: MenuItem[]): void {
  const r = e.currentTarget.getBoundingClientRect();
  openedByKeyboard = e.detail === 0;
  void openContextMenu({ clientX: Math.round(r.left), clientY: Math.round(r.bottom + 2), preventDefault: () => e.preventDefault() }, target, context, items);
}

/**
 * The context-menu key / Shift+F10: fire `contextmenu` at the focused item (or
 * the option a listbox points at with aria-activedescendant). Returns whether
 * there was an item to open a menu for.
 */
export function openContextMenuFromKeyboard(): boolean {
  let el = document.activeElement as HTMLElement | null;
  if (!el || el === document.body) return false;
  const descendant = el.getAttribute("aria-activedescendant");
  if (descendant) el = document.getElementById(descendant) ?? el;
  const r = el.getBoundingClientRect();
  openedByKeyboard = true;
  const handled = !el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: Math.round(r.left + Math.min(24, r.width / 2)), clientY: Math.round(r.top + Math.min(r.height, 28)) }));
  if (!handled) openedByKeyboard = false;
  return handled;
}

type Row =
  | { kind: "action"; action: ActionDescriptor; id: string; label: string; enabled: boolean; category: string }
  | { kind: "item"; item: MenuItem; id: string; label: string; enabled: boolean; category: string };

export function ContextMenu() {
  const live = useApp((s) => s.contextMenu);
  const close = useApp((s) => s.closeContextMenu);
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const ref = useRef<HTMLDivElement>(null);
  // The closed menu stays mounted, inert, while its exit plays. Everything
  // below that positions, focuses or listens is keyed on the live state.
  const { value: menu, motion, exitProps } = usePresence(live, ref);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [active, setActive] = useState(-1);
  // The renderer's own items first, then the registry's actions; both run from the same keyboard model.
  const rows: Row[] = [
    ...(menu?.items ?? []).map((m): Row => ({ kind: "item", item: m, id: m.id, label: m.label, enabled: m.enabled && !m.heading, category: "items" })),
    ...(menu?.actions ?? []).map((a): Row => ({ kind: "action", action: a, id: a.id, label: a.label, enabled: a.enabled, category: a.category })),
  ];
  const actions = rows;

  useLayoutEffect(() => {
    if (!live || !ref.current) return;
    // Layout size, not getBoundingClientRect(): the menu opens scaled down by its entrance animation.
    const r = { width: ref.current.offsetWidth, height: ref.current.offsetHeight };
    const x = Math.min(live.x, window.innerWidth - r.width - 6);
    const y = Math.min(live.y, window.innerHeight - r.height - 6);
    setPos({ x: Math.max(4, x), y: Math.max(4, y) });
    // Keyboard-opened menus start on the first enabled item, like native menus.
    setActive(live.keyboard ? [...(live.items ?? []).map((m) => m.enabled && !m.heading), ...live.actions.map((a) => a.enabled)].findIndex(Boolean) : -1);
    ref.current.focus();
  }, [live]);

  // Focus goes back to the trigger when the menu closes (unless an action moved it on purpose).
  const wasOpen = useRef(false);
  useEffect(() => {
    if (live) { wasOpen.current = true; return; }
    if (!wasOpen.current) return;
    wasOpen.current = false;
    const el = returnFocus;
    returnFocus = null;
    const current = document.activeElement;
    const focusLost = !current || current === document.body || !!current.closest?.("[role=menu]");
    // A dialog the action opened takes the trigger over (lib/focus handOnFocus) and restores it itself.
    if (useApp.getState().dialog || useApp.getState().paletteOpen) return;
    handOnFocus(null);
    if (el && el.isConnected && focusLost) el.focus({ preventScroll: true });
  }, [live]);

  useEffect(() => {
    if (!live) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) close();
    };
    const onBlur = () => close();
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
    };
  }, [live, close]);

  if (!menu) return null;
  const run = (r: Row) => {
    if (!r.enabled) return;
    handOnFocus(returnFocus);
    close();
    if (r.kind === "item") r.item.run();
    else void executeAction(r.action.id, menu.target, menu.context);
  };
  const enabledIdx = actions.map((a, i) => (a.enabled ? i : -1)).filter((i) => i >= 0);
  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const cur = enabledIdx.indexOf(active);
      const next = e.key === "ArrowDown" ? enabledIdx[(cur + 1) % enabledIdx.length] : enabledIdx[(cur - 1 + enabledIdx.length) % enabledIdx.length];
      if (next !== undefined) setActive(next);
      return;
    }
    if ((e.key === "Enter" || e.key === " ") && active >= 0) {
      e.preventDefault();
      const a = actions[active];
      if (a) run(a);
      return;
    }
    if (e.key === "Home") { e.preventDefault(); setActive(enabledIdx[0] ?? -1); }
    if (e.key === "End") { e.preventDefault(); setActive(enabledIdx[enabledIdx.length - 1] ?? -1); }
    // Type-ahead: the next enabled item whose label starts with the key.
    if (e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const k = e.key.toLowerCase();
      const order = [...enabledIdx.filter((i) => i > active), ...enabledIdx.filter((i) => i <= active)];
      const hit = order.find((i) => actions[i]?.label.toLowerCase().startsWith(k));
      if (hit !== undefined) setActive(hit);
    }
  };
  const chordFor = (id: string) => {
    const s = shortcuts.find((x) => x.actionId === id)?.shortcut ?? menu.actions.find((a) => a.id === id)?.defaultShortcut;
    const c = s ? parseChord(s) : undefined;
    return c ? formatChord(c, platform) : undefined;
  };
  let lastCategory: string | undefined;
  return (
    <div {...exitProps} className={`menu ${motion}`} role="menu" aria-activedescendant={active >= 0 ? `cm-item-${active}` : undefined} tabIndex={-1} style={{ left: pos.x, top: pos.y }} onKeyDown={onKey} data-testid="context-menu">
      {actions.map((r, i) => {
        const item = r.kind === "item" ? r.item : undefined;
        const sep = (lastCategory !== undefined && lastCategory !== r.category) || (!!item?.heading && i > 0);
        lastCategory = r.category;
        if (item?.heading) {
          return (
            <Fragment key={`h-${r.id}`}>
              {sep ? <div className="sep" role="separator" /> : null}
              <div className="menu-heading" role="presentation">{item.label}</div>
            </Fragment>
          );
        }
        const chord = r.kind === "action" ? chordFor(r.id) : undefined;
        const icon = r.kind === "action" ? r.action.icon : r.item.icon;
        const destructive = r.kind === "action" ? r.action.destructive : !!r.item.destructive;
        const radio = item?.checked !== undefined;
        return (
          <Fragment key={`${r.kind}-${r.id}`}>
            {sep ? <div className="sep" role="separator" /> : null}
            <div
              id={`cm-item-${i}`}
              className={`mi ${r.enabled ? "" : "disabled"} ${destructive ? "destructive" : ""} ${active === i ? "active" : ""}`}
              role={radio ? "menuitemradio" : "menuitem"}
              aria-checked={radio ? !!item?.checked : undefined}
              aria-disabled={!r.enabled || undefined}
              onMouseMove={(e) => { if (e.movementX || e.movementY) setActive(i); }}
              onClick={() => run(r)}
              data-action={r.id}
            >
              {hasIcon(icon) ? <Icon name={icon} size={14} /> : <span style={{ width: 14 }} aria-hidden="true" />}
              <span className="grow truncate">{r.label}</span>
              {radio && item?.checked ? <Icon name="check" size={14} /> : null}
              {chord ? <span className="kbd">{chord}</span> : null}
            </div>
          </Fragment>
        );
      })}
    </div>
  );
}
