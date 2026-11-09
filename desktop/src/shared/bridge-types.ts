// Shape of `window.hocket`, the contextBridge surface installed by the preload
// script. Everything crossing it is structured-clone-safe JSON.
import type { ArtworkLayout, ArtworkLayoutRequest, Command, Event, Query, QueryResult, RoomDiagnosticsStatus } from "@core/api";

export type WindowKind = "main" | "mini";

export interface AppMeta {
  version: string;
  platform: "linux" | "macOs" | "windows";
  coreKind: "native" | "fake";
  mediaSession: "attached" | "unavailable";
  windowKind: WindowKind;
  deviceName: string;
  dataDir: string;
  cacheDir: string;
}

/**
 * Where server passwords go: "os" = encrypted by the OS keystore and
 * replayed on the next start; "volatile" = kept in memory for this session
 * only (no usable keystore, e.g. Linux without a keyring), so the user
 * signs in again after a restart.
 */
export type CredentialStorage = "os" | "volatile";

export interface WindowState {
  maximized: boolean;
  fullscreen: boolean;
  focused: boolean;
  visible: boolean;
  alwaysOnTop: boolean;
}

export interface SaveDialogRequest {
  title: string;
  defaultPath?: string;
  filters?: { name: string; extensions: string[] }[];
}

export interface OpenDialogRequest {
  title: string;
  filters?: { name: string; extensions: string[] }[];
}

/** Result of `dialog.openText`: the file the user picked and its contents. */
export interface OpenedTextFile {
  path: string;
  text: string;
}

/** Device-local app preferences the core registry doesn't define. */
export interface AppPrefs {
  closeToTray: boolean;
}

/** The developer diagnostics page's view of a bundle (main keeps the full text). */
export interface DiagnosticsSummary {
  fileName: string;
  generatedAt: number;
  /** Environment and the core's diagnostics: everything before the log. */
  head: string;
  /** The newest log lines, for the preview. */
  tail: string;
  logLines: number;
  logBytes: number;
  windowMs: number;
  /** Whether `share()` can open the OS share sheet (macOS). */
  canShare: boolean;
}

/** What a saved, copied or shared bundle covers. */
export interface DiagnosticsOptions {
  /**
   * Also carry the bundles of the Connect room and every other device in it (each answers only
   * while its own developer mode is on): the ones `collect()` gathered, waiting for it while it
   * runs. Without a collection, one runs first (up to 45 seconds).
   */
  allDevices?: boolean;
}

/** A screenshot main resized and re-encoded for a report, staged until it is sent or removed. */
export interface StagedScreenshot {
  /** What the page names it by (it never sees the path). */
  id: string;
  name: string;
  /** A small JPEG `data:` URL for the dialog's thumbnail. */
  thumbnailDataUrl: string;
}

/** A screen recording picked for a report: sent as it is, from where it is. */
export interface PickedRecording {
  id: string;
  name: string;
  size: number;
}

/** A picked image main couldn't decode (WebP): the page decodes it and hands it back with `stageImage`. */
export interface UndecodedImage {
  name: string;
  bytes: Uint8Array;
}

/** What the Save to Coordinator dialog sends: the staged files by id. */
export interface ReportSendRequest {
  note: string;
  screenshots: string[];
  recording?: string;
  /** As `DiagnosticsOptions.allDevices`, for the bundle that goes with it. */
  allDevices: boolean;
}

/** At most this many screenshots go with one report. */
export const REPORT_MAX_SCREENSHOTS = 5;

