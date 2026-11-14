// Set up another device: a QR code of this device's hocket://setup link (the
// server, who is signed in, the coordinator when there is one or else the
// settings changed from their defaults, which a coordinator would sync, and the
// password only when switched on), for Hocket's scanner or a phone camera, plus Copy link
// for another computer. Main builds the link so the password leaves the
// credential store only when asked for (IPC.setupLink).
import { useEffect, useMemo, useState } from "react";
import { encode } from "uqr";
import { t } from "@shared/strings";
import { packSetupSettings, setupSettingsCount } from "@shared/setup-settings";
import type { SetupLinkResult } from "@shared/bridge-types";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { showToast } from "../lib/ui-toast";
import { Icon } from "./Icon";
import { Switch } from "./controls";

export function SetupDeviceDialog() {
  const close = useApp((s) => s.closeDialog);
  const server = useApp((s) => s.accounts[0]);
  const coordinator = useApp((s) => s.connection.coordinatorUrl) || undefined;
  const [includePassword, setIncludePassword] = useState(false);
  const [result, setResult] = useState<SetupLinkResult>();
  // Without a coordinator the settings ride in the link: the core's JSON, then packed.
  // "loading" until they're known, so no link (or Copy) goes out without them.
  const [settings, setSettings] = useState<{ json: string; packed: string } | "loading" | undefined>(coordinator ? undefined : "loading");

  useEffect(() => {
    if (coordinator) return setSettings(undefined);
    setSettings("loading");
    let live = true;
    void (async () => {
      const r = await bridge().query({ type: "setupSettings" });
      const json = r.type === "text" && setupSettingsCount(r.data) > 0 ? r.data : undefined;
      const next = json ? { json, packed: await packSetupSettings(json) } : undefined;
      if (live) setSettings(next);
    })().catch((err: unknown) => {
      console.error("setup settings failed", err);
      if (live) setSettings(undefined);
    });
    return () => { live = false; };
  }, [coordinator]);

  useEffect(() => {
    if (!server || settings === "loading") return;
    let live = true;
    bridge().setupLink({ server: server.url, user: server.username, coordinator, settings: settings?.packed, includePassword })
      .then((r) => { if (live) setResult(r); })
      .catch((err: unknown) => console.error("setup link failed", err));
    return () => { live = false; };
  }, [server, coordinator, settings, includePassword]);

  if (!server) return null;
  const host = server.url.replace(/^https?:\/\//, "").replace(/\/$/, "");
  const passwordOn = includePassword && !!result?.passwordAvailable;
  const unavailable = result !== undefined && !result.passwordAvailable;
  return (
    <>
      <h2 className="dialog-title" id="dialog-title">{t("setupDevice.title")}</h2>
      <p className="muted small">{t("setupDevice.body")}</p>
      <div className="setup-device-code" role="img" aria-label={t("setupDevice.code", { host })} data-testid="setup-device-code">
        {result ? <QrCode text={result.codeLink} /> : null}
      </div>
      <dl className="setup-device-facts">
        <dt>{t("setupDevice.server")}</dt>
        <dd>{t("setupDevice.serverValue", { user: server.username, host })}</dd>
        <dt>{t("setupDevice.coordinator")}</dt>
        <dd data-testid="setup-device-coordinator">{coordinator ?? t("setupDevice.noCoordinator")}</dd>
        <dt>{t("setupDevice.settings")}</dt>
        <dd data-testid="setup-device-settings">{settings === "loading" ? "…" : settingsSummary(coordinator, settings?.json, result)}</dd>
      </dl>
      <label className="setup-device-password">
        <span className="grow">
          <span>{t("setupDevice.includePassword")}</span>
          <span id="setup-device-password-desc" className={`small ${passwordOn ? "setup-device-warn" : "muted"}`}>
            {unavailable ? t("setupDevice.passwordUnavailable") : passwordOn ? t("setupDevice.includePasswordOn") : t("setupDevice.includePasswordOff")}
          </span>
        </span>
        <Switch checked={passwordOn} disabled={unavailable} onChange={setIncludePassword} aria-describedby="setup-device-password-desc" data-testid="setup-device-password" />
      </label>
      <div className="buttons">
        <button type="button" className="btn" disabled={!result || settings === "loading"} onClick={() => { if (result) { bridge().clipboard.writeText(result.link); showToast(t("setupDevice.copied")); } }} data-testid="setup-device-copy"><Icon name="copy" size={18} />{t("setupDevice.copy")}</button>
        <button type="button" className="btn primary" onClick={close} data-autofocus>{t("misc.close")}</button>
      </div>
    </>
  );
}

function settingsSummary(coordinator: string | undefined, json: string | undefined, result: SetupLinkResult | undefined): string {
  if (coordinator) return t("setupDevice.settingsSynced");
  const count = setupSettingsCount(json);
  if (count === 0) return t("setupDevice.settingsNone");
  if (result && result.codeLink !== result.link) return t("setupDevice.settingsTooMany");
  return count === 1 ? t("setupDevice.settingsCountOne") : t("setupDevice.settingsCount", { count });
}

/** [text] as a QR code: dark modules on white (scanners want a light quiet zone), crisp at any size. */
function QrCode({ text }: { text: string }) {
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
