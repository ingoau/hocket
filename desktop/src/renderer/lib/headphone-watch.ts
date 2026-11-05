import { useEffect } from "react";
import { bridge } from "../core/bridge";

/** The system's virtual entries: they name whichever real output is the default. */
const VIRTUAL = new Set(["default", "communications"]);
/** An output counts as just connected for this long, so the default moving to it can come in a later event. */
export const FRESH_MS = 10_000;
/** Linux: Chromium hears about device changes from udev, which Bluetooth sinks behind PipeWire don't touch, so poll. */
const POLL_MS = 4_000;

export type Output = { deviceId: string; groupId: string; label: string };

/** What the watcher remembers between checks. */
export type WatchState = {
  /** When each output was first seen; outputs there at start count as seen long ago. */
  seen: Map<string, number>;
  /** Outputs already reported, so one connect is reported once. */
  reported: Set<string>;
  /** False until the baseline check. */
  started: boolean;
};

/**
 * Headphone handoff (beta). Chromium can't tell headphones from other outputs, so the test is: an
 * output that connected moments ago is now the system default. That is what a headset connecting
 * does; an HDMI monitor or a dock usually doesn't take over the default. The new output and the
 * default switch can arrive as separate device changes (PipeWire, CoreAudio), hence the window.
 *
 * Returns the output to report, if any, and updates `state`. The first call is the
 * baseline: outputs already there don't count.
 */
export function checkOutputs(state: WatchState, outputs: Output[], now: number): Output | undefined {
  const baseline = !state.started;
  state.started = true;
  const real = outputs.filter((d) => !VIRTUAL.has(d.deviceId));
  for (const d of real) if (!state.seen.has(d.deviceId)) state.seen.set(d.deviceId, baseline ? -Infinity : now);
  const present = new Set(real.map((d) => d.deviceId));
  for (const id of [...state.seen.keys()]) {
    if (!present.has(id)) {
      // Gone: if it comes back, that is a new connect.
      state.seen.delete(id);
      state.reported.delete(id);
    }
  }
  if (baseline) return undefined;
  const def = outputs.find((d) => d.deviceId === "default");
  // ALSA names no real device as the default (its groupId is "default"): nothing to go on.
  const current = def && real.find((d) => d.groupId === def.groupId);
  if (!current || state.reported.has(current.deviceId)) return undefined;
  if (now - (state.seen.get(current.deviceId) ?? -Infinity) > FRESH_MS) return undefined;
  state.reported.add(current.deviceId);
  return current;
}

/** The label without Chromium's "Default - " prefix (on the virtual entry only, but cheap). */
function outputName(d: Output): string | undefined {
  return d.label.replace(/^Default - /, "").trim() || undefined;
}

async function outputs(): Promise<Output[]> {
  const all = await navigator.mediaDevices.enumerateDevices();
  return all.filter((d) => d.kind === "audiooutput").map(({ deviceId, groupId, label }) => ({ deviceId, groupId, label }));
}

/**
 * Tells the core when headphones connect here, so it can pull playback from a device that plays
 * (the core checks the setting). The native engine lists ALSA devices, which don't show Bluetooth
 * sinks behind PipeWire, so this asks Chromium instead.
 */
export function useHeadphoneWatch(): void {
  useEffect(() => {
    const md = navigator.mediaDevices;
    if (!md?.enumerateDevices) return;
    const state: WatchState = { seen: new Map(), reported: new Set(), started: false };
    let stopped = false;
    // One check at a time, in order: overlapping enumerations could resolve out of order.
    let chain = Promise.resolve();
    const check = () => {
      chain = chain.then(async () => {
        const now = await outputs().catch(() => undefined);
        if (stopped || !now) return;
        const added = checkOutputs(state, now, Date.now());
        if (added) bridge().dispatch({ type: "headphonesConnected", data: { name: outputName(added) } });
      });
    };
    check();
    md.addEventListener("devicechange", check);
    const poll = navigator.userAgent.includes("Linux") ? window.setInterval(check, POLL_MS) : undefined;
    return () => {
      stopped = true;
      md.removeEventListener("devicechange", check);
      if (poll !== undefined) window.clearInterval(poll);
    };
  }, []);
}
