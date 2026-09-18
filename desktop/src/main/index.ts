// Electron main process entry. Owns the one core, the windows, the tray, the
// media session and the OS signals the core needs (visibility, power,
// network). No business logic lives here: everything is forwarded to the core.
import { BrowserWindow, app, net, powerMonitor, protocol, session } from "electron";
import { randomUUID } from "node:crypto";
import { hostname } from "node:os";
import { join, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import type { Command, CoreConfig, Event } from "@core/api";
import { API_SCHEMA_VERSION, APP_SCHEME, ART_SCHEME, DEV_SERVER_URL } from "@shared/constants";
import { IPC } from "@shared/bridge-types";
import { createCore } from "./core-host";
import { ServerCredentialStore } from "./credentials";
import { deepLinkFromArgv, registerDeepLinks } from "./deep-link";
import { installIpc } from "./ipc";
import { MediaSessionHost } from "./media-session";
import { installAppMenu } from "./menu";
import { StoreFile } from "./store-file";
import { AppTray, appIcon } from "./tray";
import { installUpdater } from "./updater";
import { Windows } from "./windows";

// Chromium must not grab the media keys or publish its own media session:
// audio comes out of Rust and the OS session is playwire's (design.md).
app.commandLine.appendSwitch("disable-features", "HardwareMediaKeyHandling,MediaSessionService");

const isDev = !app.isPackaged && process.env.HOCKET_DEV === "1";
const appRoot = app.isPackaged ? app.getAppPath() : resolve(__dirname, "..", "..");
const forceFake = process.env.HOCKET_FAKE_CORE === "1";
const userData = process.env.HOCKET_USER_DATA ? resolve(process.env.HOCKET_USER_DATA) : app.getPath("userData");
if (process.env.HOCKET_USER_DATA) app.setPath("userData", userData);

protocol.registerSchemesAsPrivileged([
  { scheme: APP_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: false } },
  { scheme: ART_SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } },
]);

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  void main();
}

