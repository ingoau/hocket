// Page-side code inside `page.evaluate` runs in the renderer: give the specs
// the DOM lib and the preload bridge's type there.
/// <reference lib="dom" />
import type { HocketBridge } from "../src/shared/bridge-types";

declare global {
  interface Window {
    hocket: HocketBridge;
  }
}
