// While the app first reaches the room, what the Connect controls show
// (volume, the device playing, the position) is this device's own idea, not
// yet the room's: dim them until it arrives. Only on launch, only with a
// coordinator to wait for, and never for long: offline or unreachable, the
// local state is all there is, so after a few seconds it shows as is.
import { useEffect, useState } from "react";
import type { ConnectionState } from "@core/api";
import { useApp } from "../store/app";

/** How long the controls wait for the room before showing local state undimmed. */
export const CONNECT_SYNC_TIMEOUT_MS = 5_000;

/**
 * Whether the Connect controls are still waiting for the room's state. `c` is
 * undefined before the first snapshot: there's no telling yet whether a
 * coordinator is configured, so they wait (only until the timeout, too).
 */
export function connectSyncing(c: ConnectionState | undefined, elapsedMs: number): boolean {
  if (elapsedMs >= CONNECT_SYNC_TIMEOUT_MS) return false;
  return !c || (!!c.coordinatorUrl && !c.connected && !c.error);
}

// Monotonic, so a clock change at login doesn't stretch the wait.
const startedAt = performance.now();
/** Once the room has answered (or we gave up), a later drop doesn't dim again. */
let settled = false;

export function useConnectSyncing(): boolean {
  const [, tick] = useState(0);
  const elapsed = performance.now() - startedAt;
  // A boolean, so the controls don't re-render on every ping once settled.
  const syncing = useApp((s) => !settled && connectSyncing(s.ready ? s.connection : undefined, elapsed));
  if (!syncing) settled = true;
  useEffect(() => {
    if (!syncing) return;
    const id = setTimeout(() => tick((n) => n + 1), Math.max(0, startedAt + CONNECT_SYNC_TIMEOUT_MS - performance.now()));
    return () => clearTimeout(id);
  }, [syncing]);
  return syncing;
}
