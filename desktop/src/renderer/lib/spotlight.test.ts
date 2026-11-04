import { describe, expect, it } from "vitest";
import type { Album, AlbumResumePoint, Artist, ListeningStats, Playlist, Query, QueryResult } from "@core/api";
import { SPOTLIGHT_MAX, loadSpotlight, rng, weightedShuffle } from "./spotlight";

const DAY = 86_400_000;
const NOW = 1_800_000_000_000;

function album(i: number, artist = i % 7, extra: Partial<Album> = {}): Album {
  return { id: `al${i}`, serverId: "s", name: `Album ${i}`, artistId: `ar${artist}`, artist: `Artist ${artist}`, year: 2000 + i, genre: i % 2 ? "Post-rock" : "Jazz", songCount: 10, durationMs: 1, coverArt: `al${i}`, rating: 0, loved: false, playCount: 30 - i, created: NOW - i * DAY, lastPlayed: NOW - i * 20 * DAY, isCompilation: false, offline: "none", ...extra } as unknown as Album;
}
const ALBUMS = Array.from({ length: 30 }, (_, i) => album(i));
const ARTISTS = Array.from({ length: 7 }, (_, i) => ({ id: `ar${i}`, serverId: "s", name: `Artist ${i}`, albumCount: 4, songCount: 40, loved: false }) as unknown as Artist);
const STATS = { periodDays: 30, totalPlays: 50, totalMs: 1, topTracks: [], topAlbums: ALBUMS.slice(0, 8), topArtists: ARTISTS.slice(0, 4), playsByHour: [], playsByWeekday: [] } as unknown as ListeningStats;
const PLAYLISTS = [{ id: "pl1", serverId: "s", name: "Mix", songCount: 12, isMine: true, changed: NOW }] as unknown as Playlist[];

async function query(q: Query): Promise<QueryResult> {
  switch (q.type) {
    case "album": return { type: "albumDetail", data: ALBUMS.find((a) => a.id === q.data.id) };
    case "albums": {
      const d = q.data as { genre?: string; artist_id?: string; sort: string; page: { limit: number } };
      let list = ALBUMS.filter((a) => (!d.genre || a.genre === d.genre) && (!d.artist_id || a.artistId === d.artist_id));
      if (d.sort === "dateAdded") list = [...list].sort((a, b) => (b.created ?? 0) - (a.created ?? 0));
      if (d.sort === "random") list = [...list].reverse();
      return { type: "albums", data: { items: list.slice(0, d.page.limit), total: list.length } } as never;
    }
    case "tracks": return { type: "tracks", data: { items: ALBUMS.slice(10, 16).map((a) => ({ id: `t${a.id}`, albumId: a.id })), total: 6 } } as never;
    case "stats": return { type: "stats", data: { ...STATS, topAlbums: ALBUMS.slice(0, 20) } } as never;
    case "playlists": return { type: "playlists", data: PLAYLISTS };
    default: throw new Error(q.type);
  }
}

const resume = (id: string, at: number): AlbumResumePoint => ({ albumId: id, trackId: "t", trackIndex: 3, positionMs: 1, durationMs: 2, updatedAt: at });

describe("spotlight", () => {
  it("puts the most recent resume first, never repeats an album or artist, and skips what's playing", async () => {
    const playing = { albumId: "al0", artistId: "ar0" };
    for (let seed = 0; seed < 40; seed++) {
      const slides = await loadSpotlight({ serverId: "s", resumes: [resume("al3", 1), resume("al5", 9), resume("al0", 20)], playing, stats: STATS, seed, now: NOW, query });
      expect(slides.length).toBeGreaterThan(1);
      expect(slides.length).toBeLessThanOrEqual(SPOTLIGHT_MAX);
      expect(slides[0]).toMatchObject({ kind: "resume", album: { id: "al5" } });
      const albums = slides.flatMap((s) => (s.album ? [s.album.id] : []));
      const artists = slides.flatMap((s) => { const a = s.album?.artistId ?? s.artist?.id; return a ? [a] : []; });
      expect(new Set(albums).size).toBe(albums.length);
      expect(new Set(artists).size).toBe(artists.length);
      expect(albums).not.toContain("al0");
      expect(artists).not.toContain("ar0");
    }
  });

  it("leaves out a reason with nothing behind it", async () => {
    const slides = await loadSpotlight({ serverId: "s", resumes: [], stats: undefined, seed: 3, now: NOW, query });
    expect(slides.some((s) => s.kind === "resume" || s.kind === "onRepeat" || s.kind === "topArtist" || s.kind === "topGenre")).toBe(false);
    expect(slides.length).toBeGreaterThan(0);
  });

  it("is the same for a seed, and varies across seeds", async () => {
    const run = (seed: number) => loadSpotlight({ serverId: "s", resumes: [], stats: STATS, seed, now: NOW, query }).then((s) => s.map((x) => `${x.kind}:${x.album?.id ?? x.artist?.id ?? x.playlist?.id}`).join());
    expect(await run(7)).toBe(await run(7));
    const seen = new Set(await Promise.all(Array.from({ length: 12 }, (_, i) => run(i))));
    expect(seen.size).toBeGreaterThan(3);
  });

  it("orders heavier weights first more often", () => {
    const rnd = rng(1);
    let heavyFirst = 0;
    for (let i = 0; i < 2000; i++) if (weightedShuffle(["heavy", "light"], rnd, (x) => (x === "heavy" ? 3 : 1))[0] === "heavy") heavyFirst++;
    expect(heavyFirst).toBeGreaterThan(1300);
  });
});
