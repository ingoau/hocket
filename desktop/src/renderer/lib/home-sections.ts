// Home's sections (`home.sections`, synced): which show, in what order and how. The catalogue,
// defaults and styles are the phone's too (android/.../screens/home/HomeSections.kt); a section
// either device doesn't know stays in the stored list but isn't shown. A pinned section (Spotlight)
// always comes first: it can be switched off but never moved.
//
// Beside the built-in sections, Home lists the shelves the core supplies (`Query::HomeShelves`:
// each account's Recently added, Most played and Recently played, and Hocket's Continue
// listening), as sections named `shelf:<shelf id>`. They order and hide like the rest.
import type { Album } from "@core/api";
import { SK } from "@shared/settings-keys";

export type SectionStyle = "row" | "grid" | "list" | "tile" | "hero";
export type HomeSectionId = "spotlight" | "rediscover" | "filters" | "favouriteArtists" | "topGenre" | "moreFromArtist" | "onRepeat" | "recentQueues" | "stats";

export interface HomeSectionDef {
  /** A [HomeSectionId], or `shelf:<shelf id>`. */
  id: string;
  styles: readonly SectionStyle[];
  defaultOn: boolean;
  defaultStyle: SectionStyle;
  pinned?: boolean;
}

export const HOME_SECTIONS: readonly HomeSectionDef[] = [
  { id: "spotlight", styles: ["hero"], defaultOn: true, defaultStyle: "hero", pinned: true },
  { id: "rediscover", styles: ["row", "list"], defaultOn: true, defaultStyle: "row" },
  { id: "filters", styles: ["row", "grid", "list"], defaultOn: true, defaultStyle: "row" },
  { id: "favouriteArtists", styles: ["row", "grid"], defaultOn: true, defaultStyle: "row" },
  { id: "topGenre", styles: ["row", "grid", "list"], defaultOn: true, defaultStyle: "grid" },
  { id: "moreFromArtist", styles: ["list", "row"], defaultOn: false, defaultStyle: "list" },
  { id: "onRepeat", styles: ["row", "grid", "list"], defaultOn: false, defaultStyle: "row" },
  { id: "recentQueues", styles: ["list", "row"], defaultOn: false, defaultStyle: "list" },
  { id: "stats", styles: ["tile"], defaultOn: false, defaultStyle: "tile" },
];

/** One entry of `home.sections`; `id` may name a section this build doesn't know. */
export interface SectionPref {
  id: string;
  on: boolean;
  style?: string;
}

/** A shelf as Home's sections know it. */
export interface ShelfInfo {
  id: string;
  title: string;
  defaultOn: boolean;
  /** What it lists, when Home has it (a `HomeShelf`'s albums). */
  albums?: readonly Album[];
}

/** The section id a shelf goes by in `home.sections`. */
export const shelfSectionId = (shelfId: string): string => `shelf:${shelfId}`;
const CONTINUE_LISTENING = shelfSectionId("hocket/continueListening");

/** The shelves the core supplied last (Home sets them as they arrive). */
let shelves: readonly ShelfInfo[] = [];

export function setShelves(list: readonly ShelfInfo[]): void {
  shelves = list;
}

/** The shelf a section id names, when the core supplied it. */
export function shelfOf(id: string): ShelfInfo | undefined {
  return id.startsWith("shelf:") ? shelves.find((s) => shelfSectionId(s.id) === id) : undefined;
}

export function sectionDef(id: string): HomeSectionDef | undefined {
  const builtIn = HOME_SECTIONS.find((s) => s.id === id);
  if (builtIn) return builtIn;
  const shelf = shelfOf(id);
  return shelf ? { id, styles: ["row", "grid", "list"], defaultOn: shelf.defaultOn, defaultStyle: "row" } : undefined;
}

/** The built-in sections' defaults. */
export const DEFAULT_SECTIONS: readonly SectionPref[] = HOME_SECTIONS.map((s) => ({ id: s.id, on: s.defaultOn, style: s.defaultStyle }));

/** The default list with the shelves: Continue listening after Spotlight, the accounts' shelves after the sections that are on. */
export function defaultSections(): SectionPref[] {
  const pref = (d: HomeSectionDef): SectionPref => ({ id: d.id, on: d.defaultOn, style: d.defaultStyle });
  const shelfDefs = shelves.map((s) => sectionDef(shelfSectionId(s.id))).filter((d): d is HomeSectionDef => !!d);
  const cont = shelfDefs.filter((d) => d.id === CONTINUE_LISTENING);
  const rest = shelfDefs.filter((d) => d.id !== CONTINUE_LISTENING);
  return [
    ...HOME_SECTIONS.filter((d) => d.pinned).map(pref),
    ...cont.map(pref),
    ...HOME_SECTIONS.filter((d) => !d.pinned && d.defaultOn).map(pref),
    ...rest.map(pref),
    ...HOME_SECTIONS.filter((d) => !d.pinned && !d.defaultOn).map(pref),
  ];
}

