// How the jobs popover reads a Job. A queued job with `waiting` set is held by
// the core until the network allows it (offline, or metered with
// downloads.wifiOnly) and starts by itself: it is neither running nor stuck,
// so it says what it waits for and doesn't spin the activity icon.
import type { Job, JobWait } from "@core/api";
import type { StringId } from "@shared/strings";

export function isWaiting(j: Pick<Job, "state" | "waiting">): boolean {
  return j.state === "queued" && !!j.waiting;
}

/** Running, paused or about to run: what the activity icon spins for. */
export function isActive(j: Pick<Job, "state" | "waiting">): boolean {
  return (j.state === "running" || j.state === "queued" || j.state === "paused") && !isWaiting(j);
}

/** Can still be cancelled or resumed (a waiting job included). */
export function isPending(j: Pick<Job, "state">): boolean {
  return j.state === "running" || j.state === "queued" || j.state === "paused";
}

/** What downloads wait for, when a download job is held (pins don't say which job is theirs). */
export function downloadWait(jobs: readonly Pick<Job, "kind" | "state" | "waiting">[]): JobWait | undefined {
  return jobs.find((j) => j.kind === "download" && isWaiting(j))?.waiting ?? undefined;
}

export function waitLabel(wait: JobWait): StringId {
  return wait === "metered" ? "job.waiting.metered" : "job.waiting.offline";
}

export function jobStateLabel(j: Pick<Job, "state" | "waiting">): StringId {
  return isWaiting(j) && j.waiting ? waitLabel(j.waiting) : `job.state.${j.state}`;
}
