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
import type { CredentialStorage } from "@shared/bridge-types";

interface StoredCredential {
  url: string;
  username: string;
  name?: string;
  /** base64 of safeStorage.encryptStringAsync(password). */
  blob: string;
}

/** A credential this session knows, decrypted. `blob` is unset until its encryption lands. */
interface KnownCredential {
  url: string;
  username: string;
  name?: string;
  password: string;
  blob?: string;
}

export type { CredentialStorage };

/** The subset of `safeStorage` the store uses; injectable for tests. */
export interface SafeStorageLike {
  isAsyncEncryptionAvailable(): Promise<boolean>;
  getSelectedStorageBackend?(): string;
  encryptStringAsync(plain: string): Promise<Buffer>;
  decryptStringAsync(blob: Buffer): Promise<{ result: string; shouldReEncrypt: boolean }>;
}

export interface CredentialStoreOptions {
  storage?: SafeStorageLike;
  platform?: NodeJS.Platform;
  /** Accept the fixed-key `basic_text` backend on Linux anyway. */
  allowInsecure?: boolean;
}

/** Whether `storage` really encrypts with an OS-held key on this platform. */
export async function encryptionUsable(storage: SafeStorageLike, platform: NodeJS.Platform, allowInsecure = false): Promise<boolean> {
  try {
    if (!(await storage.isAsyncEncryptionAvailable())) return false;
  } catch (err) {
    console.error("[credentials] couldn't reach the OS keystore", err);
    return false;
  }
  if (platform !== "linux" || allowInsecure) return true;
  const backend = storage.getSelectedStorageBackend?.();
  return backend !== "basic_text" && backend !== "unknown";
}

/**
 * Passwords are decrypted once, when the store opens, and kept in memory: the
 * core needs them anyway, and lookups (replay, config secrets) stay synchronous.
 * Encryption is asynchronous, so a new password reaches disk a moment after
 * `remember`; the stale blob for that server is gone from memory at once and
 * from disk by then.
 */
export class ServerCredentialStore {
  /** Persisted credentials, and those on their way to disk. Insertion order is replay order. */
  private known = new Map<string, KnownCredential>();
  /** Stored credentials that wouldn't decrypt (a locked keyring, say): kept on disk, never replayed. */
  private undecryptable = new Map<string, StoredCredential>();
  /** Passwords entered this session when OS encryption isn't available. */
  private volatile = new Map<string, string>();
  /** Set by a RemoveServer on its way to the core; the next ServersChanged may then prune. */
  private removalPending = false;

  private constructor(
    private readonly path: string,
    private readonly storage: SafeStorageLike,
    private readonly usable: boolean,
  ) {}

  static async open(userData: string, opts: CredentialStoreOptions = {}): Promise<ServerCredentialStore> {
    const storage = opts.storage ?? safeStorage;
    const platform = opts.platform ?? process.platform;
    const allowInsecure = opts.allowInsecure ?? process.env.HOCKET_INSECURE_CREDENTIAL_STORE === "1";
    const usable = await encryptionUsable(storage, platform, allowInsecure);
    const backend = platform === "linux" ? storage.getSelectedStorageBackend?.() : undefined;
    console.log(`[credentials] OS encryption ${usable ? "usable" : "NOT usable"}${backend ? ` (backend: ${backend})` : ""}; passwords are ${usable ? "persisted encrypted" : "kept in memory for this session only"}`);
    const store = new ServerCredentialStore(join(userData, "credentials.json"), storage, usable);
    await store.load();
    return store;
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
    if (command.type === "addServer") void this.remember(command.data.url, command.data.username, command.data.password, command.data.name);
    if (command.type === "removeServer") {
      // Server ids are core-assigned; we don't know the mapping, so the
      // removal is applied by retainOnly() once ServersChanged arrives.
      this.removalPending = true;
    }
  }

