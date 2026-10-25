// An artist: the header (its picture, never stretched — see DetailHead), the
// top songs, the discography as one shelf per release kind (albums, EPs,
// singles, compilations; "View more" opens the kind's full grid, newest or
// oldest first), then About from the server's artist info (Last.fm).
import { useEffect, useState } from "react";
import type { Album } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { TrackTable } from "../components/TrackTable";
import { Icon } from "../components/Icon";
import { Heart } from "../components/Stars";
import { Tile, TileList } from "../components/Tile";
import { openContextMenu, openMenuFromButton } from "../components/ContextMenu";
import { DetailHead } from "../components/DetailHead";
import { TOP_SONGS_COUNT, topSongsSection } from "../lib/top-songs";
import { byReleaseKind, type ReleaseKind } from "../lib/release-kind";

/** Top songs shown before "View more". */
const TOP_PREVIEW = 5;
/** Releases on a shelf before "View more". */
const SHELF = 8;

export function releaseCounts(groups: { kind: ReleaseKind; albums: unknown[] }[]): string {
  return groups.map((g) => t(g.albums.length === 1 ? `release.count.${g.kind}.one` : `release.count.${g.kind}.other`, { count: g.albums.length })).join(" · ");
}

export function ArtistDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const artist = useQuery(() => ({ type: "artist", data: { id } }), "artistDetail", [id]);
  const albums = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: id, genre: undefined, sort: "year", descending: true, page: { offset: 0, limit: 500 } } }), "albums", [id, serverId]);
  const top = useQuery(() => ({ type: "artistTopSongs", data: { id, count: TOP_SONGS_COUNT } }), "trackList", [id]);
  const info = useQuery(() => ({ type: "artistInfo", data: { id } }), "externalInfo", [id], { static: true });
  const [allTop, setAllTop] = useState(false);
  const [grid, setGrid] = useState<ReleaseKind | null>(null);
  const [oldest, setOldest] = useState(false);
  useEffect(() => { setGrid(null); setAllTop(false); }, [id]);
  const a = artist.data;
  const play = (shuffle: boolean) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "artist", data: { id } }, label: a?.name ?? "", sort: "default", tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
  const topAll = top.data ?? [];
  const topList = allTop ? topAll : topAll.slice(0, TOP_PREVIEW);
  const topSection = topSongsSection(top.data ? topList.length : undefined, top.error !== undefined);
  const groups = byReleaseKind(albums.data?.items ?? []);
  const about = a?.biography || info.data?.text;
  const tile = (al: Album) => (
    <Tile key={al.id} title={al.name} subtitle={`${al.year ?? ""}${al.year ? " · " : ""}${t("misc.tracks", { count: al.songCount })}`} coverArt={al.coverArt} onOpen={() => navigate({ view: "album", id: al.id })} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [al.id] } })} dragTarget={{ type: "albums", data: { ids: [al.id] } }} testId="artist-album" />
  );
  const open = groups.find((g) => g.kind === grid);
  return (
    <div className="view" data-testid="view-artist">
      <DetailHead coverArt={a?.coverArt} round icon="artist" onContextMenu={(e) => a && void openContextMenu(e, { type: "artists", data: { ids: [a.id] } })}>
        <div className="muted small">{t("nav.artists")}</div>
        <h1>{a?.name ?? ""}</h1>
        <div className="muted" data-testid="artist-counts">{groups.length ? releaseCounts(groups) : a ? t("artist.albumsCount", { albums: a.albumCount, songs: a.songCount }) : ""}</div>
        <div className="actions">
          <button type="button" className="btn primary fab" onClick={() => play(false)}><Icon name="play" size={24} filled /> {t("album.play")}</button>
          <button type="button" className="btn tonal" onClick={() => play(true)}><Icon name="shuffle" size={20} /> {t("album.shuffle")}</button>
          {a ? <Heart on={a.loved} onToggle={() => bridge().dispatch({ type: "setArtistLoved", data: { artist_id: a.id, loved: !a.loved } })} /> : null}
          {a ? <button type="button" className="btn icon" aria-haspopup="menu" aria-label={t("misc.more")} title={t("misc.more")} onClick={(e) => openMenuFromButton(e, { type: "artists", data: { ids: [a.id] } })} data-testid="artist-more"><Icon name="more" size={22} /></button> : null}
        </div>
      </DetailHead>
      <div className="view-body">
        {open ? (
          <section className="home-section" aria-labelledby="artist-grid-title" data-testid="artist-grid">
            <div className="shelf-head">
              <button type="button" className="btn icon" aria-label={t("nowPlaying.back")} title={t("nowPlaying.back")} onClick={() => setGrid(null)} data-testid="artist-grid-back"><Icon name="back" size={22} /></button>
              <h2 className="section-heading" id="artist-grid-title">{t(`release.kind.${open.kind}`)}</h2>
              <span className="muted">{releaseCounts([open])}</span>
              <span className="spacer" />
              <div className="seg-group" role="group" aria-label={t("release.order")}>
                <button type="button" className={`seg ${!oldest ? "on" : ""}`} aria-pressed={!oldest} onClick={() => setOldest(false)} data-testid="artist-grid-newest">{t("release.newest")}</button>
                <button type="button" className={`seg ${oldest ? "on" : ""}`} aria-pressed={oldest} onClick={() => setOldest(true)} data-testid="artist-grid-oldest">{t("release.oldest")}</button>
              </div>
            </div>
            <TileList label={t(`release.kind.${open.kind}`)} className="grid">{(oldest ? [...open.albums].reverse() : open.albums).map(tile)}</TileList>
          </section>
        ) : (
          <>
            {topSection.kind === "loading" ? (
              <>
                <h2 className="section-heading">{t("artist.topSongs")}</h2>
                <div style={{ height: topSection.height }} aria-busy="true" data-testid="artist-top-loading" />
              </>
            ) : topSection.kind === "list" ? (
              <>
                <div className="shelf-head">
                  <h2 className="section-heading">{t("artist.topSongs")}</h2>
                  <span className="spacer" />
                  {topAll.length > TOP_PREVIEW ? <button type="button" className="btn text sm" aria-expanded={allTop} onClick={() => setAllTop(!allTop)} data-testid="artist-top-more">{allTop ? t("release.viewLess") : t("release.viewMore")}</button> : null}
                </div>
                <div style={{ height: topSection.height, display: "flex", flexDirection: "column" }}>
                  <TrackTable tracks={topList} total={topList.length} columns={["art", "title", "album", "rating", "plays", "duration"]} scope={`artist-top:${id}`} label={t("artist.topSongs")} onPlay={(i) => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: topAll.map((x) => x.id), start_index: i, label: `${a?.name ?? ""} · ${t("artist.topSongs")}`, shuffle: false } })} playingTrackId={playing} testId="artist-top" />
                </div>
              </>
            ) : null}
            {groups.map((g) => (
              <section key={g.kind} className="home-section artist-shelf" aria-labelledby={`artist-shelf-${g.kind}`} data-testid={`artist-shelf-${g.kind}`}>
                <div className="shelf-head">
                  <h2 className="section-heading" id={`artist-shelf-${g.kind}`}>{t(`release.kind.${g.kind}`)}</h2>
                  <span className="spacer" />
                  {g.albums.length > SHELF ? <button type="button" className="btn text sm" onClick={() => { setGrid(g.kind); setOldest(false); }} data-testid={`artist-shelf-more-${g.kind}`}>{t("release.viewMore")}</button> : null}
                </div>
                <TileList label={t(`release.kind.${g.kind}`)} className="scroller">{g.albums.slice(0, SHELF).map(tile)}</TileList>
              </section>
            ))}
            {about ? (
              <section className="artist-about" aria-labelledby="artist-about" data-testid="artist-about">
                <h2 className="section-heading" id="artist-about">{t("artist.about")}</h2>
                <p>{about}</p>
                {info.data?.lastFmUrl ? <a className="plain-link small" href={info.data.lastFmUrl} target="_blank" rel="noreferrer">{t("artist.readOnLastFm")}</a> : null}
              </section>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
