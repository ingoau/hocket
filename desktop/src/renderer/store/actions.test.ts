import { afterEach, describe, expect, it, vi } from "vitest";
import { deletePlaylistsDialog, resolveTrackRefs } from "./actions";

describe("deleting several selected playlists", () => {
  it("asks once, listing every playlist, and deletes all of them on confirm", () => {
    const deleteOne = vi.fn();
    const dialog = deletePlaylistsDialog(["p1", "p2", "p3"], ["Morning", "Evening", "Night"], deleteOne);
    expect(dialog.kind).toBe("confirm");
    expect(dialog.destructive).toBe(true);
    expect(dialog.message).toContain("3 playlists");
    for (const n of ["Morning", "Evening", "Night"]) expect(dialog.message).toContain(n);
    dialog.onConfirm();
    expect(deleteOne.mock.calls.map((c) => c[0])).toEqual(["p1", "p2", "p3"]);
  });

  it("keeps the single-playlist wording for one", () => {
    const dialog = deletePlaylistsDialog(["p1"], ["Morning"], () => undefined);
    expect(dialog.message).toContain("“Morning”");
    expect(dialog.message).not.toContain("playlists");
  });
});

describe("resolving albums to songs", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("skips an album that fails to load and keeps the others", async () => {
    const query = vi.fn(async (q: { type: string; data: { id: string } }) => {
      if (q.data.id === "bad") throw new Error("the query failed");
      return { type: "trackList", data: [{ id: `${q.data.id}-1`, serverId: "ytm" }] };
    });
    vi.stubGlobal("window", { hocket: { query } });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const refs = await resolveTrackRefs({ type: "albums", data: { ids: ["a", "bad", "b"] } });
    expect(refs.map((r) => r.id)).toEqual(["a-1", "b-1"]);
    expect(query).toHaveBeenCalledTimes(3);
  });
});
