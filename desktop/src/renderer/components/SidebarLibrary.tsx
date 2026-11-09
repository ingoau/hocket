// The sidebar's "Your library": every playlist and the albums and artists played lately, under a
// header with a filter field, the sort and Create playlist, and Playlists / Albums / Artists chips.
// The header sticks while the sidebar scrolls; pinned items are left out (the pins sit above).
// As a rail (collapsed, or a narrow window) it is just the covers, of everything. The rows are
// virtualised against the sidebar's scroller, so a library of thousands of playlists draws (and
// fetches the artwork of) only what is on screen. lib/sidebar-library.ts has the rules for what is
// listed and in what order.
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { bridge } from "../core/bridge";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { useRoving } from "../lib/roving";
import { libraryDragProps } from "../lib/queue-drag";
import { NARROW, useMediaQuery } from "../lib/media";
import { LIBRARY_KINDS, LIBRARY_SORTS, collectLibrary, entryKey, parseLibraryView, viewLibrary, type CurrentContext, type LibraryEntry, type LibraryKind, type LibrarySort, type LibraryView } from "../lib/sidebar-library";
import { openContextMenu, openItemsMenuFromButton } from "./ContextMenu";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";

const VIEW_KEY = "sidebarLibrary";
const KIND_CHIP: Record<LibraryKind, "nav.playlists" | "nav.albums" | "nav.artists"> = { playlist: "nav.playlists", album: "nav.albums", artist: "nav.artists" };
const SORT_LABEL: Record<LibrarySort, "library.sort.recents" | "library.sort.added" | "library.sort.alpha"> = { recents: "library.sort.recents", added: "library.sort.added", alpha: "library.sort.alpha" };
/** Row heights (panes.css: .lib-item, and the rail's, and the narrow rail's). */
const ROW_H = 56;
const RAIL_ROW_H = 52;
const NARROW_RAIL_ROW_H = 46;

const targetOf = (e: LibraryEntry): ActionTarget =>
  e.kind === "album" ? { type: "albums", data: { ids: [e.id] } } : e.kind === "artist" ? { type: "artists", data: { ids: [e.id] } } : { type: "playlists", data: { ids: [e.id] } };

/**
 * Artists' pictures, by id, for the ones the history names (it carries only album covers). Kept for
 * the session: a new artist is fetched once, the ones already known never again, so a change of
 * song doesn't refetch (or blank) the covers.
 */
const artistCovers = new Map<string, string | null>();
const artistPending = new Set<string>();

