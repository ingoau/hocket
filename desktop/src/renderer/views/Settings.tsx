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
import { CUSTOMISABLE } from "@shared/action-orders";
import { DEFAULT_KEYMAP } from "@shared/keymap";
import { executeAction } from "../store/actions";
import { CredentialWarning } from "../components/CredentialWarning";
import { DEFAULT_ACCENT, SK } from "@shared/settings-keys";
import { LYRICS_SIZES, setLyricsAnimated, setLyricsSize, useLyricsAnimated, useLyricsSize, type LyricsSize } from "../lib/lyrics-size";
import { Select, Slider, Switch } from "../components/controls";

const SECTIONS = ["general", "audio", "transcoding", "connect", "storage", "lyrics", "appearance", "customisation", "shortcuts", "backup", "diagnostics", "about"] as const;
type Section = (typeof SECTIONS)[number];
const SECTION_ICONS: Record<Section, string> = { general: "tune", audio: "equalizer", transcoding: "transcode", connect: "connectCast", storage: "cached", lyrics: "lyrics", appearance: "palette", customisation: "customise", shortcuts: "keyboard", backup: "backup", diagnostics: "bug", about: "info" };

export function Settings({ section }: { section?: string }) {
  const navigate = useApp((s) => s.navigate);
  const current: Section = (SECTIONS as readonly string[]).includes(section ?? "") ? (section as Section) : "general";
  return (
    <div className="view page-settings" data-testid="view-settings">
      <div className="settings">
        <nav className="settings-nav" aria-label={t("settings.title")}>
          {SECTIONS.map((s) => (
            <a key={s} href="#" className={`nav-item ${current === s ? "active" : ""}`} aria-current={current === s ? "page" : undefined} onClick={(e) => { e.preventDefault(); navigate({ view: "settings", param: s }, true); }} data-testid={`settings-nav-${s}`}><Icon name={SECTION_ICONS[s]} size={20} filled={current === s} /><span>{t(`settings.section.${s}` as never)}</span></a>
          ))}
        </nav>
        <div className="settings-body">
          <h1 className="settings-heading" id="settings-heading">{t(`settings.section.${current}` as never)}</h1>
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
  return <span className={`badge ${scope === "accountSynced" ? "synced" : ""}`}>{scope === "accountSynced" ? t("settings.scope.synced") : t("settings.scope.local")}</span>;
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
      <Switch checked={!!value} onChange={(checked) => set(settingKey, checked)} aria-label={title} data-testid={`setting-${settingKey}`} />
    </Row>
  );
}

function General() {
  const servers = useApp((s) => s.servers);
  const cap = useSetting(SK.queueSavedCap, 10);
  const set = useApp((s) => s.setSetting);
  const openDialog = useApp((s) => s.openDialog);
  const bridgeOn = useSetting(SK.ratingsLoveBridgeEnabled, false);
  const threshold = useSetting(SK.ratingsLoveBridgeThreshold, 4);
  const prefs = useApp((s) => s.prefs);
  const setPrefs = useApp((s) => s.setPrefs);
  return (
    <>
      <h2 className="section-title">{t("settings.servers")}</h2>
      <CredentialWarning />
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
      <Row title={t("settings.closeToTray")} desc={t("settings.scope.local")}>
        <Switch checked={prefs.closeToTray} onChange={(checked) => setPrefs({ closeToTray: checked })} aria-label={t("settings.closeToTray")} data-testid="pref-closeToTray" />
      </Row>
      <Toggle settingKey={SK.syncEnabled} title={t("settings.settingsSync")} />
      <Row title={t("settings.savedQueueCap", { n: cap })} settingKey={SK.queueSavedCap}>
        <Slider min={0} max={50} value={cap} bubble={cap} onChange={(v) => bridge().dispatch({ type: "setSavedQueueCap", data: { cap: v } })} aria-label={t("settings.savedQueueCap", { n: cap })} />
        <span className="mono small" style={{ width: 24 }}>{cap}</span>
      </Row>
      <Row title={t("settings.ratingBridge")} settingKey={SK.ratingsLoveBridgeThreshold}>
        <Select value={bridgeOn ? threshold : 0} aria-label={t("settings.ratingBridge")} onChange={(e) => { const n = Number(e.target.value); set(SK.ratingsLoveBridgeEnabled, n > 0); if (n > 0) set(SK.ratingsLoveBridgeThreshold, n); }} data-testid="setting-loveBridge">
          <option value={0}>{t("settings.ratingBridgeOff")}</option>
          {[3, 4, 5].map((n) => <option key={n} value={n}>{t("settings.ratingBridgeStars", { n })}</option>)}
        </Select>
      </Row>
      <QueueModeRow />
    </>
  );
}

