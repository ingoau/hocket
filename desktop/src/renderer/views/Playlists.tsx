import { useMemo, useState } from "react";
import type { Playlist } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { AlbumGrid, type GridItem } from "../components/AlbumGrid";
import { openContextMenu } from "../components/ContextMenu";
import { ListGroups, ListHeader, TileRow, usePinnedIds, usePinsOf, type GroupItem } from "../components/ListGroups";
import { SortMenu } from "../components/SortMenu";
import { executeAction } from "../store/actions";
import { Icon } from "../components/Icon";
import { loadListSort, saveListSort, sortPlaylists, type ListSort } from "../lib/list-sort";
import { recentPlaylists } from "../lib/list-groups";

/** Playlists: Pinned, the most recently changed, then all of them (the core hands them over whole, so they're sorted here). */
export function Playlists() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const [order, setOrder] = useState<ListSort>(() => loadListSort("playlists"));
  const { data } = useQuery(() => ({ type: "playlists", data: { server_id: serverId } }), "playlists", [serverId]);
  const sorted = useMemo(() => sortPlaylists(data ?? [], order.sort, order.descending), [data, order]);
  const items: GridItem[] = sorted.map((p) => ({ id: p.id, title: p.name, subtitle: playlistLine(p), coverArt: p.coverArt, badge: p.offline === "downloaded" ? "download" : undefined }));
  const play = (id: string, label: string) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "playlist", data: { id } }, label, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const sorter = <SortMenu list="playlists" value={order} onChange={(v) => { setOrder(v); saveListSort("playlists", v); }} testId="playlists-sort" />;
  return (
    <div className="view" data-testid="view-playlists">
      <div className="view-header">
        <h1 className="brand-type">{t("playlists.title")}</h1>
        <span className="muted count-chip">{items.length}</span>
        <div className="actions"><button type="button" className="btn tonal new-playlist" onClick={() => void executeAction("ui.newPlaylist")}><Icon name="plus" size={18} /> {t("playlists.new")}</button></div>
      </div>
      <AlbumGrid items={items} total={items.length} scope="playlists" targetKind="playlists" onOpen={(it) => navigate({ view: "playlist", id: it.id })} onPlay={(it) => play(it.id, it.title)} emptyMessage={t("playlists.empty")} testId="playlists-grid" label={t("playlists.title")}
        header={<><PlaylistGroups playlists={data ?? []} play={play} /><ListHeader id="playlists-all" title={t("playlists.all")} actions={sorter} className="list-all" /></>} />
    </div>
  );
}

/** Fewer than the other lists' Recent: playlists are few, and the whole list is right below. */
const RECENT_PLAYLISTS = 4;

function playlistLine(p: Playlist): string {
  return `${t("misc.tracks", { count: p.songCount })}${p.isSmart ? ` · ${t("playlists.smart")}` : ""}${!p.isMine && p.owner ? ` · ${t("playlists.owner", { owner: p.owner })}` : ""}`;
}

function PlaylistGroups({ playlists, play }: { playlists: Playlist[]; play: (id: string, label: string) => void }) {
  const navigate = useApp((s) => s.navigate);
  const pins = usePinsOf("playlist");
  const pinnedIds = usePinnedIds("playlist");
  const byId = useMemo(() => new Map(playlists.map((p) => [p.id, p])), [playlists]);
  const recent = useMemo(() => recentPlaylists(playlists, pinnedIds, RECENT_PLAYLISTS), [playlists, pinnedIds]);
  const item = (id: string, title: string, subtitle: string | undefined, coverArt: string | undefined): GroupItem => ({
    key: id, title, subtitle, coverArt,
    open: () => navigate({ view: "playlist", id }),
    play: () => play(id, title),
    menu: (e) => void openContextMenu(e, { type: "playlists", data: { ids: [id] } }),
  });
  const pinned = pins.map((rp) => {
    const p = byId.get(rp.pin.id);
    return p ? item(p.id, p.name, playlistLine(p), p.coverArt) : item(rp.pin.id, rp.title, rp.subtitle, rp.coverArt);
  });
  return <ListGroups list="playlists" pinned={pinned} recent={recent.map((p) => item(p.id, p.name, playlistLine(p), p.coverArt))} render={(list, label, testId) => <TileRow items={list} label={label} testId={testId} />} />;
}
