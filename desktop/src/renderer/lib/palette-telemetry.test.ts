import { beforeEach, describe, expect, it } from "vitest";
import { MAX_EVENTS, UNDO_WINDOW_MS, clearTelemetry, noteUndo, recordPaletteEvent, setTelemetryEnabled, summarise, telemetryEnabled, telemetryEvents, type PickEvent } from "./palette-telemetry";

const pick = (over: Partial<PickEvent> = {}): PickEvent => ({
  type: "pick", at: 1_000, query: "abc", label: "Song", kind: "track", position: 0, group: "tracks", groupPosition: 0,
  action: "play", via: "enter", rows: 5, arrowPresses: 0, msOpen: 900, emptyQueries: [], resultsPending: false, ...over,
});

describe("palette telemetry", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    (globalThis as { localStorage?: unknown }).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
    setTelemetryEnabled(false);
    clearTelemetry();
  });

  it("is off by default and records nothing while off", () => {
    expect(telemetryEnabled()).toBe(false);
    recordPaletteEvent(pick());
    noteUndo("undo", 1_100);
    expect(telemetryEvents()).toEqual([]);
  });

  it("records once on, and keeps only the newest events", () => {
    setTelemetryEnabled(true);
    for (let i = 0; i < MAX_EVENTS + 3; i++) recordPaletteEvent(pick({ at: i }));
    const events = telemetryEvents();
    expect(events).toHaveLength(MAX_EVENTS);
    expect(events[0]?.at).toBe(3);
  });

  it("logs an undo right after a pick, once", () => {
    setTelemetryEnabled(true);
    recordPaletteEvent(pick({ position: 2 }));
    noteUndo("undo", 1_500);
    noteUndo("undo", 1_600);
    const undos = telemetryEvents().filter((e) => e.type === "undo");
    expect(undos).toEqual([{ type: "undo", at: 1_500, source: "undo", msSincePick: 500, pick: { query: "abc", label: "Song", kind: "track", position: 2, action: "play", via: "enter" } }]);
  });

  it("ignores an undo long after the pick, and Back after a pick that didn't navigate", () => {
    setTelemetryEnabled(true);
    recordPaletteEvent(pick());
    noteUndo("back", 1_100);
    noteUndo("undo", 1_000 + UNDO_WINDOW_MS + 1);
    expect(telemetryEvents().filter((e) => e.type === "undo")).toEqual([]);
    recordPaletteEvent(pick({ action: "open", kind: "album" }));
    noteUndo("back", 1_200);
    expect(telemetryEvents().filter((e) => e.type === "undo")).toHaveLength(1);
  });

  it("summarises picks, positions, dismissals and issues", () => {
    const sum = summarise([
      pick(),
      pick({ position: 1, emptyQueries: ["zz"] }),
      pick({ position: 7, resultsPending: true }),
      { type: "dismiss", at: 0, query: "q", rows: 0, arrowPresses: 0, msOpen: 10, emptyQueries: ["q"] },
      { type: "error", at: 0, where: "search", message: "boom" },
      { type: "error", at: 0, where: "slowSearch", message: "400 ms", ms: 400 },
      { type: "undo", at: 0, source: "undo", msSincePick: 300, pick: { query: "", label: "", kind: "track", position: 0, action: "play", via: "enter" } },
    ]);
    expect(sum).toEqual({
      picks: 3, firstRow: 1, notFirst: 2, meanPosition: 8 / 3, byPosition: { "0": 1, "1": 1, "5+": 1 },
      dismissed: 1, emptySearches: 2, pendingPicks: 1, errors: 1, slowSearches: 1, undos: 1,
    });
  });
});
