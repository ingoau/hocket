import { describe, expect, it } from "vitest";
import { FULL_RECONCILE_DAYS, SYNC_INTERVAL_MINUTES, reconcileChoices, reconcileLabel, syncIntervalChoices, syncIntervalLabel } from "./sync-interval";

describe("sync interval choices", () => {
  it("offers the presets, with a value set elsewhere kept in order", () => {
    expect(syncIntervalChoices(60)).toEqual(SYNC_INTERVAL_MINUTES);
    expect(syncIntervalChoices(45)).toEqual([15, 30, 45, 60, 120, 360, 720, 1440]);
    expect(reconcileChoices(7)).toEqual(FULL_RECONCILE_DAYS);
    expect(reconcileChoices(10)).toEqual([1, 3, 7, 10, 14, 30, 90]);
  });

  it("stays within the core's ranges", () => {
    for (const m of SYNC_INTERVAL_MINUTES) expect(m >= 5 && m <= 1440).toBe(true);
    for (const d of FULL_RECONCILE_DAYS) expect(d >= 1 && d <= 90).toBe(true);
  });

  it("reads the way people say it", () => {
    expect(syncIntervalLabel(15)).toBe("Every 15 minutes");
    expect(syncIntervalLabel(60)).toBe("Every hour");
    expect(syncIntervalLabel(90)).toBe("Every 90 minutes");
    expect(syncIntervalLabel(360)).toBe("Every 6 hours");
    expect(syncIntervalLabel(1440)).toBe("Once a day");
    expect(reconcileLabel(1)).toBe("Every day");
    expect(reconcileLabel(14)).toBe("Every 14 days");
  });
});
