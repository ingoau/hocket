import { describe, expect, it } from "vitest";
import { fitColumns, type ColumnId } from "./TrackTable";

const SONGS: ColumnId[] = ["art", "title", "artist", "album", "year", "genre", "rating", "love", "plays", "bpm", "duration", "offline"];

describe("track table columns at narrow widths (200% zoom, small windows)", () => {
  it("keeps every column when there is room (or before the table is measured)", () => {
    expect(fitColumns(SONGS, 2000)).toEqual(SONGS);
    expect(fitColumns(SONGS, 0)).toEqual(SONGS);
  });

  it("drops the least important columns first and keeps the original order", () => {
    const mid = fitColumns(SONGS, 704);
    expect(mid).not.toContain("bpm");
    expect(mid).not.toContain("genre");
    expect(mid).toEqual(SONGS.filter((c) => mid.includes(c)));
    expect(mid).toEqual(expect.arrayContaining(["title", "artist", "album", "duration", "rating"]));
  });

  it("never drops the title, and keeps title, time and artist at a phone-sized width", () => {
    expect(fitColumns(SONGS, 330)).toEqual(expect.arrayContaining(["title", "duration", "artist"]));
    expect(fitColumns(SONGS, 10)).toEqual(["title"]);
  });

  it("fits: the kept columns' minimum widths never exceed the width", () => {
    const min: Record<string, number> = { art: 34, title: 140, artist: 100, album: 100, year: 52, genre: 70, rating: 92, love: 30, plays: 52, bpm: 52, duration: 56, offline: 22 };
    for (const w of [400, 500, 600, 704, 824, 900]) {
      const kept = fitColumns(SONGS, w);
      expect(kept.reduce((s, c) => s + (min[c] ?? 0), 34), `width ${w}`).toBeLessThanOrEqual(w);
    }
  });
});
