// Setup: the entire first screen, nothing else. "Where's your music?" offers a
// card per source (a Navidrome server, or another device's setup link); the
// Navidrome card's form takes the server's address, then username and password.
// Once the server answers, the card turns into what was found (the Navidrome
// version, who is signed in where, which features it offers) while the first
// library sync runs, then "Add another source?" before the app. Settings →
// Accounts reuses the same cards to add one.
import { useEffect, useState, useSyncExternalStore } from "react";
import type { AccountInfo } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { normaliseServerUrl } from "../lib/server-url";
import { parseDeepLink, type SetupLink } from "@shared/deep-link";
import { unpackSetupSettings } from "@shared/setup-settings";
import { CredentialWarning } from "../components/CredentialWarning";
import { Icon } from "../components/Icon";
import { BrandMark } from "../components/BrandMark";

// Whether this window is showing setup over an app that has an account: the post-connect
// summary, "Add another source?", or the form while adding one. Set when Connect is pressed (or
// Settings → Accounts asks to add one), cleared by Done. App keeps Setup on screen while it is set.
let summaryOpen = false;
// When Connect was pressed: the summary's sync is the first library sync queued after it.
let openedAt = 0;
// What setup shows while it is open over an account: the summary of the one just added, the
// "Add another source?" cards, or a form for the next.
type Phase = "summary" | "another" | "form";
let phase: Phase = "summary";
// "Use a different server" was pressed for this account: the summary stays up until it is gone
// (so the app behind it never shows for an account being removed), then gives way to the form.
let leaving: string | undefined;
const listeners = new Set<() => void>();
function notify(): void {
  for (const l of listeners) l();
}
function setSummary(open: boolean): void {
  summaryOpen = open;
  if (open) {
    openedAt = Date.now();
    phase = "summary";
  }
  notify();
}
function setPhase(next: Phase): void {
  phase = next;
  notify();
}
/** Settings → Accounts' Add: the setup cards, over the app. */
export function startAddAccount(): void {
  summaryOpen = true;
  phase = "form";
  notify();
}
/** True from pressing Connect until Done on "Add another source?". */
export function useSetupSummary(): boolean {
  return useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => summaryOpen);
}
function usePhase(): Phase {
  return useSyncExternalStore((l) => { listeners.add(l); return () => listeners.delete(l); }, () => phase);
}

export function Setup() {
  const accounts = useApp((s) => s.accounts);
  // The account just added goes last in the priority order.
  const server = accounts[accounts.length - 1];
  const summary = useSetupSummary();
  const current = usePhase();
  useEffect(() => {
    if (leaving && !accounts.some((a) => a.id === leaving)) {
      leaving = undefined;
      if (accounts.length) setPhase("form");
      else setSummary(false);
    }
  }, [accounts]);
  const body = !server || !summary
    ? <Sources />
    : current === "another"
      ? <AddAnother />
      : current === "form"
        ? <Sources adding />
        : <Summary server={server} />;
  return (
    <main className="setup" data-testid="setup">
      {/* The lockup (docs/brand.md): the bare wave and the lowercase word mark, read once as "Hocket". */}
      <p className="brand brand-lockup setup-lockup"><BrandMark /><span aria-hidden="true">hocket</span><span className="sr-only">{t("app.name")}</span></p>
      {body}
    </main>
  );
}

/**
 * "Where's your music?": a card per kind of source. The Navidrome card is the
 * sign-in form itself; the other device's card points at it, since a setup link
 * pasted into Server URL fills the form in.
 */
