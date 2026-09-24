// Pure event reducer: mirrors the pieces of the snapshot the UI renders.
// No business logic — the core owns the truth; this is view state.
import type {
  AudioSettings, ConnectionState, DeviceInfo, Event, Filter, Job, Lyrics, MediaSessionState, OutputDevice, Pin, Problem, QueueEntry, QueueView, ResumeOffer, SavedQueue, ServerInfo, SessionDocument, Setting, Shortcut, SleepTimer, Snapshot, StorageSummary, SyncProgress, Toast, TransportState, UndoState, NetworkState, SearchResults,
} from "@core/api";

export interface CoreState {
  ready: boolean;
  servers: ServerInfo[];
  session: SessionDocument | undefined;
  queue: QueueView;
  transport: TransportState;
  nowPlaying: QueueEntry | undefined;
  connection: ConnectionState;
  devices: DeviceInfo[];
  handoff: { open: boolean; targets: DeviceInfo[] };
  jobs: Job[];
  problems: Problem[];
  undo: UndoState;
  settings: Record<string, Setting>;
  audio: AudioSettings;
  outputDevices: OutputDevice[];
  mediaSession: MediaSessionState;
  resumeOffer: ResumeOffer | undefined;
  sleepTimer: SleepTimer | undefined;
  network: NetworkState | undefined;
  batterySaver: boolean;
  syncProgress: SyncProgress | undefined;
  savedQueues: SavedQueue[];
  pins: Pin[];
  storage: StorageSummary | undefined;
  filters: Filter[];
  shortcuts: Shortcut[];
  /** Lyrics for the current track (or the last one that was current). */
  lyrics: { trackId: string; lyrics: Lyrics | undefined } | undefined;
  playerNotice: string | undefined;
  toasts: Toast[];
  /** Bumped on LibraryChanged so views refetch. */
  libraryVersion: number;
  libraryChangedIds: string[];
  /** Bumped on ActionsChanged. */
  actionsVersion: number;
  lastServerSearch: SearchResults | undefined;
  lastError: { kind: string; message: string; detail?: string; at: number } | undefined;
  /** Undo entry ids already announced with a toast (the core only emits UndoChanged for a fresh mutation). */
  announcedUndo: string[];
  exported: { kind: "nsp" | "config"; document: string; path?: string; at: number } | undefined;
}

export const EMPTY_QUEUE: QueueView = { contextLabel: undefined, history: [], current: undefined, playingNext: [], upcoming: [], shuffle: false, repeat: "off", autoplay: false, mode: "apple", totalUpcoming: 0 };

export const EMPTY_TRANSPORT: TransportState = { lease: { owner: undefined, epoch: 0, expiresAt: 0 }, position: { positionMs: 0, takenAt: 0, rate: 1, isPlaying: false }, buffering: false, playedMs: 0, volume: 1 };

export const initialCoreState: CoreState = {
  ready: false,
  servers: [],
  session: undefined,
  queue: EMPTY_QUEUE,
  transport: EMPTY_TRANSPORT,
  nowPlaying: undefined,
  connection: { tier: "local", connected: false, coordinatorUrl: undefined, clockOffsetMs: 0, roundTripMs: undefined, peerCount: 0, error: undefined },
  devices: [],
  handoff: { open: false, targets: [] },
  jobs: [],
  problems: [],
  undo: { canUndo: false, undoLabel: undefined, canRedo: false, redoLabel: undefined, history: [] },
  settings: {},
  audio: { replayGain: "off", replayGainPreampDb: 0, normalisation: false, eq: { enabled: false, preampDb: 0, bands: [], preset: undefined }, gapless: true, outputDevice: undefined, exclusive: false },
  outputDevices: [],
  mediaSession: { metadata: undefined, isPlaying: false, position: { positionMs: 0, takenAt: 0, rate: 1, isPlaying: false }, shuffle: false, repeat: "off", volume: 1, actions: [], ownsTransport: false },
  resumeOffer: undefined,
  sleepTimer: undefined,
  network: undefined,
  batterySaver: false,
  syncProgress: undefined,
  savedQueues: [],
  pins: [],
  storage: undefined,
  filters: [],
  shortcuts: [],
  lyrics: undefined,
  playerNotice: undefined,
  toasts: [],
  libraryVersion: 0,
  libraryChangedIds: [],
  actionsVersion: 0,
  lastServerSearch: undefined,
  lastError: undefined,
  exported: undefined,
  announcedUndo: [],
};


export function applySnapshot(state: CoreState, s: Snapshot): CoreState {
  return {
    ...state,
    ready: true,
    servers: s.servers,
    session: s.session,
    queue: s.queue,
    transport: s.transport,
    nowPlaying: s.queue.current,
    connection: s.connection,
    devices: s.devices,
    jobs: s.jobs,
    problems: s.problems,
    undo: s.undo,
    settings: Object.fromEntries(s.settings.map((x) => [x.key, x])),
    audio: s.audio,
    mediaSession: s.mediaSession,
    resumeOffer: s.resumeOffer,
    sleepTimer: s.sleepTimer,
    network: s.network,
    batterySaver: s.batterySaver,
    syncProgress: s.syncProgress,
    savedQueues: s.session?.savedQueues ?? state.savedQueues,
  };
}

