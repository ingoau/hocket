// Settings: every scoped setting with a synced/local badge, audio (ReplayGain,
// EQ canvas, output device, exclusive, gapless), transcoding, Connect,
// storage, battery saver, lyrics, appearance, customisation (choose-and-order
// for sidebar / context menu / media session), keyboard shortcuts editor,
// config backup, diagnostics, about.
import { useEffect, useMemo, useRef, useState } from "react";
import type { ActionDescriptor, AudioSettings, EqBand, QueueMode, ReplayGainMode, Setting } from "@core/api";
import { t } from "@shared/strings";
import { useApp, useSetting } from "../store/app";
import { useQuery } from "../store/queries";
import { bridge } from "../core/bridge";
import { Icon } from "../components/Icon";
import { fmtBytes, fmtRelative } from "../lib/format";
import { chordFromEvent, chordToString, formatChord, parseChord } from "../store/shortcuts";
import { useKeymap } from "../store/keyboard";
import { DEFAULT_KEYMAP } from "@shared/keymap";
import { executeAction } from "../store/actions";

const SECTIONS = ["general", "audio", "transcoding", "connect", "storage", "lyrics", "appearance", "customisation", "shortcuts", "backup", "diagnostics", "about"] as const;
type Section = (typeof SECTIONS)[number];

export function Settings({ section }: { section?: string }) {
  const navigate = useApp((s) => s.navigate);
  const current: Section = (SECTIONS as readonly string[]).includes(section ?? "") ? (section as Section) : "general";
  return (
    <div className="view" data-testid="view-settings">
      <div className="settings">
        <nav className="settings-nav" aria-label={t("settings.title")}>
          {SECTIONS.map((s) => (
            <a key={s} href="#" className={`nav-item ${current === s ? "active" : ""}`} onClick={(e) => { e.preventDefault(); navigate({ view: "settings", param: s }, true); }} data-testid={`settings-nav-${s}`}>{t(`settings.section.${s}` as never)}</a>
          ))}
        </nav>
        <div className="settings-body">
          <h2>{t(`settings.section.${current}` as never)}</h2>
          {current === "general" ? <General /> : null}
          {current === "audio" ? <Audio /> : null}
          {current === "transcoding" ? <Transcoding /> : null}
          {current === "connect" ? <Connect /> : null}
          {current === "storage" ? <Storage /> : null}
          {current === "lyrics" ? <LyricsSettings /> : null}
          {current === "appearance" ? <Appearance /> : null}
          {current === "customisation" ? <Customisation /> : null}
          {current === "shortcuts" ? <Shortcuts /> : null}
          {current === "backup" ? <Backup /> : null}
          {current === "diagnostics" ? <Diagnostics /> : null}
          {current === "about" ? <About /> : null}
        </div>
      </div>
    </div>
  );
}

function ScopeBadge({ settingKey }: { settingKey: string }) {
  const s = useApp((st) => st.settings[settingKey]);
  const scope = s?.scope ?? "deviceLocal";
  return <span className={`badge ${scope === "accountSynced" ? "synced" : ""}`} title={scope}>{scope === "accountSynced" ? t("settings.scope.synced") : t("settings.scope.local")}</span>;
}

function Row({ title, desc, settingKey, children, stacked = false }: { title: string; desc?: string; settingKey?: string; children: React.ReactNode; stacked?: boolean }) {
  return (
    <div className={`setting-row ${stacked ? "stacked" : ""}`}>
      <div className="label">
        <div className="title">{title}{settingKey ? <ScopeBadge settingKey={settingKey} /> : null}</div>
        {desc ? <div className="desc">{desc}</div> : null}
      </div>
      <div className="control">{children}</div>
    </div>
  );
}

function Toggle({ settingKey, title, desc }: { settingKey: string; title: string; desc?: string }) {
  const value = useSetting(settingKey, false);
  const set = useApp((s) => s.setSetting);
  return (
    <Row title={title} desc={desc} settingKey={settingKey}>
      <input type="checkbox" checked={!!value} onChange={(e) => set(settingKey, e.target.checked)} aria-label={title} data-testid={`setting-${settingKey}`} />
    </Row>
  );
}

