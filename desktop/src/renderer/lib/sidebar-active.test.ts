import { describe, expect, it } from "vitest";
import type { Route, ViewName } from "../store/app";
import { isSidebarItemActive } from "./sidebar-active";

const ALL: ViewName[] = ["home", "library", "albums", "artists", "playlists", "songs", "genres", "filters", "downloads", "stats"];
const active = (route: Route, items: ViewName[] = ALL, pinnedOpen = false) => items.filter((v) => isSidebarItemActive(v, route, items, pinnedOpen));

describe("the highlighted sidebar item", () => {
  it("is only the detail page's own item, not Library as well", () => {
    expect(active({ view: "album", id: "a1" })).toEqual(["albums"]);
    expect(active({ view: "artist", id: "ar1" })).toEqual(["artists"]);
    expect(active({ view: "playlist", id: "p1" })).toEqual(["playlists"]);
    expect(active({ view: "genre", id: "Rock" })).toEqual(["genres"]);
    expect(active({ view: "filter", id: "f1" })).toEqual(["filters"]);
  });

  it("is the page's own item on list pages", () => {
    expect(active({ view: "songs" })).toEqual(["songs"]);
    expect(active({ view: "albums" })).toEqual(["albums"]);
    expect(active({ view: "home" })).toEqual(["home"]);
  });

  it("is Library on its own page", () => {
    expect(active({ view: "library" })).toEqual(["library"]);
  });

  it("falls back to Library on library pages without an item of their own", () => {
    const items: ViewName[] = ["home", "library"];
    expect(active({ view: "album", id: "a1" }, items)).toEqual(["library"]);
    expect(active({ view: "songs" }, items)).toEqual(["library"]);
    expect(active({ view: "snoozed" })).toEqual(["library"]);
  });

  it("is nothing in the sidebar list when a pin is the open page", () => {
    expect(active({ view: "album", id: "a1" }, ["home", "library"], true)).toEqual([]);
    expect(active({ view: "album", id: "a1" }, ALL, true)).toEqual(["albums"]);
  });

  it("is not Library on pages outside the library", () => {
    expect(active({ view: "search" })).toEqual([]);
    expect(active({ view: "settings" })).toEqual([]);
  });
});
