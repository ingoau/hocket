// An artist: the header (its picture, never stretched — see DetailHead), the
// top songs, the discography as one shelf per release kind (albums, EPs,
// singles, compilations; "View more" opens the kind's full grid, newest or
// oldest first), "Featured on" (others' albums they're credited on a track of:
// a guest with no albums of their own still has a page), then About from the
// server's artist info (Last.fm).
import { useEffect, useRef, useState } from "react";
import type { Album } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { Artwork } from "../components/Artwork";
import { fmtTime } from "../lib/format";
import { Icon } from "../components/Icon";
import { Heart } from "../components/Stars";
import { Tile, TileList } from "../components/Tile";
import { openContextMenu, openMenuFromButton } from "../components/ContextMenu";
import { DetailHead } from "../components/DetailHead";
import { TOP_SONGS_COUNT, TOP_SONGS_PREVIEW as TOP_PREVIEW, topSongsSection } from "../lib/top-songs";
import { byReleaseKind, type ReleaseKind } from "../lib/release-kind";

/** Releases on a shelf before "View more". */
const SHELF = 8;

/** A shelf on the page: a release kind, or the albums they're featured on. */
type Shelf = ReleaseKind | "featured";

export function releaseCounts(groups: { kind: ReleaseKind; albums: unknown[] }[]): string {
  return groups.map((g) => t(g.albums.length === 1 ? `release.count.${g.kind}.one` : `release.count.${g.kind}.other`, { count: g.albums.length })).join(" · ");
}

