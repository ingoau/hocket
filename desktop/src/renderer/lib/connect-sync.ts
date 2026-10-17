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

/** Whether the Connect controls are still waiting for the room's state. */
export function connectSyncing(c: ConnectionState, elapsedMs: number): boolean {
  return !!c.coordinatorUrl && !c.connected && !c.error && elapsedMs < CONNECT_SYNC_TIMEOUT_MS;
}

const startedAt = Date.now();
/** Once the room has answered (or we gave up), a later drop doesn't dim again. */
let settled = false;

export function useConnectSyncing(): boolean {
  const connection = useApp((s) => s.connection);
  const ready = useApp((s) => s.ready);
  const [, tick] = useState(0);
  const elapsed = Date.now() - startedAt;
  // Before the first snapshot there's no telling whether a coordinator is configured: wait.
  const syncing = !settled && (ready ? connectSyncing(connection, elapsed) : elapsed < CONNECT_SYNC_TIMEOUT_MS);
  if (!syncing) settled = true;
  useEffect(() => {
    if (!syncing) return;
    const id = setTimeout(() => tick((n) => n + 1), Math.max(0, startedAt + CONNECT_SYNC_TIMEOUT_MS - Date.now()));
    return () => clearTimeout(id);
  }, [syncing]);
  return syncing;
}