function QueueModeRow() {
  const mode = useApp((s) => s.queue.mode);
  return (
    <Row title={t("settings.queueMode")} settingKey={SK.queueMode}>
      <Select value={mode} aria-label={t("settings.queueMode")} onChange={(e) => bridge().dispatch({ type: "setQueueMode", data: { mode: e.target.value as QueueMode } })}>
        <option value="apple">{t("queue.mode.apple")}</option>
        <option value="youTube">{t("queue.mode.youtube")}</option>
      </Select>
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
        <Select value={audio.replayGain} aria-label={t("settings.replayGain")} onChange={(e) => update({ replayGain: e.target.value as ReplayGainMode })} data-testid="setting-replaygain">
          {(["off", "track", "album", "auto"] as ReplayGainMode[]).map((m) => <option key={m} value={m}>{t(`settings.replayGain.${m}` as never)}</option>)}
        </Select>
      </Row>
      <Row title={t("settings.replayGainPreamp")} settingKey="audio.replayGain">
        <Slider min={-15} max={15} step={0.5} value={audio.replayGainPreampDb} bubble={`${audio.replayGainPreampDb.toFixed(1)} dB`} onChange={(v) => update({ replayGainPreampDb: v })} aria-label={t("settings.replayGainPreamp")} />
        <span className="mono small" style={{ width: 52 }}>{audio.replayGainPreampDb.toFixed(1)} dB</span>
      </Row>
      <Row title={t("settings.normalisation")} settingKey="audio.replayGain"><Switch checked={audio.normalisation} onChange={(checked) => update({ normalisation: checked })} aria-label={t("settings.normalisation")} /></Row>
      <Row title={t("settings.gapless")} settingKey="audio.gapless"><Switch checked={audio.gapless} onChange={(checked) => update({ gapless: checked })} aria-label={t("settings.gapless")} /></Row>
      <Row title={t("settings.outputDevice")} settingKey="audio.outputDevice">
        <Select value={audio.outputDevice ?? ""} aria-label={t("settings.outputDevice")} onChange={(e) => bridge().dispatch({ type: "setOutputDevice", data: { id: e.target.value || undefined } })} data-testid="setting-output">
          <option value="">{t("settings.outputDefault")}</option>
          {list.map((d) => <option key={d.id} value={d.id}>{d.isDefault ? t("settings.outputDeviceDefault", { name: d.name }) : d.name}</option>)}
        </Select>
        <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "refreshOutputDevices" })}>{t("settings.refreshDevices")}</button>
      </Row>
      <Row title={t("settings.exclusive")} settingKey="audio.exclusive"><Switch checked={audio.exclusive} onChange={(checked) => update({ exclusive: checked })} aria-label={t("settings.exclusive")} /></Row>
      <Row title={t("settings.eq")} settingKey="audio.eq" stacked>
        <div className="eq" style={{ width: "100%" }}>
          <div className="row">
            <label className="switch"><Switch checked={audio.eq.enabled} onChange={(checked) => update({ eq: { ...audio.eq, enabled: checked, bands } })} /> {t("settings.eq")}</label>
            <label className="row"><span className="small muted">{t("settings.eqPreset")}</span>
              <Select value={audio.eq.preset ?? "custom"} onChange={(e) => { const p = PRESETS[e.target.value]; if (p) setBands(bands.map((b, i) => ({ ...b, gainDb: p[i] ?? 0 })), e.target.value); }}>
                <option value="custom">Custom</option>
                {Object.keys(PRESETS).map((p) => <option key={p} value={p}>{p === "flat" ? t("settings.eqFlat") : p}</option>)}
              </Select>
            </label>
            <label className="row"><span className="small muted">{t("settings.eqPreamp")}</span><Slider min={-12} max={12} step={0.5} value={audio.eq.preampDb} bubble={`${audio.eq.preampDb.toFixed(1)} dB`} onChange={(v) => update({ eq: { ...audio.eq, preampDb: v, bands } })} /><span className="mono small">{audio.eq.preampDb.toFixed(1)} dB</span></label>
            <button type="button" className="btn sm" onClick={() => setBands(bands.map((b) => ({ ...b, gainDb: 0 })), "flat")}>{t("settings.eqReset")}</button>
          </div>
          <EqCanvas bands={bands} onChange={(b) => setBands(b)} disabled={!audio.eq.enabled} />
          {/* The keyboard (and screen reader) way to set each band; the canvas is the pointer way. */}
          <div className="bands">{bands.map((b, i) => (
            <label key={b.frequencyHz} className="band">
              <span aria-hidden="true">{b.frequencyHz >= 1000 ? `${b.frequencyHz / 1000}k` : b.frequencyHz}</span>
              <Slider orientation="vertical" className="eq-slider" min={-15} max={15} step={0.5} value={b.gainDb} disabled={!audio.eq.enabled} bubble={`${b.gainDb > 0 ? "+" : ""}${b.gainDb.toFixed(1)}`}
                aria-label={t("a11y.eqBand", { freq: b.frequencyHz })} aria-valuetext={t("a11y.eqBandValue", { gain: `${b.gainDb > 0 ? "+" : ""}${b.gainDb.toFixed(1)}` })}
                onChange={(v) => setBands(bands.map((x, j) => (j === i ? { ...x, gainDb: v } : x)))} data-testid={`eq-band-${i}`} />
              <span className="mono" aria-hidden="true">{b.gainDb > 0 ? "+" : ""}{b.gainDb.toFixed(1)}</span>
            </label>
          ))}</div>
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
        <Select value={format} aria-label={t("settings.transcodingFormat")} onChange={(e) => setFormat(e.target.value)}><option value="">{t("settings.transcodingOriginal")}</option>{["opus", "mp3", "aac", "flac"].map((f) => <option key={f} value={f}>{f}</option>)}</Select>
      </Row>
      <Row title={t("settings.transcodingBitrate")} settingKey="transcoding">
        <Select value={bitrate} aria-label={t("settings.transcodingBitrate")} onChange={(e) => setBitrate(Number(e.target.value))}><option value={0}>{t("settings.transcodingUnlimited")}</option>{[96, 128, 192, 256, 320].map((b) => <option key={b} value={b}>{b} kbps</option>)}</Select>
      </Row>
      <Row title={t("settings.transcodingCannotDecode")} settingKey="transcoding"><input className="input" value={cannot} aria-label={t("settings.transcodingCannotDecode")} onChange={(e) => setCannot(e.target.value)} style={{ width: 240 }} /></Row>
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
      <Row title={t("settings.coordinatorUrl")} desc={connection.error ?? (connection.connected ? `${connection.tier} · ${connection.roundTripMs ?? "?"} ms` : undefined)} settingKey={SK.connectCoordinatorUrl}>
        <input className="input" placeholder="wss://coordinator.example.org" aria-label={t("settings.coordinatorUrl")} value={url} onChange={(e) => setUrl(e.target.value)} onBlur={() => bridge().dispatch({ type: "setCoordinatorUrl", data: { url: url.trim() || undefined } })} style={{ width: 260 }} />
        {connection.tier === "coordinator" && connection.connected ? <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "disconnectCoordinator" })}>{t("settings.coordinatorDisconnect")}</button> : <button type="button" className="btn sm" onClick={() => { bridge().dispatch({ type: "setCoordinatorUrl", data: { url: url.trim() || undefined } }); bridge().dispatch({ type: "connectCoordinator" }); }} disabled={!url.trim()}>{t("settings.coordinatorConnect")}</button>}
      </Row>
      <Toggle settingKey={SK.connectLanDiscovery} title={t("settings.lanDiscovery")} />
      <h2 className="section-title">{t("connect.title")}</h2>
      {devices.map((d) => <div key={d.id} className="setting-row"><div className="label"><div className="title">{d.name}{d.isSelf ? <span className="badge">{t("connect.thisDevice")}</span> : null}{d.playing ? <span className="badge ok">{t("connect.playing")}</span> : null}</div><div className="desc">{d.platform} · {d.appVersion} · {fmtRelative(d.lastSeen)}</div></div><div className="control" /></div>)}
    </>
  );
}

