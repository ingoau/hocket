import { afterEach, describe, expect, it, vi } from "vitest";
import type { CoreConfig } from "@core/api";
import { NativeCoreHandle, NativeCoreUnavailable, SHUTDOWN_TIMEOUT_MS, createCore, type NativeModule } from "./core-host";

const config: CoreConfig = { dataDir: "/nonexistent/x", cacheDir: "/nonexistent/y", deviceId: "d", deviceName: "n", platform: "linux", appVersion: "0", audio: "native", coordinatorListen: undefined };

function fakeModule(core: Partial<InstanceType<NativeModule["HocketCore"]>>): NativeModule {
  const HocketCore = function (this: unknown) {
    return Object.assign({ setListener: () => undefined, dispatch: () => undefined, query: async () => "{}" }, core);
  } as unknown as NativeModule["HocketCore"];
  return { HocketCore, MediaSession: class {} as unknown as NativeModule["MediaSession"], initLogging: () => undefined, coreVersion: () => "0", apiSchemaVersion: () => 1, mediaSessionAvailable: () => false };
}

describe("NativeCoreHandle.shutdown", () => {
  afterEach(() => vi.useRealTimers());

  it("resolves when the addon's flush resolves, and joins a second call", async () => {
    let done!: () => void;
    const shutdown = vi.fn(() => new Promise<void>((r) => (done = r)));
    const handle = new NativeCoreHandle(fakeModule({ shutdown }), config);
    const first = handle.shutdown();
    const second = handle.shutdown();
    expect(shutdown).toHaveBeenCalledTimes(1);
    let settled = false;
    void first.then(() => (settled = true));
    await Promise.resolve();
    expect(settled).toBe(false);
    done();
    await first;
    await second;
    expect(settled).toBe(true);
  });

  it("gives up after the timeout when the core never answers", async () => {
    vi.useFakeTimers();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const handle = new NativeCoreHandle(fakeModule({ shutdown: () => new Promise<void>(() => undefined) }), config);
    let settled = false;
    void handle.shutdown().then(() => (settled = true));
    await vi.advanceTimersByTimeAsync(SHUTDOWN_TIMEOUT_MS - 1);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(2);
    expect(settled).toBe(true);
  });

  it("still resolves when the addon throws or predates shutdown()", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const throwing = new NativeCoreHandle(fakeModule({ shutdown: () => { throw new Error("boom"); } }), config);
    await expect(throwing.shutdown()).resolves.toBeUndefined();
    const dispatch = vi.fn();
    const old = new NativeCoreHandle(fakeModule({ dispatch, shutdown: undefined }), config);
    await expect(old.shutdown()).resolves.toBeUndefined();
    expect(dispatch).toHaveBeenCalledWith(JSON.stringify({ type: "shutdown" }));
  });
});

describe("createCore", () => {
  it("is fatal without the addon when a native core is required (packaged builds)", () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    expect(() => createCore(config, { appRoot: "/nonexistent/app", forceFake: false, requireNative: true })).toThrow(NativeCoreUnavailable);
  });
});