function useArtistCovers(ids: readonly string[]): ReadonlyMap<string, string | null> {
  const [, setVersion] = useState(0);
  const key = ids.join(",");
  useEffect(() => {
    const missing = ids.filter((id) => !artistCovers.has(id) && !artistPending.has(id));
    if (!missing.length) return;
    let live = true;
    for (const id of missing) artistPending.add(id);
    void Promise.all(missing.map((id) => bridge().query({ type: "artist", data: { id } })
      .then((r) => { artistCovers.set(id, r.type === "artistDetail" ? r.data?.coverArt ?? null : null); })
      .catch(() => { artistCovers.set(id, null); })
      .finally(() => artistPending.delete(id))))
      .then(() => { if (live) setVersion((v) => v + 1); });
    return () => { live = false; };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  return artistCovers;
}

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
  const artistIds = useMemo(() => entries.filter((e) => e.kind === "artist" && !e.coverArt).map((e) => e.id), [entries]);
  const covers = useArtistCovers(artistIds);
  const known = artistIds.filter((id) => covers.get(id)).length;
  return useMemo(
    () => (known ? entries.map((e) => (e.kind === "artist" && !e.coverArt && covers.get(e.id) ? { ...e, coverArt: covers.get(e.id) ?? undefined } : e)) : entries),
    [entries, covers, known],
  );
}

export interface LibraryViewState {
  view: LibraryView;
  update(patch: Partial<LibraryView>): void;
  searching: boolean;
  setSearching(on: boolean): void;
  query: string;
  setQuery(q: string): void;
}

/**
 * Your library's view: the kind and sort (remembered on the device) and the filter field. The
 * Sidebar holds it, so its highlight follows what the list shows. As a rail the filter closes.
 */
export function useLibraryView(rail: boolean): LibraryViewState {
  const [view, setView] = useState<LibraryView>(() => parseLibraryView(loadLocal(VIEW_KEY, {})));
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  useEffect(() => {
    if (!rail) return;
    setSearching(false);
    setQuery("");
  }, [rail]);
  const update = useCallback((patch: Partial<LibraryView>) => {
    setView((cur) => {
      const next = { ...cur, ...patch };
      saveLocal(VIEW_KEY, { kind: next.kind, sort: next.sort });
      return next;
    });
  }, []);
  return useMemo(() => ({ view, update, searching, setSearching, query, setQuery }), [view, update, searching, query]);
}

/** What the list shows for a view: as a rail, everything (newest first or as sorted), unfiltered. */
export function shownLibrary(entries: readonly LibraryEntry[], pinned: ReadonlySet<string>, lib: LibraryViewState, rail: boolean): LibraryEntry[] {
  return viewLibrary(entries, pinned, rail ? { sort: lib.view.sort } : { ...lib.view, query: lib.query });
}

/** [shown]: the rows (shownLibrary); [scroller]: the sidebar's scrolling column, which the rows are virtualised against. */
export function SidebarLibrary({ rail, shown, lib, scroller }: { rail: boolean; shown: LibraryEntry[]; lib: LibraryViewState; scroller: RefObject<HTMLDivElement | null> }) {
  const routeKey = useApp((s) => (s.route.id ? `${s.route.view}:${s.route.id}` : ""));
  const playingKey = useApp((s) => {
    const k = s.session?.context?.kind;
    return k && (k.type === "album" || k.type === "artist" || k.type === "playlist") ? `${k.type}:${k.data.id}` : undefined;
  });
  const isPlaying = useApp((s) => s.transport.position.isPlaying);
  const narrow = useMediaQuery(NARROW);
  const { view, update, searching, setSearching, query, setQuery } = lib;
  const field = useRef<HTMLInputElement>(null);
  const searchBtn = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const roving = useRoving(list);
  const rowH = rail ? (narrow ? NARROW_RAIL_ROW_H : RAIL_ROW_H) : ROW_H;

  // Where the list starts in the scroller's content: the places, pins and this header sit above it.
  const [margin, setMargin] = useState(0);
  const measure = useCallback(() => {
    const el = scroller.current;
    const ul = list.current;
    if (!el || !ul) return;
    const m = Math.max(0, Math.round(ul.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop));
    setMargin((cur) => (cur === m ? cur : m));
  }, [scroller]);
  useLayoutEffect(measure);
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(measure);
    for (const child of Array.from(el.children)) ro.observe(child);
    return () => ro.disconnect();
  }, [scroller, measure]);
  const virt = useVirtualizer({ count: shown.length, getScrollElement: () => scroller.current, estimateSize: () => rowH, overscan: 8, scrollMargin: margin });
  useEffect(() => virt.measure(), [rowH, virt]);

  useEffect(() => { if (searching) field.current?.focus(); }, [searching]);
  // Closing the filter hands focus back to its toggle (it would otherwise fall to the page).
  const closeSearch = () => {
    setSearching(false);
    setQuery("");
    searchBtn.current?.focus();
  };

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
            <button ref={searchBtn} type="button" className={`btn icon sm ${searching ? "on" : ""}`} aria-label={t("library.searchYours")} aria-pressed={searching} title={t("library.searchYours")} onClick={() => (searching ? closeSearch() : setSearching(true))} data-testid="side-lib-search"><Icon name="search" size={18} /></button>
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
        <ul ref={list} className="plain-list side-lib-list" aria-labelledby="side-lib-title" style={{ height: virt.getTotalSize(), position: "relative" }} {...roving}>
          {virt.getVirtualItems().map((v) => {
            const e = shown[v.index]!;
            const key = entryKey(e);
            return (
              <LibraryRow key={key} entry={e} rail={rail} top={v.start - margin} height={rowH} active={key === routeKey} playing={key === playingKey} isPlaying={key === playingKey && isPlaying} />
            );
          })}
        </ul>
      ) : rail ? null : (
        <p className="side-lib-empty small muted" data-testid="side-lib-empty">{query.trim() ? t("library.yoursNoMatch", { query: query.trim() }) : t("library.yoursEmpty")}</p>
      )}
    </section>
  );
}

/** One row: memoised, so a change of route or song redraws only the rows it touches. */
const LibraryRow = memo(function LibraryRow({ entry: e, rail, top, height, active, playing, isPlaying }: { entry: LibraryEntry; rail: boolean; top: number; height: number; active: boolean; playing: boolean; isPlaying: boolean }) {
  const navigate = useApp((s) => s.navigate);
  const key = entryKey(e);
  const kind = t(`library.kind.${e.kind}` as never);
  const sub = e.owner ? `${kind} · ${e.owner}` : kind;
  const target = useMemo(() => targetOf(e), [e]);
  return (
    <li style={{ position: "absolute", top: 0, left: 0, right: 0, height, transform: `translateY(${top}px)` }}>
      <a href="#" data-roving className={`nav-item lib-item ${active ? "active" : ""} ${playing ? "playing" : ""}`} aria-current={active ? "page" : undefined}
        title={rail ? `${e.title} · ${sub}` : undefined}
        onClick={(ev) => { ev.preventDefault(); navigate({ view: e.kind, id: e.id }); }}
        onContextMenu={(ev) => void openContextMenu(ev, target)}
        {...libraryDragProps(target)}
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
});
