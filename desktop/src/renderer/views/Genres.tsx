import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { EmptyState } from "../components/EmptyState";

export function Genres() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const { data, loading } = useQuery(() => ({ type: "genres", data: { server_id: serverId } }), "genres", [serverId]);
  return (
    <div className="view" data-testid="view-genres">
      <div className="view-header"><h1>{t("genres.title")}</h1><span className="muted">{data?.length ?? ""}</span></div>
      <div className="view-body">
        {!loading && !data?.length ? <EmptyState message={t("genres.empty")} /> : null}
        <div className="grid" style={{ "--tile-w": "200px" } as React.CSSProperties}>
          {(data ?? []).map((g) => (
            <div key={g.name} className="genre-tile" role="button" tabIndex={0} onClick={() => navigate({ view: "genre", id: g.name })} onKeyDown={(e) => e.key === "Enter" && navigate({ view: "genre", id: g.name })} data-testid="genre-tile">
              <div style={{ fontWeight: 600 }}>{g.name}</div>
              <div className="small muted">{t("genres.count", { albums: g.albumCount, songs: g.songCount })}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
