// Setup: the entire first screen, nothing else. "Where's your music?" offers a
// card per source (a Navidrome server, or pairing with another device); the
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
import { parseDeepLink } from "@shared/deep-link";
import { CredentialWarning } from "../components/CredentialWarning";
import { Icon } from "../components/Icon";
import { BrandMark } from "../components/BrandMark";
import { BareTitleBar } from "../components/TopBar";

// Whether this window is showing setup over an app that has an account: the post-connect
// summary, "Add another source?", or the form while adding one. Set when Connect is pressed (or
// Settings → Accounts asks to add one), cleared by Done. App keeps Setup on screen while it is set.
let summaryOpen = false;
// When Connect was pressed: the summary's sync is the first library sync queued after it.
let openedAt = 0;
// Which source card the form opens on ("Add another source?" picks one).
type Choice = "navidrome" | "device" | "ytm";
let initialChoice: Choice = "navidrome";
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
    <div className="setup-shell">
      <BareTitleBar />
      <main className="setup" data-testid="setup">
        {/* The lockup (docs/brand.md): the bare wave and the lowercase word mark, read once as "Hocket". */}
        <p className="brand brand-lockup setup-lockup"><BrandMark /><span aria-hidden="true">hocket</span><span className="sr-only">{t("app.name")}</span></p>
        {body}
      </main>
    </div>
  );
}

/**
 * "Where's your music?": a card per kind of source. The Navidrome card is the
 * sign-in form itself; the other device's card pairs with it (a pairing link
 * from its Add a device). Once paired, the form asks for the passwords of the
 * accounts the other device named (unless credential sync brought them).
 */
function Sources({ adding = false }: { adding?: boolean }) {
  const [choice, setChoice] = useState<Choice>(() => {
    const c = initialChoice;
    initialChoice = "navidrome";
    return c;
  });
  const finished = useApp((s) => s.pairing.finished);
  const pendingPair = useApp((s) => s.pendingPair);
  useEffect(() => {
    if (pendingPair) setChoice("device");
  }, [pendingPair]);
  const waiting = finished?.[0];
  const body = choice === "ytm"
    ? <YtmCard />
    : choice === "device" && finished === undefined
      ? <PairForm />
      : <ConnectForm adding={adding} initial={waiting && { url: waiting.url, username: waiting.username }} />;
  return (
    <div className="setup-sources" data-testid="setup-sources">
      <h1 className="setup-heading">{adding ? t("setup.sources.addTitle") : t("setup.sources.title")}</h1>
      {body}
      {choice !== "navidrome" ? (
        <button type="button" className="setup-source" onClick={() => setChoice("navidrome")} data-testid="setup-source-navidrome">
          <Icon name="library" size={24} />
          <span className="setup-source-text"><span className="setup-source-title">{t("setup.sources.navidrome")}</span><span className="setup-hint">{t("setup.sources.navidromeHint")}</span></span>
        </button>
      ) : null}
      {choice !== "ytm" ? (
        <button type="button" className="setup-source" onClick={() => setChoice("ytm")} data-testid="setup-source-ytm">
          <Icon name="cloud" size={24} />
          <span className="setup-source-text"><span className="setup-source-title">{t("setup.sources.ytm")}</span><span className="setup-hint">{t("setup.sources.ytmHint")}</span></span>
        </button>
      ) : null}
      {adding ? (
        <div className="setup-buttons">
          <button type="button" className="btn tonal" onClick={() => setSummary(false)} data-testid="setup-cancel">{t("setup.sources.cancel")}</button>
        </div>
      ) : choice !== "device" ? (
        <button type="button" className="setup-source" onClick={() => setChoice("device")} aria-pressed={false} data-testid="setup-source-device">
          <Icon name="devices" size={24} />
          <span className="setup-source-text"><span className="setup-source-title">{t("setup.sources.device")}</span><span className="setup-hint">{t("setup.sources.deviceHint")}</span></span>
        </button>
      ) : null}
    </div>
  );
}

