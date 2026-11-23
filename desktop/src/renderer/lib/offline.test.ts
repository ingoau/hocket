import { describe, expect, it } from "vitest";
import type { AccountInfo, NetworkState } from "@core/api";
import { leavingOfflineMode, offlineReason, ownsPlayback, unavailableOffline, unreachableServers } from "./offline";

const wifi: NetworkState = { kind: "wifi", metered: false, networkId: "home" };
const none: NetworkState = { kind: "offline", metered: false, networkId: undefined };

describe("offline", () => {
  it("tells offline mode from no connection", () => {
    expect(offlineReason(wifi, false)).toBeUndefined();
    expect(offlineReason(undefined, false)).toBeUndefined();
    expect(offlineReason(none, false)).toBe("noConnection");
    expect(offlineReason(none, true)).toBe("offlineMode");
    // Turning it on shows at once, before the core reports the network as offline.
    expect(offlineReason(wifi, true)).toBe("offlineMode");
  });

  it("doesn't flash no connection while offline mode turns off", () => {
    // The setting is off, the core's NetworkChanged not in yet: the offline network is offline mode's.
    expect(offlineReason(none, false, true)).toBeUndefined();
    expect(offlineReason(wifi, false, true)).toBeUndefined();
    // Offline mode on again wins.
    expect(offlineReason(none, true, true)).toBe("offlineMode");
    // The browser itself has no network: say so at once.
    expect(leavingOfflineMode(true, false)).toBe(false);
    expect(offlineReason(none, false, leavingOfflineMode(true, false))).toBe("noConnection");
    expect(leavingOfflineMode(true, true)).toBe(true);
    // Once the grace is over (or NetworkChanged came), an offline network is no connection.
    expect(leavingOfflineMode(false, true)).toBe(false);
    expect(offlineReason(none, false, leavingOfflineMode(false, true))).toBe("noConnection");
  });

  it("dims only what can't play offline at all, and only while offline", () => {
    expect(unavailableOffline(false, "none")).toBe(false);
    expect(unavailableOffline(true, "none")).toBe(true);
    // A pending download may already be complete in the stream cache: the core plays it.
    expect(unavailableOffline(true, "downloading")).toBe(false);
    expect(unavailableOffline(true, "cached")).toBe(false);
    expect(unavailableOffline(true, "downloaded")).toBe(false);
    expect(unavailableOffline(true, undefined)).toBe(false);
  });

  it("applies offline skipping only when this device owns playback", () => {
    expect(ownsPlayback(undefined, "me")).toBe(true);
    expect(ownsPlayback(null, "me")).toBe(true);
    expect(ownsPlayback("me", "me")).toBe(true);
    expect(ownsPlayback("pixel", "me")).toBe(false);
    expect(ownsPlayback("pixel", undefined)).toBe(false);
  });

  it("names unreachable servers only while the network is up", () => {
    const accounts = [{ id: "a", reachable: true }, { id: "b", reachable: false }] as AccountInfo[];
    expect(unreachableServers(accounts, false).map((a) => a.id)).toEqual(["b"]);
    expect(unreachableServers(accounts, true)).toEqual([]);
  });
});