function General() {
  const servers = useApp((s) => s.servers);
  const cap = useSetting("queue.savedCap", 10);
  const set = useApp((s) => s.setSetting);
  const openDialog = useApp((s) => s.openDialog);
  const threshold = useSetting("ratings.loveThreshold", 0);
  return (
    <>
      <div className="section-title">{t("settings.servers")}</div>
      {servers.map((s) => (
        <div key={s.id} className="setting-row" data-testid="server-row">
          <div className="label">
            <div className="title">{s.name} <span className="muted small">{s.url} · {s.username}</span></div>
            <div className="desc">{s.capabilities.serverVersion ? t("settings.serverVersion", { version: s.capabilities.serverVersion }) : ""}{!s.capabilities.meetsFloor ? ` · ${t("settings.serverBelowFloor")}` : ""}{!s.reachable ? ` · ${t("settings.serverUnreachable")}` : ""}{s.lastSync ? ` · ${t("sync.done")} ${fmtRelative(s.lastSync)}` : ""}</div>
          </div>
          <div className="control">
            <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "syncLibrary", data: { server_id: s.id, full: false } })}>{t("settings.serverResync")}</button>
            <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "syncLibrary", data: { server_id: s.id, full: true } })}>{t("settings.serverFullResync")}</button>
            <button type="button" className="btn sm danger" onClick={() => openDialog({ kind: "confirm", title: t("settings.serverRemove"), message: t("dialog.removeServer", { name: s.name }), destructive: true, confirmLabel: t("dialog.delete"), onConfirm: () => bridge().dispatch({ type: "removeServer", data: { server_id: s.id } }) })}>{t("settings.serverRemove")}</button>
          </div>
        </div>
      ))}
      <Toggle settingKey="general.closeToTray" title={t("settings.closeToTray")} />
      <Toggle settingKey="sync.enabled" title={t("settings.settingsSync")} />
      <Row title={t("settings.savedQueueCap", { n: cap })} settingKey="queue.savedCap">
        <input type="range" min={0} max={50} value={cap} onChange={(e) => { set("queue.savedCap", Number(e.target.value)); bridge().dispatch({ type: "setSavedQueueCap", data: { cap: Number(e.target.value) } }); }} aria-label={t("settings.savedQueueCap", { n: cap })} />
        <span className="mono small" style={{ width: 24 }}>{cap}</span>
      </Row>
      <Row title={t("settings.ratingBridge")} settingKey="ratings.loveThreshold">
        <select className="select" value={threshold} onChange={(e) => set("ratings.loveThreshold", Number(e.target.value))}>
          <option value={0}>{t("settings.ratingBridgeOff")}</option>
          {[3, 4, 5].map((n) => <option key={n} value={n}>{"★".repeat(n)}</option>)}
        </select>
      </Row>
      <QueueModeRow />
    </>
  );
}

function QueueModeRow() {
  const mode = useApp((s) => s.queue.mode);
  return (
    <Row title={t("settings.queueMode")} settingKey="queue.mode">
      <select className="select" value={mode} onChange={(e) => bridge().dispatch({ type: "setQueueMode", data: { mode: e.target.value as QueueMode } })}>
        <option value="apple">{t("queue.mode.apple")}</option>
        <option value="youTube">{t("queue.mode.youtube")}</option>
      </select>
    </Row>
  );
}

