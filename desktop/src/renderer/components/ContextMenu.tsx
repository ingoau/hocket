// Native-feeling context menu generated from Query.Actions(surface = contextMenu), laid out per the
// menu design (lib/menu-model): a header naming the item, the play actions as one icon-only strip,
// an inline Rating row, Love / Download / Pin as checkboxes, Add to playlist as a submenu
// (PlaylistSubmenu), then the rest with the destructive item last.
// Keyboard: the context-menu key or Shift+F10 opens it for the focused item (a synthetic
// `contextmenu` event at the item, so every surface's existing onContextMenu handler serves both),
// focus moves into the menu with the first item active, ↑↓ move between rows and ←→ along the strip
// and the stars, Home/End jump, Enter/Space run, → (or Enter) opens the submenu, a letter jumps to
// the next item starting with it, a shortcut shown in the menu runs on the menu's item, Escape/Tab
// close and focus returns to whatever opened it.
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ActionTarget } from "@core/api";
import { chordFromEvent, formatChord, parseChord } from "../store/shortcuts";
import { useKeymap } from "../store/keyboard";
import { DEFAULT_KEYMAP, canonicalActionId } from "@shared/keymap";
import { t } from "@shared/strings";
import { fetchActions } from "../store/queries";
import { useApp, type MenuItem } from "../store/app";
import { executeAction, type ActionContext } from "../store/actions";
import { Icon } from "./Icon";
import { Artwork } from "./Artwork";
import { PlaylistSubmenu } from "./PlaylistSubmenu";
import { handOnFocus } from "../lib/focus";
import { usePresence } from "../lib/presence";
import { fetchMenuFacts } from "../lib/menu-facts";
import { toastUndoable } from "../lib/ui-toast";
import { ratingText } from "../lib/a11y";
import { buildMenu, loveToast, nextRating, rateToast, removeToast, starLabel, type MenuHeader, type MenuRow } from "../lib/menu-model";

/** Where focus goes back to when the menu closes. */
let returnFocus: HTMLElement | null = null;
let openedByKeyboard = false;

/** Pin to Library and Unpin from Library: only the one that applies is shown (the other is disabled). */
const LIBRARY_PIN_IDS = new Set(["pinToLibrary", "unpinFromLibrary"]);

/** Shortcuts the menu's rows share with a keymap action: Delete removes from the list being shown. */
const CHORD_OF: Record<string, string> = { removeFromPlaylist: "remove", removeFromQueue: "remove" };

