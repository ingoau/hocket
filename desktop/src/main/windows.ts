// Window management: the main window, the always-on-top mini player and a
// persistent hidden anchor window (Windows SMTC is tied to an HWND; the main
// window is destroyed on close-to-tray, this one never is).
import { BrowserWindow, app, nativeTheme, screen } from "electron";
import { join } from "node:path";
import { APP_SCHEME, DEV_SERVER_URL } from "@shared/constants";
import type { ThemeChoice, WindowKind, WindowState, WindowTheme } from "@shared/bridge-types";
import { IPC } from "@shared/bridge-types";
import type { StoreFile } from "./store-file";

/**
 * The window's ground (global.css --bg) and ink (--fg) per theme: the first paint before the
 * renderer has drawn, and Windows' caption buttons, which sit on the top bar.
 */
export const WINDOW_CHROME: Record<WindowTheme, { ground: string; ink: string }> = {
  dark: { ground: "#1b1730", ink: "#ece9ff" },
  light: { ground: "#eceaf4", ink: "#1b1730" },
};
/** Windows' caption buttons over the fullscreen player: white on a light dark wash, whatever the artwork. */
const PLAYER_OVERLAY = { color: "#00000033", symbolColor: "#ffffff" };
/** The top bar's height (m3-shell.css --topbar-h): Windows' caption buttons fill it. */
const TOPBAR_HEIGHT = 56;

