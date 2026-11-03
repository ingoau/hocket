import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Command, QueueView } from "@core/api";
import { expectResult } from "@shared/core-handle";
import { FakeCore } from "./index";

let dir: string;
let core: FakeCore;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "fake-queue-"));
  process.env.HOCKET_FAKE_CORE_FRESH = "1";
  core = new FakeCore(
    { dataDir: join(dir, "data"), cacheDir: join(dir, "cache"), deviceId: "dev-test", deviceName: "Test", platform: "linux", appVersion: "0.0.0", audio: "native" } as never,
    { trackCount: 300 },
  );
  core.dispatch({ type: "addServer", data: { url: "https://music.test", username: "me", password: "x" } } as unknown as Command);
});

afterEach(async () => {
  await core.shutdown();
  rmSync(dir, { recursive: true, force: true });
});

const view = async (): Promise<QueueView> => expectResult(await core.query({ type: "queue" }), "queue");
const sid = () => "srv";

describe("FakeCore queue", () => {
  it("plays, queues into the group, steps and windows the view", async () => {
    expect((await view()).total).toBe(0);
    const lib = (core as unknown as { lib: { tracks: { id: string }[] } }).lib;
    const t = lib.tracks.slice(0, 250).map((x) => x.id);
    core.dispatch({ type: "playTracks", data: { server_id: sid(), track_ids: t, start_index: 120, label: "Mix", shuffle: false } });
    let v = await view();
    expect(v.total).toBe(250);
    expect(v.currentIndex).toBe(120);
    expect(v.windowStart).toBe(20);
    expect(v.entries.length).toBe(201);
    expect(v.current?.item.trackId).toBe(t[120]);

    core.dispatch({ type: "addToQueue", data: { server_id: sid(), track_ids: [lib.tracks[260]!.id] } });
    core.dispatch({ type: "playNext", data: { server_id: sid(), track_ids: [lib.tracks[261]!.id] } });
    v = await view();
    expect(v.groupCount).toBe(2);
    const after = v.entries.slice(v.currentIndex! - v.windowStart + 1, v.currentIndex! - v.windowStart + 3);
    expect(after.map((e) => [e.track.id, e.inGroup])).toEqual([[lib.tracks[261]!.id, true], [lib.tracks[260]!.id, true]]);

    core.dispatch({ type: "next" });
    core.dispatch({ type: "next" });
    core.dispatch({ type: "previousTrack" });
    core.dispatch({ type: "previousTrack" });
    v = await view();
    expect(v.current?.item.trackId).toBe(t[120]);
    expect(v.groupCount).toBe(2);
    expect(v.total).toBe(252);

    const range = expectResult(await core.query({ type: "queueRange", data: { start: 0, count: 10 } }), "queue");
    expect(range.windowStart).toBe(0);
    expect(range.entries.map((e) => e.track.id)).toEqual(t.slice(0, 10));
    expect(range.total).toBe(252);

    core.dispatch({ type: "clearQueueGroup" });
    v = await view();
    expect(v.groupCount).toBe(0);
    expect(v.total).toBe(250);
  });

  it("replacing the context auto-saves the outgoing list", async () => {
    const lib = (core as unknown as { lib: { tracks: { id: string }[] } }).lib;
    core.dispatch({ type: "playTracks", data: { server_id: sid(), track_ids: lib.tracks.slice(0, 5).map((x) => x.id), start_index: 2, label: "A", shuffle: false } });
    core.dispatch({ type: "playTracks", data: { server_id: sid(), track_ids: lib.tracks.slice(10, 15).map((x) => x.id), start_index: 0, label: "B", shuffle: false } });
    const saved = expectResult(await core.query({ type: "savedQueues" }), "savedQueues");
    expect(saved[0]?.label).toBe("A");
    expect(saved[0]?.history.length).toBe(2);
    expect(saved[0]?.upcoming.length).toBe(2);
    core.dispatch({ type: "restoreSavedQueue", data: { id: saved[0]!.id } });
    const v = await view();
    expect(v.current?.item.trackId).toBe(lib.tracks[2]!.id);
    expect(v.historyCount).toBe(2);
  });
});
