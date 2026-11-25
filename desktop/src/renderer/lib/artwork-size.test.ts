import { describe, expect, it } from "vitest";
import { ARTWORK_SIZES } from "@shared/constants";
import { SAVER_OFF } from "../store/reducer";
import { largeArtworkSize } from "./artwork-size";

describe("large artwork size", () => {
  it("drops to the grid's size under battery saver's smallArtwork (#319)", () => {
    expect(largeArtworkSize(SAVER_OFF)).toBe(ARTWORK_SIZES.full);
    expect(largeArtworkSize({ level: "medium", automatic: false, features: ["stillBackground"] })).toBe(ARTWORK_SIZES.full);
    expect(largeArtworkSize({ level: "max", automatic: true, features: ["smallArtwork"] })).toBe(ARTWORK_SIZES.grid);
  });
});
