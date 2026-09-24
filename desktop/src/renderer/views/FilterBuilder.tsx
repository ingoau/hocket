// Filter builder: rules tree, live count, server-expressibility indicator and
// the four outputs (save / play / smart playlist / static playlist / export .nsp).
import { useEffect, useMemo, useState } from "react";
import type { Filter, FilterField, FilterNode, FilterOp, FilterPreview, FilterRule, FilterValue, SortOrder } from "@core/api";
import { FilterFieldValues, FilterOpValues, SortOrderValues } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { Icon } from "../components/Icon";
import { filterName, isBuiltinFilter } from "../lib/filters";
import { Artwork } from "../components/Artwork";
import { Select, Switch } from "../components/controls";

const TEXT_FIELDS: FilterField[] = ["title", "album", "artist", "albumArtist", "genre", "filePath", "fileType", "comment", "lyrics", "key", "mood"];
const NUM_FIELDS: FilterField[] = ["year", "playCount", "rating", "duration", "bitRate", "discNumber", "trackNumber", "bpm", "energy", "localPlayCount"];
const DATE_FIELDS: FilterField[] = ["dateAdded", "dateModified", "lastPlayed", "localLastPlayed"];
const BOOL_FIELDS: FilterField[] = ["loved", "hasCoverArt", "compilation", "downloaded", "cached", "availableOffline"];
const OPS: Record<"text" | "num" | "date" | "bool" | "list", FilterOp[]> = {
  text: ["is", "isNot", "contains", "notContains", "startsWith", "endsWith"],
  num: ["is", "isNot", "gt", "lt", "inTheRange"],
  date: ["before", "after", "inTheLast", "notInTheLast", "inTheRange"],
  bool: ["isTrue", "isFalse"],
  list: ["is", "isNot"],
};
const OP_LABEL: Record<FilterOp, string> = { is: "is", isNot: "is not", contains: "contains", notContains: "doesn't contain", startsWith: "starts with", endsWith: "ends with", gt: "greater than", lt: "less than", inTheRange: "in the range", before: "before", after: "after", inTheLast: "in the last (days)", notInTheLast: "not in the last (days)", isTrue: "is true", isFalse: "is false" };

function kindOf(f: FilterField): keyof typeof OPS {
  if (TEXT_FIELDS.includes(f)) return "text";
  if (NUM_FIELDS.includes(f)) return "num";
  if (DATE_FIELDS.includes(f)) return "date";
  if (BOOL_FIELDS.includes(f)) return "bool";
  return "list";
}

function defaultRule(): FilterRule {
  return { field: "artist", op: "contains", value: { type: "text", data: "" } };
}

function defaultValue(field: FilterField, op: FilterOp): FilterValue {
  const k = kindOf(field);
  if (op === "inTheRange") return { type: "range", data: { low: 0, high: 10 } };
  if (op === "inTheLast" || op === "notInTheLast") return { type: "days", data: 30 };
  if (k === "num") return { type: "number", data: 0 };
  if (k === "date") return { type: "date", data: new Date().toISOString().slice(0, 10) };
  if (k === "bool") return { type: "bool", data: true };
  if (k === "list") return { type: "list", data: [] };
  return { type: "text", data: "" };
}

