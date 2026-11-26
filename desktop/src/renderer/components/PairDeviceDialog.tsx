// Add a device: the core makes a pairing invite (identity, this device's key,
// a one-time secret, where to reach it, the account configs without
// passwords) and this shows it as a QR code (Hocket's scanner on a phone, or
// its camera) with Copy link for another computer. The dialog stays open
// until the new device joined (DevicePaired) or the invite runs out.
import { useEffect, useMemo } from "react";
import { encode } from "uqr";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { showToast } from "../lib/ui-toast";
import { Icon } from "./Icon";

export function PairDeviceDialog() {
  const close = useApp((s) => s.closeDialog);
  const pairing = useApp((s) => s.pairing);
  useEffect(() => {
    bridge().dispatch({ type: "createPairingInvite" });
  }, []);
  const invite = pairing.invite;
  return (
    <>
      <h2 className="dialog-title" id="dialog-title">{t("pairDevice.title")}</h2>
      <p className="dialog-body">{t("pairDevice.body")}</p>
      <div className="setup-device-code" role="img" aria-label={t("pairDevice.code")} data-testid="pair-device-code">
        {invite ? <QrCode text={invite.link} /> : null}
      </div>
      <p className="dialog-status" role="status" data-testid="pair-device-status">
        {pairing.paired
          ? t("pairDevice.paired", { name: pairing.paired.name })
          : pairing.error
            ? t("pairDevice.failed", { message: pairing.error })
            : invite
              ? `${t("pairDevice.waiting")} ${t("pairDevice.expires", { time: clock(invite.expiresAt) })}`
              : "…"}
      </p>
      <div className="buttons">
        <button type="button" className="btn" disabled={!invite} onClick={() => { if (invite) { bridge().clipboard.writeText(invite.link); showToast(t("pairDevice.copied")); } }} data-testid="pair-device-copy"><Icon name="copy" size={18} />{t("pairDevice.copy")}</button>
        <button type="button" className="btn primary" onClick={close} data-autofocus>{t("misc.close")}</button>
      </div>
    </>
  );
}

/** A time of day, as the system shows it. */
export function clock(epoch: number): string {
  return new Date(epoch).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Remove a device: says what removal does and doesn't undo, then removes it. */
export function RemoveDeviceDialog({ deviceId, name, self }: { deviceId: string; name: string; self: boolean }) {
  const close = useApp((s) => s.closeDialog);
  return (
    <>
      <h2 className="dialog-title" id="dialog-title">{t("identity.removeTitle", { name })}</h2>
      <p className="dialog-body" id="dialog-message">{self ? t("identity.removeSelfBody") : t("identity.removeBody")}</p>
      <div className="buttons">
        <button type="button" className="btn" onClick={close} data-autofocus>{t("dialog.cancel")}</button>
        <button type="button" className="btn danger" onClick={() => { bridge().dispatch({ type: "removeDevice", data: { device_id: deviceId } }); close(); }} data-testid="remove-device-confirm">{t("identity.removeConfirm")}</button>
      </div>
    </>
  );
}

/** [text] as a QR code: dark modules on white (scanners want a light quiet zone), crisp at any size. */
export function QrCode({ text }: { text: string }) {
  const path = useMemo(() => {
    const { data, size } = encode(text, { ecc: "M", border: 0 });
    let d = "";
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (data[y]![x]) d += `M${x} ${y}h1v1h-1z`;
    return { d, size };
  }, [text]);
  return (
    <svg viewBox={`-2 -2 ${path.size + 4} ${path.size + 4}`} shapeRendering="crispEdges" aria-hidden="true">
      <rect x={-2} y={-2} width={path.size + 4} height={path.size + 4} fill="#fff" />
      <path d={path.d} fill="#000" />
    </svg>
  );
}
