// The FakeCore's enhanced-lyrics adapter mirrors the core's: inclusive byte
// offsets decide joins, background-agent lines become sub-lines that follow
// their main line, cue-less lines fall back to line tier, gaps are kept.
import { describe, expect, it } from "vitest";
import { adaptEnhanced, byteLength } from "./enhanced-lyrics";
import { SHOWCASE_TRACK_ID, showcaseLyrics } from "./showcase-lyrics";

describe("enhanced lyrics adapter", () => {
  const entry = showcaseLyrics();
  const l = adaptEnhanced(SHOWCASE_TRACK_ID, entry);

  it("fixture offsets are 0-based inclusive byte offsets into the UTF-8 value", () => {
    for (const cl of entry.cueLine ?? []) {
      for (const c of cl.cue ?? []) {
        expect(c.byteEnd! - c.byteStart! + 1).toBe(byteLength(c.value));
        const prefix = new TextEncoder().encode(cl.value).slice(c.byteStart!, c.byteEnd! + 1);
        expect(new TextDecoder().decode(prefix)).toBe(c.value);
      }
    }
    const first = entry.cueLine![0]!.cue![0]!;
    expect([first.value, first.byteStart, first.byteEnd]).toEqual(["I", 0, 0]);
  });

  it("keeps every cue line (more than `line`), background lines right after their main line", () => {
    expect(entry.cueLine!.length).toBeGreaterThan(entry.line.length);
    expect(l.lines).toHaveLength(entry.cueLine!.length);
    expect(l.tier).toBe("syllable");
    const bg = l.lines.map((x, i) => (x.background ? i : -1)).filter((i) => i >= 0);
    expect(bg.length).toBe(entry.cueLine!.filter((c) => c.agentId === "__nd_bg__|v1").length);
    for (const i of bg) {
      expect(l.lines[i - 1]!.background).toBe(false);
      expect(l.lines[i]!.agent).toBe("__nd_bg__|v1");
      expect(l.lines[i]!.startMs!).toBeGreaterThanOrEqual(l.lines[i - 1]!.startMs!);
    }
    expect(l.agents).toEqual([{ id: "v1", name: undefined, side: 0 }, { id: "__nd_bg__|v1", name: undefined, side: 0 }]);
  });

  it("joins syllables by byte adjacency, not whitespace guessing", () => {
    const first = l.lines[0]!;
    expect(first.syllables.map((s) => [s.text, s.joined])).toEqual([["I", false], ["lost", false], ["my", false], ["rank", false], ["and", false], ["ti", true], ["tle", false]]);
    const cafe = l.lines.find((x) => x.text.includes("café"))!;
    expect(cafe.syllables.map((s) => s.text + (s.joined ? "+" : " ")).join("")).toBe("Met you at the ca+fé on the cor+ner ");
  });

  it("a cue line without cues is a plain timed line; a gap before the next line is preserved", () => {
    const plain = l.lines.find((x) => x.text.startsWith("We've seen"))!;
    expect(plain.syllables).toEqual([]);
    expect(plain.startMs).toBe(21_000);
    expect(plain.endMs).toBe(23_500);
    const gap = l.lines.find((x) => x.text.startsWith("I wanted to progress"))!;
    const after = l.lines[l.lines.indexOf(gap) + 1]!;
    expect(gap.endMs).toBe(10_500);
    expect(after.startMs!).toBeGreaterThan(gap.endMs!);
    // The last syllable holds until the line's end, nothing is stretched across the gap.
    expect(gap.syllables[gap.syllables.length - 1]!.endMs).toBeLessThan(after.startMs!);
  });

  it("degrades honestly: no per-cue start → line tier; not synced → unsynced", () => {
    const noStarts = adaptEnhanced("t", { synced: true, line: [{ start: 10, value: "a b" }], cueLine: [{ index: 0, start: 10, end: 500, value: "a b", cue: [{ value: "a" }, { value: "b" }] }] });
    expect(noStarts.tier).toBe("line");
    expect(noStarts.lines[0]!.syllables).toEqual([]);
    const unsynced = adaptEnhanced("t", { synced: false, line: [{ value: "a" }, { value: "b" }] });
    expect(unsynced.tier).toBe("unsynced");
    expect(unsynced.lines.map((x) => x.startMs)).toEqual([undefined, undefined]);
  });

  it("applies the server offset with the LRC sign and the user offset verbatim", () => {
    const shifted = adaptEnhanced("t", { ...showcaseLyrics(), offset: 100 }, 250);
    expect(shifted.lines[0]!.startMs).toBe(1400);
    expect(shifted.lines[0]!.syllables[0]!.startMs).toBe(1400);
    expect(shifted.offsetMs).toBe(250);
  });
});
