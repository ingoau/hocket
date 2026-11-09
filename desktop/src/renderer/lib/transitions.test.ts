import { describe, expect, it } from "vitest";
import { DEFAULT_TRANSITIONS, clampCrossfade, isTransitionMode, withCrossfade, withMode } from "./transitions";

describe("transition settings", () => {
  it("keeps the crossfade length in whole seconds from 1 to 12", () => {
    expect(clampCrossfade(0)).toBe(1);
    expect(clampCrossfade(30)).toBe(12);
    expect(clampCrossfade(4.4)).toBe(4);
    expect(clampCrossfade(Number.NaN)).toBe(DEFAULT_TRANSITIONS.crossfadeSeconds);
  });

  it("changing the mode keeps the length, and changing the length keeps the mode", () => {
    expect(withMode({ mode: "off", crossfadeSeconds: 9 }, "autoMix")).toEqual({ mode: "autoMix", crossfadeSeconds: 9 });
    expect(withCrossfade({ mode: "crossfade", crossfadeSeconds: 6 }, 20)).toEqual({ mode: "crossfade", crossfadeSeconds: 12 });
  });

  it("knows the three modes", () => {
    expect(["off", "crossfade", "autoMix"].every(isTransitionMode)).toBe(true);
    expect(isTransitionMode("automix")).toBe(false);
  });
});
