// Modal dialogs: prompt, confirm, add-to-playlist, sleep timer, Connect picker, track info.
import { useEffect, useRef, useState } from "react";
import type { Playlist, Track } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { fmtBytes, fmtDate, fmtTime } from "../lib/format";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";

export function Dialogs() {
  const dialog = useApp((s) => s.dialog);
  const close = useApp((s) => s.closeDialog);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!dialog) return;
    const first = ref.current?.querySelector<HTMLElement>("input, button, [tabindex]");
    first?.focus();
    if (dialog.kind === "connect") return () => bridge().dispatch({ type: "closeHandoffPicker" });
    return undefined;
  }, [dialog]);
  if (!dialog) return null;
  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Escape") close();
  };
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && close()} onKeyDown={onKey}>
      <div ref={ref} className="dialog fade-in" role="dialog" aria-modal="true" data-testid={`dialog-${dialog.kind}`}>
        {dialog.kind === "prompt" ? <Prompt {...dialog} /> : null}
        {dialog.kind === "confirm" ? <Confirm {...dialog} /> : null}
        {dialog.kind === "addToPlaylist" ? <AddToPlaylist trackIds={dialog.trackIds} /> : null}
        {dialog.kind === "sleepTimer" ? <SleepTimerDialog /> : null}
        {dialog.kind === "connect" ? <ConnectDialog /> : null}
        {dialog.kind === "trackInfo" ? <TrackInfo trackId={dialog.trackId} /> : null}
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
      <h3>{title}</h3>
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
      <h3>{title}</h3>
      <div>{message}</div>
      <div className="buttons">
        <button type="button" className="btn" onClick={close}>{t("dialog.cancel")}</button>
        <button type="button" className={`btn ${destructive ? "danger" : "primary"}`} onClick={() => { onConfirm(); close(); }} data-testid="confirm-ok">{confirmLabel ?? t("dialog.confirm")}</button>
      </div>
    </>
  );
}

function AddToPlaylist({ trackIds }: { trackIds: string[] }) {
  const close = useApp((s) => s.closeDialog);
  const serverId = useApp((s) => s.servers[0]?.id);
  const { data } = useQuery(() => (serverId ? { type: "playlists", data: { server_id: serverId } } : null), "playlists", [serverId]);
  const editable = (data ?? []).filter((p: Playlist) => !p.isSmart && p.isMine);
  const add = (p: Playlist) => {
    bridge().dispatch({ type: "playlistAdd", data: { playlist_id: p.id, track_ids: trackIds, at_index: undefined } });
    close();
  };
  const create = () => {
    close();
    useApp.getState().openDialog({ kind: "prompt", title: t("dialog.newPlaylist"), label: t("dialog.playlistName"), onConfirm: (name) => serverId && bridge().dispatch({ type: "createPlaylist", data: { server_id: serverId, name, track_ids: trackIds } }) });
  };
  return (
    <>
      <h3>{t("dialog.addToPlaylist")}</h3>
      <div className="muted small">{t("misc.tracks", { count: trackIds.length })}</div>
      <div className="list">
        <div className="li" onClick={create} role="button" tabIndex={0} onKeyDown={(e) => e.key === "Enter" && create()}>
          <Icon name="plus" size={14} /> {t("action.newPlaylist")}
        </div>
        {editable.map((p: Playlist) => (
          <div key={p.id} className="li" role="button" tabIndex={0} onClick={() => add(p)} onKeyDown={(e) => e.key === "Enter" && add(p)} data-testid="playlist-choice">
            <Icon name="playlist" size={14} />
            <span className="grow truncate">{p.name}</span>
            <span className="faint small">{p.songCount}</span>
          </div>
        ))}
      </div>
      <div className="buttons">
        <button type="button" className="btn" onClick={close}>{t("dialog.cancel")}</button>
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
      <h3>{t("player.sleepTimer")}</h3>
      {timer ? <div className="muted small">{timer.endsAt ? t("player.sleepTimerActive", { remaining: fmtTime(Math.max(0, timer.endsAt - Date.now())) }) : t("player.sleepTimerEndOfTrack")}</div> : null}
      <div className="list">
        <div className="li" role="button" tabIndex={0} onClick={() => set(undefined)}>{t("player.sleepTimerOff")}</div>
        {[15, 30, 45, 60, 90].map((m) => (
          <div key={m} className="li" role="button" tabIndex={0} onClick={() => set(m)}>{t("player.sleepTimerMinutes", { minutes: m })}</div>
        ))}
        <div className="li" role="button" tabIndex={0} onClick={() => set(undefined, true)}>{t("player.sleepTimerEndOfTrack")}</div>
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
      <h3>{t("connect.title")}</h3>
      <div className="muted small">{tier}{connection.peerCount ? ` · ${t("connect.peers", { count: connection.peerCount })}` : ""}</div>
      <div className="list">
        {self ? (
          <div className="li" role="button" tabIndex={0} onClick={() => pick(self.id)}>
            <Icon name="devices" size={14} />
            <span className="grow">{self.name} <span className="faint">· {t("connect.thisDevice")}</span></span>
            {self.playing ? <span className="badge ok">{t("connect.playing")}</span> : null}
          </div>
        ) : null}
        {others.map((d) => {
          const target = handoff.targets.find((x) => x.id === d.id) ?? d;
          return (
            <div key={d.id} className="li" role="button" tabIndex={0} onClick={() => pick(d.id)} data-testid="device-choice">
              <Icon name="devices" size={14} />
              <span className="grow">{d.name} <span className="faint">· {d.platform}</span></span>
              {d.playing ? <span className="badge ok">{t("connect.playing")}</span> : target.ready ? <span className="badge synced">{t("connect.ready")}</span> : handoff.open ? <span className="badge">{t("connect.preparing")}</span> : null}
            </div>
          );
        })}
        {!others.length ? <div className="li muted" style={{ height: "auto", padding: 10 }}>{t("connect.noDevices")}</div> : null}
      </div>
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
  if (!tr) return <div className="muted">{t("misc.loading")}</div>;
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
          <h3 className="truncate">{tr.title}</h3>
          <div className="muted truncate">{tr.artist}</div>
        </div>
      </div>
      <div className="list" style={{ padding: 6 }}>
        {rows.filter(([, v]) => v !== undefined && v !== "").map(([k, v]) => (
          <div key={k} className="row small" style={{ padding: "2px 4px" }}>
            <span className="muted" style={{ width: 90, flex: "0 0 auto" }}>{k}</span>
            <span className="grow truncate" title={String(v)}>{String(v)}</span>
          </div>
        ))}
      </div>
      <div className="buttons">
        {tr.path ? <button type="button" className="btn" onClick={() => bridge().shell.showItemInFolder(tr.path as string)}><Icon name="folder" size={14} /> {t("action.showInFolder")}</button> : null}
        <button type="button" className="btn" onClick={close}>{t("misc.close")}</button>
      </div>
    </>
  );
}
