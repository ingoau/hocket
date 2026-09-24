// Top bar: back/forward, window controls (Windows/Linux), search (local-first
// with server results appended below a divider, container never shrinking),
// and the job/problems indicator with its popover.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Job, SearchResults } from "@core/api";
import { t } from "@shared/strings";
import { useApp } from "../store/app";
import { bridge } from "../core/bridge";
import { Artwork } from "./Artwork";
import { Icon } from "./Icon";
import { fmtRelative } from "../lib/format";

export function TopBar() {
  const platform = useApp((s) => s.meta?.platform ?? "linux");
  const back = useApp((s) => s.back);
  const forward = useApp((s) => s.forward);
  const canBack = useApp((s) => s.history.length > 0);
  const canForward = useApp((s) => s.future.length > 0);
  const win = useApp((s) => s.windowState);
  return (
    <div className={`topbar ${platform === "macOs" ? "mac" : ""}`} data-testid="topbar">
      <div className="nav-buttons">
        <button type="button" className="btn icon" aria-label={t("action.back")} disabled={!canBack} onClick={back}><Icon name="chevronLeft" /></button>
        <button type="button" className="btn icon" aria-label={t("action.forward")} disabled={!canForward} onClick={forward}><Icon name="chevronRight" /></button>
      </div>
      <div className="grow" />
      <SearchBox />
      <JobsIndicator />
      {platform !== "macOs" ? (
        <div className="window-controls">
          <button type="button" className="btn icon" aria-label={t("misc.minimize")} onClick={() => bridge().window.minimize()}><Icon name="minimize" size={14} /></button>
          <button type="button" className="btn icon" aria-label={win.maximized ? t("misc.restore") : t("misc.maximize")} onClick={() => (win.maximized ? bridge().window.unmaximize() : bridge().window.maximize())}><Icon name={win.maximized ? "restoreWin" : "maximize"} size={13} /></button>
          <button type="button" className="btn icon close" aria-label={t("misc.close")} onClick={() => bridge().window.close()}><Icon name="close" size={14} /></button>
        </div>
      ) : null}
    </div>
  );
}

