import { describe, expect, it } from "vitest";
import { mergeOrder } from "./action-orders";

describe("mergeOrder", () => {
  const stored = ["play", "removeFromQueue", "love", "rate1", "deletePlaylist"];
  const listed = new Set(["play", "love", "rate1", "goToAlbum"]);

  it("keeps the ids the list doesn't hold in their places", () => {
    expect(mergeOrder(stored, listed, ["love", "play", "rate1"])).toEqual(["love", "removeFromQueue", "play", "rate1", "deletePlaylist"]);
  });

  it("drops a switched-off id without touching the others", () => {
    expect(mergeOrder(stored, listed, ["play", "love"])).toEqual(["play", "removeFromQueue", "love", "deletePlaylist"]);
  });

  it("puts a newly switched-on id last, after the listed ones' order", () => {
    expect(mergeOrder(stored, listed, ["play", "goToAlbum", "love", "rate1"])).toEqual(["play", "removeFromQueue", "goToAlbum", "love", "deletePlaylist", "rate1"]);
  });
});
