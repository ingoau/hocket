import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { AlbumGrid, type GridItem } from "../components/AlbumGrid";
import { executeAction } from "../store/actions";
import { Icon } from "../components/Icon";

export function Playlists() {
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const navigate = useApp((s) => s.navigate);
  const { data } = useQuery(() => ({ type: "playlists", data: { server_id: serverId } }), "playlists", [serverId]);
  const items: GridItem[] = (data ?? []).map((p) => ({ id: p.id, title: p.name, subtitle: `${t("misc.tracks", { count: p.songCount })}${p.isSmart ? ` · ${t("playlists.smart")}` : ""}${!p.isMine && p.owner ? ` · ${t("playlists.owner", { owner: p.owner })}` : ""}`, coverArt: p.coverArt, badge: p.offline === "downloaded" ? "↓" : undefined }));
  const play = (it: GridItem) => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "playlist", data: { id: it.id } }, label: it.title, sort: "default", tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  return (
    <div className="view" data-testid="view-playlists">
      <div className="view-header">
        <h1>{t("playlists.title")}</h1>
        <span className="muted">{items.length}</span>
        <div className="actions"><button type="button" className="btn" onClick={() => void executeAction("ui.newPlaylist")}><Icon name="plus" size={14} /> {t("playlists.new")}</button></div>
      </div>
      <AlbumGrid items={items} total={items.length} scope="playlists" targetKind="playlists" onOpen={(it) => navigate({ view: "playlist", id: it.id })} onPlay={play} emptyMessage={t("playlists.empty")} testId="playlists-grid" />
    </div>
  );
}
