// Tray icon with close-to-tray. The icon is generated at runtime (no binary
// assets), see png.ts.
import { Menu, Tray, nativeImage } from "electron";
import { appIconPng } from "./png";

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
    const size = process.platform === "darwin" ? 22 : 32;
    const img = nativeImage.createFromBuffer(appIconPng(size));
    if (process.platform === "darwin") img.setTemplateImage(false);
    this.tray = new Tray(img);
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

export function appIcon(size = 256) {
  return nativeImage.createFromBuffer(appIconPng(size));
}
