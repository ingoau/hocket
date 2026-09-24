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
import { useApp } from "../store/app";
import { executeAction, type ActionContext } from "../store/actions";
import { Icon, hasIcon } from "./Icon";

/** Where focus goes back to when the menu closes. */
let returnFocus: HTMLElement | null = null;
let openedByKeyboard = false;

/** Open the menu for a target at the pointer. Call from an onContextMenu handler. */
export async function openContextMenu(e: { clientX: number; clientY: number; preventDefault(): void }, target: ActionTarget, context?: ActionContext): Promise<void> {
  e.preventDefault();
  const keyboard = openedByKeyboard;
  openedByKeyboard = false;
  const active = document.activeElement;
  returnFocus = active instanceof HTMLElement && active !== document.body ? active : null;
  const actions = await fetchActions("contextMenu", target);
  if (!actions.length) return;
  useApp.getState().openContextMenu({ x: e.clientX, y: e.clientY, target, actions, context, keyboard });
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

export function ContextMenu() {
  const menu = useApp((s) => s.contextMenu);
  const close = useApp((s) => s.closeContextMenu);
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [active, setActive] = useState(-1);
  const actions = menu?.actions ?? [];

  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    const x = Math.min(menu.x, window.innerWidth - r.width - 6);
    const y = Math.min(menu.y, window.innerHeight - r.height - 6);
    setPos({ x: Math.max(4, x), y: Math.max(4, y) });
    // Keyboard-opened menus start on the first enabled item, like native menus.
    setActive(menu.keyboard ? menu.actions.findIndex((a) => a.enabled) : -1);
    ref.current.focus();
  }, [menu]);

  // Focus goes back to the trigger when the menu closes (unless an action moved it on purpose).
  const wasOpen = useRef(false);
  useEffect(() => {
    if (menu) { wasOpen.current = true; return; }
    if (!wasOpen.current) return;
    wasOpen.current = false;
    const el = returnFocus;
    returnFocus = null;
    const current = document.activeElement;
    const focusLost = !current || current === document.body || !!current.closest?.("[role=menu]");
    if (el && el.isConnected && focusLost && !useApp.getState().dialog) el.focus({ preventScroll: true });
  }, [menu]);

  useEffect(() => {
    if (!menu) return;
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
  }, [menu, close]);

  if (!menu) return null;
  const run = (a: ActionDescriptor) => {
    if (!a.enabled) return;
    close();
    void executeAction(a.id, menu.target, menu.context);
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
    const s = shortcuts.find((x) => x.actionId === id)?.shortcut ?? actions.find((a) => a.id === id)?.defaultShortcut;
    const c = s ? parseChord(s) : undefined;
    return c ? formatChord(c, platform) : undefined;
  };
  let lastCategory: string | undefined;
  return (
    <div ref={ref} className="menu fade-in" role="menu" aria-activedescendant={active >= 0 ? `cm-item-${active}` : undefined} tabIndex={-1} style={{ left: pos.x, top: pos.y }} onKeyDown={onKey} data-testid="context-menu">
      {actions.map((a, i) => {
        const sep = lastCategory !== undefined && lastCategory !== a.category;
        lastCategory = a.category;
        const chord = chordFor(a.id);
        return (
          <Fragment key={a.id}>
            {sep ? <div className="sep" role="separator" /> : null}
            <div
              id={`cm-item-${i}`}
              className={`mi ${a.enabled ? "" : "disabled"} ${a.destructive ? "destructive" : ""} ${active === i ? "active" : ""}`}
              role="menuitem"
              aria-disabled={!a.enabled || undefined}
              onMouseEnter={() => setActive(i)}
              onClick={() => run(a)}
              data-action={a.id}
            >
              {hasIcon(a.icon) ? <Icon name={a.icon} size={14} /> : <span style={{ width: 14 }} aria-hidden="true" />}
              <span className="grow truncate">{a.label}</span>
              {chord ? <span className="kbd">{chord}</span> : null}
            </div>
          </Fragment>
        );
      })}
    </div>
  );
}
