// The renderer store: core state mirrored from events (reducer.ts) plus view
// state. Components read slices with `useApp(selector)` and act through
// `dispatch`/`query` which go straight to the bridge.
import { useMemo } from "react";
import { create } from "zustand";
import type { ActionTarget, Command, Event, MenuItem as LayoutItem, MenuScreen, Query, Setting, TrackRef } from "@core/api";
import type { AppMeta, AppPrefs, CredentialStorage, WindowState } from "@shared/bridge-types";
import { expectResult, snapshotOf, type ResultData } from "@shared/core-handle";
import { bridge } from "../core/bridge";
import { forgetArtwork } from "../components/Artwork";
import { loadLocal, saveLocal } from "../lib/local-settings";
import { noteUndo } from "../lib/palette-telemetry";
import { type CoreState, dismissToast, initialCoreState, reduce, saverHas, settingValue } from "./reducer";
import { EMPTY_SELECTION, type Selection } from "./selection";
import { ticker } from "./position";
import { BACKGROUND_FPS, perfForWindow } from "../lib/kawarp-budget";
import { SK } from "@shared/settings-keys";
import type { MenuFacts } from "../lib/menu-model";
import { LEAVE_OFFLINE_MODE_GRACE_MS, leavingOfflineMode, offlineReason, ownsPlayback, type OfflineReason } from "../lib/offline";

export type ViewName = "home" | "library" | "albums" | "artists" | "playlists" | "songs" | "genres" | "downloads" | "filters" | "stats" | "settings" | "developer" | "album" | "artist" | "playlist" | "genre" | "filter" | "search" | "snoozed";

export interface Route {
  view: ViewName;
  id?: string;
  /** Settings section, search query, etc. */
  param?: string;
}

/** What the fullscreen player's stage shows: the big artwork, or one of its panes (the artwork shrinks next to the title). */
export type SelectionKind = "tracks" | "albums" | "artists" | "playlists" | "queueItems";
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
  /** The sidebar folded to a rail of icons and covers (the top bar's first button, Mod+B). */
  sidebarCollapsed: boolean;
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
  /** The core's rows for the target (Query.Menu), in display order. */
  layout: LayoutItem[];
  /** Renderer-run items shown above the core's rows (or alone). */
  items?: MenuItem[];
  /** Extra renderer-only context (e.g. playlist id for "remove from playlist", where it was opened). */
  context?: { playlistId?: string; indices?: number[]; savedQueueId?: string; screen?: MenuScreen };
  /** Opened with the context-menu key / Shift+F10: the first item starts active. */
  keyboard?: boolean;
  /** The header, the checkboxes' and stars' state and the songs to add (lib/menu-facts). */
  facts?: MenuFacts;
}

export type DialogState =
  | { kind: "prompt"; title: string; label: string; initial?: string; confirmLabel?: string; onConfirm: (value: string) => void }
  | { kind: "confirm"; title: string; message: string; confirmLabel?: string; destructive?: boolean; onConfirm: () => void }
  | { kind: "sleepTimer" }
  /** Snooze songs until a chosen date and time (the Snooze submenu's Custom row). */
  | { kind: "snoozeUntil"; tracks: TrackRef[] }
  | { kind: "connect" }
  | { kind: "trackInfo"; track: TrackRef }
  /** Name a new playlist, and with several accounts pick where it lives (or Mixed). */
  | { kind: "newPlaylist"; tracks: TrackRef[]; initial?: string }
  | { kind: "albumInfo"; albumId: string }
  /** Save to Coordinator; `allDevices` as the diagnostics page's bundle options. */
  | { kind: "report"; allDevices: boolean }
  | { kind: "pairDevice" }
  /** Remove a device from this identity (with the password-change prompt). */
  | { kind: "removeDevice"; deviceId: string; name: string; self: boolean };

/** The Add to playlist picker opened on its own (a "+" button, the shortcut), beside what opened it. */
export interface PlaylistPickerState {
  tracks: TrackRef[];
  /** The button (or focused item) it opens from, in window coordinates. */
  anchor: { left: number; right: number; top: number; bottom: number };
}

