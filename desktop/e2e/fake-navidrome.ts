// A small fake Navidrome (Subsonic JSON API) for end-to-end tests against the
// real core. Serves generated data shaped like the fixtures in
// crates/hocket-core/src/subsonic/fixtures: ping (0.63.x), extensions,
// getArtists, getAlbumList2, search3 (paged, empty query), playlists, genres,
// scan status, cover art (generated PNG), stream (generated WAV with Range
// support), structured lyrics, and the write endpoints (star/unstar/setRating/
// scrobble/updatePlaylist) which are recorded. /auth/login is 404.
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { coverPng } from "../src/main/png";

export interface FakeSong {
  id: string;
  title: string;
  albumId: string;
  album: string;
  artistId: string;
  artist: string;
  track: number;
  year: number;
  genre: string;
  duration: number;
  created: string;
  starred?: string;
  userRating?: number;
  playCount: number;
}

export interface FakeAlbum {
  id: string;
  name: string;
  artistId: string;
  artist: string;
  year: number;
  genre: string;
  created: string;
  songs: FakeSong[];
}

export interface FakeArtist {
  id: string;
  name: string;
  albums: FakeAlbum[];
}

export interface RecordedCall {
  endpoint: string;
  params: Record<string, string>;
}

const ENVELOPE = { status: "ok", version: "1.16.1", type: "navidrome", serverVersion: "0.63.1 (e2e)", openSubsonic: true };

export function generateLibrary(): FakeArtist[] {
  const artists: FakeArtist[] = [];
  const names = ["Marrow", "Foxglove", "Halcyon Wire", "Anna Dune"];
  const genres = ["Indie Rock", "Electronic", "Ambient", "Folk"];
  let song = 0;
  names.forEach((name, ai) => {
    const artist: FakeArtist = { id: `ar${ai + 1}`, name, albums: [] };
    for (let a = 0; a < 2; a++) {
      const albumId = `al${ai + 1}${a + 1}`;
      const album: FakeAlbum = { id: albumId, name: `${["Silver", "Night", "Paper", "Glass"][ai]} ${["Harbour", "Machines"][a]}`, artistId: artist.id, artist: name, year: 1998 + ai * 5 + a, genre: genres[ai] as string, created: `2024-0${ai + 1}-1${a}T10:00:00Z`, songs: [] };
      for (let s = 0; s < 6; s++) {
        song += 1;
        album.songs.push({ id: `s${song}`, title: `${["Hold", "Carry", "Wait", "Find", "Burn", "Drift"][s]} the ${["Light", "Signal", "Coast", "Morning", "Lantern", "River"][(s + ai) % 6]}`, albumId, album: album.name, artistId: artist.id, artist: name, track: s + 1, year: album.year, genre: album.genre, duration: 20 + s * 3, created: album.created, playCount: (song * 7) % 13, userRating: s === 2 ? 3 : undefined, starred: s === 4 ? "2024-05-01T00:00:00Z" : undefined });
      }
      artist.albums.push(album);
    }
    artists.push(artist);
  });
  return artists;
}

function songJson(s: FakeSong, ratings: Map<string, number>, stars: Set<string>) {
  const starred = stars.has(s.id) ? "2024-05-01T00:00:00Z" : undefined;
  return {
    id: s.id, parent: s.albumId, isDir: false, title: s.title, album: s.album, artist: s.artist, track: s.track, year: s.year, genre: s.genre,
    coverArt: `mf-${s.id}`, size: 44100 * 2 * s.duration + 44, contentType: "audio/wav", suffix: "wav", duration: s.duration, bitRate: 705,
    path: `${s.artist}/${s.album}/${String(s.track).padStart(2, "0")} - ${s.title}.wav`, playCount: s.playCount, discNumber: 1, created: s.created,
    starred, albumId: s.albumId, artistId: s.artistId, type: "music", userRating: ratings.get(s.id) ?? s.userRating, isVideo: false, bpm: 90 + s.track * 4,
    sortName: s.title.toLowerCase(), mediaType: "song", genres: [{ name: s.genre }], replayGain: { trackGain: -6.0, albumGain: -7.0, trackPeak: 0.9, albumPeak: 0.95 },
    channelCount: 1, samplingRate: 44100, bitDepth: 16, artists: [{ id: s.artistId, name: s.artist }], displayArtist: s.artist,
  };
}

