// The sidebar's "Your library": every playlist and the albums and artists played lately, under a
// header with a filter field, the sort and New playlist, and Playlists / Albums / Artists chips.
// The header sticks while the sidebar scrolls; pinned items are left out (the pins sit above).
// As a rail (collapsed, or a narrow window) it is just the covers. lib/sidebar-library.ts has the
// rules for what is listed and in what order.
import { useEffect, useMemo, useRef, useState } from "react";
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { useRoving } from "../lib/roving";
import { libraryDragProps } from "../lib/queue-drag";
import { LIBRARY_KINDS, LIBRARY_SORTS, collectLibrary, entryKey, parseLibraryView, viewLibrary, type CurrentContext, type LibraryEntry, type LibraryKind, type LibrarySort, type LibraryView } from "../lib/sidebar-library";
import { useDetails } from "./ListGroups";
import { openContextMenu, openItemsMenuFromButton } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";

const VIEW_KEY = "sidebarLibrary";
const KIND_CHIP: Record<LibraryKind, "nav.playlists" | "nav.albums" | "nav.artists"> = { playlist: "nav.playlists", album: "nav.albums", artist: "nav.artists" };
const SORT_LABEL: Record<LibrarySort, "library.sort.recents" | "library.sort.added" | "library.sort.alpha"> = { recents: "library.sort.recents", added: "library.sort.added", alpha: "library.sort.alpha" };

const targetOf = (e: LibraryEntry): ActionTarget =>
  e.kind === "album" ? { type: "albums", data: { ids: [e.id] } } : e.kind === "artist" ? { type: "artists", data: { ids: [e.id] } } : { type: "playlists", data: { ids: [e.id] } };

/** The library's items, as the sidebar lists them before its view (kind, sort, filter) applies. */
export function useLibraryEntries(): LibraryEntry[] {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const queues = useApp((s) => s.savedQueues);
  const nowId = useApp((s) => s.session?.current?.trackId);
  const playlists = useQuery(() => (serverId ? { type: "playlists", data: { server_id: serverId } } : null), "playlists", [serverId]).data;
  // The history moves on as songs change, so what was just played comes to the top.
  const history = useQuery(() => ({ type: "recentlyPlayed", data: { limit: 300 } }), "history", [nowId]).data;
  // What plays now counts as just played, before its songs reach the history.
  const context = useApp((s) => s.session?.context);
  const cover = useApp((s) => s.nowPlaying?.track.coverArt);
  const contextKey = context ? JSON.stringify(context.kind) : "";
  // When this context started playing: the first render that saw it.
  const started = useRef({ key: "", at: 0 });
  if (started.current.key !== contextKey) started.current = { key: contextKey, at: Date.now() };
  const startedAt = started.current.at;
  const current = useMemo<CurrentContext | undefined>(() => (context ? { kind: context.kind, label: context.label, coverArt: cover, at: startedAt } : undefined), [contextKey, context?.label, cover, startedAt]); // eslint-disable-line react-hooks/exhaustive-deps
  const entries = useMemo(() => collectLibrary(playlists ?? [], queues, history ?? [], current), [playlists, queues, history, current]);
  // Artists have no cover of their own in the history: their details carry the picture.
  const artistIds = useMemo(() => entries.filter((e) => e.kind === "artist" && !e.coverArt).map((e) => e.id), [entries]);
  const artists = useDetails("artist", artistIds);
  return useMemo(() => {
    if (!artists.length) return entries;
    const covers = new Map(artists.map((a) => [a.id, a.coverArt]));
    return entries.map((e) => (e.kind === "artist" && !e.coverArt && covers.get(e.id) ? { ...e, coverArt: covers.get(e.id) } : e));
  }, [entries, artists]);
}