export function FilterBuilder({ id }: { id: string }) {
  const filters = useApp((s) => s.filters);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const nativeApi = useApp((s) => s.servers[0]?.capabilities.nativeApi ?? false);
  const exported = useApp((s) => s.exported);
  const existing = filters.find((f) => f.id === id);
  const [filter, setFilter] = useState<Filter>(() => existing ?? { id: `flt-${Date.now().toString(36)}`, name: "", root: { type: "all", data: [{ type: "rule", data: defaultRule() }] }, sort: "title", descending: false, limit: undefined });
  const [preview, setPreview] = useState<FilterPreview | undefined>(undefined);
  useEffect(() => { if (existing) setFilter(existing); }, [existing]);
  useEffect(() => {
    const h = setTimeout(() => {
      void bridge().query({ type: "filterPreview", data: { filter } }).then((r) => r.type === "preview" && setPreview(r.data));
    }, 150);
    return () => clearTimeout(h);
  }, [filter]);
  useEffect(() => {
    if (exported?.kind === "nsp" && Date.now() - exported.at < 2000 && exported.path) {
      useApp.getState().applyEvent({ type: "toast", data: { toast: { id: `nsp-${exported.at}`, message: t("filters.exported", { path: exported.path }), actionLabel: undefined, actionCommand: undefined, durationMs: 4000 } } });
    }
  }, [exported]);

  const valid = filter.name.trim().length > 0;
  const save = () => bridge().dispatch({ type: "saveFilter", data: { filter: { ...filter, name: filter.name.trim() } } });
  const play = () => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "filter", data: { filter } }, label: filter.name || t("filters.builder"), sort: filter.sort, tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const exportNsp = async () => {
    const path = await bridge().dialog.save({ title: t("dialog.exportNsp"), defaultPath: `${filter.name || "filter"}.nsp`, filters: [{ name: "Navidrome smart playlist", extensions: ["nsp"] }] });
    if (path) bridge().dispatch({ type: "exportNsp", data: { filter, path } });
  };
  const localOnly = preview?.capability.localOnlyFields ?? [];
  return (
    <div className="view page-filter-builder" data-testid="view-filter-builder">
      <div className="view-header">
        <h1>{existing ? filterName(existing) : t("filters.builder")}</h1>
        <div className="actions">
          <span className="muted" data-testid="filter-count">{preview ? t("filters.count", { count: preview.count }) : <Icon name="spinner" className="spin" size={13} />}</span>
          <button type="button" className="btn tonal" onClick={play} disabled={!preview?.count}><Icon name="play" size={13} filled /> {t("filters.play")}</button>
          <button type="button" className="btn primary" onClick={save} disabled={!valid} data-testid="filter-save">{t("filters.save")}</button>
        </div>
      </div>
      <div className="view-body filter-builder">
        <div className="stack">
          <label className="stack" style={{ gap: 4 }}><span className="small muted">{t("filters.name")}</span><input className="input" value={filter.name} onChange={(e) => setFilter({ ...filter, name: e.target.value })} data-testid="filter-name" /></label>
          <div className={`badge capability-chip ${preview?.capability.serverExpressible ? "ok" : "warn"}`} style={{ alignSelf: "flex-start" }} data-testid="filter-capability">
            <Icon name={preview?.capability.serverExpressible ? "cloud" : "warn"} size={16} />{preview?.capability.serverExpressible ? t("filters.serverExpressible") : t("filters.localOnly", { fields: localOnly.join(", ") })}
          </div>
          <div className="rules"><GroupEditor node={filter.root} onChange={(root) => setFilter({ ...filter, root })} root /></div>
          <div className="row">
            <label className="row"><span className="small muted">{t("filters.sort")}</span>
              <Select value={filter.sort} onChange={(e) => setFilter({ ...filter, sort: e.target.value as SortOrder })}>{SortOrderValues.map((s) => <option key={s} value={s}>{t(`sort.${s}` as never)}</option>)}</Select>
            </label>
            <label className="switch"><Switch checked={filter.descending} onChange={(checked) => setFilter({ ...filter, descending: checked })} /> {t("filters.descending")}</label>
            <label className="row"><span className="small muted">{t("filters.limit")}</span><input className="input" type="number" min={0} style={{ width: 80 }} value={filter.limit ?? ""} onChange={(e) => setFilter({ ...filter, limit: e.target.value ? Number(e.target.value) : undefined })} /></label>
          </div>
          <div className="row" style={{ flexWrap: "wrap" }}>
            <button type="button" className="btn" disabled={!valid || !preview?.capability.serverExpressible || !nativeApi} title={!nativeApi ? t("filters.smartUnavailable") : undefined} onClick={() => bridge().dispatch({ type: "createSmartPlaylist", data: { server_id: serverId, filter, name: filter.name } })}>{t("filters.smartPlaylist")}</button>
            <button type="button" className="btn" disabled={!valid} onClick={() => bridge().dispatch({ type: "createStaticPlaylistFromFilter", data: { server_id: serverId, filter, name: filter.name } })}>{t("filters.staticPlaylist")}</button>
            <button type="button" className="btn" disabled={!valid} onClick={() => void exportNsp()} data-testid="filter-export">{t("filters.exportNsp")}</button>
            {existing && !isBuiltinFilter(existing.id) ? <button type="button" className="btn danger" onClick={() => { bridge().dispatch({ type: "deleteFilter", data: { id: existing.id } }); navigate({ view: "filters" }); }}>{t("filters.delete")}</button> : null}
          </div>
        </div>
        <div className="filter-sample">
          <div className="card-title filter-sample-title">{t("filters.sample")}</div>
          {(preview?.sample ?? []).map((s) => (
            <div key={s.id} className="qrow" style={{ height: 36 }}><Artwork id={s.coverArt} size={64} className="art" /><div className="text"><div className="t1">{s.title}</div><div className="t2">{s.artist}</div></div></div>
          ))}
        </div>
      </div>
    </div>
  );
}

