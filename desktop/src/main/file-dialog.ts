// File dialogs that start in the folder the user last picked from. Since Electron 43 a dialog
// without an absolute `defaultPath` opens in Downloads every time instead of where the OS
// left off, so the app remembers the folder itself (for this session).
import { basename, dirname, isAbsolute, join } from "node:path";
import { dialog, type BrowserWindow, type OpenDialogOptions, type OpenDialogReturnValue, type SaveDialogOptions, type SaveDialogReturnValue } from "electron";

let lastDir: string | undefined;

/** `defaultPath` placed in the last-used folder, unless it already names one. */
export function startingPath(defaultPath: string | undefined): string | undefined {
  if (lastDir === undefined || (defaultPath !== undefined && isAbsolute(defaultPath))) return defaultPath;
  return defaultPath ? join(lastDir, basename(defaultPath)) : lastDir;
}

export function rememberFolderOf(path: string | undefined): void {
  if (path) lastDir = dirname(path);
}

export async function showOpenDialog(win: BrowserWindow, opts: OpenDialogOptions): Promise<OpenDialogReturnValue> {
  const r = await dialog.showOpenDialog(win, { ...opts, defaultPath: startingPath(opts.defaultPath) });
  if (!r.canceled) rememberFolderOf(r.filePaths[0]);
  return r;
}

export async function showSaveDialog(win: BrowserWindow, opts: SaveDialogOptions): Promise<SaveDialogReturnValue> {
  const r = await dialog.showSaveDialog(win, { ...opts, defaultPath: startingPath(opts.defaultPath) });
  if (!r.canceled) rememberFolderOf(r.filePath);
  return r;
}

/** Tests only. */
export function forgetLastFolder(): void {
  lastDir = undefined;
}