function Sources({ adding = false }: { adding?: boolean }) {
  const [fromDevice, setFromDevice] = useState(false);
  const pickDevice = () => {
    setFromDevice(true);
    document.querySelector<HTMLInputElement>("[data-testid='setup-url']")?.focus();
  };
  return (
    <div className="setup-sources" data-testid="setup-sources">
      <h1 className="setup-heading">{adding ? t("setup.sources.addTitle") : t("setup.sources.title")}</h1>
      <ConnectForm fromDevice={fromDevice} adding={adding} />
      {adding ? (
        <div className="setup-buttons">
          <button type="button" className="btn tonal" onClick={() => setSummary(false)} data-testid="setup-cancel">{t("setup.sources.cancel")}</button>
        </div>
      ) : (
        <button type="button" className="setup-source" onClick={pickDevice} aria-pressed={fromDevice} data-testid="setup-source-device">
          <Icon name="devices" size={24} />
          <span className="setup-source-text"><span className="setup-source-title">{t("setup.sources.device")}</span><span className="setup-hint">{t("setup.sources.deviceHint")}</span></span>
        </button>
      )}
    </div>
  );
}

/** After the first source: another one, or into the app. */
function AddAnother() {
  return (
    <section className="setup-card setup-sources" aria-labelledby="setup-another-title" data-testid="setup-another">
      <h1 id="setup-another-title" className="setup-title">{t("setup.another.title")}</h1>
      <p className="setup-subtitle">{t("setup.another.subtitle")}</p>
      <div className="setup-source-list">
        <button type="button" className="setup-source" onClick={() => setPhase("form")} data-testid="setup-another-navidrome">
          <Icon name="library" size={24} />
          <span className="setup-source-text"><span className="setup-source-title">{t("setup.sources.navidrome")}</span><span className="setup-hint">{t("setup.sources.navidromeHint")}</span></span>
        </button>
      </div>
      <div className="setup-buttons">
        <button type="button" className="btn primary" onClick={() => setSummary(false)} autoFocus data-testid="setup-done">{t("setup.another.done")}</button>
      </div>
    </section>
  );
}

