// The sidebar's "Your library": every playlist and the albums and artists played lately, under a
// header with a filter field, the sort and Create playlist, and Playlists / Albums / Artists chips.
// The header sticks while the sidebar scrolls; pinned items are left out (the pins sit above).
// As a rail (collapsed, or a narrow window) it is just the covers, of everything. The rows are
// virtualised against the sidebar's scroller, so a library of thousands of playlists draws (and
// fetches the artwork of) only what is on screen. lib/sidebar-library.ts has the rules for what is
// listed and in what order.
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import type { ActionTarget } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { bridge } from "../core/bridge";
import { loadLocal, saveLocal } from "../lib/local-settings";
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
 * Artists' pictures, for the ones the history names (it carries only album covers), keyed by server
 * and artist. An answer is kept until the library changes (then fetched again, the old picture shown
 * meanwhile); a failure isn't kept, so the next look tries again. One fetch per artist at a time,
 * and each one that lands tells every mounted list (StrictMode's second mount and a list whose ids
 * moved on included).
 */
const artistCovers = new Map<string, { cover: string | null; version: number }>();
const artistFetches = new Set<string>();
const artistListeners = new Set<() => void>();

function fetchArtistCover(key: string, id: string, version: number): void {
  if (artistFetches.has(key)) return;
  artistFetches.add(key);
  void bridge().query({ type: "artist", data: { id } })
    .then((r) => { if (r.type === "artistDetail") artistCovers.set(key, { cover: r.data?.coverArt ?? null, version }); })
    .catch(() => undefined)
    .finally(() => {
      artistFetches.delete(key);
      for (const l of artistListeners) l();
    });
}

/** The pictures of [ids] on [serverId] (a lookup), and a tick that moves whenever one arrives. */
function useArtistCovers(serverId: string, ids: readonly string[]): [(id: string) => string | undefined, number] {
  const [tick, setTick] = useState(0);
  const libraryVersion = useApp((s) => s.libraryVersion);
  useEffect(() => {
    const listener = () => setTick((n) => n + 1);
    artistListeners.add(listener);
    return () => { artistListeners.delete(listener); };
  }, []);
  const key = ids.join(",");
  useEffect(() => {
    if (!serverId) return;
    for (const id of ids) {
      const k = `${serverId}:${id}`;
      if (artistCovers.get(k)?.version !== libraryVersion) fetchArtistCover(k, id, libraryVersion);
    }
  }, [serverId, key, libraryVersion]); // eslint-disable-line react-hooks/exhaustive-deps
  const lookup = useCallback((id: string) => artistCovers.get(`${serverId}:${id}`)?.cover ?? undefined, [serverId]);
  return [lookup, tick];
}

