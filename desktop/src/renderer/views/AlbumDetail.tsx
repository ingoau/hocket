import { useMemo } from "react";
import { t } from "@shared/strings";
import { useApp, useSetting } from "../store/app";
import { SK } from "@shared/settings-keys";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { sameArtist, TrackTable, type ColumnId } from "../components/TrackTable";
import { DetailHead } from "../components/DetailHead";
import { Icon } from "../components/Icon";
import { Heart, Stars } from "../components/Stars";
import { openContextMenu, openItemsMenu, openMenuFromButton } from "../components/ContextMenu";
import { fmtDate, fmtDuration, fmtTime } from "../lib/format";
import { Tile, TileList } from "../components/Tile";
import { executeAction } from "../store/actions";
import { trackTableHeight } from "../lib/track-row";
import { releaseKind } from "../lib/release-kind";
import { offlineLabel } from "../components/OfflineBadge";
import { useAlbumDwellPrime, usePlayIntent } from "../lib/use-prime";
import type { AlbumResumePoint, Track } from "@core/api";

function resumeFraction(list: Track[], resume: AlbumResumePoint): number {
  const duration = list.find((t) => t.id === resume.trackId)?.durationMs || resume.durationMs;
  return duration > 0 ? Math.min(1, resume.positionMs / duration) : 0;
}

