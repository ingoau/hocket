import { useEffect, useMemo, useRef, useState } from "react";
import { t } from "@shared/strings";
import type { Album, Artist, Filter, SavedQueue, TrackSummary } from "@core/api";
import { useApp, useJsonSetting, type MenuItem } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { EmptyState } from "../components/EmptyState";
import { openContextMenu, openItemsMenu, openItemsMenuFromButton } from "../components/ContextMenu";
import { Tile, TileList } from "../components/Tile";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { BrandMark } from "../components/BrandMark";
import { filterName } from "../lib/filters";
import { showToast } from "../lib/ui-toast";
import { filterMenuItems, playFilter } from "../lib/pin-menus";
import { HOME_SECTIONS_KEY, moveBefore, moveKnown, moveShown, parseSections, resolvedStyle, sectionDef, sectionsValue, updateSection, type HomeSectionId, type SectionPref, type SectionStyle } from "../lib/home-sections";

/** How many items a section loads. */
const SECTION_ITEMS = 12;

/** "Loved or rated 4+, and not played in the last 90 days". */
const REDISCOVER = {
  type: "all",
  data: [
    { type: "any", data: [
      { type: "rule", data: { field: "loved", op: "isTrue", value: { type: "bool", data: true } } },
      { type: "rule", data: { field: "rating", op: "gt", value: { type: "number", data: 3 } } },
    ] },
    { type: "rule", data: { field: "lastPlayed", op: "notInTheLast", value: { type: "days", data: 90 } } },
  ],
} as const;

/** One thing a section shows, ready to draw in any style. */
interface Item {
  key: string;
  title: string;
  subtitle?: string;
  coverArt?: string;
  round?: boolean;
  /** Filters have no cover: drawn as an icon. */
  icon?: string;
  open: () => void;
  play?: () => void;
  menu: (e: React.MouseEvent) => void;
}

/**
 * Home: the sections the user chose (`home.sections`, synced), each a header naming its topic over
 * a row, grid or list of albums, artists, songs, filters or queues. Customise (top right, or a
 * section header's menu) opens a side panel to switch sections on and off, drag them into order and
 * choose how each is shown.
 */
export function Home() {
  const stored = useJsonSetting(HOME_SECTIONS_KEY);
  const prefs = useMemo(() => parseSections(stored), [stored]);
  const setSetting = useApp((s) => s.setSetting);
  const sync = useApp((s) => s.syncProgress);
  const [customising, setCustomising] = useState(false);
  const save = (next: SectionPref[]) => setSetting(HOME_SECTIONS_KEY, sectionsValue(next));
  const shown = prefs.filter((p) => p.on && sectionDef(p.id));
  const [empties, setEmpties] = useState<Record<string, boolean>>({});
  const reportEmpty = (id: string, empty: boolean) => setEmpties((m) => (m[id] === empty ? m : { ...m, [id]: empty }));
  const allEmpty = shown.length === 0 || shown.every((p) => empties[p.id] === true);
  const sectionMenu = (p: SectionPref, title: string): MenuItem[] => {
    const def = sectionDef(p.id);
    const i = shown.findIndex((s) => s.id === p.id);
    const items: MenuItem[] = [
      { id: "moveUp", label: t("home.moveUp"), icon: "arrowUp", enabled: i > 0, run: () => save(moveShown(prefs, p.id, -1)) },
      { id: "moveDown", label: t("home.moveDown"), icon: "arrowDown", enabled: i >= 0 && i < shown.length - 1, run: () => save(moveShown(prefs, p.id, 1)) },
    ];
    if (def && def.styles.length > 1) {
      items.push({ id: "showAs", label: t("home.showAs"), icon: "", enabled: false, heading: true, run: () => {} });
      for (const st of def.styles) items.push({ id: `style-${st}`, label: t(`home.style.${st}` as never), icon: styleIcon(st), enabled: true, checked: resolvedStyle(p) === st, run: () => save(updateSection(prefs, p.id, { style: st })) });
    }
    items.push({ id: "hide", label: t("home.hideSection"), icon: "remove", enabled: true, heading: false, run: () => {
      save(updateSection(prefs, p.id, { on: false }));
      // Undo shows just this section again, on whatever the list is by then.
      showToast(t("home.hidden", { title }), () => save(updateSection(parseSections(useApp.getState().setting<unknown>(HOME_SECTIONS_KEY, [])), p.id, { on: true })));
    } });
    items.push({ id: "customise", label: t("home.customiseHome"), icon: "tune", enabled: true, run: () => setCustomising(true) });
    return items;
  };
  return (
    <div className={`view page-home ${customising ? "customising" : ""}`} data-testid="view-home">
      <div className="home-layout">
        <div className="home-main">
          <div className="view-header">
            <h1 className="brand-lockup"><BrandMark /><span aria-hidden="true">hocket</span><span className="sr-only">{t("home.title")}</span></h1>
            {sync && !sync.finished ? <span className="badge">{t("sync.syncing", { phase: sync.phase })}</span> : null}
            <div className="actions">
              <button type="button" className={`btn ${customising ? "primary" : "tonal"}`} aria-pressed={customising} onClick={() => setCustomising((c) => !c)} data-testid="home-customise"><Icon name="tune" size={14} /> {t("home.customise")}</button>
            </div>
          </div>
          <div className="view-body">
            {shown.map((p) => (
              <Section key={p.id} pref={p} onEmpty={(e) => reportEmpty(p.id, e)} menu={(title) => sectionMenu(p, title)} />
            ))}
            {allEmpty ? <EmptyState message={t("home.emptySections")} icon="home" action={<button type="button" className="btn" onClick={() => setCustomising(true)}>{t("home.customiseHome")}</button>} /> : (
              <div className="home-footer"><button type="button" className="btn outlined" onClick={() => setCustomising(true)}><Icon name="tune" size={14} /> {t("home.customiseHome")}</button></div>
            )}
          </div>
        </div>
        {customising ? <CustomisePanel prefs={prefs} save={save} onClose={() => setCustomising(false)} /> : null}
      </div>
    </div>
  );
}

