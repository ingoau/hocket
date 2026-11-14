import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { accountIdFor } from "./account-id";

vi.mock("electron", () => ({ safeStorage: undefined }));
const { ServerCredentialStore, encryptionUsable } = await import("./credentials");
type SafeStorageLike = import("./credentials").SafeStorageLike;

function fakeStorage(backend: string, available = true): SafeStorageLike {
  return {
    isAsyncEncryptionAvailable: async () => available,
    getSelectedStorageBackend: () => backend,
    encryptStringAsync: async (s) => Buffer.from(`enc:${s}`),
    decryptStringAsync: async (b) => {
      const t = b.toString();
      if (t.startsWith("old:")) return { result: t.slice(4), shouldReEncrypt: true };
      if (!t.startsWith("enc:")) throw new Error("bad blob");
      return { result: t.slice(4), shouldReEncrypt: false };
    },
  };
}

const open = (dir: string, backend = "gnome_libsecret", storage = fakeStorage(backend)) => ServerCredentialStore.open(dir, { storage, platform: "linux" });
const onDisk = (dir: string) => JSON.parse(readFileSync(join(dir, "credentials.json"), "utf8")) as { url: string; blob: string }[];
const write = (dir: string, entries: { url: string; username: string; blob: string }[]) => writeFileSync(join(dir, "credentials.json"), JSON.stringify(entries));
const b64 = (s: string) => Buffer.from(s).toString("base64");

describe("ServerCredentialStore", () => {
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "hocket-cred-"));
    vi.spyOn(console, "log").mockImplementation(() => undefined);
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });
  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
    vi.restoreAllMocks();
  });

  it("treats the Linux basic_text backend as no encryption at all", async () => {
    expect(await encryptionUsable(fakeStorage("basic_text"), "linux")).toBe(false);
    expect(await encryptionUsable(fakeStorage("gnome_libsecret"), "linux")).toBe(true);
    expect(await encryptionUsable(fakeStorage("kwallet6"), "linux")).toBe(true);
    expect(await encryptionUsable(fakeStorage("basic_text"), "darwin")).toBe(true);
    expect(await encryptionUsable(fakeStorage("basic_text"), "linux", true)).toBe(true);
    expect(await encryptionUsable(fakeStorage("gnome_libsecret", false), "linux")).toBe(false);
  });

  it("with basic_text the password is volatile: nothing on disk, nothing replayed, but the UI is told", async () => {
    const store = await open(dir, "basic_text");
    expect(store.storageKind).toBe("volatile");
    store.intercept({ type: "addAccount", data: { setup: { type: "navidrome", data: { url: "https://a.example/", username: "alice", password: "pw", name: undefined } } } });
    expect(existsSync(join(dir, "credentials.json"))).toBe(false);
    expect(store.replayCommands()).toEqual([]);
    expect(store.hasAny()).toBe(true);
    expect(store.passwordFor("https://a.example", "alice")).toBe("pw");
  });

  it("persists encrypted with a real backend and replays on the next start", async () => {
    const store = await open(dir);
    expect(store.storageKind).toBe("os");
    await store.remember("https://a.example", "alice", "pw", "Home");
    const raw = readFileSync(join(dir, "credentials.json"), "utf8");
    expect(raw).not.toContain("pw\"");
    expect(raw).toContain(Buffer.from("enc:pw").toString("base64"));
    const again = await open(dir);
    expect(again.replayCommands()).toEqual([{ type: "addAccount", data: { setup: { type: "navidrome", data: { url: "https://a.example", username: "alice", password: "pw", name: "Home" } } } }]);
    expect(again.passwordFor("https://a.example/", "alice")).toBe("pw");
  });

  it("falling back to volatile removes the stale persisted entry for that server", async () => {
    await (await open(dir)).remember("https://a.example", "alice", "old");
    const store = await open(dir, "basic_text");
    expect(store.replayCommands()).toHaveLength(1);
    await store.remember("https://a.example", "alice", "new");
    expect(store.replayCommands()).toEqual([]);
    expect(store.passwordFor("https://a.example", "alice")).toBe("new");
    expect(onDisk(dir)).toEqual([]);
  });

  it("a new password replaces the old one in memory at once and on disk once encrypted", async () => {
    const store = await open(dir);
    await store.remember("https://a.example", "alice", "old");
    const pending = store.remember("https://a.example", "alice", "new");
    expect(store.passwordFor("https://a.example", "alice")).toBe("new");
    expect(store.replayCommands()).toHaveLength(1);
    // The old blob is off disk before the new one lands.
    expect(onDisk(dir)).toEqual([]);
    await pending;
    expect(onDisk(dir).map((e) => e.blob)).toEqual([b64("enc:new")]);
  });

  it("forgetting a server while its password encrypts never writes it back", async () => {
    const store = await open(dir);
    const pending = store.remember("https://a.example", "alice", "pw");
    store.forget("https://a.example", "alice");
    await pending;
    expect(onDisk(dir)).toEqual([]);
    expect(store.hasAny()).toBe(false);
  });

  it("a password that can't be encrypted is kept for the session, and the stale blob still goes", async () => {
    const storage = fakeStorage("gnome_libsecret");
    const store = await open(dir, undefined, storage);
    await store.remember("https://a.example", "alice", "old");
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    storage.encryptStringAsync = async () => {
      throw new Error("keyring locked");
    };
    await store.remember("https://a.example", "alice", "new");
    expect(store.passwordFor("https://a.example", "alice")).toBe("new");
    expect(onDisk(dir)).toEqual([]);
  });

  it("re-encrypts blobs the OS asks for, and keeps ones it can't decrypt without replaying them", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    write(dir, [
      { url: "https://a.example", username: "alice", blob: b64("old:pw") },
      { url: "https://b.example", username: "bob", blob: b64("garbage") },
    ]);
    const store = await open(dir);
    expect(store.replayCommands()).toEqual([{ type: "addAccount", data: { setup: { type: "navidrome", data: { url: "https://a.example", username: "alice", password: "pw", name: undefined } } } }]);
    expect(onDisk(dir).map((e) => e.blob).sort()).toEqual([b64("enc:pw"), b64("garbage")].sort());
    expect(store.hasAny()).toBe(true);
    store.retainOnly([{ id: accountIdFor("https://a.example", "alice") }]);
    expect(onDisk(dir).map((e) => e.url)).toEqual(["https://a.example"]);
  });

  it("an empty AccountsChanged only prunes after a RemoveAccount", async () => {
    const store = await open(dir);
    await store.remember("https://a.example", "alice", "pw");
    // Start: the core's server table couldn't be read → ServersChanged([]) with Started.
    store.retainOnly([]);
    expect(store.replayCommands()).toHaveLength(1);
    // A non-empty list still prunes others.
    store.retainOnly([{ id: accountIdFor("https://a.example", "alice") }]);
    expect(store.replayCommands()).toHaveLength(1);
    store.intercept({ type: "removeAccount", data: { account_id: "s1" } });
    store.retainOnly([]);
    expect(store.replayCommands()).toEqual([]);
    expect(store.hasAny()).toBe(false);
  });
});
