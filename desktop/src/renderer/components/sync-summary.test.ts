// How the sync summary in the jobs popover words a sync's duration.
import { describe, expect, it } from "vitest";
import { fmtElapsed } from "./TopBar";

describe("fmtElapsed", () => {
  it("uses seconds, then minutes, then hours", () => {
    expect(fmtElapsed(200)).toBe("1 s");
    expect(fmtElapsed(40_000)).toBe("40 s");
    expect(fmtElapsed(185_000)).toBe("3 min 5 s");
    expect(fmtElapsed(3_700_000)).toBe("1 h 1 min");
  });
});
