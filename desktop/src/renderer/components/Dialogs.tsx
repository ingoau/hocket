// Modal dialogs: prompt, confirm, add-to-playlist, sleep timer, Connect picker, track info.
import { useEffect, useRef, useState } from "react";
import type { Album, DeviceInfo } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { fmtDate, fmtDuration, fmtRelative, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { deviceIcon } from "./ConnectPill";
import { Slider } from "./controls";
import { useVolumeControl } from "../lib/volume";
import { SongInfo } from "./SongInfo";
import { trapTab, useReturnFocus } from "../lib/focus";
import { usePresence } from "../lib/presence";

export function Dialogs() {
  const live = useApp((s) => s.dialog);
  const close = useApp((s) => s.closeDialog);
  const ref = useRef<HTMLDivElement>(null);
  // The closed dialog stays mounted, inert, while its exit plays; focus goes
  // back and the Connect picker closes on the live state, not the animation.
  const { value: dialog, closing, motion, exitProps } = usePresence(live, ref);
  useReturnFocus(!!live);
  useEffect(() => {
    if (!live) return;
    // Initial focus: the field, else the safe (non-destructive) button, else the first control.
    const root = ref.current;
    const first = root?.querySelector<HTMLElement>("input, textarea, select") ?? root?.querySelector<HTMLElement>("[data-autofocus]") ?? root?.querySelector<HTMLElement>("button, [tabindex='0']");
    first?.focus();
    if (live.kind === "connect") return () => bridge().dispatch({ type: "closeHandoffPicker" });
    return undefined;
  }, [live]);
  if (!dialog) return null;
  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      return;
    }
    trapTab(e);
  };
  return (
    <div className={`overlay ${dialog.kind === "connect" ? "overlay-popover" : ""} ${closing ? "closing" : ""}`} inert={closing} onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div {...exitProps} className={`dialog ${dialog.kind === "trackInfo" ? "dialog-wide" : ""} ${dialog.kind === "connect" ? "dialog-popover connect-popover" : ""} ${motion}`} role={dialog.kind === "confirm" ? "alertdialog" : "dialog"} aria-modal="true" aria-labelledby="dialog-title" aria-describedby={dialog.kind === "confirm" ? "dialog-message" : undefined} onKeyDown={onKey} data-testid={`dialog-${dialog.kind}`}>
        {dialog.kind === "prompt" ? <Prompt {...dialog} /> : null}
        {dialog.kind === "confirm" ? <Confirm {...dialog} /> : null}
        {dialog.kind === "sleepTimer" ? <SleepTimerDialog /> : null}
        {dialog.kind === "connect" ? <ConnectDialog /> : null}
        {dialog.kind === "trackInfo" ? <SongInfo trackId={dialog.trackId} /> : null}
        {dialog.kind === "albumInfo" ? <AlbumInfo albumId={dialog.albumId} /> : null}
      </div>
    </div>
  );
}

function Prompt({ title, label, initial, confirmLabel, onConfirm }: { title: string; label: string; initial?: string; confirmLabel?: string; onConfirm: (v: string) => void }) {
  const close = useApp((s) => s.closeDialog);
  const [value, setValue] = useState(initial ?? "");
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!value.trim()) return;
    onConfirm(value.trim());
    close();
  };
  return (
    <form onSubmit={submit}>
      <h2 className="dialog-title" id="dialog-title">{title}</h2>
      <label style={{ marginTop: 10 }}>
        {label}
        <input className="input" value={value} onChange={(e) => setValue(e.target.value)} data-testid="prompt-input" />
      </label>
      <div className="buttons">
        <button type="button" className="btn" onClick={close}>{t("dialog.cancel")}</button>
        <button type="submit" className="btn primary" disabled={!value.trim()}>{confirmLabel ?? t("dialog.ok")}</button>
      </div>
    </form>
  );
}

function Confirm({ title, message, confirmLabel, destructive, onConfirm }: { title: string; message: string; confirmLabel?: string; destructive?: boolean; onConfirm: () => void }) {
  const close = useApp((s) => s.closeDialog);
  return (
    <>
      <h2 className="dialog-title" id="dialog-title">{title}</h2>
      <div id="dialog-message">{message}</div>
      <div className="buttons">
        <button type="button" className="btn" onClick={close} data-autofocus>{t("dialog.cancel")}</button>
        <button type="button" className={`btn ${destructive ? "danger" : "primary"}`} onClick={() => { onConfirm(); close(); }} data-testid="confirm-ok">{confirmLabel ?? t("dialog.confirm")}</button>
      </div>
    </>
  );
}

