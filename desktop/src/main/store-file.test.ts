import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { StoreFile } from "./store-file";

describe("StoreFile", () => {
  let dir: string;
  let path: string;
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "hocket-store-"));
    path = join(dir, "main-state.json");
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    rmSync(dir, { recursive: true, force: true });
  });
  const onDisk = () => JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>;
  const bounds = (x: number) => ({ x, y: 0, width: 1000, height: 700, maximized: false });

  it("writes a drag's worth of bounds once, with the last value, after the delay", () => {
    const store = new StoreFile(path);
    for (let x = 0; x < 60; x++) store.setSoon("mainBounds", bounds(x));
    expect(() => statSync(path)).toThrow();
    expect(store.get("mainBounds")?.x).toBe(59);
    vi.advanceTimersByTime(500);
    expect(onDisk().mainBounds).toEqual(bounds(59));
  });

  it("writes held values with the next set or flush, and set at once", () => {
    const store = new StoreFile(path);
    store.setSoon("mainBounds", bounds(5));
    store.set("closeToTray", false);
    expect(onDisk()).toEqual({ mainBounds: bounds(5), closeToTray: false });
    store.setSoon("miniBounds", { x: 1, y: 2 });
    store.flush();
    expect(onDisk().miniBounds).toEqual({ x: 1, y: 2 });
    // Nothing left to write: the timer was cleared with the flush.
    expect(vi.getTimerCount()).toBe(0);
    expect(new StoreFile(path).get("miniBounds")).toEqual({ x: 1, y: 2 });
  });
});
