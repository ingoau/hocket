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
      <div className="view-header"><h1>{t("genres.title")}</h1>{data?.length ? <span className="muted count-chip">{data.length}</span> : null}</div>
      <div className="view-body">
        {!loading && !data?.length ? <EmptyState message={t("genres.empty")} icon="genre" /> : null}
        <TileList label={t("genres.title")} className="grid genre-grid">
          {(data ?? []).map((g) => (
            <li key={g.name}>
              <button type="button" className="genre-tile" data-roving onClick={() => navigate({ view: "genre", id: g.name })} data-testid="genre-tile" style={{ "--genre-hue": genreHue(g.name) } as React.CSSProperties}>
                <span className="genre-name">{g.name}</span>
                <span className="genre-count small muted">{t("genres.count", { albums: g.albumCount, songs: g.songCount })}</span>
              </button>
            </li>
          ))}
        </TileList>
      </div>
    </div>
  );
}

/** A stable hue per genre name, for the decorative shape on its card (never behind text). */
function genreHue(name: string): number {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return (h % 24) * 15;
}
