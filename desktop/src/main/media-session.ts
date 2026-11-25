// Feeds the playwire addon (crates/hocket-node MediaSession) from
// Event.MediaSession and maps its callbacks to core commands.
//
// Per design.md "OS media session": position is published every tick (the
// addon diffs), artwork is a file path resolved through the image cache, and
// on Windows the session is anchored to a persistent hidden window's HWND.
import type { Command, Event, MediaSessionState } from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import { snapshotOf } from "@shared/core-handle";
import type { NativeModule } from "./core-host";

interface AddonMessage {
  kind: "command" | "raise" | "quit" | "openUri";
  command?: Command;
  uri?: string;
}

export interface MediaSessionHostOptions {
  core: CoreHandle;
  native: NativeModule | undefined;
  hwnd: number | undefined;
  onRaise(): void;
  onQuit(): void;
  onOpenUri(uri: string): void;
}

/**
 * Position to publish on a ticker tick. `takenAt` is session-clock time, so
 * local `nowMs` is shifted by the Connect clock offset first; a duration of 0
 * means "unknown" and never pins the position.
 */
export function tickPosition(state: MediaSessionState, nowMs: number, clockOffsetMs: number): number {
  const p = state.position;
  const elapsed = Math.max(0, nowMs + clockOffsetMs - p.takenAt);
  const pos = Math.max(0, Math.round(p.positionMs + elapsed * p.rate));
  const duration = state.metadata?.durationMs ?? 0;
  return duration > 0 ? Math.min(pos, duration) : pos;
}

export class MediaSessionHost {
  private session: InstanceType<NativeModule["MediaSession"]> | undefined;
  private ticker: NodeJS.Timeout | undefined;
  private unsubscribe: (() => void) | undefined;
  /** Session clock minus local clock, from ConnectionChanged (0 when local). */
  private clockOffsetMs = 0;
  readonly status: "attached" | "unavailable";

  constructor(private readonly opts: MediaSessionHostOptions) {
    this.status = this.attach() ? "attached" : "unavailable";
    this.unsubscribe = opts.core.onEvent((e) => this.onEvent(e));
  }

  private attach(): boolean {
    const mod = this.opts.native;
    if (!mod || !mod.mediaSessionAvailable?.()) {
      console.warn("[media-session] addon unavailable; OS media controls disabled");
      return false;
    }
    try {
      this.session = new mod.MediaSession(
        { name: "Hocket", desktopEntry: "hocket", hwnd: process.platform === "win32" ? this.opts.hwnd : undefined, trackIdPrefix: "/app/hocket/track" },
        (json) => this.onAddonMessage(json),
      );
      return true;
    } catch (err) {
      console.warn("[media-session] attach failed; continuing without OS media controls:", err);
      this.session = undefined;
      return false;
    }
  }

  private onAddonMessage(json: string): void {
    let msg: AddonMessage;
    try {
      msg = JSON.parse(json) as AddonMessage;
    } catch (err) {
      console.error("[media-session] bad callback payload", err);
      return;
    }
    switch (msg.kind) {
      case "command":
        if (msg.command) this.opts.core.dispatch(msg.command);
        break;
      case "raise":
        this.opts.onRaise();
        break;
      case "quit":
        this.opts.onQuit();
        break;
      case "openUri":
        if (msg.uri) this.opts.onOpenUri(msg.uri);
        break;
    }
  }

  private onEvent(e: Event): void {
    const snapshot = snapshotOf(e);
    if (snapshot) {
      this.clockOffsetMs = snapshot.connection.clockOffsetMs ?? 0;
      this.publish(snapshot.mediaSession);
    } else if (e.type === "mediaSession") this.publish(e.data.state);
    else if (e.type === "connectionChanged") this.clockOffsetMs = e.data.state.clockOffsetMs ?? 0;
  }

  /**
   * Published as it comes, artwork or not: while the core is still fetching the cover the state
   * has no `artworkPath`, and the core sends another one with it when the fetch lands. Waiting on a
   * fetch of our own would hold play/pause and position back (offline, until the HTTP timeout).
   */
  private publish(state: MediaSessionState): void {
    if (!this.session) return;
    try {
      this.session.setState(JSON.stringify(state));
    } catch (err) {
      console.warn("[media-session] publish failed", err);
    }
    this.updateTicker(state);
  }

  private updateTicker(state: MediaSessionState): void {
    if (this.ticker) {
      clearInterval(this.ticker);
      this.ticker = undefined;
    }
    if (!state.isPlaying || !state.metadata) return;
    this.ticker = setInterval(() => {
      try {
        this.session?.setPosition(tickPosition(state, Date.now(), this.clockOffsetMs));
      } catch (err) {
        console.warn("[media-session] tick failed", err);
      }
    }, 1000);
  }

  dispose(): void {
    if (this.ticker) clearInterval(this.ticker);
    this.unsubscribe?.();
    try {
      this.session?.detach();
    } catch {
      /* ignore */
    }
    this.session = undefined;
  }
}
