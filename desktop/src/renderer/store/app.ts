// The renderer store: core state mirrored from events (reducer.ts) plus view
// state. Components read slices with `useApp(selector)` and act through
// `dispatch`/`query` which go straight to the bridge.
import { create } from "zustand";
import type { ActionDescriptor, ActionTarget, Command, Event, Query, Setting } from "@core/api";
import type { AppMeta, WindowState } from "@shared/bridge-types";
import { expectResult, type ResultData } from "@shared/core-handle";
import { bridge } from "../core/bridge";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { type CoreState, dismissToast, initialCoreState, reduce, settingValue } from "./reducer";
import { EMPTY_SELECTION, type Selection } from "./selection";
import { ticker } from "./position";

export type ViewName = "home" | "albums" | "artists" | "playlists" | "songs" | "genres" | "downloads" | "filters" | "stats" | "settings" | "album" | "artist" | "playlist" | "genre" | "filter" | "search";

export interface Route {
  view: ViewName;
  id?: string;
  /** Settings section, search query, etc. */
  param?: string;
}

export interface PanelState {
  rightOpen: boolean;
  queueCollapsed: boolean;
  lyricsCollapsed: boolean;
  /** Fraction of the right panel height given to the queue (0.15–0.85). */
  splitRatio: number;
  rightWidth: number;
  sidebarWidth: number;
}

export interface ContextMenuState {
  x: number;
  y: number;
  target: ActionTarget;
  actions: ActionDescriptor[];
  /** Extra renderer-only context (e.g. playlist id for "remove from playlist"). */
  context?: { playlistId?: string; indices?: number[]; savedQueueId?: string };
}

export type DialogState =
  | { kind: "prompt"; title: string; label: string; initial?: string; confirmLabel?: string; onConfirm: (value: string) => void }
  | { kind: "confirm"; title: string; message: string; confirmLabel?: string; destructive?: boolean; onConfirm: () => void }
  | { kind: "addToPlaylist"; trackIds: string[] }
  | { kind: "sleepTimer" }
  | { kind: "connect" }
  | { kind: "trackInfo"; trackId: string };

export interface UiState {
  meta: AppMeta | undefined;
  windowState: WindowState;
  route: Route;
  history: Route[];
  future: Route[];
  panels: PanelState;
  fullscreen: boolean;
  paletteOpen: boolean;
  contextMenu: ContextMenuState | undefined;
  dialog: DialogState | undefined;
  selection: Selection;
  /** View id the selection belongs to; a route change clears it. */
  selectionScope: string;
  searchQuery: string;
  searchFocusNonce: number;
  accent: string | undefined;
  pageVisible: boolean;
  /** Renderer performance mode derived from visibility/focus/battery. */
  perf: "full" | "background" | "stopped";
}

export interface AppStore extends CoreState, UiState {
  dispatch(command: Command): void;
  query<Q extends Query>(query: Q): Promise<ResultData<Extract<Q, Query>["type"] extends never ? never : never>>;
  applyEvent(event: Event): void;
  dismissToast(id: string): void;
  navigate(route: Route, replace?: boolean): void;
  back(): void;
  forward(): void;
  setPanels(patch: Partial<PanelState>): void;
  setFullscreen(on: boolean): void;
  setPaletteOpen(open: boolean): void;
  openContextMenu(menu: ContextMenuState): void;
  closeContextMenu(): void;
  openDialog(dialog: DialogState): void;
  closeDialog(): void;
  setSelection(selection: Selection, scope: string): void;
  clearSelection(): void;
  setSearchQuery(q: string): void;
  focusSearch(): void;
  setAccent(accent: string | undefined): void;
  setMeta(meta: AppMeta): void;
  setWindowState(state: WindowState): void;
  setPageVisible(visible: boolean): void;
  setting<T>(key: string, fallback: T): T;
  setSetting(key: string, value: unknown): void;
  runAction(actionId: string, target?: ActionTarget): void;
}

const defaultPanels: PanelState = { rightOpen: true, queueCollapsed: false, lyricsCollapsed: false, splitRatio: 0.55, rightWidth: 340, sidebarWidth: 220 };