const GIB = 1024 ** 3;

function Storage() {
  const storage = useApp((s) => s.storage);
  const { data } = useQuery(() => ({ type: "storage" }), "storage", [], { static: true });
  const s = storage ?? data;
  const warn = useSetting<number | null>(SK.storageWarnThresholdBytes, null);
  const partial = s?.partialCacheBytes ?? 0;
  const budget = s?.cacheBudgetBytes ?? 0;
  return (
    <>
      <Row title={t("settings.storageUsage")} settingKey="storage"><span className="muted small" data-testid="storage-usage">{s ? t("settings.storageUsageDetail", { downloads: fmtBytes(s.downloadsBytes), images: fmtBytes(s.imagesBytes) }) : "–"}</span></Row>
      <Row title={t("settings.cacheUsage")} settingKey="storage" stacked>
        <div className="cache-usage" data-testid="cache-usage">
          {/* The text says it all; the bar is a picture of it (complete, then partial, against the budget). */}
          <div className="cache-bar" aria-hidden="true">
            <div className="complete" style={{ width: `${budget ? Math.min(100, ((s?.cacheBytes ?? 0) - partial) / budget * 100) : 0}%` }} />
            <div className="partial" style={{ width: `${budget ? Math.min(100, partial / budget * 100) : 0}%` }} />
          </div>
          <span className="muted small">{s ? t("settings.cacheUsageDetail", { total: fmtBytes(s.cacheBytes), budget: budget ? fmtBytes(budget) : "–", complete: fmtBytes(Math.max(0, s.cacheBytes - partial)), partial: fmtBytes(partial) }) : "–"}</span>
        </div>
      </Row>
      <CacheBudgetRow budget={budget} auto={s?.cacheBudgetAuto ?? true} />
      <Row title={t("settings.dataSaved")} settingKey="storage" stacked>
        <div className="data-saved">
          <span className="figure" data-testid="data-saved">{fmtBytes(s?.dataSavedBytes ?? 0)}</span>
          <span className="muted small">{t("settings.dataSavedDetail", { served: fmtBytes(s?.servedFromDiskBytes ?? 0), fetched: fmtBytes(s?.fetchedBytes ?? 0) })}</span>
        </div>
      </Row>
      <Toggle settingKey={SK.storagePrefetchOnMobileData} title={t("settings.prefetchMobile")} desc={t("settings.prefetchMobileDesc")} />
      <Row title={t("settings.storageWarn")} settingKey={SK.storageWarnThresholdBytes}>
        <Select value={warn ? Math.round(warn / GIB) : 0} aria-label={t("settings.storageWarn")} onChange={(e) => bridge().dispatch({ type: "setStorageWarnThreshold", data: { bytes: Number(e.target.value) ? Number(e.target.value) * GIB : undefined } })}>
          <option value={0}>–</option>
          {[2, 4, 5, 10, 20, 50, 100].map((g) => <option key={g} value={g}>{g} GB</option>)}
        </Select>
      </Row>
      <Row title={t("downloads.clearCache")} settingKey="storage"><button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "clearStreamCache" })} data-testid="clear-cache">{t("downloads.clearCache")}</button></Row>
      <Toggle settingKey={SK.batteryAutoEngage} title={t("settings.batterySaverAuto")} desc={t("settings.batterySaverNow")} />
      <BatterySaverRow />
    </>
  );
}