function Audio() {
  const audio = useApp((s) => s.audio);
  const devices = useApp((s) => s.outputDevices);
  const { data: fetchedDevices } = useQuery(() => ({ type: "outputDevices" }), "outputDevices", [], { static: true });
  const list = devices.length ? devices : (fetchedDevices ?? []);
  const update = (patch: Partial<AudioSettings>) => bridge().dispatch({ type: "setAudioSettings", data: { settings: { ...audio, ...patch } } });
  const bands = audio.eq.bands.length ? audio.eq.bands : [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000].map((f) => ({ frequencyHz: f, gainDb: 0, q: 1.1 }));
  const setBands = (b: EqBand[], preset?: string) => update({ eq: { ...audio.eq, bands: b, preset: preset ?? undefined } });
  const PRESETS: Record<string, number[]> = { flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], bass: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0], treble: [0, 0, 0, 0, 0, 1, 2, 4, 5, 6], vocal: [-2, -1, 0, 2, 4, 4, 3, 1, 0, -1], loudness: [5, 4, 2, 0, -1, -1, 0, 2, 4, 5] };
  return (
    <>
      <Row title={t("settings.replayGain")} settingKey="audio.replayGain">
        <select className="select" value={audio.replayGain} onChange={(e) => update({ replayGain: e.target.value as ReplayGainMode })} data-testid="setting-replaygain">
          {(["off", "track", "album", "auto"] as ReplayGainMode[]).map((m) => <option key={m} value={m}>{t(`settings.replayGain.${m}` as never)}</option>)}
        </select>
      </Row>
      <Row title={t("settings.replayGainPreamp")} settingKey="audio.replayGain">
        <input type="range" min={-15} max={15} step={0.5} value={audio.replayGainPreampDb} onChange={(e) => update({ replayGainPreampDb: Number(e.target.value) })} aria-label={t("settings.replayGainPreamp")} />
        <span className="mono small" style={{ width: 52 }}>{audio.replayGainPreampDb.toFixed(1)} dB</span>
      </Row>
      <Row title={t("settings.normalisation")} settingKey="audio.replayGain"><input type="checkbox" checked={audio.normalisation} onChange={(e) => update({ normalisation: e.target.checked })} aria-label={t("settings.normalisation")} /></Row>
      <Row title={t("settings.gapless")} settingKey="audio.gapless"><input type="checkbox" checked={audio.gapless} onChange={(e) => update({ gapless: e.target.checked })} aria-label={t("settings.gapless")} /></Row>
      <Row title={t("settings.outputDevice")} settingKey="audio.outputDevice">
        <select className="select" value={audio.outputDevice ?? ""} onChange={(e) => bridge().dispatch({ type: "setOutputDevice", data: { id: e.target.value || undefined } })} data-testid="setting-output">
          <option value="">{t("settings.outputDefault")}</option>
          {list.map((d) => <option key={d.id} value={d.id}>{d.name}{d.isDefault ? " ★" : ""}</option>)}
        </select>
        <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "refreshOutputDevices" })}>{t("settings.refreshDevices")}</button>
      </Row>
      <Row title={t("settings.exclusive")} settingKey="audio.exclusive"><input type="checkbox" checked={audio.exclusive} onChange={(e) => update({ exclusive: e.target.checked })} aria-label={t("settings.exclusive")} /></Row>
      <Row title={t("settings.eq")} settingKey="audio.eq" stacked>
        <div className="eq" style={{ width: "100%" }}>
          <div className="row">
            <label className="switch"><input type="checkbox" checked={audio.eq.enabled} onChange={(e) => update({ eq: { ...audio.eq, enabled: e.target.checked, bands } })} /> {t("settings.eq")}</label>
            <label className="row"><span className="small muted">{t("settings.eqPreset")}</span>
              <select className="select" value={audio.eq.preset ?? "custom"} onChange={(e) => { const p = PRESETS[e.target.value]; if (p) setBands(bands.map((b, i) => ({ ...b, gainDb: p[i] ?? 0 })), e.target.value); }}>
                <option value="custom">Custom</option>
                {Object.keys(PRESETS).map((p) => <option key={p} value={p}>{p === "flat" ? t("settings.eqFlat") : p}</option>)}
              </select>
            </label>
            <label className="row"><span className="small muted">{t("settings.eqPreamp")}</span><input type="range" min={-12} max={12} step={0.5} value={audio.eq.preampDb} onChange={(e) => update({ eq: { ...audio.eq, preampDb: Number(e.target.value), bands } })} /><span className="mono small">{audio.eq.preampDb.toFixed(1)} dB</span></label>
            <button type="button" className="btn sm" onClick={() => setBands(bands.map((b) => ({ ...b, gainDb: 0 })), "flat")}>{t("settings.eqReset")}</button>
          </div>
          <EqCanvas bands={bands} onChange={(b) => setBands(b)} disabled={!audio.eq.enabled} />
          <div className="bands">{bands.map((b) => <div key={b.frequencyHz}>{b.frequencyHz >= 1000 ? `${b.frequencyHz / 1000}k` : b.frequencyHz}<br /><span className="mono">{b.gainDb > 0 ? "+" : ""}{b.gainDb.toFixed(1)}</span></div>)}</div>
        </div>
      </Row>
    </>
  );
}

