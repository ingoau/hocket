import { beforeEach, describe, expect, it } from "vitest";
import { clearSearches, forgetSearch, recentSearches, rememberSearch } from "./recent-searches";

describe("recent searches", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    (globalThis as { localStorage?: unknown }).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
  });

  it("keeps the newest first, once each ignoring case, at most six", () => {
    for (const q of ["a", "b", "c", "d", "e", "f", "g"]) rememberSearch(q);
    expect(recentSearches()).toEqual(["g", "f", "e", "d", "c", "b"]);
    rememberSearch(" C ");
    expect(recentSearches()).toEqual(["C", "g", "f", "e", "d", "b"]);
    rememberSearch("   ");
    expect(recentSearches()).toHaveLength(6);
  });

  it("forgets one or all", () => {
    rememberSearch("one");
    rememberSearch("two");
    expect(forgetSearch("one")).toEqual(["two"]);
    expect(clearSearches()).toEqual([]);
    expect(recentSearches()).toEqual([]);
  });
});