/** Round to a quarter GB for the custom field. */
function toGb(bytes: number): number {
  return Math.max(0.25, Math.round((bytes / GIB) * 4) / 4);
}

/**
 * The stream cache budget: automatic (storage.cacheMaxBytes null, its
 * default; the core sizes it from the volume and reports it) or a custom size
 * in GB, any size (2 GB included) being the user's own.
 */
function CacheBudgetRow({ budget, auto }: { budget: number; auto: boolean }) {
  const set = useApp((s) => s.setSetting);
  const [custom, setCustom] = useState(!auto);
  const [draft, setDraft] = useState(() => String(toGb(budget || 4 * GIB)));
  useEffect(() => { setCustom(!auto); }, [auto]);
  useEffect(() => { if (!auto && budget) setDraft(String(toGb(budget))); }, [auto, budget]);
  const commit = (gb: number) => {
    if (!Number.isFinite(gb) || gb <= 0) return;
    set(SK.storageCacheMaxBytes, Math.round(gb * GIB));
  };
  const choose = (mode: string) => {
    if (mode === "auto") {
      setCustom(false);
      set(SK.storageCacheMaxBytes, null);
      return;
    }
    setCustom(true);
    // Starts from the current (automatic) size, which may well be 2 GB.
    const gb = toGb(budget || 4 * GIB);
    setDraft(String(gb));
    commit(gb);
  };
  return (
    <Row title={t("settings.cacheBudget")} desc={t("settings.cacheBudgetDesc")} settingKey={SK.storageCacheMaxBytes}>
      <Select value={custom ? "custom" : "auto"} aria-label={t("settings.cacheBudget")} onChange={(e) => choose(e.target.value)} data-testid="cache-budget-mode">
        <option value="auto">{t("settings.cacheBudgetAuto", { size: auto && budget ? fmtBytes(budget) : "–" })}</option>
        <option value="custom">{t("settings.cacheBudgetCustom")}</option>
      </Select>
      {custom ? (
        <>
          <input className="input" type="number" min={0.25} step={0.25} style={{ width: 80 }} value={draft} aria-label={t("settings.cacheBudgetCustomValue")} onChange={(e) => setDraft(e.target.value)} onBlur={() => commit(Number(draft))} onKeyDown={(e) => { if (e.key === "Enter") commit(Number(draft)); }} data-testid="cache-budget-gb" />
          <span className="muted small">{t("settings.cacheBudgetGb")}</span>
        </>
      ) : null}
    </Row>
  );
}

