import { useCallback, useMemo, useState } from "react";
import type { SortOrder } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { openContextMenu } from "../components/ContextMenu";
import { ListGroups, ListHeader, SongRows, usePinnedIds, usePinsOf, useRecentHistory, type GroupItem } from "../components/ListGroups";
import { SortMenu } from "../components/SortMenu";
import { TrackTable, usePagedTracks } from "../components/TrackTable";
import { loadListSort, saveListSort, type ListSort } from "../lib/list-sort";
import { GROUP_SIZE } from "../lib/list-groups";

export function Songs() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const playing = useApp((s) => s.nowPlaying?.track.id);
  const [order, setOrder] = useState<ListSort>(() => loadListSort("songs"));
  const sort = order.sort as SortOrder;
  const desc = order.descending;
  const choose = (v: ListSort) => { setOrder(v); saveListSort("songs", v); };
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
      <div className="view-header"><h1 className="brand-type">{t("songs.title")}</h1><span className="muted count-chip">{total}</span></div>
      <div className="list-top">
        {/* The table below keeps most of the height: the groups scroll on their own when the window is short. */}
        <div className="list-top-groups"><SongGroups /></div>
        <ListHeader id="songs-all" title={t("songs.all")} actions={<SortMenu list="songs" value={order} onChange={choose} testId="songs-sort" />} className="list-all" />
      </div>
      <div className="view-body no-pad" style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <TrackTable tracks={rows} total={total} columns={["art", "title", "artist", "album", "year", "genre", "rating", "love", "plays", "bpm", "duration", "offline"]} scope="songs" sort={sort} descending={desc} onSort={(s, d) => choose({ sort: s, descending: d })} onNeedRange={onNeedRange} onPlay={onPlay} playingTrackId={playing} emptyMessage={t("songs.empty")} testId="songs-table" />
      </div>
    </div>
  );
}

/** Pinned songs, then the songs played lately; a click plays that one song. */
function SongGroups() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const pins = usePinsOf("track");
  const pinned = usePinnedIds("track");
  const history = useRecentHistory();
  const recent = useMemo(() => {
    const seen = new Set<string>();
    return (history ?? []).map((h) => h.track).filter((tr) => !pinned.has(tr.id) && !seen.has(tr.id) && !!seen.add(tr.id)).slice(0, GROUP_SIZE);
  }, [history, pinned]);
  const item = (id: string, title: string, subtitle: string | undefined, coverArt: string | undefined): GroupItem => ({
    key: id, title, subtitle, coverArt,
    open: () => bridge().dispatch({ type: "playTracks", data: { server_id: serverId, track_ids: [id], start_index: 0, label: title, shuffle: false } }),
    menu: (e) => void openContextMenu(e, { type: "tracks", data: { ids: [id] } }),
    dragTarget: { type: "tracks", data: { ids: [id] } },
  });
  return (
    <ListGroups list="songs" pinned={pins.map((p) => item(p.pin.id, p.title, p.subtitle, p.coverArt))} recent={recent.map((tr) => item(tr.id, tr.title, tr.artist ?? t("misc.unknownArtist"), tr.coverArt))}
      render={(list, label, testId) => <SongRows items={list} label={label} testId={testId} />} />
  );
}