function styleIcon(st: SectionStyle): string {
  return st === "grid" ? "selectAll" : st === "list" ? "queue" : st === "tile" ? "stats" : "sidePanel";
}

/** One section: loads its items (only while shown) and draws them in its style. */
function Section({ pref, onEmpty, menu }: { pref: SectionPref; onEmpty: (empty: boolean) => void; menu: (title: string) => MenuItem[] }) {
  const id = pref.id as HomeSectionId;
  const style = resolvedStyle(pref);
  const data = useSectionData(id);
  const empty = data.ready && data.items.length === 0 && !data.listening;
  // Home shows its empty state when every section is empty.
  useEffect(() => onEmpty(empty), [empty]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!data.ready || empty) return null;
  const title = data.title ?? t(`home.section.${id}` as never);
  const headingId = `home-${id}`;
  const items = menu(title);
  return (
    <section className="home-section" aria-labelledby={headingId} data-testid={`home-section-${id}`}>
      <div className="home-section-head" onContextMenu={(e) => openItemsMenu(e, items)}>
        <div className="grow">
          <h2 className="section-heading" id={headingId}>{title}</h2>
        </div>
        <button type="button" className="btn icon section-more" aria-label={t("home.sectionOptions", { title })} onClick={(e) => openItemsMenuFromButton(e, items)} data-testid={`home-section-more-${id}`}><Icon name="more" size={18} /></button>
      </div>
      {data.listening ? <ListeningTile {...data.listening} /> : <SectionItems items={data.items} style={style} label={title} />}
    </section>
  );
}

function SectionItems({ items, style, label }: { items: Item[]; style: SectionStyle; label: string }) {
  if (style === "list") {
    return (
      <ul className="plain-list home-list" aria-label={label}>
        {items.slice(0, 8).map((it) => (
          <li key={it.key} className="home-list-row" onContextMenu={it.menu}>
            <button type="button" className="home-list-main" onClick={it.open} onDoubleClick={it.play}>
              {it.icon ? <span className="home-icon-art" aria-hidden="true"><Icon name={it.icon} size={20} /></span> : <Artwork id={it.coverArt} size={96} round={it.round} />}
              <span className="text"><span className="t1">{it.title}</span>{it.subtitle ? <span className="t2">{it.subtitle}</span> : null}</span>
            </button>
            <button type="button" className="btn icon" aria-label={`${t("misc.more")}: ${it.title}`} onClick={(e) => it.menu(e)}><Icon name="more" size={16} /></button>
          </li>
        ))}
      </ul>
    );
  }
  const tiles = (style === "grid" ? items.slice(0, 12) : items).map((it) =>
    it.icon ? (
      <li key={it.key} className="tile home-icon-tile" onContextMenu={it.menu}>
        <button type="button" className="tile-main" data-roving aria-label={it.subtitle ? `${it.title}, ${it.subtitle}` : it.title} onClick={it.open}>
          <span className="art home-icon-art" aria-hidden="true"><Icon name={it.icon} size={36} /></span>
          <span className="t1" title={it.title}>{it.title}</span>
          {it.subtitle ? <span className="t2">{it.subtitle}</span> : null}
        </button>
      </li>
    ) : (
      <Tile key={it.key} title={it.title} subtitle={it.subtitle} coverArt={it.coverArt} round={it.round} onOpen={it.open} onPlay={it.play} onContextMenu={it.menu} testId="home-item" />
    ),
  );
  return <TileList label={label} className={style === "grid" ? "grid" : "scroller"}>{tiles}</TileList>;
}

