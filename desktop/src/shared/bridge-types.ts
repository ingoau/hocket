// Shape of `window.hocket`, the contextBridge surface installed by the preload
// script. Everything crossing it is structured-clone-safe JSON.
import type { Command, Event, Query, QueryResult } from "@core/api";

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
  /**
   * Where server passwords go: "os" = encrypted by the OS keystore and
   * replayed on the next start; "volatile" = kept in memory for this session
   * only (no usable keystore, e.g. Linux without a keyring), so the user
   * signs in again after a restart.
   */
  credentialStorage: "os" | "volatile";
}

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

export interface HocketBridge {
  prefs: {
    get(): Promise<AppPrefs>;
    set(patch: Partial<AppPrefs>): Promise<AppPrefs>;
  };
  dispatch(command: Command): void;
  query(query: Query): Promise<QueryResult>;
  onEvent(listener: (event: Event) => void): () => void;
  meta(): Promise<AppMeta>;
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
  onDeepLink(listener: (url: string) => void): () => void;
  /** Tray/menu asks the renderer to run a UI action ("ui.palette" etc.). */
  onUiAction(listener: (actionId: string) => void): () => void;
}

export const IPC = {
  dispatch: "hocket:dispatch",
  query: "hocket:query",
  event: "hocket:event",
  meta: "hocket:meta",
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
} as const;

export type WindowControl =
  | "minimize"
  | "maximize"
  | "unmaximize"
  | "close"
  | "openMiniPlayer"
  | "closeMiniPlayer"
  | "showMain"
  | { alwaysOnTop: boolean }
  | { fullscreen: boolean };
