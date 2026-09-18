// Feeds the playwire addon (crates/hocket-node MediaSession) from
// Event.MediaSession and maps its callbacks to core commands.
//
// Per design.md "OS media session": position is published every tick (the
// addon diffs), artwork is a file path resolved through the image cache, and
// on Windows the session is anchored to a persistent hidden window's HWND.
import type { Command, Event, MediaSessionState } from "@core/api";
import type { CoreHandle } from "@shared/core-handle";
import { expectResult } from "@shared/core-handle";
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

export class MediaSessionHost {
  private session: InstanceType<NativeModule["MediaSession"]> | undefined;
  private last: MediaSessionState | undefined;
  private ticker: NodeJS.Timeout | undefined;
  private artworkCache = new Map<string, string | undefined>();
  private unsubscribe: (() => void) | undefined;
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
    if (e.type === "started") this.publish(e.data.snapshot.mediaSession);
    else if (e.type === "mediaSession") this.publish(e.data.state);
  }

  private publish(state: MediaSessionState): void {
    this.last = state;
    if (!this.session) return;
    void this.withArtwork(state).then((s) => {
      if (this.last !== state) return; // superseded
      try {
        this.session?.setState(JSON.stringify(s));
      } catch (err) {
        console.warn("[media-session] publish failed", err);
      }
      this.updateTicker(s);
    });
  }

  /** The core normally resolves artwork itself; fall back to Query.Artwork when it didn't. */
  private async withArtwork(state: MediaSessionState): Promise<MediaSessionState> {
    const meta = state.metadata;
    if (!meta || meta.artworkPath || !meta.trackId) return state;
    if (!this.artworkCache.has(meta.trackId)) {
      try {
        const track = expectResult(await this.opts.core.query({ type: "track", data: { id: meta.trackId } }), "trackDetail");
        const path = track?.coverArt ? expectResult(await this.opts.core.query({ type: "artwork", data: { id: track.coverArt, size: 300 } }), "path") : undefined;
        this.artworkCache.set(meta.trackId, path);
        if (this.artworkCache.size > 50) this.artworkCache.delete(this.artworkCache.keys().next().value as string);
      } catch {
        this.artworkCache.set(meta.trackId, undefined);
      }
    }
    return { ...state, metadata: { ...meta, artworkPath: this.artworkCache.get(meta.trackId) } };
  }

  private updateTicker(state: MediaSessionState): void {
    if (this.ticker) {
      clearInterval(this.ticker);
      this.ticker = undefined;
    }
    if (!state.isPlaying || !state.metadata) return;
    this.ticker = setInterval(() => {
      const p = state.position;
      const pos = Math.max(0, Math.round(p.positionMs + (Date.now() - p.takenAt) * p.rate));
      try {
        this.session?.setPosition(Math.min(pos, state.metadata?.durationMs ?? pos));
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
