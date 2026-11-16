import { describe, expect, it } from "vitest";
import { heroMode } from "./DetailHead";

describe("heroMode", () => {
  it("is None without artwork, Small while it is still resolving", () => {
    expect(heroMode({ width: 3000, height: 3000 }, undefined, 900, 420, 1)).toBe("none");
    expect(heroMode(undefined, null, 900, 420, 1)).toBe("small");
  });
  it("is Large only when the cover fills the header without scaling up", () => {
    expect(heroMode({ width: 1000, height: 1000 }, "u", 900, 420, 1)).toBe("large");
    expect(heroMode({ width: 1000, height: 1000 }, "u", 900, 420, 2)).toBe("small");
    expect(heroMode({ width: 600, height: 600 }, "u", 900, 420, 1)).toBe("small");
    // A wide banner is limited by its height.
    expect(heroMode({ width: 2000, height: 400 }, "u", 900, 420, 1)).toBe("small");
  });
  it("is Small until the image has loaded and the header is measured", () => {
    expect(heroMode(undefined, "u", 900, 420, 1)).toBe("small");
    expect(heroMode({ width: 1000, height: 1000 }, "u", 0, 420, 1)).toBe("small");
  });
});
