import { useCallback, useState } from "react";
import type { Album, SortOrder } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { AlbumGrid, type GridItem } from "../components/AlbumGrid";
import { usePagedGrid } from "./paged";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { Icon } from "../components/Icon";

const SORTS: SortOrder[] = ["title", "artist", "year", "dateAdded", "rating", "playCount", "random"];

export function Albums({ artistId, genre, title }: { artistId?: string; genre?: string; title?: string } = {}) {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const [sort, setSort] = useState<SortOrder>(() => loadLocal("albums.sort", "dateAdded" as SortOrder));
  const [desc, setDesc] = useState(() => loadLocal("albums.desc", true));
  const fetchPage = useCallback(async (offset: number, limit: number) => {
    const r = await bridge().query({ type: "albums", data: { server_id: serverId, artist_id: artistId, genre, sort, descending: desc, page: { offset, limit } } });
    if (r.type !== "albums") throw new Error("bad result");
    return { items: r.data.items, total: r.data.total };
  }, [serverId, artistId, genre, sort, desc]);
  const { rows, total, onNeedRange } = usePagedGrid<Album>(fetchPage, [serverId, artistId, genre, sort, desc], 120);
  const items: (GridItem | undefined)[] = rows.map((a) => (a ? { id: a.id, title: a.name, subtitle: [a.artist, a.year].filter(Boolean).join(" · "), coverArt: a.coverArt, badge: a.offline === "downloaded" ? "download" : undefined } : undefined));
  const play = (it: GridItem) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "album", data: { id: it.id } }, label: it.title, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  return (
    <div className="view" data-testid="view-albums">
      <div className="view-header">
        <h1>{title ?? t("albums.title")}</h1>
        <span className="muted">{total}</span>
        <div className="actions">
          <select className="select" value={sort} aria-label={t("sort.label", { sort: "" })} onChange={(e) => { setSort(e.target.value as SortOrder); saveLocal("albums.sort", e.target.value); }} data-testid="albums-sort">
            {SORTS.map((s) => <option key={s} value={s}>{t(`sort.${s}` as never)}</option>)}
          </select>
          <button type="button" className="btn icon" aria-label={t("filters.descending")} aria-pressed={desc} onClick={() => { setDesc(!desc); saveLocal("albums.desc", !desc); }}><Icon name={desc ? "arrowDown" : "arrowUp"} size={14} /></button>
        </div>
      </div>
      <AlbumGrid items={items} total={total} scope={`albums:${artistId ?? ""}:${genre ?? ""}`} targetKind="albums" onOpen={(it) => navigate({ view: "album", id: it.id })} onPlay={play} onNeedRange={onNeedRange} emptyMessage={t("albums.empty")} testId="albums-grid" />
    </div>
  );
}
