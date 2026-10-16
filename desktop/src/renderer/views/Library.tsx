import { t } from "@shared/strings";
import type { Album } from "@core/api";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { executeAction } from "../store/actions";
import { bridge } from "../core/bridge";
import { openContextMenu } from "../components/ContextMenu";
import { Tile, TileList } from "../components/Tile";
import { Icon } from "../components/Icon";
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
  const playAlbum = (a: Album) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const tile = (view: ViewName, label: string, icon: string, cls: string, testId: string, param?: string) => (
    <button type="button" className={`lib-tile ${cls}`} onClick={() => navigate({ view, param })} data-testid={testId}>
      <span className="lib-tile-icon" aria-hidden="true"><Icon name={icon} size={22} /></span>
      <span className="lib-tile-label">{label}</span>
    </button>
  );
  return (
    <div className="view page-library" data-testid="view-library">
      <div className="view-header"><h1>{t("library.title")}</h1></div>
      <div className="view-body">
        <h2 className="section-heading">{t("library.browse")}</h2>
        <div className="lib-browse">
          {tile("albums", t("nav.albums"), "album", "wide", "library-albums")}
          {tile("artists", t("nav.artists"), "artist", "artists", "library-artists")}
          {tile("playlists", t("nav.playlists"), "playlist", "", "library-playlists")}
          {tile("songs", t("nav.songs"), "song", "", "library-songs")}
          {tile("genres", t("nav.genres"), "genre", "", "library-genres")}
        </div>
        <div className="lib-links">
          <button type="button" className="chip" onClick={() => navigate({ view: "filters" })} data-testid="library-filters"><Icon name="filter" size={14} /> {t("nav.filters")}</button>
          <button type="button" className="chip" onClick={() => void executeAction("navigateRecent")} data-testid="library-queues"><Icon name="restore" size={14} /> {t("library.recentQueues")}</button>
          <button type="button" className="chip" onClick={() => navigate({ view: "downloads", param: "offline" })} data-testid="library-offline"><Icon name="cached" size={14} /> {t("downloads.tab.offline")}</button>
          <button type="button" className="chip" onClick={() => navigate({ view: "downloads" })} data-testid="library-downloads"><Icon name="download" size={14} /> {t("nav.downloads")}</button>
          <button type="button" className="chip" onClick={() => navigate({ view: "stats" })} data-testid="library-stats"><Icon name="stats" size={14} /> {t("library.statistics")}</button>
        </div>
        <div className="lib-recent-head">
          <h2 className="section-heading">{t("library.recentlyAdded")}</h2>
          <span className="small muted">{t("library.newestFirst")}</span>
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
