import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";

export function Filters() {
  const filters = useApp((s) => s.filters);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const play = (id: string) => {
    const f = filters.find((x) => x.id === id);
    if (f) bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "filter", data: { filter: f } }, label: f.name, sort: f.sort, tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  };
  return (
    <div className="view" data-testid="view-filters">
      <div className="view-header">
        <h1>{t("filters.title")}</h1>
        <div className="actions"><button type="button" className="btn primary" onClick={() => navigate({ view: "filter", id: "new" })} data-testid="new-filter"><Icon name="plus" size={14} /> {t("filters.new")}</button></div>
      </div>
      <div className="view-body">
        {!filters.length ? <EmptyState message={t("filters.empty")} action={<button type="button" className="btn" onClick={() => navigate({ view: "filter", id: "new" })}>{t("filters.new")}</button>} /> : null}
        {filters.map((f) => (
          <div key={f.id} className="pin-row" style={{ gridTemplateColumns: "auto 1fr auto auto" }} data-testid="filter-row">
            <Icon name="filter" size={18} />
            <button type="button" className="link-button" onClick={() => navigate({ view: "filter", id: f.id })}>
              <span style={{ display: "block" }}>{f.name}</span>
              <span className="small muted" style={{ display: "block" }}>{t("sort.label", { sort: t(`sort.${f.sort}` as never) })}{f.limit ? ` · ${t("filters.limit")} ${f.limit}` : ""}</span>
            </button>
            <button type="button" className="btn" aria-label={`${t("filters.play")}: ${f.name}`} onClick={() => play(f.id)}><Icon name="play" size={13} style={{ fill: "currentColor" }} /> {t("filters.play")}</button>
            <button type="button" className="btn icon" aria-label={`${t("filters.delete")}: ${f.name}`} onClick={() => bridge().dispatch({ type: "deleteFilter", data: { id: f.id } })}><Icon name="trash" size={14} /></button>
          </div>
        ))}
      </div>
    </div>
  );
}
