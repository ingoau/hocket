// FakeCore's stand-in for the core's stream cache policy (crates/hocket-core/
// src/downloads, core/handlers/{cache,prefetch,prime}.rs), enough to demo it:
// partial entries from the album primer, tracks that become complete (Cached)
// as they play or are prefetched, a budget (automatic or custom) that evicts
// complete entries, and the served / fetched / data-saved counters.
import type { OfflineState, Track } from "@core/api";

const KIB = 1024;
const MIB = 1024 * KIB;
const GIB = 1024 * MIB;
/** The registry default of storage.cacheMaxBytes; a value equal to it means "automatic". */
export const CACHE_MAX_DEFAULT = 2 * GIB;
/** The core primes ~8 s, clamped to 256 KiB..1.5 MiB. */
const PRIME_MIN = 256 * KIB;
const PRIME_MAX = 1.5 * MIB;
const PRIME_WINDOW_MS = 10 * 60_000;
const PRIME_WINDOW_MAX = 20;

export interface PrimeGate {
  offline: boolean;
  metered: boolean;
  batterySaver: boolean;
}

export interface PrimeRecord {
  /** What asked: the album page / a play button. */
  kind: "album" | "track";
  /** The album or track id the command named. */
  id: string;
  trackId: string | undefined;
  at: number;
  outcome: "primed" | "skipped" | "declined";
  reason?: string;
  bytes?: number;
}

type CacheTrack = Pick<Track, "id" | "offline" | "sizeBytes" | "bitRate" | "durationMs">;

export function trackBytes(t: Pick<Track, "sizeBytes" | "bitRate" | "durationMs">): number {
  return t.sizeBytes ?? Math.round(((t.bitRate ?? 320) * 1000 / 8) * (t.durationMs / 1000));
}

export function primeBytes(t: Pick<Track, "bitRate">): number {
  return Math.round(Math.min(PRIME_MAX, Math.max(PRIME_MIN, (t.bitRate ?? 320) * 1000)));
}

export class FakeStreamCache {
  /** Partial entries (primed starts, seeks): track id → bytes held. Not "available offline". */
  readonly partial = new Map<string, number>();
  /** Complete entries in the order they were filled (the eviction order here). */
  private readonly complete: string[] = [];
  readonly primes: PrimeRecord[] = [];
  servedFromDiskBytes: number;
  fetchedBytes: number;
  dataSavedBytes: number;

  constructor(seedCompleted: CacheTrack[] = []) {
    for (const t of seedCompleted) if (t.offline === "cached") this.complete.push(t.id);
    // Some history so the Settings figure has something to show.
    this.servedFromDiskBytes = 1.3 * GIB;
    this.fetchedBytes = 3.1 * GIB;
    this.dataSavedBytes = 0.9 * GIB;
  }

  /** Available with no network: downloaded, or complete in the cache. */
  static availableOffline(t: { offline: OfflineState }): boolean {
    return t.offline === "downloaded" || t.offline === "cached";
  }

  /** PrimeAlbum / PrimeTrack on the device that owns playback (this one, in the fake). */
  prime(kind: "album" | "track", id: string, track: CacheTrack | undefined, gate: PrimeGate, now: number): PrimeRecord {
    const rec: PrimeRecord = { kind, id, trackId: track?.id, at: now, outcome: "declined" };
    this.primes.push(rec);
    if (this.primes.length > 200) this.primes.splice(0, this.primes.length - 200);
    if (!track) return Object.assign(rec, { reason: "unknown" });
    if (gate.offline || gate.metered) return Object.assign(rec, { reason: "metered" });
    if (gate.batterySaver) return Object.assign(rec, { reason: "battery saver" });
    if (track.offline === "downloaded" || track.offline === "cached" || this.partial.has(track.id)) return Object.assign(rec, { outcome: "skipped", reason: "already on disk" });
    const recent = this.primes.filter((p) => p.outcome === "primed" && now - p.at < PRIME_WINDOW_MS).length;
    if (recent >= PRIME_WINDOW_MAX) return Object.assign(rec, { reason: "rate limited" });
    const bytes = primeBytes(track);
    this.partial.set(track.id, bytes);
    this.fetchedBytes += bytes;
    this.dataSavedBytes = Math.max(0, this.dataSavedBytes - bytes);
    return Object.assign(rec, { outcome: "primed", bytes });
  }

  /** A track starts playing: from disk (served, saved) or from the server (fetched). */
  playStarted(t: CacheTrack): void {
    const size = trackBytes(t);
    if (FakeStreamCache.availableOffline(t)) {
      this.servedFromDiskBytes += size;
      this.dataSavedBytes += size;
      return;
    }
    const head = this.partial.get(t.id) ?? 0;
    // A primed start plays from disk; the rest streams.
    this.servedFromDiskBytes += head;
    this.dataSavedBytes += head;
    this.fetchedBytes += size - head;
  }

  /** Read through (played or prefetched): the entry is complete, Cached. `prefetch` counts the fetch. */
  fill(t: CacheTrack & { offline: OfflineState }, prefetch: boolean): boolean {
    if (t.offline !== "none") return false;
    const size = trackBytes(t);
    if (prefetch) {
      this.fetchedBytes += size - (this.partial.get(t.id) ?? 0);
      this.dataSavedBytes = Math.max(0, this.dataSavedBytes - size);
    }
    this.partial.delete(t.id);
    t.offline = "cached";
    this.complete.push(t.id);
    return true;
  }

  /** Complete entries past `budget` go, oldest first. Returns the evicted ids. */
  evict(tracksById: Map<string, CacheTrack & { offline: OfflineState }>, budget: number): string[] {
    const out: string[] = [];
    let used = this.usedBytes(tracksById);
    while (used > budget && this.complete.length) {
      const id = this.complete.shift()!;
      const t = tracksById.get(id);
      if (!t || t.offline !== "cached") continue;
      t.offline = "none";
      used -= trackBytes(t);
      out.push(id);
    }
    return out;
  }

  /** ClearStreamCache: everything evictable goes. */
  clear(tracksById: Map<string, CacheTrack & { offline: OfflineState }>): string[] {
    const ids = [...this.complete].filter((id) => tracksById.get(id)?.offline === "cached");
    for (const id of ids) tracksById.get(id)!.offline = "none";
    this.complete.length = 0;
    this.partial.clear();
    return ids;
  }

  partialBytes(): number {
    let n = 0;
    for (const b of this.partial.values()) n += b;
    return n;
  }

  usedBytes(tracksById: Map<string, CacheTrack>): number {
    let n = this.partialBytes();
    for (const id of this.complete) {
      const t = tracksById.get(id);
      if (t?.offline === "cached") n += trackBytes(t);
    }
    return n;
  }

  /** The effective budget: a custom setting, else min(2 GiB, 10% of free space). */
  static budget(setting: number | undefined, freeBytes: number): { bytes: number; auto: boolean } {
    if (setting !== undefined && setting > 0 && setting !== CACHE_MAX_DEFAULT) return { bytes: setting, auto: false };
    return { bytes: Math.min(CACHE_MAX_DEFAULT, Math.floor(freeBytes * 0.1)), auto: true };
  }
}
