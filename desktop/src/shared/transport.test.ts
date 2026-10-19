import { describe, expect, it } from "vitest";
import type { Event, Snapshot, TransportState } from "@core/api";
import { PositionTickFilter } from "./transport";

const base: TransportState = { lease: { owner: "me", epoch: 3, expiresAt: 1 }, position: { positionMs: 5000, takenAt: 0, rate: 1, isPlaying: true }, buffering: false, playedMs: 0, volume: 0.5, muted: false, volumeDevice: undefined };
const at = (takenAt: number, positionMs: number, patch: Partial<TransportState> = {}): Event => ({ type: "transportChanged", data: { transport: { ...base, ...patch, position: { ...base.position, positionMs, takenAt, ...patch.position } } } });

describe("PositionTickFilter", () => {
  it("drops the ~1 Hz position reports and sends a fresh stamp every 2.5 s", () => {
    const f = new PositionTickFilter();
    const sent = [0, 1000, 2000, 3000, 4000, 5000, 6000].filter((t) => f.pass(at(t, 5000 + t)));
    // The first, then the first report more than 2.5 s after the last one sent, and so on.
    expect(sent).toEqual([0, 3000, 6000]);
  });

  it("sends seeks, pauses and volume changes at once", () => {
    const f = new PositionTickFilter();
    expect(f.pass(at(0, 5000))).toBe(true);
    expect(f.pass(at(1000, 90_000))).toBe(true);
    expect(f.pass(at(2000, 91_000, { position: { positionMs: 91_000, takenAt: 2000, rate: 1, isPlaying: false } }))).toBe(true);
    expect(f.pass(at(2100, 91_000, { volume: 0.9 }))).toBe(true);
    expect(f.pass(at(2200, 91_000, { buffering: true }))).toBe(true);
  });

  it("measures ticks against a window's snapshot and passes every other event", () => {
    const f = new PositionTickFilter();
    expect(f.pass(at(0, 5000))).toBe(true);
    const snapshot = { transport: { ...base, position: { ...base.position, positionMs: 80_000, takenAt: 500 } } } as unknown as Snapshot;
    expect(f.pass({ type: "snapshot", data: { snapshot } } as Event)).toBe(true);
    // A tick of the snapshot's stamp, not of the one sent before it.
    expect(f.pass(at(1500, 81_000))).toBe(false);
    expect(f.pass({ type: "queueChanged", data: { queue: {} } } as unknown as Event)).toBe(true);
  });
});
