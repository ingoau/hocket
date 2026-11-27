// Long-pressing a queue button switches to the queue played before this one:
// the most recently played saved queue that isn't the playing one.
import type { SavedQueue } from "@core/api";
import { bridge } from "../core/bridge";
import { useApp } from "../store/app";

/** The most recently played saved queue other than the playing one (`currentId`, the view's `savedQueueId`). */
export function mostRecentOtherQueue(saved: readonly SavedQueue[], currentId: string | undefined): SavedQueue | undefined {
  let best: SavedQueue | undefined;
  for (const q of saved) {
    if (q.id === currentId) continue;
    if (!best || q.lastInteractedAt > best.lastInteractedAt) best = q;
  }
  return best;
}

/** Restore the most recent other queue. Returns whether there was one. */
export function switchToRecentQueue(): boolean {
  const s = useApp.getState();
  const q = mostRecentOtherQueue(s.savedQueues, s.queue.savedQueueId);
  if (!q) return false;
  bridge().dispatch({ type: "restoreSavedQueue", data: { id: q.id } });
  return true;
}