/** Frequency-response curve drawn on canvas with draggable band handles. Peaking biquad magnitude, log-frequency axis. */
function EqCanvas({ bands, onChange, disabled }: { bands: EqBand[]; onChange: (b: EqBand[]) => void; disabled: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [drag, setDrag] = useState<number | undefined>(undefined);
  const [local, setLocal] = useState(bands);
  useEffect(() => setLocal(bands), [bands]);
  const RANGE = 15;
  const fx = (f: number, w: number) => ((Math.log10(f) - Math.log10(20)) / (Math.log10(20000) - Math.log10(20))) * w;
  const gy = (g: number, h: number) => h / 2 - (g / RANGE) * (h / 2);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = w * dpr;
    c.height = h * dpr;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    const css = getComputedStyle(document.documentElement);
    const accent = css.getPropertyValue("--accent").trim() || "#6f5cff";
    const faint = css.getPropertyValue("--fg-faint").trim() || "#888";
    ctx.clearRect(0, 0, w, h);
    ctx.strokeStyle = faint;
    ctx.globalAlpha = 0.25;
    for (const g of [-12, -6, 0, 6, 12]) { ctx.beginPath(); ctx.moveTo(0, gy(g, h)); ctx.lineTo(w, gy(g, h)); ctx.stroke(); }
    for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) { ctx.beginPath(); ctx.moveTo(fx(f, w), 0); ctx.lineTo(fx(f, w), h); ctx.stroke(); }
    ctx.globalAlpha = disabled ? 0.4 : 1;
    // Response: sum of peaking-EQ magnitudes (dB) at each pixel column.
    const sr = 48000;
    ctx.beginPath();
    for (let x = 0; x <= w; x++) {
      const f = 20 * Math.pow(1000, x / w);
      let db = 0;
      for (const b of local) db += peakingDb(f, b.frequencyHz, b.gainDb, b.q, sr);
      const y = gy(Math.max(-RANGE, Math.min(RANGE, db)), h);
      if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = accent;
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.lineTo(w, h / 2);
    ctx.lineTo(0, h / 2);
    ctx.closePath();
    ctx.fillStyle = accent;
    ctx.globalAlpha = disabled ? 0.08 : 0.15;
    ctx.fill();
    ctx.globalAlpha = disabled ? 0.4 : 1;
    local.forEach((b, i) => {
      ctx.beginPath();
      ctx.arc(fx(b.frequencyHz, w), gy(b.gainDb, h), drag === i ? 8 : 6, 0, Math.PI * 2);
      ctx.fillStyle = accent;
      ctx.fill();
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 1.5;
      ctx.stroke();
    });
  }, [local, drag, disabled]);
  const hit = (e: React.PointerEvent) => {
    const c = ref.current;
    if (!c) return undefined;
    const r = c.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    let best: number | undefined;
    let bd = 14;
    local.forEach((b, i) => {
      const d = Math.hypot(fx(b.frequencyHz, r.width) - x, gy(b.gainDb, r.height) - y);
      if (d < bd) { bd = d; best = i; }
    });
    return best;
  };
  const gainAt = (e: React.PointerEvent) => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return 0;
    const g = ((r.height / 2 - (e.clientY - r.top)) / (r.height / 2)) * RANGE;
    return Math.round(Math.max(-RANGE, Math.min(RANGE, g)) * 2) / 2;
  };
  return (
    <canvas ref={ref} role="img" aria-label={t("settings.eq")} data-testid="eq-canvas"
      onPointerDown={(e) => { if (disabled) return; const i = hit(e); if (i !== undefined) { setDrag(i); (e.target as HTMLElement).setPointerCapture(e.pointerId); } }}
      onPointerMove={(e) => { if (drag === undefined) return; const g = gainAt(e); setLocal((l) => l.map((b, i) => (i === drag ? { ...b, gainDb: g } : b))); }}
      onPointerUp={() => { if (drag !== undefined) { onChange(local); setDrag(undefined); } }}
      onDoubleClick={(e) => { const i = hit(e as unknown as React.PointerEvent); if (i !== undefined) onChange(local.map((b, j) => (j === i ? { ...b, gainDb: 0 } : b))); }}
    />
  );
}

/** Magnitude response in dB of an RBJ peaking EQ biquad at frequency f. */
export function peakingDb(f: number, f0: number, gainDb: number, q: number, sr: number): number {
  const A = Math.pow(10, gainDb / 40);
  const w0 = (2 * Math.PI * f0) / sr;
  const alpha = Math.sin(w0) / (2 * q);
  const b0 = 1 + alpha * A;
  const b1 = -2 * Math.cos(w0);
  const b2 = 1 - alpha * A;
  const a0 = 1 + alpha / A;
  const a1 = -2 * Math.cos(w0);
  const a2 = 1 - alpha / A;
  const w = (2 * Math.PI * f) / sr;
  const cos1 = Math.cos(w);
  const cos2 = Math.cos(2 * w);
  const sin1 = Math.sin(w);
  const sin2 = Math.sin(2 * w);
  const nr = b0 + b1 * cos1 + b2 * cos2;
  const ni = -(b1 * sin1 + b2 * sin2);
  const dr = a0 + a1 * cos1 + a2 * cos2;
  const di = -(a1 * sin1 + a2 * sin2);
  const mag = Math.sqrt((nr * nr + ni * ni) / (dr * dr + di * di));
  return 20 * Math.log10(Math.max(1e-9, mag));
}

