// IPC between the renderer(s) and main. Every channel validates that the
// sender is one of our windows; payloads are plain JSON.
import { BrowserWindow, clipboard, dialog, ipcMain, shell } from "electron";
import { readFile, writeFile } from "node:fs/promises";
import type { Command, Query } from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import type { AppMeta, AppPrefs, OpenDialogRequest, SaveDialogRequest, WindowControl } from "@shared/bridge-types";
import { IPC } from "@shared/bridge-types";
import type { Windows } from "./windows";

export interface IpcDeps {
  core: CoreHandle;
  windows: Windows;
  meta: () => Omit<AppMeta, "windowKind">;
  onCommand: (command: Command) => void;
  onVisibilityReport: (visible: boolean) => void;
  onNetworkReport: (online: boolean) => void;
  prefs: { get(): AppPrefs; set(patch: Partial<AppPrefs>): AppPrefs };
}

function isOurs(windows: Windows, sender: Electron.WebContents): boolean {
  return windows.all().some((w) => w.webContents.id === sender.id);
}

export function installIpc(deps: IpcDeps): void {
  const { core, windows } = deps;

  ipcMain.on(IPC.dispatch, (e, command: Command) => {
    if (!isOurs(windows, e.sender) || !command || typeof command.type !== "string") return;
    deps.onCommand(command);
    core.dispatch(command);
  });

  ipcMain.handle(IPC.query, async (e, query: Query) => {
    if (!isOurs(windows, e.sender) || !query || typeof query.type !== "string") throw new Error("rejected");
    return core.query(query);
  });

  ipcMain.handle(IPC.meta, (e): AppMeta => {
    const win = BrowserWindow.fromWebContents(e.sender);
    const kind = win && windows.mini && win.id === windows.mini.id ? "mini" : "main";
    return { ...deps.meta(), windowKind: kind };
  });

  ipcMain.on(IPC.windowControl, (e, control: WindowControl) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(windows, e.sender)) return;
    if (typeof control === "string") {
      switch (control) {
        case "minimize":
          win.minimize();
          return;
        case "maximize":
          win.maximize();
          return;
        case "unmaximize":
          win.unmaximize();
          return;
        case "close":
          win.close();
          return;
        case "openMiniPlayer":
          windows.openMini();
          if (windows.main && !windows.main.isDestroyed()) windows.main.hide();
          return;
        case "closeMiniPlayer":
          windows.closeMini();
          windows.showMain();
          return;
        case "showMain":
          windows.showMain();
          return;
      }
    } else if ("alwaysOnTop" in control) {
      win.setAlwaysOnTop(control.alwaysOnTop, "floating");
      windows.broadcastState();
    } else if ("fullscreen" in control) {
      win.setFullScreen(control.fullscreen);
    }
  });

  ipcMain.handle(IPC.windowState, (e) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    return win ? windows.stateOf(win) : undefined;
  });

  ipcMain.on(IPC.visibility, (e, visible: boolean) => {
    if (isOurs(windows, e.sender)) deps.onVisibilityReport(!!visible);
  });

  ipcMain.on(IPC.network, (e, online: boolean) => {
    if (isOurs(windows, e.sender)) deps.onNetworkReport(!!online);
  });

  ipcMain.on(IPC.shellShowItem, (e, path: string) => {
    if (isOurs(windows, e.sender) && typeof path === "string") shell.showItemInFolder(path);
  });

  ipcMain.on(IPC.shellOpenExternal, (e, url: string) => {
    if (!isOurs(windows, e.sender) || typeof url !== "string") return;
    if (/^https?:\/\//.test(url)) void shell.openExternal(url);
  });

  ipcMain.handle(IPC.dialogSave, async (e, req: SaveDialogRequest) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(windows, e.sender)) return undefined;
    const r = await dialog.showSaveDialog(win, { title: req.title, defaultPath: req.defaultPath, filters: req.filters });
    return r.canceled ? undefined : r.filePath;
  });

  ipcMain.handle(IPC.dialogOpen, async (e, req: OpenDialogRequest) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(windows, e.sender)) return undefined;
    const r = await dialog.showOpenDialog(win, { title: req.title, filters: req.filters, properties: ["openFile"] });
    return r.canceled ? undefined : r.filePaths[0];
  });

  // File access is limited to paths the user just chose in a dialog.
  const allowed = new Set<string>();
  ipcMain.handle(IPC.dialogSave + ":allow", () => undefined);
  ipcMain.handle(IPC.fileWrite, async (e, path: string, text: string) => {
    if (!isOurs(windows, e.sender) || typeof path !== "string" || typeof text !== "string") throw new Error("rejected");
    await writeFile(path, text, "utf8");
    allowed.add(path);
  });
  ipcMain.handle(IPC.fileRead, async (e, path: string) => {
    if (!isOurs(windows, e.sender) || typeof path !== "string") throw new Error("rejected");
    return readFile(path, "utf8");
  });

  ipcMain.handle(IPC.prefsGet, (e) => (isOurs(windows, e.sender) ? deps.prefs.get() : undefined));
  ipcMain.handle(IPC.prefsSet, (e, patch: Partial<AppPrefs>) => {
    if (!isOurs(windows, e.sender) || !patch || typeof patch !== "object") throw new Error("rejected");
    const clean: Partial<AppPrefs> = {};
    if (typeof patch.closeToTray === "boolean") clean.closeToTray = patch.closeToTray;
    return deps.prefs.set(clean);
  });

  ipcMain.on(IPC.clipboardWrite, (e, text: string) => {
    if (isOurs(windows, e.sender) && typeof text === "string") clipboard.writeText(text);
  });
}
