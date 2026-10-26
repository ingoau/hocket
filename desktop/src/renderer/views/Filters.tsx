import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { EmptyState } from "../components/EmptyState";
import { Icon } from "../components/Icon";
import { filterName, isBuiltinFilter } from "../lib/filters";
import { openItemsMenu, openItemsMenuFromButton } from "../components/ContextMenu";
import { filterMenuItems } from "../lib/pin-menus";
import { FilterBuilder, describeNode } from "./FilterBuilder";

/** A steady hue per filter, for its avatar. */
function hue(id: string): number {
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/** Saved filters beside the editor of the selected one (the first, until one is picked; "new" for a new one). */
export function Filters({ selected }: { selected?: string }) {
  const filters = useApp((s) => s.filters);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const play = (id: string) => {
    const f = filters.find((x) => x.id === id);
    if (f) bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "filter", data: { filter: f } }, label: filterName(f), sort: f.sort, tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  };
  const current = selected ?? filters[0]?.id;
  return (
    <div className="view page-filters" data-testid="view-filters">
      <div className="filters-split">
        <div className="filters-col">
        <div className="view-header filters-head">
          <h1 className="brand-type">{t("filters.title")}</h1>
          <div className="actions"><button type="button" className="btn primary" onClick={() => navigate({ view: "filter", id: "new" })} data-testid="new-filter"><Icon name="plus" size={20} /> {t("filters.new")}</button></div>
        </div>
        <div className="filters-list" role={filters.length ? "list" : undefined} aria-label={filters.length ? t("filters.title") : undefined}>
        {!filters.length ? <EmptyState message={t("filters.empty")} icon="filter" action={<button type="button" className="btn" onClick={() => navigate({ view: "filter", id: "new" })}>{t("filters.new")}</button>} /> : null}
        {filters.map((f) => {
          const items = [...filterMenuItems(f, serverId), ...(isBuiltinFilter(f.id) ? [] : [{ id: "deleteFilter", label: t("filters.delete"), icon: "trash", enabled: true, run: () => bridge().dispatch({ type: "deleteFilter", data: { id: f.id } }) }])];
          return (
          <div key={f.id} role="listitem" className={`filter-row ${f.id === current ? "selected" : ""}`} aria-current={f.id === current || undefined} data-testid="filter-row" onContextMenu={(e) => openItemsMenu(e, items)}>
            <button type="button" className="filter-open" onClick={() => navigate({ view: "filter", id: f.id })}>
              <span className="filter-avatar" aria-hidden="true" style={{ "--hue": hue(f.id) } as React.CSSProperties} />
              <span className="filter-text">
                <span className="filter-name"><span className="truncate">{filterName(f)}</span>{isBuiltinFilter(f.id) ? <span className="badge">{t("filters.builtin")}</span> : null}</span>
                <span className="filter-desc">{describeNode(f.root) || t("sort.label", { sort: t(`sort.${f.sort}` as never) })}</span>
              </span>
            </button>
            <button type="button" className="btn icon sm filter-hover" aria-label={`${t("filters.play")}: ${filterName(f)}`} title={t("filters.play")} onClick={() => play(f.id)}><Icon name="play" size={18} filled /></button>
            <button type="button" className="btn icon sm filter-hover" aria-label={`${t("misc.more")}: ${filterName(f)}`} aria-haspopup="menu" onClick={(e) => openItemsMenuFromButton(e, items)} data-testid="filter-more"><Icon name="more" size={18} /></button>
          </div>
          );
        })}
        </div>
        </div>
        {current ? <div className="filters-detail"><FilterBuilder key={current} id={current} embedded /></div> : null}
      </div>
    </div>
  );
}
