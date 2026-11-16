import { describe, expect, it } from "vitest";
import { MIXED_SOURCE, sourceOptions } from "./sources";

const a = { id: "a", name: "Home", url: "https://a/" };
const b = { id: "b", name: "", url: "https://b/" };

describe("sourceOptions", () => {
  it("offers nothing with one account", () => {
    expect(sourceOptions([a])).toEqual([]);
    expect(sourceOptions([a], true)).toEqual([]);
  });

  it("offers All, each account and Mixed for playlists", () => {
    expect(sourceOptions([a, b]).map((o) => o.id)).toEqual(["", "a", "b"]);
    expect(sourceOptions([a, b], true).map((o) => o.id)).toEqual(["", "a", "b", MIXED_SOURCE]);
    expect(sourceOptions([a, b])[2]?.label).toBe("https://b/");
  });
});
