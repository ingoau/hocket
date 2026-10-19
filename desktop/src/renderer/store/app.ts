// The renderer store: core state mirrored from events (reducer.ts) plus view
// state. Components read slices with `useApp(selector)` and act through
// `dispatch`/`query` which go straight to the bridge.
import { useMemo } from "react";
import { create } from "zustand";
import type { ActionDescriptor, ActionTarget, Command, Event, Query, Setting } from "@core/api";
import type { AppMeta, AppPrefs, WindowState } from "@shared/bridge-types";
import { expectResult, snapshotOf, type ResultData } from "@shared/core-handle";
import { bridge } from "../core/bridge";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { type CoreState, dismissToast, initialCoreState, reduce, settingValue } from "./reducer";
import { EMPTY_SELECTION, type Selection } from "./selection";
import { ticker } from "./position";
import { BACKGROUND_FPS, perfForWindow } from "../lib/kawarp-budget";
import { SK } from "@shared/settings-keys";
import type { MenuFacts } from "../lib/menu-model";

export type ViewName = "home" | "library" | "albums" | "artists" | "playlists" | "songs" | "genres" | "downloads" | "filters" | "stats" | "settings" | "album" | "artist" | "playlist" | "genre" | "filter" | "search";

export interface Route {
  view: ViewName;
  id?: string;
  /** Settings section, search query, etc. */
  param?: string;
}

/** What the fullscreen player's stage shows: the big artwork, or one of its panes (the artwork shrinks next to the title). */
export type NowPlayingMode = "art" | "lyrics" | "queue" | "about";
const NOW_PLAYING_MODES: readonly NowPlayingMode[] = ["art", "lyrics", "queue", "about"];

/** What the now playing side panel shows: the scrolling overview, or one of its drill-ins. */
export type SideView = "overview" | "lyrics" | "queue" | "about" | "recent";
const SIDE_VIEWS: readonly SideView[] = ["overview", "lyrics", "queue", "about", "recent"];

export interface PanelState {
  rightOpen: boolean;
  rightView: SideView;
  rightWidth: number;
  sidebarWidth: number;
}

/** A menu item the renderer runs itself (a pin's "When clicked" choice, moving a Home section). */
export interface MenuItem {
  id: string;
  label: string;
  icon: string;
  enabled: boolean;
  /** A choice among siblings (radio): shown with a check, announced as checked. */
  checked?: boolean;
  destructive?: boolean;
  /** A heading over a group of items; not selectable. */
  heading?: boolean;
  run: () => void;
}

export interface ContextMenuState {
  x: number;
  y: number;
  target: ActionTarget;
  actions: ActionDescriptor[];
  /** Renderer-run items shown above the registry's actions (or alone). */
  items?: MenuItem[];
  /** Extra renderer-only context (e.g. playlist id for "remove from playlist"). */
  context?: { playlistId?: string; indices?: number[]; savedQueueId?: string };
  /** Opened with the context-menu key / Shift+F10: the first item starts active. */
  keyboard?: boolean;
  /** The header, the checkboxes' and stars' state and the songs to add (lib/menu-facts). */
  facts?: MenuFacts;
}

export type DialogState =
  | { kind: "prompt"; title: string; label: string; initial?: string; confirmLabel?: string; onConfirm: (value: string) => void }
  | { kind: "confirm"; title: string; message: string; confirmLabel?: string; destructive?: boolean; onConfirm: () => void }
  | { kind: "sleepTimer" }
  | { kind: "connect" }
  | { kind: "trackInfo"; trackId: string }
  | { kind: "albumInfo"; albumId: string };

/** The Add to playlist picker opened on its own (a "+" button, the shortcut), beside what opened it. */
export interface PlaylistPickerState {
  trackIds: string[];
  /** The button (or focused item) it opens from, in window coordinates. */
  anchor: { left: number; right: number; top: number; bottom: number };
}

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
  playlistPicker: PlaylistPickerState | undefined;
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
  /** Remembered on this device across closing and reopening the player. */
  nowPlayingMode: NowPlayingMode;
  prefs: AppPrefs;
  /** Narrow windows (and 200% zoom) show the side panel as a drawer, closed until asked for. */
  drawerOpen: boolean;
  /** The Edit pins panel is open (beside the sidebar, over any page). */
  editingPins: boolean;
  /** The pin (kind:id) whose menu opened it, where focus goes back to. */
  editPinsFrom: string | undefined;
  /**
   * A library drag is over the player: the side panel (and the fullscreen
   * player) show the queue without writing the remembered view, and go back
   * when the drag ends.
   */
  queueDropReveal: boolean;
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
  openPlaylistPicker(picker: PlaylistPickerState): void;
  closePlaylistPicker(): void;
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
  setNowPlayingMode(mode: NowPlayingMode): void;
  setPrefs(patch: Partial<AppPrefs>): void;
  setDrawerOpen(open: boolean): void;
  setEditingPins(open: boolean, from?: string): void;
  setQueueDropReveal(on: boolean): void;
  setting<T>(key: string, fallback: T): T;
  setSetting(key: string, value: unknown): void;
  runAction(actionId: string, target?: ActionTarget): void;
}

