// IPC between the renderer(s) and main. Every channel validates that the
// sender is one of our windows and the shape of its payload; nothing a
// renderer sends can throw out of a handler (a bad payload is logged and
// dropped), and the renderer never names a filesystem path: files are read
// and written inside the dialog handlers, and the one core command that
// writes a file (ExportNsp) may only target a path a save dialog just chose.
import { BrowserWindow, clipboard, dialog, ipcMain, shell } from "electron";
import { readFile, writeFile } from "node:fs/promises";
import type { ArtworkLayout, Command, Query, QueryResult } from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import type { AppMeta, AppPrefs, OpenDialogRequest, OpenedTextFile, SaveDialogRequest, WindowControl } from "@shared/bridge-types";
import { ARTWORK_LAYOUT_MAX_SIDE, IPC } from "@shared/bridge-types";
import { t } from "@shared/strings";
import type { ArtworkRegistry } from "./artwork";
import { extractSecrets, parseConfigDocument, resolveSecretReferences } from "./config-secrets";
import type { Windows } from "./windows";

export interface IpcDeps {
  core: CoreHandle;
  windows: Windows;
  artwork: ArtworkRegistry;
  meta: () => Omit<AppMeta, "windowKind">;
  /** Observers of commands on their way to the core (credential capture, mirrored settings). */
  onCommand: (command: Command) => void;
  onVisibilityReport: (visible: boolean) => void;
  onNetworkReport: (online: boolean) => void;
  prefs: { get(): AppPrefs; set(patch: Partial<AppPrefs>): AppPrefs };
  credentials: { passwordFor(url: string, username: string): string | undefined };
  /** The core's immersive-artwork classifier (JSON in and out), when the native core has it. */
  artworkLayout?: (rgba: Buffer, width: number, height: number, requestJson: string) => string;
}

/** A side for `artworkLayout`: a whole number of pixels from 1 to ARTWORK_LAYOUT_MAX_SIDE. */
function isSide(v: unknown): v is number {
  return typeof v === "number" && Number.isInteger(v) && v >= 1 && v <= ARTWORK_LAYOUT_MAX_SIDE;
}

const MAX_TEXT_FILE = 16 * 1024 * 1024;

function isOurs(windows: Windows, sender: Electron.WebContents): boolean {
  return windows.all().some((w) => w.webContents.id === sender.id);
}

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** A `Command`/`Query` envelope: `type` is a string and `data`, when present, is a plain object. */
function isEnvelope(v: unknown): v is { type: string; data?: Record<string, unknown> } {
  return isObject(v) && typeof v.type === "string" && (v.data === undefined || isObject(v.data));
}

function filtersOf(v: unknown): Electron.FileFilter[] | undefined {
  if (!Array.isArray(v)) return undefined;
  const out: Electron.FileFilter[] = [];
  for (const f of v) {
    if (!isObject(f) || typeof f.name !== "string" || !Array.isArray(f.extensions)) continue;
    const extensions = f.extensions.filter((x): x is string => typeof x === "string" && /^[A-Za-z0-9]{1,16}$/.test(x));
    if (extensions.length) out.push({ name: f.name.slice(0, 64), extensions });
  }
  return out.length ? out : undefined;
}

function saveRequest(v: unknown): SaveDialogRequest | undefined {
  if (!isObject(v) || typeof v.title !== "string") return undefined;
  const defaultPath = typeof v.defaultPath === "string" && /^[^/\\:\0]{1,128}$/.test(v.defaultPath) ? v.defaultPath : undefined;
  return { title: v.title.slice(0, 128), defaultPath, filters: filtersOf(v.filters) };
}

function openRequest(v: unknown): OpenDialogRequest | undefined {
  if (!isObject(v) || typeof v.title !== "string") return undefined;
  return { title: v.title.slice(0, 128), filters: filtersOf(v.filters) };
}