function Transcoding() {
  const network = useApp((s) => s.network);
  const [format, setFormat] = useState("");
  const [bitrate, setBitrate] = useState(0);
  const [cannot, setCannot] = useState("ape, dsf, dff");
  const apply = () => bridge().dispatch({ type: "setTranscodingProfile", data: { network_id: network?.networkId, profile: { format: format || undefined, maxBitRate: bitrate || undefined, cannotDecode: cannot.split(",").map((s) => s.trim()).filter(Boolean) } } });
  return (
    <>
      <div className="muted small" style={{ marginBottom: 8 }}>{t("settings.transcodingNetwork")}: {network?.networkId ?? network?.kind ?? "–"}</div>
      <Row title={t("settings.transcodingFormat")} settingKey="transcoding">
        <select className="select" value={format} onChange={(e) => setFormat(e.target.value)}><option value="">{t("settings.transcodingOriginal")}</option>{["opus", "mp3", "aac", "flac"].map((f) => <option key={f} value={f}>{f}</option>)}</select>
      </Row>
      <Row title={t("settings.transcodingBitrate")} settingKey="transcoding">
        <select className="select" value={bitrate} onChange={(e) => setBitrate(Number(e.target.value))}><option value={0}>{t("settings.transcodingUnlimited")}</option>{[96, 128, 192, 256, 320].map((b) => <option key={b} value={b}>{b} kbps</option>)}</select>
      </Row>
      <Row title={t("settings.transcodingCannotDecode")} settingKey="transcoding"><input className="input" value={cannot} onChange={(e) => setCannot(e.target.value)} style={{ width: 240 }} /></Row>
      <div className="row" style={{ marginTop: 10 }}><button type="button" className="btn primary" onClick={apply}>{t("dialog.ok")}</button></div>
    </>
  );
}

function Connect() {
  const connection = useApp((s) => s.connection);
  const devices = useApp((s) => s.devices);
  const meta = useApp((s) => s.meta);
  const [url, setUrl] = useState(connection.coordinatorUrl ?? "");
  useEffect(() => setUrl(connection.coordinatorUrl ?? ""), [connection.coordinatorUrl]);
  return (
    <>
      <Row title={t("settings.deviceName")} settingKey="device.name"><span className="muted">{meta?.deviceName}</span></Row>
      <Row title={t("settings.coordinatorUrl")} desc={connection.error ?? (connection.connected ? `${connection.tier} · ${connection.roundTripMs ?? "?"} ms` : undefined)} settingKey="connect.coordinatorUrl">
        <input className="input" placeholder="wss://coordinator.example.org" value={url} onChange={(e) => setUrl(e.target.value)} onBlur={() => bridge().dispatch({ type: "setCoordinatorUrl", data: { url: url.trim() || undefined } })} style={{ width: 260 }} />
        {connection.tier === "coordinator" && connection.connected ? <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "disconnectCoordinator" })}>{t("settings.coordinatorDisconnect")}</button> : <button type="button" className="btn sm" onClick={() => { bridge().dispatch({ type: "setCoordinatorUrl", data: { url: url.trim() || undefined } }); bridge().dispatch({ type: "connectCoordinator" }); }} disabled={!url.trim()}>{t("settings.coordinatorConnect")}</button>}
      </Row>
      <Toggle settingKey="connect.lanDiscovery" title={t("settings.lanDiscovery")} />
      <div className="section-title">{t("connect.title")}</div>
      {devices.map((d) => <div key={d.id} className="setting-row"><div className="label"><div className="title">{d.name}{d.isSelf ? <span className="badge">{t("connect.thisDevice")}</span> : null}{d.playing ? <span className="badge ok">{t("connect.playing")}</span> : null}</div><div className="desc">{d.platform} · {d.appVersion} · {fmtRelative(d.lastSeen)}</div></div><div className="control" /></div>)}
    </>
  );
}

function Storage() {
  const storage = useApp((s) => s.storage);
  const { data } = useQuery(() => ({ type: "storage" }), "storage", [], { static: true });
  const s = storage ?? data;
  const warn = useSetting<number | null>("storage.warnBytes", null);
  return (
    <>
      <Row title={t("settings.storage")} settingKey="storage"><span className="muted small">{s ? t("downloads.usage", { downloads: fmtBytes(s.downloadsBytes), cache: fmtBytes(s.cacheBytes), images: fmtBytes(s.imagesBytes) }) : "–"}</span></Row>
      <Row title={t("settings.storageWarn")} settingKey="storage.warnBytes">
        <select className="select" value={warn ?? 0} onChange={(e) => bridge().dispatch({ type: "setStorageWarnThreshold", data: { bytes: Number(e.target.value) || undefined } })}>
          <option value={0}>–</option>
          {[5, 10, 20, 50, 100].map((g) => <option key={g} value={g * 1024 ** 3}>{g} GB</option>)}
        </select>
      </Row>
      <Row title={t("downloads.clearCache")} settingKey="storage"><button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "clearStreamCache" })}>{t("downloads.clearCache")}</button></Row>
      <Toggle settingKey="power.batterySaverAuto" title={t("settings.batterySaverAuto")} desc={t("settings.batterySaverNow")} />
      <BatterySaverRow />
    </>
  );
}

