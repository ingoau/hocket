// Settings → Connect: the identity's devices (with when each last synced, and
// Remove), Add a device, credential sync, and the coordinator: with none
// pinned, claim one with a `hocket-coordinator pair` code or join one with an
// invite; once pinned, its address, whose it is, the connection to it and
// Disconnect / Reconnect, and for the identity that claimed it, invites to
// give other people.
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
  const leftover = useApp((s) => s.connection.coordinatorUrl);
  const [url, setUrl] = useState("");
  const [code, setCode] = useState("");
  useEffect(() => {
    if (pin?.owner) {
      bridge().dispatch({ type: "listCoordinatorInvites" });
      bridge().dispatch({ type: "coordinatorYtmStatus" });
    }
  }, [pin?.owner, pin?.url]);
  const ytmSignIn = useCoordinatorYtmSignIn();
  const join = (claim: boolean) => bridge().dispatch(claim ? { type: "claimCoordinator", data: { url: url.trim(), code: code.trim() } } : { type: "redeemCoordinatorInvite", data: { url: url.trim(), code: code.trim() } });
  return (
    <>
      {/* The settings search's "coordinator" entry lands here (`setting-coordinator`). */}
      <div id="setting-coordinator" data-testid="identity-coordinator-section">
        <h2 className="section-title">{t("identity.coordinator")}</h2>
        {pin ? <CoordinatorConnection title={pin.owner ? t("identity.coordinatorOwner", { url: pin.url }) : t("identity.coordinatorGuest", { url: pin.url })} /> : (
          <>
            {/* An address without a pin (set before claiming worked, or left when the pin
                went): shown with its connection, so it can always be forgotten. */}
            {leftover ? <CoordinatorConnection title={t("identity.coordinatorLeftover", { url: leftover })} leftover /> : null}
            <div className="setting-row">
              <div className="label"><div className="desc" data-testid="identity-coordinator">{t("identity.coordinatorNone")}</div></div>
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
          </>
        )}
        {state.error ? <div className="setting-row"><div className="label"><div className="desc error" role="alert">{state.error}</div></div></div> : null}
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
                  {ytmSignIn.error ? <div className="desc error" role="alert" data-testid="identity-coord-ytm-error">{ytmSignIn.error}</div> : null}
                </div>
                <div className="control">
                  {state.ytm.signedIn ? (
                    <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "signCoordinatorInToYoutubeMusic", data: { cookies: undefined } })}>{t("identity.coordYtmSignOut")}</button>
                  ) : (
                    <button type="button" className="btn sm" disabled={ytmSignIn.busy} onClick={ytmSignIn.start} data-testid="identity-coord-ytm-sign-in">{ytmSignIn.busy ? <Icon name="spinner" size={16} className="spin" /> : null}{ytmSignIn.busy ? t("identity.coordYtmSigningIn") : t("identity.coordYtmSignIn")}</button>
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

/**
 * A coordinator address ([title] says which, and whose) with the connection to it: Disconnect or
 * Reconnect for the pinned one; for a [leftover] address with no pin behind it, Forget address,
 * which clears it.
 */
function CoordinatorConnection({ title, leftover = false }: { title: string; leftover?: boolean }) {
  const connection = useApp((s) => s.connection);
  const connected = connection.tier === "coordinator" && connection.connected;
  const status = connected
    ? t("identity.coordinatorConnected", { peers: t("connect.peers", { count: connection.peerCount }), ms: connection.roundTripMs ?? "?" })
    : t("identity.coordinatorDisconnected");
  const forget = () => {
    bridge().dispatch({ type: "setCoordinatorUrl", data: { url: undefined } });
    bridge().dispatch({ type: "disconnectCoordinator" });
  };
  return (
    <div className="setting-row" data-testid={leftover ? "identity-coordinator-leftover" : "identity-coordinator-pinned"}>
      <div className="label">
        <div className="title" data-testid={leftover ? "identity-coordinator-leftover-url" : "identity-coordinator"}>{title}</div>
        <div className="desc" data-testid="identity-coordinator-status">{status}</div>
        {connection.error ? <div className="desc error" role="alert">{connection.error}</div> : null}
      </div>
      <div className="control">
        {leftover
          ? <button type="button" className="btn sm" onClick={forget} data-testid="identity-coordinator-forget">{t("identity.coordinatorForget")}</button>
          : connected
            ? <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "disconnectCoordinator" })} data-testid="identity-coordinator-disconnect">{t("identity.coordinatorDisconnect")}</button>
            : <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "connectCoordinator" })} data-testid="identity-coordinator-reconnect">{t("identity.coordinatorReconnect")}</button>}
      </div>
    </div>
  );
}

/** The coordinator's YouTube Music sign-in: one at a time, with what went wrong shown. */
function useCoordinatorYtmSignIn() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);
  const start = () => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    bridge()
      .ytm.signInCoordinator()
      .then((r) => {
        // "signedIn": the status arrives as coordinatorYtmStatus. "busy": main focused the open window.
        if (r === "cancelled") setError(t("setup.ytm.cancelled"));
      })
      .catch((err: unknown) => {
        console.error("coordinator YouTube Music sign-in failed", err);
        setError(t("identity.coordYtmFailed"));
      })
      .finally(() => setBusy(false));
  };
  return { busy, error, start };
}