function albumJson(a: FakeAlbum) {
  return { id: a.id, name: a.name, artist: a.artist, artistId: a.artistId, coverArt: `al-${a.id}`, songCount: a.songs.length, duration: a.songs.reduce((x, s) => x + s.duration, 0), playCount: a.songs.reduce((x, s) => x + s.playCount, 0), created: a.created, year: a.year, genre: a.genre, isCompilation: false, genres: [{ name: a.genre }], artists: [{ id: a.artistId, name: a.artist }], displayArtist: a.artist, sortName: a.name.toLowerCase() };
}

function artistJson(a: FakeArtist) {
  return { id: a.id, name: a.name, coverArt: `ar-${a.id}`, albumCount: a.albums.length, sortName: a.name.toLowerCase() };
}

/** 16-bit mono PCM WAV of a soft sine with a slow fade, `seconds` long. */
export function wavBytes(seconds: number, freq: number): Buffer {
  const rate = 44100;
  const n = Math.floor(rate * seconds);
  const buf = Buffer.alloc(44 + n * 2);
  buf.write("RIFF", 0);
  buf.writeUInt32LE(36 + n * 2, 4);
  buf.write("WAVE", 8);
  buf.write("fmt ", 12);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22);
  buf.writeUInt32LE(rate, 24);
  buf.writeUInt32LE(rate * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write("data", 36);
  buf.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) {
    const env = Math.min(1, i / 2000, (n - i) / 2000);
    buf.writeInt16LE(Math.round(Math.sin((2 * Math.PI * freq * i) / rate) * 6000 * env), 44 + i * 2);
  }
  return buf;
}

export class FakeNavidrome {
  readonly artists = generateLibrary();
  readonly songs = new Map<string, FakeSong>();
  readonly albums = new Map<string, FakeAlbum>();
  readonly calls: RecordedCall[] = [];
  readonly ratings = new Map<string, number>();
  readonly stars = new Set<string>();
  readonly playlists: { id: string; name: string; owner: string; readonly: boolean; songIds: string[]; changed: string }[] = [];
  private server: Server | undefined;
  private wavs = new Map<number, Buffer>();
  port = 0;
  /** Username the fake accepts; anything else is Subsonic error 40. */
  username = "alice";

  constructor() {
    for (const a of this.artists) for (const al of a.albums) {
      this.albums.set(al.id, al);
      for (const s of al.songs) this.songs.set(s.id, s);
    }
    const first = this.artists[0]?.albums[0]?.songs ?? [];
    this.playlists.push({ id: "pl1", name: "Evening", owner: "alice", readonly: false, songIds: first.slice(0, 4).map((s) => s.id), changed: "2024-06-10T12:00:00Z" });
    this.playlists.push({ id: "pl2", name: "Loved (smart)", owner: "bob", readonly: true, songIds: [...this.songs.values()].filter((s) => s.starred).map((s) => s.id), changed: "2024-06-11T12:00:00Z" });
  }

  get url(): string {
    return `http://127.0.0.1:${this.port}`;
  }

  async start(): Promise<string> {
    this.server = createServer((req, res) => this.handle(req, res));
    await new Promise<void>((resolve) => this.server?.listen(0, "127.0.0.1", () => resolve()));
    const addr = this.server?.address();
    this.port = typeof addr === "object" && addr ? addr.port : 0;
    return this.url;
  }

  async stop(): Promise<void> {
    await new Promise<void>((resolve) => (this.server ? this.server.close(() => resolve()) : resolve()));
  }

  callsTo(endpoint: string): RecordedCall[] {
    return this.calls.filter((c) => c.endpoint === endpoint);
  }

  private ok(res: ServerResponse, body: Record<string, unknown>): void {
    const json = JSON.stringify({ "subsonic-response": { ...ENVELOPE, ...body } });
    res.writeHead(200, { "content-type": "application/json", "content-length": Buffer.byteLength(json) });
    res.end(json);
  }

  private fail(res: ServerResponse, code: number, message: string): void {
    const json = JSON.stringify({ "subsonic-response": { ...ENVELOPE, status: "failed", error: { code, message } } });
    res.writeHead(200, { "content-type": "application/json" });
    res.end(json);
  }

