import { afterEach, describe, expect, it, vi } from "vitest";
import { BACKGROUND_FPS, perfForWindow, startKawarpBudget, type KawarpDriver } from "./kawarp-budget";

function mockKawarp(): KawarpDriver & { start: ReturnType<typeof vi.fn>; stop: ReturnType<typeof vi.fn>; renderFrame: ReturnType<typeof vi.fn> } {
  return {
    start: vi.fn(),
    stop: vi.fn(),
    renderFrame: vi.fn(),
  };
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
  afterEach(() => {
    vi.useRealTimers();
  });

  it("stops the renderer when hidden", () => {
    const k = mockKawarp();
    expect(startKawarpBudget(k, "stopped")).toBeUndefined();
    expect(k.stop).toHaveBeenCalledOnce();
    expect(k.start).not.toHaveBeenCalled();
    expect(k.renderFrame).not.toHaveBeenCalled();
  });

  it("starts Kawarp's rAF loop when focused, and the disposer stops it", () => {
    const k = mockKawarp();
    const dispose = startKawarpBudget(k, "full");
    expect(k.start).toHaveBeenCalledOnce();
    expect(k.stop).not.toHaveBeenCalled();
    dispose?.();
    expect(k.stop).toHaveBeenCalledOnce();
  });

  it("paints ~24 fps without a timestamp when unfocused (wall-clock ms would flicker the warp)", () => {
    vi.useFakeTimers();
    const k = mockKawarp();
    const dispose = startKawarpBudget(k, "background");
    expect(k.stop).toHaveBeenCalledOnce();
    expect(k.start).not.toHaveBeenCalled();
    expect(k.renderFrame).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1000);
    expect(k.renderFrame).toHaveBeenCalledTimes(BACKGROUND_FPS);
    for (const call of k.renderFrame.mock.calls) {
      expect(call, "renderFrame must not be given a time; Kawarp treats that as animation seconds").toEqual([]);
    }

    k.renderFrame.mockClear();
    dispose?.();
    vi.advanceTimersByTime(1000);
    expect(k.renderFrame).not.toHaveBeenCalled();
  });
});