function BatterySaverRow() {
  const on = useApp((s) => s.batterySaver);
  return <Row title={t("settings.batterySaver")} settingKey="power.batterySaver"><Switch checked={on} onChange={(checked) => bridge().dispatch({ type: "setBatterySaver", data: { enabled: checked } })} aria-label={t("settings.batterySaver")} data-testid="setting-battery-saver" /></Row>;
}

function LyricsSettings() {
  const external = useSetting(SK.lyricsExternalEnabled, false);
  const fps = useSetting(SK.displayLyricsFps, 60);
  const batteryFps = useSetting(SK.batteryLyricsFps, 30);
  const set = useApp((s) => s.setSetting);
  return (
    <>
      <Row title={t("settings.externalLyrics")} desc={t("settings.externalLyricsPrivacy")} settingKey={SK.lyricsExternalEnabled}>
        <Switch checked={external} onChange={(checked) => bridge().dispatch({ type: "setExternalLyricsEnabled", data: { enabled: checked } })} aria-label={t("settings.externalLyrics")} data-testid="setting-external-lyrics" />
      </Row>
      <Row title="Lyrics frame rate cap" settingKey={SK.displayLyricsFps}>
        <Select value={fps} aria-label="Lyrics frame rate cap" onChange={(e) => set(SK.displayLyricsFps, Number(e.target.value))}>{[30, 60, 120, 144].map((f) => <option key={f} value={f}>{f} fps</option>)}</Select>
      </Row>
      <Row title="Lyrics frame rate in battery saver" settingKey={SK.batteryLyricsFps}>
        <Select value={batteryFps} aria-label="Lyrics frame rate in battery saver" onChange={(e) => set(SK.batteryLyricsFps, Number(e.target.value))}>{[15, 24, 30].map((f) => <option key={f} value={f}>{f} fps</option>)}</Select>
      </Row>
    </>
  );
}

function Appearance() {
  const theme = useSetting(SK.displayTheme, "system");
  const accent = useSetting<string | null>(SK.displayAccent, null) || DEFAULT_ACCENT;
  const immersive = useSetting(SK.displayImmersiveArtwork, "automatic");
  const set = useApp((s) => s.setSetting);
  return (
    <>
      <Row title={t("settings.theme")} settingKey={SK.displayTheme}>
        <Select value={theme} aria-label={t("settings.theme")} onChange={(e) => set(SK.displayTheme, e.target.value)} data-testid="setting-theme">{["system", "light", "dark"].map((v) => <option key={v} value={v}>{t(`settings.theme.${v}` as never)}</option>)}</Select>
      </Row>
      <Row title={t("settings.accent")} settingKey={SK.displayAccent}><input type="color" value={accent} onChange={(e) => set(SK.displayAccent, e.target.value)} aria-label={t("settings.accent")} /></Row>
      <Toggle settingKey={SK.displayDynamicColour} title={t("settings.dynamicAccent")} />
      <Toggle settingKey={SK.displayAnimatedBackground} title={t("settings.animatedBackground")} />
      <Row title={t("settings.immersiveArtwork")} settingKey={SK.displayImmersiveArtwork}>
        <Select value={immersive} aria-label={t("settings.immersiveArtwork")} onChange={(e) => set(SK.displayImmersiveArtwork, e.target.value)} data-testid="setting-immersive-artwork">{["automatic", "always", "never"].map((v) => <option key={v} value={v}>{t(`settings.immersiveArtwork.${v}` as never)}</option>)}</Select>
      </Row>
      <LyricsSizeRow />
      <LyricsAnimatedRow />
    </>
  );
}

