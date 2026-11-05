import { describe, expect, it } from "vitest";
import { newDefaultOutput } from "./headphone-watch";

const speakers = { deviceId: "spk", groupId: "g-spk", label: "Built-in Audio" };
const buds = { deviceId: "buds", groupId: "g-buds", label: "Pixel Buds Pro" };
const hdmi = { deviceId: "hdmi", groupId: "g-hdmi", label: "HDMI" };
const def = (g: { groupId: string; label: string }) => ({ deviceId: "default", groupId: g.groupId, label: `Default - ${g.label}` });

describe("newDefaultOutput", () => {
  it("finds headphones that connected and became the default", () => {
    expect(newDefaultOutput([def(speakers), speakers], [def(buds), speakers, buds])).toEqual(buds);
  });
  it("ignores a new output that didn't take over the default", () => {
    expect(newDefaultOutput([def(speakers), speakers], [def(speakers), speakers, hdmi])).toBeUndefined();
  });
  it("ignores the default moving to an output that was already there", () => {
    expect(newDefaultOutput([def(speakers), speakers, buds], [def(buds), speakers, buds])).toBeUndefined();
  });
  it("accepts a new output when there is no default entry", () => {
    expect(newDefaultOutput([speakers], [speakers, buds])).toEqual(buds);
  });
});
