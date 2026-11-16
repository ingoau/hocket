import { describe, expect, it } from "vitest";
import { byReleaseKind, releaseKind } from "./release-kind";

describe("release kind", () => {
  it("follows the core: compilation, then EP, then single, else album", () => {
    expect(releaseKind({ isCompilation: true, releaseTypes: ["ep"] })).toBe("compilation");
    expect(releaseKind({ isCompilation: false, releaseTypes: ["compilation"] })).toBe("compilation");
    expect(releaseKind({ isCompilation: false, releaseTypes: ["single", "ep"] })).toBe("ep");
    expect(releaseKind({ isCompilation: false, releaseTypes: ["single"] })).toBe("single");
    expect(releaseKind({ isCompilation: false, releaseTypes: ["live"] })).toBe("album");
    expect(releaseKind({ isCompilation: false })).toBe("album");
  });
  it("groups in shelf order, keeping order inside and dropping empty groups", () => {
    const a = (id: string, t: string[]) => ({ id, isCompilation: false, releaseTypes: t });
    const groups = byReleaseKind([a("1", ["single"]), a("2", []), a("3", ["single"])]);
    expect(groups.map((g) => [g.kind, g.albums.map((x) => x.id)])).toEqual([["album", ["2"]], ["single", ["1", "3"]]]);
  });
});
