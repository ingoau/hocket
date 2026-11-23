import { describe, expect, it } from "vitest";
import type { Job } from "@core/api";
import { t } from "@shared/strings";
import { downloadWait, isActive, isPending, isWaiting, jobStateLabel } from "./jobs";

type J = Pick<Job, "state" | "waiting">;

describe("jobs", () => {
  it("says what a waiting job waits for", () => {
    expect(t(jobStateLabel({ state: "queued", waiting: "offline" }))).toBe("Waiting for a connection");
    expect(t(jobStateLabel({ state: "queued", waiting: "metered" }))).toBe("Waiting for an unmetered connection");
    expect(t(jobStateLabel({ state: "queued" }))).toBe("Queued");
    expect(t(jobStateLabel({ state: "running" }))).toBe("Running");
  });

  it("only a queued job waits", () => {
    expect(isWaiting({ state: "queued", waiting: "offline" })).toBe(true);
    expect(isWaiting({ state: "queued" })).toBe(false);
    // A stale flag on a job that has moved on doesn't count.
    expect(isWaiting({ state: "running", waiting: "offline" } as J)).toBe(false);
    expect(jobStateLabel({ state: "done", waiting: "offline" } as J)).toBe("job.state.done");
  });

  it("finds what downloads wait for", () => {
    expect(downloadWait([])).toBeUndefined();
    expect(downloadWait([{ kind: "librarySync", state: "queued", waiting: "offline" }])).toBeUndefined();
    expect(downloadWait([{ kind: "download", state: "running" }, { kind: "download", state: "queued", waiting: "metered" }])).toBe("metered");
  });

  it("doesn't count a waiting job as running, but it can still be cancelled", () => {
    const waiting: J = { state: "queued", waiting: "metered" };
    expect(isActive(waiting)).toBe(false);
    expect(isPending(waiting)).toBe(true);
    expect(isActive({ state: "queued" })).toBe(true);
    expect(isActive({ state: "paused" })).toBe(true);
    expect(isActive({ state: "done" })).toBe(false);
  });
});
