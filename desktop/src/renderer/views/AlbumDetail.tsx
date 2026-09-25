import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { TrackTable } from "../components/TrackTable";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { Heart, Stars } from "../components/Stars";
import { openContextMenu, openMenuFromButton } from "../components/ContextMenu";
import { fmtDuration } from "../lib/format";
import { offlineLabel } from "../components/OfflineBadge";
import { useAlbumDwellPrime, usePlayIntent } from "../lib/use-prime";

export function AlbumDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const album = useQuery(() => ({ type: "album", data: { id } }), "albumDetail", [id]);
  const tracks = useQuery(() => ({ type: "albumTracks", data: { id } }), "trackList", [id]);
  const a = album.data;
  const list = tracks.data ?? [];
  // The album primer: 2 s on the page primes the album's start; resting on a
  // play button primes that track. Once per visit (a remount is a new visit).
  useAlbumDwellPrime(a?.id);
  const primer = usePlayIntent(`album:${id}`);
  const play = (start = 0, shuffle = false) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label: a?.name ?? "", sort: "default", tracks: [] }, startIndex: start, shuffle, saveOutgoing: true } } });
  return (
    <div className="view" data-testid="view-album">
      <div className="detail-head" onContextMenu={(e) => a && void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })}>
        <Artwork id={a?.coverArt} size={300} className="art" />
        <div className="meta">
          <div className="muted small">{t("nav.albums")}{offlineLabel(a?.offline) ? <span className="badge ok" style={{ marginLeft: 8 }} data-testid="album-offline">{offlineLabel(a?.offline)}</span> : null}</div>
          <h1>{a?.name ?? ""}</h1>
          <div>
            <a href="#" className="plain-link" onClick={(e) => { e.preventDefault(); if (a?.artistId) navigate({ view: "artist", id: a.artistId }); }}>{a?.artist}</a>
            <span className="muted"> · {a?.year ?? ""}{a?.genre ? ` · ${a.genre}` : ""} · {t("album.tracks", { count: a?.songCount ?? 0, duration: fmtDuration(a?.durationMs ?? 0) })}</span>
          </div>
          <div className="actions">
            <button type="button" className="btn primary fab" onClick={() => play()} {...primer.bind(list[0]?.id)} data-testid="album-play"><Icon name="play" size={24} filled /> {t("album.play")}</button>
            <button type="button" className="btn tonal" onClick={() => play(0, true)}><Icon name="shuffle" size={20} /> {t("album.shuffle")}</button>
            {a ? <Heart on={a.loved} onToggle={() => bridge().dispatch({ type: "setLoved", data: { targets: [{ type: "album", data: { id: a.id } }], loved: !a.loved } })} /> : null}
            {a ? <Stars value={a.rating} label={t("a11y.ratingOf", { title: a.name })} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [{ type: "album", data: { id: a.id } }], rating: r } })} /> : null}
            <button type="button" className="btn icon" aria-label={t("misc.more")} aria-haspopup="menu" title={t("misc.more")} onClick={(e) => a && openMenuFromButton(e, { type: "albums", data: { ids: [a.id] } })} data-testid="album-more"><Icon name="more" /></button>
          </div>
        </div>
      </div>
      <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <TrackTable tracks={list} total={list.length} columns={["index", "title", "artist", "rating", "love", "plays", "duration", "offline"]} scope={`album:${id}`} label={a?.name} onPlay={(i) => play(i)} playingTrackId={playing} emptyMessage={t("songs.empty")} testId="album-tracks" primer={primer} />
      </div>
    </div>
  );
}
