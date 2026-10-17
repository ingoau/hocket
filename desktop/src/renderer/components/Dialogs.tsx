// Modal dialogs: prompt, confirm, add-to-playlist, sleep timer, Connect picker, track info.
import { useEffect, useRef, useState } from "react";
import type { Album, Track } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { fmtBytes, fmtDate, fmtDuration, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
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
    <div className={`overlay ${closing ? "closing" : ""}`} inert={closing} onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div {...exitProps} className={`dialog ${motion}`} role={dialog.kind === "confirm" ? "alertdialog" : "dialog"} aria-modal="true" aria-labelledby="dialog-title" aria-describedby={dialog.kind === "confirm" ? "dialog-message" : undefined} onKeyDown={onKey} data-testid={`dialog-${dialog.kind}`}>
        {dialog.kind === "prompt" ? <Prompt {...dialog} /> : null}
        {dialog.kind === "confirm" ? <Confirm {...dialog} /> : null}
        {dialog.kind === "sleepTimer" ? <SleepTimerDialog /> : null}
        {dialog.kind === "connect" ? <ConnectDialog /> : null}
        {dialog.kind === "trackInfo" ? <TrackInfo trackId={dialog.trackId} /> : null}
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

function ConnectDialog() {
  const close = useApp((s) => s.closeDialog);
  const devices = useApp((s) => s.devices);
  const handoff = useApp((s) => s.handoff);
  const connection = useApp((s) => s.connection);
  const others = devices.filter((d) => !d.isSelf);
  const self = devices.find((d) => d.isSelf);
  const pick = (id: string) => {
    bridge().dispatch({ type: "handoffTo", data: { device_id: id } });
    close();
  };
  const tier = connection.tier === "coordinator" ? t("connect.tier.coordinator") : connection.tier === "lan" ? t("connect.tier.lan") : t("connect.tier.local");
  return (
    <>
      <h2 className="dialog-title" id="dialog-title">{t("connect.title")}</h2>
      <div className="muted small">{tier}{connection.peerCount ? ` · ${t("connect.peers", { count: connection.peerCount })}` : ""}</div>
      <ul className="list plain-list">
        {self ? (
          <li><button type="button" className="li" onClick={() => pick(self.id)}>
            <Icon name="devices" size={14} />
            <span className="grow">{self.name} <span className="faint">· {t("connect.thisDevice")}</span></span>
            {self.playing ? <span className="badge ok">{t("connect.playing")}</span> : null}
          </button></li>
        ) : null}
        {others.map((d) => {
          const target = handoff.targets.find((x) => x.id === d.id) ?? d;
          return (
            <li key={d.id}><button type="button" className="li" onClick={() => pick(d.id)} data-testid="device-choice">
              <Icon name="devices" size={14} />
              <span className="grow">{d.name} <span className="faint">· {d.platform}</span></span>
              {d.playing ? <span className="badge ok">{t("connect.playing")}</span> : target.ready ? <span className="badge synced">{t("connect.ready")}</span> : handoff.open ? <span className="badge">{t("connect.preparing")}</span> : null}
            </button></li>
          );
        })}
        {!others.length ? <li className="li muted" style={{ height: "auto", padding: 10 }}>{t("connect.noDevices")}</li> : null}
      </ul>
      <div className="buttons">
        <button type="button" className="btn" onClick={close}>{t("misc.close")}</button>
      </div>
    </>
  );
}

function TrackInfo({ trackId }: { trackId: string }) {
  const close = useApp((s) => s.closeDialog);
  const { data } = useQuery(() => ({ type: "track", data: { id: trackId } }), "trackDetail", [trackId]);
  const tr = data as Track | undefined;
  if (!tr) return <><h2 className="dialog-title" id="dialog-title">{t("misc.loading")}</h2></>;
  const rows: [string, string | number | undefined][] = [
    ["Title", tr.title], ["Artist", tr.artist], ["Album", tr.album], ["Album artist", tr.albumArtist], ["Year", tr.year], ["Genre", tr.genre],
    ["Track", tr.trackNumber !== undefined ? `${tr.discNumber ?? 1}-${tr.trackNumber}` : undefined], ["Duration", fmtTime(tr.durationMs)],
    ["Format", tr.suffix ? `${tr.suffix.toUpperCase()} · ${tr.bitRate ?? "?"} kbps · ${tr.sampleRate ?? "?"} Hz · ${tr.bitDepth ?? "?"} bit` : undefined],
    ["Size", fmtBytes(tr.sizeBytes)], ["Path", tr.path], ["Plays", tr.playCount], ["Last played", fmtDate(tr.lastPlayed)], ["Added", fmtDate(tr.created)],
    ["ReplayGain", tr.replayGain?.trackGainDb !== undefined ? `track ${tr.replayGain.trackGainDb.toFixed(1)} dB · album ${tr.replayGain.albumGainDb?.toFixed(1) ?? "–"} dB` : undefined],
    ["BPM", tr.sonic?.bpm], ["Key", tr.sonic?.key], ["Energy", tr.sonic?.energy !== undefined ? Math.round(tr.sonic.energy * 100) + "%" : undefined], ["Mood", tr.sonic?.mood],
    ["Offline", tr.offline !== "none" ? tr.offline : undefined],
  ];
  return (
    <>
      <div className="row">
        <Artwork id={tr.coverArt} size={64} className="art" />
        <div className="grow">
          <h2 className="dialog-title truncate" id="dialog-title">{tr.title}</h2>
          <div className="muted truncate">{tr.artist}</div>
        </div>
      </div>
      {/* Scrolls when long: focusable so the keyboard can scroll it. */}
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
        {tr.path ? <button type="button" className="btn" onClick={() => bridge().shell.showItemInFolder(tr.path as string)}><Icon name="folder" size={14} /> {t("action.showInFolder")}</button> : null}
        <button type="button" className="btn" onClick={close}>{t("misc.close")}</button>
      </div>
    </>
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
