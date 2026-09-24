// Built-in filters (the core lists them first, ids `builtin:<name>`): their
// display names come from the strings table (`filter.builtin.<name>`), the
// core's `name` is the English fallback. They can't be deleted.
import type { Filter, FilterNode } from "@core/api";
import { hasString, t } from "@shared/strings";
import { AVAILABLE_OFFLINE_FILTER_ID, BUILTIN_FILTER_PREFIX } from "@shared/constants";

export function isBuiltinFilter(id: string): boolean {
  return id.startsWith(BUILTIN_FILTER_PREFIX);
}

export function filterName(f: Pick<Filter, "id" | "name">): string {
  if (!isBuiltinFilter(f.id)) return f.name;
  const key = `filter.builtin.${f.id.slice(BUILTIN_FILTER_PREFIX.length)}`;
  return hasString(key) ? t(key) : f.name;
}

/** Playable with no network: downloaded, or complete in the stream cache. */
export const AVAILABLE_OFFLINE_NODE: FilterNode = { type: "all", data: [{ type: "rule", data: { field: "availableOffline", op: "isTrue", value: { type: "bool", data: true } } }] };

/** The built-in "Available offline" filter, as the core defines it (used when the list hasn't arrived). */
export const AVAILABLE_OFFLINE_FILTER: Filter = { id: AVAILABLE_OFFLINE_FILTER_ID, name: "Available offline", root: AVAILABLE_OFFLINE_NODE, sort: "artist", descending: false, limit: undefined };
