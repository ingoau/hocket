import { describe, expect, it } from "vitest";
import { SETTINGS_INDEX, SETTINGS_SECTIONS, searchSettings } from "./settings-index";

const ids = (q: string) => searchSettings(q).map((h) => (h.kind === "setting" ? h.id : `section:${h.section}`));

describe("settings search", () => {
  it("finds nothing for a blank query", () => {
    expect(searchSettings("")).toEqual([]);
    expect(searchSettings("   ")).toEqual([]);
  });

  it("matches labels regardless of case, accents and word order", () => {
    expect(ids("GAPLESS")).toEqual(["gapless"]);
    expect(ids("cache stream")).toContain("cacheBudget");
    expect(ids("Équalizer")).toContain("eq");
  });

  it("matches the words people use that the labels don't", () => {
    expect(ids("dark mode")).toEqual(["theme"]);
    expect(ids("bass")).toContain("eq");
    expect(ids("headphones")).toContain("outputDevice");
    expect(ids("keys")).toContain("section:shortcuts");
  });

  it("finds Transitions by the words for mixing", () => {
    for (const q of ["crossfade", "transition", "automix", "mix", "dj", "blend"]) expect(ids(q)).toContain("transitions");
    expect(ids("gapless")).toEqual(["gapless"]);
  });

  it("puts a label that starts with the query first, and a section before its settings", () => {
    expect(ids("lyrics")[0]).toBe("section:lyrics");
    expect(ids("stream cache")[0]).toBe("cacheUsage");
    const audio = ids("audio");
    expect(audio[0]).toBe("section:audio");
    expect(audio).toContain("replayGain");
  });

  it("finds the one Coordinator section by its name and by claiming or joining", () => {
    expect(ids("coordinator")).toEqual(["coordinator"]);
    for (const q of ["claim", "invite", "relay", "coordinator address"]) expect(ids(q)).toContain("coordinator");
    expect(SETTINGS_INDEX.find((e) => e.id === "coordinator")?.section).toBe("connect");
  });

  it("finds offline mode, the sync intervals and server search", () => {
    expect(ids("offline mode")[0]).toBe("offlineMode");
    expect(ids("airplane")).toContain("offlineMode");
    expect(ids("refresh")).toContain("syncInterval");
    expect(ids("reconcile")).toContain("fullReconcile");
    expect(ids("search server")).toContain("searchServer");
    expect(ids("mobile data")).toContain("prefetchMobile");
  });

  it("needs every word to match", () => {
    expect(ids("gapless bass")).toEqual([]);
    expect(ids("zzzz")).toEqual([]);
  });

  it("has a unique id and a known section for every entry", () => {
    expect(new Set(SETTINGS_INDEX.map((e) => e.id)).size).toBe(SETTINGS_INDEX.length);
    for (const e of SETTINGS_INDEX) expect(SETTINGS_SECTIONS).toContain(e.section);
  });
});
