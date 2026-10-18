import { useCallback, useMemo, useState } from "react";
import type { Album, SortOrder } from "@core/api";
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

/** Albums: the library's (Pinned, Recent, then all of them), or one genre's or artist's when given. */
export function Albums({ artistId, genre, title }: { artistId?: string; genre?: string; title?: string } = {}) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const [order, setOrder] = useState<ListSort>(() => loadListSort("albums"));
  const sort = order.sort as SortOrder;
  const desc = order.descending;
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "albums", data: { server_id: serverId, artist_id: artistId, genre, sort, descending: desc, page: { offset, limit } } });
    if (r.type !== "albums") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [serverId, artistId, genre, sort, desc]);
  const { rows, total, onNeedRange } = usePagedGrid<Album>(fetchPage, [serverId, artistId, genre, sort, desc], 120);
  const items: (GridItem | undefined)[] = rows.map((a) => (a ? { id: a.id, title: a.name, subtitle: [a.artist, a.year].filter(Boolean).join(" · "), coverArt: a.coverArt, badge: a.offline === "downloaded" ? "download" : undefined } : undefined));
  const playAlbum = (id: string, label: string) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id } }, label, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const sorter = <SortMenu list="albums" value={order} onChange={(v) => { setOrder(v); saveListSort("albums", v); }} testId="albums-sort" />;
  // A genre's or an artist's albums are a plain list; the library's has its groups and the wordmark title.
  const library = !artistId && !genre;
  return (
    <div className="view" data-testid="view-albums">
      <div className="view-header">
        <h1 className={library ? "brand-type" : undefined}>{title ?? t("albums.title")}</h1>
        <span className="muted count-chip">{total}</span>
        {library ? null : <div className="actions">{sorter}</div>}
      </div>
      <AlbumGrid items={items} total={total} scope={`albums:${artistId ?? ""}:${genre ?? ""}`} targetKind="albums" onOpen={(it) => navigate({ view: "album", id: it.id })} onPlay={(it) => playAlbum(it.id, it.title)} onNeedRange={onNeedRange} emptyMessage={t("albums.empty")} testId="albums-grid" label={title ?? t("albums.title")}
        header={library ? <><AlbumGroups play={playAlbum} /><ListHeader id="albums-all" title={t("albums.all")} actions={sorter} className="list-all" /></> : undefined} />
    </div>
  );
}

function AlbumGroups({ play }: { play: (id: string, label: string) => void }) {
  const navigate = useApp((s) => s.navigate);
  const pins = usePinsOf("album");
  const history = useRecentHistory();
  const pinnedIds = usePinnedIds("album");
  const recentIds = useMemo(() => recentFromHistory(history ?? [], (tr) => tr.albumId, pinnedIds), [history, pinnedIds]);
  const recent = useDetails("album", recentIds);
  const item = (id: string, title: string, subtitle: string | undefined, coverArt: string | undefined): GroupItem => ({
    key: id, title, subtitle, coverArt,
    open: () => navigate({ view: "album", id }),
    play: () => play(id, title),
    menu: (e) => void openContextMenu(e, { type: "albums", data: { ids: [id] } }),
    dragTarget: { type: "albums", data: { ids: [id] } },
  });
  return (
    <ListGroups list="albums" pinned={pins.map((p) => item(p.pin.id, p.title, p.subtitle, p.coverArt))} recent={recent.map((a) => item(a.id, a.name, a.artist, a.coverArt))}
      render={(list, label, testId) => <TileRow items={list} label={label} testId={testId} />} />
  );
}
