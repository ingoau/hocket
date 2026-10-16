// The volume control drives the device playing (Connect): this one, or the
// device holding transport, labelled with its name. Each device keeps its
// own level and mute; the core routes the commands.
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { useRemoteDevice } from "../components/ConnectPill";
import { volumeValueText } from "./a11y";

export interface VolumeControl {
  /** The level the slider shows (0 while muted). */
  shown: number;
  /** The device's level, kept across a mute. */
  level: number;
  muted: boolean;
  /** Another device plays and hasn't reported its volume (an older version). */
  unknown: boolean;
  /** "Volume", or "Volume – Pixel 8" while another device plays. */
  label: string;
  /** The name of the other device playing, if one is. */
  device: string | undefined;
  valueText: string;
  set: (volume: number) => void;
  toggleMute: () => void;
  /** Arrows move 5 %, Page keys 20 %; the native 1 % step stays for the pointer. */
  onKeyDown: (e: React.KeyboardEvent) => void;
  onWheel: (e: React.WheelEvent) => void;
}

const clamp = (v: number) => Math.round(Math.max(0, Math.min(1, v)) * 100) / 100;

/**
 * The level a relative step (keys, wheel) goes to, or `undefined` for none:
 * up while muted unmutes from silence; down while muted stays muted (setting
 * a level would unmute at 0 and lose the level the unmute goes back to), as
 * the core's volume down does.
 */
export function volumeStep(level: number, muted: boolean, delta: number): number | undefined {
  if (muted && delta < 0) return undefined;
  return clamp((muted ? 0 : level) + delta);
}

export function useVolumeControl(): VolumeControl {
  const level = useApp((s) => s.transport.volume);
  const muted = useApp((s) => s.transport.muted ?? false);
  const device = useApp((s) => s.transport.volumeDevice);
  const remote = useRemoteDevice();
  const unknown = !!remote && device !== remote.id;
  const shown = muted || unknown ? 0 : level;
  const set = (volume: number) => bridge().dispatch({ type: "setVolume", data: { volume: clamp(volume) } });
  const step = (delta: number) => {
    const to = volumeStep(level, muted, delta);
    if (to !== undefined) set(to);
  };
  return {
    shown,
    level,
    muted,
    unknown,
    label: remote ? t("player.volumeOn", { device: remote.name }) : t("player.volume"),
    device: remote?.name,
    valueText: muted ? t("a11y.muted") : volumeValueText(shown),
    set,
    toggleMute: () => bridge().dispatch({ type: "setMuted", data: { muted: !muted } }),
    onKeyDown: (e) => {
      const d = e.key === "ArrowUp" || e.key === "ArrowRight" ? 0.05 : e.key === "ArrowDown" || e.key === "ArrowLeft" ? -0.05 : e.key === "PageUp" ? 0.2 : e.key === "PageDown" ? -0.2 : 0;
      if (!d || e.ctrlKey || e.metaKey || e.altKey) return;
      e.preventDefault();
      e.stopPropagation();
      step(d);
    },
    onWheel: (e) => step(-Math.sign(e.deltaY) * 0.05),
  };
}
