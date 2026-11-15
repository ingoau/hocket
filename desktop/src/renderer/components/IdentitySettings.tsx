// Settings → Devices: the identity's devices (with when each last synced, and
// Remove), Add a device, credential sync, and the coordinator (claim it with
// a `hocket-coordinator pair` code, join one with an invite, and, for the
// identity that claimed it, invites to give other people).
import { useEffect, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { fmtRelative } from "../lib/format";
import { Icon } from "./Icon";
import { Switch } from "./controls";
import { clock } from "./PairDeviceDialog";

export function IdentitySettings() {
  const identity = useApp((s) => s.identity);
  const openDialog = useApp((s) => s.openDialog);
  return (
    <>
      <h2 className="section-title">{t("identity.devices")}</h2>
      {identity.removed ? (
        <div className="setting-row" role="alert" data-testid="identity-removed">
          <div className="label"><div className="desc">{t("identity.removedBanner")}</div></div>
          <div className="control"><button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "resetIdentity" })}>{t("identity.startOver")}</button></div>
        </div>
      ) : null}
      {identity.devices.map((d) => (
        <div key={d.id} className="setting-row" data-testid={`identity-device-${d.id}`}>
          <div className="label">
            <div className="title">{d.name}{d.isSelf ? <span className="badge">{t("identity.thisDevice")}</span> : null}</div>
            <div className="desc">{d.lastSynced ? t("identity.lastSynced", { when: fmtRelative(d.lastSynced) }) : t("identity.neverSynced")}</div>
          </div>
          <div className="control">
            <button type="button" className="btn sm" onClick={() => openDialog({ kind: "removeDevice", deviceId: d.id, name: d.name, self: d.isSelf })} data-testid={`identity-remove-${d.id}`}>{t("identity.remove")}</button>
          </div>
        </div>
      ))}
      <div className="setting-row">
        <div className="label" />
        <div className="control"><button type="button" className="btn" onClick={() => openDialog({ kind: "pairDevice" })} data-testid="identity-add"><Icon name="qr" size={18} />{t("identity.add")}</button></div>
      </div>
      <div className="setting-row" data-testid="identity-credential-sync">
        <div className="label">
          <div className="title">{t("identity.credentialSync")}</div>
          <div className="desc" id="identity-credential-sync-desc">{identity.credentialSync ? (identity.coordinator ? t("identity.credentialSyncCoordinator") : t("identity.credentialSyncOn")) : t("identity.credentialSyncOff")}</div>
        </div>
        <div className="control"><Switch checked={identity.credentialSync} onChange={(on) => bridge().dispatch({ type: "setCredentialSync", data: { on } })} aria-label={t("identity.credentialSync")} aria-describedby="identity-credential-sync-desc" /></div>
      </div>
      <Coordinator />
    </>
  );
}

function Coordinator() {
  const pin = useApp((s) => s.identity.coordinator);
  const state = useApp((s) => s.coordinator);
  const [url, setUrl] = useState("");
  const [code, setCode] = useState("");
  useEffect(() => {
    if (pin?.owner) {
      bridge().dispatch({ type: "listCoordinatorInvites" });
      bridge().dispatch({ type: "coordinatorYtmStatus" });
    }
  }, [pin?.owner, pin?.url]);
  const join = (claim: boolean) => bridge().dispatch(claim ? { type: "claimCoordinator", data: { url: url.trim(), code: code.trim() } } : { type: "redeemCoordinatorInvite", data: { url: url.trim(), code: code.trim() } });
  return (
    <>
      <h2 className="section-title">{t("identity.coordinator")}</h2>
      <div className="setting-row">
        <div className="label">
          <div className="desc" data-testid="identity-coordinator">{pin ? (pin.owner ? t("identity.coordinatorOwner", { url: pin.url }) : t("identity.coordinatorGuest", { url: pin.url })) : t("identity.coordinatorNone")}</div>
          {state.error ? <div className="desc error" role="alert">{state.error}</div> : null}
        </div>
      </div>
      <div className="setting-row">
        <div className="label" />
        <div className="control">
          <input className="input" placeholder="wss://coordinator.example.org/ws" aria-label={t("identity.coordinatorUrl")} value={url} onChange={(e) => setUrl(e.target.value)} style={{ width: 220 }} data-testid="identity-coordinator-url" />
          <input className="input" placeholder="ABCD-EFGH" aria-label={t("identity.coordinatorCode")} value={code} onChange={(e) => setCode(e.target.value)} style={{ width: 110 }} data-testid="identity-coordinator-code" />
          <button type="button" className="btn sm" disabled={!code.trim()} onClick={() => join(true)} data-testid="identity-claim">{t("identity.claim")}</button>
          <button type="button" className="btn sm" disabled={!code.trim() || !url.trim()} onClick={() => join(false)} data-testid="identity-redeem">{t("identity.redeem")}</button>
        </div>
      </div>
      {pin?.owner ? (
        <>
          <h2 className="section-title">{t("identity.invites")}</h2>
          <p className="muted small">{t("identity.invitesHint")}</p>
          {state.invites.length === 0 ? <p className="muted small">{t("identity.invitesNone")}</p> : null}
          {state.invites.map((i) => (
            <div key={i.code} className="setting-row" data-testid={`identity-invite-${i.code}`}>
              <div className="label">
                <div className="title"><code>{i.code}</code>{i.code === state.created ? <span className="badge ok">new</span> : null}</div>
                <div className="desc">{i.redeemed ? t("identity.inviteUsed") : t("identity.inviteUntil", { time: `${new Date(i.expiresAt).toLocaleDateString()} ${clock(i.expiresAt)}` })}</div>
              </div>
              <div className="control">{i.redeemed ? null : <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "revokeCoordinatorInvite", data: { code: i.code } })}>{t("identity.inviteRevoke")}</button>}</div>
            </div>
          ))}
          <div className="setting-row">
            <div className="label" />
            <div className="control">
              <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "listCoordinatorInvites" })}>{t("identity.invitesRefresh")}</button>
              <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "createCoordinatorInvite" })} data-testid="identity-invite-create">{t("identity.inviteCreate")}</button>
            </div>
          </div>
          {state.ytm?.enabled ? (
            <>
              <h2 className="section-title">{t("identity.coordYtm")}</h2>
              <div className="setting-row">
                <div className="label">
                  <div className="desc" data-testid="identity-coord-ytm">{state.ytm.signedIn ? t("identity.coordYtmSignedIn", { name: state.ytm.account ?? "" }) : t("identity.coordYtmAnon")}</div>
                  {state.ytm.error ? <div className="desc error" role="alert">{state.ytm.error}</div> : null}
                </div>
                <div className="control">
                  {state.ytm.signedIn ? (
                    <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "signCoordinatorInToYoutubeMusic", data: { cookies: undefined } })}>{t("identity.coordYtmSignOut")}</button>
                  ) : (
                    <button type="button" className="btn sm" onClick={() => void window.hocket.ytm.signInCoordinator()} data-testid="identity-coord-ytm-sign-in">{t("identity.coordYtmSignIn")}</button>
                  )}
                </div>
              </div>
            </>
          ) : null}
        </>
      ) : null}
    </>
  );
}
