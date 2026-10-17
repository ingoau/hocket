// The Connect button of the player bar and the fullscreen player: the device's
// icon, and, only while another device is playing, its name on an accent fill
// (playing here needs no name). Click opens the device picker; right-click
// takes playback back to this device.
import type { DeviceInfo, Platform } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { executeAction } from "../store/actions";
import { Icon } from "./Icon";
import { useConnectSyncing } from "../lib/connect-sync";

/** A device kind's icon (Material Symbols Rounded). */
export function deviceIcon(platform: Platform | undefined): string {
  switch (platform) {
    case "android":
      return "phone";
    case "macOs":
    case "windows":
    case "linux":
      return "laptop";
    default:
      return "devices";
  }
}

/** The device holding the transport lease when it is not this one. */
export function useRemoteDevice(): DeviceInfo | undefined {
  return useApp((s) => {
    const owner = s.devices.find((x) => x.id === s.transport.lease.owner);
    return owner && !owner.isSelf ? owner : undefined;
  });
}

export function ConnectPill({ className = "", testId, dropdown = false, iconSize = 20 }: { className?: string; testId: string; dropdown?: boolean; iconSize?: number }) {
  const remote = useRemoteDevice();
  const syncing = useConnectSyncing();
  const label = remote ? t("nowPlaying.connectOn", { device: remote.name }) : t("player.connect");
  const takeBack = (e: React.MouseEvent) => {
    e.preventDefault();
    const s = useApp.getState();
    const self = s.devices.find((x) => x.isSelf);
    if (remote && self) bridge().dispatch({ type: "handoffTo", data: { device_id: self.id } });
    else void executeAction("ui.playOn");
  };
  return (
    <button type="button" className={`connect-pill ${remote ? "remote" : ""} ${dropdown ? "dropdown" : ""} ${syncing ? "connect-syncing" : ""} ${className}`} aria-busy={syncing || undefined} aria-label={label} aria-haspopup="dialog" title={remote ? `${label} (${t("player.connectHint")})` : t("player.connectHint")}
      onClick={() => void executeAction("ui.playOn")} onContextMenu={takeBack} data-remote={remote ? "" : undefined} data-testid={testId}>
      <Icon name={remote ? deviceIcon(remote.platform) : "devices"} size={iconSize} />
      {remote ? <span className="connect-name" data-testid={`${testId}-name`}>{remote.name}</span> : null}
      {dropdown ? <Icon name="dropDown" size={iconSize} className="connect-caret" /> : null}
    </button>
  );
}
