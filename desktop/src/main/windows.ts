// Window management: the main window, the always-on-top mini player and a
// persistent hidden anchor window (Windows SMTC is tied to an HWND; the main
// window is destroyed on close-to-tray, this one never is).
import { BrowserWindow, app, screen } from "electron";
import { join } from "node:path";
import { APP_SCHEME, DEV_SERVER_URL } from "@shared/constants";
import type { WindowKind, WindowState } from "@shared/bridge-types";
import { IPC } from "@shared/bridge-types";
import type { StoreFile } from "./store-file";

export interface WindowsOptions {
  appRoot: string;
  isDev: boolean;
  store: StoreFile;
  /** Called when the main window is asked to close: return true to hide instead of closing. */
  shouldHideOnClose: () => boolean;
  onVisibility: (visible: boolean, focused: boolean) => void;
}

export class Windows {
  main: BrowserWindow | undefined;
  mini: BrowserWindow | undefined;
  anchor: BrowserWindow | undefined;
  quitting = false;
  private readonly preload: string;

  constructor(private readonly opts: WindowsOptions) {
    this.preload = join(opts.appRoot, "out", "preload", "index.cjs");
  }

  private baseWebPreferences() {
    return {
      preload: this.preload,
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      webSecurity: true,
      spellcheck: false,
      backgroundThrottling: true,
    };
  }

  private load(win: BrowserWindow, kind: WindowKind): void {
    if (this.opts.isDev) {
      void win.loadURL(`${DEV_SERVER_URL}/index.html?window=${kind}`);
    } else {
      void win.loadURL(`${APP_SCHEME}://renderer/index.html?window=${kind}`);
    }
  }

  ensureAnchor(): BrowserWindow {
    if (this.anchor && !this.anchor.isDestroyed()) return this.anchor;
    this.anchor = new BrowserWindow({ show: false, width: 1, height: 1, skipTaskbar: true, title: "Hocket", webPreferences: { sandbox: true, contextIsolation: true } });
    this.anchor.on("close", (e) => {
      if (!this.quitting) e.preventDefault();
    });
    return this.anchor;
  }

  /** HWND (or X11 window id) of the anchor as a number for the media session addon. */
  anchorHandle(): number | undefined {
    try {
      const buf = this.ensureAnchor().getNativeWindowHandle();
      if (buf.length >= 8) {
        const big = buf.readBigUInt64LE(0);
        return big <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(big) : undefined;
      }
      return buf.readUInt32LE(0);
    } catch {
      return undefined;
    }
  }

  createMain(): BrowserWindow {
    if (this.main && !this.main.isDestroyed()) {
      this.showMain();
      return this.main;
    }
    const saved = this.opts.store.get("mainBounds");
    const bounds = saved && this.boundsOnScreen(saved) ? saved : { width: 1280, height: 820, x: undefined, y: undefined, maximized: false };
    const win = new BrowserWindow({
      width: bounds.width,
      height: bounds.height,
      x: bounds.x,
      y: bounds.y,
      minWidth: 900,
      minHeight: 600,
      show: false,
      title: "Hocket",
      backgroundColor: "#121214",
      autoHideMenuBar: true,
      titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
      trafficLightPosition: process.platform === "darwin" ? { x: 14, y: 14 } : undefined,
      webPreferences: this.baseWebPreferences(),
    });
    this.main = win;
    if (bounds.maximized) win.maximize();
    win.once("ready-to-show", () => win.show());
    const persist = () => {
      if (win.isDestroyed()) return;
      const b = win.getNormalBounds();
      this.opts.store.set("mainBounds", { ...b, maximized: win.isMaximized() });
    };
    win.on("resize", persist);
    win.on("move", persist);
    win.on("maximize", persist);
    win.on("unmaximize", persist);
    const vis = () => {
      if (win.isDestroyed()) return;
      this.opts.onVisibility(win.isVisible() && !win.isMinimized(), win.isFocused());
      this.broadcastState();
    };
    for (const ev of ["show", "hide", "minimize", "restore", "focus", "blur", "maximize", "unmaximize", "enter-full-screen", "leave-full-screen", "always-on-top-changed"] as const) {
      win.on(ev as "show", vis);
    }
    win.on("close", (e) => {
      if (!this.quitting && this.opts.shouldHideOnClose()) {
        e.preventDefault();
        win.hide();
        return;
      }
      persist();
    });
    win.on("closed", () => {
      this.main = undefined;
    });
    win.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    win.webContents.on("will-navigate", (e) => e.preventDefault());
    this.load(win, "main");
    return win;
  }

  showMain(): void {
    const win = this.main && !this.main.isDestroyed() ? this.main : this.createMain();
    if (win.isMinimized()) win.restore();
    win.show();
    win.focus();
  }

  openMini(): BrowserWindow {
    if (this.mini && !this.mini.isDestroyed()) {
      this.mini.show();
      this.mini.focus();
      return this.mini;
    }
    const saved = this.opts.store.get("miniBounds");
    const win = new BrowserWindow({
      width: 380,
      height: 112,
      x: saved?.x,
      y: saved?.y,
      minWidth: 300,
      minHeight: 96,
      maxHeight: 160,
      frame: false,
      alwaysOnTop: true,
      skipTaskbar: true,
      resizable: true,
      fullscreenable: false,
      title: "Hocket mini player",
      backgroundColor: "#121214",
      webPreferences: this.baseWebPreferences(),
    });
    win.setAlwaysOnTop(true, "floating");
    win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    this.mini = win;
    win.on("move", () => {
      if (!win.isDestroyed()) {
        const [x, y] = win.getPosition();
        this.opts.store.set("miniBounds", { x: x ?? 0, y: y ?? 0 });
      }
    });
    win.on("closed", () => {
      this.mini = undefined;
    });
    win.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    this.load(win, "mini");
    return win;
  }

  closeMini(): void {
    if (this.mini && !this.mini.isDestroyed()) this.mini.close();
  }

  all(): BrowserWindow[] {
    return [this.main, this.mini].filter((w): w is BrowserWindow => !!w && !w.isDestroyed());
  }

  stateOf(win: BrowserWindow): WindowState {
    return { maximized: win.isMaximized(), fullscreen: win.isFullScreen(), focused: win.isFocused(), visible: win.isVisible() && !win.isMinimized(), alwaysOnTop: win.isAlwaysOnTop() };
  }

  broadcastState(): void {
    for (const w of this.all()) w.webContents.send(IPC.windowStateChanged, this.stateOf(w));
  }

  private boundsOnScreen(b: { x: number; y: number; width: number; height: number }): boolean {
    return screen.getAllDisplays().some((d) => {
      const a = d.workArea;
      return b.x + b.width > a.x + 50 && b.x < a.x + a.width - 50 && b.y + b.height > a.y + 50 && b.y < a.y + a.height - 50;
    });
  }

  destroyAll(): void {
    this.quitting = true;
    for (const w of [this.main, this.mini, this.anchor]) if (w && !w.isDestroyed()) w.destroy();
    app.quit();
  }
}
