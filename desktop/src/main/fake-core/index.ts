// FakeCore: a TypeScript stand-in for crates/hocket-node that honours the
// api.rs contract closely enough to build and test the whole UI against.
// Selected with HOCKET_FAKE_CORE=1, and used automatically when the native
// addon is missing (the renderer shows a dev banner in that case).
//
// It is deliberately a simulation, not a reimplementation: queue semantics
// follow docs/design.md "Queue model" (nothing consumed, history above,
// insertions, shuffle permutation), undo is snapshot-based, jobs progress on
// timers, and there is one fake remote device ("Pixel 8") so Connect UI has
// something to show.
import { mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type {
  ActionTarget, AudioSettings, RatingTarget, AutoplaySettings, Command, ConfigDocument, ConnectionState, CoreConfig, DeviceInfo, Event, Filter, FilterNode, FilterRule, Job, Lyrics, MediaSessionAction, MediaSessionState, OutputDevice, Pin, PlayHistoryEntry, Problem, Query, QueryResult, QueueContext, QueueMode, QueueEntry, QueueItem, QueueView, RelatedTrack, RepeatMode, ResumeOffer, SavedQueue, SearchResults, ServerInfo, SessionDocument, Setting, Shortcut, SleepTimer, Snapshot, SortOrder, StorageSummary, Toast, Track, TrackSummary, TransportState, UndoEntry, UndoState,
} from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import { DEFAULT_KEYMAP, canonicalActionId } from "@shared/keymap";
import { coverPng } from "../png";
import { describeActions, shortcutDefaults, DEFAULT_ORDERS } from "./actions";
import { generateLibrary, summary, coverSeed, type FakeLibrary } from "./library";
import { lyricsFor } from "./lyrics";
import { Rng, hash32 } from "./random";

const ARTWORK_SIZES = [64, 300, 1000];
const SEEK_STEP_MS = 10_000;
const HISTORY_CAP = 200;
const UPCOMING_VIEW_CAP = 400;

interface SessionState {
  context?: QueueContext;
  /** Permuted order: indices into context.tracks. */
  order: number[];
  cursor: number;
  current?: QueueItem;
  history: QueueItem[];
  insertions: QueueItem[];
  shuffle?: { seed: number; anchor?: number };
  repeat: RepeatMode;
  autoplay: boolean;
  mode: QueueMode;
  revision: number;
}

interface UndoRecord {
  id: string;
  label: string;
  at: number;
  undo: () => string | undefined;
  redo: () => void;
  selection?: ActionTarget;
}

interface FakeOptions {
  /** Speed multiplier for timers (e2e uses >1 to make jobs finish faster). */
  timeScale?: number;
  seed?: number;
  trackCount?: number;
}

function clone<T>(v: T): T {
  return structuredClone(v);
}

let keyCounter = 0;
function newKey(): string {
  keyCounter += 1;
  return `qk-${Date.now().toString(36)}-${keyCounter}`;
}
let idCounter = 0;
function newId(prefix: string): string {
  idCounter += 1;
  return `${prefix}-${Date.now().toString(36)}${idCounter}`;
}

function permutation(n: number, seed: number, anchor: number | undefined): number[] {
  const rng = new Rng(seed);
  const order = Array.from({ length: n }, (_, i) => i);
  rng.shuffle(order);
  if (anchor !== undefined && anchor < n) {
    const at = order.indexOf(anchor);
    if (at > 0) {
      order.splice(at, 1);
      order.unshift(anchor);
    }
  }
  return order;
}

export class FakeCore implements CoreHandle {
  readonly kind = "fake" as const;
  private listeners = new Set<(e: Event) => void>();
  private readonly config: CoreConfig;
  private readonly opts: Required<FakeOptions>;
  private lib: FakeLibrary | undefined;
  private servers: ServerInfo[] = [];
  private session: SessionState = { order: [], cursor: 0, history: [], insertions: [], repeat: "off", autoplay: true, mode: "apple", revision: 1 };
  private transport: TransportState = { lease: { owner: undefined, epoch: 0, expiresAt: 0 }, position: { positionMs: 0, takenAt: Date.now(), rate: 1, isPlaying: false }, buffering: false, playedMs: 0, volume: 0.8 };
  private savedQueues: SavedQueue[] = [];
  private savedQueueCap = 10;
  private undoStack: UndoRecord[] = [];
  private redoStack: UndoRecord[] = [];
  private jobs: Job[] = [];
  private problems: Problem[] = [];
  private devices: DeviceInfo[] = [];
  private connection: ConnectionState = { tier: "local", connected: false, coordinatorUrl: undefined, clockOffsetMs: 0, roundTripMs: undefined, peerCount: 0, error: undefined };
  private settings = new Map<string, Setting>();
  private audio: AudioSettings;
  private outputDevices: OutputDevice[] = [
    { id: "default", name: "System default", isDefault: true },
    { id: "hdmi", name: "HDMI / DisplayPort", isDefault: false },
    { id: "usb-dac", name: "USB Audio DAC", isDefault: false },
  ];
  private shortcutOverrides = new Map<string, string | undefined>();
  private actionOrders = new Map<string, string[]>();
  private pins: Pin[] = [];
  private filters: Filter[] = [];
  private lyricsOffsets = new Map<string, number>();
  private externalLyrics = false;
  private batterySaver = false;
  private network: Snapshot["network"] = { kind: "wifi", metered: false, networkId: "home" };
  private resumeOffer: ResumeOffer | undefined;
  private sleepTimer: SleepTimer | undefined;
  private selection: ActionTarget = { type: "none" };
  private playHistory: PlayHistoryEntry[] = [];
  private playerNotice: string | undefined;
  private autoplaySettings: AutoplaySettings = { chain: ["sonicSimilarity", "similarSongs", "topSongs", "random"], seedWindow: 5, minSimilarity: 0.4, exclusionWindow: 50, filterId: undefined };
  private ticker: NodeJS.Timeout | undefined;
  private timers = new Set<NodeJS.Timeout>();
  private started = false;
  private lastPositionEmit = 0;
  private consecutiveFailures = 0;
  private handoffOpen = false;
  private shutDown = false;

  constructor(config: CoreConfig, opts: FakeOptions = {}) {
    this.config = config;
    this.opts = { timeScale: opts.timeScale ?? 1, seed: opts.seed ?? 7, trackCount: opts.trackCount ?? 3200 };
    this.audio = {
      replayGain: "auto",
      replayGainPreampDb: 0,
      normalisation: false,
      eq: { enabled: false, preampDb: 0, bands: [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000].map((f) => ({ frequencyHz: f, gainDb: 0, q: 1.1 })), preset: "flat" },
      gapless: true,
      outputDevice: undefined,
      exclusive: false,
    };
    this.seedSettings();
    mkdirSync(join(config.cacheDir, "images"), { recursive: true });
    mkdirSync(config.dataDir, { recursive: true });
    const stateFile = join(config.dataDir, "fake-core-state.json");
    // Remember only whether a server was added, so restarts skip setup.
    void SEEK_STEP_MS;
    if (existsSync(stateFile) && process.env.HOCKET_FAKE_CORE_FRESH !== "1") {
      try {
        const saved = JSON.parse(readFileSync(stateFile, "utf8")) as { server?: { url: string; username: string; name: string } };
        if (saved.server) this.addServer(saved.server.url, saved.server.username, saved.server.name, true);
      } catch {
        /* ignore corrupt state */
      }
    }
  }

  // ---- CoreHandle ------------------------------------------------------
  onEvent(listener: (e: Event) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async shutdown(): Promise<void> {
    this.shutDown = true;
    if (this.ticker) clearInterval(this.ticker);
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
  }

  private emit(e: Event): void {
    if (this.shutDown) return;
    for (const l of this.listeners) {
      try {
        l(e);
      } catch (err) {
        console.error("[fake-core] listener threw", err);
      }
    }
  }

  private later(ms: number, fn: () => void): void {
    const t = setTimeout(() => {
      this.timers.delete(t);
      if (!this.shutDown) fn();
    }, Math.max(0, ms / this.opts.timeScale));
    this.timers.add(t);
  }

  // ---- Commands --------------------------------------------------------
  dispatch(cmd: Command): void {
    if (this.shutDown) return;
    try {
      this.handle(cmd);
    } catch (err) {
      this.emit({ type: "error", data: { kind: "internal", message: `fake core failed on ${cmd.type}`, detail: String(err) } });
    }
  }

  private handle(cmd: Command): void {
    switch (cmd.type) {
      case "start":
        this.start();
        return;
      case "shutdown":
        void this.shutdown();
        return;
      case "requestSnapshot":
        this.emitSnapshot();
        return;
      case "setNetworkState":
        this.network = cmd.data.state;
        return;
      case "setVisibility":
        return;
      case "setBatterySaver":
        this.batterySaver = cmd.data.enabled;
        return;
      case "addServer":
        this.addServer(cmd.data.url, cmd.data.username, cmd.data.name ?? new URL(cmd.data.url).host, false);
        return;
      case "removeServer":
        this.servers = this.servers.filter((s) => s.id !== cmd.data.server_id);
        this.lib = undefined;
        this.emit({ type: "serversChanged", data: { servers: this.servers } });
        this.persistServer(undefined);
        return;
      case "probeServer":
      case "syncLibrary":
        this.runSyncJob(cmd.type === "syncLibrary" && cmd.data.full);
        return;
      case "setTranscodingProfile":
        return;
      case "play":
        this.setPlaying(true);
        return;
      case "pause":
        this.setPlaying(false);
        return;
      case "togglePlay":
        if (!this.session.current && this.lib) {
          // Nothing loaded: play something sensible rather than nothing.
          return;
        }
        this.setPlaying(!this.transport.position.isPlaying);
        return;
      case "stop":
        this.stop();
        return;
      case "next":
        this.next(true);
        return;
      case "previous":
        this.previous();
        return;
      case "seekTo":
        this.seek(cmd.data.position_ms);
        return;
      case "seekBy":
        this.seek(this.positionNow() + cmd.data.delta_ms);
        return;
      case "setVolume":
        this.transport.volume = Math.max(0, Math.min(1, cmd.data.volume));
        this.emitTransport();
        return;
      case "playContext":
        this.playContext(cmd.data.args.context, cmd.data.args.startIndex, cmd.data.args.shuffle, cmd.data.args.saveOutgoing);
        return;
      case "playTracks":
        this.playContext({ serverId: cmd.data.server_id, kind: { type: "adHoc", data: { label: cmd.data.label } }, label: cmd.data.label, sort: "default", tracks: cmd.data.track_ids }, cmd.data.start_index, cmd.data.shuffle, true);
        return;
      case "playNext":
        this.insert(cmd.data.track_ids, true);
        return;
      case "playLater":
        this.insert(cmd.data.track_ids, false);
        return;
      case "jumpToQueueItem":
        this.jumpTo(cmd.data.key);
        return;
      case "removeQueueItems":
        this.removeItems(cmd.data.keys);
        return;
      case "moveQueueItem":
        this.moveItem(cmd.data.key, cmd.data.to_index);
        return;
      case "clearQueue":
        this.withUndo("Clear queue", () => {
          this.session = { ...this.session, context: undefined, order: [], cursor: 0, current: undefined, insertions: [], history: [] };
          this.stop();
        });
        return;
      case "clearInsertions":
        this.withUndo("Clear playing next", () => {
          this.session.insertions = [];
        });
        return;
      case "setShuffle":
        this.setShuffle(cmd.data.enabled);
        return;
      case "setRepeat":
        this.session.repeat = cmd.data.mode;
        this.emitQueue();
        return;
      case "setAutoplay":
        this.session.autoplay = cmd.data.enabled;
        this.emitQueue();
        return;
      case "setQueueMode":
        this.session.mode = cmd.data.mode;
        this.setSettingValue("queue.mode", JSON.stringify(cmd.data.mode));
        this.emitQueue();
        return;
      case "skipUnavailable":
        this.markUnavailable(cmd.data.key);
        return;
      case "restoreSavedQueue":
        this.restoreSavedQueue(cmd.data.id);
        return;
      case "pinSavedQueue": {
        const q = this.savedQueues.find((s) => s.id === cmd.data.id);
        if (q) {
          q.pinned = cmd.data.pinned;
          q.updatedAt = Date.now();
          this.emitSavedQueues();
        }
        return;
      }
      case "deleteSavedQueue":
        this.savedQueues = this.savedQueues.filter((s) => s.id !== cmd.data.id);
        this.emitSavedQueues();
        return;
      case "saveQueueAsPlaylist":
        this.saveQueueAsPlaylist(cmd.data.saved_queue_id, cmd.data.name);
        return;
      case "setSavedQueueCap":
        this.setSettingValue("queue.savedCap", JSON.stringify(cmd.data.cap));
        return;
      case "undo":
        this.undo();
        return;
      case "redo":
        this.redo();
        return;
      case "undoEntry": {
        const idx = this.undoStack.findIndex((u) => u.id === cmd.data.id);
        if (idx >= 0) {
          while (this.undoStack.length > idx) this.undo();
        }
        return;
      }
      case "restoreSelection":
        return;
      case "setRating":
        this.setRating(cmd.data.targets, cmd.data.rating);
        return;
      case "setLoved":
        this.setLoved(cmd.data.targets, cmd.data.loved);
        return;
      case "setArtistLoved": {
        const a = this.lib?.artists.find((x) => x.id === cmd.data.artist_id);
        if (a) {
          const prev = a.loved;
          a.loved = cmd.data.loved;
          this.libraryChanged("artists", [a.id]);
          this.pushUndo(cmd.data.loved ? "Love artist" : "Unlove artist", () => {
            a.loved = prev;
            this.libraryChanged("artists", [a.id]);
            return undefined;
          }, () => {
            a.loved = cmd.data.loved;
            this.libraryChanged("artists", [a.id]);
          });
        }
        return;
      }
      case "createPlaylist":
        this.createPlaylist(cmd.data.name, cmd.data.track_ids);
        return;
      case "deletePlaylist":
        if (this.lib) {
          this.lib.playlists = this.lib.playlists.filter((p) => p.playlist.id !== cmd.data.playlist_id);
          this.libraryChanged("playlists", [cmd.data.playlist_id]);
        }
        return;
      case "renamePlaylist": {
        const p = this.lib?.playlists.find((x) => x.playlist.id === cmd.data.playlist_id);
        if (p) {
          const prev = { name: p.playlist.name, comment: p.playlist.comment, pub: p.playlist.public };
          p.playlist.name = cmd.data.name;
          if (cmd.data.comment !== undefined) p.playlist.comment = cmd.data.comment;
          if (cmd.data.public !== undefined) p.playlist.public = cmd.data.public;
          p.playlist.changed = Date.now();
          this.libraryChanged("playlists", [p.playlist.id]);
          this.pushUndo("Rename playlist", () => {
            p.playlist.name = prev.name;
            p.playlist.comment = prev.comment;
            p.playlist.public = prev.pub;
            this.libraryChanged("playlists", [p.playlist.id]);
            return undefined;
          }, () => {
            p.playlist.name = cmd.data.name;
            this.libraryChanged("playlists", [p.playlist.id]);
          });
        }
        return;
      }
      case "playlistAdd":
        this.playlistAdd(cmd.data.playlist_id, cmd.data.track_ids, cmd.data.at_index);
        return;
      case "playlistRemove":
        this.playlistRemove(cmd.data.playlist_id, cmd.data.indices);
        return;
      case "playlistMove":
        this.playlistMove(cmd.data.playlist_id, cmd.data.from_index, cmd.data.to_index);
        return;
      case "scrobble":
        return;
      case "pin":
        this.pin(cmd.data.target, cmd.data.transcode);
        return;
      case "unpin":
        this.unpin(cmd.data.target);
        return;
      case "clearStreamCache":
        if (this.lib) for (const t of this.lib.tracks) if (t.offline === "cached") t.offline = "none";
        this.emit({ type: "storageChanged", data: { storage: this.storage() } });
        this.libraryChanged("tracks", []);
        return;
      case "setStorageWarnThreshold":
        this.setSettingValue("storage.warnThresholdBytes", JSON.stringify(cmd.data.bytes ?? 4 * 1024 ** 3));
        this.emit({ type: "storageChanged", data: { storage: this.storage() } });
        return;
      case "cancelJob":
        this.updateJob(cmd.data.id, (j) => (j.state = "cancelled"));
        return;
      case "retryJob":
        this.updateJob(cmd.data.id, (j) => {
          j.state = "running";
          j.failed = 0;
        });
        this.problems = this.problems.filter((p) => p.jobId !== cmd.data.id);
        this.emitProblems();
        return;
      case "pauseJob":
        this.updateJob(cmd.data.id, (j) => (j.state = "paused"));
        return;
      case "resumeJob":
        this.updateJob(cmd.data.id, (j) => (j.state = "running"));
        return;
      case "retryProblem":
      case "dismissProblem":
        this.problems = this.problems.filter((p) => p.id !== cmd.data.id);
        this.emitProblems();
        return;
      case "dismissAllProblems":
        this.problems = [];
        this.emitProblems();
        return;
      case "saveFilter": {
        const idx = this.filters.findIndex((f) => f.id === cmd.data.filter.id);
        if (idx >= 0) this.filters[idx] = cmd.data.filter;
        else this.filters.push(cmd.data.filter);
        this.emit({ type: "filtersChanged", data: { filters: this.filters } });
        return;
      }
      case "deleteFilter":
        this.filters = this.filters.filter((f) => f.id !== cmd.data.id);
        this.emit({ type: "filtersChanged", data: { filters: this.filters } });
        return;
      case "createSmartPlaylist":
        this.createPlaylist(cmd.data.name, this.evaluateFilter(cmd.data.filter).map((t) => t.id), true, cmd.data.filter);
        return;
      case "createStaticPlaylistFromFilter":
        this.createPlaylist(cmd.data.name, this.evaluateFilter(cmd.data.filter).map((t) => t.id));
        return;
      case "exportNsp": {
        const doc = this.nspDocument(cmd.data.filter);
        if (cmd.data.path) {
          try {
            writeFileSync(cmd.data.path, doc);
          } catch (err) {
            this.emit({ type: "error", data: { kind: "storage", message: "Couldn't write the .nsp file", detail: String(err) } });
            return;
          }
        }
        this.emit({ type: "nspExported", data: { filter_id: cmd.data.filter.id, document: doc, path: cmd.data.path } });
        return;
      }
      case "setAutoplaySettings":
        this.autoplaySettings = cmd.data.settings;
        return;
      case "setLyricsOffset":
        this.lyricsOffsets.set(cmd.data.track_id, cmd.data.offset_ms);
        this.emitLyrics(cmd.data.track_id);
        return;
      case "setExternalLyricsEnabled":
        this.externalLyrics = cmd.data.enabled;
        this.setSettingValue("lyrics.external.enabled", JSON.stringify(cmd.data.enabled));
        if (this.session.current) this.emitLyrics(this.session.current.trackId);
        return;
      case "fetchLyrics":
        this.emitLyrics(cmd.data.track_id);
        return;
      case "setSetting":
        this.setSettingValue(cmd.data.key, cmd.data.value);
        return;
      case "resetSetting":
        this.seedSettings(cmd.data.key);
        return;
      case "setSettingsSync":
        this.setSettingValue("sync.enabled", JSON.stringify(cmd.data.enabled));
        return;
      case "exportConfig":
        this.emit({ type: "configExported", data: { document: JSON.stringify(this.configDocument(cmd.data.include_secrets), null, 2) } });
        return;
      case "importConfig":
        this.importConfig(cmd.data.document);
        return;
      case "setAudioSettings":
        this.audio = cmd.data.settings;
        this.emit({ type: "audioSettingsChanged", data: { settings: this.audio } });
        return;
      case "setOutputDevice":
        this.audio.outputDevice = cmd.data.id;
        this.emit({ type: "audioSettingsChanged", data: { settings: this.audio } });
        return;
      case "refreshOutputDevices":
        this.emit({ type: "outputDevicesChanged", data: { devices: this.outputDevices } });
        return;
      case "setCoordinatorUrl":
        this.connection.coordinatorUrl = cmd.data.url;
        this.setSettingValue("connect.coordinatorUrl", JSON.stringify(cmd.data.url ?? null));
        this.emit({ type: "connectionChanged", data: { state: this.connection } });
        return;
      case "connectCoordinator":
        this.connection = { ...this.connection, tier: "coordinator", connected: true, roundTripMs: 48, peerCount: this.devices.length - 1, error: undefined };
        this.emit({ type: "connectionChanged", data: { state: this.connection } });
        return;
      case "disconnectCoordinator":
        this.connection = { ...this.connection, tier: "lan", connected: true, roundTripMs: 4 };
        this.emit({ type: "connectionChanged", data: { state: this.connection } });
        return;
      case "setLanDiscovery":
        this.setSettingValue("connect.lanDiscovery", JSON.stringify(cmd.data.enabled));
        return;
      case "openHandoffPicker":
        this.handoffOpen = true;
        this.emit({ type: "handoffPickerChanged", data: { open: true, targets: this.devices.filter((d) => !d.isSelf) } });
        this.later(700, () => {
          if (!this.handoffOpen) return;
          for (const d of this.devices) if (!d.isSelf) d.ready = true;
          this.emit({ type: "devicesChanged", data: { devices: this.devices } });
          this.emit({ type: "handoffPickerChanged", data: { open: true, targets: this.devices.filter((d) => !d.isSelf) } });
        });
        return;
      case "closeHandoffPicker":
        this.handoffOpen = false;
        for (const d of this.devices) if (!d.isSelf) d.ready = false;
        this.emit({ type: "handoffPickerChanged", data: { open: false, targets: [] } });
        return;
      case "handoffTo":
        this.handoffTo(cmd.data.device_id);
        return;
      case "resumeHere":
        this.resumeHere();
        return;
      case "dismissResumeOffer":
        this.resumeOffer = undefined;
        this.emit({ type: "resumeOfferChanged", data: { offer: undefined } });
        return;
      case "setSleepTimer":
        this.sleepTimer = cmd.data.timer;
        this.emit({ type: "sleepTimerChanged", data: { timer: this.sleepTimer } });
        return;
      case "backendReport":
        return;
      case "mediaSessionCommand":
        this.mediaSessionCommand(cmd.data.action, cmd.data.value);
        return;
      case "runAction":
        this.runAction(cmd.data.action_id, cmd.data.target);
        return;
      case "setShortcut":
        this.shortcutOverrides.set(cmd.data.action_id, cmd.data.shortcut);
        this.emit({ type: "shortcutsChanged", data: { shortcuts: this.shortcuts() } });
        return;
      case "setActionOrder":
        this.actionOrders.set(cmd.data.surface, cmd.data.action_ids.map(canonicalActionId));
        this.setSettingValue(`actions.order.${cmd.data.surface}`, JSON.stringify(cmd.data.action_ids.map(canonicalActionId)));
        this.emit({ type: "actionsChanged", data: { surface: cmd.data.surface } });
        return;
      case "setSelection":
        this.selection = cmd.data.target;
        return;
      case "touch":
        return;
      default: {
        const never: never = cmd;
        void never;
      }
    }
  }

  // ---- Lifecycle -------------------------------------------------------
  private start(): void {
    if (this.started) {
      this.emitSnapshot();
      return;
    }
    this.started = true;
    this.devices = [
      { id: this.config.deviceId, name: this.config.deviceName, platform: this.config.platform, appVersion: this.config.appVersion, playing: false, ready: false, lastSeen: Date.now(), isSelf: true },
    ];
    this.emit({ type: "started", data: { snapshot: this.snapshot() } });
    this.ticker = setInterval(() => this.tick(), 250);
    if (this.lib) this.afterServerReady();
  }

  private afterServerReady(): void {
    if (!this.lib) return;
    // A remote device that was playing something: the dormant resume offer.
    this.later(1500, () => {
      if (!this.lib) return;
      const pixel: DeviceInfo = { id: "dev-pixel8", name: "Pixel 8", platform: "android", appVersion: this.config.appVersion, playing: false, ready: false, lastSeen: Date.now() - 60_000, isSelf: false };
      if (!this.devices.some((d) => d.id === pixel.id)) this.devices.push(pixel);
      this.connection = { ...this.connection, tier: "lan", connected: true, roundTripMs: 6, peerCount: 1 };
      this.emit({ type: "devicesChanged", data: { devices: this.devices } });
      this.emit({ type: "connectionChanged", data: { state: this.connection } });
      if (!this.session.current && !this.resumeOffer) {
        const track = this.lib.tracks[42];
        if (track) {
          this.resumeOffer = { deviceName: "Pixel 8", track: summary(track), positionMs: 83_000, lastSeen: Date.now() - 60_000 };
          this.emit({ type: "resumeOfferChanged", data: { offer: this.resumeOffer } });
        }
      }
    });
  }

  private emitSnapshot(): void {
    const s = this.snapshot();
    this.emit({ type: "started", data: { snapshot: s } });
  }

  private snapshot(): Snapshot {
    return {
      servers: this.servers,
      session: this.sessionDocument(),
      queue: this.queueView(),
      transport: this.transport,
      connection: this.connection,
      devices: this.devices,
      jobs: this.jobs,
      problems: this.problems,
      undo: this.undoState(),
      settings: [...this.settings.values()],
      audio: this.audio,
      mediaSession: this.mediaSessionState(),
      resumeOffer: this.resumeOffer,
      sleepTimer: this.sleepTimer,
      network: this.network,
      batterySaver: this.batterySaver,
      syncProgress: undefined,
    };
  }

  private sessionDocument(): SessionDocument | undefined {
    if (!this.servers[0]) return undefined;
    return {
      schemaVersion: 1,
      sessionId: "fake-session",
      scope: `${this.servers[0].id}:${this.servers[0].username}`,
      revision: this.session.revision,
      updatedAt: Date.now(),
      context: this.session.context,
      mode: this.session.mode,
      cursor: this.session.cursor,
      current: this.session.current,
      history: this.session.history,
      insertions: this.session.insertions,
      shuffle: this.session.shuffle ? { seed: this.session.shuffle.seed, anchor: this.session.shuffle.anchor } : undefined,
      repeat: this.session.repeat,
      autoplay: this.session.autoplay,
      transport: this.transport,
      savedQueues: this.savedQueues,
    };
  }

  // ---- Servers and sync ------------------------------------------------
  private addServer(url: string, username: string, name: string, silent: boolean): void {
    if (!silent && !/^https?:\/\//.test(url)) {
      this.emit({ type: "error", data: { kind: "network", message: "Server URL must start with http:// or https://", detail: undefined } });
      return;
    }
    if (!silent && username.toLowerCase() === "wrong") {
      // Like the core: a failed probe is Error + toast only; no ServersChanged, no job, no problem.
      this.later(600, () => {
        this.toast("Couldn't reach the server: authentication failed: Wrong username or password", false);
        this.emit({ type: "error", data: { kind: "auth", message: "server probe failed", detail: "authentication failed: Wrong username or password" } });
      });
      return;
    }
    const id = `srv-${hash32(url + username).toString(16)}`;
    const server: ServerInfo = {
      id,
      url,
      username,
      name,
      capabilities: { serverVersion: "0.63.1", openSubsonic: true, extensions: ["transcodeOffset", "formPost", "songLyrics", "sonicSimilarity"], transcodeOffset: true, formPost: true, songLyrics: true, sonicSimilarity: true, apiKeyAuthentication: false, transcodingExtension: true, nativeApi: true, meetsFloor: true },
      lastSync: undefined,
      reachable: true,
    };
    this.servers = [server];
    this.lib = generateLibrary(id, this.opts.seed, this.opts.trackCount);
    this.pins = this.lib.albums.filter((a) => a.offline === "downloaded").map((a) => ({ target: { type: "album", data: { id: a.id } }, label: `${a.artist} — ${a.name}`, coverArt: a.coverArt, trackCount: a.songCount, downloadedCount: a.songCount, bytes: a.durationMs * 120, createdAt: Date.now() - 86_400_000, transcoded: false }));
    const firstPl = this.lib.playlists[0];
    if (firstPl) this.pins.push({ target: { type: "playlist", data: { id: firstPl.playlist.id } }, label: firstPl.playlist.name, coverArt: firstPl.playlist.coverArt, trackCount: firstPl.trackIds.length, downloadedCount: firstPl.trackIds.length, bytes: firstPl.playlist.durationMs * 100, createdAt: Date.now() - 3 * 86_400_000, transcoded: true });
    this.filters = [
      { id: "flt-1", name: "Loved, last 90 days", root: { type: "all", data: [{ type: "rule", data: { field: "loved", op: "isTrue", value: { type: "bool", data: true } } }, { type: "rule", data: { field: "lastPlayed", op: "inTheLast", value: { type: "days", data: 90 } } }] }, sort: "lastPlayed" as unknown as SortOrder, descending: true, limit: undefined },
      { id: "flt-2", name: "Downloaded high energy", root: { type: "all", data: [{ type: "rule", data: { field: "downloaded", op: "isTrue", value: { type: "bool", data: true } } }, { type: "rule", data: { field: "energy", op: "gt", value: { type: "number", data: 0.7 } } }] }, sort: "energy", descending: true, limit: 100 },
    ];
    this.filters[0]!.sort = "playCount";
    this.playHistory = this.seedHistory();
    if (!silent) {
      this.persistServer({ url, username, name });
      this.later(400, () => {
        this.emit({ type: "serversChanged", data: { servers: this.servers } });
        this.runSyncJob(true);
        if (this.started) this.afterServerReady();
      });
    } else {
      server.lastSync = Date.now() - 3_600_000;
    }
  }

  private persistServer(server: { url: string; username: string; name: string } | undefined): void {
    try {
      writeFileSync(join(this.config.dataDir, "fake-core-state.json"), JSON.stringify({ server }));
    } catch {
      /* best effort */
    }
  }

  private runSyncJob(full: boolean): void {
    const server = this.servers[0];
    if (!server || !this.lib) return;
    const total = this.lib.tracks.length;
    const job: Job = { id: newId("job"), kind: "librarySync", label: full ? `Full library sync · ${server.name}` : `Library refresh · ${server.name}`, state: "running", done: 0, total, failed: 0, createdAt: Date.now(), cancellable: true };
    this.jobs.unshift(job);
    this.emitJobs();
    const phases = ["artists", "albums", "tracks", "playlists"];
    let phaseIdx = 0;
    const step = () => {
      if (job.state === "cancelled") return;
      if (job.state === "paused") {
        this.later(300, step);
        return;
      }
      job.done = Math.min(total, job.done + Math.ceil(total / 12));
      phaseIdx = Math.min(phases.length - 1, Math.floor((job.done / total) * phases.length));
      this.emit({ type: "syncProgress", data: { progress: { serverId: server.id, phase: phases[phaseIdx] ?? "tracks", done: job.done, total, readyTables: phases.slice(0, phaseIdx), finished: false } } });
      this.emitJobs();
      if (job.done >= total) {
        job.state = "done";
        server.lastSync = Date.now();
        this.emit({ type: "syncProgress", data: { progress: { serverId: server.id, phase: "done", done: total, total, readyTables: phases, finished: true } } });
        this.emit({ type: "serversChanged", data: { servers: this.servers } });
        this.emit({ type: "libraryChanged", data: { server_id: server.id, tables: [], ids: [] } });
        this.emitJobs();
        return;
      }
      this.later(250, step);
    };
    this.later(250, step);
  }

  // ---- Queue -----------------------------------------------------------
  private contextTracks(ctx: QueueContext): string[] {
    if (!this.lib) return [];
    if (ctx.tracks && ctx.tracks.length) return ctx.tracks;
    const k = ctx.kind;
    switch (k.type) {
      case "album":
        return this.lib.albumTracks.get(k.data.id) ?? [];
      case "artist":
        return (this.lib.artistAlbums.get(k.data.id) ?? []).flatMap((a) => this.lib?.albumTracks.get(a) ?? []);
      case "playlist":
        return this.lib.playlists.find((p) => p.playlist.id === k.data.id)?.trackIds ?? [];
      case "genre":
        return this.lib.tracks.filter((t) => t.genre === k.data.name).map((t) => t.id);
      case "filter":
        return this.evaluateFilter(k.data.filter).map((t) => t.id);
      case "adHoc":
      case "autoplay":
        return ctx.tracks ?? [];
    }
  }

  private playContext(ctx: QueueContext, startIndex: number | undefined, shuffle: boolean, saveOutgoing: boolean): void {
    if (!this.lib) return;
    const tracks = this.contextTracks(ctx);
    if (!tracks.length) return;
    const resolved: QueueContext = { ...ctx, tracks };
    const before = this.captureSession();
    const outgoingPos = this.positionNow();
    if (saveOutgoing) this.autoSaveQueue();
    const start = Math.min(startIndex ?? 0, tracks.length - 1);
    const seed = shuffle ? (Date.now() & 0xffff) : 0;
    const order = shuffle ? permutation(tracks.length, seed, startIndex) : tracks.map((_, i) => i);
    this.session = {
      ...this.session,
      context: resolved,
      order,
      cursor: shuffle ? 0 : start,
      current: { key: newKey(), trackId: tracks[shuffle ? order[0]! : start]!, source: { type: "context", data: { index: shuffle ? order[0]! : start } }, unavailable: false },
      insertions: [],
      history: [],
      shuffle: shuffle ? { seed, anchor: startIndex } : undefined,
      revision: this.session.revision + 1,
    };
    this.loadCurrent(0, true);
    this.pushUndo(`Play ${ctx.label}`, () => {
      this.restoreSession(before);
      this.seek(outgoingPos);
      return undefined;
    }, () => this.playContext(resolved, startIndex, shuffle, false));
    this.emitAll();
  }

  private insert(trackIds: string[], next: boolean): void {
    if (!this.lib) return;
    const items: QueueItem[] = trackIds.filter((id) => this.lib?.tracksById.has(id)).map((id) => ({ key: newKey(), trackId: id, source: { type: "inserted" }, unavailable: false }));
    if (!items.length) return;
    this.withUndo(next ? `Play next (${items.length})` : `Play later (${items.length})`, () => {
      if (next) this.session.insertions.unshift(...items);
      else this.session.insertions.push(...items);
      if (!this.session.current) {
        const first = this.session.insertions.shift();
        if (first) {
          this.session.current = first;
          this.loadCurrent(0, true);
        }
      }
    });
    this.toast(next ? "Playing next" : "Added to queue", true);
  }

  private jumpTo(key: string): void {
    const s = this.session;
    const hIdx = s.history.findIndex((i) => i.key === key);
    const iIdx = s.insertions.findIndex((i) => i.key === key);
    const uIdx = s.order.slice(s.cursor + 1).findIndex((ci) => this.upcomingKey(ci) === key);
    this.withUndo("Jump in queue", () => {
      if (hIdx >= 0) {
        const item = s.history[hIdx]!;
        // Items after it in history go back to the future (insertions keep type).
        const after = s.history.splice(hIdx);
        after.shift();
        if (s.current) s.history.push(s.current);
        // Reverse-move: things that were played after the target were context items; leave cursor at target.
        if (item.source.type === "context") s.cursor = s.order.indexOf(item.source.data.index);
        else s.insertions.unshift(...after.filter((a) => a.source.type === "inserted"));
        s.current = { ...item, key: newKey() };
      } else if (iIdx >= 0) {
        if (s.current) s.history.push(s.current);
        const skipped = s.insertions.splice(0, iIdx + 1);
        s.current = skipped.pop();
        s.history.push(...skipped);
      } else if (uIdx >= 0) {
        if (s.current) s.history.push(s.current);
        s.history.push(...s.insertions);
        s.insertions = [];
        s.cursor = s.cursor + 1 + uIdx;
        const ci = s.order[s.cursor]!;
        s.current = { key: newKey(), trackId: s.context?.tracks?.[ci] ?? "", source: { type: "context", data: { index: ci } }, unavailable: false };
      }
      this.trimHistory();
      this.loadCurrent(0, true);
    });
  }

  private upcomingKey(contextIndex: number): string {
    return `ctx-${this.session.revision}-${contextIndex}`;
  }

  private removeItems(keys: string[]): void {
    const set = new Set(keys);
    this.withUndo(`Remove ${keys.length} from queue`, () => {
      const s = this.session;
      s.insertions = s.insertions.filter((i) => !set.has(i.key));
      s.history = s.history.filter((i) => !set.has(i.key));
      // Upcoming context entries removed => drop from order (explicit permutation).
      const removedCtx = new Set<number>();
      s.order.slice(s.cursor + 1).forEach((ci) => {
        if (set.has(this.upcomingKey(ci))) removedCtx.add(ci);
      });
      if (removedCtx.size) {
        const cursorCi = s.order[s.cursor];
        s.order = s.order.filter((ci) => !removedCtx.has(ci));
        s.cursor = cursorCi === undefined ? 0 : Math.max(0, s.order.indexOf(cursorCi));
      }
      if (s.current && set.has(s.current.key)) this.next(false);
    });
  }

  private moveItem(key: string, toIndex: number): void {
    this.withUndo("Reorder queue", () => {
      const s = this.session;
      const combined: { key: string; item?: QueueItem; ci?: number }[] = [
        ...s.insertions.map((i) => ({ key: i.key, item: i })),
        ...s.order.slice(s.cursor + 1).map((ci) => ({ key: this.upcomingKey(ci), ci })),
      ];
      const from = combined.findIndex((c) => c.key === key);
      if (from < 0) return;
      const [moved] = combined.splice(from, 1);
      combined.splice(Math.max(0, Math.min(toIndex, combined.length)), 0, moved!);
      // Split back: leading inserted items stay insertions; a context item moved
      // into the insertions region becomes an insertion (YouTube-style splice is
      // the mode's job; the fake keeps it simple and honest).
      const newIns: QueueItem[] = [];
      const newOrderTail: number[] = [];
      let seenCtx = false;
      for (const c of combined) {
        if (c.item) {
          if (!seenCtx) newIns.push(c.item);
          else newOrderTail.push(this.adoptAsContext(c.item));
        } else if (c.ci !== undefined) {
          seenCtx = true;
          newOrderTail.push(c.ci);
        }
      }
      s.insertions = newIns;
      s.order = [...s.order.slice(0, s.cursor + 1), ...newOrderTail];
      s.revision += 1;
    });
  }

  /** An inserted item dragged into the context region: append its track to the context. */
  private adoptAsContext(item: QueueItem): number {
    const s = this.session;
    if (!s.context) {
      s.context = { serverId: this.servers[0]?.id ?? "", kind: { type: "adHoc", data: { label: "Queue" } }, label: "Queue", sort: "default", tracks: [] };
    }
    s.context.tracks = [...(s.context.tracks ?? []), item.trackId];
    return s.context.tracks.length - 1;
  }

  private setShuffle(enabled: boolean): void {
    this.withUndo(enabled ? "Shuffle on" : "Shuffle off", () => {
      const s = this.session;
      if (!s.context?.tracks) return;
      const curCi = s.order[s.cursor];
      if (enabled) {
        const seed = Date.now() & 0xffff;
        s.shuffle = { seed, anchor: curCi };
        s.order = permutation(s.context.tracks.length, seed, curCi);
        s.cursor = 0;
      } else {
        s.shuffle = undefined;
        s.order = s.context.tracks.map((_, i) => i);
        s.cursor = curCi ?? 0;
      }
      s.revision += 1;
    });
  }

  private next(userInitiated: boolean): void {
    const s = this.session;
    if (!s.current) return;
    if (s.repeat === "one" && !userInitiated) {
      this.loadCurrent(0, true);
      return;
    }
    this.recordPlay();
    s.history.push(s.current);
    this.trimHistory();
    if (s.insertions.length) {
      s.current = s.insertions.shift();
    } else if (s.cursor + 1 < s.order.length) {
      s.cursor += 1;
      const ci = s.order[s.cursor]!;
      s.current = { key: newKey(), trackId: s.context?.tracks?.[ci] ?? "", source: { type: "context", data: { index: ci } }, unavailable: false };
    } else if (s.repeat === "all" && s.order.length) {
      s.cursor = 0;
      const ci = s.order[0]!;
      s.current = { key: newKey(), trackId: s.context?.tracks?.[ci] ?? "", source: { type: "context", data: { index: ci } }, unavailable: false };
    } else if (s.autoplay && this.lib) {
      const seedTrack = this.lib.tracksById.get(s.current.trackId);
      const pick = this.related(seedTrack?.id ?? "", 5).find((r) => !s.history.some((h) => h.trackId === r.track.id));
      if (pick) {
        s.current = { key: newKey(), trackId: pick.track.id, source: { type: "autoplay", data: { provider: pick.provider, reason: pick.reason, score: pick.score } }, unavailable: false };
      } else {
        s.current = undefined;
      }
    } else {
      s.current = undefined;
    }
    s.revision += 1;
    this.loadCurrent(0, s.current !== undefined && this.transport.position.isPlaying);
    if (!s.current) this.stop();
    this.emitAll();
  }

  private previous(): void {
    const s = this.session;
    if (this.positionNow() > 3000 || !s.history.length) {
      this.seek(0);
      return;
    }
    const item = s.history.pop()!;
    if (s.current) {
      if (s.current.source.type === "inserted" || s.current.source.type === "autoplay") s.insertions.unshift(s.current);
    }
    if (item.source.type === "context") {
      const at = s.order.indexOf(item.source.data.index);
      if (at >= 0) s.cursor = at;
    }
    s.current = item;
    s.revision += 1;
    this.loadCurrent(0, this.transport.position.isPlaying);
    this.emitAll();
  }

  private markUnavailable(key: string): void {
    const s = this.session;
    if (s.current?.key === key) {
      s.current.unavailable = true;
      this.playerNotice = `Couldn't play ${this.trackTitle(s.current.trackId)}, skipped`;
      this.emit({ type: "playerNotice", data: { message: this.playerNotice } });
      this.next(false);
    }
  }

  private trimHistory(): void {
    if (this.session.history.length > HISTORY_CAP) this.session.history.splice(0, this.session.history.length - HISTORY_CAP);
  }

  private loadCurrent(positionMs: number, play: boolean): void {
    const cur = this.session.current;
    this.transport = {
      ...this.transport,
      lease: { owner: this.config.deviceId, epoch: this.transport.lease.epoch + 1, expiresAt: Date.now() + 20_000 },
      position: { positionMs, takenAt: Date.now(), rate: 1, isPlaying: play && !!cur },
      buffering: false,
      playedMs: 0,
    };
    this.consecutiveFailures = 0;
    if (cur && this.playerNotice && !cur.unavailable) {
      this.playerNotice = undefined;
      this.emit({ type: "playerNotice", data: { message: undefined } });
    }
    this.emitNowPlaying();
    this.emitTransport();
    if (cur) this.emitLyrics(cur.trackId);
  }

  private setPlaying(playing: boolean): void {
    if (!this.session.current) {
      if (playing && this.resumeOffer) this.resumeHere();
      return;
    }
    const pos = this.positionNow();
    this.transport.position = { positionMs: pos, takenAt: Date.now(), rate: 1, isPlaying: playing };
    if (!this.transport.lease.owner) this.transport.lease = { owner: this.config.deviceId, epoch: this.transport.lease.epoch + 1, expiresAt: Date.now() + 20_000 };
    this.emitTransport();
  }

  private stop(): void {
    this.transport.position = { positionMs: 0, takenAt: Date.now(), rate: 1, isPlaying: false };
    this.emitTransport();
    this.emitNowPlaying();
  }

  private seek(ms: number): void {
    const cur = this.currentTrack();
    const max = cur?.durationMs ?? 0;
    const pos = Math.max(0, Math.min(max, Math.round(ms)));
    this.transport.position = { ...this.transport.position, positionMs: pos, takenAt: Date.now() };
    this.emitTransport();
  }

  private positionNow(): number {
    const p = this.transport.position;
    if (!p.isPlaying) return p.positionMs;
    return p.positionMs + (Date.now() - p.takenAt) * p.rate;
  }

  private tick(): void {
    const cur = this.currentTrack();
    if (!cur || !this.transport.position.isPlaying) return;
    const pos = this.positionNow();
    this.transport.playedMs += 250;
    if (this.sleepTimer?.endsAt !== undefined && Date.now() >= this.sleepTimer.endsAt) {
      this.sleepTimer = undefined;
      this.emit({ type: "sleepTimerChanged", data: { timer: undefined } });
      this.setPlaying(false);
      return;
    }
    if (pos >= cur.durationMs) {
      if (this.sleepTimer?.stopAtEndOfTrack) {
        this.sleepTimer = undefined;
        this.emit({ type: "sleepTimerChanged", data: { timer: undefined } });
        this.setPlaying(false);
        return;
      }
      this.next(false);
    }
  }

  private currentTrack(): Track | undefined {
    const id = this.session.current?.trackId;
    return id ? this.lib?.tracksById.get(id) : undefined;
  }

  private trackTitle(id: string): string {
    return this.lib?.tracksById.get(id)?.title ?? id;
  }

  private recordPlay(): void {
    const t = this.currentTrack();
    if (!t) return;
    const played = this.transport.playedMs;
    if (played < 30_000 && played < t.durationMs / 2) return;
    t.playCount += 1;
    t.lastPlayed = Date.now();
    this.playHistory.unshift({ track: summary(t), playedAt: Date.now(), playedMs: played, scrobbled: true, deviceId: this.config.deviceId });
  }

  private queueView(): QueueView {
    const s = this.session;
    const entry = (item: QueueItem): QueueEntry | undefined => {
      const t = this.lib?.tracksById.get(item.trackId);
      return t ? { item, track: summary(t) } : undefined;
    };
    const upcomingCis = s.order.slice(s.cursor + 1);
    return {
      contextLabel: s.context?.label,
      history: s.history.map(entry).filter((e): e is QueueEntry => !!e),
      current: s.current ? entry(s.current) : undefined,
      playingNext: s.insertions.map(entry).filter((e): e is QueueEntry => !!e),
      upcoming: upcomingCis
        .slice(0, UPCOMING_VIEW_CAP)
        .map((ci) => entry({ key: this.upcomingKey(ci), trackId: s.context?.tracks?.[ci] ?? "", source: { type: "context", data: { index: ci } }, unavailable: false }))
        .filter((e): e is QueueEntry => !!e),
      shuffle: !!s.shuffle,
      repeat: s.repeat,
      autoplay: s.autoplay,
      mode: s.mode,
      totalUpcoming: upcomingCis.length,
    };
  }

  // ---- Saved queues ----------------------------------------------------
  private contextIdentity(ctx: QueueContext): string {
    const k = ctx.kind;
    switch (k.type) {
      case "album":
      case "artist":
      case "playlist":
        return `${k.type}:${k.data.id}`;
      case "genre":
        return `genre:${k.data.name}`;
      case "filter":
        return `filter:${k.data.filter.id}`;
      case "adHoc":
        return `adhoc:${hash32((ctx.tracks ?? []).join(","))}`;
      case "autoplay":
        return "autoplay";
    }
  }

  private autoSaveQueue(): void {
    const s = this.session;
    if (!s.context || !s.current) return;
    if ((s.context.tracks?.length ?? 0) <= 1 && !s.insertions.length) return;
    if (this.savedQueueCap === 0) return;
    const identity = this.contextIdentity(s.context);
    const existing = this.savedQueues.find((q) => this.contextIdentity(q.context) === identity && !q.pinned);
    const cover = this.currentTrack()?.coverArt;
    const snap: SavedQueue = {
      id: existing?.id ?? newId("sq"),
      context: { ...s.context, tracks: s.context.kind.type === "adHoc" ? s.context.tracks : [] },
      label: s.context.label,
      cursor: s.cursor,
      current: s.current,
      history: s.history.slice(-HISTORY_CAP),
      insertions: s.insertions,
      shuffle: s.shuffle ? { seed: s.shuffle.seed, anchor: s.shuffle.anchor } : undefined,
      repeat: s.repeat,
      positionMs: Math.round(this.positionNow()),
      pinned: existing?.pinned ?? false,
      createdAt: existing?.createdAt ?? Date.now(),
      lastInteractedAt: Date.now(),
      updatedAt: Date.now(),
      trackCount: (s.context.tracks?.length ?? 0) + s.insertions.length,
      coverArt: cover,
    };
    this.savedQueues = [snap, ...this.savedQueues.filter((q) => q.id !== snap.id)];
    this.evictSavedQueues();
    this.emitSavedQueues();
  }

  private evictSavedQueues(): void {
    const unpinned = this.savedQueues.filter((q) => !q.pinned).sort((a, b) => b.lastInteractedAt - a.lastInteractedAt);
    const keep = new Set(unpinned.slice(0, this.savedQueueCap).map((q) => q.id));
    const month = Date.now() - 30 * 86_400_000;
    this.savedQueues = this.savedQueues.filter((q) => q.pinned || (keep.has(q.id) && q.lastInteractedAt > month));
  }

  private restoreSavedQueue(id: string): void {
    const q = this.savedQueues.find((s) => s.id === id);
    if (!q || !this.lib) return;
    const before = this.captureSession();
    this.autoSaveQueue();
    const tracks = this.contextTracks(q.context);
    this.session = {
      ...this.session,
      context: { ...q.context, tracks },
      order: q.shuffle ? permutation(tracks.length, q.shuffle.seed, q.shuffle.anchor) : tracks.map((_, i) => i),
      cursor: q.cursor,
      current: q.current,
      history: [...q.history],
      insertions: [...q.insertions],
      shuffle: q.shuffle ? { seed: q.shuffle.seed, anchor: q.shuffle.anchor } : undefined,
      repeat: q.repeat,
      revision: this.session.revision + 1,
    };
    q.lastInteractedAt = Date.now();
    this.loadCurrent(q.positionMs, false);
    this.pushUndo(`Restore ${q.label}`, () => {
      this.restoreSession(before);
      return undefined;
    }, () => this.restoreSavedQueue(id));
    this.emitAll();
  }

  private saveQueueAsPlaylist(savedQueueId: string | undefined, name: string): void {
    let ids: string[];
    if (savedQueueId) {
      const q = this.savedQueues.find((s) => s.id === savedQueueId);
      if (!q) return;
      ids = [...q.history, ...(q.current ? [q.current] : []), ...q.insertions].map((i) => i.trackId).concat(this.contextTracks(q.context).slice(q.cursor + 1));
    } else {
      const v = this.queueView();
      ids = [...v.history, ...(v.current ? [v.current] : []), ...v.playingNext, ...v.upcoming].map((e) => e.track.id);
    }
    this.createPlaylist(name, [...new Set(ids)]);
  }

  // ---- Undo ------------------------------------------------------------
  private captureSession(): { session: SessionState; transport: TransportState } {
    return { session: clone(this.session), transport: clone(this.transport) };
  }

  private restoreSession(snap: { session: SessionState; transport: TransportState }): void {
    this.session = clone(snap.session);
    this.transport = { ...clone(snap.transport), position: { ...snap.transport.position, takenAt: Date.now() } };
    this.emitAll();
  }

  private withUndo(label: string, mutate: () => void): void {
    const before = this.captureSession();
    const selection = clone(this.selection);
    mutate();
    this.session.revision += 1;
    const after = this.captureSession();
    this.pushUndo(label, () => {
      this.restoreSession(before);
      return undefined;
    }, () => this.restoreSession(after), selection);
    this.emitAll();
  }

  private pushUndo(label: string, undo: () => string | undefined, redo: () => void, selection?: ActionTarget): void {
    const now = Date.now();
    const top = this.undoStack[this.undoStack.length - 1];
    // Coalesce rapid repeats of the same label (dragging a rating, holding a key).
    if (top && top.label === label && now - top.at < 800) {
      top.redo = redo;
      top.at = now;
    } else {
      this.undoStack.push({ id: newId("undo"), label, at: now, undo, redo, selection });
      if (this.undoStack.length > 200) this.undoStack.shift();
    }
    this.redoStack = [];
    this.emitUndo();
  }

  private undo(): void {
    const rec = this.undoStack.pop();
    if (!rec) return;
    const note = rec.undo();
    if (rec.selection) this.selection = rec.selection;
    this.redoStack.push(rec);
    this.emitUndo();
    this.emit({ type: "toast", data: { toast: { id: newId("toast"), message: note ? `Undone: ${rec.label} (${note})` : `Undone: ${rec.label}`, actionLabel: "Redo", actionCommand: JSON.stringify({ type: "redo" } satisfies Command), durationMs: 5000 } } });
  }

  private redo(): void {
    const rec = this.redoStack.pop();
    if (!rec) return;
    rec.redo();
    this.undoStack.push(rec);
    this.emitUndo();
    this.emit({ type: "toast", data: { toast: { id: newId("toast"), message: `Redone: ${rec.label}`, actionLabel: "Undo", actionCommand: JSON.stringify({ type: "undo" } satisfies Command), durationMs: 5000 } } });
  }

  private undoState(): UndoState {
    const top = this.undoStack[this.undoStack.length - 1];
    const rtop = this.redoStack[this.redoStack.length - 1];
    const history: UndoEntry[] = [...this.undoStack].reverse().slice(0, 20).map((u) => ({ id: u.id, label: u.label, deviceId: this.config.deviceId, at: u.at, note: undefined }));
    return { canUndo: !!top, undoLabel: top?.label, canRedo: !!rtop, redoLabel: rtop?.label, history };
  }

  private toast(message: string, undoable: boolean): void {
    const toast: Toast = { id: newId("toast"), message, actionLabel: undoable ? "Undo" : undefined, actionCommand: undoable ? JSON.stringify({ type: "undo" } satisfies Command) : undefined, durationMs: 4000 };
    this.emit({ type: "toast", data: { toast } });
  }

  // ---- Library mutations ----------------------------------------------
  private ratingTargets(targets: RatingTarget[]): { tracks: Track[]; albums: NonNullable<FakeLibrary["albums"]> } {
    const tracks: Track[] = [];
    const albums: FakeLibrary["albums"] = [];
    for (const t of targets) {
      if (t.type === "track") {
        const tr = this.lib?.tracksById.get(t.data.id);
        if (tr) tracks.push(tr);
      } else {
        const al = this.lib?.albums.find((a) => a.id === t.data.id);
        if (al) albums.push(al);
      }
    }
    return { tracks, albums };
  }

  private setRating(targets: RatingTarget[], rating: number): void {
    const { tracks, albums } = this.ratingTargets(targets);
    const prev = new Map<string, number>([...tracks.map((t) => [t.id, t.rating] as const), ...albums.map((a) => [a.id, a.rating] as const)]);
    const apply = (r: (id: string) => number) => {
      for (const t of tracks) t.rating = r(t.id);
      for (const a of albums) a.rating = r(a.id);
      this.libraryChanged("tracks", [...tracks.map((t) => t.id), ...albums.map((a) => a.id)]);
      this.emitNowPlaying();
    };
    apply(() => rating);
    const bridgeOn = this.settings.get("ratings.loveBridge.enabled")?.value === "true";
    const threshold = this.settingNumber("ratings.loveBridge.threshold");
    if (bridgeOn && threshold > 0 && rating >= threshold) for (const t of tracks) t.loved = true;
    const n = tracks.length + albums.length;
    if (n > 20) this.runBulkJob("bulkRating", `Rate ${n} items`, n);
    this.pushUndo(n === 1 ? `Rate ${rating ? "★".repeat(rating) : "cleared"}` : `Rate ${n} items`, () => {
      let changed = 0;
      for (const t of tracks) if (t.rating !== rating) changed += 1;
      apply((id) => prev.get(id) ?? 0);
      return changed ? `undid ${n - changed} of ${n}, ${changed} changed elsewhere` : undefined;
    }, () => apply(() => rating), clone(this.selection));
  }

  private setLoved(targets: RatingTarget[], loved: boolean): void {
    const { tracks, albums } = this.ratingTargets(targets);
    const prev = new Map<string, boolean>([...tracks.map((t) => [t.id, t.loved] as const), ...albums.map((a) => [a.id, a.loved] as const)]);
    const apply = (f: (id: string) => boolean) => {
      for (const t of tracks) t.loved = f(t.id);
      for (const a of albums) a.loved = f(a.id);
      this.libraryChanged("tracks", [...tracks.map((t) => t.id), ...albums.map((a) => a.id)]);
      this.emitNowPlaying();
    };
    apply(() => loved);
    const n = tracks.length + albums.length;
    if (n > 20) this.runBulkJob("bulkLove", `${loved ? "Love" : "Unlove"} ${n} items`, n);
    this.pushUndo(n === 1 ? (loved ? "Love" : "Unlove") : `${loved ? "Love" : "Unlove"} ${n} items`, () => {
      apply((id) => prev.get(id) ?? false);
      return undefined;
    }, () => apply(() => loved), clone(this.selection));
  }

  private runBulkJob(kind: Job["kind"], label: string, total: number): void {
    const job: Job = { id: newId("job"), kind, label, state: "running", done: 0, total, failed: 0, createdAt: Date.now(), cancellable: true };
    this.jobs.unshift(job);
    this.emitJobs();
    const step = () => {
      if (job.state === "cancelled") return;
      if (job.state === "paused") {
        this.later(300, step);
        return;
      }
      job.done = Math.min(total, job.done + Math.max(1, Math.ceil(total / 10)));
      if (job.done >= total) {
        // One in ~3 bulk jobs finishes with a couple of problems, so the
        // "finished with problems" state is exercised.
        if (total > 40 && hash32(job.id) % 3 === 0) {
          job.failed = 2;
          job.state = "failed";
          this.problems.push({ id: newId("prb"), jobId: job.id, summary: `${label}: 2 items couldn't be saved`, detail: "Server returned 503 for setRating; the outbox will retry", createdAt: Date.now(), retryable: true });
          this.emitProblems();
        } else {
          job.state = "done";
        }
      }
      this.emitJobs();
      if (job.state === "running") this.later(200, step);
    };
    this.later(200, step);
  }

  private createPlaylist(name: string, trackIds: string[], smart = false, filter?: Filter): void {
    if (!this.lib) return;
    const id = newId("pl");
    const durationMs = trackIds.reduce((s, tid) => s + (this.lib?.tracksById.get(tid)?.durationMs ?? 0), 0);
    this.lib.playlists.push({
      playlist: { id, serverId: this.lib.serverId, name, comment: smart && filter ? `Smart: ${filter.name}` : undefined, owner: "you", public: false, songCount: trackIds.length, durationMs, coverArt: this.lib.tracksById.get(trackIds[0] ?? "")?.coverArt, created: Date.now(), changed: Date.now(), isSmart: smart, isMine: true, offline: "none" },
      trackIds: [...trackIds],
    });
    this.libraryChanged("playlists", [id]);
    this.toast(`Created playlist ${name}`, false);
  }

  private playlistAdd(playlistId: string, trackIds: string[], atIndex: number | undefined): void {
    const p = this.lib?.playlists.find((x) => x.playlist.id === playlistId);
    if (!p || p.playlist.isSmart) return;
    const before = [...p.trackIds];
    const apply = () => {
      if (atIndex === undefined) p.trackIds.push(...trackIds);
      else p.trackIds.splice(atIndex, 0, ...trackIds);
      this.refreshPlaylist(p);
    };
    apply();
    this.pushUndo(`Add ${trackIds.length} to ${p.playlist.name}`, () => {
      p.trackIds = [...before];
      this.refreshPlaylist(p);
      return undefined;
    }, apply, clone(this.selection));
  }

  private playlistRemove(playlistId: string, indices: number[]): void {
    const p = this.lib?.playlists.find((x) => x.playlist.id === playlistId);
    if (!p || p.playlist.isSmart) return;
    const before = [...p.trackIds];
    const set = new Set(indices);
    const apply = () => {
      p.trackIds = before.filter((_, i) => !set.has(i));
      this.refreshPlaylist(p);
    };
    apply();
    this.pushUndo(`Remove ${indices.length} from ${p.playlist.name}`, () => {
      p.trackIds = [...before];
      this.refreshPlaylist(p);
      return undefined;
    }, apply, clone(this.selection));
  }

  private playlistMove(playlistId: string, from: number, to: number): void {
    const p = this.lib?.playlists.find((x) => x.playlist.id === playlistId);
    if (!p || p.playlist.isSmart) return;
    const before = [...p.trackIds];
    const apply = () => {
      const arr = [...before];
      const [m] = arr.splice(from, 1);
      if (m !== undefined) arr.splice(to, 0, m);
      p.trackIds = arr;
      this.refreshPlaylist(p);
    };
    apply();
    this.pushUndo(`Reorder ${p.playlist.name}`, () => {
      p.trackIds = [...before];
      this.refreshPlaylist(p);
      return undefined;
    }, apply);
  }

  private refreshPlaylist(p: FakeLibrary["playlists"][number]): void {
    p.playlist.songCount = p.trackIds.length;
    p.playlist.durationMs = p.trackIds.reduce((s, tid) => s + (this.lib?.tracksById.get(tid)?.durationMs ?? 0), 0);
    p.playlist.changed = Date.now();
    this.libraryChanged("playlists", [p.playlist.id]);
  }

  private libraryChanged(table: string, ids: string[]): void {
    if (!this.lib) return;
    this.emit({ type: "libraryChanged", data: { server_id: this.lib.serverId, tables: [table], ids } });
  }

  // ---- Downloads -------------------------------------------------------
  private pin(target: Pin["target"], transcode: boolean): void {
    if (!this.lib) return;
    const ids = this.pinTrackIds(target);
    const label = this.pinLabel(target);
    if (this.pins.some((p) => JSON.stringify(p.target) === JSON.stringify(target))) return;
    const pin: Pin = { target, label, coverArt: this.lib.tracksById.get(ids[0] ?? "")?.coverArt, trackCount: ids.length, downloadedCount: 0, bytes: 0, createdAt: Date.now(), transcoded: transcode };
    this.pins.push(pin);
    this.emit({ type: "pinsChanged", data: { pins: this.pins } });
    const job: Job = { id: newId("job"), kind: "download", label: `Download ${label}`, state: "running", done: 0, total: ids.length, failed: 0, createdAt: Date.now(), cancellable: true };
    this.jobs.unshift(job);
    this.emitJobs();
    const step = () => {
      if (job.state === "cancelled") return;
      if (job.state === "paused") {
        this.later(300, step);
        return;
      }
      const id = ids[job.done];
      if (id) {
        const t = this.lib?.tracksById.get(id);
        if (t) {
          t.offline = "downloaded";
          pin.bytes += t.sizeBytes ?? 0;
        }
        job.done += 1;
        pin.downloadedCount = job.done;
      }
      if (job.done >= ids.length) {
        job.state = "done";
        this.libraryChanged("tracks", ids);
      }
      this.emitJobs();
      this.emit({ type: "pinsChanged", data: { pins: this.pins } });
      this.emit({ type: "storageChanged", data: { storage: this.storage() } });
      if (job.state === "running") this.later(120, step);
    };
    this.later(120, step);
  }

  private unpin(target: Pin["target"]): void {
    const key = JSON.stringify(target);
    const ids = this.pinTrackIds(target);
    this.pins = this.pins.filter((p) => JSON.stringify(p.target) !== key);
    for (const id of ids) {
      const t = this.lib?.tracksById.get(id);
      if (t && t.offline === "downloaded") t.offline = "none";
    }
    this.emit({ type: "pinsChanged", data: { pins: this.pins } });
    this.emit({ type: "storageChanged", data: { storage: this.storage() } });
    this.libraryChanged("tracks", ids);
  }

  private pinTrackIds(target: Pin["target"]): string[] {
    if (!this.lib) return [];
    switch (target.type) {
      case "track":
        return [target.data.id];
      case "album":
        return this.lib.albumTracks.get(target.data.id) ?? [];
      case "playlist":
        return this.lib.playlists.find((p) => p.playlist.id === target.data.id)?.trackIds ?? [];
    }
  }

  private pinLabel(target: Pin["target"]): string {
    if (!this.lib) return "";
    switch (target.type) {
      case "track":
        return this.lib.tracksById.get(target.data.id)?.title ?? target.data.id;
      case "album": {
        const a = this.lib.albums.find((x) => x.id === target.data.id);
        return a ? `${a.artist} — ${a.name}` : target.data.id;
      }
      case "playlist":
        return this.lib.playlists.find((p) => p.playlist.id === target.data.id)?.playlist.name ?? target.data.id;
    }
  }

  private storage(): StorageSummary {
    const dl = this.lib?.tracks.filter((t) => t.offline === "downloaded").reduce((s, t) => s + (t.sizeBytes ?? 0), 0) ?? 0;
    const cache = this.lib?.tracks.filter((t) => t.offline === "cached").reduce((s, t) => s + (t.sizeBytes ?? 0), 0) ?? 0;
    const warn = this.settingNumber("storage.warnThresholdBytes");
    return { downloadsBytes: dl, cacheBytes: cache, imagesBytes: 48 * 1024 * 1024, warnThresholdBytes: warn > 0 ? warn : undefined, freeBytes: 120 * 1024 ** 3 };
  }

  // ---- Settings --------------------------------------------------------
  private seedSettings(onlyKey?: string): void {
    // Mirrors crates/hocket-core/src/settings/registry.rs (keys, scopes, defaults).
    const defaults: [string, unknown, Setting["scope"]][] = [
      ["queue.mode", "apple", "accountSynced"],
      ["queue.savedCap", 10, "accountSynced"],
      ["queue.historyCap", 200, "accountSynced"],
      ["lyrics.external.enabled", false, "accountSynced"],
      ["lyrics.external.provider", "lrclib", "accountSynced"],
      ["lyrics.defaultOffsetMs", 0, "accountSynced"],
      ["lyrics.showTranslations", true, "accountSynced"],
      ["ratings.loveBridge.enabled", false, "accountSynced"],
      ["ratings.loveBridge.threshold", 4, "accountSynced"],
      ["battery.autoEngage", true, "deviceLocal"],
      ["battery.lyricsFps", 30, "deviceLocal"],
      ["battery.smallArtwork", true, "deviceLocal"],
      ["battery.pausePrefetch", true, "deviceLocal"],
      ["display.animatedBackground", true, "deviceLocal"],
      ["display.lyricsFps", 60, "deviceLocal"],
      ["display.theme", "system", "deviceLocal"],
      ["display.accent", null, "deviceLocal"],
      ["display.dynamicColour", true, "deviceLocal"],
      ["display.queuePanelSplit", 0.5, "deviceLocal"],
      ["actions.order.contextMenu", [], "accountSynced"],
      ["actions.order.sidebar", [], "accountSynced"],
      ["actions.order.mediaSession", [], "accountSynced"],
      ["shortcuts", {}, "deviceLocal"],
      ["sync.enabled", true, "deviceLocal"],
      ["connect.coordinatorUrl", null, "deviceLocal"],
      ["connect.lanDiscovery", true, "deviceLocal"],
      ["storage.warnThresholdBytes", 4 * 1024 ** 3, "deviceLocal"],
      ["storage.cacheMaxBytes", 2 * 1024 ** 3, "deviceLocal"],
      ["downloads.transcode", false, "deviceLocal"],
      ["downloads.wifiOnly", true, "deviceLocal"],
      ["sleep.defaultMinutes", 30, "accountSynced"],
      ["sleep.stopAtEndOfTrack", true, "accountSynced"],
      ["scrobble.enabled", true, "accountSynced"],
      ["scrobble.nowPlaying", true, "accountSynced"],
      ["library.syncIntervalMinutes", 60, "deviceLocal"],
      ["library.fullReconcileDays", 7, "deviceLocal"],
      ["search.includeServer", true, "deviceLocal"],
    ];
    for (const [key, value, scope] of defaults) {
      if (onlyKey && key !== onlyKey) continue;
      const setting: Setting = { key, value: JSON.stringify(value), scope, updatedAt: Date.now() };
      this.settings.set(key, setting);
      if (onlyKey) this.emit({ type: "settingChanged", data: { setting } });
    }
  }

  private setSettingValue(key: string, value: string): void {
    const prev = this.settings.get(key);
    const setting: Setting = { key, value, scope: prev?.scope ?? "deviceLocal", updatedAt: Date.now() };
    this.settings.set(key, setting);
    if (key === "queue.savedCap") {
      this.savedQueueCap = Number(JSON.parse(value)) || 0;
      this.evictSavedQueues();
      this.emitSavedQueues();
    }
    this.emit({ type: "settingChanged", data: { setting } });
  }

  private settingNumber(key: string): number {
    try {
      const v = JSON.parse(this.settings.get(key)?.value ?? "0");
      return typeof v === "number" ? v : 0;
    } catch {
      return 0;
    }
  }

  private shortcuts(): Shortcut[] {
    const defaults = new Map(shortcutDefaults().map((s) => [s.actionId, s.shortcut]));
    for (const k of DEFAULT_KEYMAP) if (!defaults.has(k.actionId)) defaults.set(k.actionId, k.shortcut);
    return [...defaults.entries()].map(([actionId, def]) => ({ actionId, shortcut: this.shortcutOverrides.has(actionId) ? this.shortcutOverrides.get(actionId) : def, defaultShortcut: def }));
  }

  private configDocument(includeSecrets: boolean): ConfigDocument {
    return {
      version: 1,
      exportedAt: Date.now(),
      settings: [...this.settings.values()],
      filters: this.filters,
      shortcuts: this.shortcuts().filter((s) => s.shortcut !== s.defaultShortcut),
      servers: this.servers,
      secrets: includeSecrets ? Object.fromEntries(this.servers.map((s) => [s.id, "<password redacted in fake core>"])) : undefined,
      audio: this.audio,
      autoplay: this.autoplaySettings,
    };
  }

  private importConfig(document: string): void {
    try {
      const doc = JSON.parse(document) as ConfigDocument;
      for (const s of doc.settings ?? []) this.settings.set(s.key, s);
      if (doc.filters) this.filters = doc.filters;
      for (const s of doc.shortcuts ?? []) this.shortcutOverrides.set(s.actionId, s.shortcut);
      if (doc.audio) this.audio = doc.audio;
      if (doc.autoplay) this.autoplaySettings = doc.autoplay;
      this.emitSnapshot();
      this.emit({ type: "filtersChanged", data: { filters: this.filters } });
      this.emit({ type: "shortcutsChanged", data: { shortcuts: this.shortcuts() } });
      this.emit({ type: "audioSettingsChanged", data: { settings: this.audio } });
      this.toast("Configuration imported", false);
    } catch (err) {
      this.emit({ type: "error", data: { kind: "storage", message: "Couldn't import the configuration document", detail: String(err) } });
    }
  }

  // ---- Connect ---------------------------------------------------------
  private handoffTo(deviceId: string): void {
    const target = this.devices.find((d) => d.id === deviceId);
    if (!target) return;
    for (const d of this.devices) d.playing = d.id === deviceId;
    this.handoffOpen = false;
    this.transport.lease = { owner: deviceId, epoch: this.transport.lease.epoch + 1, expiresAt: Date.now() + 20_000 };
    this.emit({ type: "handoffPickerChanged", data: { open: false, targets: [] } });
    this.emit({ type: "devicesChanged", data: { devices: this.devices } });
    this.emitTransport();
    this.emitMediaSession();
    this.toast(`Playing on ${target.name}`, false);
  }

  private resumeHere(): void {
    const offer = this.resumeOffer;
    if (!offer || !this.lib) return;
    const t = this.lib.tracksById.get(offer.track.id);
    if (!t || !t.albumId) return;
    const album = this.lib.albums.find((a) => a.id === t.albumId);
    const tracks = this.lib.albumTracks.get(t.albumId) ?? [];
    const idx = tracks.indexOf(t.id);
    this.resumeOffer = undefined;
    this.emit({ type: "resumeOfferChanged", data: { offer: undefined } });
    this.playContext({ serverId: this.lib.serverId, kind: { type: "album", data: { id: t.albumId } }, label: album?.name ?? "Album", sort: "default", tracks: [] }, idx, false, true);
    this.seek(offer.positionMs);
  }

  private mediaSessionCommand(action: MediaSessionAction, value: number | undefined): void {
    switch (action) {
      case "play":
        this.setPlaying(true);
        break;
      case "pause":
        this.setPlaying(false);
        break;
      case "next":
        this.next(true);
        break;
      case "previous":
        this.previous();
        break;
      case "seek":
        if (value !== undefined) this.seek(value);
        break;
      case "stop":
        this.stop();
        break;
      case "shuffle":
        this.setShuffle(value === undefined ? !this.session.shuffle : value > 0.5);
        break;
      case "repeat": {
        const modes: RepeatMode[] = ["off", "all", "one"];
        const cur = modes.indexOf(this.session.repeat);
        this.session.repeat = value === undefined ? modes[(cur + 1) % 3]! : (modes[Math.round(value)] ?? "off");
        this.emitQueue();
        break;
      }
      case "love": {
        const t = this.currentTrack();
        if (t) this.setLoved([{ type: "track", data: { id: t.id } }], !t.loved);
        break;
      }
      case "rate": {
        const t = this.currentTrack();
        if (t && value !== undefined) this.setRating([{ type: "track", data: { id: t.id } }], Math.round(value));
        break;
      }
    }
    this.emitMediaSession();
  }

  private mediaSessionState(): MediaSessionState {
    const t = this.currentTrack();
    const order = this.actionOrders.get("mediaSession") ?? DEFAULT_ORDERS.mediaSession ?? [];
    const map: Record<string, MediaSessionAction[]> = { togglePlay: ["play", "pause"], pause: ["pause"], stop: ["stop"], next: ["next"], previous: ["previous"], shuffle: ["shuffle"], repeat: ["repeat"], love: ["love"], rate: ["rate"] };
    const actions = [...new Set(order.flatMap((id) => map[canonicalActionId(id)] ?? []))];
    actions.push("seek");
    return {
      metadata: t ? { title: t.title, artist: t.artist, album: t.album, durationMs: t.durationMs, artworkPath: t.coverArt ? this.artworkPath(t.coverArt, 300) : undefined, trackId: t.id, loved: t.loved, rating: t.rating } : undefined,
      isPlaying: this.transport.position.isPlaying,
      position: this.transport.position,
      shuffle: !!this.session.shuffle,
      repeat: this.session.repeat,
      volume: this.transport.volume,
      actions,
      ownsTransport: this.transport.lease.owner === this.config.deviceId,
    };
  }

  private runAction(rawId: string, target: ActionTarget): void {
    const actionId = canonicalActionId(rawId);
    const trackIds = this.targetTrackIds(target);
    const sid = this.lib?.serverId ?? "";
    const ratingTargets = target.type === "albums" ? target.data.ids.map((id) => ({ type: "album" as const, data: { id } })) : trackIds.map((id) => ({ type: "track" as const, data: { id } }));
    switch (actionId) {
      case "play":
        if (target.type === "albums" && target.data.ids.length === 1) {
          const a = this.lib?.albums.find((x) => x.id === target.data.ids[0]);
          if (a) this.playContext({ serverId: sid, kind: { type: "album", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, 0, false, true);
        } else if (target.type === "playlists" && target.data.ids.length === 1) {
          const p = this.lib?.playlists.find((x) => x.playlist.id === target.data.ids[0]);
          if (p) this.playContext({ serverId: sid, kind: { type: "playlist", data: { id: p.playlist.id } }, label: p.playlist.name, sort: "default", tracks: [] }, 0, false, true);
        } else if (target.type === "artists" && target.data.ids.length === 1) {
          const a = this.lib?.artists.find((x) => x.id === target.data.ids[0]);
          if (a) this.playContext({ serverId: sid, kind: { type: "artist", data: { id: a.id } }, label: a.name, sort: "default", tracks: [] }, 0, false, true);
        } else if (trackIds.length) {
          this.playContext({ serverId: sid, kind: { type: "adHoc", data: { label: "Selection" } }, label: "Selection", sort: "default", tracks: trackIds }, 0, false, true);
        }
        return;
      case "playShuffled":
        if (trackIds.length) this.playContext({ serverId: sid, kind: { type: "adHoc", data: { label: "Selection" } }, label: this.targetLabel(target), sort: "default", tracks: trackIds }, undefined, true, true);
        return;
      case "playNext":
        if (target.type === "queueItems") {
          const keys = new Set(target.data.keys);
          this.withUndo("Play next", () => {
            const s = this.session;
            const moved = [...s.insertions.filter((i) => keys.has(i.key)), ...s.history.filter((i) => keys.has(i.key)).map((i) => ({ ...i, key: newKey() }))];
            s.insertions = [...moved, ...s.insertions.filter((i) => !keys.has(i.key))];
            s.history = s.history.filter((i) => !keys.has(i.key));
          });
        } else this.insert(trackIds, true);
        return;
      case "playLater":
        this.insert(trackIds, false);
        return;
      case "love":
      case "unlove": {
        const loved = actionId === "love";
        if (target.type === "none") {
          const t = this.currentTrack();
          if (t) this.setLoved([{ type: "track", data: { id: t.id } }], loved);
        } else if (target.type === "artists") for (const id of target.data.ids) this.handle({ type: "setArtistLoved", data: { artist_id: id, loved } });
        else this.setLoved(ratingTargets, loved);
        return;
      }
      case "download":
        if (target.type === "albums") for (const id of target.data.ids) this.pin({ type: "album", data: { id } }, false);
        else if (target.type === "playlists") for (const id of target.data.ids) this.pin({ type: "playlist", data: { id } }, false);
        else for (const id of trackIds) this.pin({ type: "track", data: { id } }, false);
        return;
      case "removeFromQueue":
        if (target.type === "queueItems") this.removeItems(target.data.keys);
        return;
      case "restoreSavedQueue":
        if (target.type === "savedQueue") this.restoreSavedQueue(target.data.id);
        return;
      case "pinSavedQueue":
      case "unpinSavedQueue":
        if (target.type === "savedQueue") this.handle({ type: "pinSavedQueue", data: { id: target.data.id, pinned: actionId === "pinSavedQueue" } });
        return;
      case "deleteSavedQueue":
        if (target.type === "savedQueue") this.handle({ type: "deleteSavedQueue", data: { id: target.data.id } });
        return;
      case "togglePlay":
        this.handle({ type: "togglePlay" });
        return;
      case "pause":
        this.setPlaying(false);
        return;
      case "stop":
        this.stop();
        return;
      case "next":
        this.next(true);
        return;
      case "previous":
        this.previous();
        return;
      case "seekBackward":
        this.seek(this.positionNow() - SEEK_STEP_MS);
        return;
      case "seekForward":
        this.seek(this.positionNow() + SEEK_STEP_MS);
        return;
      case "shuffle":
        this.setShuffle(!this.session.shuffle);
        return;
      case "repeat":
        this.mediaSessionCommand("repeat", undefined);
        return;
      case "autoplay":
        this.session.autoplay = !this.session.autoplay;
        this.emitQueue();
        return;
      case "volumeUp":
        this.handle({ type: "setVolume", data: { volume: this.transport.volume + 0.05 } });
        return;
      case "volumeDown":
        this.handle({ type: "setVolume", data: { volume: this.transport.volume - 0.05 } });
        return;
      case "resumeHere":
        this.resumeHere();
        return;
      case "clearQueue":
        this.handle({ type: "clearQueue" });
        return;
      case "undo":
        this.undo();
        return;
      case "redo":
        this.redo();
        return;
      default:
        if (/^rate[0-5]$/.test(actionId)) {
          const r = Number(actionId.slice(4));
          const targets = ratingTargets.length ? ratingTargets : this.currentTrack() ? [{ type: "track" as const, data: { id: this.currentTrack()!.id } }] : [];
          if (targets.length) this.setRating(targets, r);
          return;
        }
        if (actionId.startsWith("ui.") || actionId.startsWith("navigate") || ["selectAll", "remove", "findInList", "openCommandPalette", "toggleQueuePanel", "toggleLyrics", "toggleFullscreen", "toggleMiniPlayer", "handoff", "sleepTimer", "copyDiagnostics", "goToAlbum", "goToArtist", "addToPlaylist", "removeFromPlaylist", "deletePlaylist", "unpin", "saveQueueAsPlaylist"].includes(actionId)) return;
        this.emit({ type: "error", data: { kind: "internal", message: `Unknown action ${actionId}`, detail: undefined } });
    }
  }

  private targetTrackIds(target: ActionTarget): string[] {
    if (!this.lib) return [];
    switch (target.type) {
      case "tracks":
        return target.data.ids;
      case "albums":
        return target.data.ids.flatMap((id) => this.lib?.albumTracks.get(id) ?? []);
      case "artists":
        return target.data.ids.flatMap((id) => (this.lib?.artistAlbums.get(id) ?? []).flatMap((a) => this.lib?.albumTracks.get(a) ?? []));
      case "playlists":
        return target.data.ids.flatMap((id) => this.lib?.playlists.find((p) => p.playlist.id === id)?.trackIds ?? []);
      case "queueItems": {
        const v = this.queueView();
        const all = [...v.history, ...(v.current ? [v.current] : []), ...v.playingNext, ...v.upcoming];
        return target.data.keys.map((k) => all.find((e) => e.item.key === k)?.track.id).filter((x): x is string => !!x);
      }
      case "savedQueue":
      case "none":
        return [];
    }
  }

  private targetLabel(target: ActionTarget): string {
    if (target.type === "albums" && target.data.ids.length === 1) return this.lib?.albums.find((a) => a.id === target.data.ids[0])?.name ?? "Album";
    if (target.type === "playlists" && target.data.ids.length === 1) return this.lib?.playlists.find((p) => p.playlist.id === target.data.ids[0])?.playlist.name ?? "Playlist";
    if (target.type === "artists" && target.data.ids.length === 1) return this.lib?.artists.find((a) => a.id === target.data.ids[0])?.name ?? "Artist";
    return "Selection";
  }

  // ---- Emit helpers ----------------------------------------------------
  private emitAll(): void {
    this.emit({ type: "sessionChanged", data: { document: this.sessionDocument()! } });
    this.emitQueue();
    this.emitNowPlaying();
    this.emitTransport();
  }

  private emitQueue(): void {
    this.emit({ type: "queueChanged", data: { queue: this.queueView() } });
    this.emitMediaSession();
  }

  private emitNowPlaying(): void {
    const cur = this.session.current;
    const t = cur ? this.lib?.tracksById.get(cur.trackId) : undefined;
    this.emit({ type: "nowPlayingChanged", data: { entry: cur && t ? { item: cur, track: summary(t) } : undefined } });
    this.emitMediaSession();
  }

  private emitTransport(): void {
    this.emit({ type: "transportChanged", data: { transport: this.transport } });
    this.emitMediaSession();
  }

  private emitMediaSession(): void {
    this.emit({ type: "mediaSession", data: { state: this.mediaSessionState() } });
  }

  private emitSavedQueues(): void {
    this.emit({ type: "savedQueuesChanged", data: { queues: this.savedQueues } });
  }

  private emitUndo(): void {
    this.emit({ type: "undoChanged", data: { state: this.undoState() } });
  }

  private emitJobs(): void {
    this.emit({ type: "jobsChanged", data: { jobs: this.jobs } });
  }

  private emitProblems(): void {
    this.emit({ type: "problemsChanged", data: { problems: this.problems } });
  }

  private updateJob(id: string, f: (j: Job) => void): void {
    const j = this.jobs.find((x) => x.id === id);
    if (j) {
      f(j);
      this.emitJobs();
    }
  }

  private emitLyrics(trackId: string): void {
    const t = this.lib?.tracksById.get(trackId);
    const lyrics = t ? this.lyrics(t) : undefined;
    this.emit({ type: "lyricsChanged", data: { track_id: trackId, lyrics } });
  }

  private lyrics(t: Track): Lyrics | undefined {
    const l = lyricsFor(t, this.lyricsOffsets.get(t.id) ?? 0);
    if (l?.source === "external" && !this.externalLyrics) return undefined;
    return l;
  }

  // ---- Queries ---------------------------------------------------------
  async query(q: Query): Promise<QueryResult> {
    try {
      return this.answer(q);
    } catch (err) {
      this.emit({ type: "error", data: { kind: "internal", message: `fake core failed on query ${q.type}`, detail: String(err) } });
      throw err;
    }
  }

  private answer(q: Query): QueryResult {
    const lib = this.lib;
    switch (q.type) {
      case "snapshot":
        return { type: "snapshotResult", data: this.snapshot() };
      case "servers":
        return { type: "servers", data: this.servers };
      case "tracks": {
        const all = this.sortTracks(q.data.filter ? this.evaluateNode(q.data.filter) : (lib?.tracks ?? []), q.data.sort, q.data.descending);
        return { type: "tracks", data: { items: all.slice(q.data.page.offset, q.data.page.offset + q.data.page.limit), offset: q.data.page.offset, total: all.length } };
      }
      case "trackCount":
        return { type: "count", data: q.data.filter ? this.evaluateNode(q.data.filter).length : (lib?.tracks.length ?? 0) };
      case "track":
        return { type: "trackDetail", data: lib?.tracksById.get(q.data.id) };
      case "tracksByIds":
        return { type: "trackList", data: q.data.ids.map((id) => lib?.tracksById.get(id)).filter((t): t is Track => !!t) };
      case "albums": {
        let items = lib?.albums ?? [];
        if (q.data.artist_id) items = items.filter((a) => a.artistId === q.data.artist_id);
        if (q.data.genre) items = items.filter((a) => a.genre === q.data.genre);
        items = this.sortAlbums(items, q.data.sort, q.data.descending);
        return { type: "albums", data: { items: items.slice(q.data.page.offset, q.data.page.offset + q.data.page.limit), offset: q.data.page.offset, total: items.length } };
      }
      case "albumCount": {
        let items = lib?.albums ?? [];
        if (q.data.artist_id) items = items.filter((a) => a.artistId === q.data.artist_id);
        if (q.data.genre) items = items.filter((a) => a.genre === q.data.genre);
        return { type: "count", data: items.length };
      }
      case "album":
        return { type: "albumDetail", data: lib?.albums.find((a) => a.id === q.data.id) };
      case "albumTracks":
        return { type: "trackList", data: (lib?.albumTracks.get(q.data.id) ?? []).map((id) => lib?.tracksById.get(id)).filter((t): t is Track => !!t) };
      case "artists": {
        const items = [...(lib?.artists ?? [])].sort((a, b) => a.name.localeCompare(b.name));
        return { type: "artists", data: { items: items.slice(q.data.page.offset, q.data.page.offset + q.data.page.limit), offset: q.data.page.offset, total: items.length } };
      }
      case "artist":
        return { type: "artistDetail", data: lib?.artists.find((a) => a.id === q.data.id) };
      case "artistTopSongs": {
        const ids = (lib?.artistAlbums.get(q.data.id) ?? []).flatMap((a) => lib?.albumTracks.get(a) ?? []);
        const tracks = ids.map((id) => lib?.tracksById.get(id)).filter((t): t is Track => !!t).sort((a, b) => b.playCount - a.playCount);
        return { type: "trackList", data: tracks.slice(0, q.data.count) };
      }
      case "genres":
        return { type: "genres", data: lib?.genres ?? [] };
      case "playlists":
        return { type: "playlists", data: lib?.playlists.map((p) => p.playlist) ?? [] };
      case "playlist":
        return { type: "playlistDetail", data: lib?.playlists.find((p) => p.playlist.id === q.data.id)?.playlist };
      case "playlistTracks": {
        const ids = lib?.playlists.find((p) => p.playlist.id === q.data.id)?.trackIds ?? [];
        const items = ids.slice(q.data.page.offset, q.data.page.offset + q.data.page.limit).map((id) => lib?.tracksById.get(id)).filter((t): t is Track => !!t);
        return { type: "tracks", data: { items, offset: q.data.page.offset, total: ids.length } };
      }
      case "search":
        return { type: "search", data: this.search(q.data.query, q.data.limit, q.data.request_id, q.data.include_server) };
      case "queue":
        return { type: "queue", data: this.queueView() };
      case "savedQueues":
        return { type: "savedQueues", data: this.savedQueues };
      case "lyrics": {
        const t = lib?.tracksById.get(q.data.track_id);
        return { type: "lyricsResult", data: t ? this.lyrics(t) : undefined };
      }
      case "related":
        return { type: "related", data: this.related(q.data.track_id, q.data.count) };
      case "stats":
        return { type: "stats", data: this.stats(q.data.period_days) };
      case "recentlyPlayed":
        return { type: "history", data: this.playHistory.slice(0, q.data.limit) };
      case "jobs":
        return { type: "jobs", data: this.jobs };
      case "problems":
        return { type: "problems", data: this.problems };
      case "pins":
        return { type: "pins", data: this.pins };
      case "storage":
        return { type: "storage", data: this.storage() };
      case "filters":
        return { type: "filters", data: this.filters };
      case "filterPreview": {
        const matches = this.evaluateFilter(q.data.filter);
        const localOnly = this.localOnlyFields(q.data.filter.root);
        return { type: "preview", data: { count: matches.length, capability: { serverExpressible: localOnly.length === 0, localOnlyFields: localOnly }, sample: matches.slice(0, 8).map(summary) } };
      }
      case "settings":
        return { type: "settings", data: [...this.settings.values()] };
      case "setting":
        return { type: "settingDetail", data: this.settings.get(q.data.key) };
      case "audioSettings":
        return { type: "audio", data: this.audio };
      case "outputDevices":
        return { type: "outputDevices", data: this.outputDevices };
      case "connection":
        return { type: "connection", data: this.connection };
      case "devices":
        return { type: "devices", data: this.devices };
      case "undoState":
        return { type: "undo", data: this.undoState() };
      case "actions":
        return { type: "actions", data: this.actions(q.data.surface, q.data.target) };
      case "shortcuts":
        return { type: "shortcuts", data: this.shortcuts() };
      case "artwork":
        return { type: "path", data: this.artworkPath(q.data.id, q.data.size) };
      case "mediaSource": {
        const t = lib?.tracksById.get(q.data.track_id);
        return { type: "source", data: t ? { key: this.session.current?.key ?? "", track: summary(t), url: `${this.servers[0]?.url ?? "https://fake"}/rest/stream?id=${t.id}`, headers: {}, mimeType: t.contentType, gainDb: 0, transcoded: false } : undefined };
      }
      case "diagnostics":
        return { type: "text", data: this.diagnostics() };
      case "configDocument":
        return { type: "text", data: JSON.stringify(this.configDocument(q.data.include_secrets), null, 2) };
      default: {
        const never: never = q;
        void never;
        throw new Error("unhandled query");
      }
    }
  }

  private actions(surface: string, target: ActionTarget) {
    const ids = this.targetTrackIds(target);
    const tracks = ids.map((id) => this.lib?.tracksById.get(id)).filter((t): t is Track => !!t);
    const allLoved = target.type === "artists" ? undefined : tracks.length ? tracks.every((t) => t.loved) : undefined;
    const anyDownloaded = tracks.some((t) => t.offline === "downloaded") || (target.type === "playlists" && this.pins.some((p) => p.target.type === "playlist" && target.data.ids.includes(p.target.data.id)));
    const sq = target.type === "savedQueue" ? this.savedQueues.find((s) => s.id === target.data.id) : undefined;
    return describeActions(surface, target, this.actionOrders.get(surface), {
      hasCurrent: !!this.session.current,
      currentLoved: this.currentTrack()?.loved,
      canUndo: this.undoStack.length > 0,
      canRedo: this.redoStack.length > 0,
      hasResumeOffer: !!this.resumeOffer,
      allLoved,
      anyDownloaded,
      savedQueuePinned: sq?.pinned,
      inPlaylist: true,
    });
  }

  private artworkPath(id: string, size: number): string | undefined {
    const s = ARTWORK_SIZES.reduce((best, c) => (Math.abs(c - size) < Math.abs(best - size) ? c : best), 300);
    const file = join(this.config.cacheDir, "images", `${id}-${s}.png`);
    if (!existsSync(file)) {
      try {
        writeFileSync(file, coverPng(coverSeed(id), Math.min(s, 300)));
      } catch {
        return undefined;
      }
    }
    return file;
  }

  private search(query: string, limit: number, requestId: string, includeServer: boolean): SearchResults {
    const q = query.trim().toLowerCase();
    const lib = this.lib;
    const empty: SearchResults = { requestId, query, tracks: [], albums: [], artists: [], playlists: [], fromServer: false };
    if (!q || !lib) return empty;
    const tracks = lib.tracks.filter((t) => t.title.toLowerCase().includes(q) || t.artist?.toLowerCase().includes(q) || t.album?.toLowerCase().includes(q));
    const albums = lib.albums.filter((a) => a.name.toLowerCase().includes(q) || a.artist?.toLowerCase().includes(q));
    const artists = lib.artists.filter((a) => a.name.toLowerCase().includes(q));
    const playlists = lib.playlists.filter((p) => p.playlist.name.toLowerCase().includes(q)).map((p) => p.playlist);
    const local: SearchResults = { requestId, query, tracks: tracks.slice(0, limit).map(summary), albums: albums.slice(0, limit), artists: artists.slice(0, limit), playlists: playlists.slice(0, limit), fromServer: false };
    if (includeServer) {
      // "search3 fires in parallel only when local results are thin": server batch after a delay.
      this.later(450, () => {
        const extraTracks = tracks.slice(limit, limit + Math.max(3, Math.floor(limit / 2)));
        const extraAlbums = albums.slice(limit, limit + 3);
        this.emit({ type: "searchResults", data: { results: { requestId, query, tracks: extraTracks.map(summary), albums: extraAlbums, artists: artists.slice(limit, limit + 3), playlists: [], fromServer: true } } });
      });
    }
    return local;
  }

  private related(trackId: string, count: number): RelatedTrack[] {
    const lib = this.lib;
    const seed = lib?.tracksById.get(trackId);
    if (!lib || !seed) return [];
    const rng = new Rng(hash32(trackId));
    const candidates = lib.tracks.filter((t) => t.id !== trackId && (t.genre === seed.genre || t.artistId === seed.artistId));
    const out: RelatedTrack[] = [];
    const seen = new Set<string>();
    while (out.length < count && candidates.length && seen.size < candidates.length) {
      const c = rng.pick(candidates);
      if (seen.has(c.id)) continue;
      seen.add(c.id);
      const sonic = rng.chance(0.6);
      const score = sonic ? 0.55 + rng.next() * 0.4 : undefined;
      out.push({
        track: summary(c),
        provider: sonic ? "sonicSimilarity" : c.artistId === seed.artistId ? "topSongs" : "similarSongs",
        score,
        reason: sonic ? `Similar energy and key (${c.sonic?.key ?? "?"})` : c.artistId === seed.artistId ? `Top song by ${c.artist ?? ""}` : `Listeners of ${seed.artist ?? "this artist"} also play this`,
      });
    }
    return out.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
  }

  private seedHistory(): PlayHistoryEntry[] {
    const lib = this.lib;
    if (!lib) return [];
    const rng = new Rng(99);
    const out: PlayHistoryEntry[] = [];
    for (let i = 0; i < 400; i++) {
      const t = rng.pick(lib.tracks);
      out.push({ track: summary(t), playedAt: Date.now() - rng.int(0, 60 * 24) * 3_600_000 - rng.int(0, 3_600_000), playedMs: t.durationMs, scrobbled: true, deviceId: rng.chance(0.7) ? this.config.deviceId : "dev-pixel8" });
    }
    return out.sort((a, b) => b.playedAt - a.playedAt);
  }

  private stats(periodDays: number) {
    const since = Date.now() - periodDays * 86_400_000;
    const entries = this.playHistory.filter((e) => e.playedAt >= since);
    const byTrack = new Map<string, { n: number; t: TrackSummary }>();
    const byAlbum = new Map<string, number>();
    const byArtist = new Map<string, number>();
    const hours = new Array<number>(24).fill(0);
    const days = new Array<number>(7).fill(0);
    let totalMs = 0;
    for (const e of entries) {
      totalMs += e.playedMs;
      const b = byTrack.get(e.track.id) ?? { n: 0, t: e.track };
      b.n += 1;
      byTrack.set(e.track.id, b);
      if (e.track.albumId) byAlbum.set(e.track.albumId, (byAlbum.get(e.track.albumId) ?? 0) + 1);
      if (e.track.artistId) byArtist.set(e.track.artistId, (byArtist.get(e.track.artistId) ?? 0) + 1);
      const d = new Date(e.playedAt);
      hours[d.getHours()] = (hours[d.getHours()] ?? 0) + 1;
      const wd = (d.getDay() + 6) % 7;
      days[wd] = (days[wd] ?? 0) + 1;
    }
    const top = <T>(m: Map<string, number>, lookup: (id: string) => T | undefined): T[] => [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 10).map(([id]) => lookup(id)).filter((x): x is T => !!x);
    return {
      periodDays,
      totalPlays: entries.length,
      totalMs,
      topTracks: [...byTrack.values()].sort((a, b) => b.n - a.n).slice(0, 10).map((b) => b.t),
      topAlbums: top(byAlbum, (id) => this.lib?.albums.find((a) => a.id === id)),
      topArtists: top(byArtist, (id) => this.lib?.artists.find((a) => a.id === id)),
      playsByHour: hours,
      playsByWeekday: days,
    };
  }

  private diagnostics(): string {
    return [
      `Hocket ${this.config.appVersion} (fake core)`,
      `platform: ${this.config.platform}`,
      `device: ${this.config.deviceName} (${this.config.deviceId})`,
      `dataDir: ${this.config.dataDir}`,
      `cacheDir: ${this.config.cacheDir}`,
      `servers: ${this.servers.map((s) => `${s.name} ${s.url} v${s.capabilities.serverVersion}`).join(", ") || "none"}`,
      `tracks: ${this.lib?.tracks.length ?? 0}`,
      `jobs: ${this.jobs.length}, problems: ${this.problems.length}`,
      `undo depth: ${this.undoStack.length}`,
      `battery saver: ${this.batterySaver}`,
      `network: ${JSON.stringify(this.network)}`,
      "recent log:",
      "  (no crashes recorded)",
    ].join("\n");
  }

  // ---- Sorting and filtering -------------------------------------------
  private sortTracks(tracks: Track[], sort: SortOrder, desc: boolean): Track[] {
    const arr = [...tracks];
    const cmp = (a: Track, b: Track): number => {
      switch (sort) {
        case "title":
          return a.title.localeCompare(b.title);
        case "artist":
          return (a.artist ?? "").localeCompare(b.artist ?? "") || (a.album ?? "").localeCompare(b.album ?? "") || (a.trackNumber ?? 0) - (b.trackNumber ?? 0);
        case "album":
          return (a.album ?? "").localeCompare(b.album ?? "") || (a.discNumber ?? 0) - (b.discNumber ?? 0) || (a.trackNumber ?? 0) - (b.trackNumber ?? 0);
        case "year":
          return (a.year ?? 0) - (b.year ?? 0);
        case "dateAdded":
          return (a.created ?? 0) - (b.created ?? 0);
        case "rating":
          return a.rating - b.rating;
        case "playCount":
          return a.playCount - b.playCount;
        case "duration":
          return a.durationMs - b.durationMs;
        case "bpm":
          return (a.sonic?.bpm ?? 0) - (b.sonic?.bpm ?? 0);
        case "energy":
          return (a.sonic?.energy ?? 0) - (b.sonic?.energy ?? 0);
        case "random":
          return hash32(a.id + "r") - hash32(b.id + "r");
        case "default":
        default:
          return 0;
      }
    };
    if (sort !== "default") arr.sort((a, b) => (desc ? -cmp(a, b) : cmp(a, b)));
    return arr;
  }

  private sortAlbums(albums: FakeLibrary["albums"], sort: SortOrder, desc: boolean): FakeLibrary["albums"] {
    const arr = [...albums];
    const cmp = (a: FakeLibrary["albums"][number], b: FakeLibrary["albums"][number]): number => {
      switch (sort) {
        case "title":
        case "album":
          return a.name.localeCompare(b.name);
        case "artist":
          return (a.artist ?? "").localeCompare(b.artist ?? "") || (a.year ?? 0) - (b.year ?? 0);
        case "year":
          return (a.year ?? 0) - (b.year ?? 0);
        case "dateAdded":
          return (a.created ?? 0) - (b.created ?? 0);
        case "rating":
          return a.rating - b.rating;
        case "playCount":
          return a.playCount - b.playCount;
        case "duration":
          return a.durationMs - b.durationMs;
        case "random":
          return hash32(a.id + "r") - hash32(b.id + "r");
        default:
          return a.name.localeCompare(b.name);
      }
    };
    arr.sort((a, b) => (desc ? -cmp(a, b) : cmp(a, b)));
    return arr;
  }

  private evaluateFilter(f: Filter): Track[] {
    let out = this.sortTracks(this.evaluateNode(f.root), f.sort, f.descending);
    if (f.limit) out = out.slice(0, f.limit);
    return out;
  }

  private evaluateNode(node: FilterNode): Track[] {
    return (this.lib?.tracks ?? []).filter((t) => this.matches(node, t));
  }

  private matches(node: FilterNode, t: Track): boolean {
    switch (node.type) {
      case "all":
        return node.data.every((n) => this.matches(n, t));
      case "any":
        return node.data.some((n) => this.matches(n, t));
      case "rule":
        return this.matchRule(node.data, t);
    }
  }

  private matchRule(r: FilterRule, t: Track): boolean {
    const v = this.fieldValue(r.field, t);
    const val = r.value;
    const num = (x: unknown) => (typeof x === "number" ? x : typeof x === "string" ? Number(x) : x === true ? 1 : 0);
    const str = (x: unknown) => (x === undefined || x === null ? "" : String(x)).toLowerCase();
    const target = val.type === "text" ? val.data.toLowerCase() : val.type === "number" ? val.data : val.type === "bool" ? val.data : val.type === "days" ? val.data : val.type === "date" ? Date.parse(val.data) : val.type === "list" ? val.data.map((s) => s.toLowerCase()) : val.data;
    switch (r.op) {
      case "is":
        return Array.isArray(target) ? target.includes(str(v)) : typeof target === "number" ? num(v) === target : str(v) === String(target).toLowerCase();
      case "isNot":
        return !this.matchRule({ ...r, op: "is" }, t);
      case "contains":
        return str(v).includes(String(target));
      case "notContains":
        return !str(v).includes(String(target));
      case "startsWith":
        return str(v).startsWith(String(target));
      case "endsWith":
        return str(v).endsWith(String(target));
      case "gt":
        return num(v) > num(target);
      case "lt":
        return num(v) < num(target);
      case "inTheRange":
        return typeof target === "object" && target !== null && "low" in target ? num(v) >= target.low && num(v) <= target.high : false;
      case "before":
        return num(v) < num(target);
      case "after":
        return num(v) > num(target);
      case "inTheLast":
        return num(v) >= Date.now() - num(target) * 86_400_000;
      case "notInTheLast":
        return num(v) < Date.now() - num(target) * 86_400_000;
      case "isTrue":
        return !!v;
      case "isFalse":
        return !v;
    }
  }

  private fieldValue(field: FilterRule["field"], t: Track): unknown {
    switch (field) {
      case "title":
        return t.title;
      case "album":
        return t.album;
      case "artist":
        return t.artist;
      case "albumArtist":
        return t.albumArtist;
      case "genre":
        return t.genre;
      case "year":
        return t.year;
      case "dateAdded":
        return t.created;
      case "dateModified":
        return t.created;
      case "lastPlayed":
      case "localLastPlayed":
        return t.lastPlayed;
      case "playCount":
      case "localPlayCount":
        return t.playCount;
      case "rating":
        return t.rating;
      case "loved":
        return t.loved;
      case "duration":
        return t.durationMs / 1000;
      case "bitRate":
        return t.bitRate;
      case "filePath":
        return t.path;
      case "fileType":
        return t.suffix;
      case "comment":
        return t.comment;
      case "lyrics":
        return "";
      case "hasCoverArt":
        return !!t.coverArt;
      case "compilation":
        return false;
      case "discNumber":
        return t.discNumber;
      case "trackNumber":
        return t.trackNumber;
      case "bpm":
        return t.sonic?.bpm;
      case "key":
        return t.sonic?.key;
      case "energy":
        return t.sonic?.energy;
      case "mood":
        return t.sonic?.mood;
      case "downloaded":
        return t.offline === "downloaded";
      case "cached":
        return t.offline === "cached" || t.offline === "downloaded";
      case "inPlaylist":
        return this.lib?.playlists.some((p) => p.trackIds.includes(t.id)) ?? false;
    }
  }

  private localOnlyFields(node: FilterNode): FilterRule["field"][] {
    const localOnly: FilterRule["field"][] = ["downloaded", "cached", "localPlayCount", "localLastPlayed", "inPlaylist", "bpm", "key", "energy", "mood"];
    const out = new Set<FilterRule["field"]>();
    const walk = (n: FilterNode) => {
      if (n.type === "rule") {
        if (localOnly.includes(n.data.field)) out.add(n.data.field);
      } else n.data.forEach(walk);
    };
    walk(node);
    return [...out];
  }

  private nspDocument(f: Filter): string {
    const rule = (n: FilterNode): unknown => {
      if (n.type === "all") return { all: n.data.map(rule) };
      if (n.type === "any") return { any: n.data.map(rule) };
      const r = n.data;
      const v = r.value.type === "range" ? [r.value.data.low, r.value.data.high] : r.value.data;
      return { [r.op]: { [r.field]: v } };
    };
    return JSON.stringify({ name: f.name, comment: "Exported from Hocket", ...(rule(f.root) as object), sort: f.sort === "default" ? undefined : f.sort, order: f.descending ? "desc" : "asc", limit: f.limit }, null, 2);
  }
}