/** How long after its renderer is gone a window is loaded again. */
const RELOAD_AFTER_CRASH_MS = 250;

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

  /** The fullscreen player is open over the main window. */
  private playerOpen = false;

  constructor(private readonly opts: WindowsOptions) {
    this.preload = join(opts.appRoot, "out", "preload", "index.cjs");
    // Following the system: its switch recolours the chrome too.
    nativeTheme.on("updated", () => {
      if ((this.opts.store.get("windowTheme") ?? "system") === "system") this.applyChrome();
    });
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
      // macOS rubber-band overscroll; ignored on other platforms.
      scrollBounce: true,
    };
  }

  private load(win: BrowserWindow, kind: WindowKind): void {
    const url = this.opts.isDev ? `${DEV_SERVER_URL}/index.html?window=${kind}` : `${APP_SCHEME}://renderer/index.html?window=${kind}`;
    // A load cut short (the window closed or reloaded meanwhile) rejects; that is not an error
    // for the main process to die of.
    win.loadURL(url).catch((err: unknown) => console.warn(`[main] the ${kind} window did not finish loading`, err));
  }

  /** Renderer crashes, per window kind, in the last minute. */
  private crashes = new Map<WindowKind, number[]>();

  /**
   * A renderer that crashes or is killed (out of memory, the OS) leaves its window blank: load
   * it again, and the store reattaches with RequestSnapshot. At most three times a minute, so a
   * page that crashes as it loads does not spin.
   */
  private recoverRenderer(win: BrowserWindow, kind: WindowKind): void {
    win.webContents.on("render-process-gone", (_e, details) => {
      if (details.reason === "clean-exit" || this.quitting || win.isDestroyed()) return;
      const now = Date.now();
      const recent = (this.crashes.get(kind) ?? []).filter((t) => now - t < 60_000);
      recent.push(now);
      this.crashes.set(kind, recent);
      if (recent.length > 3) {
        console.error(`[main] the ${kind} window's renderer is gone again (${details.reason}); not reloading it`);
        return;
      }
      console.error(`[main] the ${kind} window's renderer is gone (${details.reason}); reloading it`);
      // Not from inside the event: the gone renderer is still being torn down, and a load
      // started now can hang the window (and quitting) instead of starting a new one.
      setTimeout(() => {
        if (!this.quitting && !win.isDestroyed()) this.load(win, kind);
      }, RELOAD_AFTER_CRASH_MS);
    });
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
    // A new main window starts with no player over it.
    this.playerOpen = false;
    const chrome = WINDOW_CHROME[this.theme()];
    // One title bar, the app's own top bar: macOS keeps its inset traffic lights, Windows draws its
    // caption buttons over the top bar (snap layouts and all), Linux has no frame and our buttons.
    const frame =
      process.platform === "darwin" ? { titleBarStyle: "hiddenInset" as const, trafficLightPosition: { x: 14, y: 14 } }
      : process.platform === "win32" ? { titleBarStyle: "hidden" as const, titleBarOverlay: { color: chrome.ground, symbolColor: chrome.ink, height: TOPBAR_HEIGHT } }
      : { frame: false };
    const win = new BrowserWindow({
      width: bounds.width,
      height: bounds.height,
      x: bounds.x,
      y: bounds.y,
      minWidth: 900,
      minHeight: 600,
      show: false,
      title: "Hocket",
      backgroundColor: chrome.ground,
      autoHideMenuBar: true,
      ...frame,
      webPreferences: this.baseWebPreferences(),
    });
    this.main = win;
    if (bounds.maximized) win.maximize();
    win.once("ready-to-show", () => win.show());
    const persist = () => {
      if (win.isDestroyed()) return;
      const b = win.getNormalBounds();
      this.opts.store.setSoon("mainBounds", { ...b, maximized: win.isMaximized() });
    };
    win.on("resize", persist);
    win.on("move", persist);
    win.on("maximize", persist);
    win.on("unmaximize", persist);
    const vis = () => {
      if (win.isDestroyed()) return;
      const visible = win.isVisible() && !win.isMinimized();
      // A visible-but-unfocused window (second monitor, fullscreen behind a
      // floating window) must keep painting at the background rate. Chromium's
      // default throttle drops rAF/timers to ~1 Hz and the fluid background
      // strobes; we already stop both renderers when hidden.
      win.webContents.setBackgroundThrottling(!visible);
      this.opts.onVisibility(visible, win.isFocused());
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
      this.opts.store.flush();
    });
    win.on("closed", () => {
      this.main = undefined;
    });
    win.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    // will-navigate is denied for every WebContents in index.ts (web-contents-created).
    this.recoverRenderer(win, "main");
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
      backgroundColor: WINDOW_CHROME[this.theme()].ground,
      webPreferences: this.baseWebPreferences(),
    });
    win.setAlwaysOnTop(true, "floating");
    win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    this.mini = win;
    win.on("move", () => {
      if (!win.isDestroyed()) {
        const [x, y] = win.getPosition();
        this.opts.store.setSoon("miniBounds", { x: x ?? 0, y: y ?? 0 });
      }
    });
    win.on("closed", () => {
      this.mini = undefined;
    });
    win.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    this.recoverRenderer(win, "mini");
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

  /** The theme the window is drawn in: the setting the renderer last reported, the system's when it follows it. */
  private theme(): WindowTheme {
    const choice = this.opts.store.get("windowTheme") ?? "system";
    return choice === "system" ? (nativeTheme.shouldUseDarkColors ? "dark" : "light") : choice;
  }

  /** The theme setting changed (or was first read): the window's ground and caption buttons follow, now and next launch. */
  setTheme(choice: ThemeChoice): void {
    if (this.opts.store.get("windowTheme") !== choice) this.opts.store.set("windowTheme", choice);
    this.applyChrome();
  }

  /** The fullscreen player opened or closed: Windows' caption buttons sit over it while it is open. */
  setPlayerOpen(open: boolean): void {
    if (this.playerOpen === open) return;
    this.playerOpen = open;
    this.applyChrome();
  }

  private applyChrome(): void {
    const chrome = WINDOW_CHROME[this.theme()];
    for (const win of [this.main, this.mini]) if (win && !win.isDestroyed()) win.setBackgroundColor(chrome.ground);
    const win = this.main;
    if (process.platform !== "win32" || !win || win.isDestroyed()) return;
    win.setTitleBarOverlay({ ...(this.playerOpen ? PLAYER_OVERLAY : { color: chrome.ground, symbolColor: chrome.ink }), height: TOPBAR_HEIGHT });
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
    // A bounds write still held by setSoon (a move just before quitting).
    this.opts.store.flush();
    for (const w of [this.main, this.mini, this.anchor]) if (w && !w.isDestroyed()) w.destroy();
    app.quit();
  }
}
