import { describe, expect, it, vi } from "vitest";
import { BACKGROUND_FPS, FOCUSED_FPS, perfForWindow, startKawarpBudget, type FrameClock, type KawarpDriver } from "./kawarp-budget";

function mockKawarp(): KawarpDriver & { start: ReturnType<typeof vi.fn>; stop: ReturnType<typeof vi.fn>; renderFrame: ReturnType<typeof vi.fn> } {
  return {
    start: vi.fn(),
    stop: vi.fn(),
    renderFrame: vi.fn(),
  };
}

/** A display at `hz`, like the position ticker uncapped: `run(ms)` fires its frames. */
function fakeClock(hz: number): FrameClock & { run(ms: number): void; subscribers(): number } {
  const subs = new Set<() => void>();
  const clock = {
    now: 0,
    subscribe(cb: () => void) {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    run(ms: number) {
      const end = clock.now + ms;
      while (clock.now + 1000 / hz <= end + 1e-6) {
        clock.now += 1000 / hz;
        for (const s of subs) s();
      }
    },
    subscribers: () => subs.size,
  };
  return clock;
}

describe("perfForWindow", () => {
  it("stops only when the Electron window is hidden, not when Chromium reports occlusion", () => {
    expect(perfForWindow(false, false)).toBe("stopped");
    expect(perfForWindow(false, true)).toBe("stopped");
    expect(perfForWindow(true, true)).toBe("full");
    expect(perfForWindow(true, false)).toBe("background");
  });
});

describe("startKawarpBudget", () => {
  it("stops the renderer when hidden", () => {
    const k = mockKawarp();
    const clock = fakeClock(120);
    expect(startKawarpBudget(k, "stopped", clock)).toBeUndefined();
    expect(k.stop).toHaveBeenCalledOnce();
    expect(k.start).not.toHaveBeenCalled();
    expect(clock.subscribers()).toBe(0);
    clock.run(1000);
    expect(k.renderFrame).not.toHaveBeenCalled();
  });

  it("paints ~30 fps without a timestamp when focused (display-rate rAF is the battery cliff)", () => {
    const k = mockKawarp();
    const clock = fakeClock(120);
    const dispose = startKawarpBudget(k, "full", clock);
    expect(k.start).not.toHaveBeenCalled();
    expect(k.stop).toHaveBeenCalledOnce();
    clock.run(1000);
    expect(k.renderFrame).toHaveBeenCalledTimes(FOCUSED_FPS);
    for (const call of k.renderFrame.mock.calls) {
      expect(call, "renderFrame must not be given a time; Kawarp treats that as animation seconds").toEqual([]);
    }
    k.renderFrame.mockClear();
    dispose?.();
    expect(clock.subscribers()).toBe(0);
    clock.run(1000);
    expect(k.renderFrame).not.toHaveBeenCalled();
  });

  it("paints ~24 fps without a timestamp when unfocused (wall-clock ms would flicker the warp)", () => {
    const k = mockKawarp();
    const clock = fakeClock(120);
    const dispose = startKawarpBudget(k, "background", clock);
    expect(k.stop).toHaveBeenCalledOnce();
    expect(k.start).not.toHaveBeenCalled();
    expect(k.renderFrame).not.toHaveBeenCalled();

    clock.run(1000);
    expect(k.renderFrame).toHaveBeenCalledTimes(BACKGROUND_FPS);
    for (const call of k.renderFrame.mock.calls) {
      expect(call, "renderFrame must not be given a time; Kawarp treats that as animation seconds").toEqual([]);
    }

    k.renderFrame.mockClear();
    dispose?.();
    clock.run(1000);
    expect(k.renderFrame).not.toHaveBeenCalled();
  });

  it("paints only on the shared clock's frames, so it adds no frames of its own", () => {
    const k = mockKawarp();
    const clock = fakeClock(60);
    const frames: number[] = [];
    clock.subscribe(() => frames.push(clock.now));
    const painted: number[] = [];
    k.renderFrame.mockImplementation(() => painted.push(clock.now));
    startKawarpBudget(k, "full", clock);
    clock.run(1000);
    expect(painted.length).toBe(FOCUSED_FPS);
    expect(painted.every((t) => frames.includes(t))).toBe(true);
  });
});