/** YouTube Music: sign in with Google (a window of its own), or carry on anonymously. */
function YtmCard() {
  const [busy, setBusy] = useState<"signIn" | "anon" | undefined>(undefined);
  const [error, setError] = useState<string | undefined>(undefined);
  const signIn = () => {
    setBusy("signIn");
    setError(undefined);
    bridge()
      .ytm.signIn()
      .then((r) => {
        if (r === "signedIn") {
          setSummary(true);
        } else {
          // "busy": another sign-in window was open (main focused it); nothing failed here.
          setBusy(undefined);
          if (r === "cancelled") setError(t("setup.ytm.cancelled"));
        }
      })
      .catch((err: unknown) => {
        console.error("YouTube Music sign-in failed", err);
        setBusy(undefined);
      });
  };
  const anonymous = () => {
    setBusy("anon");
    setSummary(true);
    bridge().dispatch({ type: "addAccount", data: { setup: { type: "youtubeMusic", data: { cookies: undefined, js_host: true } } } });
  };
  return (
    <section className="setup-card" aria-labelledby="setup-ytm-title" data-testid="setup-ytm">
      <h2 id="setup-ytm-title" className="setup-title">{t("setup.ytm.title")}</h2>
      <p className="setup-subtitle">{t("setup.ytm.subtitle")}</p>
      {error ? <div className="error" role="alert" data-testid="setup-error"><Icon name="warn" size={18} filled />{error}</div> : null}
      <button type="button" className="btn connect primary" disabled={!!busy} onClick={signIn} autoFocus data-testid="setup-ytm-signin">{busy === "signIn" ? <Icon name="spinner" size={20} className="spin" /> : null}{busy === "signIn" ? t("setup.ytm.signingIn") : t("setup.ytm.signIn")}</button>
      <button type="button" className="btn tonal" disabled={!!busy} onClick={anonymous} data-testid="setup-ytm-anon">{t("setup.ytm.anonymous")}</button>
    </section>
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
        <button type="button" className="setup-source" onClick={() => { initialChoice = "ytm"; setPhase("form"); }} data-testid="setup-another-ytm">
          <Icon name="cloud" size={24} />
          <span className="setup-source-text"><span className="setup-source-title">{t("setup.sources.ytm")}</span><span className="setup-hint">{t("setup.sources.ytmHint")}</span></span>
        </button>
      </div>
      <div className="setup-buttons">
        <button type="button" className="btn primary" onClick={() => setSummary(false)} autoFocus data-testid="setup-done">{t("setup.another.done")}</button>
      </div>
    </section>
  );
}

/** Paste (or open) another device's pairing link; the core does the exchange. */
function PairForm() {
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState(false);
  const error = useApp((s) => s.pairing.error);
  const failures = useApp((s) => s.pairing.failures);
  const pendingPair = useApp((s) => s.pendingPair);
  const join = (value: string) => {
    const at = value.search(/hocket:\/\/pair\?/i);
    const parsed = at >= 0 ? parseDeepLink(value.slice(at).trim()) : undefined;
    if (parsed?.kind !== "pair") return;
    setBusy(true);
    bridge().dispatch({ type: "pairWithInvite", data: { link: value.slice(at).trim() } });
  };
  useEffect(() => {
    if (!pendingPair) return;
    useApp.getState().setPendingPair(undefined);
    setLink(pendingPair);
    join(pendingPair);
  }, [pendingPair]);
  // Each failure ends the attempt, even one with the same message as the last.
  useEffect(() => {
    if (failures) setBusy(false);
  }, [failures]);
  return (
    <form className="setup-card" onSubmit={(e) => { e.preventDefault(); join(link); }} aria-labelledby="setup-title" aria-describedby="setup-subtitle" data-testid="setup-pair">
      <h2 id="setup-title" className="setup-title">{t("setup.device.title")}</h2>
      <p className="setup-subtitle" id="setup-subtitle">{t("setup.device.subtitle")}</p>
      <label className="setup-field">{t("setup.device.link")}<input className="input" placeholder="hocket://pair?d=…" value={link} onChange={(e) => setLink(e.target.value)} autoFocus data-testid="setup-pair-link" /></label>
      {error && !busy ? <div className="error" role="alert" data-testid="setup-error"><Icon name="warn" size={18} filled />{error}</div> : null}
      <button type="submit" className="btn connect primary" disabled={busy || !/hocket:\/\/pair\?/i.test(link)} data-testid="setup-pair-join">{busy ? <Icon name="spinner" size={20} className="spin" /> : null}{busy ? t("setup.device.joining") : t("setup.device.join")}</button>
    </form>
  );
}