function SleepTimerDialog() {
  const close = useApp((s) => s.closeDialog);
  const timer = useApp((s) => s.sleepTimer);
  const set = (minutes: number | undefined, endOfTrack = false) => {
    bridge().dispatch({ type: "setSleepTimer", data: { timer: minutes === undefined && !endOfTrack ? undefined : { endsAt: minutes !== undefined ? Date.now() + minutes * 60_000 : undefined, stopAtEndOfTrack: endOfTrack } } });
    close();
  };
  return (
    <>
      <h2 className="dialog-title" id="dialog-title">{t("player.sleepTimer")}</h2>
      {timer ? <div className="muted small">{timer.endsAt ? t("player.sleepTimerActive", { remaining: fmtTime(Math.max(0, timer.endsAt - Date.now())) }) : t("player.sleepTimerEndOfTrack")}</div> : null}
      <ul className="list plain-list">
        <li><button type="button" className="li" onClick={() => set(undefined)}>{t("player.sleepTimerOff")}</button></li>
        {[15, 30, 45, 60, 90].map((m) => (
          <li key={m}><button type="button" className="li" onClick={() => set(m)}>{t("player.sleepTimerMinutes", { minutes: m })}</button></li>
        ))}
        <li><button type="button" className="li" onClick={() => set(undefined, true)}>{t("player.sleepTimerEndOfTrack")}</button></li>
      </ul>
      <div className="buttons">
        <button type="button" className="btn" onClick={close}>{t("dialog.cancel")}</button>
      </div>
    </>
  );
}

// Play on: this computer (one row, whatever it outputs to), then the other
// devices signed in to the server, each saying whether it is playing, ready to
// take over, still preparing, or when it was last seen.
function ConnectDialog() {
  const close = useApp((s) => s.closeDialog);
  const devices = useApp((s) => s.devices);
  const handoff = useApp((s) => s.handoff);
  const connection = useApp((s) => s.connection);
  const navigate = useApp((s) => s.navigate);
  const others = devices.filter((d) => !d.isSelf);
  const self = devices.find((d) => d.isSelf);
  const pick = (id: string) => {
    bridge().dispatch({ type: "handoffTo", data: { device_id: id } });
    close();
  };
  const tier = connection.tier === "coordinator" ? t("connect.tier.coordinator") : connection.tier === "lan" ? t("connect.tier.lan") : t("connect.tier.local");
  const row = (d: DeviceInfo, name: string, sub: string, trailing: React.ReactNode, testId?: string) => (
    <li key={d.id}>
      <button type="button" className={`connect-row ${d.playing ? "on" : ""}`} aria-current={d.playing || undefined} onClick={() => pick(d.id)} data-testid={testId}>
        <span className="connect-row-icon"><Icon name={d.isSelf ? "laptop" : deviceIcon(d.platform)} size={22} /></span>
        <span className="grow" style={{ minWidth: 0 }}>
          <span className="truncate connect-row-name">{name}</span>
          <span className="truncate connect-row-sub">{sub}</span>
        </span>
        {d.playing ? <Icon name="equalizer" size={22} /> : trailing}
      </button>
    </li>
  );
  return (
    <>
      <div className="connect-head">
        <h2 className="dialog-title" id="dialog-title">{t("connect.title")}</h2>
        <button type="button" className="btn icon" aria-label={t("misc.close")} title={t("misc.close")} onClick={close} data-testid="connect-close"><Icon name="close" size={22} /></button>
      </div>
      <ConnectVolume />
      <ul className="plain-list connect-group" data-testid="connect-self">
        {self ? row(self, t("connect.thisComputer"), self.playing ? t("connect.playing") : t("connect.playHere"), null) : null}
      </ul>
      <div className="connect-heading">
        <h3>{t("connect.others")}</h3>
        <span className="small muted">{tier}</span>
      </div>
      {others.length ? (
        <ul className="plain-list connect-group">
          {others.map((d) => {
            const target = handoff.targets.find((x) => x.id === d.id) ?? d;
            const preparing = !d.playing && !target.ready && handoff.open;
            const sub = d.playing ? t("connect.playing") : target.ready ? t("connect.ready") : preparing ? t("connect.preparing") : t("connect.seen", { when: fmtRelative(d.lastSeen) });
            const trailing = preparing ? <Icon name="spinner" size={20} className="spin" /> : target.ready && d.lastSeen ? <span className="small muted connect-seen">{t("connect.seen", { when: fmtRelative(d.lastSeen) })}</span> : null;
            return row(d, d.name, sub, trailing, "device-choice");
          })}
        </ul>
      ) : <p className="small muted connect-empty">{t("connect.noDevices")}</p>}
      <button type="button" className="btn tonal connect-settings" onClick={() => { close(); navigate({ view: "settings", param: "connect" }); }} data-testid="connect-settings"><Icon name="devices" size={20} />{t("connect.settings")}</button>
    </>
  );
}

