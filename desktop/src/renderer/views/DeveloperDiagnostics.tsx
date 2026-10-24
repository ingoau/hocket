// Developer diagnostics: the last hour of logs plus versions and device details, to save, copy,
// show in the file manager or (macOS) share, with the bundles of the other devices in the Connect
// room and its coordinator. With developer mode on, the page asks them as it opens (and on Refresh)
// and shows each one's progress; the actions wait for that to finish. Main builds the bundle on
// every action; the page only previews this device's.
// Opened from Settings → Advanced, or with the developer-mode shortcut.
import { useCallback, useEffect, useRef, useState } from "react";
import type { DeviceInfo, DiagnosticsSourceStatus, RoomDiagnosticsStatus } from "@core/api";
import type { DiagnosticsOptions, DiagnosticsSummary } from "@shared/bridge-types";
import { SK } from "@shared/settings-keys";
import { t } from "@shared/strings";
import { bridge } from "../core/bridge";
import { Icon } from "../components/Icon";
import { Switch } from "../components/controls";
import { fmtBytes } from "../lib/format";
import { showToast } from "../lib/ui-toast";
import { useApp, useSetting } from "../store/app";

export function DeveloperDiagnostics() {
  const devMode = useSetting(SK.developerMode, false);
  const setSetting = useApp((s) => s.setSetting);
  const roomStatus = useApp((s) => s.roomDiagnostics);
  const [summary, setSummary] = useState<DiagnosticsSummary | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  // Other devices only ever answer while developer mode is on here too.
  const options: DiagnosticsOptions = { allDevices: devMode };
  /** Asking the other devices; the actions wait for it (their bundles go with every one). From the first frame, so none runs before. */
  const [collecting, setCollecting] = useState(devMode);
  /** What collect() said the newest status is: a reused collection sends no events. */
  const [collected, setCollected] = useState<RoomDiagnosticsStatus | undefined>(undefined);

  const refresh = useCallback(() => {
    void bridge()
      .diagnostics.summary()
      .then(setSummary)
      .catch((err: unknown) => console.error("diagnostics summary failed", err));
  }, []);
  useEffect(refresh, [refresh]);
  const collect = useCallback((force = false) => {
    setCollecting(true);
    void bridge()
      .diagnostics.collect({ force })
      .then(setCollected)
      .catch((err: unknown) => console.error("diagnostics collection failed", err))
      .finally(() => setCollecting(false));
  }, []);
  // As the page opens, and whenever developer mode comes on.
  useEffect(() => {
    if (devMode) collect();
    else setCollecting(false);
  }, [devMode, collect]);
  // The newest lines are what matter: keep the preview scrolled to its end.
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [summary]);

  /** Runs one action; the page refreshes afterwards so the preview matches what was written. */
  const run = (action: () => Promise<string | undefined>) => {
    setBusy(true);
    void action()
      .then((message) => {
        if (message) showToast(message);
      })
      .catch((err: unknown) => showToast(t("developer.failed", { error: err instanceof Error ? err.message : String(err) })))
      .finally(() => {
        setBusy(false);
        refresh();
      });
  };
  const fileName = (path: string) => path.split(/[/\\]/).pop() ?? path;
  const waiting = devMode && collecting;
  const blocked = busy || waiting;
  // Main asks one collection at a time (and reuses one that just finished), so the newest status is this page's.
  const status = roomStatus ?? collected;

  return (
    <div className="view page-developer" data-testid="view-developer">
      <div className="view-header">
        <h1 className="brand-type">{t("developer.title")}</h1>
        <div className="actions">
          <button type="button" className="btn" onClick={() => { refresh(); if (devMode) collect(true); }} disabled={blocked} data-testid="developer-refresh"><Icon name="refresh" size={16} /> {t("developer.refresh")}</button>
        </div>
      </div>
      <div className="view-body">
        <div className="stack developer-stack">
          <p className="muted">{t("developer.subtitle")}</p>
          <div className={`developer-mode card ${devMode ? "on" : ""}`} data-testid="developer-mode-card">
            <Icon name="code" size={22} className="developer-mode-icon" />
            <div className="developer-mode-text">{devMode ? t("developer.modeOn") : t("developer.modeOff")}</div>
            <Switch checked={devMode} onChange={(checked) => setSetting(SK.developerMode, checked)} aria-label={t("settings.developerMode")} data-testid="developer-mode-switch" />
          </div>
          <div className="developer-actions">
            <button type="button" className="btn primary" disabled={blocked} onClick={() => run(async () => { const p = await bridge().diagnostics.save(options); return p ? t("developer.saved", { name: fileName(p) }) : undefined; })} data-testid="developer-save"><Icon name="save" size={16} /> {t("developer.save")}</button>
            {summary?.canShare ? (
              <button type="button" className="btn" disabled={blocked} onClick={() => run(async () => { await bridge().diagnostics.share(options); return undefined; })} data-testid="developer-share"><Icon name="share" size={16} /> {t("developer.share")}</button>
            ) : null}
            <button type="button" className="btn" disabled={blocked} onClick={() => run(async () => { await bridge().diagnostics.copy(options); return t("developer.copied"); })} data-testid="developer-copy"><Icon name="copy" size={16} /> {t("developer.copy")}</button>
            <button type="button" className="btn" disabled={blocked} onClick={() => run(async () => { const p = await bridge().diagnostics.reveal(options); return p ? t("developer.revealed", { name: fileName(p) }) : undefined; })} data-testid="developer-reveal"><Icon name="folder" size={16} /> {t("developer.reveal")}</button>
            {waiting ? <span className="muted small" data-testid="developer-collecting">{t("developer.waitForOthers")}</span> : null}
            {summary ? <span className="muted small developer-stats" data-testid="developer-stats">{t("developer.stats", { lines: summary.logLines, size: fmtBytes(summary.logBytes), minutes: Math.round(summary.windowMs / 60_000) })}</span> : null}
          </div>
          <RoomSection devMode={devMode} collecting={waiting} status={status} />
          <section className="card developer-section">
            <h2 className="card-title">{t("developer.environment")}</h2>
            <pre className="developer-pre" data-testid="developer-head" tabIndex={0}>{summary ? summary.head.trimEnd() : t("developer.loading")}</pre>
          </section>
          <section className="card developer-section">
            <h2 className="card-title">{t("developer.log")}</h2>
            {summary ? <p className="muted small">{t("developer.logTail", { n: 400 })}</p> : null}
            <pre ref={logRef} className="developer-pre developer-log" data-testid="developer-log" tabIndex={0}>{summary ? summary.tail || t("developer.empty") : t("developer.loading")}</pre>
          </section>
        </div>
      </div>
    </div>
  );
}