function LyricsAnimatedRow() {
  const on = useLyricsAnimated();
  return (
    <div className="setting-row">
      <div className="label">
        <div className="title">{t("settings.lyricsAnimated")}<span className="badge">{t("settings.scope.local")}</span></div>
        <div className="desc" id="lyrics-animated-desc">{t("settings.lyricsAnimatedDesc")}</div>
      </div>
      <div className="control">
        <Switch checked={on} onChange={(checked) => setLyricsAnimated(checked)} aria-label={t("settings.lyricsAnimated")} aria-describedby="lyrics-animated-desc" data-testid="setting-lyrics-animated" />
      </div>
    </div>
  );
}

function LyricsSizeRow() {
  const size = useLyricsSize();
  return (
    <div className="setting-row">
      <div className="label">
        <div className="title">{t("settings.lyricsSize")}<span className="badge" title="deviceLocal">{t("settings.scope.local")}</span></div>
        <div className="desc">{t("settings.lyricsSizeDesc")}</div>
      </div>
      <div className="control">
        <Select value={size} onChange={(e) => setLyricsSize(e.target.value as LyricsSize)} aria-label={t("settings.lyricsSize")} data-testid="setting-lyrics-size">
          {LYRICS_SIZES.map((v) => <option key={v} value={v}>{t(`lyrics.size.${v}` as never)}</option>)}
        </Select>
      </div>
    </div>
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
  const target = { type: surface === "contextMenu" ? "tracks" : "none", data: surface === "contextMenu" ? { ids: ["__all__"] } : undefined } as never;
  // The chosen ones, in order, from Query.Actions (an empty stored order = the default); the
  // switched-off rest of CUSTOMISABLE after them, described by the palette (which lists everything).
  const chosen = useQuery(() => ({ type: "actions", data: { surface, target } }), "actions", [surface, version], { static: true });
  const palette = useQuery(() => ({ type: "actions", data: { surface: "palette", target } }), "actions", [surface], { static: true });
  const [items, setItems] = useState<{ a: ActionDescriptor; on: boolean }[]>([]);
  const [dragIdx, setDragIdx] = useState<number | undefined>(undefined);
  useEffect(() => {
    if (!chosen.data || !palette.data) return;
    const on = new Set(chosen.data.map((a) => a.id));
    const off = (CUSTOMISABLE[surface] ?? []).filter((id) => !on.has(id)).flatMap((id) => palette.data?.filter((a) => a.id === id) ?? []);
    setItems([...chosen.data.map((a) => ({ a, on: true })), ...off.map((a) => ({ a, on: false }))]);
  }, [chosen.data, palette.data, surface]);
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
      <h2 className="section-title" id={`order-${surface}-title`}>{title}</h2>
      <ul className="order-list plain-list" aria-labelledby={`order-${surface}-title`} data-testid={`order-${surface}`}>
        {items.map((it, i) => (
          <li key={it.a.id} className="order-item" draggable onDragStart={() => setDragIdx(i)} onDragOver={(e) => e.preventDefault()} onDrop={() => { if (dragIdx !== undefined && dragIdx !== i) move(dragIdx, i); setDragIdx(undefined); }}>
            <span className="grip" aria-hidden="true"><Icon name="grip" size={14} /></span>
            <Switch checked={it.on} onChange={(checked) => commit(items.map((x, j) => (j === i ? { ...x, on: checked } : x)))} aria-label={it.a.label} />
            <Icon name={it.a.icon} size={14} />
            <span className="grow">{it.a.label}</span>
            <button type="button" className="btn icon sm" aria-label={t("a11y.moveUp", { label: it.a.label })} disabled={i === 0} onClick={() => move(i, i - 1)}><Icon name="chevronUp" size={12} /></button>
            <button type="button" className="btn icon sm" aria-label={t("a11y.moveDown", { label: it.a.label })} disabled={i === items.length - 1} onClick={() => move(i, i + 1)}><Icon name="chevronDown" size={12} /></button>
          </li>
        ))}
      </ul>
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
    return [...ids].filter((id) => id !== "redoAlt").map((id) => {
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
  // Rows are already sorted by category: one grouped list per category.
  const groups: { category: string; rows: typeof rows }[] = [];
  for (const r of rows) {
    const last = groups[groups.length - 1];
    if (last && last.category === r.category) last.rows.push(r);
    else groups.push({ category: r.category, rows: [r] });
  }
  return (
    <>
      <div className="row settings-lede"><span className="muted small grow">{t("settings.shortcutsHint")}</span><button type="button" className="btn sm" onClick={() => rows.forEach((r) => bridge().dispatch({ type: "setShortcut", data: { action_id: r.id, shortcut: r.def } }))}>{t("settings.shortcutResetAll")}</button></div>
      {groups.map((g) => (
        <section key={g.category} className="shortcut-group">
          <h2 className="section-title">{g.category}</h2>
          {g.rows.map((r) => {
            const chord = r.current ? parseChord(r.current) : undefined;
            const conf = conflicts.get(r.id);
            return (
              <div key={r.id} className="shortcut-row" data-testid={`shortcut-${r.id}`}>
                <div>{r.label}{conf?.length ? <div className="conflict">{t("settings.shortcutConflict", { action: conf.map((c) => labels.get(c) ?? c).join(", ") })}</div> : null}</div>
                <button type="button" className={`btn rec ${recording === r.id ? "recording" : ""}`} onClick={() => setRecording(r.id)} onKeyDown={recording === r.id ? (e) => onKey(e, r.id) : undefined} onBlur={() => recording === r.id && setRecording(undefined)} aria-label={`${r.label}: ${chord ? formatChord(chord, platform) : "unbound"}`}>
                  {recording === r.id ? t("settings.shortcutRecording") : chord ? <span className="kbd">{formatChord(chord, platform)}</span> : <span className="faint">—</span>}
                </button>
                <button type="button" className="btn sm ghost" disabled={r.current === r.def} onClick={() => bridge().dispatch({ type: "setShortcut", data: { action_id: r.id, shortcut: r.def } })}>{t("settings.shortcutReset")}</button>
              </div>
            );
          })}
        </section>
      ))}
    </>
  );
}

function Backup() {
  const exported = useApp((s) => s.exported);
  const [secrets, setSecrets] = useState(false);
  const [pending, setPending] = useState<string | undefined>(undefined);
  useEffect(() => {
    if (!pending || exported?.kind !== "config" || exported.at < Number(pending)) return;
    const doc = exported.document;
    setPending(undefined);
    // Main resolves the keystore references (with a plain-text warning) and
    // writes the file to a path the user picks; the page never sees a path.
    void bridge().config.export(doc).catch((err: unknown) => console.error("config export failed", err));
  }, [exported, pending]);
  // Main picks the file, confirms, adds any servers whose passwords the file carries, then imports.
  const doImport = () => bridge().config.import().catch((err: unknown) => console.error("config import failed", err));
  return (
    <>
      <Row title={t("settings.exportConfig")} settingKey="backup">
        <label className="switch small"><Switch checked={secrets} onChange={(checked) => setSecrets(checked)} data-testid="export-secrets" /> {t("settings.exportConfigSecrets")}</label>
        <button type="button" className="btn" onClick={() => { setPending(String(Date.now())); bridge().dispatch({ type: "exportConfig", data: { include_secrets: secrets } }); }} data-testid="export-config">{t("settings.exportConfig")}</button>
      </Row>
      <Row title={t("settings.importConfig")} settingKey="backup"><button type="button" className="btn" onClick={() => void doImport()} data-testid="import-config">{t("settings.importConfig")}</button></Row>
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
      <textarea className="textarea" readOnly value={data ?? ""} rows={16} style={{ width: "100%", marginTop: 12 }} aria-label={t("settings.section.diagnostics")} />
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
