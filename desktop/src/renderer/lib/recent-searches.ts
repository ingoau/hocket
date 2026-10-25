// Recent searches, kept on this device only (never synced to the server or
// other devices): newest first, de-duplicated ignoring case, at most six.
import { loadLocal, saveLocal } from "./local-settings";

const KEY = "recentSearches";
const MAX = 6;

export function recentSearches(): string[] {
  const list = loadLocal<unknown>(KEY, []);
  return Array.isArray(list) ? list.filter((x): x is string => typeof x === "string").slice(0, MAX) : [];
}

export function rememberSearch(query: string): string[] {
  const q = query.trim();
  if (!q) return recentSearches();
  const next = [q, ...recentSearches().filter((x) => x.toLowerCase() !== q.toLowerCase())].slice(0, MAX);
  saveLocal(KEY, next);
  return next;
}

export function forgetSearch(query: string): string[] {
  const next = recentSearches().filter((x) => x !== query);
  saveLocal(KEY, next);
  return next;
}

export function clearSearches(): string[] {
  saveLocal(KEY, []);
  return [];
}