function GroupEditor({ node, onChange, root = false }: { node: FilterNode; onChange: (n: FilterNode) => void; root?: boolean }) {
  if (node.type === "rule") return null;
  const children = node.data;
  const setChild = (i: number, n: FilterNode) => onChange({ ...node, data: children.map((c, j) => (j === i ? n : c)) });
  const remove = (i: number) => onChange({ ...node, data: children.filter((_, j) => j !== i) });
  return (
    <div className="rule-group" data-testid="rule-group">
      <div className="group-head">
        <Select value={node.type} onChange={(e) => onChange({ type: e.target.value as "all" | "any", data: children })} aria-label="Match">
          <option value="all">{t("filters.matchAll")}</option>
          <option value="any">{t("filters.matchAny")}</option>
        </Select>
        <span className="grow" />
        <button type="button" className="btn sm" onClick={() => onChange({ ...node, data: [...children, { type: "rule", data: defaultRule() }] })} data-testid="add-rule"><Icon name="plus" size={12} /> {t("filters.addRule")}</button>
        <button type="button" className="btn sm" onClick={() => onChange({ ...node, data: [...children, { type: "all", data: [{ type: "rule", data: defaultRule() }] }] })}><Icon name="plus" size={12} /> {t("filters.addGroup")}</button>
      </div>
      {children.map((c, i) => c.type === "rule" ? (
        <RuleEditor key={i} rule={c.data} onChange={(r) => setChild(i, { type: "rule", data: r })} onRemove={children.length > 1 || !root ? () => remove(i) : undefined} />
      ) : (
        <div key={i} className="row" style={{ alignItems: "flex-start" }}>
          <div className="grow"><GroupEditor node={c} onChange={(n) => setChild(i, n)} /></div>
          <button type="button" className="btn icon sm" aria-label={t("filters.removeRule")} onClick={() => remove(i)}><Icon name="close" size={12} /></button>
        </div>
      ))}
    </div>
  );
}

function RuleEditor({ rule, onChange, onRemove }: { rule: FilterRule; onChange: (r: FilterRule) => void; onRemove?: () => void }) {
  const kind = kindOf(rule.field);
  const ops = OPS[kind];
  const setField = (field: FilterField) => {
    const op = OPS[kindOf(field)].includes(rule.op) ? rule.op : (OPS[kindOf(field)][0] as FilterOp);
    onChange({ field, op, value: defaultValue(field, op) });
  };
  const setOp = (op: FilterOp) => onChange({ ...rule, op, value: rule.value.type === defaultValue(rule.field, op).type ? rule.value : defaultValue(rule.field, op) });
  const v = rule.value;
  const fields = useMemo(() => [...FilterFieldValues], []);
  return (
    <div className="rule" data-testid="rule">
      <Select value={rule.field} onChange={(e) => setField(e.target.value as FilterField)} aria-label="Field">{fields.map((f) => <option key={f} value={f}>{f}{["downloaded", "cached", "availableOffline", "localPlayCount", "localLastPlayed", "inPlaylist"].includes(f) ? " (local)" : ""}</option>)}</Select>
      <Select value={rule.op} onChange={(e) => setOp(e.target.value as FilterOp)} aria-label="Operator">{ops.map((o) => <option key={o} value={o}>{OP_LABEL[o]}</option>)}</Select>
      {v.type === "text" ? <input className="input" value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "text", data: e.target.value } })} aria-label="Value" data-testid="rule-value" /> : null}
      {v.type === "number" ? <input className="input" type="number" step="any" value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "number", data: Number(e.target.value) } })} aria-label="Value" style={{ width: 100 }} /> : null}
      {v.type === "days" ? <input className="input" type="number" min={1} value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "days", data: Number(e.target.value) } })} aria-label="Days" style={{ width: 80 }} /> : null}
      {v.type === "date" ? <input className="input" type="date" value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "date", data: e.target.value } })} aria-label="Date" /> : null}
      {v.type === "range" ? <><input className="input" type="number" step="any" value={v.data.low} onChange={(e) => onChange({ ...rule, value: { type: "range", data: { ...v.data, low: Number(e.target.value) } } })} style={{ width: 80 }} aria-label="Low" /><span className="muted">–</span><input className="input" type="number" step="any" value={v.data.high} onChange={(e) => onChange({ ...rule, value: { type: "range", data: { ...v.data, high: Number(e.target.value) } } })} style={{ width: 80 }} aria-label="High" /></> : null}
      {v.type === "list" ? <input className="input" value={v.data.join(", ")} onChange={(e) => onChange({ ...rule, value: { type: "list", data: e.target.value.split(",").map((s) => s.trim()).filter(Boolean) } })} aria-label="Values" /> : null}
      {onRemove ? <button type="button" className="btn icon sm" aria-label={t("filters.removeRule")} onClick={onRemove}><Icon name="close" size={12} /></button> : null}
      {FilterOpValues.length ? null : null}
    </div>
  );
}
