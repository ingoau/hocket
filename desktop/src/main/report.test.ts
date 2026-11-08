import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Command, Event, Query, QueryResult, ReportUploadStatus } from "@core/api";
import { IPC } from "@shared/bridge-types";

type Listener = (e: { sender: { id: number } }, ...args: unknown[]) => unknown;
const handlers = new Map<string, Listener>();

/** A stand-in for Electron's NativeImage: a size, and JPEG bytes that say what was encoded. */
class FakeImage {
  constructor(readonly width: number, readonly height: number) {}
  isEmpty() {
    return this.width === 0;
  }
  getSize() {
    return { width: this.width, height: this.height };
  }
  resize(o: { width?: number; height?: number }) {
    const scale = o.width !== undefined ? o.width / this.width : (o.height ?? this.height) / this.height;
    return new FakeImage(Math.round(this.width * scale), Math.round(this.height * scale));
  }
  toJPEG(quality: number) {
    return Buffer.from(`jpeg ${this.width}x${this.height} q${quality}`);
  }
}

const images = new Map<string, FakeImage>();
const dialogs = { showOpenDialog: vi.fn() };
/** Clipboard entries: MIME type to bytes. Empty means the clipboard holds nothing. */
type ClipItem = Record<string, string>;
const clipboardMock = {
  read: vi.fn(async (): Promise<{ types: string[]; getType(t: string): Promise<Blob> }[]> => []),
};
const clipboardHolds = (...items: ClipItem[]) =>
  clipboardMock.read.mockResolvedValueOnce(items.map((it) => ({ types: Object.keys(it), getType: async (t: string) => new Blob([it[t]!]) })));
const fsMock = {
  writeFile: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined),
  readFile: vi.fn<(path: string) => Promise<Buffer>>(async (path) => Buffer.from(path)),
  mkdir: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined),
  rm: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined),
  stat: vi.fn<(path: string) => Promise<{ size: number; isFile(): boolean }>>(async () => ({ size: 1000, isFile: () => true })),
};

vi.mock("electron", () => ({
  ipcMain: { handle: (ch: string, l: Listener) => handlers.set(ch, l) },
  BrowserWindow: { fromWebContents: (wc: { id: number }) => (wc.id === 1 ? { id: 1 } : undefined) },
  clipboard: clipboardMock,
  dialog: dialogs,
  // The fake "decodes" a buffer by its text: the path readFile was given.
  nativeImage: { createFromBuffer: (b: Buffer) => images.get(b.toString()) ?? new FakeImage(0, 0) },
}));
vi.mock("node:fs/promises", () => fsMock);

const { installReportIpc, SCREENSHOT_MAX_SIDE } = await import("./report");

const ours = { sender: { id: 1 } };
const stranger = { sender: { id: 99 } };
const DIR = "/data/report-staging";

function setup(upload?: ReportUploadStatus) {
  handlers.clear();
  const dispatch = vi.fn<(c: Command) => void>();
  const query = vi.fn<(q: Query) => Promise<QueryResult>>(async () => ({ type: "reportUpload", data: upload }));
  const build = vi.fn(async (all: boolean) => ({ text: all ? "bundle with others" : "bundle" }));
  const listeners = new Set<(e: Event) => void>();
  const onEvent = (l: (e: Event) => void) => {
    listeners.add(l);
    return () => listeners.delete(l);
  };
  installReportIpc({ core: { kind: "fake", dispatch, query, onEvent, shutdown: async () => undefined }, stagingDir: DIR, isOurs: (s) => s.id === 1, build });
  // The cleanup waits for the core to start.
  expect(query).not.toHaveBeenCalled();
  for (const l of [...listeners]) l({ type: "started", data: { snapshot: {} } } as Event);
  const call = (channel: string, ...args: unknown[]) => handlers.get(channel)!(ours, ...args);
  return { dispatch, query, build, call };
}

const uploading: ReportUploadStatus = { state: "uploading", sentBytes: 1, totalBytes: 10, reportId: undefined, error: undefined, errorDetail: undefined, startedAt: 1 };

