import { useCallback } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { TrackTable, usePagedTracks } from "../components/TrackTable";
import { DetailHead } from "../components/DetailHead";
import { Icon } from "../components/Icon";
import { openContextMenu, openMenuFromButton } from "../components/ContextMenu";
import { fmtDuration } from "../lib/format";
import { usePlayIntent } from "../lib/use-prime";
import { executeAction } from "../store/actions";
import { trackTableHeight } from "../lib/track-row";

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
  const play = (start = 0, shuffle = false) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "playlist", data: { id } }, label: p?.name ?? "", sort: "default", tracks: [] }, startIndex: start, shuffle, saveOutgoing: true } } });
  const by = p?.owner ? t("playlist.by", { owner: p.owner }) : "";
  // Downloaded or on its way; fully in the stream cache is not kept, so the button offers to download it.
  const kept = p?.offline === "downloaded" || p?.offline === "downloading";
  return (
    <div className="view detail-page" data-testid="view-playlist">
      <DetailHead coverArt={p?.coverArt} icon="playlist" onContextMenu={(e) => p && void openContextMenu(e, { type: "playlists", data: { ids: [p.id] } })} testId="playlist-head" actions={
        <>
          <button type="button" className="btn primary fab" onClick={() => play()} {...primer.bind(rows[0]?.id)} data-testid="playlist-play"><Icon name="play" size={24} filled /> {t("album.play")}</button>
          <button type="button" className="btn icon" aria-label={t("album.shuffle")} title={t("album.shuffle")} onClick={() => play(0, true)} data-testid="playlist-shuffle"><Icon name="shuffle" size={24} /></button>
          {p ? <button type="button" className={`btn icon ${kept ? "on" : ""}`} aria-pressed={kept} aria-label={kept ? t("album.downloaded") : t("album.download")} title={kept ? t("album.downloaded") : t("album.download")} onClick={() => void executeAction(kept ? "unpin" : "download", { type: "playlists", data: { ids: [p.id] } })} data-testid="playlist-download"><Icon name="download" size={24} filled={kept} /></button> : null}
          <button type="button" className="btn icon" aria-label={t("misc.more")} aria-haspopup="menu" title={t("misc.more")} onClick={(e) => p && openMenuFromButton(e, { type: "playlists", data: { ids: [p.id] } })} data-testid="playlist-more"><Icon name="more" /></button>
        </>
      }>
          {p ? <span className="eyebrow">{p.isSmart ? t("playlists.smart") : p.public ? t("playlist.public") : t("playlist.private")}</span> : null}
          <h1>{p?.name ?? ""}</h1>
          <div className="detail-sub"><span>{[by, t("playlist.tracks", { count: p?.songCount ?? 0, duration: fmtDuration(p?.durationMs ?? 0) })].filter(Boolean).join(" · ")}</span></div>
          {p?.comment ? <div className="detail-note">{p.comment}</div> : null}
          {p?.isSmart ? <div className="detail-sub"><span><Icon name="info" size={12} /> {t("playlists.readOnly")}</span></div> : null}
      </DetailHead>
      <div className="view-body">
        <section className="detail-section" aria-labelledby="playlist-tracks-title">
          <div className="detail-section-head"><h2 id="playlist-tracks-title">{t("album.tracksTitle")}</h2></div>
          <div style={{ height: trackTableHeight(total), display: "flex", flexDirection: "column" }}>
            <TrackTable tracks={rows} total={total} columns={["position", "art", "title", "artist", "album", "love", "duration", "offline"]} scope={`playlist:${id}`} label={p?.name} onNeedRange={onNeedRange} onPlay={(i) => play(i)} playingTrackId={playing} context={{ playlistId: id }}
              onReorder={editable ? (from, to) => bridge().dispatch({ type: "playlistMove", data: { playlist_id: id, from_index: from, to_index: to } }) : undefined}
              onDelete={editable ? (indices) => bridge().dispatch({ type: "playlistRemove", data: { playlist_id: id, indices } }) : undefined}
              emptyMessage={t("songs.empty")} testId="playlist-tracks" primer={primer} />
          </div>
        </section>
      </div>
    </div>
  );
}
