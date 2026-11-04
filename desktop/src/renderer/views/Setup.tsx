// Server setup: the entire first screen, nothing else. The lockup over one
// card: the server's address, then username and password side by side. Once
// the server answers, the card turns into what was found (the Navidrome
// version, who is signed in where, which features it offers) while the first
// library sync runs, with Continue into the app or Use a different server.
import { useEffect, useState, useSyncExternalStore } from "react";
import type { ServerInfo } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { normaliseServerUrl } from "../lib/server-url";
import { CredentialWarning } from "../components/CredentialWarning";
import { Icon } from "../components/Icon";
import { BrandMark } from "../components/BrandMark";

// Whether this window is showing the post-connect summary: set when Connect is
// pressed, cleared by Continue. App keeps Setup on screen while it is set.
let summaryOpen = false;
// When Connect was pressed: the summary's sync is the first library sync queued after it.
let openedAt = 0;
// "Use a different server" was pressed: the summary stays up until the server is gone (so the app
// behind it never shows for a server being removed), then gives way to the form.
let leaving = false;
const listeners = new Set<() => void>();
function setSummary(open: boolean): void {
  summaryOpen = open;
  if (open) openedAt = Date.now();
  for (const l of listeners) l();
}
/** True from pressing Connect until Continue on the summary. */
export function useSetupSummary(): boolean {
  return useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => summaryOpen);
}

export function Setup() {
  const server = useApp((s) => s.servers[0]);
  const summary = useSetupSummary();
  useEffect(() => {
    if (!server && leaving) { leaving = false; setSummary(false); }
  }, [server]);
  return (
    <main className="setup" data-testid="setup">
      {/* The lockup (docs/brand.md): the bare wave and the lowercase word mark, read once as "Hocket". */}
      <p className="brand brand-lockup setup-lockup"><BrandMark /><span aria-hidden="true">hocket</span><span className="sr-only">{t("app.name")}</span></p>
      {server && summary ? <Summary server={server} /> : <ConnectForm />}
    </main>
  );
}

function ConnectForm() {
  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const lastError = useApp((s) => s.lastError);
  const [submittedAt, setSubmittedAt] = useState(0);

  useEffect(() => {
    if (lastError && lastError.at >= submittedAt && busy) {
      setBusy(false);
      setSummary(false);
      setError(t("setup.failed", { message: lastError.detail && lastError.detail !== lastError.message ? `${lastError.message}: ${lastError.detail}` : lastError.message }));
    }
  }, [lastError, submittedAt, busy]);

  const cleartext = /^http:\/\/./i.test(url.trim());
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const u = normaliseServerUrl(url);
    if (!u) return setError(t("setup.errorUrl"));
    if (!username.trim() || !password) return setError(t("setup.errorCredentials"));
    setError(undefined);
    setBusy(true);
    setSubmittedAt(Date.now());
    setSummary(true);
    bridge().dispatch({ type: "addServer", data: { url: u, username: username.trim(), password, name: undefined } });
  };
  const ready = !!url.trim() && !!username.trim() && !!password;
  return (
    <form className="setup-card" onSubmit={submit} aria-labelledby="setup-title" aria-describedby="setup-subtitle">
      <h1 id="setup-title" className="setup-title">{t("setup.title")}</h1>
      <p className="setup-subtitle" id="setup-subtitle">{t("setup.subtitle")}</p>
      <CredentialWarning />
      <label className="setup-field">{t("setup.url")}<input className="input" type="url" aria-required="true" aria-invalid={error === t("setup.errorUrl") || undefined} aria-describedby={[error ? "setup-error" : "", "setup-url-hint", cleartext ? "setup-cleartext" : ""].filter(Boolean).join(" ")} placeholder={t("setup.urlPlaceholder")} value={url} onChange={(e) => setUrl(e.target.value)} autoFocus data-testid="setup-url" /><span className="setup-hint" id="setup-url-hint">{t("setup.urlHint")}</span></label>
      {cleartext ? <div className="setup-cleartext" id="setup-cleartext" data-testid="setup-cleartext"><Icon name="warn" size={18} />{t("setup.cleartext")}</div> : null}
      <div className="setup-pair">
        <label className="setup-field">{t("setup.username")}<input className="input" aria-required="true" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" data-testid="setup-username" /></label>
        <label className="setup-field">{t("setup.password")}
          <span className="setup-password">
            <input className="input" type={showPassword ? "text" : "password"} aria-required="true" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" data-testid="setup-password" />
            <button type="button" className="btn icon sm" aria-label={showPassword ? t("setup.hidePassword") : t("setup.showPassword")} title={showPassword ? t("setup.hidePassword") : t("setup.showPassword")} aria-pressed={showPassword} onClick={() => setShowPassword(!showPassword)}><Icon name={showPassword ? "visibilityOff" : "visibility"} size={20} /></button>
          </span>
        </label>
      </div>
      {error ? <div className="error" id="setup-error" role="alert" data-testid="setup-error"><Icon name="warn" size={18} filled />{error}</div> : null}
      <button type="submit" className={`btn connect ${ready || busy ? "primary" : "tonal"}`} disabled={busy} data-testid="setup-connect">{busy ? <Icon name="spinner" size={20} className="spin" /> : null}{busy ? t("setup.connecting") : t("setup.connect")}</button>
      <div className="setup-foot">
        <button type="button" className="btn text" onClick={() => void bridge().config.import().catch((err: unknown) => console.error("config import failed", err))} data-testid="setup-import"><Icon name="restore" size={18} />{t("setup.import")}</button>
        <span className="setup-hint" aria-hidden="true">{t("setup.enterHint")}</span>
      </div>
    </form>
  );
}