/** The volume of whatever is playing, as in the player bar, with its level. */
function ConnectVolume() {
  const v = useVolumeControl();
  const silent = v.muted || v.shown === 0;
  return (
    <div className="connect-volume" role="group" aria-labelledby="connect-volume-label">
      <div id="connect-volume-label" className="connect-volume-label">{t("connect.volume")}</div>
      <div className="connect-volume-row">
        <button type="button" className="btn icon sm" aria-label={t("player.mute")} aria-pressed={v.muted} title={v.muted ? t("a11y.unmute") : t("player.mute")} disabled={v.unknown} onClick={v.toggleMute}><Icon name={silent ? "mute" : "volume"} size={20} /></button>
        <Slider min={0} max={1} step={0.01} value={v.shown} disabled={v.unknown} aria-label={v.label} aria-valuetext={v.valueText} onKeyDown={v.onKeyDown} onChange={v.set} onWheel={v.onWheel} className="grow" data-testid="connect-volume" />
        <span className="connect-volume-pct">{v.unknown ? "–" : `${Math.round(v.shown * 100)}%`}</span>
      </div>
    </div>
  );
}

/** "Album info" in an album's menu: the same details Android's album sheet shows. */
function AlbumInfo({ albumId }: { albumId: string }) {
  const close = useApp((s) => s.closeDialog);
  const { data } = useQuery(() => ({ type: "album", data: { id: albumId } }), "albumDetail", [albumId]);
  const al = data as Album | undefined;
  if (!al) return <><h2 className="dialog-title" id="dialog-title">{t("misc.loading")}</h2></>;
  const rows: [string, string | number | undefined][] = [
    ["Artist", al.artist], ["Year", al.year || undefined], ["Genre", al.genre], ["Songs", al.songCount], ["Duration", fmtDuration(al.durationMs)],
    ["Plays", al.playCount], ["Added", fmtDate(al.created)], ["Last played", fmtDate(al.lastPlayed)],
    ["ReplayGain", al.replayGain?.albumGainDb !== undefined ? `${al.replayGain.albumGainDb.toFixed(1)} dB` : undefined],
    ["Offline", al.offline !== "none" ? al.offline : undefined],
  ];
  const artistId = al.artistId;
  return (
    <>
      <div className="row">
        <Artwork id={al.coverArt} size={64} className="art" />
        <div className="grow">
          <h2 className="dialog-title truncate" id="dialog-title">{al.name}</h2>
          <div className="muted truncate">{al.artist}</div>
        </div>
      </div>
      <div className="list" style={{ padding: 6 }} tabIndex={0} role="group" aria-labelledby="dialog-title">
      <dl className="info-list">
        {rows.filter(([, v]) => v !== undefined && v !== "").map(([k, v]) => (
          <div key={k} className="row small" style={{ padding: "2px 4px" }}>
            <dt className="muted" style={{ width: 90, flex: "0 0 auto" }}>{k}</dt>
            <dd className="grow truncate" title={String(v)}>{String(v)}</dd>
          </div>
        ))}
      </dl>
      </div>
      <div className="buttons">
        {artistId ? <button type="button" className="btn" onClick={() => { close(); useApp.getState().navigate({ view: "artist", id: artistId }); }}><Icon name="artist" size={14} /> {t("action.goToArtist")}</button> : null}
        <button type="button" className="btn" onClick={close}>{t("misc.close")}</button>
      </div>
    </>
  );
}
