import { describe, expect, it } from "vitest";
import type { ConnectionState } from "@core/api";
import { CONNECT_SYNC_TIMEOUT_MS, connectSyncing } from "./connect-sync";

const conn = (patch: Partial<ConnectionState>): ConnectionState => ({ tier: "local", connected: false, coordinatorUrl: "wss://c/ws", clockOffsetMs: 0, roundTripMs: undefined, peerCount: 0, error: undefined, ...patch });

describe("connect controls waiting for the room", () => {
  it("wait while a coordinator is configured and not yet reached", () => {
    expect(connectSyncing(conn({}), 0)).toBe(true);
  });

  it("show local state once connected, on an error, or without a coordinator", () => {
    expect(connectSyncing(conn({ connected: true, tier: "coordinator" }), 0)).toBe(false);
    expect(connectSyncing(conn({ error: "unreachable" }), 0)).toBe(false);
    expect(connectSyncing(conn({ coordinatorUrl: undefined }), 0)).toBe(false);
  });

  it("stop waiting after the timeout (offline, a coordinator that never answers)", () => {
    expect(connectSyncing(conn({}), CONNECT_SYNC_TIMEOUT_MS - 1)).toBe(true);
    expect(connectSyncing(conn({}), CONNECT_SYNC_TIMEOUT_MS)).toBe(false);
  });
});
