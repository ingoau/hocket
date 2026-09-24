// A plausible library of a few thousand tracks, deterministic from a seed.
import type { Album, Artist, Genre, Playlist, Track, TrackSummary } from "@core/api";
import { Rng, hash32 } from "./random";
import { SHOWCASE_DURATION_MS, SHOWCASE_TITLE, SHOWCASE_TRACK_ID } from "./showcase-lyrics";

const FIRST = ["Silver", "Night", "Paper", "Glass", "Velvet", "Iron", "Cold", "Wild", "Quiet", "Electric", "Hollow", "Golden", "Broken", "Blue", "Neon", "Slow", "Last", "Empty", "Little", "Northern"];
const SECOND = ["Harbour", "Machines", "Gardens", "Signal", "Wolves", "Lanterns", "Cities", "Mirrors", "Orchards", "Rivers", "Stations", "Voices", "Fires", "Horizons", "Bridges", "Sisters", "Engines", "Fields", "Lights", "Coast"];
const ARTIST_FIRST = ["The", "", "", "", "Saint", "Little", "Big"];
const ARTIST_NAMES = ["Marrow", "Foxglove", "Ceramic Hearts", "Anna Dune", "Low Tide", "Kestrel", "Odd Meridian", "Halcyon Wire", "Tamsin Vale", "Bellweather", "Idle Static", "June Carver", "Slowdive Club", "Oriel", "Parallax", "Nova Sable", "The Ambers", "Mira Holt", "Stray Current", "Wren & Co", "Hollis", "Larkspur", "Deep Sea Radio", "Faro", "Gilded Ash", "Ines Marlow", "Copper Choir", "Winter Post", "Aldous Bay", "Lumen", "Bright Fold", "Rust Valley", "Cal Rivers", "Sona", "The Quiet Room", "Aster Lane", "Corvid", "Ellery Park", "Ninefold", "Pale Motor"];
const GENRES = ["Indie Rock", "Electronic", "Ambient", "Jazz", "Folk", "Hip-Hop", "Classical", "Pop", "Post-Rock", "Soul", "Techno", "Shoegaze"];
const MOODS = ["calm", "bright", "melancholic", "energetic", "dreamy", "tense"];
const KEYS = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"].flatMap((k) => [`${k} major`, `${k} minor`]);

export interface FakeLibrary {
  serverId: string;
  tracks: Track[];
  albums: Album[];
  artists: Artist[];
  genres: Genre[];
  playlists: { playlist: Playlist; trackIds: string[] }[];
  tracksById: Map<string, Track>;
  albumTracks: Map<string, string[]>;
  artistAlbums: Map<string, string[]>;
}

export function summary(t: Track): TrackSummary {
  return {
    id: t.id,
    serverId: t.serverId,
    title: t.title,
    artist: t.artist,
    album: t.album,
    albumId: t.albumId,
    artistId: t.artistId,
    durationMs: t.durationMs,
    coverArt: t.coverArt,
    rating: t.rating,
    loved: t.loved,
    offline: t.offline,
  };
}

function titleFor(rng: Rng): string {
  const forms = [
    () => `${rng.pick(FIRST)} ${rng.pick(SECOND)}`,
    () => `${rng.pick(SECOND)}`,
    () => `${rng.pick(FIRST)} ${rng.pick(SECOND)} (${rng.pick(["Reprise", "Live", "Edit", "Demo", "Pt. 2"])})`,
    () => `${rng.pick(["I", "We", "You", "They"])} ${rng.pick(["Were", "Are", "Became", "Left"])} ${rng.pick(SECOND)}`,
  ];
  return rng.pick(forms)();
}

