import { describe, expect, it } from "vitest";
import { firesInTextField } from "./keyboard";

describe("global shortcuts inside editable fields", () => {
  it("only the palette and Escape fire; every other chord, modifier or not, belongs to the field", () => {
    expect(firesInTextField("openCommandPalette")).toBe(true);
    expect(firesInTextField("ui.escape")).toBe(true);
    for (const id of ["ui.back", "ui.forward", "volumeUp", "volumeDown", "love", "undo", "redo", "redoAlt", "selectAll", "remove", "findInList", "togglePlay", "seekBackward", "rate3", "navigateSettings"]) {
      expect(firesInTextField(id), id).toBe(false);
    }
  });
});
