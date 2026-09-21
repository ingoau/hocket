import { useCallback, useState } from "react";
import type { SortOrder } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { TrackTable, usePagedTracks } from "../components/TrackTable";
import { loadLocal, saveLocal } from "../lib/local-settings";

export function Songs() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const [sort, setSort] = useState<SortOrder>(() => loadLocal("songs.sort", "title" as SortOrder));
  const [desc, setDesc] = useState(() => loadLocal("songs.desc", false));
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "tracks", data: { server_id: serverId, filter: undefined, sort, descending: desc, page: { offset, limit } } });
    if (r.type !== "tracks") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [serverId, sort, desc]);
  const { rows, total, onNeedRange } = usePagedTracks(fetchPage, [serverId, sort, desc]);
  const onPlay = (index: number) => {
    // "All songs" is an ad-hoc context; play from the sorted list around the row.
    const ids = rows.filter((x): x is NonNullable<typeof x> => !!x).map((x) => x.id);
    const start = rows.slice(0, index).filter(Boolean).length;
    bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: ids, start_index: start, label: t("songs.title"), shuffle: false } });
  };
  return (
    <div className="view" data-testid="view-songs">
      <div className="view-header"><h1>{t("songs.title")}</h1><span className="muted">{total}</span></div>
      <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden", padding: "0 8px" }}>
        <TrackTable tracks={rows} total={total} columns={["art", "title", "artist", "album", "year", "genre", "rating", "love", "plays", "bpm", "duration", "offline"]} scope="songs" sort={sort} descending={desc} onSort={(s, d) => { setSort(s); setDesc(d); saveLocal("songs.sort", s); saveLocal("songs.desc", d); }} onNeedRange={onNeedRange} onPlay={onPlay} playingTrackId={playing} emptyMessage={t("songs.empty")} testId="songs-table" />
      </div>
    </div>
  );
}
