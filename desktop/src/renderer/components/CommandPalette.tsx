// Ctrl/Cmd+K: the command palette, Raycast-style. One search over the library (the core's local
// search, so it answers at once) and the app's commands, in groups: Suggestions (recent picks, with
// no query), Songs, Albums, Artists, Playlists, Commands. ↑↓ move, Enter runs the row's primary
// action (play a song, open an album, artist or playlist, run a command). Ctrl/Cmd+K again opens
// the actions menu for the row, anchored bottom-right: the context menu's actions (lib/palette-model
// over lib/menu-model), each with its chord as keycaps and its own filter field. Those chords also
// work straight from the search field. Escape closes the menu, then the palette.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { ActionDescriptor, SearchResults } from "@core/api";
import { canonicalActionId } from "@shared/keymap";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { type Ranked } from "../store/palette-rank";
import { executeAction } from "../store/actions";
import { fetchActions } from "../store/queries";
import { useKeymap } from "../store/keyboard";
import { chordFromEvent, parseChord, type Chord } from "../store/shortcuts";
import { Artwork } from "./Artwork";
import { LyricSnippet } from "./LyricSnippet";
import { openContextMenu, openContextMenuFromKeyboard } from "./ContextMenu";
import { refOf } from "../lib/track-ref";
import { Icon, hasIcon } from "./Icon";
import { trapTab, useReturnFocus } from "../lib/focus";
import { usePresence } from "../lib/presence";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { fetchMenuFacts } from "../lib/menu-facts";
import { buildMenu } from "../lib/menu-model";
import {
  actionForChord, ariaShortcut, entryId, filterActions, flatRows, groupLabel, isPaletteChord, keycaps, menuActions, paletteActions,
  paletteGroups, pushRecent, rawId, sanitizeRecents, targetOf, typeLabel, type PaletteAction, type PaletteEntry,
} from "../lib/palette-model";

const RECENTS_KEY = "palette.recent";
type Platform = "macOs" | "linux" | "windows";

interface ActionsMenu {
  entry: PaletteEntry;
  /** Undefined while the row's actions load. */
  groups?: PaletteAction[][];
  filter: string;
  active: number;
}

