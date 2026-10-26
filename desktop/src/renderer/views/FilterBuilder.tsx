// Filter builder. The rules read as sentences ("Genre is “Ambient”"), grouped
// under Match all of / any of; clicking one opens its editor in a popover
// (field, condition and value as chips and fields). Sort, Descending and Limit
// below; beside them the live count, whether it stays on this device, Play with
// the other outputs (save, smart or static playlist, export .nsp) on its arrow,
// and a sample of what matches.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Filter, FilterField, FilterNode, FilterOp, FilterPreview, FilterRule, FilterValue, SortOrder } from "@core/api";
import { FilterFieldValues, SortOrderValues } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { Icon } from "../components/Icon";
import { filterName, isBuiltinFilter } from "../lib/filters";
import { ARTIST_ONLY_SORTS } from "../lib/list-sort";
import { Artwork } from "../components/Artwork";
import { Switch } from "../components/controls";
import { openItemsMenuFromButton } from "../components/ContextMenu";
import type { MenuItem } from "../store/app";
import { fmtTime } from "../lib/format";

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

/** The condition as it reads in a rule's sentence: the editor's label without its "(days)" unit hint. */
const COND_LABEL: Record<FilterOp, string> = { ...OP_LABEL, inTheLast: "in the last", notInTheLast: "not in the last" };

/**
 * Arrow keys (and Home/End) move the choice within a radio group, as a native radio group does,
 * so the group is one Tab stop: only the chosen radio (see [rovingTab]) is in the Tab order.
 */
function radioKeys(e: React.KeyboardEvent<HTMLElement>) {
  if (!["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp", "Home", "End"].includes(e.key)) return;
  const radios = [...e.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]:not([disabled])')];
  const i = radios.indexOf(document.activeElement as HTMLElement);
  if (i < 0) return;
  e.preventDefault();
  e.stopPropagation();
  const n = radios.length;
  const j = e.key === "Home" ? 0 : e.key === "End" ? n - 1 : (i + (e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : -1) + n) % n;
  radios[j]!.focus();
  radios[j]!.click();
}
/** The Tab stop of a radio group: the chosen radio, or the first when none is. */
const rovingTab = (checked: boolean, index: number, anyChecked: boolean) => (checked || (!anyChecked && index === 0) ? 0 : -1);

/**
 * After a rule leaves the tree, focus goes to the rule now in its place (or the one before it), else
 * to its group's Add rule, so the keyboard isn't dropped on the page.
 */
function focusAfterRemove(row: HTMLElement | null) {
  const list = row?.parentElement;
  const index = list ? [...list.children].indexOf(row!) : -1;
  const root = row?.closest(".page-filter-builder");
  requestAnimationFrame(() => {
    const rules = list?.isConnected ? [...list.querySelectorAll<HTMLElement>(":scope > [data-testid='rule'] > .fb-rule-main")] : [];
    const target = rules[Math.min(index, rules.length - 1)] ?? root?.querySelector<HTMLElement>("[data-testid='add-rule']");
    target?.focus();
  });
}

const FIELD_NAMES: Partial<Record<FilterField, string>> = { bpm: "BPM", bitRate: "Bit rate", filePath: "File path", fileType: "File type", hasCoverArt: "Has cover art", localPlayCount: "Plays on this device", localLastPlayed: "Last played on this device", availableOffline: "Available offline" };

