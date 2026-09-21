import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { TrackTable } from "../components/TrackTable";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { Heart, Stars } from "../components/Stars";
import { openContextMenu } from "../components/ContextMenu";
import { fmtDuration } from "../lib/format";

export function AlbumDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const album = useQuery(() => ({ type: "album", data: { id } }), "albumDetail", [id]);
  const tracks = useQuery(() => ({ type: "albumTracks", data: { id } }), "trackList", [id]);
  const a = album.data;
  const list = tracks.data ?? [];
  const play = (start = 0, shuffle = false) => a && bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, startIndex: start, shuffle, saveOutgoing: true } } });
  return (
    <div className="view" data-testid="view-album">
      <div className="detail-head" onContextMenu={(e) => a && void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })}>
        <Artwork id={a?.coverArt} size={300} className="art" />
        <div className="meta">
          <div className="muted small">{t("nav.albums")}{a?.offline === "downloaded" ? <span className="badge ok" style={{ marginLeft: 8 }}>{t("col.offline")}</span> : null}</div>
          <h1>{a?.name ?? ""}</h1>
          <div>
            <a href="#" onClick={(e) => { e.preventDefault(); if (a?.artistId) navigate({ view: "artist", id: a.artistId }); }} style={{ textDecoration: "none", color: "var(--fg)" }}>{a?.artist}</a>
            <span className="muted"> · {a?.year ?? ""}{a?.genre ? ` · ${a.genre}` : ""} · {t("album.tracks", { count: a?.songCount ?? 0, duration: fmtDuration(a?.durationMs ?? 0) })}</span>
          </div>
          <div className="actions">
            <button type="button" className="btn primary" onClick={() => play()} data-testid="album-play"><Icon name="play" size={14} style={{ fill: "currentColor" }} /> {t("album.play")}</button>
            <button type="button" className="btn" onClick={() => play(0, true)}><Icon name="shuffle" size={14} /> {t("album.shuffle")}</button>
            {a ? <Heart on={a.loved} onToggle={() => bridge().dispatch({ type: "setLoved", data: { targets: [{ type: "album", data: { id: a.id } }], loved: !a.loved } })} /> : null}
            {a ? <Stars value={a.rating} onChange={(r) => bridge().dispatch({ type: "setRating", data: { targets: [{ type: "album", data: { id: a.id } }], rating: r } })} /> : null}
            <button type="button" className="btn icon" aria-label={t("misc.more")} onClick={(e) => a && void openContextMenu(e, { type: "albums", data: { ids: [a.id] } })}><Icon name="more" /></button>
          </div>
        </div>
      </div>
      <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden", padding: "0 8px" }}>
        <TrackTable tracks={list} total={list.length} columns={["index", "title", "artist", "rating", "love", "plays", "duration", "offline"]} scope={`album:${id}`} onPlay={(i) => play(i)} playingTrackId={playing} emptyMessage={t("songs.empty")} testId="album-tracks" />
      </div>
    </div>
  );
}
