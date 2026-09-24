// Ctrl/Cmd+K: library results and actions in one list, results first.
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import type { ActionDescriptor, SearchResults } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { rank, type Rankable } from "../store/palette-rank";
import { executeAction } from "../store/actions";
import { formatChord, parseChord } from "../store/shortcuts";
import { Artwork } from "./Artwork";
import { Icon, hasIcon } from "./Icon";
import { trapTab, useReturnFocus } from "../lib/focus";

interface Item extends Rankable {
  icon?: string;
  coverArt?: string;
  shortcut?: string;
  run: () => void;
}

export function CommandPalette() {
  const open = useApp((s) => s.paletteOpen);
  const setOpen = useApp((s) => s.setPaletteOpen);
  const serverId = useApp((s) => s.servers[0]?.id);
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const navigate = useApp((s) => s.navigate);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResults | undefined>(undefined);
  const [actions, setActions] = useState<ActionDescriptor[]>([]);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const seq = useRef(0);
  useReturnFocus(open);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
    setResults(undefined);
    void bridge().query({ type: "actions", data: { surface: "palette", target: { type: "none" } } }).then((r) => r.type === "actions" && setActions(r.data));
    setTimeout(() => input.current?.focus(), 0);
  }, [open]);

  useEffect(() => {
    if (!open || !serverId || !query.trim()) {
      setResults(undefined);
      return;
    }
    const mine = ++seq.current;
    const id = `pal-${mine}`;
    const h = setTimeout(() => {
      void bridge().query({ type: "search", data: { server_id: serverId, query, limit: 8, include_server: false, request_id: id } }).then((r) => mine === seq.current && r.type === "search" && setResults(r.data));
    }, 60);
    return () => clearTimeout(h);
  }, [open, query, serverId]);

  const items = useMemo<Item[]>(() => {
    const out: Item[] = [];
    const sid = serverId ?? "";
    for (const tr of results?.tracks ?? []) out.push({ id: `t:${tr.id}`, label: tr.title, sub: [tr.artist, tr.album].filter(Boolean).join(" · "), kind: "track", coverArt: tr.coverArt, run: () => bridge().dispatch({ type: "playTracks", data: { server_id: sid, track_ids: [tr.id], start_index: 0, label: tr.title, shuffle: false } }) });
    for (const al of results?.albums ?? []) out.push({ id: `al:${al.id}`, label: al.name, sub: al.artist, kind: "album", coverArt: al.coverArt, run: () => navigate({ view: "album", id: al.id }) });
    for (const ar of results?.artists ?? []) out.push({ id: `ar:${ar.id}`, label: ar.name, kind: "artist", coverArt: ar.coverArt, run: () => navigate({ view: "artist", id: ar.id }) });
    for (const pl of results?.playlists ?? []) out.push({ id: `pl:${pl.id}`, label: pl.name, kind: "playlist", coverArt: pl.coverArt, run: () => navigate({ view: "playlist", id: pl.id }) });
    for (const a of actions) {
      if (!a.enabled) continue;
      const sc = shortcuts.find((s) => s.actionId === a.id)?.shortcut ?? a.defaultShortcut;
      out.push({ id: `act:${a.id}`, label: a.label, sub: a.category, kind: "action", icon: a.icon, shortcut: sc, run: () => void executeAction(a.id) });
    }
    return out;
  }, [results, actions, serverId, shortcuts, navigate]);

  const ranked = useMemo(() => rank(query, items, 40), [query, items]);
  useEffect(() => setActive(0), [ranked.length, query]);
  useEffect(() => {
    if (open) document.getElementById(`pal-opt-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [active, open]);

  if (!open) return null;
  const run = (i: Item) => {
    setOpen(false);
    i.run();
  };
  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    e.stopPropagation();
    if (e.key === "Escape") { e.preventDefault(); setOpen(false); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(ranked.length - 1, a + 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(0, a - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); const r = ranked[active]; if (r) run(r.item); }
    else trapTab(e);
  };
  const firstAction = ranked.findIndex((r) => r.item.kind === "action");
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && setOpen(false)}>
      <div className="palette fade-in" role="dialog" aria-modal="true" aria-label={t("action.palette")} onKeyDown={onKey} data-testid="palette">
        <input ref={input} className="input" placeholder={t("palette.placeholder")} value={query} onChange={(e) => setQuery(e.target.value)} role="combobox" aria-label={t("action.palette")} aria-autocomplete="list" aria-expanded={ranked.length > 0} aria-controls="palette-list" aria-activedescendant={ranked[active] ? `pal-opt-${active}` : undefined} data-testid="palette-input" />
        {ranked.length === 0 ? <div className="group" role="status">{t("palette.empty")}</div> : null}
        <div className="list" id="palette-list" role="listbox" aria-label={t("a11y.paletteResults")}>
          {ranked.map((r, i) => {
            const it = r.item;
            const chord = it.shortcut ? parseChord(it.shortcut) : undefined;
            return (
              <Fragment key={it.id}>
                {i === 0 && it.kind !== "action" ? <div className="group" role="presentation">{t("palette.results")}</div> : null}
                {i === firstAction ? <div className="group" role="presentation">{t("palette.actions")}</div> : null}
                <div id={`pal-opt-${i}`} className={`pi ${i === active ? "active" : ""}`} role="option" aria-selected={i === active} onMouseEnter={() => setActive(i)} onClick={() => run(it)} data-testid="palette-item">
                  {it.kind === "action" ? (hasIcon(it.icon ?? "") ? <Icon name={it.icon as string} size={16} /> : <Icon name="command" size={16} />) : <Artwork id={it.coverArt} size={64} className="art" />}
                  <div className="grow truncate">
                    <Highlight text={it.label} matches={r.matches} />
                    {it.sub ? <span className="sub"> · {it.sub}</span> : null}
                  </div>
                  {chord ? <span className="kbd">{formatChord(chord, platform)}</span> : null}
                </div>
              </Fragment>
            );
          })}
        </div>
      </div>
    </div>
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