export interface HocketBridge {
  prefs: {
    get(): Promise<AppPrefs>;
    set(patch: Partial<AppPrefs>): Promise<AppPrefs>;
  };
  dispatch(command: Command): void;
  query(query: Query): Promise<QueryResult>;
  onEvent(listener: (event: Event) => void): () => void;
  meta(): Promise<AppMeta>;
  /** Resolves once main has opened the OS keystore, which may wait on a Keychain prompt. */
  credentialStorage(): Promise<CredentialStorage>;
  window: {
    minimize(): void;
    maximize(): void;
    unmaximize(): void;
    close(): void;
    setAlwaysOnTop(flag: boolean): void;
    openFullscreen(flag: boolean): void;
    openMiniPlayer(): void;
    closeMiniPlayer(): void;
    showMain(): void;
    /** The resolved theme, for the window chrome main draws (and the next launch's first paint). */
    setTheme(theme: WindowTheme): void;
    getState(): Promise<WindowState>;
    onState(listener: (state: WindowState) => void): () => void;
    /** Report page visibility so main can forward SetVisibility with focus. */
    reportVisibility(visible: boolean): void;
    reportNetwork(online: boolean): void;
  };
  shell: {
    showItemInFolder(path: string): void;
    openExternal(url: string): void;
  };
  dialog: {
    /**
     * Ask where a core-written file should go (Command.ExportNsp). The chosen
     * path is the only one main will let the next `exportNsp` write to; the
     * renderer never reads or writes files by path itself.
     */
    save(req: SaveDialogRequest): Promise<string | undefined>;
    /** Ask where to save, then write `text` there in main. Resolves to the path, or undefined when cancelled. */
    saveText(req: SaveDialogRequest & { text: string }): Promise<string | undefined>;
    /** Ask which file to open, then read it in main. Undefined when cancelled. */
    openText(req: OpenDialogRequest): Promise<OpenedTextFile | undefined>;
  };
  config: {
    /**
     * Export a `ConfigDocument`: main resolves any `keystore://` secret
     * references from the OS credential store (after an explicit plain-text
     * warning) and writes the file to a dialog-chosen path. Resolves to the
     * path, or undefined when cancelled.
     */
    export(document: string): Promise<string | undefined>;
    /**
     * Pick a config file, confirm, add every server whose password the file
     * carries, then import the rest. Resolves to true when an import was dispatched.
     */
    import(): Promise<boolean>;
  };
  clipboard: {
    writeText(text: string): void;
  };
  /**
   * Share one track's original audio file. Main resolves the file (a finished download, or a fresh
   * copy in the cache) and never tells the page where it is: macOS opens the share sheet, elsewhere
   * a save dialog copies it.
   */
  shareAudio(trackId: string): Promise<"shared" | "saved" | "cancelled" | "failed">;
  /**
   * The developer diagnostics bundle: environment, the core's diagnostics and the last hour of
   * logs. Every call builds a fresh one in main; the page only ever sees a summary.
   */
  diagnostics: {
    summary(): Promise<DiagnosticsSummary>;
    /**
     * Ask the Connect room and the other devices for their bundles, for the next `allDevices`
     * save, copy or share. Resolves once they are in (or the 45 s are up); progress arrives as
     * `roomDiagnosticsChanged` events. A call while one runs joins it; one within 15 s of the last
     * reuses that unless `force`d (the room turns a request that soon away; the core then answers
     * with the collection before). Resolves to the newest collection's status, for a page that
     * missed its events.
     */
    collect(options?: { force?: boolean }): Promise<RoomDiagnosticsStatus | undefined>;
    /** Ask where to save, then write the bundle there. Resolves to the path, or undefined when cancelled. */
    save(options?: DiagnosticsOptions): Promise<string | undefined>;
    /** Put the whole bundle on the clipboard. */
    copy(options?: DiagnosticsOptions): Promise<void>;
    /** Write the bundle to the app's log folder and show it in the file manager. */
    reveal(options?: DiagnosticsOptions): Promise<string | undefined>;
    /** macOS: write the bundle and open the share sheet with it. False where there is none. */
    share(options?: DiagnosticsOptions): Promise<boolean>;
  };
  /**
   * Save to Coordinator (docs/reports.md): main stages the attachments (screenshots resized to
   * at most 2000 px a side and re-encoded as JPEG, which drops their metadata) and starts the
   * core's upload; progress arrives as `reportUploadChanged` events.
   */
  report: {
    /** Pick screenshots (PNG, JPEG, WebP), as many as `room` allows. */
    pickScreenshots(room: number): Promise<(StagedScreenshot | UndecodedImage)[]>;
    /** Stage the image on the clipboard; undefined when it holds none. */
    pasteScreenshot(): Promise<StagedScreenshot | undefined>;
    /** Stage an image from its encoded bytes (PNG or JPEG); undefined when they don't decode. */
    stageImage(bytes: Uint8Array, name: string): Promise<StagedScreenshot | undefined>;
    /** Pick one screen recording (mp4, webm, mov, mkv). */
    pickRecording(): Promise<PickedRecording | undefined>;
    /** Forget staged attachments (deleting the screenshots' copies). */
    discard(ids: string[]): Promise<void>;
    /** Build the bundle and start the upload. False when one is already running or nothing matched. */
    send(request: ReportSendRequest): Promise<boolean>;
  };
  onDeepLink(listener: (url: string) => void): () => void;
  /**
   * How the full player shows a cover (the core's immersive-artwork classifier): its pixels as
   * RGBA8 (at most ARTWORK_LAYOUT_MAX_SIDE a side) and the request. Undefined without the native core.
   */
  artworkLayout(rgba: Uint8Array, width: number, height: number, request: ArtworkLayoutRequest): Promise<ArtworkLayout | undefined>;
  /** Tray/menu asks the renderer to run a UI action ("ui.palette" etc.). */
  onUiAction(listener: (actionId: string) => void): () => void;
}

