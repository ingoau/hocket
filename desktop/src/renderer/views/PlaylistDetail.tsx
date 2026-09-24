import { useCallback } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { TrackTable, usePagedTracks } from "../components/TrackTable";
import { Artwork } from "../components/Artwork";
import { Icon } from "../components/Icon";
import { openContextMenu, openMenuFromButton } from "../components/ContextMenu";
import { fmtDuration } from "../lib/format";
import { usePlayIntent } from "../lib/use-prime";

export function PlaylistDetail({ id }: { id: string }) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const pl = useQuery(() => ({ type: "playlist", data: { id } }), "playlistDetail", [id]);
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "playlistTracks", data: { id, page: { offset, limit } } });
    if (r.type !== "tracks") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [id]);
  const { rows, total, onNeedRange } = usePagedTracks(fetchPage, [id]);
  const p = pl.data;
  // Resting on a play button primes that track's start (the album primer, per track).
  const primer = usePlayIntent(`playlist:${id}`);
  const editable = !!p && !p.isSmart && p.isMine;
  const play = (start = 0, shuffle = false) => p && bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "playlist", data: { id: p.id } }, label: p.name, sort: "default", tracks: [] }, startIndex: start, shuffle, saveOutgoing: true } } });
  return (
    <div className="view" data-testid="view-playlist">
      <div className="detail-head" onContextMenu={(e) => p && void openContextMenu(e, { type: "playlists", data: { ids: [p.id] } })}>
        <Artwork id={p?.coverArt} size={300} className="art" />
        <div className="meta">
          <div className="muted small">{p?.isSmart ? t("playlists.smart") : t("nav.playlists")}{p && !p.isMine && p.owner ? ` · ${t("playlists.owner", { owner: p.owner })}` : ""}</div>
          <h1>{p?.name ?? ""}</h1>
          <div className="muted">{t("playlist.tracks", { count: p?.songCount ?? 0, duration: fmtDuration(p?.durationMs ?? 0) })}{p?.comment ? ` · ${p.comment}` : ""}</div>
          {p?.isSmart ? <div className="small muted"><Icon name="info" size={12} /> {t("playlists.readOnly")}</div> : null}
          <div className="actions">
            <button type="button" className="btn primary fab" onClick={() => play()} {...primer.bind(rows[0]?.id)} data-testid="playlist-play"><Icon name="play" size={24} filled /> {t("album.play")}</button>
            <button type="button" className="btn tonal" onClick={() => play(0, true)}><Icon name="shuffle" size={20} /> {t("album.shuffle")}</button>
            <button type="button" className="btn icon" aria-label={t("misc.more")} aria-haspopup="menu" title={t("misc.more")} onClick={(e) => p && openMenuFromButton(e, { type: "playlists", data: { ids: [p.id] } })} data-testid="playlist-more"><Icon name="more" /></button>
          </div>
        </div>
      </div>
      <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <TrackTable tracks={rows} total={total} columns={["index", "art", "title", "artist", "album", "rating", "love", "duration", "offline"]} scope={`playlist:${id}`} label={p?.name} onNeedRange={onNeedRange} onPlay={(i) => play(i)} playingTrackId={playing} context={{ playlistId: id }}
          onReorder={editable ? (from, to) => bridge().dispatch({ type: "playlistMove", data: { playlist_id: id, from_index: from, to_index: to } }) : undefined}
          onDelete={editable ? (indices) => bridge().dispatch({ type: "playlistRemove", data: { playlist_id: id, indices } }) : undefined}
          emptyMessage={t("songs.empty")} testId="playlist-tracks" primer={primer} />
      </div>
    </div>
  );
}
