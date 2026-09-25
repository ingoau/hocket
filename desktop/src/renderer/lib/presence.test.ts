import { describe, expect, it } from "vitest";
import { mergePresence, type PresenceEntry } from "./presence";

type Toast = { id: string; message: string };
const keyOf = (t: Toast) => t.id;
const a = { id: "a", message: "A" };
const b = { id: "b", message: "B" };
const c = { id: "c", message: "C" };

describe("mergePresence", () => {
  it("starts with every live item open, in order", () => {
    expect(mergePresence<Toast>([], [a, b], keyOf)).toEqual([
      { item: a, key: "a", closing: false },
      { item: b, key: "b", closing: false },
    ]);
  });

  it("keeps a removed item in place, closing, and appends new ones", () => {
    const rendered = mergePresence<Toast>([], [a, b], keyOf);
    expect(mergePresence(rendered, [b, c], keyOf)).toEqual([
      { item: a, key: "a", closing: true },
      { item: b, key: "b", closing: false },
      { item: c, key: "c", closing: false },
    ]);
  });

  it("reopens an item that comes back while closing", () => {
    const closing: PresenceEntry<Toast>[] = [{ item: a, key: "a", closing: true }];
    expect(mergePresence(closing, [a], keyOf)).toEqual([{ item: a, key: "a", closing: false }]);
  });

  it("keeps entry identity when nothing about an item changed", () => {
    const rendered = mergePresence<Toast>([], [a], keyOf);
    const next = mergePresence(rendered, [a], keyOf);
    expect(next[0]).toBe(rendered[0]);
    const gone = mergePresence(next, [], keyOf);
    expect(mergePresence(gone, [], keyOf)[0]).toBe(gone[0]);
  });

  it("takes the live item's newer value for an open key", () => {
    const rendered = mergePresence<Toast>([], [a], keyOf);
    const a2 = { id: "a", message: "A again" };
    expect(mergePresence(rendered, [a2], keyOf)[0]?.item).toBe(a2);
  });
});
