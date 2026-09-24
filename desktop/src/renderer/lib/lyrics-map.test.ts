import { describe, expect, it } from "vitest";
import type { Lyrics } from "@core/api";
import { activeLineIndex, mapLyrics } from "./lyrics-map";

const base: Lyrics = { trackId: "t", tier: "syllable", lang: "en", displayArtist: undefined, displayTitle: undefined, agents: [{ id: "v1", name: "A", side: 0 }, { id: "v2", name: "B", side: 1 }], lines: [], source: "server", offsetMs: 0 };

describe("lyrics mapping", () => {
  it("maps syllables to words, keeping joins and trailing spaces", () => {
    const l: Lyrics = { ...base, lines: [{ startMs: 1000, endMs: 3000, text: "hold the light", agent: "v1", background: false, translation: undefined, syllables: [{ text: "ho", startMs: 1000, endMs: 1200, joined: true }, { text: "ld", startMs: 1200, endMs: 1500, joined: false }, { text: "the", startMs: 1500, endMs: 2000, joined: false }, { text: "light", startMs: 2000, endMs: 3000, joined: false }] }] };
    const m = mapLyrics(l);
    expect(m.synced).toBe(true);
    expect(m.lines[0]?.words.map((w) => w.word)).toEqual(["ho", "ld ", "the ", "light"]);
    expect(m.lines[0]?.isDuet).toBe(false);
  });

  it("line tier becomes exactly one word per line: never fabricated syllables", () => {
    const l: Lyrics = { ...base, tier: "line", lines: [{ startMs: 500, endMs: 2500, text: "a line", syllables: [], agent: "v2", background: false, translation: "une ligne" }] };
    const m = mapLyrics(l);
    expect(m.lines[0]?.words).toEqual([{ word: "a line", startTime: 500, endTime: 2500 }]);
    expect(m.lines[0]?.isDuet).toBe(true);
    expect(m.lines[0]?.translatedLyric).toBe("une ligne");
  });

  it("unsynced is a static list", () => {
    const l: Lyrics = { ...base, tier: "unsynced", lines: [{ text: "one", syllables: [], background: false }, { text: "two", syllables: [], background: false }] };
    const m = mapLyrics(l);
    expect(m.synced).toBe(false);
    expect(m.lines).toHaveLength(2);
    expect(m.lines[1]!.startTime).toBeGreaterThan(m.lines[0]!.startTime);
  });

  it("applies the per-track offset and marks background lines", () => {
    const l: Lyrics = { ...base, tier: "line", offsetMs: -300, lines: [{ startMs: 1000, endMs: 2000, text: "x", syllables: [], background: false }, { startMs: 1500, endMs: 2200, text: "(bg)", syllables: [], background: true }] };
    const m = mapLyrics(l);
    expect(m.lines[0]?.startTime).toBe(700);
    expect(m.lines[1]?.isBG).toBe(true);
    expect(activeLineIndex(m.lines, 800)).toBe(0);
    expect(activeLineIndex(m.lines, 100)).toBe(-1);
  });

  it("skips synced lines without timing rather than guessing", () => {
    const l: Lyrics = { ...base, tier: "line", lines: [{ text: "no time", syllables: [], background: false }, { startMs: 10, endMs: 20, text: "ok", syllables: [], background: false }] };
    expect(mapLyrics(l).lines).toHaveLength(1);
  });
});

describe("lyrics mapping of an enhanced (syllable + background agent) document", () => {
  it("becomes AMLL words per syllable and isBG sub-lines after their main line", async () => {
    const { adaptEnhanced } = await import("../../main/fake-core/enhanced-lyrics");
    const { showcaseLyrics } = await import("../../main/fake-core/showcase-lyrics");
    const m = mapLyrics(adaptEnhanced("tr-1", showcaseLyrics()));
    expect(m.tier).toBe("syllable");
    expect(m.synced).toBe(true);
    // "title" sweeps as two words that read as one: "ti" (no trailing space) then "tle".
    expect(m.lines[0]!.words.map((w) => w.word)).toEqual(["I ", "lost ", "my ", "rank ", "and ", "ti", "tle"]);
    expect(m.lines[0]!.words[5]!.endTime).toBe(m.lines[0]!.words[6]!.startTime);
    // Background lines are AMLL sub-lines of the line before them.
    const bg = m.lines.map((l, i) => (l.isBG ? i : -1)).filter((i) => i >= 0);
    expect(bg.length).toBeGreaterThan(0);
    for (const i of bg) expect(m.lines[i - 1]!.isBG).toBe(false);
    expect(m.lines.filter((l) => l.isBG).every((l) => !l.isDuet)).toBe(true);
    // A cue-less line is one word with the line's timing.
    const plain = m.lines.find((l) => l.words[0]!.word.startsWith("We've"))!;
    expect(plain.words).toEqual([{ word: "We've seen it several times", startTime: 21_000, endTime: 23_500 }]);
    // Gap: the line ends before the next starts; nothing is stretched.
    const gap = m.lines.find((l) => l.words[0]!.word === "I " && l.startTime === 8800)!;
    expect(gap.endTime).toBe(10_500);
    expect(activeLineIndex(m.lines, 11_000)).toBe(m.lines.indexOf(gap));
  });
});
