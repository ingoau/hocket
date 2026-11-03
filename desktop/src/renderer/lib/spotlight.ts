// Home's Spotlight: up to SPOTLIGHT_MAX big slides, each one reason to play something. A slide with
// nothing behind it (no album to resume, no plays yet) isn't made; what's playing now isn't shown,
// and no album or artist is shown twice. Which reasons come up, and which album each picks, is a
// little random: the same for a day within one run of the app, so a song changing doesn't reshuffle
// the carousel. The phone's twin is android/.../screens/home/Spotlight.kt; keep the two in step.
import type { Album, AlbumResumePoint, Artist, ListeningStats, Playlist, Query, QueryResult } from "@core/api";

export type SlideKind = "resume" | "onRepeat" | "newlyAdded" | "rediscover" | "topGenre" | "topArtist" | "playlist" | "luckyDip";

export interface SpotlightSlide {
  kind: SlideKind;
  album?: Album;
  artist?: Artist;
  playlist?: Playlist;
  resume?: AlbumResumePoint;
  /** The genre a topGenre slide is about. */
  genre?: string;
  /** Artwork for an artist without an image of their own: one of their albums'. */
  fallbackCover?: string;
}

export const SPOTLIGHT_MAX = 5;

/** How often each reason tends to come up (Resume, when there is one, is always first). */
const WEIGHTS: Record<Exclude<SlideKind, "resume">, number> = { onRepeat: 3, newlyAdded: 3, rediscover: 3, topGenre: 2, topArtist: 2.5, playlist: 1.5, luckyDip: 1 };
const KIND_ORDER: SlideKind[] = ["resume", "onRepeat", "newlyAdded", "rediscover", "topGenre", "topArtist", "playlist", "luckyDip"];

const DAY_MS = 86_400_000;
/** An album last played this long ago counts as one to rediscover. */
const REDISCOVER_AFTER_MS = 60 * DAY_MS;

const LOVED_NOT_LATELY = {
  type: "all",
  data: [
    { type: "any", data: [
      { type: "rule", data: { field: "loved", op: "isTrue", value: { type: "bool", data: true } } },
      { type: "rule", data: { field: "rating", op: "gt", value: { type: "number", data: 3 } } },
    ] },
    { type: "rule", data: { field: "lastPlayed", op: "notInTheLast", value: { type: "days", data: 60 } } },
  ],
} as const;

const sessionSeed = Math.floor(Math.random() * 2 ** 31);
export function spotlightSeed(now = Date.now()): number {
  return (sessionSeed ^ Math.floor(now / DAY_MS)) >>> 0;
}

export function slideKey(s: SpotlightSlide): string {
  return `${s.kind}:${s.album?.id ?? s.artist?.id ?? s.playlist?.id}`;
}
export function slideTitle(s: SpotlightSlide): string {
  return s.album?.name ?? s.artist?.name ?? s.playlist?.name ?? "";
}
export function slideCover(s: SpotlightSlide): string | undefined {
  return s.album?.coverArt ?? s.artist?.coverArt ?? s.playlist?.coverArt ?? s.fallbackCover;
}

/** A small seeded generator (mulberry32), so a seed gives the same picks every time. */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function shuffled<T>(items: readonly T[], rnd: () => number): T[] {
  const a = [...items];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rnd() * (i + 1));
    [a[i], a[j]] = [a[j] as T, a[i] as T];
  }
  return a;
}

/** [items] in a random order where heavier ones tend to come first (Efraimidis–Spirakis). */
export function weightedShuffle<T>(items: readonly T[], rnd: () => number, weight: (t: T) => number): T[] {
  return items.map((it) => ({ it, k: rnd() ** (1 / weight(it)) })).sort((a, b) => b.k - a.k).map((x) => x.it);
}

/** Collects slides, refusing an album or artist already shown (or playing). */
export class SpotlightPicker {
  readonly slides: SpotlightSlide[] = [];
  private albums = new Set<string>();
  private artists = new Set<string>();
  constructor(playingAlbum?: string, playingArtist?: string) {
    if (playingAlbum) this.albums.add(playingAlbum);
    if (playingArtist) this.artists.add(playingArtist);
  }
  get full(): boolean {
    return this.slides.length >= SPOTLIGHT_MAX;
  }
  canUse(albumId?: string, artistId?: string): boolean {
    return (!albumId || !this.albums.has(albumId)) && (!artistId || !this.artists.has(artistId));
  }
  offer(slide: SpotlightSlide): boolean {
    if (this.full) return false;
    const albumId = slide.album?.id;
    const artistId = slide.album?.artistId ?? slide.artist?.id;
    if (!this.canUse(albumId, artistId)) return false;
    if (slide.playlist && this.slides.some((s) => s.playlist?.id === slide.playlist?.id)) return false;
    if (albumId) this.albums.add(albumId);
    if (artistId) this.artists.add(artistId);
    this.slides.push(slide);
    return true;
  }
  offerAlbums(kind: SlideKind, candidates: readonly Album[], genre?: string): void {
    for (const a of candidates) if (this.offer({ kind, album: a, ...(genre ? { genre } : {}) })) return;
  }
}