export function ArtistDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const artist = useQuery(() => ({ type: "artist", data: { id } }), "artistDetail", [id]);
  const albums = useQuery(() => ({ type: "albums", data: { server_id: serverId, artist_id: id, genre: undefined, sort: "year", descending: true, page: { offset: 0, limit: 500 } } }), "albums", [id, serverId]);
  const featured = useQuery(() => ({ type: "artistAppearsOn", data: { id } }), "albums", [id]);
  const top = useQuery(() => ({ type: "artistTopSongs", data: { id, count: TOP_SONGS_COUNT } }), "trackList", [id]);
  const info = useQuery(() => ({ type: "artistInfo", data: { id } }), "externalInfo", [id], { static: true });
  const [allTop, setAllTop] = useState(false);
  const [grid, setGrid] = useState<Shelf | null>(null);
  const [oldest, setOldest] = useState(false);
  useEffect(() => { setGrid(null); setAllTop(false); }, [id]);
  // Focus follows the swap: onto the grid's back arrow going in, back onto that shelf's View more coming out.
  const lastGrid = useRef<Shelf | null>(null);
  useEffect(() => {
    const target = grid ? '[data-testid="artist-grid-back"]' : lastGrid.current ? `[data-testid="artist-shelf-more-${lastGrid.current}"]` : null;
    if (grid) lastGrid.current = grid;
    if (target && document.activeElement === document.body) document.querySelector<HTMLElement>(target)?.focus({ preventScroll: !grid });
  }, [grid]);
  const a = artist.data;
  const play = (shuffle: boolean) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "artist", data: { id } }, label: a?.name ?? "", sort: "default", tracks: [] }, startIndex: 0, shuffle, saveOutgoing: true } } });
  const topAll = top.data ?? [];
  const topList = allTop ? topAll : topAll.slice(0, TOP_PREVIEW);
  const topSection = topSongsSection(top.data ? topList.length : undefined, top.error !== undefined);
  const playTop = (i: number) => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: topAll.map((x) => x.id), start_index: i, label: `${a?.name ?? ""} · ${t("artist.topSongs")}`, shuffle: false } });
  const groups = byReleaseKind(albums.data?.items ?? []);
  const featuredOn = featured.data?.items ?? [];
  const shelves: { kind: Shelf; title: string; count: string; albums: Album[] }[] = [
    ...groups.map((g) => ({ kind: g.kind, title: t(`release.kind.${g.kind}`), count: releaseCounts([g]), albums: g.albums })),
    ...(featuredOn.length ? [{ kind: "featured" as const, title: t("artist.featuredOn"), count: releaseCounts([{ kind: "album", albums: featuredOn }]), albums: featuredOn }] : []),
  ];
  const about = a?.biography || info.data?.text;
  // Featured on names whose album it is; their own shelves need not.
  const tile = (al: Album) => (
    <Tile key={al.id} title={al.name} subtitle={al.artistId !== id && al.artist ? [al.artist, al.year].filter(Boolean).join(" · ") : `${al.year ?? ""}${al.year ? " · " : ""}${t("misc.tracks", { count: al.songCount })}`} coverArt={al.coverArt} onOpen={() => navigate({ view: "album", id: al.id })} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [al.id] } })} dragTarget={{ type: "albums", data: { ids: [al.id] } }} testId="artist-album" />
  );
  const open = shelves.find((g) => g.kind === grid);
  return (
    <div className="view detail-page" data-testid="view-artist">
      <DetailHead coverArt={a?.coverArt} round icon="artist" onContextMenu={(e) => a && void openContextMenu(e, { type: "artists", data: { ids: [a.id] } })} actions={
        <>
          <button type="button" className="btn primary fab" onClick={() => play(false)} data-testid="artist-play"><Icon name="play" size={24} filled /> {t("album.play")}</button>
          <button type="button" className="btn icon" aria-label={t("album.shuffle")} title={t("album.shuffle")} onClick={() => play(true)} data-testid="artist-shuffle"><Icon name="shuffle" size={24} /></button>
          {a ? <Heart size={24} on={a.loved} onToggle={() => bridge().dispatch({ type: "setArtistLoved", data: { artist_id: a.id, loved: !a.loved } })} /> : null}
          {a ? <button type="button" className="btn icon" aria-haspopup="menu" aria-label={t("misc.more")} title={t("misc.more")} onClick={(e) => openMenuFromButton(e, { type: "artists", data: { ids: [a.id] } })} data-testid="artist-more"><Icon name="more" size={24} /></button> : null}
        </>
      }>
        <h1>{a?.name ?? ""}</h1>
        <div className="detail-sub" data-testid="artist-counts"><span>{groups.length ? releaseCounts(groups) : a ? t("artist.albumsCount", { albums: a.albumCount, songs: a.songCount }) : ""}</span></div>
      </DetailHead>
      <div className="view-body">
        {open ? (
          <section className="detail-section home-section" aria-labelledby="artist-grid-title" data-testid="artist-grid">
            <div className="detail-section-head shelf-head">
              <button type="button" className="btn icon" aria-label={t("nowPlaying.back")} title={t("nowPlaying.back")} onClick={() => setGrid(null)} data-testid="artist-grid-back"><Icon name="back" size={22} /></button>
              <h2 id="artist-grid-title">{open.title}</h2>
              <span className="muted">{open.count}</span>
              <span className="spacer" />
              <div className="seg-group" role="group" aria-label={t("release.order")}>
                <button type="button" className={`seg ${!oldest ? "on" : ""}`} aria-pressed={!oldest} onClick={() => setOldest(false)} data-testid="artist-grid-newest">{t("release.newest")}</button>
                <button type="button" className={`seg ${oldest ? "on" : ""}`} aria-pressed={oldest} onClick={() => setOldest(true)} data-testid="artist-grid-oldest">{t("release.oldest")}</button>
              </div>
            </div>
            <TileList label={open.title} className="grid">{(oldest ? [...open.albums].reverse() : open.albums).map(tile)}</TileList>
          </section>
        ) : (
          <>
            {topSection.kind !== "none" ? (
              <section className="detail-section" aria-labelledby="artist-top-title">
                <div className="detail-section-head">
                  <h2 id="artist-top-title">{t("artist.topSongs")}</h2>
                  {topAll.length > TOP_PREVIEW ? <button type="button" className="btn view-more" aria-expanded={allTop} onClick={() => setAllTop(!allTop)} data-testid="artist-top-more">{allTop ? t("release.viewLess") : t("release.viewMore")}<Icon name={allTop ? "chevronUp" : "chevronRight"} size={18} /></button> : null}
                </div>
                {topSection.kind === "loading" ? (
                  <ol className="plain-list top-songs" aria-busy="true" aria-labelledby="artist-top-title" data-testid="artist-top-loading">
                    {Array.from({ length: topSection.rows }, (_, i) => <li key={i} className="top-song top-song-placeholder" aria-hidden="true" />)}
                  </ol>
                ) : (
                  <ol className="plain-list top-songs" aria-labelledby="artist-top-title" data-testid="artist-top">
                    {topList.map((tr, i) => (
                      <li key={tr.id} className={`top-song ${tr.id === playing ? "playing" : ""}`} onContextMenu={(e) => void openContextMenu(e, { type: "tracks", data: { ids: [tr.id] } })}>
                        <button type="button" className="top-song-main" onClick={() => playTop(i)} aria-label={tr.id === playing ? `${t("artist.playTop", { title: tr.title, n: i + 1 })}, ${t("a11y.playingTrack")}` : t("artist.playTop", { title: tr.title, n: i + 1 })} aria-current={tr.id === playing || undefined} data-testid="artist-top-row">
                          <span className="top-song-art"><Artwork id={tr.coverArt} size={96} className="art" /><span className="top-song-n" aria-hidden="true">{tr.id === playing ? <Icon name="equalizer" size={18} /> : i + 1}</span></span>
                          <span className="grow" style={{ minWidth: 0 }}>
                            <span className="truncate top-song-title">{tr.title}</span>
                            <span className="truncate top-song-sub">{[tr.album, fmtTime(tr.durationMs)].filter(Boolean).join(" · ")}</span>
                          </span>
                        </button>
                        <Heart on={tr.loved} size={20} onToggle={() => bridge().dispatch({ type: "setLoved", data: { targets: [{ type: "track", data: { id: tr.id } }], loved: !tr.loved } })} />
                      </li>
                    ))}
                  </ol>
                )}
              </section>
            ) : null}
            {shelves.map((g) => (
              <section key={g.kind} className="detail-section home-section artist-shelf" aria-labelledby={`artist-shelf-${g.kind}`} data-testid={`artist-shelf-${g.kind}`}>
                <div className="detail-section-head">
                  <h2 id={`artist-shelf-${g.kind}`}>{g.title}</h2>
                  {g.albums.length > SHELF ? <button type="button" className="btn view-more" onClick={() => { setGrid(g.kind); setOldest(false); }} data-testid={`artist-shelf-more-${g.kind}`}>{t("release.viewMore")}<Icon name="chevronRight" size={18} /></button> : null}
                </div>
                <TileList label={g.title} className="scroller">{g.albums.slice(0, SHELF).map(tile)}</TileList>
              </section>
            ))}
            {about ? (
              <section className="detail-section artist-about" aria-labelledby="artist-about" data-testid="artist-about">
                <div className="detail-section-head"><h2 id="artist-about">{t("artist.about")}</h2></div>
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
