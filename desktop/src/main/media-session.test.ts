import { describe, expect, it, vi } from "vitest";
import type { MediaSessionState } from "@core/api";

vi.mock("electron", () => ({}));
const { tickPosition } = await import("./media-session");

const base: MediaSessionState = {
  metadata: { title: "T", artist: undefined, album: undefined, durationMs: 10_000, artworkPath: undefined, trackId: "t", loved: false, rating: 0 },
  isPlaying: true,
  position: { positionMs: 1000, takenAt: 100_000, rate: 1, isPlaying: true },
  shuffle: false,
  repeat: "off",
  volume: 1,
  actions: [],
  ownsTransport: true,
};

describe("media session ticker position", () => {
  it("extrapolates on the session clock (local time + offset)", () => {
    // takenAt is session time; local clock is 2 s behind the session clock.
    expect(tickPosition(base, 100_500, 2000)).toBe(3500);
    expect(tickPosition(base, 100_500, 0)).toBe(1500);
    expect(tickPosition({ ...base, position: { ...base.position, rate: 2 } }, 101_000, 0)).toBe(3000);
  });

  it("clamps to a known duration and never to an unknown (0) one", () => {
    expect(tickPosition(base, 200_000, 0)).toBe(10_000);
    const unknown = { ...base, metadata: { ...base.metadata!, durationMs: 0 } };
    expect(tickPosition(unknown, 100_500, 0)).toBe(1500);
    expect(tickPosition({ ...base, metadata: undefined }, 100_500, 0)).toBe(1500);
  });

  it("never goes negative when the stamp is in the future", () => {
    expect(tickPosition(base, 90_000, 0)).toBe(1000);
  });
});
