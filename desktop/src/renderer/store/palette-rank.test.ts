import { describe, expect, it } from "vitest";
import { rank, scoreLabel, type Rankable } from "./palette-rank";

const items: Rankable[] = [
  { id: "t1", label: "Silver Harbour", sub: "Marrow", kind: "track" },
  { id: "al1", label: "Harbour Lights", sub: "Foxglove", kind: "album" },
  { id: "ar1", label: "Halcyon Wire", kind: "artist" },
  { id: "act1", label: "Toggle shuffle", sub: "playback", kind: "action" },
  { id: "act2", label: "Search", sub: "app", kind: "action" },
  { id: "t2", label: "Shuffle Song", sub: "Sona", kind: "track" },
];

describe("palette ranking", () => {
  it("prefers prefix over word-prefix over substring over fuzzy", () => {
    expect(scoreLabel("har", "Harbour Lights")!.score).toBeGreaterThan(scoreLabel("har", "Silver Harbour")!.score);
    expect(scoreLabel("har", "Silver Harbour")!.score).toBeGreaterThan(scoreLabel("arb", "Silver Harbour")!.score);
    expect(scoreLabel("arb", "Silver Harbour")!.score).toBeGreaterThan(scoreLabel("svh", "Silver Harbour")!.score);
    expect(scoreLabel("xyz", "Silver Harbour")).toBeUndefined();
  });

  it("puts library results before actions regardless of score", () => {
    const r = rank("shuffle", items);
    expect(r.map((x) => x.item.id)).toEqual(["t2", "act1"]);
  });

  it("matches secondary text with lower weight and highlights indices", () => {
    const r = rank("marrow", items);
    expect(r[0]?.item.id).toBe("t1");
    expect(r[0]?.matches).toEqual([]);
    const h = rank("har", items)[0]!;
    expect(h.item.id).toBe("al1");
    expect(h.matches).toEqual([0, 1, 2]);
  });

  it("empty query returns everything, results first", () => {
    const r = rank("", items);
    expect(r).toHaveLength(items.length);
    expect(r[r.length - 1]?.item.kind).toBe("action");
  });
});