function ListeningTile({ totalMs, byWeekday, onOpen }: { totalMs: number; byWeekday: number[]; onOpen: () => void }) {
  const minutes = Math.floor(totalMs / 60000);
  const time = minutes >= 60 ? t("home.stats.hours", { h: Math.floor(minutes / 60), m: minutes % 60 }) : t("home.stats.minutes", { m: minutes });
  const max = Math.max(1, ...byWeekday);
  return (
    <button type="button" className="listen-tile" onClick={onOpen}>
      <span className="listen-text"><span className="listen-time">{time}</span><span className="muted">{t("home.stats.listened")}</span><span className="listen-link">{t("home.stats.open")}</span></span>
      <span className="listen-bars" aria-hidden="true">{byWeekday.map((n, i) => <span key={i} style={{ height: `${Math.max(6, (n / max) * 100)}%` }} />)}</span>
    </button>
  );
}

interface SectionData {
  ready: boolean;
  items: Item[];
  title?: string;
  listening?: { totalMs: number; byWeekday: number[]; onOpen: () => void };
}

/** A section's items from the core's existing queries (each section only runs its own). */
function useSectionData(id: HomeSectionId): SectionData {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const filters = useApp((s) => s.filters);
  const saved = useApp((s) => s.savedQueues);
  const nowId = useApp((s) => s.session?.current?.trackId);
  const needsStats = id === "favouriteArtists" || id === "topGenre" || id === "moreFromArtist" || id === "onRepeat";
  const month = useQuery(() => (needsStats ? { type: "stats", data: { period_days: 30 } } : null), "stats", [needsStats, nowId]);
  // A quiet month falls back to the year, so favourites are still there.
  const year = useQuery(() => (needsStats && month.data && month.data.totalPlays === 0 ? { type: "stats", data: { period_days: 365 } } : null), "stats", [needsStats, month.data?.totalPlays]);
  const stats = month.data && month.data.totalPlays === 0 && year.data ? year.data : month.data;
  const genre = useMemo(() => {
    const counts = new Map<string, number>();
    for (const a of stats?.topAlbums ?? []) if (a.genre?.trim()) counts.set(a.genre, (counts.get(a.genre) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  }, [stats]);
  const topArtist = stats?.topArtists[0];
  const rediscover = useQuery(() => (id === "rediscover" ? { type: "tracks", data: { server_id: serverId, page: { offset: 0, limit: SECTION_ITEMS }, sort: "rating", descending: true, filter: REDISCOVER as never } } : null), "tracks", [id, serverId]);
  const genreAlbums = useQuery(() => (id === "topGenre" && genre ? { type: "albums", data: { server_id: serverId, artist_id: undefined, genre, sort: "playCount", descending: true, page: { offset: 0, limit: SECTION_ITEMS } } } : null), "albums", [id, serverId, genre]);
  const newest = useQuery(() => (id === "newInLibrary" ? { type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "dateAdded", descending: true, page: { offset: 0, limit: SECTION_ITEMS } } } : null), "albums", [id, serverId]);
  const artistSongs = useQuery(() => (id === "moreFromArtist" && topArtist ? { type: "artistTopSongs", data: { id: topArtist.id, count: SECTION_ITEMS } } : null), "trackList", [id, topArtist?.id]);
  const history = useQuery(() => (id === "recentlyPlayed" ? { type: "recentlyPlayed", data: { limit: 100 } } : null), "history", [id, nowId]);
  const recentIds = useMemo(() => [...new Set((history.data ?? []).map((h) => h.track.albumId).filter((x): x is string => !!x))].slice(0, SECTION_ITEMS), [history.data]);
  const recentAlbums = useAlbumsByIds(id === "recentlyPlayed" ? recentIds : []);
  const week = useQuery(() => (id === "stats" ? { type: "stats", data: { period_days: 7 } } : null), "stats", [id, nowId]);

  const albumItem = (a: Album): Item => ({
    key: `al:${a.id}`, title: a.name, subtitle: a.artist ?? t("misc.unknownArtist"), coverArt: a.coverArt,
    open: () => navigate({ view: "album", id: a.id }),
    play: () => playContext(serverId, { type: "album", data: { id: a.id } }, a.name),
    menu: (e) => void openContextMenu(e, { type: "albums", data: { ids: [a.id] } }),
  });
  const artistItem = (a: Artist): Item => ({
    key: `ar:${a.id}`, title: a.name, subtitle: t("artist.albums", { n: a.albumCount }), coverArt: a.coverArt, round: true,
    open: () => navigate({ view: "artist", id: a.id }),
    play: () => playContext(serverId, { type: "artist", data: { id: a.id } }, a.name),
    menu: (e) => void openContextMenu(e, { type: "artists", data: { ids: [a.id] } }),
  });
  const trackItems = (tracks: TrackSummary[], label: string): Item[] => tracks.map((tr, i) => ({
    key: `t:${i}:${tr.id}`, title: tr.title, subtitle: tr.artist ?? t("misc.unknownArtist"), coverArt: tr.coverArt,
    open: () => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: tracks.map((x) => x.id), start_index: i, label, shuffle: false } }),
    menu: (e) => void openContextMenu(e, { type: "tracks", data: { ids: [tr.id] } }),
  }));
  const filterItem = (f: Filter): Item => ({
    key: `f:${f.id}`, title: filterName(f), subtitle: t("library.kind.filter"), icon: "filter",
    open: () => playFilter(serverId, f),
    play: () => playFilter(serverId, f),
    menu: (e) => openItemsMenu(e, filterMenuItems(f, serverId)),
  });
  const queueItem = (q: SavedQueue): Item => ({
    key: `q:${q.id}`, title: q.label, subtitle: t("savedQueues.tracks", { n: q.trackCount }), coverArt: q.coverArt ?? undefined,
    open: () => bridge().dispatch({ type: "restoreSavedQueue", data: { id: q.id } }),
    menu: (e) => void openContextMenu(e, { type: "savedQueue", data: { id: q.id } }),
  });

  switch (id) {
    case "rediscover":
      return { ready: !!rediscover.data, items: trackItems((rediscover.data?.items ?? []).map(toSummary), t("home.section.rediscover")) };
    case "filters":
      return { ready: true, items: filters.slice(0, SECTION_ITEMS).map(filterItem) };
    case "favouriteArtists":
      return { ready: !!stats, items: (stats?.topArtists ?? []).slice(0, SECTION_ITEMS).map(artistItem) };
    case "topGenre":
      return { ready: !!stats && (!genre || !!genreAlbums.data), items: (genreAlbums.data?.items ?? []).map(albumItem), title: genre ? t("home.section.topGenre.named", { genre }) : undefined };
    case "newInLibrary":
      return { ready: !!newest.data, items: (newest.data?.items ?? []).map(albumItem) };
    case "moreFromArtist":
      return { ready: !!stats && (!topArtist || !!artistSongs.data), items: trackItems((artistSongs.data ?? []).map(toSummary), topArtist?.name ?? ""), title: topArtist ? t("home.section.moreFromArtist.named", { name: topArtist.name }) : undefined };
    case "onRepeat":
      return { ready: !!stats, items: (stats?.topAlbums ?? []).slice(0, SECTION_ITEMS).map(albumItem) };
    case "recentlyPlayed":
      return { ready: !!history.data && recentAlbums.ready, items: recentAlbums.albums.map(albumItem) };
    case "recentQueues":
      return { ready: true, items: saved.slice(0, SECTION_ITEMS).map(queueItem) };
    case "stats":
      return { ready: !!week.data, items: [], listening: week.data && week.data.totalPlays > 0 ? { totalMs: week.data.totalMs, byWeekday: week.data.playsByWeekday, onOpen: () => navigate({ view: "stats" }) } : undefined };
  }
}