export const useApp = create<AppStore>((set, get) => ({
  ...initialCoreState,
  meta: undefined,
  windowState: { maximized: false, fullscreen: false, focused: true, visible: true, alwaysOnTop: false },
  route: { view: "home" },
  history: [],
  future: [],
  panels: { ...defaultPanels, ...loadLocal<Partial<PanelState>>("panels", {}) },
  fullscreen: false,
  paletteOpen: false,
  contextMenu: undefined,
  dialog: undefined,
  selection: EMPTY_SELECTION,
  selectionScope: "",
  searchQuery: "",
  searchFocusNonce: 0,
  accent: undefined,
  pageVisible: true,
  perf: "full",

  dispatch(command) {
    bridge().dispatch(command);
  },
  // Typed at the call sites via `queryAs`; kept loose here.
  query: (async (q: Query) => bridge().query(q)) as AppStore["query"],
  applyEvent(event) {
    set((s) => reduce(s, event) as Partial<AppStore>);
  },
  dismissToast(id) {
    set((s) => dismissToast(s, id) as Partial<AppStore>);
  },
  navigate(route, replace = false) {
    const cur = get().route;
    if (cur.view === route.view && cur.id === route.id && cur.param === route.param) return;
    set((s) => ({ route, history: replace ? s.history : [...s.history, s.route].slice(-50), future: [], selection: EMPTY_SELECTION, selectionScope: "", contextMenu: undefined }));
  },
  back() {
    const { history, route } = get();
    const prev = history[history.length - 1];
    if (!prev) return;
    set((s) => ({ route: prev, history: history.slice(0, -1), future: [route, ...s.future].slice(0, 50), selection: EMPTY_SELECTION }));
  },
  forward() {
    const { future, route } = get();
    const next = future[0];
    if (!next) return;
    set((s) => ({ route: next, future: future.slice(1), history: [...s.history, route], selection: EMPTY_SELECTION }));
  },
  setPanels(patch) {
    const panels = { ...get().panels, ...patch };
    saveLocal("panels", panels);
    set({ panels });
  },
  setFullscreen(on) {
    set({ fullscreen: on, contextMenu: undefined });
  },
  setPaletteOpen(open) {
    set({ paletteOpen: open, contextMenu: undefined });
  },
  openContextMenu(menu) {
    set({ contextMenu: menu });
  },
  closeContextMenu() {
    if (get().contextMenu) set({ contextMenu: undefined });
  },
  openDialog(dialog) {
    set({ dialog, contextMenu: undefined });
  },
  closeDialog() {
    set({ dialog: undefined });
  },
  setSelection(selection, scope) {
    set({ selection, selectionScope: scope });
  },
  clearSelection() {
    set({ selection: EMPTY_SELECTION });
  },
  setSearchQuery(q) {
    set({ searchQuery: q });
  },
  focusSearch() {
    set((s) => ({ searchFocusNonce: s.searchFocusNonce + 1 }));
  },
  setAccent(accent) {
    set({ accent });
  },
  setMeta(meta) {
    set({ meta });
  },
  setWindowState(windowState) {
    set({ windowState });
    recomputePerf();
  },
  setPageVisible(pageVisible) {
    set({ pageVisible });
    recomputePerf();
  },
  setting<T>(key: string, fallback: T): T {
    return settingValue(get(), key, fallback);
  },
  setSetting(key, value) {
    // Optimistic mirror so controls don't flicker; the core confirms with SettingChanged.
    const prev = get().settings[key];
    const setting: Setting = { key, value: JSON.stringify(value), scope: prev?.scope ?? "deviceLocal", updatedAt: Date.now() };
    set((s) => ({ settings: { ...s.settings, [key]: setting } }));
    bridge().dispatch({ type: "setSetting", data: { key, value: JSON.stringify(value) } });
  },
  runAction(actionId, target = { type: "none" }) {
    bridge().dispatch({ type: "runAction", data: { action_id: actionId, target } });
  },
}));

/** Typed query helper: `await queryAs({ type: "album", data: { id } }, "albumDetail")`. */
export async function queryAs<K extends Query["type"], R extends ResultDataKeys>(query: Extract<Query, { type: K }>, result: R): Promise<ResultData<R>> {
  return expectResult(await bridge().query(query), result);
}
type ResultDataKeys = Parameters<typeof expectResult>[1];

function recomputePerf(): void {
  const s = useApp.getState();
  const visible = s.windowState.visible && s.pageVisible;
  const perf: UiState["perf"] = !visible ? "stopped" : s.windowState.focused ? "full" : "background";
  if (perf !== s.perf) useApp.setState({ perf });
  // Position ticker: stopped when hidden; 24 fps in the background; capped by
  // battery saver (30 fps) and the lyrics fps setting when focused.
  ticker.setPaused(perf === "stopped");
  const cap = s.batterySaver ? 30 : Math.max(10, Math.min(144, settingValue(s, "lyrics.fpsCap", 60)));
  ticker.fps = perf === "background" ? Math.min(24, cap) : cap;
}

/** Wire the bridge once at startup. Returns a disposer. */
export function connectStore(): () => void {
  const b = bridge();
  const offEvent = b.onEvent((e) => {
    useApp.getState().applyEvent(e);
    if (e.type === "started" || (e.type === "settingChanged" && e.data.setting.key === "lyrics.fpsCap") || e.type === "toast") recomputePerf();
    if (e.type === "started") recomputePerf();
  });
  const offState = b.window.onState((st) => useApp.getState().setWindowState(st));
  void b.meta().then((m) => useApp.getState().setMeta(m));
  void b.window.getState().then((st) => st && useApp.getState().setWindowState(st));
  const onVis = () => {
    const visible = document.visibilityState === "visible";
    useApp.getState().setPageVisible(visible);
    b.window.reportVisibility(visible);
  };
  document.addEventListener("visibilitychange", onVis);
  const onOnline = () => b.window.reportNetwork(navigator.onLine);
  window.addEventListener("online", onOnline);
  window.addEventListener("offline", onOnline);
  // Attach: the core re-emits everything with current state.
  b.dispatch({ type: "requestSnapshot" });
  return () => {
    offEvent();
    offState();
    document.removeEventListener("visibilitychange", onVis);
    window.removeEventListener("online", onOnline);
    window.removeEventListener("offline", onOnline);
  };
}

export function useSetting<T>(key: string, fallback: T): T {
  return useApp((s) => settingValue(s, key, fallback));
}
