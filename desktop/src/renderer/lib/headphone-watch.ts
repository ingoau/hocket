import { useEffect } from "react";
import { bridge } from "../core/bridge";

/** The system's virtual entries: they name whichever real output is the default. */
const VIRTUAL = new Set(["default", "communications"]);

type Output = { deviceId: string; groupId: string; label: string };

/**
 * Headphone handoff (beta). Tells the core when headphones connect here, so it can pull playback
 * from a device that plays (the core checks the setting). The native engine lists ALSA devices,
 * which don't show Bluetooth sinks behind PipeWire, so this asks Chromium instead.
 *
 * Chromium can't tell headphones from other outputs, so the test is: an output that wasn't there
 * before appeared and became the system default. That is what a headset connecting does; an
 * HDMI monitor or a dock usually doesn't take over the default.
 */
export function newDefaultOutput(before: Output[], after: Output[]): Output | undefined {
  const known = new Set(before.map((d) => d.deviceId));
  const def = after.find((d) => d.deviceId === "default");
  return after.find((d) => !VIRTUAL.has(d.deviceId) && !known.has(d.deviceId) && (!def || d.groupId === def.groupId));
}

/** The label without Chromium's "Default - " prefix (on the virtual entry only, but cheap). */
function outputName(d: Output): string | undefined {
  return d.label.replace(/^Default - /, "").trim() || undefined;
}

async function outputs(): Promise<Output[]> {
  const all = await navigator.mediaDevices.enumerateDevices();
  return all.filter((d) => d.kind === "audiooutput").map(({ deviceId, groupId, label }) => ({ deviceId, groupId, label }));
}

export function useHeadphoneWatch(): void {
  useEffect(() => {
    const md = navigator.mediaDevices;
    if (!md?.enumerateDevices) return;
    let known: Output[] | undefined;
    let stopped = false;
    const check = async () => {
      const now = await outputs().catch(() => undefined);
      if (stopped || !now) return;
      const before = known;
      known = now;
      if (!before) return;
      const added = newDefaultOutput(before, now);
      if (added) bridge().dispatch({ type: "headphonesConnected", data: { name: outputName(added) } });
    };
    void check();
    md.addEventListener("devicechange", check);
    return () => {
      stopped = true;
      md.removeEventListener("devicechange", check);
    };
  }, []);
}
