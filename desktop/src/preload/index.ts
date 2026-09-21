// Preload: installs the typed `window.hocket` bridge. contextIsolation is on,
// sandbox is on, nothing from Node reaches the page.
import { contextBridge, ipcRenderer } from "electron";
import type { Command, Event, Query, QueryResult } from "@core/api";
import type { AppMeta, AppPrefs, HocketBridge, OpenDialogRequest, SaveDialogRequest, WindowControl, WindowState } from "@shared/bridge-types";
import { IPC } from "@shared/bridge-types";

function subscribe<T>(channel: string, listener: (payload: T) => void): () => void {
  const handler = (_e: Electron.IpcRendererEvent, payload: T) => listener(payload);
  ipcRenderer.on(channel, handler);
  return () => ipcRenderer.removeListener(channel, handler);
}

const bridge: HocketBridge = {
  prefs: {
    get: () => ipcRenderer.invoke(IPC.prefsGet) as Promise<AppPrefs>,
    set: (patch: Partial<AppPrefs>) => ipcRenderer.invoke(IPC.prefsSet, patch) as Promise<AppPrefs>,
  },
  dispatch(command: Command) {
    ipcRenderer.send(IPC.dispatch, command);
  },
  query(query: Query): Promise<QueryResult> {
    return ipcRenderer.invoke(IPC.query, query) as Promise<QueryResult>;
  },
  onEvent(listener: (event: Event) => void) {
    return subscribe<Event>(IPC.event, listener);
  },
  meta(): Promise<AppMeta> {
    return ipcRenderer.invoke(IPC.meta) as Promise<AppMeta>;
  },
  window: {
    minimize: () => ipcRenderer.send(IPC.windowControl, "minimize" satisfies WindowControl),
    maximize: () => ipcRenderer.send(IPC.windowControl, "maximize" satisfies WindowControl),
    unmaximize: () => ipcRenderer.send(IPC.windowControl, "unmaximize" satisfies WindowControl),
    close: () => ipcRenderer.send(IPC.windowControl, "close" satisfies WindowControl),
    setAlwaysOnTop: (flag: boolean) => ipcRenderer.send(IPC.windowControl, { alwaysOnTop: flag } satisfies WindowControl),
    openFullscreen: (flag: boolean) => ipcRenderer.send(IPC.windowControl, { fullscreen: flag } satisfies WindowControl),
    openMiniPlayer: () => ipcRenderer.send(IPC.windowControl, "openMiniPlayer" satisfies WindowControl),
    closeMiniPlayer: () => ipcRenderer.send(IPC.windowControl, "closeMiniPlayer" satisfies WindowControl),
    showMain: () => ipcRenderer.send(IPC.windowControl, "showMain" satisfies WindowControl),
    getState: () => ipcRenderer.invoke(IPC.windowState) as Promise<WindowState>,
    onState: (listener) => subscribe<WindowState>(IPC.windowStateChanged, listener),
    reportVisibility: (visible: boolean) => ipcRenderer.send(IPC.visibility, visible),
    reportNetwork: (online: boolean) => ipcRenderer.send(IPC.network, online),
  },
  shell: {
    showItemInFolder: (path: string) => ipcRenderer.send(IPC.shellShowItem, path),
    openExternal: (url: string) => ipcRenderer.send(IPC.shellOpenExternal, url),
  },
  dialog: {
    save: (req: SaveDialogRequest) => ipcRenderer.invoke(IPC.dialogSave, req) as Promise<string | undefined>,
    open: (req: OpenDialogRequest) => ipcRenderer.invoke(IPC.dialogOpen, req) as Promise<string | undefined>,
    writeTextFile: (path: string, text: string) => ipcRenderer.invoke(IPC.fileWrite, path, text) as Promise<void>,
    readTextFile: (path: string) => ipcRenderer.invoke(IPC.fileRead, path) as Promise<string>,
  },
  clipboard: {
    writeText: (text: string) => ipcRenderer.send(IPC.clipboardWrite, text),
  },
  onDeepLink: (listener) => subscribe<string>(IPC.deepLink, listener),
  onUiAction: (listener) => subscribe<string>(IPC.uiAction, listener),
};

contextBridge.exposeInMainWorld("hocket", bridge);
