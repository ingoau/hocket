// The search field at the top of Settings and its results. Typing replaces the
// nav's sections with the settings that match (each naming its section);
// picking one opens that section, scrolls to the setting, highlights it for a
// moment and puts focus on its control. Escape clears the field; Mod+F
// (findInList) focuses it while Settings is open. The index is data, in
// lib/settings-index.
import { useEffect, useRef, useState } from "react";
import { t } from "@shared/strings";
import { searchSettings, sectionLabel, type SettingsHit, type SettingsSection } from "../lib/settings-index";
import { Icon } from "./Icon";

const FLASH_MS = 1800;

export function SettingsSearch({ query, onQuery, onOpen }: { query: string; onQuery: (q: string) => void; onOpen: (section: SettingsSection, settingId?: string) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const hits = searchSettings(query);
  const searching = query.trim().length > 0;
  const pick = (h: SettingsHit) => {
    onQuery("");
    onOpen(h.section, h.kind === "setting" ? h.id : undefined);
  };
  const results = () => [...(list.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
  const onListKey = (e: React.KeyboardEvent) => {
    const all = results();
    const i = all.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") { e.preventDefault(); all[Math.min(all.length - 1, i + 1)]?.focus(); }
    else if (e.key === "ArrowUp") { e.preventDefault(); if (i <= 0) input.current?.focus(); else all[i - 1]?.focus(); }
    else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onQuery(""); input.current?.focus(); }
  };
  return (
    <div className="settings-search" role="search" aria-label={t("settingsSearch.placeholder")}>
      <label className="settings-search-field">
        <Icon name="search" size={20} />
        <input
          ref={input} type="search" className="settings-search-input" value={query} placeholder={t("settingsSearch.placeholder")} aria-label={t("settingsSearch.placeholder")}
          aria-controls={searching ? "settings-search-results" : undefined} autoComplete="off" spellCheck={false}
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) { e.preventDefault(); e.stopPropagation(); onQuery(""); }
            else if (e.key === "ArrowDown" && hits.length) { e.preventDefault(); results()[0]?.focus(); }
            else if (e.key === "Enter" && hits[0]) { e.preventDefault(); pick(hits[0]); }
          }}
          data-testid="settings-search"
        />
        {query ? <button type="button" className="btn icon sm" aria-label={t("settingsSearch.clear")} title={t("settingsSearch.clear")} onClick={() => { onQuery(""); input.current?.focus(); }} data-testid="settings-search-clear"><Icon name="close" size={18} /></button> : null}
      </label>
      <p className="sr-only" role="status" aria-live="polite">{searching ? (hits.length ? t(hits.length === 1 ? "settingsSearch.count.one" : "settingsSearch.count.other", { count: hits.length }) : t("settingsSearch.none", { query: query.trim() })) : ""}</p>
      {searching ? (
        hits.length ? (
          <ul ref={list} className="plain-list settings-search-results" id="settings-search-results" aria-label={t("settingsSearch.results")} onKeyDown={onListKey} data-testid="settings-search-results">
            {hits.map((h) => (
              <li key={h.kind === "setting" ? h.id : `section-${h.section}`}>
                <button type="button" className="settings-search-hit" onClick={() => pick(h)} data-testid={h.kind === "setting" ? `settings-hit-${h.id}` : `settings-hit-section-${h.section}`}>
                  <span className="settings-search-hit-label">{h.label}</span>
                  <span className="settings-search-hit-section">{h.kind === "setting" ? sectionLabel(h.section) : t("settingsSearch.section")}</span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <div className="settings-search-empty" data-testid="settings-search-empty">
            <p>{t("settingsSearch.none", { query: query.trim() })}</p>
            <p className="muted small">{t("settingsSearch.noneHint")}</p>
          </div>
        )
      ) : null}
    </div>
  );
}

/**
 * Once [section] is showing, brings the picked setting (`setting-<id>`) into view, highlights it
 * briefly and moves focus to its control (or the row itself when it has none); with no setting,
 * focus goes to the section's heading. Returns the function to call with the picked setting's id.
 */
export function useSettingsJump(section: SettingsSection): (settingId?: string) => void {
  const [pending, setPending] = useState<{ id?: string; n: number } | null>(null);
  useEffect(() => {
    if (!pending) return;
    const frame = requestAnimationFrame(() => {
      setPending(null);
      if (!pending.id) { document.getElementById("settings-heading")?.focus({ preventScroll: true }); return; }
      const row = document.getElementById(`setting-${pending.id}`);
      if (!row) return;
      const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      row.scrollIntoView({ block: "center", behavior: reduce ? "auto" : "smooth" });
      row.classList.remove("setting-flash");
      void row.offsetWidth;
      row.classList.add("setting-flash");
      window.setTimeout(() => row.classList.remove("setting-flash"), FLASH_MS);
      const control = row.querySelector<HTMLElement>("input:not([type=hidden]), select, textarea, button, [role=switch], [role=slider], [tabindex]:not([tabindex='-1'])");
      if (control) control.focus({ preventScroll: true });
      else { if (!row.hasAttribute("tabindex")) row.tabIndex = -1; row.focus({ preventScroll: true }); }
    });
    return () => cancelAnimationFrame(frame);
  }, [pending, section]);
  return (settingId) => setPending((p) => ({ id: settingId, n: (p?.n ?? 0) + 1 }));
}