  private handle(req: IncomingMessage, res: ServerResponse): void {
    const url = new URL(req.url ?? "/", this.url);
    const params: Record<string, string> = {};
    url.searchParams.forEach((v, k) => (params[k] = v));
    if (url.pathname.startsWith("/auth/") || url.pathname.startsWith("/api/")) {
      res.writeHead(404, { "content-type": "text/plain" });
      res.end("not found");
      return;
    }
    const m = url.pathname.match(/^\/rest\/([A-Za-z0-9]+)(?:\.view)?$/);
    if (!m) {
      res.writeHead(404);
      res.end();
      return;
    }
    const endpoint = m[1] as string;
    this.calls.push({ endpoint, params });
    if (params.u !== this.username) return this.fail(res, 40, "Wrong username or password");
    const songs = [...this.songs.values()];
    const sj = (s: FakeSong) => songJson(s, this.ratings, this.stars);
    switch (endpoint) {
      case "ping":
        return this.ok(res, {});
      case "getOpenSubsonicExtensions":
        return this.ok(res, { openSubsonicExtensions: [{ name: "transcodeOffset", versions: [1] }, { name: "formPost", versions: [1] }, { name: "songLyrics", versions: [1, 2] }, { name: "sonicSimilarity", versions: [1] }] });
      case "getMusicFolders":
        return this.ok(res, { musicFolders: { musicFolder: [{ id: 1, name: "Music" }] } });
      case "getArtists":
        return this.ok(res, { artists: { ignoredArticles: "The", index: [{ name: "A-Z", artist: this.artists.map(artistJson) }] } });
      case "getArtist": {
        const a = this.artists.find((x) => x.id === params.id);
        if (!a) return this.fail(res, 70, "not found");
        return this.ok(res, { artist: { ...artistJson(a), album: a.albums.map(albumJson) } });
      }
      case "getAlbum": {
        const a = this.albums.get(params.id ?? "");
        if (!a) return this.fail(res, 70, "not found");
        return this.ok(res, { album: { ...albumJson(a), song: a.songs.map(sj) } });
      }
      case "getAlbumList2": {
        const all = [...this.albums.values()].sort((a, b) => (params.type === "newest" ? b.created.localeCompare(a.created) : a.name.localeCompare(b.name)));
        const off = Number(params.offset ?? 0);
        const size = Number(params.size ?? 500);
        return this.ok(res, { albumList2: { album: all.slice(off, off + size).map(albumJson) } });
      }
      case "getSong": {
        const s = this.songs.get(params.id ?? "");
        if (!s) return this.fail(res, 70, "not found");
        return this.ok(res, { song: sj(s) });
      }
      case "getRandomSongs":
        return this.ok(res, { randomSongs: { song: songs.slice(0, Number(params.size ?? 10)).map(sj) } });
      case "getSongsByGenre":
        return this.ok(res, { songsByGenre: { song: songs.filter((s) => s.genre === params.genre).map(sj) } });
      case "getGenres": {
        const g = new Map<string, { songCount: number; albumCount: number }>();
        for (const a of this.albums.values()) {
          const e = g.get(a.genre) ?? { songCount: 0, albumCount: 0 };
          e.albumCount += 1;
          e.songCount += a.songs.length;
          g.set(a.genre, e);
        }
        return this.ok(res, { genres: { genre: [...g.entries()].map(([value, c]) => ({ value, ...c })) } });
      }
      case "getStarred2":
        return this.ok(res, { starred2: { song: songs.filter((s) => this.stars.has(s.id) || s.starred).map(sj), album: [], artist: [] } });
      case "getPlaylists":
        return this.ok(res, { playlists: { playlist: this.playlists.map((p) => this.playlistJson(p)) } });
      case "getPlaylist": {
        const p = this.playlists.find((x) => x.id === params.id);
        if (!p) return this.fail(res, 70, "not found");
        return this.ok(res, { playlist: { ...this.playlistJson(p), entry: p.songIds.map((id) => this.songs.get(id)).filter((s): s is FakeSong => !!s).map(sj) } });
      }
      case "updatePlaylist":
        return this.ok(res, {});
      case "createPlaylist": {
        const id = `pl${this.playlists.length + 1}`;
        this.playlists.push({ id, name: params.name ?? "New", owner: "alice", readonly: false, songIds: url.searchParams.getAll("songId"), changed: new Date().toISOString() });
        return this.ok(res, { playlist: this.playlistJson(this.playlists[this.playlists.length - 1]!) });
      }
      case "deletePlaylist":
        return this.ok(res, {});
      case "search3": {
        const q = (params.query ?? "").trim().toLowerCase();
        const match = (s: string) => !q || s.toLowerCase().includes(q);
        const sOff = Number(params.songOffset ?? 0);
        const sCount = Number(params.songCount ?? 20);
        const aOff = Number(params.albumOffset ?? 0);
        const aCount = Number(params.albumCount ?? 20);
        const rOff = Number(params.artistOffset ?? 0);
        const rCount = Number(params.artistCount ?? 20);
        return this.ok(res, {
          searchResult3: {
            artist: this.artists.filter((a) => match(a.name)).slice(rOff, rOff + rCount).map(artistJson),
            album: [...this.albums.values()].filter((a) => match(a.name) || match(a.artist)).slice(aOff, aOff + aCount).map(albumJson),
            song: songs.filter((s) => match(s.title) || match(s.artist) || match(s.album)).slice(sOff, sOff + sCount).map(sj),
          },
        });
      }
      case "getLyricsBySongId": {
        const s = this.songs.get(params.id ?? "");
        if (!s) return this.ok(res, { lyricsList: { structuredLyrics: [] } });
        const words = s.title.split(" ");
        const line = (i: number) => ({ start: 1000 + i * 3000, value: `${words.join(" ")} (${i + 1})` });
        return this.ok(res, { lyricsList: { structuredLyrics: [{ displayArtist: s.artist, displayTitle: s.title, lang: "eng", synced: true, offset: 0, kind: "main", line: [0, 1, 2, 3, 4].map(line) }] } });
      }
      case "getSimilarSongs2":
        return this.ok(res, { similarSongs2: { song: songs.filter((s) => s.id !== params.id).slice(0, Number(params.count ?? 10)).map(sj) } });
      case "getTopSongs":
        return this.ok(res, { topSongs: { song: songs.filter((s) => s.artist === params.artist).slice(0, Number(params.count ?? 10)).map(sj) } });
      case "getArtistInfo2":
        return this.ok(res, { artistInfo2: { biography: "An artist.", similarArtist: [] } });
      case "getSonicSimilarTracks":
        return this.ok(res, { sonicSimilarTracks: { song: songs.filter((s) => s.id !== params.id).slice(0, Number(params.count ?? 10)).map((s) => ({ ...sj(s), similarity: 0.8 })) } });
      case "star":
        for (const id of url.searchParams.getAll("id")) this.stars.add(id);
        return this.ok(res, {});
      case "unstar":
        for (const id of url.searchParams.getAll("id")) this.stars.delete(id);
        return this.ok(res, {});
      case "setRating":
        if (params.id) this.ratings.set(params.id, Number(params.rating ?? 0));
        return this.ok(res, {});
      case "scrobble":
        return this.ok(res, {});
      case "getScanStatus":
        return this.ok(res, { scanStatus: { scanning: false, count: this.songs.size, folderCount: 1, lastScan: "2024-06-12T01:02:03Z" } });
      case "getPlayQueue":
        return this.ok(res, {});
      case "getNowPlaying":
        return this.ok(res, { nowPlaying: { entry: [] } });
      case "getCoverArt": {
        const id = params.id ?? "x";
        const size = Math.min(1000, Number(params.size ?? 300));
        let h = 0;
        for (const c of id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
        const png = coverPng(h, Math.min(size, 128));
        res.writeHead(200, { "content-type": "image/png", "content-length": png.length });
        res.end(png);
        return;
      }
      case "stream":
      case "download": {
        const s = this.songs.get(params.id ?? "");
        if (!s) {
          res.writeHead(404);
          res.end();
          return;
        }
        const wav = this.wav(s.duration, 220 + s.track * 55);
        const range = req.headers.range;
        const m2 = range?.match(/bytes=(\d+)-(\d*)/);
        const start = m2 ? Number(m2[1]) : 0;
        const end = m2 && m2[2] ? Math.min(Number(m2[2]), wav.length - 1) : wav.length - 1;
        if (start >= wav.length) {
          res.writeHead(416, { "content-range": `bytes */${wav.length}` });
          res.end();
          return;
        }
        const body = wav.subarray(start, end + 1);
        res.writeHead(m2 ? 206 : 200, { "content-type": "audio/wav", "content-length": body.length, "accept-ranges": "bytes", ...(m2 ? { "content-range": `bytes ${start}-${end}/${wav.length}` } : {}) });
        res.end(body);
        return;
      }
      default:
        return this.fail(res, 0, `unhandled endpoint ${endpoint}`);
    }
  }

  private playlistJson(p: (typeof this.playlists)[number]) {
    return { id: p.id, name: p.name, comment: "", owner: p.owner, public: false, songCount: p.songIds.length, duration: p.songIds.reduce((x, id) => x + (this.songs.get(id)?.duration ?? 0), 0), created: "2024-03-01T00:00:00Z", changed: p.changed, coverArt: `pl-${p.id}`, readonly: p.readonly };
  }

  private wav(seconds: number, freq: number): Buffer {
    const key = seconds * 10000 + freq;
    let b = this.wavs.get(key);
    if (!b) {
      b = wavBytes(seconds, freq);
      this.wavs.set(key, b);
    }
    return b;
  }
}
