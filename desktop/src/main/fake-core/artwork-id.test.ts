import { describe, expect, it } from "vitest";
import { safeArtworkId } from "./index";

describe("FakeCore artwork ids", () => {
  it("never lets a server-supplied id name a file outside the images dir", () => {
    expect(safeArtworkId("../../credentials.json")).not.toContain("/");
    expect(safeArtworkId("../../credentials.json")).not.toMatch(/^\./);
    expect(safeArtworkId("..\\..\\x")).not.toContain("\\");
    expect(safeArtworkId("al-1")).toBe("al-1");
    expect(safeArtworkId("")).toBe("_");
    expect(safeArtworkId("x".repeat(500)).length).toBeLessThanOrEqual(120);
  });
});