describe("report ipc", () => {
  beforeEach(() => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    images.clear();
    dialogs.showOpenDialog.mockReset();
    for (const f of Object.values(fsMock)) f.mockClear();
  });

  it("refuses strangers on every channel", async () => {
    setup();
    for (const ch of [IPC.reportPickScreenshots, IPC.reportPasteScreenshot, IPC.reportStageImage, IPC.reportPickRecording, IPC.reportDiscard, IPC.reportSend]) {
      await expect(Promise.resolve().then(() => handlers.get(ch)!(stranger, 1))).rejects.toThrow("rejected");
    }
    expect(dialogs.showOpenDialog).not.toHaveBeenCalled();
  });

  it("clears stale staging files at startup only when no upload is pending", async () => {
    const a = setup();
    await a.call(IPC.reportPasteScreenshot); // waits for the cleanup
    expect(fsMock.rm).toHaveBeenCalledWith(DIR, { recursive: true, force: true });
    fsMock.rm.mockClear();
    const b = setup({ ...uploading, state: "failed", error: "network" });
    await b.call(IPC.reportPasteScreenshot);
    expect(fsMock.rm).not.toHaveBeenCalled();
  });

  it("stages picked screenshots resized to 2000 px as JPEG, as many as there is room for", async () => {
    const { call } = setup();
    images.set("/pics/big.png", new FakeImage(4000, 3000));
    images.set("/pics/tall.jpg", new FakeImage(1000, 5000));
    images.set("/pics/third.png", new FakeImage(100, 100));
    dialogs.showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ["/pics/big.png", "/pics/tall.jpg", "/pics/third.png"] });
    const staged = (await call(IPC.reportPickScreenshots, 2)) as { id: string; name: string; thumbnailDataUrl: string }[];
    expect(dialogs.showOpenDialog.mock.calls[0]![1]).toMatchObject({ properties: ["openFile", "multiSelections"], filters: [{ extensions: ["png", "jpg", "jpeg", "webp"] }] });
    expect(staged.map((s) => s.name)).toEqual(["big.png", "tall.jpg"]);
    expect(staged[0]!.thumbnailDataUrl).toMatch(/^data:image\/jpeg;base64,/);
    expect(Buffer.from(staged[0]!.thumbnailDataUrl.split(",")[1]!, "base64").toString()).toBe("jpeg 160x120 q80");
    const writes = fsMock.writeFile.mock.calls.map(([path, data]) => [path, String(data)]);
    expect(writes).toEqual([
      [`${DIR}/${staged[0]!.id}.jpg`, `jpeg ${SCREENSHOT_MAX_SIDE}x1500 q85`],
      [`${DIR}/${staged[1]!.id}.jpg`, `jpeg 400x${SCREENSHOT_MAX_SIDE} q85`],
    ]);
    // No room: no dialog at all.
    dialogs.showOpenDialog.mockClear();
    expect(await call(IPC.reportPickScreenshots, 0)).toEqual([]);
    expect(dialogs.showOpenDialog).not.toHaveBeenCalled();
  });

  it("hands a WebP back to the page to decode, and stages the PNG it returns", async () => {
    const { call } = setup();
    dialogs.showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ["/pics/shot.webp", "/pics/notes.txt.png"] });
    const picked = (await call(IPC.reportPickScreenshots, 5)) as { name: string; bytes: Uint8Array }[];
    expect(picked).toHaveLength(1);
    expect(picked[0]!.name).toBe("shot.webp");
    expect(Buffer.from(picked[0]!.bytes).toString()).toBe("/pics/shot.webp");
    images.set("decoded png", new FakeImage(800, 600));
    const staged = (await call(IPC.reportStageImage, new Uint8Array(Buffer.from("decoded png")), "shot.webp")) as { id: string };
    expect(staged.id).toMatch(/^[0-9a-f-]{36}$/);
    expect(await call(IPC.reportStageImage, new Uint8Array([1, 2, 3]), "junk")).toBeUndefined();
    await expect(Promise.resolve().then(() => call(IPC.reportStageImage, "not bytes", "x"))).rejects.toThrow("rejected");
  });

  it("pastes the clipboard's image, and nothing when it holds none", async () => {
    const { call } = setup();
    expect(await call(IPC.reportPasteScreenshot)).toBeUndefined();
    clipboardHolds({ "text/plain": "no picture here" });
    expect(await call(IPC.reportPasteScreenshot)).toBeUndefined();
    images.set("clipboard png", new FakeImage(2560, 1440));
    clipboardHolds({ "text/plain": "caption" }, { "text/html": "<img>", "image/png": "clipboard png" });
    const s = (await call(IPC.reportPasteScreenshot)) as { name: string };
    expect(s.name).toBe("Pasted image");
    expect(String(fsMock.writeFile.mock.calls[0]![1])).toBe("jpeg 2000x1125 q85");
  });

  it("discard deletes a staged screenshot's copy but never the user's recording", async () => {
    const { call } = setup();
    images.set("/pics/a.png", new FakeImage(10, 10));
    dialogs.showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ["/pics/a.png"] });
    const [shot] = (await call(IPC.reportPickScreenshots, 5)) as { id: string }[];
    dialogs.showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ["/videos/screen.mp4"] });
    const rec = (await call(IPC.reportPickRecording)) as { id: string; name: string; size: number };
    expect(rec).toMatchObject({ name: "screen.mp4", size: 1000 });
    fsMock.rm.mockClear();
    await call(IPC.reportDiscard, [shot!.id, rec.id, "../../etc/passwd"]);
    expect(fsMock.rm.mock.calls).toEqual([[`${DIR}/${shot!.id}.jpg`, { force: true }]]);
  });

  it("send builds the bundle and starts the upload with the staged files; once only", async () => {
    const { call, dispatch, build } = setup();
    images.set("/pics/a.png", new FakeImage(10, 10));
    dialogs.showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ["/pics/a.png"] });
    const [shot] = (await call(IPC.reportPickScreenshots, 5)) as { id: string }[];
    dialogs.showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ["/videos/screen.mp4"] });
    const rec = (await call(IPC.reportPickRecording)) as { id: string };
    expect(await call(IPC.reportSend, { note: "  Queue jumped  ", screenshots: [shot!.id, "00000000-0000-0000-0000-000000000000"], recording: rec.id, allDevices: true })).toBe(true);
    expect(build).toHaveBeenCalledWith(true);
    expect(dispatch).toHaveBeenCalledWith({
      type: "startReportUpload",
      data: {
        note: "Queue jumped",
        bundle: "bundle with others",
        attachments: [
          { path: `${DIR}/${shot!.id}.jpg`, kind: "screenshot", deleteAfter: true },
          { path: "/videos/screen.mp4", kind: "recording", deleteAfter: false },
        ],
      },
    });
    // The core owns them now: the same ids carry nothing, and a discard deletes nothing.
    fsMock.rm.mockClear();
    await call(IPC.reportDiscard, [shot!.id]);
    expect(fsMock.rm).not.toHaveBeenCalled();
    await call(IPC.reportSend, { note: "", screenshots: [shot!.id], allDevices: false });
    expect(dispatch.mock.calls[1]![0]).toEqual({ type: "startReportUpload", data: { note: "", bundle: "bundle", attachments: [] } });
    await expect(Promise.resolve().then(() => call(IPC.reportSend, { note: 3, screenshots: [] }))).rejects.toThrow("rejected");
  });

  it("send refuses while an upload is running and keeps the files for the page to discard", async () => {
    const { call, dispatch } = setup(uploading);
    images.set("/pics/a.png", new FakeImage(10, 10));
    dialogs.showOpenDialog.mockResolvedValueOnce({ canceled: false, filePaths: ["/pics/a.png"] });
    const [shot] = (await call(IPC.reportPickScreenshots, 5)) as { id: string }[];
    expect(await call(IPC.reportSend, { note: "x", screenshots: [shot!.id], allDevices: false })).toBe(false);
    expect(dispatch).not.toHaveBeenCalled();
    fsMock.rm.mockClear();
    await call(IPC.reportDiscard, [shot!.id]);
    expect(fsMock.rm).toHaveBeenCalledWith(`${DIR}/${shot!.id}.jpg`, { force: true });
  });
});