function ConnectForm({ adding, initial }: { adding: boolean; initial?: { url: string; username: string } }) {
  const [url, setUrl] = useState(initial?.url ?? "");
  const [username, setUsername] = useState(initial?.username ?? "");
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
  const connect = (rawUrl: string, user: string, pass: string) => {
    const u = normaliseServerUrl(rawUrl);
    if (!u) return setError(t("setup.errorUrl"));
    if (!user.trim() || !pass) return setError(t("setup.errorCredentials"));
    setError(undefined);
    setBusy(true);
    setSubmittedAt(Date.now());
    setSummary(true);
    bridge().dispatch({ type: "addAccount", data: { setup: { type: "navidrome", data: { url: u, username: user.trim(), password: pass, name: undefined } } } });
  };
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    connect(url, username, password);
  };
  const onUrl = (value: string) => {
    // A pasted pairing link, rather than an address: pair with that device instead.
    const at = value.search(/hocket:\/\/pair\?/i);
    if (at >= 0 && parseDeepLink(value.slice(at).trim())?.kind === "pair") {
      useApp.getState().setPendingPair(value.slice(at).trim());
      return;
    }
    setUrl(value);
  };
  const ready = !!url.trim() && !!username.trim() && !!password;
  return (
    <form className="setup-card" onSubmit={submit} aria-labelledby="setup-title" aria-describedby="setup-subtitle">
      <h2 id="setup-title" className="setup-title">{t("setup.sources.navidrome")}</h2>
      <p className="setup-subtitle" id="setup-subtitle">{initial ? t("setup.device.password", { user: initial.username, host: initial.url.replace(/^https?:\/\//, "") }) : t("setup.subtitle")}</p>
      <CredentialWarning />
      <label className="setup-field">{t("setup.url")}<input className="input" type="url" aria-required="true" aria-invalid={error === t("setup.errorUrl") || undefined} aria-describedby={[error ? "setup-error" : "", "setup-url-hint", cleartext ? "setup-cleartext" : ""].filter(Boolean).join(" ")} placeholder={t("setup.urlPlaceholder")} value={url} onChange={(e) => onUrl(e.target.value)} autoFocus data-testid="setup-url" /><span className="setup-hint" id="setup-url-hint">{t("setup.urlHint")}</span></label>
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
        {adding ? null : <button type="button" className="btn text" onClick={() => void bridge().config.import().catch((err: unknown) => console.error("config import failed", err))} data-testid="setup-import"><Icon name="restore" size={18} />{t("setup.import")}</button>}
        <span className="setup-hint" aria-hidden="true">{t("setup.enterHint")}</span>
      </div>
    </form>
  );
}

/** What the server turned out to be, while the first sync runs. */
function Summary({ server }: { server: AccountInfo }) {
  if (server.kind === "youtubeMusic") return <YtmSummary account={server} />;
  return <NavidromeSummary server={server} />;
}

/** A YouTube Music account: who is signed in (or nobody), what it offers, its first sync. */
function YtmSummary({ account }: { account: AccountInfo }) {
  const jobs = useApp((s) => s.jobs);
  const signedIn = account.features.librarySync;
  const sync = jobs.filter((j) => j.kind === "librarySync" && j.createdAt >= openedAt - 1000).sort((a, b) => b.createdAt - a.createdAt)[0];
  const synced = sync ? sync.state === "done" : !!account.lastSync;
  const failed = sync?.state === "failed" || sync?.state === "cancelled";
  const pct = synced ? 100 : sync?.total ? Math.min(100, Math.round((sync.done / sync.total) * 100)) : 0;
  const premium = account.capabilities.serverVersion === "premium";
  const caps: [string, boolean][] = [[t("setup.ytm.cap.search"), true], [t("setup.ytm.cap.lyrics"), true], [t("setup.ytm.cap.library"), signedIn], [t("setup.ytm.cap.premium"), premium]];
  const different = () => { leaving = account.id; bridge().dispatch({ type: "removeAccount", data: { account_id: account.id } }); };
  return (
    <section className="setup-card setup-summary" aria-labelledby="setup-found" data-testid="setup-summary">
      <h1 id="setup-found" className="setup-version">{t("setup.ytm.title")}</h1>
      <p className="setup-who" data-testid="setup-ytm-who">{signedIn ? t("setup.ytm.who", { name: account.username || account.name }) : t("setup.ytm.anon")}</p>
      <ul className="plain-list setup-caps" aria-label={t("setup.capabilities")}>
        {caps.map(([label, ok]) => <li key={label} className={`setup-cap ${ok ? "ok" : ""}`}><Icon name={ok ? "check" : "close"} size={18} />{label}<span className="sr-only">, {ok ? t("setup.cap.yes") : t("setup.cap.no")}</span></li>)}
      </ul>
      {!signedIn ? null : failed ? <div className="setup-old" role="alert" data-testid="setup-sync-failed">{t("setup.syncFailed")}</div> : (
        <>
          <div className="setup-sync" role="status"><span>{synced ? t("setup.synced") : t("setup.syncing")}</span><span>{pct}%</span></div>
          <div className="setup-meter" role="progressbar" aria-label={t("setup.syncing")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}><span style={{ width: `${pct}%` }} /></div>
        </>
      )}
      <div className="setup-buttons">
        <button type="button" className="btn tonal" onClick={different} data-testid="setup-different">{t("setup.different")}</button>
        <button type="button" className="btn primary" onClick={() => setPhase("another")} autoFocus data-testid="setup-continue">{t("setup.continue")}</button>
      </div>
    </section>
  );
}

function NavidromeSummary({ server }: { server: AccountInfo }) {
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