/** [entries]: useLibraryEntries(), from the Sidebar (which also needs them for its highlight). */
export function SidebarLibrary({ rail, pinned, entries }: { rail: boolean; pinned: ReadonlySet<string>; entries: LibraryEntry[] }) {
  const navigate = useApp((s) => s.navigate);
  const route = useApp((s) => s.route);
  const playingKey = useApp((s) => {
    const k = s.session?.context?.kind;
    return k && (k.type === "album" || k.type === "artist" || k.type === "playlist") ? `${k.type}:${k.data.id}` : undefined;
  });
  const isPlaying = useApp((s) => s.transport.position.isPlaying);
  const [view, setView] = useState<LibraryView>(() => parseLibraryView(loadLocal(VIEW_KEY, {})));
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const field = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const roving = useRoving(list);
  const shown = useMemo(() => viewLibrary(entries, pinned, { ...view, query: rail ? "" : query }), [entries, pinned, view, query, rail]);

  const update = (patch: Partial<LibraryView>) => {
    const next = { ...view, ...patch };
    setView(next);
    saveLocal(VIEW_KEY, { kind: next.kind, sort: next.sort });
  };
  useEffect(() => { if (searching) field.current?.focus(); }, [searching]);
  const closeSearch = () => { setSearching(false); setQuery(""); };

  const sortMenu = (e: React.MouseEvent<HTMLElement>) => openItemsMenuFromButton(e, [
    { id: "sortBy", label: t("library.sortBy"), icon: "sort", enabled: false, heading: true, run: () => {} },
    ...LIBRARY_SORTS.map((s) => ({ id: `sort.${s}`, label: t(SORT_LABEL[s]), icon: "check", enabled: true, checked: view.sort === s, run: () => update({ sort: s }) })),
  ]);

  return (
    <section className={`side-lib ${rail ? "rail" : ""}`} aria-labelledby="side-lib-title" data-testid="sidebar-library">
      {rail ? <h2 className="sr-only" id="side-lib-title">{t("library.yours")}</h2> : (
        <div className="side-lib-head">
          <div className="side-lib-titlebar">
            <h2 className="side-lib-title" id="side-lib-title">{t("library.yours")}</h2>
            <button type="button" className={`btn icon sm ${searching ? "on" : ""}`} aria-label={t("library.searchYours")} aria-pressed={searching} title={t("library.searchYours")} onClick={() => (searching ? closeSearch() : setSearching(true))} data-testid="side-lib-search"><Icon name="search" size={18} /></button>
            <button type="button" className="btn sm side-lib-sort" aria-haspopup="menu" title={t("library.sortBy")} onClick={sortMenu} data-testid="side-lib-sort">
              <span>{t(SORT_LABEL[view.sort])}</span><Icon name="sort" size={18} />
            </button>
            <button type="button" className="btn icon sm" aria-label={t("library.createPlaylist")} title={t("library.createPlaylist")} onClick={() => void executeAction("ui.newPlaylist")} data-testid="side-lib-new"><Icon name="plus" size={20} /></button>
          </div>
          {searching ? (
            <div className="side-lib-filter">
              <Icon name="search" size={16} />
              <input ref={field} className="input" type="search" value={query} placeholder={t("library.filterYours")} aria-label={t("library.filterYours")}
                onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); closeSearch(); } }} data-testid="side-lib-filter" />
            </div>
          ) : null}
          <div className="side-lib-chips" role="group" aria-label={t("library.show")}>
            {LIBRARY_KINDS.map((k) => (
              <button key={k} type="button" className={`side-lib-chip ${view.kind === k ? "on" : ""}`} aria-pressed={view.kind === k} onClick={() => update({ kind: view.kind === k ? undefined : k })} data-testid={`side-lib-chip-${k}`}>{t(KIND_CHIP[k])}</button>
            ))}
          </div>
        </div>
      )}
      {shown.length ? (
        <ul ref={list} className="plain-list side-lib-list" aria-labelledby="side-lib-title" {...roving}>
          {shown.map((e) => {
            const key = entryKey(e);
            const active = route.view === e.kind && route.id === e.id;
            const playing = key === playingKey;
            const kind = t(`library.kind.${e.kind}` as never);
            const sub = e.owner ? `${kind} · ${e.owner}` : kind;
            return (
              <li key={key}>
                <a href="#" data-roving className={`nav-item lib-item ${active ? "active" : ""} ${playing ? "playing" : ""}`} aria-current={active ? "page" : undefined}
                  title={rail ? `${e.title} · ${sub}` : undefined}
                  onClick={(ev) => { ev.preventDefault(); navigate({ view: e.kind, id: e.id }); }}
                  onContextMenu={(ev) => void openContextMenu(ev, targetOf(e))}
                  {...libraryDragProps(targetOf(e))}
                  data-testid="sidebar-library-item" data-entry={key}>
                  <span className={`lib-thumb ${e.kind}`} aria-hidden="true"><Artwork id={e.coverArt} size={96} round={e.kind === "artist"} /></span>
                  <span className="lib-text">
                    <span className="lib-title">{e.title}</span>
                    <span className="lib-sub">{sub}</span>
                  </span>
                  {playing ? <Icon name={isPlaying ? "equalizer" : "volume"} size={16} className="lib-playing" title={t("library.playingNow")} /> : null}
                </a>
              </li>
            );
          })}
        </ul>
      ) : rail ? null : (
        <p className="side-lib-empty small muted" data-testid="side-lib-empty">{query.trim() ? t("library.yoursNoMatch", { query: query.trim() }) : t("library.yoursEmpty")}</p>
      )}
    </section>
  );
}
