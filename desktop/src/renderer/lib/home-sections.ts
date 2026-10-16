// Home's sections (`home.sections`, synced): which show, in what order and how. The catalogue,
// defaults and styles are the phone's too (android/.../screens/home/HomeSections.kt); a section
// either device doesn't know stays in the stored list but isn't shown.
import { SK } from "@shared/settings-keys";

export type SectionStyle = "row" | "grid" | "list" | "tile";
export type HomeSectionId = "rediscover" | "filters" | "favouriteArtists" | "topGenre" | "newInLibrary" | "moreFromArtist" | "onRepeat" | "recentlyPlayed" | "recentQueues" | "stats";

export interface HomeSectionDef {
  id: HomeSectionId;
  styles: readonly SectionStyle[];
  defaultOn: boolean;
  defaultStyle: SectionStyle;
}

export const HOME_SECTIONS: readonly HomeSectionDef[] = [
  { id: "rediscover", styles: ["row", "list"], defaultOn: true, defaultStyle: "row" },
  { id: "filters", styles: ["row", "grid", "list"], defaultOn: true, defaultStyle: "row" },
  { id: "favouriteArtists", styles: ["row", "grid"], defaultOn: true, defaultStyle: "row" },
  { id: "topGenre", styles: ["row", "grid", "list"], defaultOn: true, defaultStyle: "grid" },
  { id: "newInLibrary", styles: ["row", "grid", "list"], defaultOn: true, defaultStyle: "row" },
  { id: "moreFromArtist", styles: ["list", "row"], defaultOn: false, defaultStyle: "list" },
  { id: "onRepeat", styles: ["row", "grid", "list"], defaultOn: false, defaultStyle: "row" },
  { id: "recentlyPlayed", styles: ["row", "grid", "list"], defaultOn: false, defaultStyle: "row" },
  { id: "recentQueues", styles: ["list", "row"], defaultOn: false, defaultStyle: "list" },
  { id: "stats", styles: ["tile"], defaultOn: false, defaultStyle: "tile" },
];

/** One entry of `home.sections`; `id` may name a section this build doesn't know. */
export interface SectionPref {
  id: string;
  on: boolean;
  style?: string;
}

export function sectionDef(id: string): HomeSectionDef | undefined {
  return HOME_SECTIONS.find((s) => s.id === id);
}

export const DEFAULT_SECTIONS: readonly SectionPref[] = HOME_SECTIONS.map((s) => ({ id: s.id, on: s.defaultOn, style: s.defaultStyle }));

/** The style to draw: the stored one when the section offers it, else its default. */
export function resolvedStyle(p: SectionPref): SectionStyle {
  const def = sectionDef(p.id);
  if (!def) return "row";
  return (def.styles as readonly string[]).includes(p.style ?? "") ? (p.style as SectionStyle) : def.defaultStyle;
}

/**
 * The stored list as Home and the editor use it: every stored entry in order (unknown ids kept so a
 * newer device's sections survive this one's edits), then any known section it doesn't mention yet
 * (added in a later release), off. An empty or unreadable value is the default.
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
  if (!stored.length) return DEFAULT_SECTIONS.map((s) => ({ ...s }));
  const missing = HOME_SECTIONS.filter((d) => !stored.some((s) => s.id === d.id)).map((d) => ({ id: d.id, on: false, style: d.defaultStyle }));
  return [...stored, ...missing];
}

/** What to store: the default is [] so a later release's new defaults reach it. */
export function sectionsValue(prefs: readonly SectionPref[]): SectionPref[] {
  const isDefault = prefs.length === DEFAULT_SECTIONS.length && prefs.every((p, i) => p.id === DEFAULT_SECTIONS[i]?.id && p.on === DEFAULT_SECTIONS[i]?.on && p.style === DEFAULT_SECTIONS[i]?.style);
  return isDefault ? [] : prefs.map((p) => ({ id: p.id, on: p.on, ...(p.style ? { style: p.style } : {}) }));
}

export function updateSection(prefs: readonly SectionPref[], id: string, patch: Partial<SectionPref>): SectionPref[] {
  return prefs.map((p) => (p.id === id ? { ...p, ...patch } : p));
}

/** [id] moved [by] places among the shown sections (hidden ones keep their slots). */
export function moveShown(prefs: readonly SectionPref[], id: string, by: number): SectionPref[] {
  const shown = prefs.filter((p) => p.on && sectionDef(p.id));
  const other = shown[shown.findIndex((p) => p.id === id) + by];
  if (!other) return [...prefs];
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
  if (id === targetId) return [...prefs];
  const moved = prefs.find((p) => p.id === id);
  if (!moved) return [...prefs];
  const list = prefs.filter((p) => p.id !== id);
  const to = list.findIndex((p) => p.id === targetId);
  if (to < 0) return [...prefs];
  list.splice(after ? to + 1 : to, 0, moved);
  return list;
}

export const HOME_SECTIONS_KEY = SK.homeSections;
