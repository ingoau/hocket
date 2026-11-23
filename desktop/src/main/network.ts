// What this device tells the core about its network. Desktop only knows online
// or offline (never metered, never which network), from the renderer's
// online/offline events and a periodic check in main. Both feed one reporter
// that passes on changes only: the poll repeats the same answer every time.
// Offline mode (network.offlineMode) is the core's to apply on top of this.
import type { NetworkState } from "@core/api";

export function networkState(online: boolean): NetworkState {
  return { kind: online ? "unknown" : "offline", metered: false, networkId: undefined };
}

export function networkReporter(push: (state: NetworkState) => void): (online: boolean) => void {
  let last: boolean | undefined;
  return (online) => {
    if (last === online) return;
    last = online;
    push(networkState(online));
  };
}
