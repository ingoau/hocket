import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("electron", () => ({ safeStorage: undefined }));
const { ServerCredentialStore, encryptionUsable } = await import("./credentials");
type SafeStorageLike = import("./credentials").SafeStorageLike;

function fakeStorage(backend: string, available = true): SafeStorageLike {
  return {
    isEncryptionAvailable: () => available,
    getSelectedStorageBackend: () => backend,
    encryptString: (s) => Buffer.from(`enc:${s}`),
    decryptString: (b) => {
      const t = b.toString();
      if (!t.startsWith("enc:")) throw new Error("bad blob");
      return t.slice(4);
    },
  };
}

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

  it("treats the Linux basic_text backend as no encryption at all", () => {
    expect(encryptionUsable(fakeStorage("basic_text"), "linux")).toBe(false);
    expect(encryptionUsable(fakeStorage("gnome_libsecret"), "linux")).toBe(true);
    expect(encryptionUsable(fakeStorage("kwallet6"), "linux")).toBe(true);
    expect(encryptionUsable(fakeStorage("basic_text"), "darwin")).toBe(true);
    expect(encryptionUsable(fakeStorage("basic_text"), "linux", true)).toBe(true);
    expect(encryptionUsable(fakeStorage("gnome_libsecret", false), "linux")).toBe(false);
  });

  it("with basic_text the password is volatile: nothing on disk, nothing replayed, but the UI is told", () => {
    const store = new ServerCredentialStore(dir, { storage: fakeStorage("basic_text"), platform: "linux" });
    expect(store.storageKind).toBe("volatile");
    store.intercept({ type: "addServer", data: { url: "https://a.example/", username: "alice", password: "pw", name: undefined } });
    expect(existsSync(join(dir, "credentials.json"))).toBe(false);
    expect(store.replayCommands()).toEqual([]);
    expect(store.hasAny()).toBe(true);
    expect(store.passwordFor("https://a.example", "alice")).toBe("pw");
  });

  it("persists encrypted with a real backend and replays on the next start", () => {
    const storage = fakeStorage("gnome_libsecret");
    const store = new ServerCredentialStore(dir, { storage, platform: "linux" });
    expect(store.storageKind).toBe("os");
    store.remember("https://a.example", "alice", "pw", "Home");
    const raw = readFileSync(join(dir, "credentials.json"), "utf8");
    expect(raw).not.toContain("pw\"");
    expect(raw).toContain(Buffer.from("enc:pw").toString("base64"));
    const again = new ServerCredentialStore(dir, { storage, platform: "linux" });
    expect(again.replayCommands()).toEqual([{ type: "addServer", data: { url: "https://a.example", username: "alice", password: "pw", name: "Home" } }]);
    expect(again.passwordFor("https://a.example/", "alice")).toBe("pw");
  });

  it("falling back to volatile removes the stale persisted entry for that server", () => {
    new ServerCredentialStore(dir, { storage: fakeStorage("gnome_libsecret"), platform: "linux" }).remember("https://a.example", "alice", "old");
    const store = new ServerCredentialStore(dir, { storage: fakeStorage("basic_text"), platform: "linux" });
    expect(store.replayCommands()).toHaveLength(1);
    store.remember("https://a.example", "alice", "new");
    expect(store.replayCommands()).toEqual([]);
    expect(store.passwordFor("https://a.example", "alice")).toBe("new");
    expect(JSON.parse(readFileSync(join(dir, "credentials.json"), "utf8"))).toEqual([]);
  });

  it("an empty ServersChanged only prunes after a RemoveServer", () => {
    const store = new ServerCredentialStore(dir, { storage: fakeStorage("gnome_libsecret"), platform: "linux" });
    store.remember("https://a.example", "alice", "pw");
    // Start: the core's server table couldn't be read → ServersChanged([]) with Started.
    store.retainOnly([]);
    expect(store.replayCommands()).toHaveLength(1);
    // A non-empty list still prunes others.
    store.retainOnly([{ url: "https://a.example", username: "alice" }]);
    expect(store.replayCommands()).toHaveLength(1);
    store.intercept({ type: "removeServer", data: { server_id: "s1" } });
    store.retainOnly([]);
    expect(store.replayCommands()).toEqual([]);
    expect(store.hasAny()).toBe(false);
  });
});
