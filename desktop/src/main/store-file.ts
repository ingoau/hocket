// Tiny JSON file store for main-process-only state (window bounds, device id,
// close-to-tray). Everything user-facing lives in the core's settings.
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export interface MainState {
  deviceId?: string;
  mainBounds?: { x: number; y: number; width: number; height: number; maximized: boolean };
  miniBounds?: { x: number; y: number };
  closeToTray?: boolean;
  batterySaverAuto?: boolean;
}

export class StoreFile {
  private state: MainState = {};
  constructor(private readonly path: string) {
    try {
      if (existsSync(path)) this.state = JSON.parse(readFileSync(path, "utf8")) as MainState;
    } catch (err) {
      console.warn("[store] unreadable state file, starting fresh", err);
      this.state = {};
    }
  }

  get<K extends keyof MainState>(key: K): MainState[K] {
    return this.state[key];
  }

  set<K extends keyof MainState>(key: K, value: MainState[K]): void {
    this.state[key] = value;
    this.flush();
  }

  private flush(): void {
    try {
      mkdirSync(dirname(this.path), { recursive: true });
      const tmp = join(dirname(this.path), `.${Date.now()}.tmp`);
      writeFileSync(tmp, JSON.stringify(this.state, null, 2));
      renameSync(tmp, this.path);
    } catch (err) {
      console.warn("[store] write failed", err);
    }
  }
}