export function installIpc(deps: IpcDeps): void {
  const { core, windows } = deps;

  /** Paths a save dialog handed out for a core-written file; consumed by the one ExportNsp that uses them. */
  const allowedExportPaths = new Set<string>();

  const dispatch = (command: Command): void => {
    try {
      deps.onCommand(command);
    } catch (err) {
      console.error(`[ipc] command observer failed on ${command.type}`, err);
    }
    try {
      core.dispatch(command);
    } catch (err) {
      console.error(`[ipc] core rejected ${command.type}`, err);
    }
  };

  ipcMain.on(IPC.dispatch, (e, command: unknown) => {
    if (!isOurs(windows, e.sender) || !isEnvelope(command)) return;
    if (command.type === "exportNsp") {
      const path = command.data?.path;
      if (path !== undefined && (typeof path !== "string" || !allowedExportPaths.delete(path))) {
        console.warn("[ipc] exportNsp refused: path was not chosen in a save dialog");
        return;
      }
    }
    dispatch(command as Command);
  });

  ipcMain.handle(IPC.query, async (e, query: unknown): Promise<QueryResult> => {
    if (!isOurs(windows, e.sender) || !isEnvelope(query)) throw new Error("rejected");
    const result = await core.query(query as Query);
    if (query.type === "artwork" && result.type === "path") {
      // Never hand a path to the page: it gets a token the protocol resolves.
      const token = typeof result.data === "string" ? await deps.artwork.register(result.data) : undefined;
      return { type: "path", data: token };
    }
    return result;
  });

  ipcMain.handle(IPC.artworkLayout, (e, rgba: unknown, width: unknown, height: unknown, request: unknown): ArtworkLayout | undefined => {
    if (!isOurs(windows, e.sender) || !deps.artworkLayout) return undefined;
    if (!(rgba instanceof Uint8Array) || !isSide(width) || !isSide(height) || rgba.length !== width * height * 4 || !isObject(request)) return undefined;
    try {
      return JSON.parse(deps.artworkLayout(Buffer.from(rgba.buffer, rgba.byteOffset, rgba.byteLength), width, height, JSON.stringify(request))) as ArtworkLayout;
    } catch (err) {
      console.error("[ipc] artworkLayout failed", err);
      return undefined;
    }
  });

  ipcMain.handle(IPC.meta, (e): AppMeta | undefined => {
    if (!isOurs(windows, e.sender)) return undefined;
    const win = BrowserWindow.fromWebContents(e.sender);
    const kind = win && windows.mini && win.id === windows.mini.id ? "mini" : "main";
    return { ...deps.meta(), windowKind: kind };
  });

  ipcMain.on(IPC.windowControl, (e, control: unknown) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(windows, e.sender)) return;
    try {
      applyWindowControl(windows, win, control);
    } catch (err) {
      console.error("[ipc] window control failed", err);
    }
  });

  ipcMain.handle(IPC.windowState, (e) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    return win && isOurs(windows, e.sender) ? windows.stateOf(win) : undefined;
  });

  ipcMain.on(IPC.visibility, (e, visible: unknown) => {
    if (isOurs(windows, e.sender)) deps.onVisibilityReport(!!visible);
  });

  ipcMain.on(IPC.network, (e, online: unknown) => {
    if (isOurs(windows, e.sender)) deps.onNetworkReport(!!online);
  });

  ipcMain.on(IPC.shellShowItem, (e, path: unknown) => {
    if (isOurs(windows, e.sender) && typeof path === "string" && path.length < 4096) shell.showItemInFolder(path);
  });

  ipcMain.on(IPC.shellOpenExternal, (e, url: unknown) => {
    if (!isOurs(windows, e.sender) || typeof url !== "string" || url.length > 4096) return;
    if (/^https?:\/\//.test(url)) void shell.openExternal(url);
  });

  ipcMain.handle(IPC.dialogSave, async (e, raw: unknown) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    const req = saveRequest(raw);
    if (!win || !isOurs(windows, e.sender) || !req) return undefined;
    const r = await dialog.showSaveDialog(win, { title: req.title, defaultPath: req.defaultPath, filters: req.filters });
    if (r.canceled || !r.filePath) return undefined;
    allowedExportPaths.add(r.filePath);
    return r.filePath;
  });

  ipcMain.handle(IPC.dialogSaveText, async (e, raw: unknown) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    const req = saveRequest(raw);
    const text = isObject(raw) ? raw.text : undefined;
    if (!win || !isOurs(windows, e.sender) || !req || typeof text !== "string" || text.length > MAX_TEXT_FILE) throw new Error("rejected");
    const r = await dialog.showSaveDialog(win, { title: req.title, defaultPath: req.defaultPath, filters: req.filters });
    if (r.canceled || !r.filePath) return undefined;
    await writeFile(r.filePath, text, "utf8");
    return r.filePath;
  });

  ipcMain.handle(IPC.dialogOpenText, async (e, raw: unknown): Promise<OpenedTextFile | undefined> => {
    const win = BrowserWindow.fromWebContents(e.sender);
    const req = openRequest(raw);
    if (!win || !isOurs(windows, e.sender) || !req) throw new Error("rejected");
    const r = await dialog.showOpenDialog(win, { title: req.title, filters: req.filters, properties: ["openFile"] });
    const path = r.filePaths[0];
    if (r.canceled || !path) return undefined;
    return { path, text: await readFile(path, "utf8") };
  });

  // Config backup: the core writes keystore references for passwords; main
  // resolves them here (never in the page) and only after a plain-text
  // warning, and turns them back into AddServer on import.
  ipcMain.handle(IPC.configExport, async (e, raw: unknown) => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(windows, e.sender) || typeof raw !== "string" || raw.length > MAX_TEXT_FILE) throw new Error("rejected");
    let text = raw;
    try {
      const doc = parseConfigDocument(raw);
      if (doc.secrets) {
        const { document, resolved, unresolved } = resolveSecretReferences(doc, (url, username) => deps.credentials.passwordFor(url, username));
        if (unresolved) console.warn(`[ipc] config export: ${unresolved} server password(s) are not in the credential store and were left out`);
        if (resolved) {
          const warn = await dialog.showMessageBox(win, {
            type: "warning",
            buttons: [t("config.exportSecretsConfirm"), t("dialog.cancel")],
            defaultId: 1,
            cancelId: 1,
            message: t("config.exportSecretsTitle"),
            detail: t("config.exportSecretsDetail", { n: resolved }),
          });
          if (warn.response !== 0) return undefined;
        }
        text = JSON.stringify(document, null, 2);
      }
    } catch (err) {
      console.error("[ipc] config export: not a config document", err);
      throw new Error("rejected");
    }
    const r = await dialog.showSaveDialog(win, { title: t("dialog.exportConfig"), defaultPath: "hocket-config.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (r.canceled || !r.filePath) return undefined;
    await writeFile(r.filePath, text, { encoding: "utf8", mode: 0o600 });
    return r.filePath;
  });

  ipcMain.handle(IPC.configImport, async (e): Promise<boolean> => {
    const win = BrowserWindow.fromWebContents(e.sender);
    if (!win || !isOurs(windows, e.sender)) throw new Error("rejected");
    const r = await dialog.showOpenDialog(win, { title: t("dialog.importConfig"), filters: [{ name: "JSON", extensions: ["json"] }], properties: ["openFile"] });
    const path = r.filePaths[0];
    if (r.canceled || !path) return false;
    let servers: ReturnType<typeof extractSecrets>["servers"];
    let document: string;
    try {
      const text = await readFile(path, "utf8");
      if (text.length > MAX_TEXT_FILE) throw new Error("file too large");
      const parsed = extractSecrets(parseConfigDocument(text));
      servers = parsed.servers;
      document = JSON.stringify(parsed.document);
    } catch (err) {
      console.error("[ipc] config import: unreadable document", err);
      await dialog.showMessageBox(win, { type: "error", message: t("config.importInvalid"), detail: String(err) });
      return false;
    }
    const confirm = await dialog.showMessageBox(win, {
      type: "question",
      buttons: [t("config.importConfirm"), t("dialog.cancel")],
      defaultId: 0,
      cancelId: 1,
      message: t("settings.importConfirm"),
      detail: servers.length ? t("config.importServers", { n: servers.length }) : undefined,
    });
    if (confirm.response !== 0) return false;
    for (const s of servers) dispatch({ type: "addServer", data: { url: s.url, username: s.username, password: s.password, name: s.name } });
    dispatch({ type: "importConfig", data: { document } });
    return true;
  });

  ipcMain.handle(IPC.prefsGet, (e) => (isOurs(windows, e.sender) ? deps.prefs.get() : undefined));
  ipcMain.handle(IPC.prefsSet, (e, patch: unknown) => {
    if (!isOurs(windows, e.sender) || !isObject(patch)) throw new Error("rejected");
    const clean: Partial<AppPrefs> = {};
    if (typeof patch.closeToTray === "boolean") clean.closeToTray = patch.closeToTray;
    return deps.prefs.set(clean);
  });

  ipcMain.on(IPC.clipboardWrite, (e, text: unknown) => {
    if (isOurs(windows, e.sender) && typeof text === "string") clipboard.writeText(text);
  });
}

function applyWindowControl(windows: Windows, win: BrowserWindow, control: unknown): void {
  if (typeof control === "string") {
    switch (control as WindowControl & string) {
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
      default:
        return;
    }
  }
  if (!isObject(control)) return;
  if ("alwaysOnTop" in control) {
    win.setAlwaysOnTop(!!control.alwaysOnTop, "floating");
    windows.broadcastState();
  } else if ("fullscreen" in control) {
    win.setFullScreen(!!control.fullscreen);
  }
}
