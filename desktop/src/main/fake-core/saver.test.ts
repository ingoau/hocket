import { describe, expect, it } from "vitest";
import { DEFAULT_CUSTOM, parseCustom, parseLevel, resolveSaver, saverFeatures } from "./saver";

describe("fake battery saver", () => {
  it("nests the presets", () => {
    expect(saverFeatures("off", [])).toEqual([]);
    expect(saverFeatures("light", [])).toEqual(["stillBackground", "stillSeekWave", "lighterLyrics"]);
    expect(saverFeatures("medium", [])).toEqual(["stillBackground", "stillSeekWave", "lighterLyrics", "refreshCap", "slowSync"]);
    expect(saverFeatures("max", [])).toHaveLength(9);
    expect(saverFeatures("custom", ["smallArtwork", "stillBackground", "smallArtwork"])).toEqual(["stillBackground", "smallArtwork"]);
  });

  it("holds the chosen level without power save", () => {
    expect(resolveSaver("light", "max", true, false, DEFAULT_CUSTOM)).toEqual({ level: "light", automatic: false, features: saverFeatures("light", []) });
    expect(resolveSaver("off", "medium", true, false, DEFAULT_CUSTOM).features).toEqual([]);
  });

  it("power save raises but never lowers, and only with auto-engage", () => {
    expect(resolveSaver("off", "medium", true, true, DEFAULT_CUSTOM)).toMatchObject({ level: "medium", automatic: true });
    expect(resolveSaver("light", "max", true, true, DEFAULT_CUSTOM)).toMatchObject({ level: "max", automatic: true });
    expect(resolveSaver("max", "light", true, true, DEFAULT_CUSTOM)).toMatchObject({ level: "max", automatic: false });
    expect(resolveSaver("medium", "medium", true, true, DEFAULT_CUSTOM)).toMatchObject({ level: "medium", automatic: false });
    expect(resolveSaver("off", "medium", false, true, DEFAULT_CUSTOM)).toMatchObject({ level: "off", automatic: false, features: [] });
  });

  it("ranks custom against nothing but off", () => {
    expect(resolveSaver("custom", "max", true, true, ["smallArtwork"])).toEqual({ level: "custom", automatic: false, features: ["smallArtwork"] });
    expect(resolveSaver("light", "custom", true, true, ["smallArtwork"])).toMatchObject({ level: "light", automatic: false });
    expect(resolveSaver("off", "custom", true, true, ["smallArtwork"])).toEqual({ level: "custom", automatic: true, features: ["smallArtwork"] });
  });

  it("parses setting values", () => {
    expect(parseLevel("max", "off")).toBe("max");
    expect(parseLevel("loud", "off")).toBe("off");
    expect(parseCustom(["slowSync", "nope"])).toEqual(["slowSync"]);
    expect(parseCustom("x")).toEqual([]);
  });
});