export function reduce(state: CoreState, e: Event): CoreState {
  switch (e.type) {
    case "started":
      return applySnapshot(state, e.data.snapshot);
    case "serversChanged":
      return { ...state, servers: e.data.servers };
    case "syncProgress":
      return { ...state, syncProgress: e.data.progress, libraryVersion: e.data.progress.finished ? state.libraryVersion + 1 : state.libraryVersion };
    case "libraryChanged":
      return { ...state, libraryVersion: state.libraryVersion + 1, libraryChangedIds: e.data.ids };
    case "searchResults":
      return { ...state, lastServerSearch: e.data.results };
    case "sessionChanged":
      return { ...state, session: e.data.document, savedQueues: e.data.document.savedQueues };
    case "queueChanged":
      return { ...state, queue: e.data.queue, nowPlaying: e.data.queue.current ?? state.nowPlaying };
    case "transportChanged":
      return { ...state, transport: e.data.transport };
    case "nowPlayingChanged":
      return { ...state, nowPlaying: e.data.entry };
    case "savedQueuesChanged":
      return { ...state, savedQueues: e.data.queues };
    case "undoChanged": {
      // design.md "Global undo": a fresh mutation gets one toast with the single
      // Undo action. The core emits only UndoChanged for it (its own toasts are
      // "Undid …/Redid …"), so announce the newest entry here, once per id.
      const top = e.data.state.history[0];
      const fresh = top && e.data.state.canUndo && !state.announcedUndo.includes(top.id) && e.data.state.history.length > state.undo.history.length;
      const toasts = fresh ? [...state.toasts, { id: `undo-${top.id}`, message: top.label, actionLabel: "Undo", actionCommand: JSON.stringify({ type: "undo" }), durationMs: 5000 }].slice(-4) : state.toasts;
      const announced = fresh ? [...state.announcedUndo, top.id].slice(-500) : state.announcedUndo;
      return { ...state, undo: e.data.state, toasts, announcedUndo: announced };
    }
    case "toast":
      return { ...state, toasts: [...state.toasts.filter((t) => t.id !== e.data.toast.id), e.data.toast].slice(-4) };
    case "playerNotice":
      return { ...state, playerNotice: e.data.message };
    case "jobsChanged":
      return { ...state, jobs: e.data.jobs };
    case "problemsChanged":
      return { ...state, problems: e.data.problems };
    case "connectionChanged":
      return { ...state, connection: e.data.state };
    case "devicesChanged":
      return { ...state, devices: e.data.devices };
    case "handoffPickerChanged":
      return { ...state, handoff: { open: e.data.open, targets: e.data.targets } };
    case "resumeOfferChanged":
      return { ...state, resumeOffer: e.data.offer };
    case "lyricsChanged":
      return { ...state, lyrics: { trackId: e.data.track_id, lyrics: e.data.lyrics } };
    case "pinsChanged":
      return { ...state, pins: e.data.pins };
    case "storageChanged":
      return { ...state, storage: e.data.storage };
    case "filtersChanged":
      return { ...state, filters: e.data.filters };
    case "settingChanged":
      return { ...state, settings: { ...state.settings, [e.data.setting.key]: e.data.setting } };
    case "audioSettingsChanged":
      return { ...state, audio: e.data.settings };
    case "outputDevicesChanged":
      return { ...state, outputDevices: e.data.devices };
    case "sleepTimerChanged":
      return { ...state, sleepTimer: e.data.timer };
    case "shortcutsChanged":
      return { ...state, shortcuts: e.data.shortcuts };
    case "actionsChanged":
      return { ...state, actionsVersion: state.actionsVersion + 1 };
    case "nspExported":
      return { ...state, exported: { kind: "nsp", document: e.data.document, path: e.data.path, at: Date.now() } };
    case "configExported":
      return { ...state, exported: { kind: "config", document: e.data.document, at: Date.now() } };
    case "backend":
      return state;
    case "mediaSession":
      return { ...state, mediaSession: e.data.state };
    case "error":
      return { ...state, lastError: { kind: e.data.kind, message: e.data.message, detail: e.data.detail, at: Date.now() } };
    case "log":
      return state;
    default: {
      const never: never = e;
      void never;
      return state;
    }
  }
}

export function dismissToast(state: CoreState, id: string): CoreState {
  return { ...state, toasts: state.toasts.filter((t) => t.id !== id) };
}

/** Parse a JSON-encoded setting value with a typed default. */
export function settingValue<T>(state: Pick<CoreState, "settings">, key: string, fallback: T): T {
  const s = state.settings[key];
  if (!s) return fallback;
  try {
    const v = JSON.parse(s.value) as unknown;
    return v === null || v === undefined ? fallback : (v as T);
  } catch {
    return fallback;
  }
}
