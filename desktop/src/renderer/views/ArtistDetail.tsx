import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { TrackTable } from "../components/TrackTable";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { Heart } from "../components/Stars";
import { Tile, TileList } from "../components/Tile";
import { openContextMenu } from "../components/ContextMenu";

export function ArtistDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const artist = useQuery(() => ({ type: "artist", data: { id } }), "artistDetail", [id]);
  const albums = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: id, genre: undefined, sort: "year", descending: true, page: { offset: 0, limit: 500 } } }), "albums", [id, serverId]);
  const top = useQuery(() => ({ type: "artistTopSongs", data: { id, count: 10 } }), "trackList", [id]);
  const a = artist.data;
  const play = (shuffle: boolean) => a && bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "artist", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
  const topList = top.data ?? [];
  return (
    <div className="view" data-testid="view-artist">
      <div className="detail-head" onContextMenu={(e) => a && void openContextMenu(e, { type: "artists", data: { ids: [a.id] } })}>
        <Artwork id={a?.coverArt} size={300} className="art" round />
        <div className="meta">
          <div className="muted small">{t("nav.artists")}</div>
          <h1>{a?.name ?? ""}</h1>
          <div className="muted">{a ? t("artist.albumsCount", { albums: a.albumCount, songs: a.songCount }) : ""}</div>
          {a?.biography ? <div className="small muted" style={{ maxWidth: 600 }}>{a.biography}</div> : null}
          <div className="actions">
            <button type="button" className="btn primary" onClick={() => play(false)}><Icon name="play" size={14} style={{ fill: "currentColor" }} /> {t("album.play")}</button>
            <button type="button" className="btn" onClick={() => play(true)}><Icon name="shuffle" size={14} /> {t("album.shuffle")}</button>
            {a ? <Heart on={a.loved} onToggle={() => bridge().dispatch({ type: "setArtistLoved", data: { artist_id: a.id, loved: !a.loved } })} /> : null}
          </div>
        </div>
      </div>
      <div className="view-body">
        {topList.length ? (
          <>
            <h2 className="section-heading" style={{ margin: "4px 0 6px" }}>{t("artist.topSongs")}</h2>
            <div style={{ height: Math.min(topList.length, 10) * 30 + 30, display: "flex", flexDirection: "column" }}>
              <TrackTable tracks={topList} total={topList.length} columns={["art", "title", "album", "rating", "plays", "duration"]} scope={`artist-top:${id}`} label={t("artist.topSongs")} onPlay={(i) => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: topList.map((x) => x.id), start_index: i, label: `${a?.name ?? ""} · ${t("artist.topSongs")}`, shuffle: false } })} playingTrackId={playing} testId="artist-top" />
            </div>
          </>
        ) : null}
        <h2 className="section-heading" style={{ margin: "16px 0 8px" }}>{t("artist.albums")}</h2>
        <TileList label={t("artist.albums")} className="grid">
          {(albums.data?.items ?? []).map((al) => (
            <Tile key={al.id} title={al.name} subtitle={`${al.year ?? ""}${al.year ? " · " : ""}${t("misc.tracks", { count: al.songCount })}`} coverArt={al.coverArt} onOpen={() => navigate({ view: "album", id: al.id })} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [al.id] } })} testId="artist-album" />
          ))}
        </TileList>
      </div>
    </div>
  );
}
