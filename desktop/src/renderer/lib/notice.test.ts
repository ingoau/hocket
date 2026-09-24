import { describe, expect, it } from "vitest";
import { PlayerNoticeCodeValues } from "@core/api";
import { hasString } from "@shared/strings";
import { isOfflineNotice, noticeText } from "./notice";

describe("player notices", () => {
  it("has a string for every code the core can send", () => {
    for (const code of PlayerNoticeCodeValues) expect(hasString(`notice.${code}`), code).toBe(true);
  });

  it("shows the renderer's text by code, filling in the detail", () => {
    expect(noticeText({ code: "couldNotPlaySkipped", detail: "Song", message: "ignored" })).toBe("Couldn't play Song, skipped");
    expect(noticeText({ code: "offlineSkipping", message: "whatever the core said" })).toBe("Offline: skipping tracks that aren't downloaded or cached");
    expect(noticeText({ code: "playbackProblem", detail: "decoder hiccup" })).toBe("Playback problem: decoder hiccup");
  });

  it("falls back to the core's message without a code (or without the detail a string needs)", () => {
    expect(noticeText({ message: "From an older core" })).toBe("From an older core");
    expect(noticeText({ code: "couldNotPlaySkipped", message: "Couldn't play X, skipped" })).toBe("Couldn't play X, skipped");
    expect(noticeText(undefined)).toBeUndefined();
  });

  it("matches the offline notices by code, not by text", () => {
    expect(isOfflineNotice({ code: "offlineSkipping" })).toBe(true);
    expect(isOfflineNotice({ code: "nothingAvailableOffline", message: "Localised elsewhere" })).toBe(true);
    expect(isOfflineNotice({ message: "Offline: skipping tracks that aren't downloaded or cached" })).toBe(false);
    expect(isOfflineNotice({ code: "couldNotPlaySkipped", detail: "X" })).toBe(false);
  });
});