/** The style to draw: the stored one when the section offers it, else its default. */
export function resolvedStyle(p: SectionPref): SectionStyle {
  const def = sectionDef(p.id);
  if (!def) return "row";
  return (def.styles as readonly string[]).includes(p.style ?? "") ? (p.style as SectionStyle) : def.defaultStyle;
}

/**
 * The stored list as Home and the editor use it: every stored entry in order (unknown ids kept so a
 * newer device's sections survive this one's edits), then any known section it doesn't mention yet
 * (added in a later release), off. A pinned section goes first, and one the list doesn't mention
 * yet arrives on. An empty or unreadable value is the default.
 */
export function parseSections(value: unknown): SectionPref[] {
  const stored: SectionPref[] = [];
  if (Array.isArray(value)) {
    for (const e of value) {
      if (!e || typeof e !== "object") continue;
      const o = e as Record<string, unknown>;
      if (typeof o.id !== "string" || typeof o.on !== "boolean" || stored.some((s) => s.id === o.id)) continue;
      stored.push({ id: o.id, on: o.on, ...(typeof o.style === "string" ? { style: o.style } : {}) });
    }
  }
  if (!stored.length) return defaultSections();
  const missing = HOME_SECTIONS.filter((d) => !stored.some((s) => s.id === d.id)).map((d) => ({ id: d.id, on: !!d.pinned && d.defaultOn, style: d.defaultStyle }));
  // A shelf the list doesn't mention yet (a new account's) arrives as it defaults.
  for (const s of shelves) {
    const id = shelfSectionId(s.id);
    if (!stored.some((p) => p.id === id)) missing.push({ id, on: s.defaultOn, style: "row" });
  }
  const all = [...stored, ...missing];
  return [...all.filter((p) => isPinned(p.id)), ...all.filter((p) => !isPinned(p.id))];
}

export function isPinned(id: string): boolean {
  return !!sectionDef(id)?.pinned;
}

/** What to store: the default is [] so a later release's new defaults reach it. */
export function sectionsValue(prefs: readonly SectionPref[]): SectionPref[] {
  const defaults = defaultSections();
  const isDefault = prefs.length === defaults.length && prefs.every((p, i) => p.id === defaults[i]?.id && p.on === defaults[i]?.on && p.style === defaults[i]?.style);
  return isDefault ? [] : prefs.map((p) => ({ id: p.id, on: p.on, ...(p.style ? { style: p.style } : {}) }));
}

export function updateSection(prefs: readonly SectionPref[], id: string, patch: Partial<SectionPref>): SectionPref[] {
  return prefs.map((p) => (p.id === id ? { ...p, ...patch } : p));
}

/** [id] moved [by] places among the shown sections (hidden ones keep their slots). */
export function moveShown(prefs: readonly SectionPref[], id: string, by: number): SectionPref[] {
  const shown = prefs.filter((p) => p.on && sectionDef(p.id));
  const other = shown[shown.findIndex((p) => p.id === id) + by];
  if (!other || isPinned(id) || isPinned(other.id)) return [...prefs];
  const list = prefs.filter((p) => p.id !== id);
  const moved = prefs.find((p) => p.id === id);
  if (!moved) return [...prefs];
  const to = list.findIndex((p) => p.id === other.id);
  list.splice(by > 0 ? to + 1 : to, 0, moved);
  return list;
}

/** [id] moved [by] places among the known sections, on or off (the editor's arrows). */
export function moveKnown(prefs: readonly SectionPref[], id: string, by: number): SectionPref[] {
  const known = prefs.filter((p) => sectionDef(p.id));
  const other = known[known.findIndex((p) => p.id === id) + by];
  if (!other) return [...prefs];
  return moveBefore(prefs, id, other.id, by > 0);
}

/** [id] placed next to [targetId]: before it, or after it when [after]. */
export function moveBefore(prefs: readonly SectionPref[], id: string, targetId: string, after = false): SectionPref[] {
  if (id === targetId || isPinned(id) || isPinned(targetId)) return [...prefs];
  const moved = prefs.find((p) => p.id === id);
  if (!moved) return [...prefs];
  const list = prefs.filter((p) => p.id !== id);
  const to = list.findIndex((p) => p.id === targetId);
  if (to < 0) return [...prefs];
  list.splice(after ? to + 1 : to, 0, moved);
  return list;
}

export const HOME_SECTIONS_KEY = SK.homeSections;
