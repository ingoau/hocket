import { describe, expect, it } from "vitest";
import type { AccountInfo } from "@core/api";
import { UNREACHABLE_GRACE_MS, UnreachableWatch } from "./unreachable";

const acc = (id: string, reachable: boolean) => ({ id, reachable }) as AccountInfo;
const G = UNREACHABLE_GRACE_MS;

describe("UnreachableWatch", () => {
  it("shows a server only once it has been out of reach for the grace period", () => {
    const w = new UnreachableWatch();
    // Launch: not checked yet, so not reachable.
    expect(w.update([acc("a", false)], false, 0)).toBe(false);
    expect(w.ids()).toEqual([]);
    expect(w.nextDueIn(0)).toBe(G);
    expect(w.refresh(G - 1)).toBe(false);
    expect(w.ids()).toEqual([]);
    expect(w.refresh(G)).toBe(true);
    expect(w.ids()).toEqual(["a"]);
    expect(w.nextDueIn(G)).toBeUndefined();
  });

  it("never shows the launch check that succeeds within the grace", () => {
    const w = new UnreachableWatch();
    w.update([acc("a", false)], false, 0);
    w.update([acc("a", true)], false, 2_000);
    expect(w.nextDueIn(2_000)).toBeUndefined();
    w.refresh(G * 2);
    expect(w.ids()).toEqual([]);
  });

  it("starts the count again after any break: reachable, or the network down", () => {
    const w = new UnreachableWatch();
    w.update([acc("a", false)], false, 0);
    w.update([acc("a", true)], false, 5_000);
    w.update([acc("a", false)], false, 6_000);
    w.refresh(G + 1);
    expect(w.ids()).toEqual([]);
    expect(w.nextDueIn(G + 1)).toBe(6_000 + G - (G + 1));
    // Offline, the offline indicator says it all; back online counts afresh.
    expect(w.update([acc("a", false)], true, 7_000)).toBe(false);
    w.update([acc("a", false)], false, 8_000);
    w.refresh(8_000 + G - 1);
    expect(w.ids()).toEqual([]);
    w.refresh(8_000 + G);
    expect(w.ids()).toEqual(["a"]);
  });

  it("drops a shown server as soon as it is reachable, removed or the network goes", () => {
    const w = new UnreachableWatch();
    w.update([acc("a", false), acc("b", false)], false, 0);
    w.refresh(G);
    expect(w.ids()).toEqual(["a", "b"]);
    expect(w.update([acc("a", true), acc("b", false)], false, G + 1)).toBe(true);
    expect(w.ids()).toEqual(["b"]);
    expect(w.update([acc("b", false)], true, G + 2)).toBe(true);
    expect(w.ids()).toEqual([]);
  });
});
