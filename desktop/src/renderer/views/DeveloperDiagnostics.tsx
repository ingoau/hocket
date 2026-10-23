// Developer diagnostics: the last hour of logs plus versions and device details, to save, copy,
// show in the file manager or (macOS) share, optionally with the bundles of the other devices in
// the Connect room. Main builds the bundle on every action; the page only previews this device's.
// Opened from Settings → Advanced, or with the developer-mode shortcut.
import { useCallback, useEffect, useRef, useState } from "react";
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
  const [summary, setSummary] = useState<DiagnosticsSummary | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [allDevices, setAllDevices] = useState(false);
  // Other devices only ever answer while developer mode is on here too.
  const options: DiagnosticsOptions = { allDevices: devMode && allDevices };
  /** The running action also asks the other devices (as it started, whatever the switches say now). */
  const [collectingOthers, setCollectingOthers] = useState(false);

  const refresh = useCallback(() => {
    void bridge()
      .diagnostics.summary()
      .then(setSummary)
      .catch((err: unknown) => console.error("diagnostics summary failed", err));
  }, []);
  useEffect(refresh, [refresh]);
  // The newest lines are what matter: keep the preview scrolled to its end.
  const logRef = useRef<HTMLPreElement>(null);
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [summary]);

  /** Runs one action; the page refreshes afterwards so the preview matches what was written. */
  const run = (action: () => Promise<string | undefined>) => {
    setBusy(true);
    setCollectingOthers(options.allDevices === true);
    void action()
      .then((message) => {
        if (message) showToast(message);
      })
      .catch((err: unknown) => showToast(t("developer.failed", { error: err instanceof Error ? err.message : String(err) })))
      .finally(() => {
        setBusy(false);
        setCollectingOthers(false);
        refresh();
      });
  };
  const fileName = (path: string) => path.split(/[/\\]/).pop() ?? path;

  return (
    <div className="view page-developer" data-testid="view-developer">
      <div className="view-header">
        <h1 className="brand-type">{t("developer.title")}</h1>
        <div className="actions">
          <button type="button" className="btn" onClick={refresh} disabled={busy} data-testid="developer-refresh"><Icon name="refresh" size={16} /> {t("developer.refresh")}</button>
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
          {devMode ? (
            <div className="developer-mode card" data-testid="developer-all-devices-card">
              <Icon name="devices" size={22} className="developer-mode-icon" />
              <div className="developer-mode-text">
                {t("developer.allDevices")}
                <div className="muted small">{t("developer.allDevicesHint")}</div>
              </div>
              <Switch checked={allDevices} onChange={setAllDevices} aria-label={t("developer.allDevices")} data-testid="developer-all-devices-switch" />
            </div>
          ) : null}
          <div className="developer-actions">
            <button type="button" className="btn primary" disabled={busy} onClick={() => run(async () => { const p = await bridge().diagnostics.save(options); return p ? t("developer.saved", { name: fileName(p) }) : undefined; })} data-testid="developer-save"><Icon name="save" size={16} /> {t("developer.save")}</button>
            {summary?.canShare ? (
              <button type="button" className="btn" disabled={busy} onClick={() => run(async () => { await bridge().diagnostics.share(options); return undefined; })} data-testid="developer-share"><Icon name="share" size={16} /> {t("developer.share")}</button>
            ) : null}
            <button type="button" className="btn" disabled={busy} onClick={() => run(async () => { await bridge().diagnostics.copy(options); return t("developer.copied"); })} data-testid="developer-copy"><Icon name="copy" size={16} /> {t("developer.copy")}</button>
            <button type="button" className="btn" disabled={busy} onClick={() => run(async () => { const p = await bridge().diagnostics.reveal(options); return p ? t("developer.revealed", { name: fileName(p) }) : undefined; })} data-testid="developer-reveal"><Icon name="folder" size={16} /> {t("developer.reveal")}</button>
            {busy && collectingOthers ? <span className="muted small" role="status" data-testid="developer-collecting">{t("developer.collecting")}</span> : null}
            {summary ? <span className="muted small developer-stats" data-testid="developer-stats">{t("developer.stats", { lines: summary.logLines, size: fmtBytes(summary.logBytes), minutes: Math.round(summary.windowMs / 60_000) })}</span> : null}
          </div>
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
