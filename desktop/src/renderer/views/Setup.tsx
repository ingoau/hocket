// Server setup: the entire first screen, nothing else.
import { useEffect, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { CredentialWarning } from "../components/CredentialWarning";
import { Icon } from "../components/Icon";

export function Setup() {
  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState<string | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const lastError = useApp((s) => s.lastError);
  const [submittedAt, setSubmittedAt] = useState(0);

  useEffect(() => {
    if (lastError && lastError.at >= submittedAt && busy) {
      setBusy(false);
      setError(t("setup.failed", { message: lastError.detail && lastError.detail !== lastError.message ? `${lastError.message}: ${lastError.detail}` : lastError.message }));
    }
  }, [lastError, submittedAt, busy]);


  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const u = url.trim().replace(/\/+$/, "");
    if (!/^https?:\/\/.+/.test(u)) return setError(t("setup.errorUrl"));
    if (!username.trim() || !password) return setError(t("setup.errorCredentials"));
    setError(undefined);
    setBusy(true);
    setSubmittedAt(Date.now());
    bridge().dispatch({ type: "addServer", data: { url: u, username: username.trim(), password, name: name.trim() || undefined } });
  };
  return (
    <main className="setup" data-testid="setup">
      <form onSubmit={submit} aria-labelledby="setup-title" aria-describedby="setup-subtitle">
        <div className="setup-hero">
          <div className="setup-shapes" aria-hidden="true">
            <span className="shape cookie" /><span className="shape pill" /><span className="shape circle" /><span className="shape square" />
          </div>
          <div className="brand" aria-hidden="true">{t("app.name")}</div>
          <h1 id="setup-title" className="setup-title">{t("setup.title")}</h1>
          <div className="muted" id="setup-subtitle">{t("setup.subtitle")}</div>
        </div>
        <div className="setup-card">
          <CredentialWarning />
          <label>{t("setup.url")}<input className="input" type="url" aria-required="true" aria-invalid={error === t("setup.errorUrl") || undefined} aria-describedby={error ? "setup-error" : undefined} placeholder={t("setup.urlPlaceholder")} value={url} onChange={(e) => setUrl(e.target.value)} autoFocus data-testid="setup-url" /></label>
          <label>{t("setup.username")}<input className="input" aria-required="true" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" data-testid="setup-username" /></label>
          <label>{t("setup.password")}<input className="input" type="password" aria-required="true" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" data-testid="setup-password" /></label>
          <label>{t("setup.name")}<input className="input" autoComplete="off" value={name} onChange={(e) => setName(e.target.value)} /></label>
          {error ? <div className="error" id="setup-error" role="alert" data-testid="setup-error"><Icon name="warn" size={18} filled />{error}</div> : null}
          <div className="setup-actions">
            <button type="submit" className="btn primary connect" disabled={busy} data-testid="setup-connect">{busy ? <Icon name="spinner" size={20} className="spin" /> : null}{busy ? t("setup.connecting") : t("setup.connect")}</button>
            <button type="button" className="btn tonal" onClick={() => void bridge().config.import().catch((err: unknown) => console.error("config import failed", err))} data-testid="setup-import"><Icon name="backup" size={18} />{t("setup.import")}</button>
          </div>
        </div>
      </form>
    </main>
  );
}
