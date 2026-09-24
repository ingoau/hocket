import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { EmptyState } from "../components/EmptyState";
import { TileList } from "../components/Tile";

export function Genres() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const { data, loading } = useQuery(() => ({ type: "genres", data: { server_id: serverId } }), "genres", [serverId]);
  return (
    <div className="view" data-testid="view-genres">
      <div className="view-header"><h1>{t("genres.title")}</h1><span className="muted">{data?.length ?? ""}</span></div>
      <div className="view-body">
        {!loading && !data?.length ? <EmptyState message={t("genres.empty")} /> : null}
        <TileList label={t("genres.title")} className="grid genre-grid">
          {(data ?? []).map((g) => (
            <li key={g.name}>
              <button type="button" className="genre-tile" data-roving onClick={() => navigate({ view: "genre", id: g.name })} data-testid="genre-tile">
                <span style={{ fontWeight: 600 }}>{g.name}</span>
                <span className="small muted">{t("genres.count", { albums: g.albumCount, songs: g.songCount })}</span>
              </button>
            </li>
          ))}
        </TileList>
      </div>
    </div>
  );
}
