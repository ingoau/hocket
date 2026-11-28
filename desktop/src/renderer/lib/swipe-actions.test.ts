/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { hasString } from "@shared/strings";
import { rowSwipeAction, sideKey, SWIPE_DEFAULTS, SWIPE_LIST_ACTIONS, SWIPE_QUEUE_ACTIONS, SWIPE_STASH_ACTIONS, swipeChoiceLabel } from "./swipe-actions";
import { DEFAULT_GESTURE_PREFS, normaliseGesturePrefs } from "./gesture-prefs";

const facts = { loved: false, downloaded: false, snoozed: false, queueSection: true };

describe("the core's swipe settings", () => {
  const registry = readFileSync(new URL("../../../../crates/hocket-core/src/settings/registry.rs", import.meta.url), "utf8");
  const list = (name: string) => [...(new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\]`).exec(registry)?.[1] ?? "").matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  it("offers the registry's choices, in its order", () => {
    expect(list("SWIPE_LIST_ACTIONS")).toEqual([...SWIPE_LIST_ACTIONS]);
    expect(list("SWIPE_QUEUE_ACTIONS")).toEqual([...SWIPE_QUEUE_ACTIONS]);
    expect(list("SWIPE_STASH_ACTIONS")).toEqual([...SWIPE_STASH_ACTIONS]);
  });
  it("falls back to the registry's defaults", () => {
    for (const [key, id] of [["SWIPE_LIST_START_TO_END", SWIPE_DEFAULTS.list.startToEnd], ["SWIPE_LIST_END_TO_START", SWIPE_DEFAULTS.list.endToStart], ["SWIPE_QUEUE_START_TO_END", SWIPE_DEFAULTS.queue.startToEnd], ["SWIPE_QUEUE_END_TO_START", SWIPE_DEFAULTS.queue.endToStart], ["SWIPE_STASH_START_TO_END", SWIPE_DEFAULTS.stash.startToEnd], ["SWIPE_STASH_END_TO_START", SWIPE_DEFAULTS.stash.endToStart]] as const) {
      expect(registry).toMatch(new RegExp(`${key},\\s*Synced,\\s*SettingKind::Enum\\(\\w+\\),\\s*\\|\\| json!\\("${id}"\\)`));
    }
  });
});

describe("rowSwipeAction", () => {
  it("names every choice in the pickers", () => {
    for (const id of [...SWIPE_LIST_ACTIONS, ...SWIPE_QUEUE_ACTIONS, ...SWIPE_STASH_ACTIONS]) {
      expect(hasString(`swipe.choice.${id}`)).toBe(true);
      expect(swipeChoiceLabel(id)).not.toBe("");
    }
  });

  it("slides a row removed from the queue or the stash out; everything else springs back", () => {
    expect(rowSwipeAction("removeFromQueue", facts)).toMatchObject({ runId: "removeFromQueue", dismisses: true, tone: "danger" });
    expect(rowSwipeAction("removeFromStash", facts)).toMatchObject({ runId: "removeFromStash", dismisses: true, tone: "danger" });
    expect(rowSwipeAction("playNext", facts)).toMatchObject({ runId: "playNext", dismisses: false });
    expect(rowSwipeAction("none", facts)).toBeNull();
  });

  it("follows the row's state: love or unlove, nothing to download, snooze or unsnooze", () => {
    expect(rowSwipeAction("love", facts)?.runId).toBe("love");
    expect(rowSwipeAction("love", { ...facts, loved: true })?.runId).toBe("unlove");
    expect(rowSwipeAction("download", { ...facts, downloaded: true })).toBeNull();
    expect(rowSwipeAction("snooze", { ...facts, snoozed: true })).toBeNull();
    expect(rowSwipeAction("unsnooze", facts)).toBeNull();
    expect(rowSwipeAction("unsnooze", { ...facts, snoozed: true })?.runId).toBe("unsnooze");
  });

  it("adds to the end while the queue section is off", () => {
    expect(rowSwipeAction("addToQueue", facts)?.runId).toBe("addToQueue");
    expect(rowSwipeAction("addToQueue", { ...facts, queueSection: false })?.runId).toBe("playLater");
  });

  it("maps a swipe right to start-to-end, mirrored right to left", () => {
    expect(sideKey(1, false)).toBe("startToEnd");
    expect(sideKey(-1, false)).toBe("endToStart");
    expect(sideKey(1, true)).toBe("endToStart");
  });
});

describe("normaliseGesturePrefs", () => {
  it("fills in what is missing and drops what it doesn't know", () => {
    expect(normaliseGesturePrefs(undefined)).toEqual(DEFAULT_GESTURE_PREFS);
    expect(normaliseGesturePrefs({ enabled: false, surfaces: { page: false }, sensitivity: "extreme", direction: "reversed", haptics: "yes" })).toEqual({
      ...DEFAULT_GESTURE_PREFS,
      enabled: false,
      surfaces: { ...DEFAULT_GESTURE_PREFS.surfaces, page: false },
      direction: "reversed",
    });
  });
});
