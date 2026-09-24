import { describe, expect, it } from "vitest";
import type { OfflineState } from "@core/api";
import { CACHE_MAX_DEFAULT, FakeStreamCache, primeBytes, trackBytes } from "./stream-cache";

const MIB = 1024 * 1024;
const GIB = 1024 * MIB;
type T = { id: string; offline: OfflineState; sizeBytes?: number; bitRate?: number; durationMs: number };
const tr = (id: string, offline: OfflineState = "none", sizeBytes = 8 * MIB): T => ({ id, offline, sizeBytes, bitRate: 320, durationMs: 200_000 });
const ok = { offline: false, metered: false, batterySaver: false };

describe("FakeStreamCache", () => {
  it("primes a partial start (not available offline) only when unmetered and not in battery saver", () => {
    const c = new FakeStreamCache();
    const a = tr("a");
    expect(c.prime("album", "alb", a, { ...ok, metered: true }, 0).outcome).toBe("declined");
    expect(c.prime("album", "alb", a, { ...ok, batterySaver: true }, 0).outcome).toBe("declined");
    const r = c.prime("album", "alb", a, ok, 0);
    expect(r).toMatchObject({ outcome: "primed", trackId: "a", bytes: primeBytes(a) });
    expect(c.partialBytes()).toBe(primeBytes(a));
    expect(a.offline).toBe("none");
    expect(FakeStreamCache.availableOffline(a)).toBe(false);
    // Deduplicated: already primed, cached or downloaded is skipped.
    expect(c.prime("track", "a", a, ok, 1).outcome).toBe("skipped");
    expect(c.prime("track", "d", tr("d", "downloaded"), ok, 1).outcome).toBe("skipped");
  });

  it("rate-limits priming to 20 per 10 minutes", () => {
    const c = new FakeStreamCache();
    const outcomes = Array.from({ length: 22 }, (_, i) => c.prime("track", `t${i}`, tr(`t${i}`), ok, 1000 + i).outcome);
    expect(outcomes.filter((o) => o === "primed")).toHaveLength(20);
    expect(c.prime("track", "late", tr("late"), ok, 1000 + 11 * 60_000).outcome).toBe("primed");
  });

  it("fills entries to Cached, evicts past the budget oldest first, and clears", () => {
    const tracks = new Map<string, T>([["a", tr("a")], ["b", tr("b")], ["c", tr("c")], ["d", tr("d", "downloaded")]]);
    const c = new FakeStreamCache();
    c.prime("track", "a", tracks.get("a"), ok, 0);
    for (const id of ["a", "b", "c"]) expect(c.fill(tracks.get(id)!, id !== "a")).toBe(true);
    expect(c.fill(tracks.get("d")!, true)).toBe(false);
    expect(c.partialBytes()).toBe(0);
    expect(c.usedBytes(tracks)).toBe(24 * MIB);
    expect(c.evict(tracks, 17 * MIB)).toEqual(["a"]);
    expect(tracks.get("a")!.offline).toBe("none");
    expect(c.clear(tracks)).toEqual(["b", "c"]);
    expect(tracks.get("d")!.offline).toBe("downloaded");
    expect(c.usedBytes(tracks)).toBe(0);
  });

  it("counts data saved when playing from disk and spends it on prefetch", () => {
    const c = new FakeStreamCache();
    const before = { saved: c.dataSavedBytes, served: c.servedFromDiskBytes, fetched: c.fetchedBytes };
    c.playStarted(tr("x", "cached"));
    expect(c.servedFromDiskBytes - before.served).toBe(8 * MIB);
    expect(c.dataSavedBytes - before.saved).toBe(8 * MIB);
    c.playStarted(tr("y"));
    expect(c.fetchedBytes - before.fetched).toBe(8 * MIB);
    c.fill(tr("z"), true);
    expect(c.dataSavedBytes - before.saved).toBe(0);
  });

  it("budget: automatic unless set to something other than the registry default", () => {
    expect(FakeStreamCache.budget(undefined, 120 * GIB)).toEqual({ bytes: CACHE_MAX_DEFAULT, auto: true });
    expect(FakeStreamCache.budget(undefined, 5 * GIB)).toEqual({ bytes: Math.floor(0.5 * GIB), auto: true });
    expect(FakeStreamCache.budget(CACHE_MAX_DEFAULT, 5 * GIB).auto).toBe(true);
    expect(FakeStreamCache.budget(4 * GIB, 5 * GIB)).toEqual({ bytes: 4 * GIB, auto: false });
    expect(trackBytes({ bitRate: 320, durationMs: 1000 })).toBe(40_000);
  });
});
