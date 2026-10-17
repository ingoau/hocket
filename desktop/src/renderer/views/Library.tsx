import { t } from "@shared/strings";
import type { Album } from "@core/api";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { bridge } from "../core/bridge";
import { openContextMenu } from "../components/ContextMenu";
import { Tile, TileList } from "../components/Tile";
import { Icon } from "../components/Icon";
import { Artwork } from "../components/Artwork";
import type { ViewName } from "../store/app";

/** How many Recently added albums the Library lists before Albums takes over. */
const RECENT_ALBUMS = 60;

/**
 * The Library: the places to browse (each list, then queues, offline, filters, downloads and
 * statistics), then Recently added as a grid. What's pinned lives in the sidebar, under the Library.
 */
export function Library() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const recent = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: undefined, genre: undefined, sort: "dateAdded", descending: true, page: { offset: 0, limit: RECENT_ALBUMS } } }), "albums", [serverId]);
  // The Albums tile shows the covers of the three most played albums (this month, else this year).
  const month = useQuery(() => ({ type: "stats", data: { period_days: 30 } }), "stats", []);
  const year = useQuery(() => (month.data && !month.data.topAlbums.length ? { type: "stats", data: { period_days: 365 } } : null), "stats", [month.data?.topAlbums.length]);
  const topCovers = ((month.data?.topAlbums.length ? month.data : year.data)?.topAlbums ?? []).map((a) => a.coverArt).filter((c): c is string => !!c).slice(0, 3);
  const playAlbum = (a: Album) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const tile = (view: ViewName, label: string, icon: string, cls: string, testId: string, param?: string, covers: string[] = []) => (
    <button type="button" className={`lib-tile ${cls}`} onClick={() => navigate({ view, param })} data-testid={testId}>
      {covers.length ? (
        <span className="lib-fan" aria-hidden="true">
          {covers.map((c, i) => <Artwork key={c} id={c} size={128} className={`lib-fan-cover n${i}`} />).reverse()}
        </span>
      ) : null}
      <span className="lib-tile-icon" aria-hidden="true"><Icon name={icon} size={22} /></span>
      <span className="lib-tile-label">{label}</span>
    </button>
  );
  // The links: one connected group of tonal tiles, each icon in its own shape.
  const link = (icon: string, label: string, onClick: () => void, testId: string, shape: string) => (
    <button type="button" className="lib-link" onClick={onClick} data-testid={testId}>
      <span className={`lib-link-icon ${shape}`} aria-hidden="true"><Icon name={icon} size={18} /></span>
      <span className="truncate">{label}</span>
    </button>
  );
  return (
    <div className="view page-library" data-testid="view-library">
      <div className="view-header"><h1 className="brand-type">{t("library.title")}</h1></div>
      <div className="view-body">
        <div className="lib-browse">
          {tile("albums", t("nav.albums"), "album", "wide", "library-albums", undefined, topCovers)}
          {tile("artists", t("nav.artists"), "artist", "artists", "library-artists")}
          {tile("playlists", t("nav.playlists"), "playlist", "", "library-playlists")}
          {tile("songs", t("nav.songs"), "song", "", "library-songs")}
          {tile("genres", t("nav.genres"), "genre", "", "library-genres")}
        </div>
        <nav className="lib-links" aria-label={t("library.more")}>
          {link("filter", t("nav.filters"), () => navigate({ view: "filters" }), "library-filters", "sunny")}
          {link("restore", t("library.recentQueues"), () => void executeAction("navigateRecent"), "library-queues", "cookie")}
          {link("cached", t("downloads.tab.offline"), () => navigate({ view: "downloads", param: "offline" }), "library-offline", "pill")}
          {link("download", t("nav.downloads"), () => navigate({ view: "downloads" }), "library-downloads", "circle")}
          {link("stats", t("library.statistics"), () => navigate({ view: "stats" }), "library-stats", "squircle")}
        </nav>
        <div className="lib-recent-head">
          <h2 className="section-heading">{t("library.recentlyAdded")}</h2>
        </div>
        <TileList label={t("library.recentlyAdded")} className="grid" testId="library-recent">
          {(recent.data?.items ?? []).map((a) => (
            <Tile key={a.id} title={a.name} subtitle={a.artist} coverArt={a.coverArt} onOpen={() => navigate({ view: "album", id: a.id })} onPlay={() => playAlbum(a)} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })} testId="library-album" />
          ))}
        </TileList>
      </div>
    </div>
  );
}
