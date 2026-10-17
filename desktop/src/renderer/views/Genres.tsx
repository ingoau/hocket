import { useMemo, useState } from "react";
import type { Genre } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { openItemsMenu } from "../components/ContextMenu";
import { genreMenuItems } from "../lib/pin-menus";
import { useQuery } from "../store/queries";
import { EmptyState } from "../components/EmptyState";
import { TileList } from "../components/Tile";
import { ListGroups, ListSection, usePinsOf, useRecentHistory } from "../components/ListGroups";
import { SortMenu } from "../components/SortMenu";
import { loadListSort, saveListSort, sortGenres, type ListSort } from "../lib/list-sort";
import { recentGenres } from "../lib/list-groups";

/** Genres: Pinned, the genres of the songs played lately, then all of them (sorted here; the core hands them over whole). */
export function Genres() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const [order, setOrder] = useState<ListSort>(() => loadListSort("genres"));
  const { data, loading } = useQuery(() => ({ type: "genres", data: { server_id: serverId } }), "genres", [serverId]);
  const sorted = useMemo(() => sortGenres(data ?? [], order.sort, order.descending), [data, order]);
  const tile = (g: Genre, testId: string) => (
    <li key={g.name} onContextMenu={(e) => openItemsMenu(e, genreMenuItems(g.name, serverId))}>
      <button type="button" className="genre-tile" data-roving onClick={() => navigate({ view: "genre", id: g.name })} data-testid={testId} style={{ "--genre-hue": genreHue(g.name) } as React.CSSProperties}>
        <span className="genre-name">{g.name}</span>
        <span className="genre-count small muted">{t("genres.count", { albums: g.albumCount, songs: g.songCount })}</span>
      </button>
    </li>
  );
  return (
    <div className="view" data-testid="view-genres">
      <div className="view-header"><h1 className="brand-type">{t("genres.title")}</h1>{data?.length ? <span className="muted count-chip">{data.length}</span> : null}</div>
      <div className="view-body">
        <GenreGroups genres={data ?? []} tile={tile} />
        <ListSection id="genres-all" title={t("genres.all")} actions={<SortMenu list="genres" value={order} onChange={(v) => { setOrder(v); saveListSort("genres", v); }} testId="genres-sort" />}>
          {!loading && !data?.length ? <EmptyState message={t("genres.empty")} icon="genre" /> : null}
          <TileList label={t("genres.all")} className="grid genre-grid">
            {sorted.map((g) => tile(g, "genre-tile"))}
          </TileList>
        </ListSection>
      </div>
    </div>
  );
}

function GenreGroups({ genres, tile }: { genres: Genre[]; tile: (g: Genre, testId: string) => React.ReactNode }) {
  const pins = usePinsOf("genre");
  const history = useRecentHistory();
  const byName = useMemo(() => new Map(genres.map((g) => [g.name, g])), [genres]);
  const pinned = useMemo(() => pins.map((p) => byName.get(p.pin.id)).filter((g): g is Genre => !!g), [pins, byName]);
  // The history's songs don't carry their genre: read the songs (one query) for it.
  const ids = useMemo(() => [...new Set((history ?? []).map((h) => h.track.id))], [history]);
  const tracks = useQuery(() => (ids.length ? { type: "tracksByIds", data: { ids } } : null), "trackList", [ids.join(",")]);
  const recent = useMemo(() => {
    const genreOf = new Map((tracks.data ?? []).map((tr) => [tr.id, tr.genre]));
    return recentGenres(ids, genreOf, new Set(byName.keys()), new Set(pinned.map((g) => g.name))).map((n) => byName.get(n)!);
  }, [tracks.data, ids, byName, pinned]);
  const asItems = (list: Genre[]) => list.map((g) => ({ key: g.name, title: g.name, open: () => undefined }));
  return (
    <ListGroups list="genres" pinned={asItems(pinned)} recent={asItems(recent)}
      render={(list, label, testId) => <TileList label={label} className="genre-row">{list.map((it) => tile(byName.get(it.key)!, testId))}</TileList>} />
  );
}

/** A stable hue per genre name, for the decorative shape on its card (never behind text). */
function genreHue(name: string): number {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return (h % 24) * 15;
}