function ConnectForm({ fromDevice, adding }: { fromDevice: boolean; adding: boolean }) {
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
  // The coordinator, or else the (packed) settings, from a pasted setup link: applied as the form
  // connects, and dropped once the person types a server or user of their own.
  const [linkCoordinator, setLinkCoordinator] = useState<string | undefined>(undefined);
  const [linkSettings, setLinkSettings] = useState<string | undefined>(undefined);
  const connect = (rawUrl: string, user: string, pass: string, coordinator = linkCoordinator, settings = linkSettings) => {
    const u = normaliseServerUrl(rawUrl);
    if (!u) return setError(t("setup.errorUrl"));
    if (!user.trim() || !pass) return setError(t("setup.errorCredentials"));
    setError(undefined);
    setBusy(true);
    setSubmittedAt(Date.now());
    setSummary(true);
    if (coordinator) {
      bridge().dispatch({ type: "setCoordinatorUrl", data: { url: coordinator } });
      bridge().dispatch({ type: "connectCoordinator" });
    }
    if (settings) {
      void unpackSetupSettings(settings).then((json) => {
        if (json) bridge().dispatch({ type: "applySetupSettings", data: { settings: json } });
      });
    }
    bridge().dispatch({ type: "addAccount", data: { setup: { type: "navidrome", data: { url: u, username: user.trim(), password: pass, name: undefined } } } });
  };
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    connect(url, username, password);
  };
  // Another device's setup code (Set up another device): fills the form (its
  // coordinator or settings are applied as the form connects). Pasted here
  // (fromHere) with the password, it connects straight away. A hocket:// link
  // opened from outside (any web page can open one) only fills in the server and
  // user and waits for Connect: its coordinator and settings are dropped, since
  // neither shows in the form and a coordinator receives the account's
  // credential when the device joins it.
  const applySetupLink = (link: SetupLink, fromHere: boolean) => {
    setUrl(link.server);
    setUsername(link.user);
    setPassword(link.password ?? "");
    setLinkCoordinator(fromHere ? link.coordinator : undefined);
    setLinkSettings(fromHere ? link.settings : undefined);
    setError(undefined);
    if (fromHere && link.password && !busy) connect(link.server, link.user, link.password, link.coordinator, link.settings);
  };
  const pendingSetup = useApp((s) => s.pendingSetup);
  useEffect(() => {
    if (!pendingSetup) return;
    useApp.getState().setPendingSetup(undefined);
    applySetupLink(pendingSetup, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- applies each link once, as it arrives
  }, [pendingSetup]);
  const onUrl = (value: string) => {
    // A pasted setup link, rather than an address (even pasted into a field that holds one).
    const at = value.search(/hocket:\/\/setup\?/i);
    const link = at >= 0 ? parseDeepLink(value.slice(at).trim()) : undefined;
    if (link?.kind === "setup") return applySetupLink(link, true);
    setUrl(value);
    forgetLink();
  };
  function forgetLink() {
    setLinkCoordinator(undefined);
    setLinkSettings(undefined);
  }
  const ready = !!url.trim() && !!username.trim() && !!password;
  return (
    <form className="setup-card" onSubmit={submit} aria-labelledby="setup-title" aria-describedby="setup-subtitle">
      <h2 id="setup-title" className="setup-title">{fromDevice ? t("setup.device.title") : t("setup.sources.navidrome")}</h2>
      <p className="setup-subtitle" id="setup-subtitle">{fromDevice ? t("setup.device.subtitle") : t("setup.subtitle")}</p>
      <CredentialWarning />
      <label className="setup-field">{t("setup.url")}<input className="input" type="url" aria-required="true" aria-invalid={error === t("setup.errorUrl") || undefined} aria-describedby={[error ? "setup-error" : "", "setup-url-hint", cleartext ? "setup-cleartext" : ""].filter(Boolean).join(" ")} placeholder={t("setup.urlPlaceholder")} value={url} onChange={(e) => onUrl(e.target.value)} autoFocus data-testid="setup-url" /><span className="setup-hint" id="setup-url-hint">{t("setup.urlHint")}</span></label>
      {cleartext ? <div className="setup-cleartext" id="setup-cleartext" data-testid="setup-cleartext"><Icon name="warn" size={18} />{t("setup.cleartext")}</div> : null}
      <div className="setup-pair">
        <label className="setup-field">{t("setup.username")}<input className="input" aria-required="true" value={username} onChange={(e) => { setUsername(e.target.value); forgetLink(); }} autoComplete="username" data-testid="setup-username" /></label>
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
        {adding ? null : <button type="button" className="btn text" onClick={() => void bridge().config.import().catch((err: unknown) => console.error("config import failed", err))} data-testid="setup-import"><Icon name="restore" size={18} />{t("setup.import")}</button>}
        <span className="setup-hint" aria-hidden="true">{t("setup.enterHint")}</span>
      </div>
    </form>
  );
}

/** What the server turned out to be, while the first sync runs. */
function Summary({ server }: { server: AccountInfo }) {
  const jobs = useApp((s) => s.jobs);
  // This server's first sync; until the core queues it, it is about to start (0%), not done.
  const sync = jobs.filter((j) => j.kind === "librarySync" && j.createdAt >= openedAt - 1000).sort((a, b) => b.createdAt - a.createdAt)[0];
  const failed = sync?.state === "failed" || sync?.state === "cancelled";
  const synced = sync ? sync.state === "done" : !!server.lastSync;
  const pct = synced ? 100 : sync?.total ? Math.min(100, Math.round((sync.done / sync.total) * 100)) : 0;
  const c = server.capabilities;
  const host = server.url.replace(/^https?:\/\//, "").replace(/\/$/, "");
  const caps: [string, boolean][] = [[t("setup.cap.lyrics"), c.songLyrics], [t("setup.cap.similar"), c.sonicSimilarity], [t("setup.cap.smart"), c.nativeApi]];
  const different = () => { leaving = server.id; bridge().dispatch({ type: "removeAccount", data: { account_id: server.id } }); };
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
        <button type="button" className="btn primary" onClick={() => setPhase("another")} autoFocus data-testid="setup-continue">{t("setup.continue")}</button>
      </div>
    </section>
  );
}