export interface UiState {
  meta: AppMeta | undefined;
  credentialStorage: CredentialStorage | undefined;
  windowState: WindowState;
  route: Route;
  history: Route[];
  future: Route[];
  panels: PanelState;
  fullscreen: boolean;
  paletteOpen: boolean;
  /** A row is being dragged out of the open palette: it steps aside (see-through, the window live) until the drag ends. */
  paletteHidden: boolean;
  /** The stash popover in the top bar. */
  stashOpen: boolean;
  contextMenu: ContextMenuState | undefined;
  playlistPicker: PlaylistPickerState | undefined;
  dialog: DialogState | undefined;
  /** A pairing link (hocket://pair) opened from outside, for the setup screen to join with. */
  pendingPair: string | undefined;
  selection: Selection;
  /** View id the selection belongs to; a route change clears it. */
  selectionScope: string;
  /** What the selected ids are (track ids, album ids, queue keys…), for shortcuts that act on it. */
  selectionKind: SelectionKind | undefined;
  /** A song selection's references (the list knows each row's account), when it is one. */
  selectionRefs: TrackRef[] | undefined;
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
  /**
   * Offline mode was just turned off and the core's NetworkChanged hasn't come yet (or the grace
   * ran out): an offline network is offline mode's, not "no connection" (`lib/offline`).
   */
  leavingOfflineMode: boolean;
  /** The source filter chip: the account the library, search and playlists show ("" = all, `hocket` = Mixed playlists). */
  librarySource: string;
  setLibrarySource(source: string): void;
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
  setPaletteHidden(hidden: boolean): void;
  setStashOpen(open: boolean): void;
  openContextMenu(menu: ContextMenuState): void;
  closeContextMenu(): void;
  openPlaylistPicker(picker: PlaylistPickerState): void;
  closePlaylistPicker(): void;
  openDialog(dialog: DialogState): void;
  closeDialog(): void;
  setPendingPair(link: string | undefined): void;
  setSelection(selection: Selection, scope: string, kind?: SelectionKind, refs?: TrackRef[]): void;
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

/** The sidebar's resize range, and the rail's width when it is collapsed (or the window narrow). */
export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 420;
export const SIDEBAR_RAIL = 72;

const defaultPanels: PanelState = { rightOpen: true, rightView: "overview", rightWidth: 360, sidebarWidth: 280, sidebarCollapsed: false };

/** The remembered panels, minus anything an older build stored (the queue/lyrics split). */
function loadPanels(): PanelState {
  const saved = loadLocal<Partial<PanelState> & Record<string, unknown>>("panels", {});
  const panels: PanelState = { ...defaultPanels };
  if (typeof saved.rightOpen === "boolean") panels.rightOpen = saved.rightOpen;
  // Saved widths are clamped to what the resize handles allow (an older build, a hand-edited file).
  if (typeof saved.rightWidth === "number") panels.rightWidth = Math.max(300, Math.min(560, saved.rightWidth));
  if (typeof saved.sidebarWidth === "number") panels.sidebarWidth = Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_MAX, saved.sidebarWidth));
  if (typeof saved.sidebarCollapsed === "boolean") panels.sidebarCollapsed = saved.sidebarCollapsed;
  if (SIDE_VIEWS.includes(saved.rightView as SideView)) panels.rightView = saved.rightView as SideView;
  return panels;
}

