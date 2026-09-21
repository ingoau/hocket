// Native-feeling context menu generated from Query.Actions(surface = contextMenu).
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ActionDescriptor, ActionTarget } from "@core/api";
import { formatChord, parseChord } from "../store/shortcuts";
import { fetchActions } from "../store/queries";
import { useApp } from "../store/app";
import { executeAction, type ActionContext } from "../store/actions";
import { Icon, hasIcon } from "./Icon";

/** Open the menu for a target at the pointer. Call from an onContextMenu handler. */
export async function openContextMenu(e: { clientX: number; clientY: number; preventDefault(): void }, target: ActionTarget, context?: ActionContext): Promise<void> {
  e.preventDefault();
  const actions = await fetchActions("contextMenu", target);
  if (!actions.length) return;
  useApp.getState().openContextMenu({ x: e.clientX, y: e.clientY, target, actions, context });
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
    setActive(-1);
    ref.current.focus();
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
    if (e.key === "Escape") return close();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const cur = enabledIdx.indexOf(active);
      const next = e.key === "ArrowDown" ? enabledIdx[(cur + 1) % enabledIdx.length] : enabledIdx[(cur - 1 + enabledIdx.length) % enabledIdx.length];
      if (next !== undefined) setActive(next);
    }
    if (e.key === "Enter" && active >= 0) {
      const a = actions[active];
      if (a) run(a);
    }
    if (e.key === "Home") setActive(enabledIdx[0] ?? -1);
    if (e.key === "End") setActive(enabledIdx[enabledIdx.length - 1] ?? -1);
  };
  const chordFor = (id: string) => {
    const s = shortcuts.find((x) => x.actionId === id)?.shortcut ?? actions.find((a) => a.id === id)?.defaultShortcut;
    const c = s ? parseChord(s) : undefined;
    return c ? formatChord(c, platform) : undefined;
  };
  let lastCategory: string | undefined;
  return (
    <div ref={ref} className="menu fade-in" role="menu" tabIndex={-1} style={{ left: pos.x, top: pos.y }} onKeyDown={onKey} data-testid="context-menu">
      {actions.map((a, i) => {
        const sep = lastCategory !== undefined && lastCategory !== a.category;
        lastCategory = a.category;
        const chord = chordFor(a.id);
        return (
          <div key={a.id}>
            {sep ? <div className="sep" /> : null}
            <div
              className={`mi ${a.enabled ? "" : "disabled"} ${a.destructive ? "destructive" : ""} ${active === i ? "active" : ""}`}
              role="menuitem"
              aria-disabled={!a.enabled}
              onMouseEnter={() => setActive(i)}
              onClick={() => run(a)}
              data-action={a.id}
            >
              {hasIcon(a.icon) ? <Icon name={a.icon} size={14} /> : <span style={{ width: 14 }} />}
              <span className="grow truncate">{a.label}</span>
              {chord ? <span className="kbd">{chord}</span> : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}
