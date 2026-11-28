import { describe, expect, it } from "vitest";
import { recallScroll, rememberScroll } from "./scroll-memory";
import type { Route } from "../store/app";

describe("scroll memory", () => {
  it("keeps a position per visit, not per page", () => {
    const visit: Route = { view: "albums" };
    const again: Route = { view: "albums" };
    rememberScroll(visit, "grid", 1200);
    expect(recallScroll(visit, "grid")).toBe(1200);
    // Going to Albums afresh is a new visit: it starts at the top.
    expect(recallScroll(again, "grid")).toBeUndefined();
  });

  it("keeps each scroller of a visit apart", () => {
    const visit: Route = { view: "library" };
    rememberScroll(visit, "a", 10);
    rememberScroll(visit, "b", 20);
    rememberScroll(visit, "a", 30);
    expect(recallScroll(visit, "a")).toBe(30);
    expect(recallScroll(visit, "b")).toBe(20);
  });
});