export function CommandPalette() {
  const open = useApp((s) => s.paletteOpen);
  const setOpen = useApp((s) => s.setPaletteOpen);
  const serverId = useApp((s) => s.accounts[0]?.id);
  // A dialog a result's menu opened (delete, rename, …) may have changed the results: search again once it closes.
  const dialogOpen = useApp((s) => !!s.dialog);
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const navigate = useApp((s) => s.navigate);
  const keymap = useKeymap();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResults | undefined>(undefined);
  const [descriptors, setDescriptors] = useState<ActionDescriptor[]>([]);
  const [recents, setRecents] = useState<PaletteEntry[]>([]);
  const [active, setActive] = useState(0);
  const [menu, setMenu] = useState<ActionsMenu | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const filterInput = useRef<HTMLInputElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  const seq = useRef(0);
  /** Bumped per opening: a run that resolves after the palette closed (or reopened) is dropped. */
  const session = useRef(0);
  /** A deferred run is in flight: a second Enter doesn't run it twice. */
  const pending = useRef(false);
  /** The press that closed the actions menu: its click activates nothing. */
  const dismissed = useRef(false);
  /** Each row's actions, fetched once per opening (prefetched for the active row). */
  const loading = useRef(new Map<string, Promise<PaletteAction[][]>>());
  const loaded = useRef(new Map<string, PaletteAction[][]>());
  // The closed palette stays mounted, inert, while its exit plays.
  const { value: shown, closing, motion, exitProps } = usePresence<true, HTMLDivElement>(open || undefined);
  useReturnFocus(open);

  // Reset as it opens, in the same render: the first frame never shows the last session's query
  // (typing it again would then change nothing).
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    setWasOpen(open);
    if (open) {
      setQuery("");
      setActive(0);
      setResults(undefined);
      setMenu(null);
    }
  }

  useLayoutEffect(() => {
    if (!open) return;
    session.current++;
    pending.current = false;
    loading.current.clear();
    loaded.current.clear();
    setRecents(sanitizeRecents(loadLocal<unknown>(RECENTS_KEY, [])));
    void bridge().query({ type: "actions", data: { surface: "palette", target: { type: "none" } } }).then((r) => r.type === "actions" && setDescriptors(r.data));
    // At once, not on a timer: the shell is inert now but keeps focus until the
    // browser's focus fixup, and a key typed meanwhile (Escape) would go to it.
    input.current?.focus();
  }, [open]);

  useEffect(() => {
    if (!open || !serverId || !query.trim()) {
      setResults(undefined);
      return;
    }
    const mine = ++seq.current;
    const id = `pal-${mine}`;
    // The local index only: it answers at once, and the palette never waits on the network.
    const h = setTimeout(() => {
      void bridge().query({ type: "search", data: { server_id: "", query, limit: 8, include_server: false, request_id: id } }).then((r) => mine === seq.current && r.type === "search" && setResults(r.data));
    }, 40);
    return () => clearTimeout(h);
  }, [open, query, serverId, dialogOpen]);

  const commands = useMemo<PaletteEntry[]>(() => descriptors.filter((a) => a.enabled).map((a) => ({
    id: entryId("action", a.id),
    label: a.label,
    kind: "action",
    icon: a.icon,
    shortcut: shortcuts.find((s) => s.actionId === a.id)?.shortcut ?? a.defaultShortcut,
  })), [descriptors, shortcuts]);

  const groups = useMemo(() => {
    // A slower answer for the previous letters must not stay on top of Enter.
    const found = results?.query.trim().toLowerCase() === query.trim().toLowerCase() ? results : undefined;
    return paletteGroups(query, {
    recents,
    commands,
    tracks: (found?.tracks ?? []).map((x) => ({ id: entryId("track", x.id), label: x.title, sub: [x.artist, x.album].filter(Boolean).join(" · "), kind: "track", coverArt: x.coverArt, ref: refOf(x) })),
    lyrics: (found?.lyrics ?? []).map((h) => ({ id: `ly:${h.track.id}`, label: h.track.title, sub: h.line.split("\n").join(" "), kind: "track", coverArt: h.track.coverArt, ref: refOf(h.track), lyric: { line: h.line, start: h.matchStart, end: h.matchEnd, startMs: h.startMs ?? undefined } })),
    albums: (found?.albums ?? []).map((x) => ({ id: entryId("album", x.id), label: x.name, sub: x.artist, kind: "album", coverArt: x.coverArt })),
    artists: (found?.artists ?? []).map((x) => ({ id: entryId("artist", x.id), label: x.name, kind: "artist", coverArt: x.coverArt })),
    playlists: (found?.playlists ?? []).map((x) => ({ id: entryId("playlist", x.id), label: x.name, kind: "playlist", coverArt: x.coverArt })),
    });
  }, [query, recents, commands, results]);
  const rows = useMemo(() => flatRows(groups), [groups]);
  const current = rows[active]?.item;

  useEffect(() => setActive(0), [rows.length, query]);
  useEffect(() => {
    if (open) document.getElementById(`pal-opt-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [active, open]);

  /** A row's actions menu, from the same descriptors and facts the context menu opens with. */
  const load = (entry: PaletteEntry): Promise<PaletteAction[][]> => {
    let p = loading.current.get(entry.id);
    if (!p) {
      const target = targetOf(entry);
      p = target
        ? Promise.all([fetchActions("contextMenu", target), fetchMenuFacts(target)]).then(([all, facts]) => {
            const registry = menuActions(all);
            return paletteActions(entry, buildMenu(registry, [], facts), registry);
          })
        : Promise.resolve(paletteActions(entry, [], []));
      p = p.then((g) => {
        loaded.current.set(entry.id, g);
        return g;
      }, () => {
        loading.current.delete(entry.id);
        return [];
      });
      loading.current.set(entry.id, p);
    }
    return p;
  };

  // Prefetch the active row's actions, so its chords and its menu answer at once.
  const currentId = current?.id;
  useEffect(() => {
    if (!open || !current || current.kind === "action") return;
    const h = setTimeout(() => void load(current), 120);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the row, not its object
  }, [open, currentId]);

  useEffect(() => {
    if (menu) filterInput.current?.focus();
  }, [menu?.entry.id]); // eslint-disable-line react-hooks/exhaustive-deps -- once per menu

  if (!shown) return null;

  const remember = (entry: PaletteEntry) => {
    const next = pushRecent(recents, entry);
    setRecents(next);
    saveLocal(RECENTS_KEY, next);
  };
  const openPage = (entry: PaletteEntry) => {
    const id = rawId(entry);
    if (entry.kind === "album") navigate({ view: "album", id });
    else if (entry.kind === "artist") navigate({ view: "artist", id });
    else if (entry.kind === "playlist") navigate({ view: "playlist", id });
  };
  /** Close, then run: the palette's return focus yields to a dialog or picker the action opens. */
  const perform = (entry: PaletteEntry, action: PaletteAction) => {
    remember(entry);
    setOpen(false);
    const run = action.run;
    if (run.type === "open") openPage(entry);
    else if (run.type === "command") void executeAction(run.id);
    else {
      const target = targetOf(entry);
      if (target) void executeAction(run.id, target);
    }
  };
  /** Enter: a song plays (the registry's Play, as the context menu's), anything else opens or runs. */
  const runPrimary = (entry: PaletteEntry) => {
    if (entry.kind === "action") return perform(entry, { key: "run", label: entry.label, run: { type: "command", id: rawId(entry) } });
    if (entry.kind !== "track") return perform(entry, { key: "open", label: "", run: { type: "open" } });
    // A lyric hit plays from the line that matched.
    if (entry.lyric) return playAlone(entry, entry.lyric.startMs ?? 0);
    const s = session.current;
    if (pending.current) return;
    pending.current = true;
    void load(entry).then((g) => {
      pending.current = false;
      if (s !== session.current || !useApp.getState().paletteOpen) return;
      const primary = g.flat().find((a) => a.shortcut === "Enter");
      if (primary) return perform(entry, primary);
      // No registry Play: play the song alone, as the palette always has.
      playAlone(entry, 0);
    });
  };
  const playAlone = (entry: PaletteEntry, from: number) => {
    remember(entry);
    setOpen(false);
    if (entry.ref) bridge().dispatch({ type: "playTracks", data: { tracks: [entry.ref], start_index: 0, label: entry.label, shuffle: false, start_position_ms: from } });
  };
  /** A menu chord pressed on a row (from the field or the menu): run its action if the row has one. */
  const runChord = (entry: PaletteEntry, chord: Chord, e: React.KeyboardEvent): boolean => {
    const ready = loaded.current.get(entry.id);
    if (ready) {
      const a = actionForChord(ready, chord);
      if (!a) return false;
      e.preventDefault();
      perform(entry, a);
      return true;
    }
    if (!isPaletteChord(chord)) return false;
    e.preventDefault();
    const s = session.current;
    if (pending.current) return true;
    pending.current = true;
    void load(entry).then((g) => {
      pending.current = false;
      const a = actionForChord(g, chord);
      if (a && s === session.current && useApp.getState().paletteOpen) perform(entry, a);
    });
    return true;
  };

  const openMenu = (entry: PaletteEntry) => {
    setMenu({ entry, groups: loaded.current.get(entry.id), filter: "", active: 0 });
    void load(entry).then((g) => setMenu((m) => (m && m.entry.id === entry.id ? { ...m, groups: g } : m)));
  };
  const closeMenu = () => {
    setMenu(null);
    input.current?.focus({ preventScroll: true });
  };
  const menuShown = menu?.groups ? filterActions(menu.groups, menu.filter) : [];
  const menuFlat = menuShown.flat();

  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    e.stopPropagation();
    const chord = chordFromEvent(e, platform);
    const toggle = !!chord && (keymap.lookup(chord).includes("openCommandPalette") || (chord.mod && !chord.alt && !chord.shift && chord.key === "K"));
    const plain = !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey;
    // Enter and Space on a focused button press it (the footer's Actions, Play).
    if ((e.key === "Enter" || e.key === " ") && (e.target as HTMLElement).tagName === "BUTTON") return;
    if (menu) {
      if (e.key === "Escape" || toggle) { e.preventDefault(); closeMenu(); }
      else if (e.key === "ArrowDown") { e.preventDefault(); setMenu({ ...menu, active: Math.min(menuFlat.length - 1, menu.active + 1) }); }
      else if (e.key === "ArrowUp") { e.preventDefault(); setMenu({ ...menu, active: Math.max(0, menu.active - 1) }); }
      else if (e.key === "Enter" && plain) { e.preventDefault(); const a = menuFlat[menu.active]; if (a) perform(menu.entry, a); }
      else if (chord && runChord(menu.entry, chord, e)) { /* ran */ }
      // Tab stays in the menu: its filter is the one stop.
      else if (e.key === "Tab") { e.preventDefault(); filterInput.current?.focus(); }
      return;
    }
    if (e.key === "Escape") { e.preventDefault(); setOpen(false); }
    else if (toggle) { e.preventDefault(); if (current) openMenu(current); else setOpen(false); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(rows.length - 1, a + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(0, a - 1)); }
    else if (e.key === "Enter" && plain) { e.preventDefault(); if (current) runPrimary(current); }
    else if (current && chord && runChord(current, chord, e)) { /* ran */ }
    // The global handler leaves these to text fields; the input points at the active option, so open its context menu.
    else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey)) { if (openContextMenuFromKeyboard()) e.preventDefault(); }
    else trapTab(e);
  };

  const paletteChord = shortcuts.find((s) => canonicalActionId(s.actionId) === "openCommandPalette")?.shortcut ?? "Mod+K";
  const primaryLabel = !current ? undefined : current.kind === "action" ? t("palette.runCommand") : current.kind === "track" ? t("menu.play") : t(current.kind === "album" ? "palette.open.album" : current.kind === "artist" ? "palette.open.artist" : "palette.open.playlist");
  let index = -1;
  let actionIndex = -1;
  return (
    <div className={`overlay ${closing ? "closing" : ""}`} inert={closing} onMouseDown={(e) => e.target === e.currentTarget && setOpen(false)}>
      <div
        {...exitProps}
        className={`palette ${motion}`}
        role="dialog"
        aria-modal="true"
        aria-label={t("action.palette")}
        onKeyDown={onKey}
        onMouseDown={(e) => {
          dismissed.current = false;
          if (menu && !popover.current?.contains(e.target as Node)) { e.preventDefault(); dismissed.current = true; closeMenu(); }
        }}
        onClickCapture={(e) => { if (dismissed.current) { dismissed.current = false; e.stopPropagation(); e.preventDefault(); } }}
        data-testid="palette"
      >
        <div className="palette-search">
          <Icon name="search" size={20} />
          <input ref={input} className="input" placeholder={t("palette.placeholder")} value={query} onChange={(e) => setQuery(e.target.value)} role="combobox" aria-label={t("action.palette")} aria-autocomplete="list" aria-expanded={rows.length > 0} aria-controls="palette-list" aria-activedescendant={rows[active] ? `pal-opt-${active}` : undefined} data-testid="palette-input" />
        </div>
        {rows.length === 0 ? <div className="group palette-empty" role="status">{t("palette.empty")}</div> : null}
        <div className="list" id="palette-list" role="listbox" aria-label={t("a11y.paletteResults")}>
          {groups.map((g) => (
            <div key={g.id} role="group" aria-labelledby={`pal-group-${g.id}`} data-testid={`palette-group-${g.id}`}>
              <div className="group" id={`pal-group-${g.id}`} role="presentation">{groupLabel(g.id)}</div>
              {g.rows.map((r) => {
                const i = ++index;
                return <Row key={r.item.id} row={r} index={i} active={i === active} platform={platform} onHover={() => { if (!menu) setActive(i); }} onRun={() => runPrimary(r.item)} onContextMenu={(e) => {
                  // The app's own context menu for a library result, as anywhere else.
                  const target = targetOf(r.item);
                  if (!target) return;
                  setActive(i);
                  input.current?.focus();
                  void openContextMenu(e, target);
                }} />;
              })}
            </div>
          ))}
        </div>
        <div className="palette-footer">
          <span className="grow" />
          {primaryLabel ? (
            <button type="button" className="pf-btn" tabIndex={-1} onClick={() => current && runPrimary(current)} data-testid="palette-primary">
              {primaryLabel}<Keycaps shortcut="Enter" platform={platform} />
            </button>
          ) : null}
          <span className="pf-sep" aria-hidden="true" />
          <button type="button" className="pf-btn" disabled={!current} aria-expanded={!!menu} aria-haspopup="dialog" onMouseDown={(e) => e.stopPropagation()} onClick={() => (menu ? closeMenu() : current && openMenu(current))} data-testid="palette-actions-button">
            {t("palette.actionsButton")}<Keycaps shortcut={paletteChord} platform={platform} />
          </button>
        </div>
        {menu ? (
          <div ref={popover} className="palette-actions" role="dialog" aria-label={t("palette.actionsFor", { name: menu.entry.label })} data-testid="palette-actions" onMouseDown={(e) => { if (e.target !== filterInput.current) e.preventDefault(); }}>
            <div className="pa-title truncate">{menu.entry.label}</div>
            {menu.groups && !menuFlat.length ? <div className="pa-empty" role="status">{t("palette.actionsEmpty")}</div> : null}
            <div className="pa-list" id="palette-actions-list" role="listbox" aria-label={t("palette.actionsFor", { name: menu.entry.label })}>
              {menuShown.map((g, gi) => (
                <div key={gi} role="group" className="pa-group">
                  {g.map((a) => {
                    const i = ++actionIndex;
                    return (
                      <div
                        key={a.key}
                        id={`pa-opt-${i}`}
                        role="option"
                        aria-selected={i === menu.active}
                        aria-keyshortcuts={ariaShortcut(a.shortcut, platform)}
                        className={`pa-item ${i === menu.active ? "active" : ""} ${a.destructive ? "destructive" : ""}`}
                        onMouseMove={(e) => { if ((e.movementX || e.movementY) && menu.active !== i) setMenu({ ...menu, active: i }); }}
                        onClick={() => perform(menu.entry, a)}
                        data-testid="palette-action"
                      >
                        <Icon name={a.icon && hasIcon(a.icon) ? a.icon : "command"} size={16} />
                        <span className="grow truncate">{a.label}</span>
                        {a.shortcut ? <Keycaps shortcut={a.shortcut} platform={platform} /> : null}
                      </div>
                    );
                  })}
                </div>
              ))}
            </div>
            <input
              ref={filterInput}
              className="pa-filter"
              placeholder={t("palette.actionsFilter")}
              value={menu.filter}
              onChange={(e) => setMenu({ ...menu, filter: e.target.value, active: 0 })}
              role="combobox"
              aria-label={t("palette.actionsFilter")}
              aria-autocomplete="list"
              aria-expanded={menuFlat.length > 0}
              aria-controls="palette-actions-list"
              aria-activedescendant={menuFlat[menu.active] ? `pa-opt-${menu.active}` : undefined}
              data-testid="palette-actions-filter"
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function Row({ row, index, active, platform, onHover, onRun, onContextMenu }: { row: Ranked<PaletteEntry>; index: number; active: boolean; platform: Platform; onHover: () => void; onRun: () => void; onContextMenu: (e: React.MouseEvent) => void }) {
  const it = row.item;
  const command = it.kind === "action";
  const chord = command && it.shortcut ? parseChord(it.shortcut) : undefined;
  return (
    <div
      id={`pal-opt-${index}`}
      className={`pi ${active ? "active" : ""}`}
      role="option"
      aria-selected={active}
      onMouseMove={(e) => { if (e.movementX || e.movementY) onHover(); }}
      // The search field keeps focus (and with it the keyboard).
      onMouseDown={(e) => e.preventDefault()}
      onClick={onRun}
      onContextMenu={onContextMenu}
      data-testid="palette-item"
      data-kind={it.kind}
    >
      {command ? <span className="pi-icon"><Icon name={it.icon && hasIcon(it.icon) ? it.icon : "command"} size={16} /></span> : <Artwork id={it.coverArt} size={64} className={`art ${it.kind === "artist" ? "round" : ""}`} />}
      <div className="grow truncate">
        <span className="pi-title truncate"><Highlight text={it.label} matches={row.matches} /></span>
        {it.lyric ? <span className="sub"><LyricSnippet line={it.lyric.line} start={it.lyric.start} end={it.lyric.end} /></span> : it.sub ? <span className="sub">{it.sub}</span> : null}
      </div>
      {chord && it.shortcut ? <Keycaps shortcut={it.shortcut} platform={platform} /> : null}
      <span className="pi-type">{typeLabel(it.kind)}</span>
    </div>
  );
}

function Keycaps({ shortcut, platform }: { shortcut: string; platform: Platform }) {
  return (
    <span className="keycaps" aria-hidden="true">
      {keycaps(shortcut, platform).map((k, i) => <kbd key={i} className="keycap">{k === "↵" ? <ReturnGlyph /> : k}</kbd>)}
    </span>
  );
}

/** ↵ drawn, not set: not every UI font has the glyph. The keycap's text stays ↵ (hidden). */
function ReturnGlyph() {
  return (
    <>
      <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M10 2.5v3.5a1.5 1.5 0 0 1-1.5 1.5H2.5M4.8 5.2 2.5 7.5l2.3 2.3" />
      </svg>
      <span hidden>↵</span>
    </>
  );
}

function Highlight({ text, matches }: { text: string; matches: number[] }) {
  if (!matches.length) return <>{text}</>;
  const set = new Set(matches);
  const parts: React.ReactNode[] = [];
  let buf = "";
  let inMatch = false;
  for (let i = 0; i <= text.length; i++) {
    const m = set.has(i);
    if (i === text.length || m !== inMatch) {
      if (buf) parts.push(inMatch ? <mark key={i}>{buf}</mark> : <span key={i}>{buf}</span>);
      buf = "";
      inMatch = m;
    }
    if (i < text.length) buf += text[i];
  }
  return <>{parts}</>;
}
