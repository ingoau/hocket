import { useCallback } from "react";
import type { Artist } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { AlbumGrid, type GridItem } from "../components/AlbumGrid";
import { usePagedGrid } from "./paged";

export function Artists() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "artists", data: { server_id: serverId, page: { offset, limit } } });
    if (r.type !== "artists") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [serverId]);
  const { rows, total, onNeedRange } = usePagedGrid<Artist>(fetchPage, [serverId], 120);
  const items: (GridItem | undefined)[] = rows.map((a) => (a ? { id: a.id, title: a.name, subtitle: t("artist.albumsCount", { albums: a.albumCount, songs: a.songCount }), coverArt: a.coverArt, round: true } : undefined));
  const play = (it: GridItem) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "artist", data: { id: it.id } }, label: it.title, sort: "default", tracks: [] }, startIndex: 0, shuffle: true, saveOutgoing: true } } });
  return (
    <div className="view" data-testid="view-artists">
      <div className="view-header"><h1>{t("artists.title")}</h1><span className="muted">{total}</span></div>
      <AlbumGrid items={items} total={total} scope="artists" targetKind="artists" onOpen={(it) => navigate({ view: "artist", id: it.id })} onPlay={play} onNeedRange={onNeedRange} tileWidth={140} emptyMessage={t("artists.empty")} testId="artists-grid" label={t("artists.title")} />
    </div>
  );
}
