import { describe, expect, it } from "vitest";
import { geniusSearch, shareChoices, songLinkIds } from "./share";

describe("share", () => {
  it("offers a Navidrome link, Song.link and the file for one song", () => {
    expect(shareChoices({ kind: "song", count: 1 })).toEqual({ navidrome: true, songLink: true, file: true });
    expect(shareChoices({ kind: "song", count: 3 })).toEqual({ navidrome: true, songLink: false, file: false });
  });

  it("offers Song.link for one album, nothing for an artist, and only a Navidrome link for a playlist", () => {
    expect(shareChoices({ kind: "album", count: 1 })).toEqual({ navidrome: true, songLink: true, file: false });
    expect(shareChoices({ kind: "album", count: 2 })).toEqual({ navidrome: true, songLink: false, file: false });
    expect(shareChoices({ kind: "artist", count: 1 })).toEqual({ navidrome: false, songLink: false, file: false });
    expect(shareChoices({ kind: "artist", count: 2 })).toEqual({ navidrome: false, songLink: false, file: false });
    expect(shareChoices({ kind: "playlist", count: 1 })).toEqual({ navidrome: true, songLink: false, file: false });
    expect(shareChoices(undefined)).toEqual({ navidrome: false, songLink: false, file: false });
  });

  it("asks the core for Song.link by the song's or the album's id", () => {
    expect(songLinkIds({ type: "tracks", data: { ids: ["t1"] } }, { kind: "song", count: 1, trackIds: ["t9"] })).toEqual({ track_id: "t9" });
    expect(songLinkIds({ type: "tracks", data: { ids: ["t1"] } }, { kind: "song", count: 1 })).toEqual({ track_id: "t1" });
    expect(songLinkIds({ type: "albums", data: { ids: ["al1"] } }, { kind: "album", count: 1 })).toEqual({ album_id: "al1" });
    expect(songLinkIds({ type: "artists", data: { ids: ["ar1"] } }, { kind: "artist", count: 1 })).toBeUndefined();
  });

  it("builds a Genius search URL with spaces as %20", () => {
    expect(geniusSearch("Roygbiv", "Boards of Canada")).toBe("https://genius.com/search?q=%22Roygbiv%22%20%22Boards%20of%20Canada%22");
    expect(geniusSearch("  ", "Boards of Canada")).toBeUndefined();
  });
});
