import { describe, expect, it } from "vitest";
import type { Event, Snapshot } from "@core/api";
import { applySnapshot, dismissToast, initialCoreState, reduce, settingValue } from "./reducer";

const snapshot: Snapshot = {
  servers: [],
  session: undefined,
  queue: { contextLabel: "Album", history: [], current: undefined, playingNext: [], upcoming: [], shuffle: true, repeat: "all", autoplay: true, mode: "apple", totalUpcoming: 0 },
  transport: { lease: { owner: "me", epoch: 3, expiresAt: 1 }, position: { positionMs: 5000, takenAt: 0, rate: 1, isPlaying: true }, buffering: false, playedMs: 0, volume: 0.5 },
  connection: { tier: "lan", connected: true, coordinatorUrl: undefined, clockOffsetMs: 0, roundTripMs: 4, peerCount: 1, error: undefined },
  devices: [],
  jobs: [],
  problems: [],
  undo: { canUndo: true, undoLabel: "Play Album", canRedo: false, redoLabel: undefined, history: [] },
  settings: [{ key: "appearance.theme", value: JSON.stringify("dark"), scope: "deviceLocal", updatedAt: 0 }],
  audio: { replayGain: "auto", replayGainPreampDb: 0, normalisation: false, eq: { enabled: false, preampDb: 0, bands: [], preset: undefined }, gapless: true, outputDevice: undefined, exclusive: false },
  mediaSession: { metadata: undefined, isPlaying: false, position: { positionMs: 0, takenAt: 0, rate: 1, isPlaying: false }, shuffle: false, repeat: "off", volume: 1, actions: [], ownsTransport: true },
  resumeOffer: undefined,
  sleepTimer: undefined,
  network: undefined,
  batterySaver: true,
  syncProgress: undefined,
};

describe("reducer", () => {
  it("applies a snapshot on Started", () => {
    const s = reduce(initialCoreState, { type: "started", data: { snapshot } });
    expect(s.ready).toBe(true);
    expect(s.queue.shuffle).toBe(true);
    expect(s.transport.volume).toBe(0.5);
    expect(s.batterySaver).toBe(true);
    expect(settingValue(s, "appearance.theme", "system")).toBe("dark");
    expect(settingValue(s, "missing", 42)).toBe(42);
  });

  it("mirrors *Changed events", () => {
    let s = applySnapshot(initialCoreState, snapshot);
    s = reduce(s, { type: "transportChanged", data: { transport: { ...snapshot.transport, volume: 0.9 } } });
    expect(s.transport.volume).toBe(0.9);
    s = reduce(s, { type: "settingChanged", data: { setting: { key: "x", value: "1", scope: "accountSynced", updatedAt: 1 } } });
    expect(settingValue(s, "x", 0)).toBe(1);
    s = reduce(s, { type: "libraryChanged", data: { server_id: "s", tables: ["tracks"], ids: ["a"] } });
    expect(s.libraryVersion).toBe(1);
    s = reduce(s, { type: "playerNotice", data: { message: "Couldn't play X, skipped" } });
    expect(s.playerNotice).toContain("skipped");
    s = reduce(s, { type: "resumeOfferChanged", data: { offer: { deviceName: "Pixel", track: { id: "t", serverId: "s", title: "T", durationMs: 1, rating: 0, loved: false, offline: "none" }, positionMs: 10, lastSeen: 0 } } });
    expect(s.resumeOffer?.deviceName).toBe("Pixel");
  });

  it("keeps at most four toasts and can dismiss one", () => {
    let s = initialCoreState;
    for (let i = 0; i < 6; i++) {
      const e: Event = { type: "toast", data: { toast: { id: `t${i}`, message: `m${i}`, actionLabel: undefined, actionCommand: undefined, durationMs: 1000 } } };
      s = reduce(s, e);
    }
    expect(s.toasts.map((t) => t.id)).toEqual(["t2", "t3", "t4", "t5"]);
    s = dismissToast(s, "t3");
    expect(s.toasts.map((t) => t.id)).toEqual(["t2", "t4", "t5"]);
  });

  it("nowPlaying follows QueueChanged and NowPlayingChanged", () => {
    const entry = { item: { key: "k", trackId: "t", source: { type: "inserted" as const }, unavailable: false }, track: { id: "t", serverId: "s", title: "T", durationMs: 1, rating: 0, loved: false, offline: "none" as const } };
    let s = reduce(initialCoreState, { type: "queueChanged", data: { queue: { ...snapshot.queue, current: entry } } });
    expect(s.nowPlaying?.track.id).toBe("t");
    s = reduce(s, { type: "nowPlayingChanged", data: { entry: undefined } });
    expect(s.nowPlaying).toBeUndefined();
  });
});
