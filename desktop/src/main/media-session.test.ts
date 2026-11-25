import { describe, expect, it, vi } from "vitest";
import type { Event, MediaSessionState } from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import type { NativeModule } from "./core-host";

vi.mock("electron", () => ({}));
const { MediaSessionHost, tickPosition } = await import("./media-session");

const base: MediaSessionState = {
  metadata: { title: "T", artist: undefined, album: undefined, durationMs: 10_000, artworkPath: undefined, trackId: "t", loved: false, rating: 0, stashed: false },
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

describe("media session host", () => {
  it("publishes each state at once, without artwork of its own, and the cover when the core sends it (#313)", () => {
    const published: MediaSessionState[] = [];
    class FakeSession {
      setState(json: string) {
        published.push(JSON.parse(json) as MediaSessionState);
      }
      setPosition() {}
      detach() {}
    }
    let listener: ((e: Event) => void) | undefined;
    const query = vi.fn();
    const core = { kind: "native", dispatch: vi.fn(), query, onEvent: (l: (e: Event) => void) => ((listener = l), () => undefined), shutdown: vi.fn() } as unknown as CoreHandle;
    const native = { MediaSession: FakeSession, mediaSessionAvailable: () => true } as unknown as NativeModule;
    const host = new MediaSessionHost({ core, native, hwnd: undefined, onRaise: vi.fn(), onQuit: vi.fn(), onOpenUri: vi.fn() });
    const paused = { ...base, isPlaying: false, position: { ...base.position, isPlaying: false, rate: 0 } };
    listener?.({ type: "mediaSession", data: { state: paused } });
    expect(published).toHaveLength(1);
    expect(published[0]!.metadata?.artworkPath).toBeUndefined();
    const withArt = { ...paused, metadata: { ...paused.metadata!, artworkPath: "/cache/a.jpg" } };
    listener?.({ type: "mediaSession", data: { state: withArt } });
    expect(published[1]!.metadata?.artworkPath).toBe("/cache/a.jpg");
    expect(query).not.toHaveBeenCalled();
    host.dispose();
  });
});