export function AlbumDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.accounts[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const isPlaying = useApp((s) => s.transport.position.isPlaying);
  const inThisAlbum = useApp((s) => {
    const kind = s.session?.context?.kind;
    return kind?.type === "album" && kind.data.id === id;
  });
  const album = useQuery(() => ({ type: "album", data: { id } }), "albumDetail", [id]);
  const tracks = useQuery(() => ({ type: "albumTracks", data: { id } }), "trackList", [id]);
  const a = album.data;
  const list = tracks.data ?? [];
  // The album primer: 2 s on the page primes the album's start; resting on a
  // play button primes that track. Once per visit (a remount is a new visit).
  useAlbumDwellPrime(a?.id);
  const primer = usePlayIntent(`album:${id}`);
  const play = (start?: number, shuffle = false) => bridge().dispatch({ type: "playContext", data: { args: { context: { source: serverId, kind: { type: "album", data: { id } }, label: a?.name ?? "", sort: "default", tracks: [] }, startIndex: start, shuffle, saveOutgoing: true } } });
  const more = useQuery(() => (a?.artistId ? { type: "albums", data: { server_id: serverId, artist_id: a.artistId, genre: undefined, sort: "year", descending: true, page: { offset: 0, limit: 13 } } } : null), "albums", [a?.artistId, serverId]);
  const others = (more.data?.items ?? []).filter((x) => x.id !== id).slice(0, 12);
  const footnote = [a?.year ? t("album.released", { year: a.year }) : "", a?.created ? t("album.added", { date: fmtDate(a.created) }) : "", a?.playCount ? t("album.plays", { count: a.playCount }) : ""].filter(Boolean).join(" · ");
  // Downloaded or on its way; fully in the stream cache is not kept, so the button offers to download it.
  // The artist column names only the tracks by someone other than the album's artist (gone when
  // none are), or every track by the setting. Decided once the album is in, so it doesn't flash.
  const allArtists = useSetting(SK.displayAlbumTrackArtists, false);
  const resumeOn = useSetting(SK.resumeAlbums, true);
  const resumes = useApp((s) => s.albumResumes);
  const resume = resumeOn && !(inThisAlbum && isPlaying) ? resumes.find((p) => p.albumId === id) : undefined;
  const startOver = () => {
    bridge().dispatch({ type: "clearAlbumResume", data: { album_id: id } });
    play();
  };
  const anyOther = !!a && list.some((x) => !sameArtist(x.artist, a.artist));
  const showArtist = allArtists || anyOther;
  const columns = useMemo<ColumnId[]>(() => ["index", "title", ...(showArtist ? ["artist" as const] : []), "rating", "love", "plays", "duration", "offline"], [showArtist]);
  const kept = a?.offline === "downloaded" || a?.offline === "downloading";
  return (
    <div className="view detail-page" data-testid="view-album">
      <DetailHead coverArt={a?.coverArt} icon="album" onContextMenu={(e) => a && void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })} testId="album-head" actions={
        <>
          <button type="button" className="btn primary fab album-play" aria-label={resume ? t("album.resumeMenu") : t("album.play")} onClick={() => (resume ? bridge().dispatch({ type: "resumeAlbum", data: { album_id: id } }) : play())} onContextMenu={resume ? (e) => { e.preventDefault(); e.stopPropagation(); openItemsMenu(e, [{ id: "startOver", label: t("album.startOver"), icon: "refresh", enabled: true, run: startOver }]); } : undefined} {...primer.bind(resume ? undefined : list[0]?.id)} data-testid="album-play"><Icon name={resume ? "resume" : "play"} size={24} filled /> {resume ? t("album.resume") : t("album.play")}</button>
          <button type="button" className="btn icon" aria-label={t("album.shuffle")} title={t("album.shuffle")} onClick={() => play(undefined, true)} data-testid="album-shuffle"><Icon name="shuffle" size={24} /></button>
          {a ? <Heart size={24} on={a.loved} onToggle={() => bridge().dispatch({ type: "setLoved", data: { targets: [{ type: "album", data: { id: a.id } }], loved: !a.loved } })} /> : null}
          {a ? <button type="button" className={`btn icon ${kept ? "on" : ""}`} aria-pressed={kept} aria-label={kept ? t("album.downloaded") : t("album.download")} title={kept ? t("album.downloaded") : t("album.download")} onClick={() => void executeAction(kept ? "unpin" : "download", { type: "albums", data: { ids: [a.id] } })} data-testid="album-download"><Icon name="download" size={24} filled={kept} /></button> : null}
          <button type="button" className="btn icon" aria-label={t("misc.more")} aria-haspopup="menu" title={t("misc.more")} onClick={(e) => a && openMenuFromButton(e, { type: "albums", data: { ids: [a.id] } }, undefined, resume ? [{ id: "startOver", label: t("album.startOver"), icon: "refresh", enabled: true, run: startOver }] : undefined)} data-testid="album-more"><Icon name="more" /></button>
          <span className="spacer" />
          {a ? (
            <div className="detail-rating">
              <span className="detail-rating-label" aria-hidden="true">{t("album.yourRating")}</span>
              <Stars value={a.rating} label={t("a11y.ratingOf", { title: a.name })} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [{ type: "album", data: { id: a.id } }], rating: r } })} />
            </div>
          ) : null}
        </>
      }>
          {offlineLabel(a?.offline) ? <span className="eyebrow" data-testid="album-offline">{offlineLabel(a?.offline)}</span> : null}
          <h1>{a?.name ?? ""}</h1>
          <div className="detail-sub">
            <a href="#" className="plain-link" data-testid="album-artist" onClick={(e) => { e.preventDefault(); if (a?.artistId) navigate({ view: "artist", id: a.artistId }); }}>{a?.artist}</a>
            <span>{[a?.year, a?.genre, t("album.tracks", { count: a?.songCount ?? 0, duration: fmtDuration(a?.durationMs ?? 0) })].filter(Boolean).join(" · ")}</span>
          </div>
      </DetailHead>
      <div className="view-body">
        <section className="detail-section" aria-labelledby="album-tracks-title">
          <div className="detail-section-head">
            <h2 id="album-tracks-title">{t("album.tracksTitle")}</h2>
            <span className="muted">{t("album.tracks", { count: list.length, duration: fmtTime(list.reduce((n, x) => n + (x.durationMs ?? 0), 0)) })}</span>
          </div>
          <div style={{ height: trackTableHeight(list.length), display: "flex", flexDirection: "column" }}>
            <TrackTable tracks={list} total={list.length} columns={columns} omitArtist={allArtists ? undefined : (a?.artist ?? "")} scope={`album:${id}`} label={a?.name} onPlay={(i) => play(i)} playingTrackId={playing} resumeTrackId={resume && playing !== resume.trackId ? resume.trackId : undefined} resumeFraction={resume ? resumeFraction(list, resume) : undefined} emptyMessage={t("songs.empty")} testId="album-tracks" primer={primer} />
          </div>
          {footnote ? <div className="detail-footnote">{footnote}</div> : null}
        </section>
        {others.length && a ? (
          <section className="detail-section home-section" aria-labelledby="album-more-title" data-testid="album-more-by">
            <div className="detail-section-head">
              <h2 id="album-more-title">{t("album.moreBy", { artist: a.artist ?? "" })}</h2>
              {a.artistId ? <button type="button" className="btn view-more" onClick={() => navigate({ view: "artist", id: a.artistId! })} aria-label={t("album.viewMoreBy", { artist: a.artist ?? "" })}>{t("release.viewMore")}<Icon name="chevronRight" size={18} /></button> : null}
            </div>
            <TileList label={t("album.moreBy", { artist: a.artist ?? "" })} className="scroller">{others.map((al) => (
              <Tile key={al.id} title={al.name} subtitle={[al.year, t(`release.one.${releaseKind(al)}`)].filter(Boolean).join(" · ")} coverArt={al.coverArt} onOpen={() => navigate({ view: "album", id: al.id })} onContextMenu={(e) => void openContextMenu(e, { type: "albums", data: { ids: [al.id] } })} dragTarget={{ type: "albums", data: { ids: [al.id] } }} />
            ))}</TileList>
          </section>
        ) : null}
      </div>
    </div>
  );
}