async function main(): Promise<void> {
  await app.whenReady();
  app.setName("Hocket");
  if (process.platform === "linux") app.setAppUserModelId("app.hocket.desktop");

  const store = new StoreFile(join(userData, "main-state.json"));
  let deviceId = store.get("deviceId");
  if (!deviceId) {
    deviceId = randomUUID().replace(/-/g, "");
    store.set("deviceId", deviceId);
  }
  const dataDir = join(userData, "core");
  const cacheDir = process.env.HOCKET_CACHE_DIR ?? join(app.getPath("cache"), "hocket", "stream-cache");
  const platform: CoreConfig["platform"] = process.platform === "darwin" ? "macOs" : process.platform === "win32" ? "windows" : "linux";
  const config: CoreConfig = {
    dataDir,
    cacheDir,
    deviceId,
    deviceName: process.env.HOCKET_DEVICE_NAME ?? hostname(),
    platform,
    appVersion: app.getVersion(),
    audio: "native",
    coordinatorListen: undefined,
  };

  const { core, native } = createCore(config, { appRoot, forceFake, fakeTimeScale: Number(process.env.HOCKET_FAKE_TIMESCALE ?? "1") || 1 });
  const credentials = new ServerCredentialStore(userData);

  const windows = new Windows({
    appRoot,
    isDev,
    store,
    shouldHideOnClose: () => store.get("closeToTray") ?? true,
    onVisibility: (visible, focused) => {
      rendererVisible = visible;
      pushVisibility(focused);
    },
  });

  let rendererVisible = true;
  let pageVisible = true;
  const pushVisibility = (focused: boolean) => {
    core.dispatch({ type: "setVisibility", data: { visible: rendererVisible && pageVisible, focused } });
  };

  installProtocols(appRoot, cacheDir, dataDir);
  installCsp(isDev);
  registerDeepLinks(isDev, appRoot);

  const tray = new AppTray({
    show: () => windows.showMain(),
    togglePlay: () => core.dispatch({ type: "togglePlay" }),
    next: () => core.dispatch({ type: "next" }),
    previous: () => core.dispatch({ type: "previous" }),
    quit: () => shutdown(),
  });
  tray.install();

  const mediaSession = new MediaSessionHost({
    core,
    native: native?.module,
    hwnd: windows.anchorHandle(),
    onRaise: () => windows.showMain(),
    onQuit: () => shutdown(),
    onOpenUri: (uri) => broadcastDeepLink(uri),
  });

  installIpc({
    core,
    windows,
    meta: () => ({
      version: app.getVersion(),
      platform,
      coreKind: core.kind,
      mediaSession: mediaSession.status,
      deviceName: config.deviceName,
      dataDir,
      cacheDir,
    }),
    onCommand: (command: Command) => {
      credentials.intercept(command);
      if (command.type === "setSetting" && command.data.key === "general.closeToTray") {
        try {
          store.set("closeToTray", JSON.parse(command.data.value) !== false);
        } catch {
          /* ignore */
        }
      }
      if (command.type === "setSetting" && command.data.key === "power.batterySaverAuto") {
        try {
          store.set("batterySaverAuto", JSON.parse(command.data.value) !== false);
          applyBattery();
        } catch {
          /* ignore */
        }
      }
    },
    onVisibilityReport: (visible) => {
      pageVisible = visible;
      pushVisibility(windows.main?.isFocused() ?? false);
    },
    onNetworkReport: (online) => pushNetwork(online),
  });

  // Fan events out to every renderer window; keep the tray in step.
  core.onEvent((event: Event) => {
    for (const w of windows.all()) w.webContents.send(IPC.event, event);
    if (event.type === "nowPlayingChanged") {
      const t = event.data.entry?.track;
      tray.update(t ? `${t.title} · ${t.artist ?? ""}` : undefined, lastPlaying);
    }
    if (event.type === "transportChanged") {
      lastPlaying = event.data.transport.position.isPlaying;
      tray.update(lastNowPlaying, lastPlaying);
    }
    if (event.type === "nowPlayingChanged") lastNowPlaying = event.data.entry ? `${event.data.entry.track.title} · ${event.data.entry.track.artist ?? ""}` : undefined;
    if (event.type === "started") {
      // Replay stored credentials: the core never persists passwords.
      for (const cmd of credentials.replayCommands()) core.dispatch(cmd);
      applySettingsFromSnapshot(event.data.snapshot.settings);
      applyBattery();
      pushNetwork(net.isOnline());
    }
    if (event.type === "serversChanged") credentials.retainOnly(event.data.servers);
    if (event.type === "settingChanged") applySettingsFromSnapshot([event.data.setting]);
  });
  let lastPlaying = false;
  let lastNowPlaying: string | undefined;

  const applySettingsFromSnapshot = (settings: { key: string; value: string }[]) => {
    for (const s of settings) {
      try {
        if (s.key === "general.closeToTray") store.set("closeToTray", JSON.parse(s.value) !== false);
        if (s.key === "power.batterySaverAuto") store.set("batterySaverAuto", JSON.parse(s.value) !== false);
      } catch {
        /* ignore */
      }
    }
  };

  // Power: battery saver engages automatically on battery when the setting is on.
  const applyBattery = () => {
    const auto = store.get("batterySaverAuto") ?? true;
    const onBattery = powerMonitor.isOnBatteryPower();
    core.dispatch({ type: "setBatterySaver", data: { enabled: auto && onBattery } });
  };
  powerMonitor.on("on-battery", applyBattery);
  powerMonitor.on("on-ac", applyBattery);
  powerMonitor.on("suspend", () => core.dispatch({ type: "setVisibility", data: { visible: false, focused: false } }));
  powerMonitor.on("resume", () => pushVisibility(windows.main?.isFocused() ?? false));

  // Network: the renderer reports online/offline transitions; main polls as a backstop.
  let lastOnline: boolean | undefined;
  const pushNetwork = (online: boolean) => {
    if (lastOnline === online) return;
    lastOnline = online;
    core.dispatch({ type: "setNetworkState", data: { state: { kind: online ? "unknown" : "offline", metered: false, networkId: undefined } } });
  };
  setInterval(() => pushNetwork(net.isOnline()), 30_000).unref();

  installAppMenu({ showMain: () => windows.showMain(), openSettings: () => sendUiAction("ui.settings"), quit: () => shutdown() });
  installUpdater({ onStatus: (s) => console.log(`[updater] ${s}`) });

  const sendUiAction = (id: string) => {
    windows.showMain();
    windows.main?.webContents.send(IPC.uiAction, id);
  };
  const broadcastDeepLink = (url: string) => {
    windows.showMain();
    for (const w of windows.all()) w.webContents.send(IPC.deepLink, url);
  };

  app.on("second-instance", (_e, argv) => {
    windows.showMain();
    const link = deepLinkFromArgv(argv);
    if (link) broadcastDeepLink(link);
  });
  app.on("open-url", (e, url) => {
    e.preventDefault();
    broadcastDeepLink(url);
  });
  app.on("activate", () => windows.showMain());
  app.on("window-all-closed", () => {
    // Close-to-tray keeps the process alive; explicit quit comes from the tray/menu.
    if (!(store.get("closeToTray") ?? true)) shutdown();
  });

  let shuttingDown = false;
  const shutdown = () => {
    if (shuttingDown) return;
    shuttingDown = true;
    windows.quitting = true;
    mediaSession.dispose();
    tray.destroy();
    void core.shutdown().finally(() => windows.destroyAll());
    setTimeout(() => app.exit(0), 3000).unref();
  };
  app.on("before-quit", () => {
    windows.quitting = true;
  });

  if (process.platform === "darwin") app.dock?.setIcon(appIcon(512));
  windows.ensureAnchor();
  windows.createMain();
  core.dispatch({ type: "start" });

  const initialLink = deepLinkFromArgv(process.argv);
  if (initialLink) windows.main?.webContents.once("did-finish-load", () => broadcastDeepLink(initialLink));

  console.log(`[main] Hocket ${app.getVersion()} · core=${core.kind} · schema=${API_SCHEMA_VERSION} · mediaSession=${mediaSession.status}`);
}