export const useApp = create<AppStore>((set, get) => ({
  ...initialCoreState,
  meta: undefined,
  credentialStorage: undefined,
  windowState: { maximized: false, fullscreen: false, focused: true, visible: true, alwaysOnTop: false },
  route: { view: "home" },
  history: [],
  future: [],
  panels: loadPanels(),
  fullscreen: false,
  paletteOpen: false,
  paletteHidden: false,
  stashOpen: false,
  contextMenu: undefined,
  playlistPicker: undefined,
  dialog: undefined,
  pendingPair: undefined,
  selection: EMPTY_SELECTION,
  selectionScope: "",
  selectionKind: undefined,
  selectionRefs: undefined,
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
  leavingOfflineMode: false,
  librarySource: "",

  dispatch(command) {
    if (command.type === "undo" || command.type === "undoEntry") noteUndo(command.type);
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
    // Going where you already are still takes the palette (and any menu) away.
    if (cur.view === route.view && cur.id === route.id && cur.param === route.param) {
      if (get().paletteOpen) set({ paletteOpen: false, contextMenu: undefined, playlistPicker: undefined });
      return;
    }
    set((s) => ({ route, history: replace ? s.history : [...s.history, s.route].slice(-50), future: [], selection: EMPTY_SELECTION, selectionScope: "", selectionKind: undefined, selectionRefs: undefined, contextMenu: undefined, playlistPicker: undefined, paletteOpen: false }));
  },
  back() {
    const { history, route } = get();
    const prev = history[history.length - 1];
    if (!prev) return;
    noteUndo("back");
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
    // Hidden for a drag stays hidden through the exit (no flash back before it leaves); the next opening shows it.
    set({ paletteOpen: open, contextMenu: undefined, playlistPicker: undefined, ...(open ? { paletteHidden: false } : null) });
  },
  setPaletteHidden(hidden) {
    if (get().paletteOpen || !hidden) set({ paletteHidden: hidden });
  },
  setStashOpen(open) {
    set({ stashOpen: open });
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
  setPendingPair(pendingPair) {
    set({ pendingPair });
  },
  setSelection(selection, scope, kind, refs) {
    set({ selection, selectionScope: scope, selectionKind: kind, selectionRefs: refs });
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
  setLibrarySource(librarySource) {
    set({ librarySource });
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
    const wasOfflineMode = offlineModeOn(get());
    const setting: Setting = { key, value: JSON.stringify(value), scope: prev?.scope ?? "deviceLocal", updatedAt: Date.now() };
    set((s) => ({ settings: { ...s.settings, [key]: setting } }));
    noteOfflineMode(wasOfflineMode);
    bridge().dispatch({ type: "setSetting", data: { key, value: JSON.stringify(value) } });
  },
  runAction(actionId, target = { type: "none" }) {
    if (actionId === "undo") noteUndo("undo");
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
  // battery saver's lighter lyrics (battery.lyricsFps) and the lyrics fps setting when focused.
  ticker.setPaused(perf === "stopped");
  const cap = saverHas(s.batterySaver, "lighterLyrics") ? Math.max(10, settingValue(s, SK.batteryLyricsFps, 30)) : Math.max(10, Math.min(144, settingValue(s, SK.displayLyricsFps, 30)));
  ticker.fps = perf === "background" ? Math.min(BACKGROUND_FPS, cap) : cap;
}

/** Wire the bridge once at startup. Returns a disposer. */
export function connectStore(kind: "main" | "mini" = "main"): () => void {
  const b = bridge();
  // The core emits LyricsChanged on every NowPlayingChanged; the snapshot on
  // (re)attach carries no lyrics, so ask once for whatever is already playing.
  const requestLyrics = (trackId: string | undefined) => {
    if (!trackId || kind === "mini") return;
    void b.query({ type: "lyrics", data: { track_id: trackId } }).then((r) => {
      if (r.type === "lyricsResult" && r.data) useApp.getState().applyEvent({ type: "lyricsChanged", data: { track_id: trackId, lyrics: r.data } });
    }).catch(() => undefined);
  };
  // Stash events sent before this window subscribed are gone: ask once.
  if (kind === "main") {
    void b.query({ type: "stash" }).then((r) => {
      if (r.type === "stashItems") useApp.getState().applyEvent({ type: "stashChanged", data: { items: r.data } });
    }).catch(() => undefined);
    void b.query({ type: "snoozes" }).then((r) => {
      if (r.type === "snoozeItems") useApp.getState().applyEvent({ type: "snoozesChanged", data: { items: r.data } });
    }).catch(() => undefined);
  }
  // A report upload already running (resumed as the core started) has sent its events by now: ask.
  let reportEventSeen = false;
  const offEvent = b.onEvent((e) => {
    if (e.type === "reportUploadChanged") reportEventSeen = true;
    if (e.type === "networkChanged" || e.type === "accountsChanged") forgetArtwork();
    const wasOfflineMode = offlineModeOn(useApp.getState());
    useApp.getState().applyEvent(e);
    if (e.type === "networkChanged") endLeavingOfflineMode();
    else noteOfflineMode(wasOfflineMode);
    const snapshot = snapshotOf(e);
    if (snapshot) requestLyrics(snapshot.queue.current?.track.id);
    if (e.type === "settingChanged" && e.data.setting.key.endsWith("lyricsFps")) recomputePerf();
    if (snapshot || e.type === "batterySaverChanged") recomputePerf();
  });
  const offState = b.window.onState((st) => useApp.getState().setWindowState(st));
  void b.meta().then((m) => useApp.getState().setMeta(m));
  void b.credentialStorage().then((credentialStorage) => useApp.setState({ credentialStorage }));
  void b.prefs.get().then((prefs) => useApp.setState({ prefs }));
  void b.window.getState().then((st) => st && useApp.getState().setWindowState(st));
  const onVis = () => {
    const visible = document.visibilityState === "visible";
    useApp.getState().setPageVisible(visible);
    b.window.reportVisibility(visible);
  };
  document.addEventListener("visibilitychange", onVis);
  // A window that starts hidden (in the tray) never sees a change to report.
  onVis();
  // The browser's view is the input main reports to the core (SetNetworkState);
  // the core's NetworkChanged event is what the UI shows.
  const onOnline = () => b.window.reportNetwork(navigator.onLine);
  window.addEventListener("online", onOnline);
  window.addEventListener("offline", onOnline);
  if (kind === "main") {
    void b.query({ type: "reportUpload" }).then((r) => {
      if (r.type === "reportUpload" && !reportEventSeen) useApp.getState().applyEvent({ type: "reportUploadChanged", data: { status: r.data } });
    }).catch(() => undefined);
  }
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

function offlineModeOn(s: CoreState): boolean {
  return settingValue<boolean>(s, SK.networkOfflineMode, false) === true;
}

let leavingTimer: ReturnType<typeof setTimeout> | undefined;

function endLeavingOfflineMode(): void {
  if (leavingTimer) clearTimeout(leavingTimer);
  leavingTimer = undefined;
  if (useApp.getState().leavingOfflineMode) useApp.setState({ leavingOfflineMode: false });
}

/**
 * After a settings change (ours, optimistic, or the core's SettingChanged): offline mode going off
 * holds "no connection" back until the core's NetworkChanged, or the grace runs out.
 */
function noteOfflineMode(was: boolean): void {
  const now = offlineModeOn(useApp.getState());
  if (was === now) return;
  if (now) {
    endLeavingOfflineMode();
    return;
  }
  if (leavingTimer) clearTimeout(leavingTimer);
  leavingTimer = setTimeout(endLeavingOfflineMode, LEAVE_OFFLINE_MODE_GRACE_MS);
  useApp.setState({ leavingOfflineMode: true });
}

/** Offline mode, or no connection (`lib/offline`); undefined while online. */
export function selectOfflineReason(s: AppStore): OfflineReason | undefined {
  const browserOnline = typeof navigator === "undefined" || navigator.onLine !== false;
  return offlineReason(s.network, offlineModeOn(s), leavingOfflineMode(s.leavingOfflineMode, browserOnline));
}

export function useOfflineReason(): OfflineReason | undefined {
  return useApp(selectOfflineReason);
}

/** This device's id, as the transport lease names it. */
export function selfDeviceId(s: CoreState): string | undefined {
  return s.identity.deviceId || s.devices.find((d) => d.isSelf)?.id;
}

/**
 * Rows to dim as not playable offline: offline, and this device owns playback (controlling another
 * device over LAN Connect, the core doesn't skip for this one's lack of network).
 */
export function useOfflineDimming(): boolean {
  return useApp((s) => !!selectOfflineReason(s) && ownsPlayback(s.transport.lease.owner, selfDeviceId(s)));
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
