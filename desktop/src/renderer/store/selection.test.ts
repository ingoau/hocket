import { describe, expect, it } from "vitest";
import { EMPTY_SELECTION, explicitIds, isSelected, navigate, selectAll, selectOnly, selectRange, selectionCount, toggle, typeAhead } from "./selection";

const order = ["a", "b", "c", "d", "e"];

describe("selection model", () => {
  it("selects one, toggles and ranges", () => {
    let s = selectOnly("b");
    expect(isSelected(s, "b")).toBe(true);
    s = toggle(s, "d");
    expect(selectionCount(s)).toBe(2);
    s = selectRange(s, "e", order, false);
    expect([...s.ids]).toEqual(["d", "e"]);
    s = selectRange(selectOnly("d"), "a", order, false);
    expect([...s.ids].sort()).toEqual(["a", "b", "c", "d"]);
  });

  it("select-all is a predicate with exclusions, never materialised", () => {
    let s = selectAll(50_000);
    expect(selectionCount(s)).toBe(50_000);
    expect(isSelected(s, "zzz")).toBe(true);
    expect(explicitIds(s)).toBeUndefined();
    s = toggle(s, "zzz");
    expect(isSelected(s, "zzz")).toBe(false);
    expect(selectionCount(s)).toBe(49_999);
    expect(explicitIds(EMPTY_SELECTION)).toEqual([]);
  });

  it("navigates with arrows, home/end and pages", () => {
    expect(navigate("ArrowDown", 0, 10, 5)).toBe(1);
    expect(navigate("ArrowUp", 0, 10, 5)).toBe(0);
    expect(navigate("End", 0, 10, 5)).toBe(9);
    expect(navigate("PageDown", 7, 10, 5)).toBe(9);
    expect(navigate("PageUp", 7, 10, 5)).toBe(2);
    expect(navigate("x", 7, 10, 5)).toBeUndefined();
    expect(navigate("ArrowDown", 0, 0, 5)).toBeUndefined();
  });

  it("type-ahead wraps and is case-insensitive", () => {
    const labels = ["Alpha", "beta", "Gamma", "banana"];
    expect(typeAhead("b", labels, 0)).toBe(1);
    expect(typeAhead("b", labels, 1)).toBe(3);
    expect(typeAhead("b", labels, 3)).toBe(1);
    expect(typeAhead("zz", labels, 0)).toBeUndefined();
  });
});