function BatterySaverRow() {
  const on = useApp((s) => s.batterySaver);
  return <Row title={t("settings.batterySaver")} settingKey="power.batterySaver"><input type="checkbox" checked={on} onChange={(e) => bridge().dispatch({ type: "setBatterySaver", data: { enabled: e.target.checked } })} aria-label={t("settings.batterySaver")} data-testid="setting-battery-saver" /></Row>;
}

function LyricsSettings() {
  const external = useSetting("lyrics.external", false);
  const fps = useSetting("lyrics.fpsCap", 60);
  const set = useApp((s) => s.setSetting);
  return (
    <>
      <Row title={t("settings.externalLyrics")} desc={t("settings.externalLyricsPrivacy")} settingKey="lyrics.external">
        <input type="checkbox" checked={external} onChange={(e) => bridge().dispatch({ type: "setExternalLyricsEnabled", data: { enabled: e.target.checked } })} aria-label={t("settings.externalLyrics")} data-testid="setting-external-lyrics" />
      </Row>
      <Row title="Lyrics frame rate cap" settingKey="lyrics.fpsCap">
        <select className="select" value={fps} onChange={(e) => set("lyrics.fpsCap", Number(e.target.value))}>{[30, 60, 120, 144].map((f) => <option key={f} value={f}>{f} fps</option>)}</select>
      </Row>
    </>
  );
}

function Appearance() {
  const theme = useSetting("appearance.theme", "system");
  const accent = useSetting("appearance.accent", "#6f5cff");
  const set = useApp((s) => s.setSetting);
  return (
    <>
      <Row title={t("settings.theme")} settingKey="appearance.theme">
        <select className="select" value={theme} onChange={(e) => set("appearance.theme", e.target.value)} data-testid="setting-theme">{["system", "light", "dark"].map((v) => <option key={v} value={v}>{t(`settings.theme.${v}` as never)}</option>)}</select>
      </Row>
      <Row title={t("settings.accent")} settingKey="appearance.accent"><input type="color" value={accent} onChange={(e) => set("appearance.accent", e.target.value)} aria-label={t("settings.accent")} /></Row>
      <Toggle settingKey="appearance.dynamicAccent" title={t("settings.dynamicAccent")} />
      <Toggle settingKey="appearance.animatedBackground" title={t("settings.animatedBackground")} />
    </>
  );
}

function Customisation() {
  return (
    <>
      <div className="muted small" style={{ marginBottom: 12 }}>{t("settings.customiseHint")}</div>
      <OrderList surface="sidebar" title={t("settings.sidebarItems")} />
      <OrderList surface="contextMenu" title={t("settings.contextMenuItems")} />
      <OrderList surface="mediaSession" title={t("settings.mediaSessionButtons")} />
    </>
  );
}

function OrderList({ surface, title }: { surface: string; title: string }) {
  const version = useApp((s) => s.actionsVersion);
  // The full set comes from the registry (empty order = all); current order from Query.Actions.
  const all = useQuery(() => ({ type: "actions", data: { surface, target: { type: surface === "contextMenu" ? "tracks" : "none", data: surface === "contextMenu" ? { ids: ["__all__"] } : undefined } as never } }), "actions", [surface, version], { static: true });
  const [items, setItems] = useState<{ a: ActionDescriptor; on: boolean }[]>([]);
  const [dragIdx, setDragIdx] = useState<number | undefined>(undefined);
  useEffect(() => { if (all.data) setItems(all.data.map((a) => ({ a, on: true }))); }, [all.data]);
  const commit = (next: typeof items) => {
    setItems(next);
    bridge().dispatch({ type: "setActionOrder", data: { surface, action_ids: next.filter((x) => x.on).map((x) => x.a.id) } });
  };
  const move = (from: number, to: number) => {
    const arr = [...items];
    const [m] = arr.splice(from, 1);
    if (m) arr.splice(to, 0, m);
    commit(arr);
  };
  return (
    <div style={{ marginBottom: 20 }}>
      <div className="section-title">{title}</div>
      <div className="order-list" data-testid={`order-${surface}`}>
        {items.map((it, i) => (
          <div key={it.a.id} className="order-item" draggable onDragStart={() => setDragIdx(i)} onDragOver={(e) => e.preventDefault()} onDrop={() => { if (dragIdx !== undefined && dragIdx !== i) move(dragIdx, i); setDragIdx(undefined); }}>
            <span className="grip"><Icon name="grip" size={14} /></span>
            <input type="checkbox" checked={it.on} onChange={(e) => commit(items.map((x, j) => (j === i ? { ...x, on: e.target.checked } : x)))} aria-label={it.a.label} />
            <Icon name={it.a.icon} size={14} />
            <span className="grow">{it.a.label}</span>
            <button type="button" className="btn icon sm" aria-label="Move up" disabled={i === 0} onClick={() => move(i, i - 1)}><Icon name="chevronUp" size={12} /></button>
            <button type="button" className="btn icon sm" aria-label="Move down" disabled={i === items.length - 1} onClick={() => move(i, i + 1)}><Icon name="chevronDown" size={12} /></button>
          </div>
        ))}
      </div>
    </div>
  );
}