/** The library's items, as the sidebar lists them before its view (kind, sort, filter) applies. */
export function useLibraryEntries(): LibraryEntry[] {
  const serverId = useApp((s) => s.accounts[0]?.id ?? "");
  const queues = useApp((s) => s.savedQueues);
  const nowId = useApp((s) => s.session?.current?.track.id);
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
  const [coverOf, tick] = useArtistCovers(serverId, artistIds);
  return useMemo(
    () => entries.map((e) => {
      const c = e.kind === "artist" && !e.coverArt ? coverOf(e.id) : undefined;
      return c ? { ...e, coverArt: c } : e;
    }),
    [entries, coverOf, tick], // eslint-disable-line react-hooks/exhaustive-deps
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
export function SidebarLibrary({ rail, shown, lib, scroller }: { rail: boolean; shown: LibraryEntry[]; lib: LibraryViewState; scroller: HTMLDivElement | null }) {
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
  const rowH = rail ? (narrow ? NARROW_RAIL_ROW_H : RAIL_ROW_H) : ROW_H;

  // Where the list starts in the scroller's content: the places, pins and this header sit above it.
  const [margin, setMargin] = useState(0);
  const measure = useCallback(() => {
    const el = scroller;
    const ul = list.current;
    if (!el || !ul) return;
    const m = Math.max(0, Math.round(ul.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop));
    setMargin((cur) => (cur === m ? cur : m));
  }, [scroller]);
  useLayoutEffect(measure);
  // The places, the pins and this section resizing (pins resolving, the filter opening) move the list.
  useEffect(() => {
    const el = scroller;
    if (!el) return;
    const ro = new ResizeObserver(measure);
    for (const child of Array.from(el.children)) ro.observe(child);
    return () => ro.disconnect();
  }, [scroller, measure]);
  // The header is stuck once the scroller has passed its place (the list's start less its height):
  // then the rows pass under a fade below it.
  const head = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(false);
  useEffect(() => {
    const el = scroller;
    if (!el || rail) return;
    const check = () => {
      const h = head.current;
      setStuck(!!h && el.scrollTop > margin - h.offsetHeight - 4);
    };
    check();
    el.addEventListener("scroll", check, { passive: true });
    return () => el.removeEventListener("scroll", check);
  }, [scroller, margin, rail]);

  // Keyboard: one tab stop, the active row (by key, so it survives a re-sort and scrolling), which is
  // always drawn; arrows, Home and End move it, scrolling it into view before it takes focus.
  const [activeKey, setActiveKey] = useState<string | undefined>(undefined);
  const found = activeKey ? shown.findIndex((e) => entryKey(e) === activeKey) : -1;
  const activeIndex = found >= 0 ? found : 0;
  const activeRef = useRef(activeIndex);
  activeRef.current = activeIndex;
  const rangeExtractor = useCallback((range: Range) => {
    const base = defaultRangeExtractor(range);
    const f = activeRef.current;
    return f < range.count && !base.includes(f) ? [...base, f].sort((a, b) => a - b) : base;
  }, []);
  const virt = useVirtualizer({ count: shown.length, getScrollElement: () => scroller, estimateSize: () => rowH, overscan: 8, scrollMargin: margin, rangeExtractor });
  useEffect(() => virt.measure(), [rowH, virt]);
  const pendingFocus = useRef<number | undefined>(undefined);
  useEffect(() => {
    const i = pendingFocus.current;
    if (i === undefined) return;
    const el = list.current?.querySelector<HTMLElement>(`li[data-index="${i}"] > a`);
    if (!el) return;
    pendingFocus.current = undefined;
    el.focus({ preventScroll: true });
  });
  const onListKey = (e: React.KeyboardEvent) => {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || !shown.length) return;
    const last = shown.length - 1;
    const to = e.key === "ArrowDown" ? Math.min(last, activeIndex + 1) : e.key === "ArrowUp" ? Math.max(0, activeIndex - 1) : e.key === "Home" ? 0 : e.key === "End" ? last : undefined;
    if (to === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    setActiveKey(entryKey(shown[to]!));
    pendingFocus.current = to;
    virt.scrollToIndex(to);
  };
  const onRowFocus = useCallback((key: string) => setActiveKey(key), []);

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
        <div ref={head} className={`side-lib-head ${stuck ? "stuck" : ""}`}>
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
        <ul ref={list} className="plain-list side-lib-list" aria-labelledby="side-lib-title" style={{ height: virt.getTotalSize(), position: "relative" }} onKeyDown={onListKey}>
          {virt.getVirtualItems().map((v) => {
            const e = shown[v.index]!;
            const key = entryKey(e);
            return (
              <LibraryRow key={key} entry={e} index={v.index} count={shown.length} tabStop={v.index === activeIndex} onFocusRow={onRowFocus} rail={rail} top={v.start - margin} height={rowH} active={key === routeKey} playing={key === playingKey} isPlaying={key === playingKey && isPlaying} />
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
const LibraryRow = memo(function LibraryRow({ entry: e, index, count, tabStop, onFocusRow, rail, top, height, active, playing, isPlaying }: { entry: LibraryEntry; index: number; count: number; tabStop: boolean; onFocusRow: (key: string) => void; rail: boolean; top: number; height: number; active: boolean; playing: boolean; isPlaying: boolean }) {
  const navigate = useApp((s) => s.navigate);
  const key = entryKey(e);
  const kind = t(`library.kind.${e.kind}` as never);
  const sub = e.owner ? `${kind} · ${e.owner}` : kind;
  const target = useMemo(() => targetOf(e), [e]);
  return (
    <li data-index={index} aria-setsize={count} aria-posinset={index + 1} style={{ position: "absolute", top: 0, left: 0, right: 0, height, transform: `translateY(${top}px)` }}>
      <a href="#" tabIndex={tabStop ? 0 : -1} onFocus={() => onFocusRow(key)} className={`nav-item lib-item ${active ? "active" : ""} ${playing ? "playing" : ""}`} aria-current={active ? "page" : undefined}
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