/** app:// serves the renderer bundle; hocket-art:// serves artwork files from the cache/data dirs only. */
function installProtocols(appRoot: string, cacheDir: string, dataDir: string): void {
  const rendererDir = join(appRoot, "out", "renderer");
  protocol.handle(APP_SCHEME, (req) => {
    const url = new URL(req.url);
    let path = decodeURIComponent(url.pathname);
    if (path === "/" || path === "") path = "/index.html";
    const file = resolve(rendererDir, `.${path}`);
    if (!file.startsWith(rendererDir + sep) && file !== rendererDir) return new Response("forbidden", { status: 403 });
    return net.fetch(pathToFileURL(file).toString());
  });
  const allowedRoots = [resolve(cacheDir), resolve(dataDir), resolve(app.getPath("cache")), resolve(app.getPath("userData"))];
  protocol.handle(ART_SCHEME, (req) => {
    const url = new URL(req.url);
    const encoded = url.pathname.replace(/^\/+/, "");
    const file = resolve(decodeURIComponent(encoded));
    if (!allowedRoots.some((root) => file === root || file.startsWith(root + sep))) return new Response("forbidden", { status: 403 });
    return net.fetch(pathToFileURL(file).toString(), { headers: { "Cache-Control": "max-age=3600" } });
  });
}

/** Strict CSP on every renderer response (dev server and app://). */
function installCsp(isDev: boolean): void {
  const csp = [
    `default-src 'self'`,
    `script-src 'self'${isDev ? " 'unsafe-inline'" : ""}`,
    `style-src 'self' 'unsafe-inline'`,
    `img-src 'self' ${ART_SCHEME}: data: blob:`,
    `font-src 'self' data:`,
    `connect-src 'self' ${ART_SCHEME}:${isDev ? ` ${DEV_SERVER_URL} ws://localhost:5178` : ""}`,
    `worker-src 'self' blob:`,
    `object-src 'none'`,
    `base-uri 'none'`,
    `form-action 'none'`,
  ].join("; ");
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
    const headers = { ...details.responseHeaders };
    if (details.resourceType === "mainFrame" || details.resourceType === "subFrame") {
      headers["Content-Security-Policy"] = [csp];
    }
    callback({ responseHeaders: headers });
  });
  session.defaultSession.setPermissionRequestHandler((_wc, _permission, cb) => cb(false));
  app.on("web-contents-created", (_e, contents) => {
    contents.on("will-attach-webview", (e) => e.preventDefault());
    contents.setWindowOpenHandler(() => ({ action: "deny" }));
  });
  void BrowserWindow;
}