function Shortcuts() {
  const shortcuts = useApp((s) => s.shortcuts);
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const keymap = useKeymap();
  const [recording, setRecording] = useState<string | undefined>(undefined);
  const labels = useMemo(() => new Map(DEFAULT_KEYMAP.map((k) => [k.actionId, t(k.labelId as never)])), []);
  const { data: paletteActions } = useQuery(() => ({ type: "actions", data: { surface: "palette", target: { type: "none" } } }), "actions", [], { static: true });
  const rows = useMemo(() => {
    const ids = new Set<string>([...DEFAULT_KEYMAP.map((k) => k.actionId), ...shortcuts.map((s) => s.actionId)]);
    return [...ids].filter((id) => id !== "redo.alt").map((id) => {
      const s = shortcuts.find((x) => x.actionId === id);
      const def = s?.defaultShortcut ?? DEFAULT_KEYMAP.find((k) => k.actionId === id)?.shortcut;
      const cur = s ? s.shortcut : def;
      const label = labels.get(id) ?? paletteActions?.find((a) => a.id === id)?.label ?? id;
      const category = DEFAULT_KEYMAP.find((k) => k.actionId === id)?.category ?? paletteActions?.find((a) => a.id === id)?.category ?? "other";
      return { id, label, category, current: cur, def };
    }).sort((a, b) => a.category.localeCompare(b.category) || a.label.localeCompare(b.label));
  }, [shortcuts, labels, paletteActions]);
  const conflicts = useMemo(() => new Map(keymap.conflicts().flatMap((c) => c.actionIds.map((id) => [id, c.actionIds.filter((x) => x !== id)] as const))), [keymap]);
  const onKey = (e: React.KeyboardEvent, id: string) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return setRecording(undefined);
    if (e.key === "Backspace" || e.key === "Delete") { bridge().dispatch({ type: "setShortcut", data: { action_id: id, shortcut: undefined } }); setRecording(undefined); return; }
    const chord = chordFromEvent(e.nativeEvent, platform);
    if (!chord) return;
    bridge().dispatch({ type: "setShortcut", data: { action_id: id, shortcut: chordToString(chord) } });
    setRecording(undefined);
  };
  let lastCat = "";
  return (
    <>
      <div className="row" style={{ marginBottom: 8 }}><span className="muted small grow">{t("settings.shortcutsHint")}</span><button type="button" className="btn sm" onClick={() => rows.forEach((r) => bridge().dispatch({ type: "setShortcut", data: { action_id: r.id, shortcut: r.def } }))}>{t("settings.shortcutResetAll")}</button></div>
      {rows.map((r) => {
        const cat = r.category !== lastCat ? <div className="section-title" key={`c-${r.category}`}>{r.category}</div> : null;
        lastCat = r.category;
        const chord = r.current ? parseChord(r.current) : undefined;
        const conf = conflicts.get(r.id);
        return (
          <div key={r.id}>
            {cat}
            <div className="shortcut-row" data-testid={`shortcut-${r.id}`}>
              <div>{r.label}{conf?.length ? <div className="conflict">{t("settings.shortcutConflict", { action: conf.map((c) => labels.get(c) ?? c).join(", ") })}</div> : null}</div>
              <button type="button" className={`btn rec ${recording === r.id ? "recording" : ""}`} onClick={() => setRecording(r.id)} onKeyDown={recording === r.id ? (e) => onKey(e, r.id) : undefined} onBlur={() => recording === r.id && setRecording(undefined)} aria-label={`${r.label}: ${chord ? formatChord(chord, platform) : "unbound"}`}>
                {recording === r.id ? t("settings.shortcutRecording") : chord ? <span className="kbd">{formatChord(chord, platform)}</span> : <span className="faint">—</span>}
              </button>
              <button type="button" className="btn sm ghost" disabled={r.current === r.def} onClick={() => bridge().dispatch({ type: "setShortcut", data: { action_id: r.id, shortcut: r.def } })}>{t("settings.shortcutReset")}</button>
            </div>
          </div>
        );
      })}
    </>
  );
}

