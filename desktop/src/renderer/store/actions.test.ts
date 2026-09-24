import { describe, expect, it, vi } from "vitest";
import { deletePlaylistsDialog } from "./actions";

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
