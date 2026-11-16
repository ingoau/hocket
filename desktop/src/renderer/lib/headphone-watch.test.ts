import { describe, expect, it } from "vitest";
import { checkOutputs, FRESH_MS, type WatchState } from "./headphone-watch";

const speakers = { deviceId: "spk", groupId: "g-spk", label: "Built-in Audio" };
const buds = { deviceId: "buds", groupId: "g-buds", label: "Pixel Buds Pro" };
const hdmi = { deviceId: "hdmi", groupId: "g-hdmi", label: "HDMI" };
const def = (g: { groupId: string; label: string }) => ({ deviceId: "default", groupId: g.groupId, label: `Default - ${g.label}` });
const fresh = (): WatchState => ({ seen: new Map(), reported: new Set(), started: false });

describe("checkOutputs", () => {
  it("finds headphones that connected and became the default", () => {
    const s = fresh();
    expect(checkOutputs(s, [def(speakers), speakers], 0)).toBeUndefined();
    expect(checkOutputs(s, [def(buds), speakers, buds], 1_000)).toEqual(buds);
  });
  it("catches the default moving in a later change", () => {
    const s = fresh();
    checkOutputs(s, [def(speakers), speakers], 0);
    expect(checkOutputs(s, [def(speakers), speakers, buds], 1_000)).toBeUndefined();
    expect(checkOutputs(s, [def(buds), speakers, buds], 2_000)).toEqual(buds);
  });
  it("reports one connect once", () => {
    const s = fresh();
    checkOutputs(s, [def(speakers), speakers], 0);
    expect(checkOutputs(s, [def(buds), speakers, buds], 1_000)).toEqual(buds);
    expect(checkOutputs(s, [def(buds), speakers, buds], 2_000)).toBeUndefined();
  });
  it("reports a reconnect again", () => {
    const s = fresh();
    checkOutputs(s, [def(speakers), speakers], 0);
    checkOutputs(s, [def(buds), speakers, buds], 1_000);
    checkOutputs(s, [def(speakers), speakers], 60_000);
    expect(checkOutputs(s, [def(buds), speakers, buds], 120_000)).toEqual(buds);
  });
  it("ignores a new output that didn't take over the default", () => {
    const s = fresh();
    checkOutputs(s, [def(speakers), speakers], 0);
    expect(checkOutputs(s, [def(speakers), speakers, hdmi], 1_000)).toBeUndefined();
  });
  it("ignores the default moving to an output connected long ago", () => {
    const s = fresh();
    checkOutputs(s, [def(speakers), speakers], 0);
    checkOutputs(s, [def(speakers), speakers, buds], 1_000);
    expect(checkOutputs(s, [def(buds), speakers, buds], 1_000 + FRESH_MS + 1)).toBeUndefined();
  });
  it("ignores outputs already there at start", () => {
    const s = fresh();
    checkOutputs(s, [def(speakers), speakers, buds], 0);
    expect(checkOutputs(s, [def(buds), speakers, buds], 1_000)).toBeUndefined();
  });
  it("does nothing when the default names no real output (ALSA)", () => {
    const s = fresh();
    const alsaDefault = { deviceId: "default", groupId: "default", label: "Default" };
    checkOutputs(s, [alsaDefault, speakers], 0);
    expect(checkOutputs(s, [alsaDefault, speakers, buds], 1_000)).toBeUndefined();
  });
});
