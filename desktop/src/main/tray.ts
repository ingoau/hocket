// Tray icon with close-to-tray. The icon is generated at runtime (no binary
// assets), see brand.ts.
import { Menu, Tray, nativeImage, type NativeImage } from "electron";
import { appIconPng, waveGlyphPng } from "./brand";

export interface TrayHandlers {
  show(): void;
  togglePlay(): void;
  next(): void;
  previous(): void;
  quit(): void;
}

export class AppTray {
  private tray: Tray | undefined;
  private nowPlaying: string | undefined;
  private playing = false;

  constructor(private readonly handlers: TrayHandlers) {}

  install(): void {
    if (this.tray) return;
    this.tray = new Tray(trayIcon());
    this.tray.setToolTip("Hocket");
    this.tray.on("click", () => this.handlers.show());
    this.tray.on("double-click", () => this.handlers.show());
    this.rebuild();
  }

  update(nowPlaying: string | undefined, playing: boolean): void {
    this.nowPlaying = nowPlaying;
    this.playing = playing;
    this.rebuild();
  }

  private rebuild(): void {
    if (!this.tray) return;
    this.tray.setToolTip(this.nowPlaying ? `Hocket · ${this.nowPlaying}` : "Hocket");
    this.tray.setContextMenu(
      Menu.buildFromTemplate([
        { label: this.nowPlaying ?? "Nothing playing", enabled: false },
        { type: "separator" },
        { label: this.playing ? "Pause" : "Play", click: () => this.handlers.togglePlay(), enabled: !!this.nowPlaying },
        { label: "Next", click: () => this.handlers.next(), enabled: !!this.nowPlaying },
        { label: "Previous", click: () => this.handlers.previous(), enabled: !!this.nowPlaying },
        { type: "separator" },
        { label: "Show Hocket", click: () => this.handlers.show() },
        { label: "Quit", click: () => this.handlers.quit() },
      ]),
    );
  }

  destroy(): void {
    this.tray?.destroy();
    this.tray = undefined;
  }
}

/**
 * macOS: the bare wave as a template image, so the menu bar tints it for light, dark
 * and selected states. Elsewhere the taskbar colour is unknown, so the full-colour tile.
 */
function trayIcon(): NativeImage {
  if (process.platform !== "darwin") return nativeImage.createFromBuffer(appIconPng(32, 0.03));
  const img = nativeImage.createEmpty();
  for (const scaleFactor of [1, 2]) img.addRepresentation({ scaleFactor, buffer: waveGlyphPng(14, scaleFactor).png });
  img.setTemplateImage(true);
  return img;
}

export function appIcon(size = 256) {
  return nativeImage.createFromBuffer(appIconPng(size));
}