/** Albums by id, in that order (recently played reads them one by one). */
function useAlbumsByIds(ids: string[]): { ready: boolean; albums: Album[] } {
  const key = ids.join(",");
  const [state, setState] = useState<{ key: string; albums: Album[] } | undefined>(undefined);
  useEffect(() => {
    if (!ids.length) return;
    let live = true;
    void Promise.all(ids.map((id) => bridge().query({ type: "album", data: { id } }).then((r) => (r.type === "albumDetail" ? r.data : undefined)).catch(() => undefined)))
      .then((list) => { if (live) setState({ key, albums: list.filter((a): a is Album => !!a) }); });
    return () => { live = false; };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!ids.length) return { ready: true, albums: [] };
  return state?.key === key ? { ready: true, albums: state.albums } : { ready: false, albums: [] };
}

function toSummary(tr: { id: string; serverId: string; title: string; artist?: string; artistId?: string; album?: string; albumId?: string; coverArt?: string; durationMs: number } & Partial<TrackSummary>): TrackSummary {
  return tr as TrackSummary;
}

function playContext(serverId: string, kind: { type: "album" | "artist" | "playlist"; data: { id: string } }, label: string, shuffle = false): void {
  bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: kind as never, label, sort: "default", tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
}

/** The Customise Home panel: every section in order, a switch, drag (or the arrows) to move, how it's shown. */
function CustomisePanel({ prefs, save, onClose }: { prefs: SectionPref[]; save: (p: SectionPref[]) => void; onClose: () => void }) {
  const known = prefs.filter((p) => sectionDef(p.id));
  const [dragId, setDragId] = useState<string | undefined>(undefined);
  const [over, setOver] = useState<{ id: string; after: boolean } | undefined>(undefined);
  const heading = useRef<HTMLHeadingElement>(null);
  // Opening moves focus into the panel; closing (Escape too) returns it to Customise.
  useEffect(() => heading.current?.focus(), []);
  const close = () => {
    onClose();
    document.querySelector<HTMLElement>('[data-testid="home-customise"]')?.focus();
  };
  const endDrag = () => { setDragId(undefined); setOver(undefined); };
  return (
    <aside className="customise-panel" aria-label={t("home.customiseHome")} data-testid="home-customise-panel" onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); close(); } }}>
      <div className="customise-head">
        <h2 className="card-title grow" tabIndex={-1} ref={heading}>{t("home.customiseHome")}</h2>
        <button type="button" className="btn icon" aria-label={t("home.customiseClose")} onClick={close}><Icon name="close" size={16} /></button>
      </div>
      <p className="small muted customise-note"><Icon name="sync" size={14} /> {t("home.customiseSync")}</p>
      <ul className="plain-list customise-list">
        {known.map((p, i) => {
          const def = sectionDef(p.id);
          if (!def) return null;
          const title = t(`home.section.${def.id}` as never);
          const style = resolvedStyle(p);
          const target = over?.id === p.id && dragId && dragId !== p.id ? (over.after ? "drop-after" : "drop-target") : "";
          return (
            <li
              key={p.id}
              className={`customise-row ${p.on ? "on" : ""} ${dragId === p.id ? "dragging" : ""} ${target}`}
              draggable
              onDragStart={(e) => { e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", p.id); setDragId(p.id); }}
              onDragOver={(e) => {
                e.preventDefault();
                // The lower half of a row drops after it, so a section can go one down or last.
                const r = e.currentTarget.getBoundingClientRect();
                const after = e.clientY > r.top + r.height / 2;
                if (over?.id !== p.id || over.after !== after) setOver({ id: p.id, after });
              }}
              onDrop={(e) => { e.preventDefault(); if (dragId) save(moveBefore(prefs, dragId, p.id, over?.id === p.id && over.after)); endDrag(); }}
              onDragEnd={endDrag}
              data-testid={`customise-${p.id}`}
            >
              <div className="customise-line">
                <span className="grip" aria-hidden="true"><Icon name="dragHandle" size={16} /></span>
                <span className="grow customise-text"><span className="t1">{title}</span><span className="t2">{t(`home.section.${def.id}.desc` as never)}</span></span>
                <button type="button" className="btn icon sm" aria-label={t("home.moveUpNamed", { title })} aria-disabled={i === 0} onClick={() => { if (i > 0) save(moveKnown(prefs, p.id, -1)); }}><Icon name="chevronUp" size={16} /></button>
                <button type="button" className="btn icon sm" aria-label={t("home.moveDownNamed", { title })} aria-disabled={i === known.length - 1} onClick={() => { if (i < known.length - 1) save(moveKnown(prefs, p.id, 1)); }}><Icon name="chevronDown" size={16} /></button>
                <button type="button" role="switch" aria-checked={p.on} aria-label={t("home.showNamed", { title })} className={`m3-switch ${p.on ? "on" : ""}`} onClick={() => save(updateSection(prefs, p.id, { on: !p.on }))} data-testid={`customise-switch-${p.id}`}><span className="m3-switch-track" /><span className="m3-switch-handle" /></button>
              </div>
              {p.on && def.styles.length > 1 ? (
                <div className="customise-styles" role="group" aria-label={t("home.showAs")}>
                  {def.styles.map((st) => (
                    <button key={st} type="button" className={`chip ${style === st ? "selected" : ""}`} aria-pressed={style === st} onClick={() => save(updateSection(prefs, p.id, { style: st }))}>{t(`home.style.${st}` as never)}</button>
                  ))}
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
      <button type="button" className="btn text" onClick={() => save(parseSections([]))}>{t("home.customiseReset")}</button>
    </aside>
  );
}