function SearchBox() {
  const serverId = useApp((s) => s.servers[0]?.id);
  const focusNonce = useApp((s) => s.searchFocusNonce);
  const navigate = useApp((s) => s.navigate);
  const serverBatch = useApp((s) => s.lastServerSearch);
  const [q, setQ] = useState("");
  const [open, setOpen] = useState(false);
  const [local, setLocal] = useState<SearchResults | undefined>(undefined);
  const [server, setServer] = useState<SearchResults | undefined>(undefined);
  const [minHeight, setMinHeight] = useState(0);
  const [active, setActive] = useState(-1);
  const input = useRef<HTMLInputElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const reqId = useRef("");

  useEffect(() => {
    if (focusNonce) {
      input.current?.focus();
      input.current?.select();
    }
  }, [focusNonce]);

  useEffect(() => {
    if (!serverId || !q.trim()) {
      setLocal(undefined);
      setServer(undefined);
      setMinHeight(0);
      return;
    }
    const id = `s-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    reqId.current = id;
    const h = setTimeout(() => {
      void bridge().query({ type: "search", data: { server_id: serverId, query: q, limit: 6, include_server: true, request_id: id } }).then((r) => {
        if (reqId.current !== id || r.type !== "search") return;
        setLocal(r.data);
        setServer(undefined);
      });
    }, 80);
    return () => clearTimeout(h);
  }, [q, serverId]);

  useEffect(() => {
    if (serverBatch && serverBatch.requestId === reqId.current && serverBatch.fromServer) setServer(serverBatch);
  }, [serverBatch]);

  // The container never shrinks between the local and server batches.
  useLayoutEffect(() => {
    if (pop.current && open) setMinHeight((h) => Math.max(h, pop.current?.offsetHeight ?? 0));
  }, [local, server, open]);
  useEffect(() => {
    if (!q.trim()) setMinHeight(0);
  }, [q]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!pop.current?.contains(e.target as Node) && e.target !== input.current) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  type Row = { key: string; kind: string; label: string; sub?: string; cover?: string; go: () => void };
  const rows = (r: SearchResults | undefined): Row[] => {
    if (!r) return [];
    const sid = serverId ?? "";
    return [
      ...r.tracks.map((x) => ({ key: `t${x.id}`, kind: t("search.tracks"), label: x.title, sub: x.artist, cover: x.coverArt, go: () => bridge().dispatch({ type: "playTracks", data: { server_id: sid, track_ids: [x.id], start_index: 0, label: x.title, shuffle: false } }) })),
      ...r.albums.map((x) => ({ key: `a${x.id}`, kind: t("search.albums"), label: x.name, sub: x.artist, cover: x.coverArt, go: () => navigate({ view: "album", id: x.id }) })),
      ...r.artists.map((x) => ({ key: `r${x.id}`, kind: t("search.artists"), label: x.name, cover: x.coverArt, go: () => navigate({ view: "artist", id: x.id }) })),
      ...r.playlists.map((x) => ({ key: `p${x.id}`, kind: t("search.playlists"), label: x.name, cover: x.coverArt, go: () => navigate({ view: "playlist", id: x.id }) })),
    ];
  };
  const localRows = rows(local);
  const serverRows = rows(server);
  const all = [...localRows, ...serverRows];
  const pick = (r: Row) => {
    r.go();
    setOpen(false);
    input.current?.blur();
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { setOpen(false); input.current?.blur(); return; }
    if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => Math.min(all.length - 1, a + 1)); }
    if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => Math.max(-1, a - 1)); }
    if (e.key === "Enter") {
      e.preventDefault();
      const r = all[active];
      if (r) pick(r);
      else if (q.trim()) { navigate({ view: "search", param: q }); setOpen(false); }
    }
  };
  const renderRow = (r: Row, i: number) => (
    <div key={r.key} id={`search-opt-${i}`} className={`result-row ${i === active ? "active" : ""}`} role="option" aria-selected={i === active} onMouseMove={(e) => { if (e.movementX || e.movementY) setActive(i); }} onMouseDown={(e) => { e.preventDefault(); pick(r); }} data-testid="search-result">
      <Artwork id={r.cover} size={64} className="art" />
      <div className="grow truncate">{r.label}{r.sub ? <span className="muted"> · {r.sub}</span> : null}</div>
      <span className="kind">{r.kind}</span>
    </div>
  );
  const expanded = open && !!q.trim();
  return (
    <div className="search" role="search">
      <Icon name="search" size={14} />
      <input ref={input} className="input" type="search" placeholder={t("search.placeholder")} value={q} onChange={(e) => { setQ(e.target.value); setOpen(true); setActive(-1); }} onFocus={() => q && setOpen(true)} onKeyDown={onKey} aria-label={t("search.placeholder")} role="combobox" aria-autocomplete="list" aria-expanded={expanded} aria-controls={expanded ? "search-results" : undefined} aria-activedescendant={expanded && active >= 0 ? `search-opt-${active}` : undefined} data-testid="search-input" />
      {expanded ? (
        <div ref={pop} id="search-results" className="search-popover fade-in" role="listbox" aria-label={t("a11y.searchResults")} style={{ minHeight }} data-testid="search-popover">
          {local && !localRows.length && !serverRows.length ? <div className="search-divider" role="presentation">{t("search.noResults", { query: q })}</div> : null}
          {localRows.length ? <div className="search-section" role="group" aria-label={t("nav.library")}>{localRows.map(renderRow)}</div> : null}
          {local ? <div className="search-divider" role="presentation" data-testid="search-divider">{server ? t("search.server") : t("search.searching")}</div> : null}
          {serverRows.length ? <div className="search-section" role="group" aria-label={t("search.server")} data-testid="search-server-batch">{serverRows.map((r, i) => renderRow(r, localRows.length + i))}</div> : null}
        </div>
      ) : null}
    </div>
  );
}

function JobsIndicator() {
  const jobs = useApp((s) => s.jobs);
  const problems = useApp((s) => s.problems);
  const sync = useApp((s) => s.syncProgress);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const running = jobs.filter((j) => j.state === "running" || j.state === "queued" || j.state === "paused");
  const failed = jobs.some((j) => j.state === "failed") || problems.length > 0;
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setOpen(false); };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);
  const cmd = (type: "cancelJob" | "retryJob" | "pauseJob" | "resumeJob", id: string) => bridge().dispatch({ type, data: { id } } as never);
  const label = problems.length ? t("jobs.problems", { count: problems.length }) : running.length ? t("jobs.running", { count: running.length }) : t("jobs.idle");
  return (
    <div ref={ref} style={{ position: "relative" }} onKeyDown={(e) => { if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); setOpen(false); button.current?.focus(); } }}>
      <button ref={button} type="button" className="btn icon jobs-btn" aria-label={`${t("jobs.title")}: ${label}`} aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen((o) => !o)} data-testid="jobs-button">
        <Icon name={running.length ? "spinner" : failed ? "warn" : "cloud"} className={running.length ? "spin" : ""} />
        {running.length || failed ? <span className={`dot ${failed ? "problem" : ""}`} /> : null}
      </button>
      {open ? (
        <div className="popover fade-in" role="dialog" aria-label={t("jobs.title")} data-testid="jobs-popover">
          <div className="section-title">{t("jobs.jobsHeading")}</div>
          {!jobs.length ? <div className="item muted">{t("jobs.idle")}</div> : null}
          {sync && !sync.finished ? <div className="item muted small">{t("sync.syncing", { phase: sync.phase })}</div> : null}
          {jobs.slice(0, 20).map((j: Job) => (
            <div key={j.id} className="item" data-testid="job-row">
              <div className="row">
                <span className="grow truncate">{j.label}</span>
                <span className={`badge ${j.state === "failed" ? "warn" : j.state === "done" ? "ok" : ""}`}>{t(`job.state.${j.state}` as never)}</span>
              </div>
              <div className={`progress ${j.state === "failed" ? "failed" : ""}`}><div style={{ width: j.total ? `${Math.min(100, (j.done / j.total) * 100)}%` : j.state === "done" ? "100%" : "30%" }} /></div>
              <div className="row small muted">
                <span className="grow">{j.total ? t("jobs.progress", { done: j.done, total: j.total }) : t("jobs.progressUnknown", { done: j.done })}{j.failed ? ` · ${t("jobs.failedCount", { failed: j.failed })}` : ""}</span>
                {j.state === "running" && j.cancellable ? <button type="button" className="btn sm" onClick={() => cmd("pauseJob", j.id)}>{t("jobs.pause")}</button> : null}
                {j.state === "paused" ? <button type="button" className="btn sm" onClick={() => cmd("resumeJob", j.id)}>{t("jobs.resume")}</button> : null}
                {(j.state === "running" || j.state === "queued" || j.state === "paused") && j.cancellable ? <button type="button" className="btn sm" onClick={() => cmd("cancelJob", j.id)}>{t("jobs.cancel")}</button> : null}
                {j.state === "failed" || j.state === "cancelled" ? <button type="button" className="btn sm" onClick={() => cmd("retryJob", j.id)}>{t("jobs.retry")}</button> : null}
              </div>
            </div>
          ))}
          {problems.length ? (
            <>
              <div className="section-title row"><span className="grow">{t("jobs.problemsHeading")}</span><button type="button" className="btn sm ghost" onClick={() => bridge().dispatch({ type: "dismissAllProblems" })}>{t("jobs.dismissAll")}</button></div>
              {problems.map((p) => (
                <div key={p.id} className="item" data-testid="problem-row">
                  <div className="row"><Icon name="warn" size={14} style={{ color: "var(--warn)" }} /><span className="grow">{p.summary}</span><span className="faint xs">{fmtRelative(p.createdAt)}</span></div>
                  {p.detail ? <div className="small muted">{p.detail}</div> : null}
                  <div className="row">
                    <span className="grow" />
                    {p.retryable ? <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "retryProblem", data: { id: p.id } })}>{t("jobs.retry")}</button> : null}
                    <button type="button" className="btn sm" onClick={() => bridge().dispatch({ type: "dismissProblem", data: { id: p.id } })}>{t("jobs.dismiss")}</button>
                  </div>
                </div>
              ))}
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
