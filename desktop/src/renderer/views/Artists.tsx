import { useCallback, useMemo, useState } from "react";
import type { Artist, SortOrder } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { AlbumGrid, type GridItem } from "../components/AlbumGrid";
import { openContextMenu } from "../components/ContextMenu";
import { ListGroups, ListHeader, TileRow, useDetails, usePinnedIds, usePinsOf, useRecentHistory, type GroupItem } from "../components/ListGroups";
import { SortMenu } from "../components/SortMenu";
import { usePagedGrid } from "./paged";
import { loadListSort, saveListSort, type ListSort } from "../lib/list-sort";
import { recentFromHistory } from "../lib/list-groups";

/** Artists: a grid of round portraits (Pinned, Recent, then all of them in the chosen order). */
export function Artists() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const [order, setOrder] = useState<ListSort>(() => loadListSort("artists"));
  const sort = order.sort as SortOrder;
  const desc = order.descending;
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "artists", data: { server_id: serverId, page: { offset, limit }, sort, descending: desc } });
    if (r.type !== "artists") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [serverId, sort, desc]);
  const { rows, total, onNeedRange } = usePagedGrid<Artist>(fetchPage, [serverId, sort, desc], 120);
  const items: (GridItem | undefined)[] = rows.map((a) => (a ? { id: a.id, title: a.name, subtitle: albumCount(a), coverArt: a.coverArt, round: true } : undefined));
  const playArtist = (id: string, label: string) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "artist", data: { id } }, label, sort: "default", tracks: [] }, startIndex: 0, shuffle: true, saveOutgoing: true } } });
  const sorter = <SortMenu list="artists" value={order} onChange={(v) => { setOrder(v); saveListSort("artists", v); }} testId="artists-sort" />;
  return (
    <div className="view" data-testid="view-artists">
      <div className="view-header"><h1 className="brand-type">{t("artists.title")}</h1><span className="muted count-chip">{total}</span></div>
      <AlbumGrid items={items} total={total} scope="artists" targetKind="artists" onOpen={(it) => navigate({ view: "artist", id: it.id })} onPlay={(it) => playArtist(it.id, it.title)} onNeedRange={onNeedRange} tileWidth={150} emptyMessage={t("artists.empty")} testId="artists-grid" label={t("artists.title")}
        header={<><ArtistGroups play={playArtist} /><ListHeader id="artists-all" title={t("artists.all")} actions={sorter} className="list-all" /></>} />
    </div>
  );
}

/** Under an artist's name: how many albums (Home's favourite artists say the same). */
function albumCount(a: Artist): string {
  return a.albumCount === 1 ? t("artists.albumCount.one") : t("artists.albumCount.other", { n: a.albumCount });
}

function ArtistGroups({ play }: { play: (id: string, label: string) => void }) {
  const navigate = useApp((s) => s.navigate);
  const pins = usePinsOf("artist");
  const history = useRecentHistory();
  const pinnedIds = usePinnedIds("artist");
  const recentIds = useMemo(() => recentFromHistory(history ?? [], (tr) => tr.artistId, pinnedIds), [history, pinnedIds]);
  const recent = useDetails("artist", recentIds);
  const item = (id: string, title: string, subtitle: string | undefined, coverArt: string | undefined): GroupItem => ({
    key: id, title, subtitle, coverArt, round: true,
    open: () => navigate({ view: "artist", id }),
    play: () => play(id, title),
    menu: (e) => void openContextMenu(e, { type: "artists", data: { ids: [id] } }),
    dragTarget: { type: "artists", data: { ids: [id] } },
  });
  return (
    <ListGroups list="artists" pinned={pins.map((p) => item(p.pin.id, p.title, p.subtitle, p.coverArt))} recent={recent.map((a) => item(a.id, a.name, albumCount(a), a.coverArt))}
      render={(list, label, testId) => <TileRow items={list} label={label} testId={testId} />} />
  );
}
