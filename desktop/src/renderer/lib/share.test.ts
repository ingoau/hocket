import { describe, expect, it } from "vitest";
import { geniusSearch, shareChoices, songLinkSearch } from "./share";

describe("share", () => {
  it("offers a Navidrome link, Song.link and the file for one song", () => {
    expect(shareChoices({ kind: "song", count: 1 })).toEqual({ navidrome: true, songLink: true, file: true });
    expect(shareChoices({ kind: "song", count: 3 })).toEqual({ navidrome: true, songLink: false, file: false });
  });

  it("offers Song.link for one album or artist, and only a Navidrome link for a playlist", () => {
    expect(shareChoices({ kind: "album", count: 1 })).toEqual({ navidrome: true, songLink: true, file: false });
    expect(shareChoices({ kind: "album", count: 2 })).toEqual({ navidrome: true, songLink: false, file: false });
    expect(shareChoices({ kind: "artist", count: 1 })).toEqual({ navidrome: false, songLink: true, file: false });
    expect(shareChoices({ kind: "artist", count: 2 })).toEqual({ navidrome: false, songLink: false, file: false });
    expect(shareChoices({ kind: "playlist", count: 1 })).toEqual({ navidrome: true, songLink: false, file: false });
    expect(shareChoices(undefined)).toEqual({ navidrome: false, songLink: false, file: false });
  });

  it("builds a Song.link search URL with spaces as %20", () => {
    expect(songLinkSearch("Boards of Canada Roygbiv")).toBe("https://song.link/s/Boards%20of%20Canada%20Roygbiv");
    expect(geniusSearch("Roygbiv", "Boards of Canada")).toBe("https://genius.com/search?q=%22Roygbiv%22%20%22Boards%20of%20Canada%22");
    expect(geniusSearch("  ", "Boards of Canada")).toBeUndefined();
  });
});