export function generateLibrary(serverId: string, seed = 7, trackTarget = 3200): FakeLibrary {
  const rng = new Rng(seed);
  const now = Date.now();
  const year0 = 1972;
  const artists: Artist[] = ARTIST_NAMES.map((n, i) => {
    const prefix = rng.pick(ARTIST_FIRST);
    const name = prefix && !n.startsWith("The") ? `${prefix} ${n}` : n;
    return {
      id: `ar-${i + 1}`,
      serverId,
      name,
      albumCount: 0,
      songCount: 0,
      coverArt: `ar-${i + 1}`,
      loved: rng.chance(0.1),
      biography: rng.chance(0.6) ? `${name} formed in ${year0 + rng.int(10, 48)} and has released music across ${rng.pick(GENRES).toLowerCase()} and beyond.` : undefined,
    } as Artist;
  });
  const albums: Album[] = [];
  const tracks: Track[] = [];
  const albumTracks = new Map<string, string[]>();
  const artistAlbums = new Map<string, string[]>();
  let trackNo = 0;
  let albumNo = 0;
  while (tracks.length < trackTarget) {
    const artist = rng.pick(artists);
    albumNo += 1;
    const albumId = `al-${albumNo}`;
    const year = year0 + rng.int(0, 53);
    const genre = rng.pick(GENRES);
    const songCount = rng.int(6, 16);
    const created = now - rng.int(0, 900) * 86_400_000;
    const albumRating = rng.chance(0.3) ? rng.int(1, 5) : 0;
    const album: Album = {
      id: albumId,
      serverId,
      name: titleFor(rng),
      artistId: artist.id,
      artist: artist.name,
      year,
      genre,
      songCount,
      durationMs: 0,
      coverArt: albumId,
      rating: albumRating,
      loved: rng.chance(0.08),
      playCount: 0,
      created,
      lastPlayed: undefined,
      isCompilation: rng.chance(0.05),
      musicBrainzId: undefined,
      replayGain: { albumGainDb: -(rng.int(30, 110) / 10), albumPeak: 0.98 },
      offline: rng.chance(0.06) ? "downloaded" : "none",
    };
    const ids: string[] = [];
    const discs = songCount > 12 && rng.chance(0.3) ? 2 : 1;
    for (let i = 0; i < songCount; i++) {
      trackNo += 1;
      const id = `tr-${trackNo}`;
      const durationMs = rng.int(95, 420) * 1000 + rng.int(0, 999);
      const plays = rng.chance(0.55) ? rng.int(0, 60) : 0;
      const track: Track = {
        id,
        serverId,
        title: titleFor(rng),
        albumId,
        album: album.name,
        artistId: artist.id,
        artist: artist.name,
        albumArtist: artist.name,
        trackNumber: discs === 2 ? (i % Math.ceil(songCount / 2)) + 1 : i + 1,
        discNumber: discs === 2 ? (i < Math.ceil(songCount / 2) ? 1 : 2) : 1,
        year,
        genre,
        durationMs,
        bitRate: rng.pick([320, 256, 192, 1411, 900]),
        sampleRate: rng.pick([44100, 48000]),
        bitDepth: rng.pick([16, 24]),
        channels: 2,
        suffix: rng.pick(["flac", "mp3", "opus", "m4a"]),
        contentType: "audio/flac",
        sizeBytes: durationMs * rng.int(20, 170),
        path: `${artist.name}/${album.name}/${String(i + 1).padStart(2, "0")} - track.flac`,
        coverArt: albumId,
        rating: rng.chance(0.25) ? rng.int(1, 5) : 0,
        loved: rng.chance(0.07),
        playCount: plays,
        lastPlayed: plays > 0 ? now - rng.int(1, 400) * 3_600_000 : undefined,
        created,
        replayGain: { trackGainDb: -(rng.int(20, 120) / 10), trackPeak: 0.97, albumGainDb: album.replayGain?.albumGainDb, albumPeak: 0.98 },
        sonic: {
          bpm: rng.int(62, 178),
          key: rng.pick(KEYS),
          energy: rng.int(0, 100) / 100,
          mood: rng.pick(MOODS),
          danceability: rng.int(0, 100) / 100,
          valence: rng.int(0, 100) / 100,
        },
        offline: album.offline === "downloaded" ? "downloaded" : rng.chance(0.04) ? "cached" : "none",
        musicBrainzId: undefined,
        explicit: rng.chance(0.05),
        comment: undefined,
      };
      album.durationMs += durationMs;
      album.playCount += plays;
      if (track.lastPlayed !== undefined && (album.lastPlayed === undefined || track.lastPlayed > album.lastPlayed)) album.lastPlayed = track.lastPlayed;
      tracks.push(track);
      ids.push(id);
    }
    albums.push(album);
    albumTracks.set(albumId, ids);
    artistAlbums.set(artist.id, [...(artistAlbums.get(artist.id) ?? []), albumId]);
    artist.albumCount += 1;
    artist.songCount += songCount;
  }
  const genreCounts = new Map<string, { songs: number; albums: number }>();
  for (const a of albums) {
    const g = genreCounts.get(a.genre ?? "") ?? { songs: 0, albums: 0 };
    g.albums += 1;
    g.songs += a.songCount;
    genreCounts.set(a.genre ?? "", g);
  }
  const genres: Genre[] = [...genreCounts.entries()].map(([name, c]) => ({ name, songCount: c.songs, albumCount: c.albums })).sort((a, b) => a.name.localeCompare(b.name));

  const playlists: FakeLibrary["playlists"] = [];
  const playlistNames = ["Morning coffee", "Late drive", "Focus", "Party starters", "Slow Sundays", "Discovered 2026", "Running", "Old favourites"];
  playlistNames.forEach((name, i) => {
    const count = rng.int(12, 80);
    const ids = new Set<string>();
    while (ids.size < count) ids.add(rng.pick(tracks).id);
    const trackIds = [...ids];
    const isSmart = i === 2 || i === 5;
    playlists.push({
      playlist: {
        id: `pl-${i + 1}`,
        serverId,
        name,
        comment: isSmart ? "Rules: rating > 3, last played in the last 90 days" : undefined,
        owner: i === 7 ? "guest" : "you",
        public: rng.chance(0.3),
        songCount: trackIds.length,
        durationMs: trackIds.reduce((s, id) => s + (tracks.find((t) => t.id === id)?.durationMs ?? 0), 0),
        coverArt: `pl-${i + 1}`,
        created: now - rng.int(10, 600) * 86_400_000,
        changed: now - rng.int(0, 10) * 86_400_000,
        isSmart,
        isMine: i !== 7,
        offline: i === 0 ? "downloaded" : "none",
      },
      trackIds,
    });
  });
  const tracksById = new Map(tracks.map((t) => [t.id, t]));
  return { serverId, tracks, albums, artists, genres, playlists, tracksById, albumTracks, artistAlbums };
}

export function coverSeed(id: string): number {
  return hash32(id);
}