const defaultPanels: PanelState = { rightOpen: true, rightView: "overview", rightWidth: 360, sidebarWidth: 220 };

/** The remembered panels, minus anything an older build stored (the queue/lyrics split). */
function loadPanels(): PanelState {
  const saved = loadLocal<Partial<PanelState> & Record<string, unknown>>("panels", {});
  const panels: PanelState = { ...defaultPanels };
  if (typeof saved.rightOpen === "boolean") panels.rightOpen = saved.rightOpen;
  // Saved widths are clamped to what the resize handles allow (an older build, a hand-edited file).
  if (typeof saved.rightWidth === "number") panels.rightWidth = Math.max(300, Math.min(560, saved.rightWidth));
  if (typeof saved.sidebarWidth === "number") panels.sidebarWidth = Math.max(160, Math.min(360, saved.sidebarWidth));
  if (SIDE_VIEWS.includes(saved.rightView as SideView)) panels.rightView = saved.rightView as SideView;
  return panels;
}

export const useApp = create<AppStore>((set, get) => ({
  ...initialCoreState,
  meta: undefined,
  windowState: { maximized: false, fullscreen: false, focused: true, visible: true, alwaysOnTop: false },
  route: { view: "home" },
  history: [],
  future: [],
  panels: loadPanels(),
  fullscreen: false,
  paletteOpen: false,
  contextMenu: undefined,
  playlistPicker: undefined,
  dialog: undefined,
  selection: EMPTY_SELECTION,
  selectionScope: "",
  searchQuery: "",
  searchFocusNonce: 0,
  accent: undefined,
  pageVisible: true,
  perf: "full",
  nowPlayingMode: loadNowPlayingMode(),
  prefs: { closeToTray: true },
  drawerOpen: false,
  editingPins: false,
  editPinsFrom: undefined,
  queueDropReveal: false,

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
    set((s) => ({ route, history: replace ? s.history : [...s.history, s.route].slice(-50), future: [], selection: EMPTY_SELECTION, selectionScope: "", contextMenu: undefined, playlistPicker: undefined }));
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
    set({ fullscreen: on, contextMenu: undefined, playlistPicker: undefined });
  },
  setPaletteOpen(open) {
    set({ paletteOpen: open, contextMenu: undefined, playlistPicker: undefined });
  },
  openContextMenu(menu) {
    set({ contextMenu: menu, playlistPicker: undefined });
  },
  closeContextMenu() {
    if (get().contextMenu) set({ contextMenu: undefined });
  },
  openPlaylistPicker(picker) {
    set({ playlistPicker: picker, contextMenu: undefined });
  },
  closePlaylistPicker() {
    if (get().playlistPicker) set({ playlistPicker: undefined });
  },
  openDialog(dialog) {
    set({ dialog, contextMenu: undefined, playlistPicker: undefined });
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
  setNowPlayingMode(nowPlayingMode) {
    saveLocal("nowPlayingMode", nowPlayingMode);
    set({ nowPlayingMode });
  },
  setPrefs(patch) {
    void bridge().prefs.set(patch).then((prefs) => set({ prefs }));
  },
  setDrawerOpen(drawerOpen) {
    set({ drawerOpen });
  },
  setEditingPins(editingPins, from) {
    set({ editingPins, editPinsFrom: editingPins ? from : undefined });
  },
  setQueueDropReveal(queueDropReveal) {
    if (get().queueDropReveal === queueDropReveal) return;
    set({ queueDropReveal });
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

/** The mode the player was last left in; the queue the first time. */
function loadNowPlayingMode(): NowPlayingMode {
  const v = loadLocal<string>("nowPlayingMode", "queue");
  return (NOW_PLAYING_MODES as readonly string[]).includes(v) ? (v as NowPlayingMode) : "queue";
}

/** Whether the window is in the narrow layout (lib/media.ts NARROW). */
export function isNarrow(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(max-width: 900px)").matches;
}

/** Whether the side panel is showing: the in-flow panel normally, the drawer when narrow. */
export function sidePanelOpen(): boolean {
  const app = useApp.getState();
  return isNarrow() ? app.drawerOpen : app.panels.rightOpen;
}

function setSidePanelOpen(open: boolean): void {
  const app = useApp.getState();
  if (isNarrow()) app.setDrawerOpen(open);
  else if (app.panels.rightOpen !== open) app.setPanels({ rightOpen: open });
}

/** Show or hide the side panel; showing it starts on the overview. */
export function toggleSidePanel(open?: boolean): void {
  const next = open ?? !sidePanelOpen();
  if (next) useApp.getState().setPanels({ rightView: "overview" });
  setSidePanelOpen(next);
}

/**
 * The player bar's lyrics and queue buttons (and L / Q): show the side panel straight on that
 * view, or close it when it is already showing it.
 */
export function toggleSideView(view: SideView): void {
  const app = useApp.getState();
  if (sidePanelOpen() && app.panels.rightView === view) {
    setSidePanelOpen(false);
    return;
  }
  app.setPanels({ rightView: view });
  setSidePanelOpen(true);
}

/** Show the side panel on `view` (a drill-in, or back to the overview). */
export function showSideView(view: SideView): void {
  useApp.getState().setPanels({ rightView: view });
  setSidePanelOpen(true);
}

/** Typed query helper: `await queryAs({ type: "album", data: { id } }, "albumDetail")`. */
export async function queryAs<K extends Query["type"], R extends ResultDataKeys>(query: Extract<Query, { type: K }>, result: R): Promise<ResultData<R>> {
  return expectResult(await bridge().query(query), result);
}
type ResultDataKeys = Parameters<typeof expectResult>[1];

function recomputePerf(): void {
  const s = useApp.getState();
  // Electron's shown/minimised is the stop signal. document.hidden (pageVisible)
  // also goes false when Chromium thinks the window is occluded, which an
  // unfocused fullscreen window often is — stopping then restarting the
  // background is the flicker in #45. Prefetch still honours page visibility
  // via SetVisibility in main.
  const perf = perfForWindow(s.windowState.visible, s.windowState.focused);
  if (perf !== s.perf) useApp.setState({ perf });
  // Position ticker: stopped when hidden; 24 fps in the background; capped by
  // battery saver (30 fps) and the lyrics fps setting when focused.
  ticker.setPaused(perf === "stopped");
  const cap = s.batterySaver ? Math.max(10, settingValue(s, SK.batteryLyricsFps, 30)) : Math.max(10, Math.min(144, settingValue(s, SK.displayLyricsFps, 30)));
  ticker.fps = perf === "background" ? Math.min(BACKGROUND_FPS, cap) : cap;
}

/** Wire the bridge once at startup. Returns a disposer. */
export function connectStore(): () => void {
  const b = bridge();
  // The core emits LyricsChanged on every NowPlayingChanged; the snapshot on
  // (re)attach carries no lyrics, so ask once for whatever is already playing.
  const requestLyrics = (trackId: string | undefined) => {
    if (!trackId) return;
    void b.query({ type: "lyrics", data: { track_id: trackId } }).then((r) => {
      if (r.type === "lyricsResult" && r.data) useApp.getState().applyEvent({ type: "lyricsChanged", data: { track_id: trackId, lyrics: r.data } });
    }).catch(() => undefined);
  };
  const offEvent = b.onEvent((e) => {
    useApp.getState().applyEvent(e);
    const snapshot = snapshotOf(e);
    if (snapshot) requestLyrics(snapshot.queue.current?.track.id);
    if (e.type === "settingChanged" && e.data.setting.key.endsWith("lyricsFps")) recomputePerf();
    if (snapshot || e.type === "batterySaverChanged") recomputePerf();
  });
  const offState = b.window.onState((st) => useApp.getState().setWindowState(st));
  void b.meta().then((m) => useApp.getState().setMeta(m));
  void b.prefs.get().then((prefs) => useApp.setState({ prefs }));
  void b.window.getState().then((st) => st && useApp.getState().setWindowState(st));
  const onVis = () => {
    const visible = document.visibilityState === "visible";
    useApp.getState().setPageVisible(visible);
    b.window.reportVisibility(visible);
  };
  document.addEventListener("visibilitychange", onVis);
  // The browser's view is the input main reports to the core (SetNetworkState);
  // the core's NetworkChanged event is what the UI shows.
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

/**
 * A list or object setting's parsed value. `useSetting` parses on every read, a new value each time,
 * which only primitives survive; this selects the stored text and parses it once per change.
 */
export function useJsonSetting(key: string): unknown {
  const raw = useApp((s) => s.settings[key]?.value);
  return useMemo(() => {
    if (raw === undefined) return undefined;
    try {
      return JSON.parse(raw) as unknown;
    } catch {
      return undefined;
    }
  }, [raw]);
}
