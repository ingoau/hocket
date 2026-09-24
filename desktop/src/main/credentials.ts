// ServerCredentialStore: the core keeps the password in memory only (token+salt
// auth needs it), so the platform stores an OS-encrypted blob and replays
// AddServer on every start. See design.md "Server baseline → Auth".
//
// "OS-encrypted" has to mean it: on Linux, Electron's safeStorage reports
// encryption as available even when it fell back to the `basic_text` backend
// (no recognised keyring), which uses a fixed key. That backend is treated as
// unavailable here, so the password stays in memory for the session and the
// UI warns that it won't be remembered. HOCKET_INSECURE_CREDENTIAL_STORE=1
// opts back in (used by the e2e suite on a keyring-less display server).
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { safeStorage } from "electron";
import type { Command } from "@core/api";

interface StoredCredential {
  url: string;
  username: string;
  name?: string;
  /** base64 of safeStorage.encryptString(password). */
  blob: string;
}

export type CredentialStorage = "os" | "volatile";

/** The subset of `safeStorage` the store uses; injectable for tests. */
export interface SafeStorageLike {
  isEncryptionAvailable(): boolean;
  getSelectedStorageBackend?(): string;
  encryptString(plain: string): Buffer;
  decryptString(blob: Buffer): string;
}

export interface CredentialStoreOptions {
  storage?: SafeStorageLike;
  platform?: NodeJS.Platform;
  /** Accept the fixed-key `basic_text` backend on Linux anyway. */
  allowInsecure?: boolean;
}

/** Whether `storage` really encrypts with an OS-held key on this platform. */
export function encryptionUsable(storage: SafeStorageLike, platform: NodeJS.Platform, allowInsecure = false): boolean {
  if (!storage.isEncryptionAvailable()) return false;
  if (platform !== "linux" || allowInsecure) return true;
  const backend = storage.getSelectedStorageBackend?.();
  return backend !== "basic_text" && backend !== "unknown";
}

export class ServerCredentialStore {
  private entries: StoredCredential[] = [];
  /** Passwords entered this session when OS encryption isn't available. */
  private volatile = new Map<string, string>();
  private readonly path: string;
  private readonly storage: SafeStorageLike;
  private readonly usable: boolean;
  /** Set by a RemoveServer on its way to the core; the next ServersChanged may then prune. */
  private removalPending = false;

  constructor(userData: string, opts: CredentialStoreOptions = {}) {
    this.path = join(userData, "credentials.json");
    this.storage = opts.storage ?? safeStorage;
    const platform = opts.platform ?? process.platform;
    const allowInsecure = opts.allowInsecure ?? process.env.HOCKET_INSECURE_CREDENTIAL_STORE === "1";
    this.usable = encryptionUsable(this.storage, platform, allowInsecure);
    const backend = platform === "linux" ? this.storage.getSelectedStorageBackend?.() : undefined;
    console.log(`[credentials] OS encryption ${this.usable ? "usable" : "NOT usable"}${backend ? ` (backend: ${backend})` : ""}; passwords are ${this.usable ? "persisted encrypted" : "kept in memory for this session only"}`);
    try {
      if (existsSync(this.path)) this.entries = JSON.parse(readFileSync(this.path, "utf8")) as StoredCredential[];
    } catch (err) {
      console.warn("[credentials] unreadable store, ignoring", err);
    }
  }

  /** What the UI should tell the user about where passwords go. */
  get storageKind(): CredentialStorage {
    return this.usable ? "os" : "volatile";
  }

  static key(url: string, username: string): string {
    return `${url.replace(/\/+$/, "").toLowerCase()}|${username}`;
  }

  /** Observe commands on their way to the core. */
  intercept(command: Command): void {
    if (command.type === "addServer") this.remember(command.data.url, command.data.username, command.data.password, command.data.name);
    if (command.type === "removeServer") {
      // Server ids are core-assigned; we don't know the mapping, so the
      // removal is applied by retainOnly() once ServersChanged arrives.
      this.removalPending = true;
    }
  }

  remember(url: string, username: string, password: string, name?: string): void {
    const key = ServerCredentialStore.key(url, username);
    // Whatever was persisted for this server is stale now: never replay an
    // old password on the next start because the new one couldn't be stored.
    const before = this.entries.length;
    this.entries = this.entries.filter((e) => ServerCredentialStore.key(e.url, e.username) !== key);
    if (!this.usable) {
      console.warn("[credentials] OS encryption unavailable; password kept in memory for this session only");
      this.volatile.set(key, password);
      if (this.entries.length !== before) this.flush();
      return;
    }
    this.volatile.delete(key);
    const blob = this.storage.encryptString(password).toString("base64");
    this.entries.push({ url, username, name, blob });
    this.flush();
  }

  forget(url: string, username: string): void {
    const key = ServerCredentialStore.key(url, username);
    this.entries = this.entries.filter((e) => ServerCredentialStore.key(e.url, e.username) !== key);
    this.volatile.delete(key);
    this.flush();
  }

  /**
   * Keep only servers the core still knows about. An empty list is honoured
   * only after a RemoveServer: the core also emits an empty ServersChanged
   * with Started when its server table couldn't be read, and that must not
   * log the user out.
   */
  retainOnly(servers: { url: string; username: string }[]): void {
    if (!servers.length && !this.removalPending) return;
    this.removalPending = false;
    const keep = new Set(servers.map((s) => ServerCredentialStore.key(s.url, s.username)));
    const before = this.entries.length;
    this.entries = this.entries.filter((e) => keep.has(ServerCredentialStore.key(e.url, e.username)));
    for (const key of [...this.volatile.keys()]) if (!keep.has(key)) this.volatile.delete(key);
    if (this.entries.length !== before) this.flush();
  }

  /** The password known for a server this session (persisted or volatile). */
  passwordFor(url: string, username: string): string | undefined {
    const key = ServerCredentialStore.key(url, username);
    const volatile = this.volatile.get(key);
    if (volatile !== undefined) return volatile;
    const entry = this.entries.find((e) => ServerCredentialStore.key(e.url, e.username) === key);
    if (!entry) return undefined;
    try {
      return this.storage.decryptString(Buffer.from(entry.blob, "base64"));
    } catch {
      return undefined;
    }
  }

  /** AddServer commands to replay at startup. */
  replayCommands(): Command[] {
    const out: Command[] = [];
    for (const e of this.entries) {
      try {
        const password = this.storage.decryptString(Buffer.from(e.blob, "base64"));
        out.push({ type: "addServer", data: { url: e.url, username: e.username, password, name: e.name } });
      } catch (err) {
        console.error(`[credentials] couldn't decrypt credential for ${e.url}; it will need to be re-entered`, err);
      }
    }
    return out;
  }

  hasAny(): boolean {
    return this.entries.length > 0 || this.volatile.size > 0;
  }

  private flush(): void {
    try {
      mkdirSync(dirname(this.path), { recursive: true });
      const tmp = `${this.path}.tmp`;
      writeFileSync(tmp, JSON.stringify(this.entries), { mode: 0o600 });
      renameSync(tmp, this.path);
    } catch (err) {
      console.error("[credentials] write failed", err);
    }
  }
}
