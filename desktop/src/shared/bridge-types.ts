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

export interface HocketBridge {
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
    save(req: SaveDialogRequest): Promise<string | undefined>;
    open(req: OpenDialogRequest): Promise<string | undefined>;
    writeTextFile(path: string, text: string): Promise<void>;
    readTextFile(path: string): Promise<string>;
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
  dialogOpen: "hocket:dialog:open",
  fileWrite: "hocket:file:write",
  fileRead: "hocket:file:read",
  clipboardWrite: "hocket:clipboard:write",
  deepLink: "hocket:deep-link",
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
