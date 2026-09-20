// Access to the preload bridge. The renderer never talks to Node directly.
import type { HocketBridge } from "@shared/bridge-types";

declare global {
  interface Window {
    hocket: HocketBridge;
  }
}

export function bridge(): HocketBridge {
  const b = window.hocket;
  if (!b) throw new Error("window.hocket is missing: preload didn't run");
  return b;
}