/** What the server turned out to be, while the first sync runs. */
function Summary({ server }: { server: ServerInfo }) {
  const jobs = useApp((s) => s.jobs);
  // This server's first sync; until the core queues it, it is about to start (0%), not done.
  const sync = jobs.filter((j) => j.kind === "librarySync" && j.createdAt >= openedAt - 1000).sort((a, b) => b.createdAt - a.createdAt)[0];
  const failed = sync?.state === "failed" || sync?.state === "cancelled";
  const synced = sync ? sync.state === "done" : !!server.lastSync;
  const pct = synced ? 100 : sync?.total ? Math.min(100, Math.round((sync.done / sync.total) * 100)) : 0;
  const c = server.capabilities;
  const host = server.url.replace(/^https?:\/\//, "").replace(/\/$/, "");
  const caps: [string, boolean][] = [[t("setup.cap.lyrics"), c.songLyrics], [t("setup.cap.similar"), c.sonicSimilarity], [t("setup.cap.smart"), c.nativeApi]];
  const different = () => { leaving = true; bridge().dispatch({ type: "removeServer", data: { server_id: server.id } }); };
  return (
    <section className="setup-card setup-summary" aria-labelledby="setup-found" data-testid="setup-summary">
      <h1 id="setup-found" className={`setup-version ${c.meetsFloor ? "" : "old"}`}>{c.serverVersion ? t("setup.version", { version: c.serverVersion }) : t("setup.connected")}</h1>
      <p className="setup-who">{t("setup.who", { user: server.username, host })}</p>
      {!c.meetsFloor ? <div className="setup-old" role="alert">{t("setup.tooOld", { version: c.serverVersion ?? "?" })}</div> : null}
      <ul className="plain-list setup-caps" aria-label={t("setup.capabilities")}>
        {caps.map(([label, ok]) => <li key={label} className={`setup-cap ${ok ? "ok" : ""}`}><Icon name={ok ? "check" : "close"} size={18} />{label}<span className="sr-only">, {ok ? t("setup.cap.yes") : t("setup.cap.no")}</span></li>)}
      </ul>
      {failed ? <div className="setup-old" role="alert" data-testid="setup-sync-failed">{t("setup.syncFailed")}</div> : (
        <>
          <div className="setup-sync" role="status"><span>{synced ? t("setup.synced") : t("setup.syncing")}</span><span>{pct}%</span></div>
          <div className="setup-meter" role="progressbar" aria-label={t("setup.syncing")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}><span style={{ width: `${pct}%` }} /></div>
        </>
      )}
      {/* An older server still works, with fewer features (the warning above says which), so Continue is always offered. */}
      <div className="setup-buttons">
        <button type="button" className="btn tonal" onClick={different} data-testid="setup-different">{t("setup.different")}</button>
        <button type="button" className="btn primary" onClick={() => setSummary(false)} autoFocus data-testid="setup-continue">{t("setup.continue")}</button>
      </div>
    </section>
  );
}
