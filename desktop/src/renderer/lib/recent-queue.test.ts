import { describe, expect, it } from "vitest";
import type { SavedQueue } from "@core/api";
import { mostRecentOtherQueue } from "./recent-queue";

const q = (id: string, lastInteractedAt: number, pinned = false) => ({ id, lastInteractedAt, pinned }) as SavedQueue;

describe("mostRecentOtherQueue", () => {
  it("picks the most recently played queue, pinned or not", () => {
    expect(mostRecentOtherQueue([q("a", 1), q("b", 3, true), q("c", 2)], undefined)?.id).toBe("b");
  });
  it("skips the playing queue's own entry", () => {
    expect(mostRecentOtherQueue([q("a", 1), q("b", 3)], "b")?.id).toBe("a");
  });
  it("has nothing to switch to with no other queue", () => {
    expect(mostRecentOtherQueue([q("b", 3)], "b")).toBeUndefined();
    expect(mostRecentOtherQueue([], undefined)).toBeUndefined();
  });
});