function Backup() {
  const exported = useApp((s) => s.exported);
  const openDialog = useApp((s) => s.openDialog);
  const [secrets, setSecrets] = useState(false);
  const [pending, setPending] = useState<string | undefined>(undefined);
  useEffect(() => {
    if (!pending || exported?.kind !== "config" || exported.at < Number(pending)) return;
    const doc = exported.document;
    setPending(undefined);
    void bridge().dialog.save({ title: t("dialog.exportConfig"), defaultPath: "hocket-config.json", filters: [{ name: "JSON", extensions: ["json"] }] }).then(async (path) => {
      if (!path) return;
      await bridge().dialog.writeTextFile(path, doc);
      useApp.getState().applyEvent({ type: "toast", data: { toast: { id: `cfg-${Date.now()}`, message: t("settings.configExported"), actionLabel: undefined, actionCommand: undefined, durationMs: 3000 } } });
    });
  }, [exported, pending]);
  const doImport = async () => {
    const path = await bridge().dialog.open({ title: t("dialog.importConfig"), filters: [{ name: "JSON", extensions: ["json"] }] });
    if (!path) return;
    const text = await bridge().dialog.readTextFile(path);
    openDialog({ kind: "confirm", title: t("settings.importConfig"), message: t("settings.importConfirm"), onConfirm: () => bridge().dispatch({ type: "importConfig", data: { document: text } }) });
  };
  return (
    <>
      <Row title={t("settings.exportConfig")} settingKey="backup">
        <label className="switch small"><input type="checkbox" checked={secrets} onChange={(e) => setSecrets(e.target.checked)} /> {t("settings.exportConfigSecrets")}</label>
        <button type="button" className="btn" onClick={() => { setPending(String(Date.now())); bridge().dispatch({ type: "exportConfig", data: { include_secrets: secrets } }); }}>{t("settings.exportConfig")}</button>
      </Row>
      <Row title={t("settings.importConfig")} settingKey="backup"><button type="button" className="btn" onClick={() => void doImport()}>{t("settings.importConfig")}</button></Row>
    </>
  );
}

function Diagnostics() {
  const meta = useApp((s) => s.meta);
  const { data } = useQuery(() => ({ type: "diagnostics" }), "text", [], { static: true });
  return (
    <>
      <Row title={t("settings.copyDiagnostics")} settingKey="diagnostics"><button type="button" className="btn" onClick={() => void executeAction("ui.copyDiagnostics")} data-testid="copy-diagnostics">{t("settings.copyDiagnostics")}</button></Row>
      <Row title={meta?.mediaSession === "attached" ? t("settings.mediaSessionState.attached") : t("settings.mediaSessionState.unavailable")} settingKey="diagnostics"><span className={`badge ${meta?.mediaSession === "attached" ? "ok" : "warn"}`}>{meta?.mediaSession}</span></Row>
      <textarea className="textarea" readOnly value={data ?? ""} rows={16} style={{ width: "100%", marginTop: 12 }} aria-label="Diagnostics" />
    </>
  );
}

function About() {
  const meta = useApp((s) => s.meta);
  const ver = navigator.userAgent.match(/Electron\/([\d.]+)/)?.[1] ?? "";
  return (
    <>
      <p>{t("settings.about", { version: meta?.version ?? "", core: meta?.coreKind ?? "", electron: ver })}</p>
      <p className="muted small">{t("settings.licence")}</p>
      <div className="row">
        <button type="button" className="btn" onClick={() => bridge().shell.openExternal("https://www.gnu.org/licenses/agpl-3.0.html")}><Icon name="external" size={13} /> {t("settings.licenceOpen")}</button>
        <button type="button" className="btn" onClick={() => bridge().shell.openExternal("https://github.com/ingoau/hocket")}><Icon name="external" size={13} /> {t("settings.sourceOpen")}</button>
      </div>
      <div className="muted small" style={{ marginTop: 16 }}>{t("app.name")} · {meta?.dataDir}</div>
    </>
  );
}

export type { Setting };