/** The other devices and the coordinator: who was asked, and where each answer stands. */
function RoomSection({ devMode, collecting, status }: { devMode: boolean; collecting: boolean; status: RoomDiagnosticsStatus | undefined }) {
  const devices = useApp((s) => s.devices);
  const viaCoordinator = useApp((s) => s.connection.tier === "coordinator" && s.connection.connected);
  const running = collecting || (status !== undefined && !status.done);
  let headline: string;
  if (!devMode) headline = t("developer.othersOff");
  else if (status?.unavailable === "developerModeOff") headline = t("developer.othersOff");
  else if (status?.unavailable === "notConnected") headline = t("developer.othersNotConnected");
  else if (!status && !running) headline = t("developer.othersNoRoom");
  else if (!status || (running && !status.roomAnswered)) headline = t("developer.othersAsking");
  else if (status.roomDeclined) headline = t("developer.othersDeclined", { reason: status.roomDeclined });
  else if (!status.roomAnswered) headline = t("developer.othersNoRoom");
  else if (running) headline = t("developer.othersCollecting", { n: status.devices.filter(settled).length, total: status.devices.length });
  else if (status.devices.length === 0) headline = t("developer.othersNone");
  else headline = t("developer.othersDone", { n: status.devices.filter((d) => d.state === "received").length, total: status.devices.length, seconds: (status.elapsedMs / 1000).toFixed(1) });
  const asked = devMode && status !== undefined && status.unavailable === undefined && !status.roomDeclined;
  return (
    <section className="card developer-section developer-room" data-testid="developer-room" aria-busy={running}>
      <div className="developer-room-head">
        <h2 className="card-title">{t("developer.others")}</h2>
        {/* Announced once it settles, not on every part that arrives. */}
        <span className={`developer-room-headline muted small ${running ? "running" : ""}`} role="status" aria-live={running ? "off" : "polite"} data-testid="developer-room-status">
          {running && devMode ? <Icon name="spinner" size={14} className="spin" /> : null}
          {headline}
        </span>
      </div>
      <p className="muted small">{t("developer.othersHint")}</p>
      {asked ? (
        <ul className="developer-sources" data-testid="developer-sources">
          <CoordinatorRow status={status} running={running} viaCoordinator={viaCoordinator} />
          {status.devices.map((d) => (
            <SourceRow key={d.id} icon={deviceIcon(devices.find((x) => x.id === d.id))} name={d.name} source={d} testId={`developer-source-${d.id}`} />
          ))}
        </ul>
      ) : null}
    </section>
  );
}