export const IPC = {
  dispatch: "hocket:dispatch",
  query: "hocket:query",
  event: "hocket:event",
  meta: "hocket:meta",
  credentialStorage: "hocket:credential-storage",
  windowControl: "hocket:window:control",
  windowState: "hocket:window:state",
  windowStateChanged: "hocket:window:state-changed",
  visibility: "hocket:window:visibility",
  network: "hocket:network",
  shellShowItem: "hocket:shell:show-item",
  shellOpenExternal: "hocket:shell:open-external",
  dialogSave: "hocket:dialog:save",
  dialogSaveText: "hocket:dialog:save-text",
  dialogOpenText: "hocket:dialog:open-text",
  configExport: "hocket:config:export",
  configImport: "hocket:config:import",
  clipboardWrite: "hocket:clipboard:write",
  deepLink: "hocket:deep-link",
  prefsGet: "hocket:prefs:get",
  prefsSet: "hocket:prefs:set",
  uiAction: "hocket:ui-action",
  artworkLayout: "hocket:artwork:layout",
  diagnosticsSummary: "hocket:diagnostics:summary",
  diagnosticsCollect: "hocket:diagnostics:collect",
  diagnosticsSave: "hocket:diagnostics:save",
  diagnosticsCopy: "hocket:diagnostics:copy",
  diagnosticsReveal: "hocket:diagnostics:reveal",
  diagnosticsShare: "hocket:diagnostics:share",
  shareAudio: "hocket:share:audio",
  reportPickScreenshots: "hocket:report:pick-screenshots",
  reportPasteScreenshot: "hocket:report:paste-screenshot",
  reportStageImage: "hocket:report:stage-image",
  reportPickRecording: "hocket:report:pick-recording",
  reportDiscard: "hocket:report:discard",
  reportSend: "hocket:report:send",
} as const;

/** The largest cover (each side) the renderer may hand `artworkLayout`; it decodes to 256. */
export const ARTWORK_LAYOUT_MAX_SIDE = 512;

export type WindowControl =
  | "minimize"
  | "maximize"
  | "unmaximize"
  | "close"
  | "openMiniPlayer"
  | "closeMiniPlayer"
  | "showMain"
  | { alwaysOnTop: boolean }
  | { fullscreen: boolean }
  /** The renderer's resolved theme: the window's background and (Windows) caption buttons follow it. */
  | { theme: WindowTheme };

export type WindowTheme = "light" | "dark";
