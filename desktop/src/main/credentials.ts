// ServerCredentialStore: the core keeps the password in memory only (token+salt
// auth needs it), so the platform stores an OS-encrypted blob and replays
// AddServer on every start. See design.md "Server baseline → Auth".
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

export class ServerCredentialStore {
  private entries: StoredCredential[] = [];
  /** Passwords entered this session when OS encryption isn't available. */
  private volatile = new Map<string, string>();
  private readonly path: string;

  constructor(userData: string) {
    this.path = join(userData, "credentials.json");
    try {
      if (existsSync(this.path)) this.entries = JSON.parse(readFileSync(this.path, "utf8")) as StoredCredential[];
    } catch (err) {
      console.warn("[credentials] unreadable store, ignoring", err);
    }
  }

  static key(url: string, username: string): string {
    return `${url.replace(/\/+$/, "").toLowerCase()}|${username}`;
  }

  /** Observe commands on their way to the core. */
  intercept(command: Command): void {
    if (command.type === "addServer") this.remember(command.data.url, command.data.username, command.data.password, command.data.name);
    if (command.type === "removeServer") {
      // Server ids are core-assigned; we don't know the mapping, so a removal
      // is applied by the caller via forgetByServer() once ServersChanged
      // arrives. Nothing to do here.
    }
  }

  remember(url: string, username: string, password: string, name?: string): void {
    const key = ServerCredentialStore.key(url, username);
    if (!safeStorage.isEncryptionAvailable()) {
      console.warn("[credentials] OS encryption unavailable; password kept in memory for this session only");
      this.volatile.set(key, password);
      return;
    }
    const blob = safeStorage.encryptString(password).toString("base64");
    this.entries = this.entries.filter((e) => ServerCredentialStore.key(e.url, e.username) !== key);
    this.entries.push({ url, username, name, blob });
    this.flush();
  }

  forget(url: string, username: string): void {
    const key = ServerCredentialStore.key(url, username);
    this.entries = this.entries.filter((e) => ServerCredentialStore.key(e.url, e.username) !== key);
    this.volatile.delete(key);
    this.flush();
  }

  /** Keep only servers the core still knows about. */
  retainOnly(servers: { url: string; username: string }[]): void {
    const keep = new Set(servers.map((s) => ServerCredentialStore.key(s.url, s.username)));
    const before = this.entries.length;
    this.entries = this.entries.filter((e) => keep.has(ServerCredentialStore.key(e.url, e.username)));
    if (this.entries.length !== before) this.flush();
  }

  /** AddServer commands to replay at startup. */
  replayCommands(): Command[] {
    const out: Command[] = [];
    for (const e of this.entries) {
      try {
        const password = safeStorage.decryptString(Buffer.from(e.blob, "base64"));
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
