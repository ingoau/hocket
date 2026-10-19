// Telling the core's ~1 Hz position reports apart from transport changes a window must see.
// Shared by main (which does not forward the reports) and the renderer's reducer (which keeps
// its transport object for them, so nothing re-renders).
import type { Event, TransportState } from "@core/api";
import { snapshotOf } from "./core-handle";

function sameTransportUi(a: TransportState, b: TransportState): boolean {
  return a.lease.owner === b.lease.owner &&
    a.lease.epoch === b.lease.epoch &&
    a.buffering === b.buffering &&
    a.volume === b.volume &&
    a.muted === b.muted &&
    a.volumeDevice === b.volumeDevice &&
    a.position.isPlaying === b.position.isPlaying &&
    a.position.rate === b.position.rate;
}

/** ~1 Hz position reports: what `prev` extrapolates to within 1.5 s, at most 2.5 s on. */
export function isPositionTick(prev: TransportState, next: TransportState): boolean {
  if (!sameTransportUi(prev, next)) return false;
  const dt = next.position.takenAt - prev.position.takenAt;
  if (dt <= 0 || dt > 2500) return false;
  const expected = prev.position.positionMs + dt * prev.position.rate;
  return Math.abs(next.position.positionMs - expected) < 1500;
}

/**
 * Main's filter on the event fan-out: a `transportChanged` that is only a position tick against
 * the transport last sent is not sent (every window would keep the one it has and extrapolate).
 * Seeks, play/pause, buffering, volume and a fresh stamp every 2.5 s still go out, and a
 * snapshot (a window attaching) resets what was last sent.
 */
export class PositionTickFilter {
  private sent: TransportState | undefined;

  pass(event: Event): boolean {
    const snapshot = snapshotOf(event);
    if (snapshot) {
      this.sent = snapshot.transport;
      return true;
    }
    if (event.type !== "transportChanged") return true;
    const next = event.data.transport;
    if (this.sent && isPositionTick(this.sent, next)) return false;
    this.sent = next;
    return true;
  }
}
