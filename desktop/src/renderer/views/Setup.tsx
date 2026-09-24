// Server setup: the entire first screen, nothing else.
import { useEffect, useState } from "react";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";

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
    <div className="setup" data-testid="setup">
      <form onSubmit={submit}>
        <div className="brand">{t("app.name")}</div>
        <h2>{t("setup.title")}</h2>
        <div className="muted small">{t("setup.subtitle")}</div>
        <label>{t("setup.url")}<input className="input" type="url" placeholder={t("setup.urlPlaceholder")} value={url} onChange={(e) => setUrl(e.target.value)} autoFocus data-testid="setup-url" /></label>
        <label>{t("setup.username")}<input className="input" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="username" data-testid="setup-username" /></label>
        <label>{t("setup.password")}<input className="input" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" data-testid="setup-password" /></label>
        <label>{t("setup.name")}<input className="input" value={name} onChange={(e) => setName(e.target.value)} /></label>
        {error ? <div className="error" role="alert" data-testid="setup-error">{error}</div> : null}
        <button type="submit" className="btn primary" disabled={busy} style={{ height: 34 }} data-testid="setup-connect">{busy ? t("setup.connecting") : t("setup.connect")}</button>
      </form>
    </div>
  );
}
