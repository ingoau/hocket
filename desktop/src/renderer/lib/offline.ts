// What "offline" means to the UI. The core decides the effective network: it
// reports kind "offline" with no connection and also while offline mode
// (network.offlineMode) is on. The setting is read too, so turning it on shows
// at once instead of waiting for the core's NetworkChanged.
import type { AccountInfo, NetworkState, OfflineState } from "@core/api";

/** Why nothing goes to the server: the person chose offline mode, or there is no network. */
export type OfflineReason = "offlineMode" | "noConnection";

/**
 * How long, after offline mode is turned off, a network the core still reports as offline isn't
 * taken for "no connection". The core sends SettingChanged and then NetworkChanged; in between
 * the setting is off and the network still offline, which must not flash "No connection".
 */
export const LEAVE_OFFLINE_MODE_GRACE_MS = 3_000;

/**
 * `leaving`: offline mode was just turned off and the core's NetworkChanged hasn't come yet
 * (`leavingOfflineMode`). Then an offline network is the stale effect of offline mode, not
 * "no connection".
 */
export function offlineReason(network: NetworkState | undefined | null, offlineMode: boolean, leaving = false): OfflineReason | undefined {
  if (offlineMode) return "offlineMode";
  if (network?.kind !== "offline" || leaving) return undefined;
  return "noConnection";
}

/**
 * Whether an offline network still waits on the core after offline mode went off: only while the
 * grace lasts (`justLeft`) and the browser doesn't itself say there is no network.
 */
export function leavingOfflineMode(justLeft: boolean, browserOnline: boolean): boolean {
  return justLeft && browserOnline;
}

/**
 * A row to dim: offline, and its song can't play from this device. Only `none` can't: the core
 * plays a downloaded or cached song, and one it reports as `downloading` may be complete in the
 * stream cache (a pending download wins over cached in what it reports).
 */
export function unavailableOffline(offline: boolean, state: OfflineState | undefined): boolean {
  return offline && state === "none";
}

/**
 * Whether this device's offline skipping applies to what plays: the core only skips songs that
 * can't play offline when this device owns playback (it holds the transport lease, or nobody
 * does). Controlling another device over LAN Connect, that device's own state decides.
 */
export function ownsPlayback(leaseOwner: string | undefined | null, selfId: string | undefined): boolean {
  return !leaseOwner || leaseOwner === selfId;
}

/**
 * The servers that are configured but can't be reached while the network is up (offline, every
 * server is out of reach and the offline indicator already says so).
 */
export function unreachableServers(accounts: readonly AccountInfo[], offline: boolean): AccountInfo[] {
  return offline ? [] : accounts.filter((a) => !a.reachable);
}
