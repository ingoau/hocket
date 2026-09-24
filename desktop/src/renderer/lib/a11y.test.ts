import { describe, expect, it } from "vitest";
import { nowPlayingAnnouncement, ratingText, seekKeyTarget, seekValueText, spokenDuration, volumeValueText } from "./a11y";

describe("spoken durations for aria-valuetext", () => {
  it("names each unit in words, singular and plural, and leaves zero parts out", () => {
    expect(spokenDuration(92_000)).toBe("1 minute 32 seconds");
    expect(spokenDuration(212_000)).toBe("3 minutes 32 seconds");
    expect(spokenDuration(60_000)).toBe("1 minute");
    expect(spokenDuration(1_000)).toBe("1 second");
    expect(spokenDuration(3_723_000)).toBe("1 hour 2 minutes 3 seconds");
    expect(spokenDuration(7_200_000)).toBe("2 hours");
  });

  it("says 0 seconds for zero, negative, missing or non-finite values, and floors milliseconds", () => {
    expect(spokenDuration(0)).toBe("0 seconds");
    expect(spokenDuration(-5)).toBe("0 seconds");
    expect(spokenDuration(undefined)).toBe("0 seconds");
    expect(spokenDuration(Number.NaN)).toBe("0 seconds");
    expect(spokenDuration(59_999)).toBe("59 seconds");
  });

  it("formats the seek slider as position of duration", () => {
    expect(seekValueText(92_000, 212_000)).toBe("1 minute 32 seconds of 3 minutes 32 seconds");
    expect(seekValueText(0, 212_000)).toBe("0 seconds of 3 minutes 32 seconds");
    // Unknown duration (nothing loaded yet): just the position.
    expect(seekValueText(5_000, undefined)).toBe("5 seconds");
    expect(seekValueText(5_000, 0)).toBe("5 seconds");
  });

  it("formats volume as a rounded, clamped percentage", () => {
    expect(volumeValueText(0.456)).toBe("46%");
    expect(volumeValueText(0)).toBe("0%");
    expect(volumeValueText(1.2)).toBe("100%");
  });

  it("names ratings", () => {
    expect(ratingText(0)).toBe("No rating");
    expect(ratingText(1)).toBe("1 star");
    expect(ratingText(4)).toBe("4 stars");
  });

  it("announces a track with and without an artist, and nothing without a title", () => {
    expect(nowPlayingAnnouncement("Word by Word", "Tally")).toBe("Now playing: Word by Word by Tally");
    expect(nowPlayingAnnouncement("Word by Word", undefined)).toBe("Now playing: Word by Word");
    expect(nowPlayingAnnouncement(undefined, "Tally")).toBe("");
  });
});

describe("seek slider keys", () => {
  it("moves 5 s per arrow and 30 s per page, clamped to the track, Home/End to the ends", () => {
    expect(seekKeyTarget("ArrowRight", 10_000, 200_000)).toBe(15_000);
    expect(seekKeyTarget("ArrowUp", 10_000, 200_000)).toBe(15_000);
    expect(seekKeyTarget("ArrowLeft", 3_000, 200_000)).toBe(0);
    expect(seekKeyTarget("ArrowDown", 10_000, 200_000)).toBe(5_000);
    expect(seekKeyTarget("PageUp", 180_000, 200_000)).toBe(200_000);
    expect(seekKeyTarget("PageDown", 40_000, 200_000)).toBe(10_000);
    expect(seekKeyTarget("Home", 40_000, 200_000)).toBe(0);
    expect(seekKeyTarget("End", 40_000, 200_000)).toBe(200_000);
  });

  it("ignores other keys and a track without a duration", () => {
    expect(seekKeyTarget("Enter", 1_000, 200_000)).toBeUndefined();
    expect(seekKeyTarget("ArrowRight", 1_000, 0)).toBeUndefined();
  });
});