/** The device has had its say (or the collection is over for it). */
function settled(d: DiagnosticsSourceStatus): boolean {
  return d.state !== "waiting" && d.state !== "receiving";
}

function deviceIcon(d: DeviceInfo | undefined): string {
  if (!d) return "devices";
  return d.platform === "android" ? "phone" : "laptop";
}

function CoordinatorRow({ status, running, viaCoordinator }: { status: RoomDiagnosticsStatus; running: boolean; viaCoordinator: boolean }) {
  const c = status.coordinator;
  if (c) {
    const detail = c.state === "received" ? (status.coordinatorLog ? t("developer.coordinatorLog") : t("developer.coordinatorVersion")) : undefined;
    return <SourceRow icon="dns" name={t("developer.coordinator")} source={c} detail={detail} testId="developer-source-coordinator" />;
  }
  // It answers ahead of the room: once the room has, there is none, or (connected to one) it is too old to.
  const text = !status.roomAnswered ? (running ? t("developer.coordinatorWaiting") : t("developer.othersNoRoom")) : viaCoordinator ? t("developer.coordinatorSilent") : t("developer.coordinatorNone");
  return (
    <li className="developer-source" data-testid="developer-source-coordinator" data-state={status.roomAnswered ? "none" : running ? "waiting" : "noAnswer"}>
      <Icon name="dns" size={20} className="developer-source-icon" />
      <div className="developer-source-text">
        <div className="developer-source-name">{t("developer.coordinator")}</div>
        <div className="muted small">{text}</div>
      </div>
      <span className={`badge developer-state ${status.roomAnswered ? "none" : running ? "waiting" : "noAnswer"}`}>{status.roomAnswered ? t("developer.state.none") : running ? t("developer.state.waiting") : t("developer.state.noAnswer")}</span>
    </li>
  );
}

function SourceRow({ icon, name, source, detail, testId }: { icon: string; name: string; source: DiagnosticsSourceStatus; detail?: string; testId: string }) {
  const s = source;
  const label = {
    waiting: t("developer.state.waiting"),
    receiving: t("developer.state.receiving", { received: s.partsReceived, parts: s.parts }),
    received: t("developer.state.received", { size: fmtBytes(s.bytes) }),
    declined: t("developer.state.declined"),
    incomplete: t("developer.state.incomplete", { received: s.partsReceived, parts: s.parts }),
    noAnswer: t("developer.state.noAnswer"),
  }[s.state];
  const stateIcon = { waiting: "schedule", receiving: "spinner", received: "check", declined: "close", incomplete: "warn", noAnswer: "warn" }[s.state];
  const sub = s.state === "declined" ? s.reason : detail;
  return (
    <li className="developer-source" data-testid={testId} data-state={s.state}>
      <Icon name={icon} size={20} className="developer-source-icon" />
      <div className="developer-source-text">
        <div className="developer-source-name">{name}</div>
        {sub ? <div className="muted small">{sub}</div> : null}
      </div>
      <span className={`badge developer-state ${s.state}`}><Icon name={stateIcon} size={12} className={s.state === "receiving" ? "spin" : ""} /> {label}</span>
    </li>
  );
}