/** "dateAdded" → "Date added", unless it has a name of its own. */
export function fieldLabel(f: FilterField): string {
  const named = FIELD_NAMES[f];
  if (named) return named;
  const words = f.replace(/([A-Z])/g, " $1").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

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

const LOCAL_FIELDS: FilterField[] = ["downloaded", "cached", "availableOffline", "localPlayCount", "localLastPlayed", "inPlaylist"];
/** The field chips in the rule editor, by what they describe. */
const FIELD_GROUPS: { label: Parameters<typeof t>[0]; fields: FilterField[] }[] = [
  { label: "filters.group.song", fields: ["title", "genre", "year", "duration", "comment", "lyrics", "discNumber", "trackNumber"] },
  { label: "filters.group.albumArtist", fields: ["album", "artist", "albumArtist", "compilation"] },
  { label: "filters.group.plays", fields: ["playCount", "rating", "loved", "lastPlayed", "dateAdded", "dateModified"] },
  { label: "filters.group.sound", fields: ["bpm", "energy", "key", "mood", "bitRate", "fileType"] },
  { label: "filters.group.file", fields: ["filePath", "hasCoverArt"] },
  { label: "filters.group.device", fields: LOCAL_FIELDS },
];
const UNGROUPED = FilterFieldValues.filter((f) => !FIELD_GROUPS.some((g) => g.fields.includes(f)));
if (UNGROUPED.length) FIELD_GROUPS[0]!.fields.push(...UNGROUPED);

const num = (field: FilterField, n: number) => (field === "duration" ? fmtTime(n * 1000) : String(n));

/** A rule as it reads in the list: its field, then its condition and value ("is “Ambient”", "less than 4:00"). */
export function describeCondition(rule: FilterRule): string {
  const v = rule.value;
  const op = COND_LABEL[rule.op];
  switch (v.type) {
    case "text": return `${op} “${v.data}”`;
    case "number": return `${op} ${num(rule.field, v.data)}`;
    case "days": return `${op} ${t(v.data === 1 ? "filters.daysCount.one" : "filters.daysCount.other", { count: v.data })}`;
    case "date": return `${op} ${v.data}`;
    case "range": return `${op} ${num(rule.field, v.data.low)}–${num(rule.field, v.data.high)}`;
    case "list": return `${op} ${v.data.map((x) => `“${x}”`).join(", ")}`;
    default: return op;
  }
}

/** A filter's rules in one line: "Rating is 5 and Duration less than 4:00", a group in brackets. */
export function describeNode(node: FilterNode, nested = false): string {
  if (node.type === "rule") return `${fieldLabel(node.data.field)} ${describeCondition(node.data)}`;
  const parts = node.data.map((c) => describeNode(c, true)).filter(Boolean);
  const text = parts.join(node.type === "any" ? " or " : " and ");
  return nested && parts.length > 1 ? `(${text})` : text;
}

type Path = number[];

/** `embedded`: inside the Filters page, beside the list (a section, not a page of its own). */
export function FilterBuilder({ id, embedded = false }: { id: string; embedded?: boolean }) {
  const filters = useApp((s) => s.filters);
  const navigate = useApp((s) => s.navigate);
  const serverId = useApp((s) => s.servers[0]?.id ?? "");
  const nativeApi = useApp((s) => s.servers[0]?.capabilities.nativeApi ?? false);
  const existing = filters.find((f) => f.id === id);
  const [filter, setFilter] = useState<Filter>(() => existing ?? { id: `flt-${Date.now().toString(36)}`, name: "", root: { type: "all", data: [{ type: "rule", data: defaultRule() }] }, sort: "title", descending: false, limit: undefined });
  const [preview, setPreview] = useState<FilterPreview | undefined>(undefined);
  // The rule whose editor is open, by its place in the tree.
  const [editing, setEditing] = useState<Path | null>(existing ? null : [0]);
  useEffect(() => { if (existing) setFilter(existing); }, [existing]);
  useEffect(() => {
    const h = setTimeout(() => {
      void bridge().query({ type: "filterPreview", data: { filter } }).then((r) => r.type === "preview" && setPreview(r.data));
    }, 150);
    return () => clearTimeout(h);
  }, [filter]);

  const valid = filter.name.trim().length > 0;
  const dirty = existing ? JSON.stringify(existing) !== JSON.stringify(filter) : true;
  // A new filter moves to its own address once the core has it (so the editor never remounts blank,
  // and a refused save leaves the draft in place).
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (saving && !existing && filters.some((f) => f.id === filter.id)) navigate({ view: "filter", id: filter.id }, true);
  }, [saving, existing, filters, filter.id, navigate]);
  // The filter was deleted elsewhere (another device, its menu): leave its editor rather than offer to recreate it.
  const seen = useRef(false);
  if (existing) seen.current = true;
  useEffect(() => {
    if (id !== "new" && seen.current && !existing) navigate({ view: "filters" }, true);
  }, [id, existing, navigate]);
  const save = () => {
    bridge().dispatch({ type: "saveFilter", data: { filter: { ...filter, name: filter.name.trim() } } });
    if (!existing) setSaving(true);
  };
  const play = () => bridge().dispatch({ type: "playContext", data: { args: { context: { serverId, kind: { type: "filter", data: { filter } }, label: filter.name || t("filters.builder"), sort: filter.sort, tracks: [] }, startIndex: 0, shuffle: false, saveOutgoing: true } } });
  const exportNsp = async () => {
    const path = await bridge().dialog.save({ title: t("dialog.exportNsp"), defaultPath: `${filter.name || "filter"}.nsp`, filters: [{ name: "Navidrome smart playlist", extensions: ["nsp"] }] });
    if (path) bridge().dispatch({ type: "exportNsp", data: { filter, path } });
  };
  const localOnly = preview?.capability.localOnlyFields ?? [];
  const setRoot = (root: FilterNode) => setFilter({ ...filter, root });
  const outputs: MenuItem[] = [
    { id: "filterSave", label: t("filters.save"), icon: "save", enabled: valid, run: save },
    { id: "filterSmart", label: t("filters.smartPlaylist"), icon: "playlist", enabled: valid && !!preview?.capability.serverExpressible && nativeApi, run: () => bridge().dispatch({ type: "createSmartPlaylist", data: { server_id: serverId, filter, name: filter.name } }) },
    { id: "filterStatic", label: t("filters.staticPlaylist"), icon: "playlistAdd", enabled: valid, run: () => bridge().dispatch({ type: "createStaticPlaylistFromFilter", data: { server_id: serverId, filter, name: filter.name } }) },
    { id: "filterExport", label: t("filters.exportNsp"), icon: "share", enabled: valid, run: () => void exportNsp() },
    ...(existing && !isBuiltinFilter(existing.id) ? [{ id: "filterDelete", label: t("filters.delete"), icon: "trash", enabled: true, run: () => { bridge().dispatch({ type: "deleteFilter", data: { id: existing.id } }); navigate({ view: "filters" }); } }] : []),
  ];
  const sorts = SortOrderValues.filter((s) => s !== "default" && !ARTIST_ONLY_SORTS.includes(s));
  const Title = embedded ? "h2" : "h1";
  return (
    <section className={`${embedded ? "filter-editor" : "view"} page-filter-builder`} aria-labelledby="filter-editor-title" data-testid="view-filter-builder"
      onKeyDown={(e) => { if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") { e.preventDefault(); e.stopPropagation(); if (valid) save(); } }}>
      <div className="fb-head">
        <Title id="filter-editor-title" className="sr-only">{existing ? filterName(existing) : t("filters.builder")}</Title>
        <input className="fb-name" value={isBuiltinFilter(filter.id) && !filter.name ? filterName(filter) : filter.name} placeholder={t("filters.namePlaceholder")} aria-label={t("filters.name")} onChange={(e) => setFilter({ ...filter, name: e.target.value })} data-testid="filter-name" />
        {dirty && valid && existing ? <span className="badge warn" data-testid="filter-dirty">{t("filters.unsaved")}</span> : null}
        <button type="button" className="btn tonal fb-save" onClick={save} disabled={!valid || (!!existing && !dirty)} title={t("filters.saveHint")} data-testid="filter-save">{t("filters.saveFilter")}</button>
      </div>
      <div className="fb-grid">
        <div className="fb-main">
          <div className="fb-card fb-rules">
            <GroupEditor node={filter.root} path={[]} root onChange={setRoot} editing={editing} onEdit={setEditing} />
          </div>
          <div className="fb-card fb-sort">
            <h3 id="fb-sort-label">{t("filters.sortBy")}</h3>
            <div className="fb-chips" role="radiogroup" aria-labelledby="fb-sort-label" onKeyDown={radioKeys}>
              {sorts.map((s, i) => <button key={s} type="button" role="radio" aria-checked={filter.sort === s} tabIndex={rovingTab(filter.sort === s, i, sorts.includes(filter.sort))} className={`fb-chip ${filter.sort === s ? "on" : ""}`} onClick={() => setFilter({ ...filter, sort: s as SortOrder })}>{t(`sort.${s}` as never)}</button>)}
            </div>
          </div>
          <div className="fb-rows">
            <label className="fb-row"><span className="grow">{t("filters.descending")}</span><Switch checked={filter.descending} onChange={(checked) => setFilter({ ...filter, descending: checked })} aria-label={t("filters.descending")} /></label>
            <label className="fb-row"><span className="grow">{t("filters.limit")}</span><input className="input fb-limit" type="number" min={0} placeholder={t("filters.noLimit")} value={filter.limit ?? ""} onChange={(e) => setFilter({ ...filter, limit: e.target.value ? Number(e.target.value) : undefined })} /></label>
          </div>
        </div>
        <aside className="fb-side">
          <div className="fb-card fb-result">
            <div className="fb-count" data-testid="filter-count">{preview ? t("filters.count", { count: preview.count }) : <Icon name="spinner" className="spin" size={20} />}</div>
            <div className={`fb-capability ${preview?.capability.serverExpressible ? "ok" : "local"}`} data-testid="filter-capability">
              <Icon name={preview?.capability.serverExpressible ? "cloud" : "laptop"} size={18} />
              <span>{preview?.capability.serverExpressible ? t("filters.serverExpressible") : t("filters.localBecause", { fields: localOnly.map(fieldLabel).join(", ") })}</span>
            </div>
            <div className="fb-split">
              <button type="button" className="btn primary fb-play" onClick={play} disabled={!preview?.count} data-testid="filter-play"><Icon name="play" size={20} filled /> {t("filters.play")}</button>
              <button type="button" className="btn primary fb-more" aria-haspopup="menu" aria-label={t("filters.moreOutputs")} title={t("filters.moreOutputs")} onClick={(e) => openItemsMenuFromButton(e, outputs)} data-testid="filter-outputs"><Icon name="chevronDown" size={20} /></button>
            </div>
          </div>
          <div className="filter-sample">
            <h3 className="fb-sample-title">{t("filters.sample")}</h3>
            <ul className="plain-list fb-sample">
              {(preview?.sample ?? []).map((s) => (
                <li key={s.id} className="fb-sample-row"><Artwork id={s.coverArt} size={96} className="art" /><div className="text"><div className="t1 truncate">{s.title}</div><div className="t2 truncate">{[s.artist, s.album].filter(Boolean).join(" · ")}</div></div></li>
              ))}
            </ul>
          </div>
        </aside>
      </div>
    </section>
  );
}

type EditProps = { editing: Path | null; onEdit: (p: Path | null) => void };
const samePath = (a: Path | null, b: Path) => !!a && a.length === b.length && a.every((x, i) => x === b[i]);

function GroupEditor({ node, path, onChange, root = false, onRemove, editing, onEdit }: { node: FilterNode; path: Path; onChange: (n: FilterNode) => void; root?: boolean; onRemove?: () => void } & EditProps) {
  if (node.type === "rule") return null;
  const children = node.data;
  const setChild = (i: number, n: FilterNode | null) => {
    // A nested group left without rules would match nothing (any) or everything (all): it goes too.
    if (!n && !root && children.length === 1 && onRemove) { onRemove(); return; }
    onChange(n ? { ...node, data: children.map((c, j) => (j === i ? n : c)) } : { ...node, data: children.filter((_, j) => j !== i) });
  };
  const addRule = () => { onChange({ ...node, data: [...children, { type: "rule", data: defaultRule() }] }); onEdit([...path, children.length]); };
  const addGroup = () => { onChange({ ...node, data: [...children, { type: "any", data: [{ type: "rule", data: defaultRule() }] }] }); onEdit([...path, children.length, 0]); };
  const toggle = (type: "all" | "any") => onChange({ type, data: children });
  const list = (
    <div className="fb-rule-list" role="list">
      {children.map((c, i) => c.type === "rule" ? (
        <RuleRow key={i} rule={c.data} path={[...path, i]} onChange={(r) => setChild(i, r ? { type: "rule", data: r } : null)} canRemove={children.length > 1 || !root} editing={editing} onEdit={onEdit} />
      ) : (
        <div key={i} role="listitem"><GroupEditor node={c} path={[...path, i]} onChange={(n) => setChild(i, n)} onRemove={() => { setChild(i, null); onEdit(null); }} editing={editing} onEdit={onEdit} /></div>
      ))}
    </div>
  );
  if (root) {
    return (
      <div className="rule-group root" data-testid="rule-group">
        <div className="fb-group-head">
          <div className="button-group" role="radiogroup" aria-label={t("filters.match")} onKeyDown={radioKeys}>
            <button type="button" role="radio" aria-checked={node.type === "all"} tabIndex={node.type === "all" ? 0 : -1} className={`seg ${node.type === "all" ? "on" : ""}`} onClick={() => toggle("all")}>{t("filters.matchAllOf")}</button>
            <button type="button" role="radio" aria-checked={node.type === "any"} tabIndex={node.type === "any" ? 0 : -1} className={`seg ${node.type === "any" ? "on" : ""}`} onClick={() => toggle("any")}>{t("filters.matchAnyOf")}</button>
          </div>
          <span className="grow" />
          <span className="small muted">{t("filters.clickToEdit")}</span>
        </div>
        {list}
        <div className="fb-add">
          <button type="button" className="btn primary" onClick={addRule} data-testid="add-rule"><Icon name="plus" size={18} /> {t("filters.addRule")}</button>
          <button type="button" className="btn tonal" onClick={addGroup} data-testid="add-group"><Icon name="filterAdd" size={18} /> {t("filters.addGroup")}</button>
        </div>
      </div>
    );
  }
  return (
    <div className="rule-group nested" data-testid="rule-group">
      <div className="fb-group-head">
        <button type="button" className="btn sm fb-match" onClick={() => toggle(node.type === "all" ? "any" : "all")} title={t("filters.switchMatch")}>{node.type === "all" ? t("filters.matchAllOf") : t("filters.matchAnyOf")}<Icon name="dropDown" size={18} /></button>
        <span className="grow" />
        <button type="button" className="btn text sm" onClick={addRule}>{t("filters.addRule")}</button>
        {onRemove ? <button type="button" className="btn icon sm" aria-label={t("filters.removeGroup")} title={t("filters.removeGroup")} onClick={onRemove}><Icon name="close" size={18} /></button> : null}
      </div>
      {list}
    </div>
  );
}

/** A rule as a sentence; clicking it opens its editor. */
function RuleRow({ rule, path, onChange, canRemove, editing, onEdit }: { rule: FilterRule; path: Path; onChange: (r: FilterRule | null) => void; canRemove: boolean } & EditProps) {
  const open = samePath(editing, path);
  const ref = useRef<HTMLDivElement>(null);
  return (
    <div role="listitem" ref={ref} className={`fb-rule ${open ? "open" : ""}`} data-testid="rule">
      <button type="button" className="fb-rule-main" aria-haspopup="dialog" aria-expanded={open} onClick={() => onEdit(open ? null : path)} data-testid="rule-edit">
        <span className="fb-rule-field">{fieldLabel(rule.field)}</span>
        <span className="fb-rule-cond truncate">{describeCondition(rule)}</span>
      </button>
      {canRemove ? <button type="button" className="btn icon sm fb-rule-x" aria-label={`${t("filters.removeRule")}: ${fieldLabel(rule.field)}`} onClick={() => { focusAfterRemove(ref.current); onChange(null); onEdit(null); }}><Icon name="close" size={18} /></button> : null}
      {open ? <RuleEditor anchor={ref} rule={rule} onChange={onChange} onClose={() => onEdit(null)} canRemove={canRemove} /> : null}
    </div>
  );
}

/** The rule editor: a popover under the rule, with the field, condition and value. Enter or Done closes it; Escape too. */
function RuleEditor({ anchor, rule, onChange, onClose, canRemove }: { anchor: React.RefObject<HTMLDivElement | null>; rule: FilterRule; onChange: (r: FilterRule | null) => void; onClose: () => void; canRemove: boolean }) {
  const kind = kindOf(rule.field);
  const ops = OPS[kind];
  const pop = useRef<HTMLElement>(null);
  // Viewport coordinates (the popover is portalled to the body, so no ancestor's containment offsets
  // it). Under the rule when there's room for it, else on whichever side has more; above, it hangs
  // from its bottom edge so a short one sits against the rule. Follows the rule as the page scrolls.
  const [pos, setPos] = useState<{ left: number; top?: number; bottom?: number; maxHeight: number } | null>(null);
  useLayoutEffect(() => {
    const place = () => {
      const r = anchor.current?.getBoundingClientRect();
      if (!r) return;
      const width = Math.min(440, window.innerWidth - 24);
      const left = Math.max(12, Math.min(r.left + 12, window.innerWidth - width - 12));
      const below = window.innerHeight - r.bottom - 18;
      const above = r.top - 18;
      if (below >= 360 || below >= above) setPos({ left, top: r.bottom + 6, maxHeight: Math.max(120, below) });
      else setPos({ left, bottom: window.innerHeight - r.top + 6, maxHeight: Math.max(120, above) });
    };
    place();
    // Opened together with its rule (a new filter's first rule), the rule's ref is attached only
    // after this effect, so place it again on the next frame.
    const frame = requestAnimationFrame(place);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { cancelAnimationFrame(frame); window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [anchor]);
  useEffect(() => {
    const down = (e: MouseEvent) => { if (!pop.current?.contains(e.target as Node) && !anchor.current?.contains(e.target as Node)) onClose(); };
    document.addEventListener("mousedown", down);
    return () => document.removeEventListener("mousedown", down);
  }, [anchor, onClose]);
  // Focus moves in once it is placed (hidden until then, it can't take focus).
  const placed = pos !== null;
  useEffect(() => { if (placed) pop.current?.querySelector<HTMLElement>("input, [aria-checked='true']")?.focus(); }, [placed]);
  const setField = (field: FilterField) => {
    const op = OPS[kindOf(field)].includes(rule.op) ? rule.op : (OPS[kindOf(field)][0] as FilterOp);
    onChange({ field, op, value: defaultValue(field, op) });
  };
  const setOp = (op: FilterOp) => onChange({ ...rule, op, value: rule.value.type === defaultValue(rule.field, op).type ? rule.value : defaultValue(rule.field, op) });
  const v = rule.value;
  const close = () => { onClose(); anchor.current?.querySelector<HTMLElement>(".fb-rule-main")?.focus(); };
  const done = (e: React.KeyboardEvent) => { if (e.key === "Enter") { e.preventDefault(); close(); } };
  const fieldsChecked = FIELD_GROUPS.some((g) => g.fields.includes(rule.field));
  let fieldIndex = 0;
  return createPortal(
    <section ref={pop} className="fb-pop" role="dialog" aria-label={t("filters.editRule")}
      style={pos ? { left: pos.left, top: pos.top, bottom: pos.bottom, maxHeight: pos.maxHeight, transformOrigin: pos.bottom !== undefined ? "bottom left" : undefined } : { visibility: "hidden" }}
      onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); } }} data-testid="rule-editor">
      <div className="fb-pop-head">
        <h2 className="truncate">{fieldLabel(rule.field)} {describeCondition(rule)}</h2>
        <button type="button" className="btn icon sm" aria-label={t("misc.close")} onClick={close}><Icon name="close" size={18} /></button>
      </div>
      <div className="fb-pop-label" id="fb-field-label">{t("filters.field")}</div>
      {/* One choice across the groups, so one Tab stop; each group's heading names its chips. */}
      <div className="fb-fields" role="radiogroup" aria-labelledby="fb-field-label" onKeyDown={radioKeys}>
        {FIELD_GROUPS.map((g) => (
          <div key={g.label} role="group" aria-labelledby={`fb-group-${g.label}`}>
            <div className="fb-field-group" id={`fb-group-${g.label}`}>{t(g.label)}</div>
            <div className="fb-chips">{g.fields.map((f) => <button key={f} type="button" role="radio" aria-checked={rule.field === f} tabIndex={rovingTab(rule.field === f, fieldIndex++, fieldsChecked)} className={`fb-chip ${rule.field === f ? "on" : ""}`} onClick={() => setField(f)}>{fieldLabel(f)}</button>)}</div>
          </div>
        ))}
      </div>
      <div className="fb-pop-label" id="fb-op-label">{t("filters.condition")}</div>
      <div className="fb-chips" role="radiogroup" aria-labelledby="fb-op-label" onKeyDown={radioKeys}>
        {ops.map((o, i) => <button key={o} type="button" role="radio" aria-checked={rule.op === o} tabIndex={rovingTab(rule.op === o, i, ops.includes(rule.op))} className={`fb-chip ${rule.op === o ? "on" : ""}`} onClick={() => setOp(o)}>{OP_LABEL[o]}</button>)}
      </div>
      {v.type === "text" ? <label className="fb-value"><span className="fb-pop-label">{t("filters.value")}</span><input className="input" value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "text", data: e.target.value } })} onKeyDown={done} data-testid="rule-value" /></label> : null}
      {v.type === "number" ? <label className="fb-value"><span className="fb-pop-label">{rule.field === "duration" ? t("filters.seconds") : t("filters.value")}</span><input className="input" type="number" step="any" value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "number", data: Number(e.target.value) } })} onKeyDown={done} data-testid="rule-value" /></label> : null}
      {v.type === "days" ? <label className="fb-value"><span className="fb-pop-label">{t("filters.days")}</span><input className="input" type="number" min={1} value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "days", data: Number(e.target.value) } })} onKeyDown={done} /></label> : null}
      {v.type === "date" ? <label className="fb-value"><span className="fb-pop-label">{t("filters.date")}</span><input className="input" type="date" value={v.data} onChange={(e) => onChange({ ...rule, value: { type: "date", data: e.target.value } })} onKeyDown={done} /></label> : null}
      {v.type === "range" ? (
        <div className="fb-range">
          <label className="fb-value"><span className="fb-pop-label">{t("filters.from")}</span><input className="input" type="number" step="any" value={v.data.low} onChange={(e) => onChange({ ...rule, value: { type: "range", data: { ...v.data, low: Number(e.target.value) } } })} onKeyDown={done} /></label>
          <label className="fb-value"><span className="fb-pop-label">{t("filters.to")}</span><input className="input" type="number" step="any" value={v.data.high} onChange={(e) => onChange({ ...rule, value: { type: "range", data: { ...v.data, high: Number(e.target.value) } } })} onKeyDown={done} /></label>
        </div>
      ) : null}
      {v.type === "list" ? <label className="fb-value"><span className="fb-pop-label">{t("filters.values")}</span><input className="input" value={v.data.join(", ")} onChange={(e) => onChange({ ...rule, value: { type: "list", data: e.target.value.split(",").map((s) => s.trim()).filter(Boolean) } })} onKeyDown={done} /></label> : null}
      {LOCAL_FIELDS.includes(rule.field) ? <p className="small muted fb-local">{t("filters.localNote")}</p> : null}
      <div className="fb-pop-foot">
        {canRemove ? <button type="button" className="btn text danger-text" onClick={() => { focusAfterRemove(anchor.current); onChange(null); onClose(); }}>{t("filters.removeRule")}</button> : null}
        <span className="grow" />
        <span className="kbd" aria-hidden="true">{t("key.enter")}</span>
        <button type="button" className="btn primary" onClick={close} data-testid="rule-done">{t("filters.done")}</button>
      </div>
    </section>,
    document.body,
  );
}
