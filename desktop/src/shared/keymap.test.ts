import { describe, expect, it } from "vitest";
import { NAV_VIEWS, canonicalActionId, sidebarPlaces } from "./keymap";

describe("sidebar places", () => {
  it("maps the phone's Library place to Albums, its nearest place here", () => {
    expect(NAV_VIEWS.navigateLibrary).toBe("albums");
    expect(canonicalActionId("navigateLibrary")).toBe("navigateLibrary");
  });

  it("lists each place once, keeps unknown ids, and leaves Settings to the footer", () => {
    const ids = ["navigateHome", "findInList", "navigateLibrary", "navigateAlbums", "navigateSettings", "navigateTracks"];
    const places = sidebarPlaces(ids.map((id) => ({ id })));
    expect(places.map((p) => [p.id, p.view])).toEqual([
      ["navigateHome", "home"],
      ["findInList", undefined],
      ["navigateLibrary", "albums"],
      ["navigateTracks", "songs"],
    ]);
  });
});
