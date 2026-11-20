// Device-local, opt-in telemetry for the Ctrl/Cmd+K palette (Settings, Diagnostics). Off by
// default. Nothing leaves the machine: events sit in localStorage (the last MAX_EVENTS) and the
// settings page summarises them, copies them as JSON or clears them. It answers how well the
// palette's ranking works: how often the pick isn't the first row, searches that find nothing or
// fail, palettes closed without a pick, and picks undone (or navigated back from) at once.
import { useSyncExternalStore } from "react";
import { loadLocal, saveLocal } from "./local-settings";

const ENABLED_KEY = "telemetry.palette";
const EVENTS_KEY = "telemetry.palette.events";
export const MAX_EVENTS = 2000;
/** An undo (or Back) this soon after a pick counts as taking the pick back. */
export const UNDO_WINDOW_MS = 30_000;
/** A local search slower than this is logged: the palette is meant to answer at once. */
export const SLOW_SEARCH_MS = 300;

/** How a pick was made: Enter, a click on the row, the footer's button, a menu chord, the actions menu. */
export type PickVia = "enter" | "click" | "footer" | "chord" | "menu";
export type UndoSource = "undo" | "undoEntry" | "uiUndo" | "back";

export interface PickEvent {
  type: "pick";
  at: number;
  query: string;
  label: string;
  kind: string;
  /** Flat 0-based row index: 0 is the row Enter runs as the palette opens on the query. */
  position: number;
  group: string;
  /** 0-based index inside its group. */
  groupPosition: number;
  /** What sat at row 0, when the pick wasn't it. */
  first?: { label: string; kind: string };
  /** The action that ran: "open", "run", "play", or a menu action's key. */
  action: string;
  /** For a command row, its action id. */
  command?: string;
  via: PickVia;
  rows: number;
  arrowPresses: number;
  msOpen: number;
  /** Queries (trimmed) this opening that found nothing. */
  emptyQueries: string[];
  /** The query had no answer yet when the pick ran: the row was from what the palette showed before. */
  resultsPending: boolean;
}
export interface DismissEvent {
  type: "dismiss";
  at: number;
  query: string;
  rows: number;
  arrowPresses: number;
  msOpen: number;
  emptyQueries: string[];
}
export interface ErrorEvent {
  type: "error";
  at: number;
  where: "search" | "actions" | "run" | "slowSearch";
  message: string;
  query?: string;
  ms?: number;
}
export interface UndoEvent {
  type: "undo";
  at: number;
  source: UndoSource;
  msSincePick: number;
  pick: Pick<PickEvent, "query" | "label" | "kind" | "position" | "action" | "via">;
}
export type PaletteEvent = PickEvent | DismissEvent | ErrorEvent | UndoEvent;

let enabled = loadLocal<unknown>(ENABLED_KEY, false) === true;
let lastPick: PickEvent | undefined;
const listeners = new Set<() => void>();
const notify = () => { for (const l of listeners) l(); };

export function telemetryEnabled(): boolean {
  return enabled;
}

export function setTelemetryEnabled(on: boolean): void {
  enabled = on;
  lastPick = undefined;
  saveLocal(ENABLED_KEY, on);
  notify();
}

/** Read from storage once, then kept in memory. */
let events: PaletteEvent[] | undefined;
export function telemetryEvents(): readonly PaletteEvent[] {
  if (!events) {
    const raw = loadLocal<unknown>(EVENTS_KEY, []);
    events = Array.isArray(raw) ? (raw as PaletteEvent[]) : [];
  }
  return events;
}

export function clearTelemetry(): void {
  lastPick = undefined;
  events = [];
  saveLocal(EVENTS_KEY, events);
  notify();
}

/** Appends (when enabled), keeping the newest MAX_EVENTS. */
export function recordPaletteEvent(event: PaletteEvent): void {
  if (!enabled) return;
  if (event.type === "pick") lastPick = event;
  events = [...telemetryEvents(), event].slice(-MAX_EVENTS);
  saveLocal(EVENTS_KEY, events);
  notify();
}

/** An undo or Back from anywhere: logged only right after a palette pick, once per pick. */
export function noteUndo(source: UndoSource, now = Date.now()): void {
  const pick = lastPick;
  if (!enabled || !pick) return;
  const ms = now - pick.at;
  if (ms < 0 || ms > UNDO_WINDOW_MS) return;
  // Back undoes a pick only when the pick navigated somewhere.
  if (source === "back" && pick.action !== "open") return;
  // Running Undo from the palette is the pick itself, not taking one back.
  if (pick.command === "undo") return;
  lastPick = undefined;
  const { query, label, kind, position, action, via } = pick;
  recordPaletteEvent({ type: "undo", at: now, source, msSincePick: ms, pick: { query, label, kind, position, action, via } });
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};
export function useTelemetryEnabled(): boolean {
  return useSyncExternalStore(subscribe, telemetryEnabled);
}
let version = 0;
listeners.add(() => { version++; });
/** Re-renders on every new event, clear or toggle. */
export function useTelemetryVersion(): number {
  return useSyncExternalStore(subscribe, () => version);
}

export interface TelemetrySummary {
  picks: number;
  firstRow: number;
  notFirst: number;
  /** Mean 0-based position of the picks. */
  meanPosition: number;
  byPosition: Record<string, number>;
  dismissed: number;
  emptySearches: number;
  pendingPicks: number;
  errors: number;
  slowSearches: number;
  undos: number;
}

export function summarise(events: readonly PaletteEvent[]): TelemetrySummary {
  const picks = events.filter((e): e is PickEvent => e.type === "pick");
  const byPosition: Record<string, number> = {};
  for (const p of picks) {
    const k = p.position >= 5 ? "5+" : String(p.position);
    byPosition[k] = (byPosition[k] ?? 0) + 1;
  }
  const errs = events.filter((e): e is ErrorEvent => e.type === "error");
  const firstRow = picks.filter((p) => p.position === 0).length;
  return {
    picks: picks.length,
    firstRow,
    notFirst: picks.length - firstRow,
    meanPosition: picks.length ? picks.reduce((n, p) => n + p.position, 0) / picks.length : 0,
    byPosition,
    dismissed: events.filter((e) => e.type === "dismiss").length,
    emptySearches: events.reduce((n, e) => n + (e.type === "pick" || e.type === "dismiss" ? e.emptyQueries.length : 0), 0),
    pendingPicks: picks.filter((p) => p.resultsPending).length,
    errors: errs.filter((e) => e.where !== "slowSearch").length,
    slowSearches: errs.filter((e) => e.where === "slowSearch").length,
    undos: events.filter((e) => e.type === "undo").length,
  };
}