/** Open the menu for a target at the pointer. Call from an onContextMenu handler. */
export async function openContextMenu(e: { clientX: number; clientY: number; preventDefault(): void }, target: ActionTarget, context?: ActionContext, items?: MenuItem[]): Promise<void> {
  e.preventDefault();
  const keyboard = openedByKeyboard;
  openedByKeyboard = false;
  const active = document.activeElement;
  returnFocus = active instanceof HTMLElement && active !== document.body ? active : null;
  const none = target.type === "none";
  const [all, facts] = await Promise.all([none ? [] : fetchActions("contextMenu", target), none ? undefined : fetchMenuFacts(target, context)]);
  const actions = all.filter((a) => !(LIBRARY_PIN_IDS.has(a.id) && !a.enabled));
  if (!actions.length && !items?.length) return;
  useApp.getState().openContextMenu({ x: e.clientX, y: e.clientY, target, actions, items, context, keyboard, facts });
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

/** One thing the keyboard can land on: a row, a button of the play strip or a star. */
interface Cell {
  /** Its line in the menu (the strip and the stars are one line each). */
  line: number;
  label: string;
  enabled: boolean;
  /** Opens the Add to playlist submenu instead of running. */
  submenu?: boolean;
  /** A star: its rating. */
  star?: number;
  /** Where ↑↓ land on this line (the current rating's star). */
  home?: boolean;
  run: () => void;
}

export function ContextMenu() {
  const live = useApp((s) => s.contextMenu);
  const close = useApp((s) => s.closeContextMenu);
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const keymap = useKeymap();
  const ref = useRef<HTMLDivElement>(null);
  // The closed menu stays mounted, inert, while its exit plays. Everything
  // below that positions, focuses or listens is keyed on the live state.
  const { value: menu, motion, exitProps } = usePresence(live, ref);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [active, setActive] = useState(-1);
  /** The active cell was reached by keyboard (it gets a ring), not the pointer. */
  const [keys, setKeys] = useState(false);
  /** The Add to playlist submenu, with where it opens from. */
  const [sub, setSub] = useState<{ left: number; right: number; top: number } | null>(null);
  const subOpen = useRef(false);
  subOpen.current = !!sub;
  const model = useMemo(() => (menu ? buildMenu(menu.actions, menu.items ?? [], menu.facts, { inPlaylist: !!menu.context?.playlistId }) : []), [menu]);

  useLayoutEffect(() => {
    if (!live || !ref.current) return;
    // Layout size, not getBoundingClientRect(): the menu opens scaled down by its entrance animation.
    const r = { width: ref.current.offsetWidth, height: ref.current.offsetHeight };
    const x = Math.min(live.x, window.innerWidth - r.width - 6);
    const y = Math.min(live.y, window.innerHeight - r.height - 6);
    setPos({ x: Math.max(4, x), y: Math.max(4, y) });
    setSub(null);
    setKeys(!!live.keyboard);
    // Keyboard-opened menus start on the first enabled item, like native menus.
    setActive(live.keyboard ? firstEnabled(ref.current) : -1);
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
    const focusLost = !current || current === document.body || !!current.closest?.("[role=menu], [data-context-layer]");
    // A dialog the action opened takes the trigger over (lib/focus handOnFocus) and restores it itself.
    if (useApp.getState().dialog || useApp.getState().paletteOpen) return;
    handOnFocus(null);
    if (el && el.isConnected && focusLost) el.focus({ preventScroll: true });
  }, [live]);

  useEffect(() => {
    if (!live) return;
    // The submenu counts as inside: both carry data-context-layer. So do the toasts while the
    // submenu is open: its changes apply at once, and their Undo shows in it.
    const onDown = (e: MouseEvent) => {
      const at = e.target as Element | null;
      if (at?.closest?.("[data-context-layer]") || (subOpen.current && at?.closest?.('[data-testid="toasts"]'))) return;
      close();
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
  const facts = menu.facts;

  /** Close, then run: a dialog the run opens takes the trigger over for focus. */
  const finish = (then?: () => void) => {
    handOnFocus(returnFocus);
    close();
    then?.();
  };
  /** A registry action on the menu's target; a change to several songs (or a removal) toasts with Undo. */
  const perform = (id: string) => finish(() => {
    const go = () => void executeAction(id, menu.target, menu.context);
    const message = id === "love" || id === "unlove" ? loveToast(facts, id === "love") : /^rate[0-5]$/.test(id) ? rateToast(facts, Number(id.slice(4))) : id === "removeFromPlaylist" ? removeToast(facts) : undefined;
    if (message) toastUndoable(message, go);
    else go();
  });
  const openSub = (cell: number) => {
    const row = document.getElementById(`cm-item-${cell}`);
    if (!ref.current || !row) return;
    const box = ref.current.getBoundingClientRect();
    setActive(cell);
    setSub({ left: box.left, right: box.right, top: row.getBoundingClientRect().top });
  };
  const closeSub = () => {
    if (!sub) return;
    setSub(null);
    ref.current?.focus({ preventScroll: true });
  };

  const bound = (id: string): string | undefined => {
    const own = shortcuts.find((s) => canonicalActionId(s.actionId) === id);
    const s = own ? (own.shortcut ?? undefined) : (menu.actions.find((a) => a.id === id)?.defaultShortcut ?? DEFAULT_KEYMAP.find((d) => d.actionId === id)?.shortcut);
    const c = s ? parseChord(s) : undefined;
    return c ? formatChord(c, platform).replace(/\bDelete$/, "Del") : undefined;
  };

  // Lay the rows out as cells (what the keyboard moves through) and the commands a shortcut can run.
  const cells: Cell[] = [];
  const rowCells: number[][] = [];
  const commands = new Map<string, () => void>();
  let line = 0;
  model.forEach((r, ri) => {
    const at: number[] = [];
    const push = (c: Omit<Cell, "line">) => { at.push(cells.length); cells.push({ ...c, line }); };
    switch (r.kind) {
      case "strip":
        for (const c of r.cells) {
          push({ label: c.label, enabled: c.enabled, run: () => perform(c.id) });
          if (c.enabled) commands.set(c.id, () => perform(c.id));
        }
        line++;
        break;
      case "rating":
        for (let n = 1; n <= 5; n++) push({ label: "", enabled: r.enabled, star: n, home: !r.mixed && r.value === n, run: () => perform(`rate${nextRating(n, r)}`) });
        if (r.enabled) for (let n = 0; n <= 5; n++) commands.set(`rate${n}`, () => perform(`rate${n}`));
        line++;
        break;
      case "check":
        push({ label: r.label, enabled: r.enabled, run: () => perform(r.runId) });
        // Love and Unlove share one chord (as Download and Remove download would): the row runs whichever applies.
        if (r.enabled) for (const id of r.ids) commands.set(id, () => perform(r.runId));
        line++;
        break;
      case "submenu":
        push({ label: r.label, enabled: r.enabled, submenu: true, run: () => undefined });
        line++;
        break;
      case "action":
        push({ label: r.label, enabled: r.action.enabled, run: () => perform(r.action.id) });
        if (r.action.enabled) {
          commands.set(r.action.id, () => perform(r.action.id));
          const alias = CHORD_OF[r.action.id];
          if (alias) commands.set(alias, () => perform(r.action.id));
        }
        line++;
        break;
      case "item":
        push({ label: r.item.label, enabled: r.item.enabled && !r.item.heading, run: () => finish(r.item.run) });
        line++;
        break;
      case "choices":
        for (const item of r.items) {
          push({ label: item.label, enabled: item.enabled, run: () => finish(item.run) });
          line++;
        }
        break;
      case "sep":
        break;
    }
    rowCells[ri] = at;
  });

  const activate = (i: number) => {
    const c = cells[i];
    if (!c?.enabled) return;
    if (c.submenu) openSub(i);
    else c.run();
  };
  const enabledIdx = cells.map((c, i) => (c.enabled ? i : -1)).filter((i) => i >= 0);
  /** Where ↑↓ land on a line: its current star, else its first enabled cell. */
  const landing = (l: number) => enabledIdx.find((i) => cells[i]?.line === l && cells[i]?.home) ?? enabledIdx.find((i) => cells[i]?.line === l);
  const lines = [...new Set(enabledIdx.map((i) => cells[i]?.line ?? 0))];
  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    setKeys(true);
    if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      close();
      return;
    }
    const cur = cells[active];
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const at = cur ? lines.indexOf(cur.line) : -1;
      const n = lines.length;
      const next = e.key === "ArrowDown" ? lines[(at + 1) % n] : lines[at <= 0 ? n - 1 : at - 1];
      const i = next === undefined ? undefined : landing(next);
      if (i !== undefined) setActive(i);
      return;
    }
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      if (cur?.submenu && e.key === "ArrowRight") {
        activate(active);
        return;
      }
      const along = enabledIdx.filter((i) => cells[i]?.line === cur?.line);
      const k = along.indexOf(active);
      const i = along[e.key === "ArrowRight" ? Math.min(along.length - 1, k + 1) : Math.max(0, k - 1)];
      if (k >= 0 && i !== undefined) setActive(i);
      return;
    }
    if ((e.key === "Enter" || e.key === " ") && active >= 0) {
      e.preventDefault();
      activate(active);
      return;
    }
    if (e.key === "Home") { e.preventDefault(); setActive(enabledIdx[0] ?? -1); return; }
    if (e.key === "End") { e.preventDefault(); setActive(enabledIdx[enabledIdx.length - 1] ?? -1); return; }
    // A shortcut shown in the menu runs on the menu's own item (right-click a song, Ctrl+L loves
    // that song, 4 rates it), not on the selection or what's playing.
    const chord = chordFromEvent(e, platform);
    const hit = (chord ? keymap.lookup(chord) : []).map((id) => commands.get(id)).find(Boolean);
    if (hit) {
      e.preventDefault();
      hit();
      return;
    }
    // Type-ahead: the next enabled item whose label starts with the key.
    if (e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey) {
      const k = e.key.toLowerCase();
      const order = [...enabledIdx.filter((i) => i > active), ...enabledIdx.filter((i) => i <= active)];
      const i = order.find((j) => cells[j]?.label.toLowerCase().startsWith(k));
      if (i !== undefined) setActive(i);
    }
  };

  /** Pointer over a cell: it becomes the active one, and the submenu closes unless it's its row. */
  const hover = (i: number) => (e: React.MouseEvent) => {
    if (!e.movementX && !e.movementY) return;
    setKeys(false);
    if (active !== i) setActive(i);
    if (sub && !cells[i]?.submenu) closeSub();
  };
  const cellProps = (i: number) => ({
    id: `cm-item-${i}`,
    "aria-disabled": !cells[i]?.enabled || undefined,
    onMouseMove: hover(i),
    onClick: () => activate(i),
  });
  const kbd = (chord: string | undefined) => (chord ? <span className="kbd">{chord}</span> : null);
  const lead = <span className="cm-lead" aria-hidden="true" />;

  const renderRow = (r: MenuRow, ri: number) => {
    const at = rowCells[ri] ?? [];
    const first = at[0] ?? -1;
    switch (r.kind) {
      case "sep":
        return <div key={`sep-${ri}`} className="sep" role="separator" />;
      case "strip":
        return (
          <div key="strip" className="cm-strip" role="group" aria-label={facts?.kind === "song" && facts.count > 1 ? t("menu.playGroupSongs", { n: facts.count }) : t("menu.playGroup")}>
            {r.cells.map((c, k) => {
              const i = at[k] ?? -1;
              return (
                <div key={c.id} {...cellProps(i)} role="menuitem" aria-label={c.label} title={c.label} className={`cm-strip-btn ${c.id === "play" ? "primary" : ""} ${c.enabled ? "" : "disabled"} ${active === i ? "active" : ""}`} data-action={c.id}>
                  <Icon name={c.icon} size={c.id === "play" ? 18 : 16} />
                </div>
              );
            })}
          </div>
        );
      case "rating": {
        const clear = bound("rate0");
        return (
          <div key="rating" className="cm-rating" role="group" aria-labelledby="cm-rating-label">
            {lead}
            <span className="cm-rating-label grow" id="cm-rating-label">
              {t("menu.rating")}
              {r.mixed ? <span className="cm-chip"><svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" aria-hidden="true"><path d="M6 12h12" /></svg>{t("menu.mixed")}</span> : null}
            </span>
            <span className="cm-stars">
              {at.map((i, k) => {
                const n = k + 1;
                const on = !r.mixed && n <= r.value;
                const current = !r.mixed && n === r.value;
                const key = bound(`rate${n}`);
                // "4 stars (4)"; on the current rating, "Clear rating (0)".
                const tip = current ? (clear ? t("menu.clearRatingTip", { key: clear }) : t("menu.clearRating")) : key ? t("menu.starTip", { label: ratingText(n), key }) : ratingText(n);
                return (
                  <div key={n} {...cellProps(i)} role="menuitemradio" aria-checked={current} aria-label={starLabel(n, r.count)} title={tip} className={`cm-star ${on ? "on" : ""} ${r.mixed ? "mixed" : ""} ${active === i ? "active" : ""}`} data-star={n} data-action={`rate${n}`}>
                    <svg width="14" height="14" viewBox="0 0 24 24" strokeWidth="2" strokeLinejoin="round" aria-hidden="true"><path d="M12 3.6l2.5 5.2 5.7.8-4.1 4 1 5.7L12 16.6l-5.1 2.7 1-5.7-4.1-4 5.7-.8z" /></svg>
                  </div>
                );
              })}
            </span>
          </div>
        );
      }
      case "check":
        return (
          <div key={r.id} {...cellProps(first)} role="menuitemcheckbox" aria-checked={r.checked} className={`mi cm-check ${r.enabled ? "" : "disabled"} ${active === first ? "active" : ""}`} data-action={r.runId}>
            <span className="cm-lead" aria-hidden="true">
              {r.checked === true ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg> : r.checked === "mixed" ? <svg width="13" height="13" viewBox="0 0 24 24" fill="none" strokeWidth="3" strokeLinecap="round"><path d="M6 12h12" /></svg> : null}
            </span>
            <span className="grow truncate">{r.label}</span>
            {r.note ? <span className="cm-note">{r.note}</span> : null}
            {kbd(r.ids.map(bound).find(Boolean))}
          </div>
        );
      case "submenu":
        return (
          <div
            key={r.id}
            {...cellProps(first)}
            onMouseMove={(e) => { hover(first)(e); if (!sub && r.enabled && (e.movementX || e.movementY)) openSub(first); }}
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={!!sub}
            className={`mi cm-subrow ${r.enabled ? "" : "disabled"} ${active === first || sub ? "active" : ""}`}
            data-action={r.id}
          >
            {lead}
            <span className="grow truncate">{r.label}</span>
            <Icon name="chevronRight" size={14} className="cm-chev" />
          </div>
        );
      case "action": {
        const destructive = r.destructive;
        return (
          <div key={`a-${r.action.id}`} {...cellProps(first)} role="menuitem" className={`mi ${r.action.enabled ? "" : "disabled"} ${destructive ? "destructive" : ""} ${active === first ? "active" : ""}`} data-action={r.action.id}>
            {lead}
            <span className="grow truncate">{r.label}</span>
            {kbd(bound(CHORD_OF[r.action.id] ?? r.action.id))}
          </div>
        );
      }
      case "item":
        return itemRow(r.item, first);
      case "choices":
        // A heading and its radios ("When clicked": Click opens, plays, shuffles) form a labelled group.
        return (
          <div key={`h-${r.heading.id}`} role="group" aria-labelledby={`cm-heading-${ri}`}>
            <div className="menu-heading" id={`cm-heading-${ri}`} role="presentation">{r.heading.label}</div>
            {r.items.map((item, k) => itemRow(item, at[k] ?? -1))}
          </div>
        );
    }
  };
  const itemRow = (item: MenuItem, i: number) => {
    const radio = item.checked !== undefined;
    return (
      <div key={`i-${item.id}`} {...cellProps(i)} role={radio ? "menuitemradio" : "menuitem"} aria-checked={radio ? !!item.checked : undefined} className={`mi ${item.enabled ? "" : "disabled"} ${item.destructive ? "destructive" : ""} ${active === i ? "active" : ""}`} data-action={item.id}>
        {/* A choice is drawn as a radio, so it can't be mistaken for an action with the same name. */}
        {/* The app's own menus (a pin's, a filter's, a Home section's) keep their icons; the item rows below a pin's follow the item menu design. */}
        {radio ? <span className={`mi-radio ${item.checked ? "on" : ""}`} aria-hidden="true" /> : <span className="cm-lead cm-icon" aria-hidden="true">{item.icon ? <Icon name={item.icon} size={15} /> : null}</span>}
        <span className="grow truncate">{item.label}</span>
      </div>
    );
  };

  return (
    <Fragment>
      <div
        {...exitProps}
        className={`menu cm ${keys ? "keys" : ""} ${motion}`}
        role="menu"
        aria-label={facts?.header?.title}
        aria-activedescendant={active >= 0 ? `cm-item-${active}` : undefined}
        tabIndex={-1}
        style={{ left: pos.x, top: pos.y }}
        onKeyDown={onKey}
        data-context-layer=""
        data-testid="context-menu"
      >
        {facts?.header ? <Header header={facts.header} /> : null}
        {model.map(renderRow)}
      </div>
      {sub && facts?.trackIds?.length ? (
        <PlaylistSubmenu trackIds={facts.trackIds} anchor={sub} closing={motion === "closing"} motion={motion} onBack={closeSub} onClose={finish} />
      ) : null}
    </Fragment>
  );
}

/** The item the menu is for: its cover (three stacked for several), title and subtitle. */
function Header({ header }: { header: MenuHeader }) {
  const covers = header.covers.length ? header.covers : [undefined];
  return (
    <div className="cm-head" aria-hidden="true">
      <div className={`cm-covers ${covers.length > 1 ? "stacked" : ""} ${header.round ? "round" : ""}`}>
        {/* Back to front, so the first item's cover is on top. */}
        {[...covers].reverse().map((c, k) => <Artwork key={`${c ?? "none"}-${k}`} id={c} size={64} className="cm-cover" />)}
      </div>
      <div className="cm-head-text">
        <span className="cm-head-title truncate">{header.title}</span>
        {header.subtitle ? <span className="cm-head-sub truncate">{header.subtitle}</span> : null}
      </div>
    </div>
  );
}

/** The first enabled cell of a freshly drawn menu (its cells carry ids in order). */
function firstEnabled(menu: HTMLElement): number {
  const el = menu.querySelector<HTMLElement>("[id^='cm-item-']:not([aria-disabled='true'])");
  return el ? Number(el.id.slice("cm-item-".length)) : -1;
}