export interface SpotlightInput {
  serverId: string;
  resumes: readonly AlbumResumePoint[];
  playing?: { albumId?: string; artistId?: string };
  /** The favourites' stats: the month's, or the year's in a quiet month. */
  stats?: ListeningStats;
  seed: number;
  now?: number;
  query: (q: Query) => Promise<QueryResult>;
}

/** The slides: Resume (the album left most recently) first, then the other reasons in a weighted shuffle. */
export async function loadSpotlight(input: SpotlightInput): Promise<SpotlightSlide[]> {
  const { serverId, resumes, playing, stats, seed, query } = input;
  const now = input.now ?? Date.now();
  const picker = new SpotlightPicker(playing?.albumId, playing?.artistId);
  const rest = weightedShuffle(Object.keys(WEIGHTS) as Exclude<SlideKind, "resume">[], rng(seed), (k) => WEIGHTS[k]);
  const ask = (q: Query) => query(q).catch(() => undefined);
  const album = async (id: string) => { const r = await ask({ type: "album", data: { id } }); return r?.type === "albumDetail" ? r.data : undefined; };
  const albums = async (sort: string, limit: number, extra: { genre?: string; artist_id?: string } = {}) => {
    const r = await ask({ type: "albums", data: { server_id: serverId, artist_id: extra.artist_id, genre: extra.genre, sort, descending: true, page: { offset: 0, limit } } } as never);
    return r?.type === "albums" ? r.data.items : [];
  };
  for (const kind of ["resume" as SlideKind, ...rest]) {
    if (picker.full) break;
    const rnd = rng(seed + KIND_ORDER.indexOf(kind) * 7919);
    switch (kind) {
      case "resume":
        for (const p of [...resumes].sort((a, b) => b.updatedAt - a.updatedAt)) {
          if (!picker.canUse(p.albumId)) continue;
          const a = await album(p.albumId);
          if (a && picker.offer({ kind, album: a, resume: p })) break;
        }
        break;
      case "onRepeat":
        picker.offerAlbums(kind, shuffled((stats?.topAlbums ?? []).slice(0, 5), rnd));
        break;
      case "newlyAdded": {
        const newest = await albums("dateAdded", 8);
        // Often the very newest; otherwise one of the last few added.
        picker.offerAlbums(kind, rnd() < 0.5 ? newest : shuffled(newest, rnd));
        break;
      }
      case "rediscover": {
        const loved = await ask({ type: "tracks", data: { server_id: serverId, page: { offset: 0, limit: 40 }, sort: "random", descending: false, filter: LOVED_NOT_LATELY as never } } as never);
        const lovedIds = shuffled([...new Set((loved?.type === "tracks" ? loved.data.items : []).map((t) => t.albumId).filter((x): x is string => !!x))], rnd).slice(0, 4);
        const year = await ask({ type: "stats", data: { period_days: 365 } });
        const yearAlbums = shuffled((year?.type === "stats" ? year.data.topAlbums : []).filter((a) => a.lastPlayed != null && now - a.lastPlayed > REDISCOVER_AFTER_MS), rnd).slice(0, 4);
        const candidates = shuffled([...yearAlbums.map((a) => ({ id: a.id, known: a as Album | undefined })), ...lovedIds.map((id) => ({ id, known: undefined }))], rnd);
        for (const c of candidates) {
          if (!picker.canUse(c.id, c.known?.artistId)) continue;
          const a = c.known ?? (await album(c.id));
          if (a && picker.offer({ kind, album: a })) break;
        }
        break;
      }
      case "topGenre": {
        const counts = new Map<string, number>();
        for (const a of stats?.topAlbums ?? []) if (a.genre?.trim()) counts.set(a.genre, (counts.get(a.genre) ?? 0) + 1);
        const genre = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
        if (genre) picker.offerAlbums(kind, shuffled(await albums("playCount", 12, { genre }), rnd), genre);
        break;
      }
      case "topArtist":
        for (const a of shuffled((stats?.topArtists ?? []).slice(0, 4), rnd)) {
          if (!picker.canUse(undefined, a.id)) continue;
          const cover = a.coverArt ? undefined : (stats?.topAlbums.find((x) => x.artistId === a.id) ?? (await albums("playCount", 1, { artist_id: a.id }))[0])?.coverArt;
          if (picker.offer({ kind, artist: a, ...(cover ? { fallbackCover: cover } : {}) })) break;
        }
        break;
      case "playlist": {
        const r = await ask({ type: "playlists", data: { server_id: serverId } });
        const lists = (r?.type === "playlists" ? r.data : []).filter((p) => p.songCount > 0).sort((a, b) => (b.changed ?? b.created ?? 0) - (a.changed ?? a.created ?? 0)).slice(0, 4);
        const p = shuffled(lists, rnd)[0];
        if (p) picker.offer({ kind, playlist: p });
        break;
      }
      case "luckyDip":
        picker.offerAlbums(kind, await albums("random", 6));
        break;
    }
  }
  return picker.slides;
}