  /** Resolves once the password is on disk, or known not to be going there. Never rejects. */
  async remember(url: string, username: string, password: string, name?: string): Promise<void> {
    const key = ServerCredentialStore.key(url, username);
    // Whatever was persisted for this server is stale now: never replay an
    // old password on the next start because the new one couldn't be stored.
    const hadBlob = this.known.get(key)?.blob !== undefined || this.undecryptable.has(key);
    this.known.delete(key);
    this.undecryptable.delete(key);
    if (!this.usable) {
      console.warn("[credentials] OS encryption unavailable; password kept in memory for this session only");
      this.volatile.set(key, password);
      if (hadBlob) this.flush();
      return;
    }
    this.volatile.delete(key);
    const entry: KnownCredential = { url, username, name, password };
    this.known.set(key, entry);
    // Off disk now, not once the new blob lands: a quit in between must not replay the old password.
    if (hadBlob) this.flush();
    try {
      entry.blob = (await this.storage.encryptStringAsync(password)).toString("base64");
    } catch (err) {
      console.error(`[credentials] couldn't encrypt the password for ${url}; it is kept for this session only`, err);
    }
    // Forgotten, or replaced by a newer password, while encrypting: that change already flushed.
    if (this.known.get(key) === entry) this.flush();
  }

  forget(url: string, username: string): void {
    const key = ServerCredentialStore.key(url, username);
    this.known.delete(key);
    this.undecryptable.delete(key);
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
    let dropped = false;
    for (const map of [this.known, this.undecryptable])
      for (const key of [...map.keys()])
        if (!keep.has(key)) {
          map.delete(key);
          dropped = true;
        }
    for (const key of [...this.volatile.keys()]) if (!keep.has(key)) this.volatile.delete(key);
    if (dropped) this.flush();
  }

  /** The password known for a server this session (persisted or volatile). */
  passwordFor(url: string, username: string): string | undefined {
    const key = ServerCredentialStore.key(url, username);
    return this.volatile.get(key) ?? this.known.get(key)?.password;
  }

  /** AddServer commands to replay at startup. */
  replayCommands(): Command[] {
    return [...this.known.values()].map((e) => ({ type: "addServer", data: { url: e.url, username: e.username, password: e.password, name: e.name } }));
  }

  hasAny(): boolean {
    return this.known.size > 0 || this.undecryptable.size > 0 || this.volatile.size > 0;
  }

  private async load(): Promise<void> {
    let stored: StoredCredential[] = [];
    try {
      if (existsSync(this.path)) stored = JSON.parse(readFileSync(this.path, "utf8")) as StoredCredential[];
    } catch (err) {
      console.warn("[credentials] unreadable store, ignoring", err);
    }
    let reEncrypted = false;
    const decrypted = await Promise.all(
      stored.map(async (e): Promise<KnownCredential | StoredCredential> => {
        let decryption: { result: string; shouldReEncrypt: boolean };
        try {
          decryption = await this.storage.decryptStringAsync(Buffer.from(e.blob, "base64"));
        } catch (err) {
          console.error(`[credentials] couldn't decrypt credential for ${e.url}; it will need to be re-entered`, err);
          return e;
        }
        const known: KnownCredential = { url: e.url, username: e.username, name: e.name, password: decryption.result, blob: e.blob };
        // The OS rotated the key, or offers a better one: store the blob it would write now.
        if (decryption.shouldReEncrypt && this.usable) {
          try {
            known.blob = (await this.storage.encryptStringAsync(known.password)).toString("base64");
            reEncrypted = true;
          } catch (err) {
            console.warn(`[credentials] couldn't re-encrypt the credential for ${e.url}; keeping the old blob`, err);
          }
        }
        return known;
      }),
    );
    for (const e of decrypted) {
      const key = ServerCredentialStore.key(e.url, e.username);
      if ("password" in e) this.known.set(key, e);
      else this.undecryptable.set(key, e);
    }
    if (reEncrypted) this.flush();
  }

  /** Writes the credentials whose encryption has landed, and those that wouldn't decrypt. */
  private flush(): void {
    const entries: StoredCredential[] = [...this.undecryptable.values()];
    for (const e of this.known.values()) if (e.blob !== undefined) entries.push({ url: e.url, username: e.username, name: e.name, blob: e.blob });
    try {
      mkdirSync(dirname(this.path), { recursive: true });
      const tmp = `${this.path}.tmp`;
      writeFileSync(tmp, JSON.stringify(entries), { mode: 0o600 });
      renameSync(tmp, this.path);
    } catch (err) {
      console.error("[credentials] write failed", err);
    }
  }
}
