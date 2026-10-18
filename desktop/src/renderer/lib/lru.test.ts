import { describe, expect, it } from "vitest";
import { LruCache } from "./lru";

describe("LruCache", () => {
  it("drops the oldest key once it is over capacity", () => {
    const c = new LruCache<number>(2);
    c.set("a", 1);
    c.set("b", 2);
    c.set("c", 3);
    expect(c.has("a")).toBe(false);
    expect(c.get("b")).toBe(2);
    expect(c.get("c")).toBe(3);
  });

  it("treats a get as a use, so the oldest unused key is evicted", () => {
    const c = new LruCache<number>(2);
    c.set("a", 1);
    c.set("b", 2);
    expect(c.get("a")).toBe(1);
    c.set("c", 3);
    expect(c.has("b")).toBe(false);
    expect(c.get("a")).toBe(1);
    expect(c.get("c")).toBe(3);
  });

  it("keeps an undefined value distinct from a miss", () => {
    const c = new LruCache<number | undefined>(1);
    c.set("gone", undefined);
    expect(c.has("gone")).toBe(true);
    expect(c.get("gone")).toBeUndefined();
  });
});
