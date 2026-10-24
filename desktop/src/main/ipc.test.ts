import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Command, Event, Query, QueryResult } from "@core/api";
import { IPC } from "@shared/bridge-types";

type Listener = (e: { sender: { id: number } }, ...args: unknown[]) => unknown;
const handlers = new Map<string, Listener>();
const listeners = new Map<string, Listener>();
const dialogs = { showSaveDialog: vi.fn(), showOpenDialog: vi.fn(), showMessageBox: vi.fn() };
const fsMock = {
  writeFile: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined),
  readFile: vi.fn<(...args: unknown[]) => Promise<string>>(async () => ""),
  mkdir: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined),
  readdir: vi.fn<(...args: unknown[]) => Promise<string[]>>(async () => []),
  rm: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined),
};
const shellMock = { showItemInFolder: vi.fn(), openExternal: vi.fn() };
const clipboardMock = { writeText: vi.fn() };

vi.mock("electron", () => ({
  ipcMain: {
    on: (ch: string, l: Listener) => listeners.set(ch, l),
    handle: (ch: string, l: Listener) => handlers.set(ch, l),
  },
  BrowserWindow: { fromWebContents: (wc: { id: number }) => (wc.id === 1 ? { id: 1, setAlwaysOnTop: vi.fn(), setFullScreen: vi.fn() } : undefined) },
  clipboard: clipboardMock,
  shell: shellMock,
  dialog: dialogs,
  ShareMenu: class {
    popup() {}
  },
}));
vi.mock("node:fs/promises", () => fsMock);

const { installIpc } = await import("./ipc");

const ours = { sender: { id: 1 } };
const stranger = { sender: { id: 99 } };

function setup(coreOverrides: Partial<{ dispatch: (c: Command) => void; query: (q: Query) => Promise<QueryResult> }> = {}) {
  handlers.clear();
  listeners.clear();
  const dispatch = vi.fn<(c: Command) => void>(coreOverrides.dispatch ?? (() => undefined));
  const query = vi.fn<(q: Query) => Promise<QueryResult>>(coreOverrides.query ?? (async () => ({ type: "unit" }) as unknown as QueryResult));
  const onCommand = vi.fn();
  // A method that needs `this`, like ServerCredentialStore.passwordFor: an unbound call must fail.
  const store = { secret: "secret", passwordFor: vi.fn(function (this: { secret: string }, _url: string, _username: string): string | undefined { return this.secret; }) };
  const passwordFor = store.passwordFor;
  const devlog = {
    bundle: vi.fn(({ coreDiagnostics, otherDevices }: { coreDiagnostics: string; otherDevices?: string }) => {
      const log = Array.from({ length: 450 }, (_, i) => `line ${i}`).join("\n") + "\n";
      const head = `Hocket diagnostics\n${coreDiagnostics}\n`;
      return { fileName: "hocket-diagnostics-20261020-030405.txt", text: head + log + (otherDevices ?? ""), head, log, logLines: 450, logBytes: log.length, windowMs: 3_600_000, generatedAt: 1 };
    }),
    append: vi.fn(),
  };
  const eventListeners: ((e: Event) => void)[] = [];
  const emit = (e: Event) => eventListeners.forEach((l) => l(e));
  const artwork = { register: vi.fn(async (p: string) => (p.startsWith("/cache/") ? "a".repeat(32) : undefined)), resolve: vi.fn() };
  installIpc({
    core: { kind: "fake", dispatch, query, onEvent: (l) => { eventListeners.push(l); return () => undefined; }, shutdown: async () => undefined },
    windows: { all: () => [{ webContents: { id: 1 } }], mini: undefined, main: undefined, stateOf: () => ({}), broadcastState: vi.fn(), openMini: vi.fn(), closeMini: vi.fn(), showMain: vi.fn() } as never,
    artwork: artwork as never,
    meta: () => ({}) as never,
    onCommand,
    onVisibilityReport: vi.fn(),
    onNetworkReport: vi.fn(),
    prefs: { get: () => ({ closeToTray: true }), set: (p) => ({ closeToTray: p.closeToTray ?? true }) },
    credentials: store,
    devlog: devlog as never,
    diagnosticsDir: "/data/logs/diagnostics",
    diagnosticsExtra: () => ({ core: "fake core" }),
  });
  return { dispatch, query, onCommand, artwork, passwordFor, emit };
}

describe("ipc", () => {
  beforeEach(() => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    dialogs.showSaveDialog.mockReset();
    dialogs.showOpenDialog.mockReset();
    dialogs.showMessageBox.mockReset();
    fsMock.writeFile.mockClear();
    fsMock.readFile.mockClear();
  });

  it("does not expose file read/write channels", () => {
    setup();
    for (const ch of [...handlers.keys(), ...listeners.keys()]) expect(ch).not.toMatch(/file:(read|write)/);
    expect(handlers.has("hocket:file:read")).toBe(false);
    expect(handlers.has("hocket:file:write")).toBe(false);
  });

  it("drops malformed commands and swallows a rejecting core or observer instead of throwing out of the listener", () => {
    const { dispatch, onCommand } = setup({ dispatch: () => { throw new Error("serde"); } });
    const on = listeners.get(IPC.dispatch)!;
    expect(() => on(ours, { type: "addServer", data: "x" })).not.toThrow();
    expect(() => on(ours, { type: "addServer", data: [] })).not.toThrow();
    expect(() => on(ours, { type: 5 })).not.toThrow();
    expect(() => on(ours, "togglePlay")).not.toThrow();
    expect(() => on(ours, null)).not.toThrow();
    expect(dispatch).not.toHaveBeenCalled();
    // A well-formed envelope reaches the core; the core's throw stays inside the listener.
    expect(() => on(ours, { type: "togglePlay" })).not.toThrow();
    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(onCommand).toHaveBeenCalledTimes(1);
    // A data-less addServer: the credential observer dereferences data and throws; the core rejects it. Neither escapes.
    onCommand.mockImplementationOnce((c: Command) => { void (c as { data: { url: string } }).data.url; });
    expect(() => on(ours, { type: "addServer" })).not.toThrow();
    expect(dispatch).toHaveBeenCalledTimes(2);
    on(stranger, { type: "togglePlay" });
    expect(dispatch).toHaveBeenCalledTimes(2);
  });

  it("lets exportNsp write only to a path a save dialog just chose, once", async () => {
    const { dispatch } = setup();
    const on = listeners.get(IPC.dispatch)!;
    const filter = { id: "f" } as unknown as Command;
    on(ours, { type: "exportNsp", data: { filter, path: "/home/u/.bashrc" } });
    expect(dispatch).not.toHaveBeenCalled();
    dialogs.showSaveDialog.mockResolvedValue({ canceled: false, filePath: "/home/u/out.nsp" });
    expect(await handlers.get(IPC.dialogSave)!(ours, { title: "Export" })).toBe("/home/u/out.nsp");
    on(ours, { type: "exportNsp", data: { filter, path: "/home/u/out.nsp" } });
    expect(dispatch).toHaveBeenCalledTimes(1);
    on(ours, { type: "exportNsp", data: { filter, path: "/home/u/out.nsp" } });
    expect(dispatch).toHaveBeenCalledTimes(1);
    // Without a path the core just returns the document: always fine.
    on(ours, { type: "exportNsp", data: { filter } });
    expect(dispatch).toHaveBeenCalledTimes(2);
  });

  it("writes and reads files only inside the dialog handlers, never for a renderer-named path", async () => {
    setup();
    dialogs.showSaveDialog.mockResolvedValue({ canceled: false, filePath: "/home/u/x.json" });
    expect(await handlers.get(IPC.dialogSaveText)!(ours, { title: "Save", text: "hello" })).toBe("/home/u/x.json");
    expect(fsMock.writeFile).toHaveBeenCalledWith("/home/u/x.json", "hello", "utf8");
    dialogs.showSaveDialog.mockResolvedValue({ canceled: true, filePath: "" });
    expect(await handlers.get(IPC.dialogSaveText)!(ours, { title: "Save", text: "hello" })).toBeUndefined();
    expect(fsMock.writeFile).toHaveBeenCalledTimes(1);
    await expect(handlers.get(IPC.dialogSaveText)!(ours, { title: "Save", text: 5 })).rejects.toThrow("rejected");
    await expect(handlers.get(IPC.dialogSaveText)!(ours, { title: "Save", text: "x", defaultPath: "../../etc/passwd" })).resolves.toBeUndefined();
    expect(dialogs.showSaveDialog).toHaveBeenLastCalledWith(expect.anything(), expect.objectContaining({ defaultPath: undefined }));
    dialogs.showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ["/home/u/in.json"] });
    fsMock.readFile.mockResolvedValue("content");
    expect(await handlers.get(IPC.dialogOpenText)!(ours, { title: "Open" })).toEqual({ path: "/home/u/in.json", text: "content" });
    await expect(handlers.get(IPC.dialogOpenText)!(stranger, { title: "Open" })).rejects.toThrow("rejected");
  });

  it("answers artwork queries with a token, never a path", async () => {
    const { artwork } = setup({ query: async () => ({ type: "path", data: "/cache/images/s/a-300.png" }) });
    const q = handlers.get(IPC.query)!;
    expect(await q(ours, { type: "artwork", data: { id: "a", size: 300 } })).toEqual({ type: "path", data: "a".repeat(32) });
    expect(artwork.register).toHaveBeenCalledWith("/cache/images/s/a-300.png");
    await expect(q(ours, { type: "artwork" , data: [] })).rejects.toThrow("rejected");
    await expect(q(stranger, { type: "artwork", data: { id: "a", size: 300 } })).rejects.toThrow("rejected");
  });

  it("refuses artwork the core answered with a path outside the cache", async () => {
    setup({ query: async () => ({ type: "path", data: "/home/u/.config/Hocket/credentials.json" }) });
    expect(await handlers.get(IPC.query)!(ours, { type: "artwork", data: { id: "a", size: 300 } })).toEqual({ type: "path", data: undefined });
  });

  it("coerces window-control payloads", () => {
    setup();
    const on = listeners.get(IPC.windowControl)!;
    expect(() => on(ours, { fullscreen: "x" })).not.toThrow();
    expect(() => on(ours, { alwaysOnTop: 1 })).not.toThrow();
    expect(() => on(ours, "nope")).not.toThrow();
    expect(() => on(ours, 42)).not.toThrow();
  });

  it("config export resolves keystore references after a warning and writes with a dialog-chosen path", async () => {
    const { passwordFor } = setup();
    const doc = { version: 1, servers: [{ id: "s1", url: "https://a.example", username: "alice", name: "A" }], secrets: { "server:s1:password": "keystore://hocket/server/s1/password" } };
    dialogs.showMessageBox.mockResolvedValue({ response: 1 });
    expect(await handlers.get(IPC.configExport)!(ours, JSON.stringify(doc))).toBeUndefined();
    expect(fsMock.writeFile).not.toHaveBeenCalled();
    dialogs.showMessageBox.mockResolvedValue({ response: 0 });
    dialogs.showSaveDialog.mockResolvedValue({ canceled: false, filePath: "/home/u/cfg.json" });
    expect(await handlers.get(IPC.configExport)!(ours, JSON.stringify(doc))).toBe("/home/u/cfg.json");
    expect(passwordFor).toHaveBeenCalledWith("https://a.example", "alice");
    const written = JSON.parse(fsMock.writeFile.mock.calls[0]![1] as string) as typeof doc;
    expect(written.secrets).toEqual({ "server:s1:password": "secret" });
    expect(fsMock.writeFile.mock.calls[0]![2]).toEqual({ encoding: "utf8", mode: 0o600 });
    // No secrets: no warning dialog at all.
    dialogs.showMessageBox.mockClear();
    await handlers.get(IPC.configExport)!(ours, JSON.stringify({ version: 1, servers: [] }));
    expect(dialogs.showMessageBox).not.toHaveBeenCalled();
    await expect(handlers.get(IPC.configExport)!(ours, "[]")).rejects.toThrow("rejected");
  });

  it("config import adds each server carried in the file, then imports the document without the secrets", async () => {
    const { dispatch } = setup();
    dialogs.showOpenDialog.mockResolvedValue({ canceled: false, filePaths: ["/home/u/cfg.json"] });
    fsMock.readFile.mockResolvedValue(JSON.stringify({ version: 1, servers: [{ id: "s1", url: "https://a.example", username: "alice", name: "A" }], secrets: { "server:s1:password": "pw" } }));
    dialogs.showMessageBox.mockResolvedValue({ response: 0 });
    expect(await handlers.get(IPC.configImport)!(ours)).toBe(true);
    expect(dispatch.mock.calls.map((c) => c[0])).toEqual([
      { type: "addServer", data: { url: "https://a.example", username: "alice", password: "pw", name: "A" } },
      { type: "importConfig", data: { document: JSON.stringify({ version: 1, servers: [{ id: "s1", url: "https://a.example", username: "alice", name: "A" }] }) } },
    ]);
    dialogs.showMessageBox.mockResolvedValue({ response: 1 });
    expect(await handlers.get(IPC.configImport)!(ours)).toBe(false);
    fsMock.readFile.mockResolvedValue("garbage");
    expect(await handlers.get(IPC.configImport)!(ours)).toBe(false);
    expect(dispatch).toHaveBeenCalledTimes(2);
  });
  it("diagnostics: refuses strangers, previews the newest lines, and writes only where main decides", async () => {
    const { query } = setup({ query: async () => ({ type: "text", data: "core says hi" }) as QueryResult });
    await expect(handlers.get(IPC.diagnosticsSummary)!(stranger)).rejects.toThrow("rejected");
    const summary = (await handlers.get(IPC.diagnosticsSummary)!(ours)) as { head: string; tail: string; logLines: number };
    expect(query).toHaveBeenCalledWith({ type: "diagnostics" });
    expect(summary.head).toContain("core says hi");
    expect(summary.tail.split("\n")).toHaveLength(400);
    expect(summary.tail.endsWith("line 449")).toBe(true);
    expect(summary.logLines).toBe(450);

    dialogs.showSaveDialog.mockResolvedValueOnce({ canceled: true });
    expect(await handlers.get(IPC.diagnosticsSave)!(ours)).toBeUndefined();
    expect(fsMock.writeFile).not.toHaveBeenCalled();
    dialogs.showSaveDialog.mockResolvedValueOnce({ canceled: false, filePath: "/home/me/d.txt" });
    expect(await handlers.get(IPC.diagnosticsSave)!(ours)).toBe("/home/me/d.txt");
    expect(fsMock.writeFile).toHaveBeenLastCalledWith("/home/me/d.txt", expect.stringContaining("line 449"), "utf8");

    await handlers.get(IPC.diagnosticsCopy)!(ours);
    expect(clipboardMock.writeText).toHaveBeenLastCalledWith(expect.stringContaining("core says hi"));

    // Show in folder keeps the newest few bundles.
    fsMock.readdir.mockResolvedValueOnce(["a.txt", ...Array.from({ length: 7 }, (_, i) => `hocket-diagnostics-2026101${i}-000000.txt`)]);
    const path = await handlers.get(IPC.diagnosticsReveal)!(ours);
    expect(path).toBe("/data/logs/diagnostics/hocket-diagnostics-20261020-030405.txt");
    expect(shellMock.showItemInFolder).toHaveBeenCalledWith(path);
    expect(fsMock.rm.mock.calls.map((c) => c[0])).toEqual(["/data/logs/diagnostics/hocket-diagnostics-20261010-000000.txt", "/data/logs/diagnostics/hocket-diagnostics-20261011-000000.txt"]);
  });

  it("diagnostics: other devices are asked for only when the page says so, once per collection", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    let n = 0;
    const { query } = setup({
      query: async (q) => ({ type: "text", data: q.type === "roomDiagnostics" ? `Other devices: phone says hi ${++n}` : "core says hi" }) as QueryResult,
    });
    const roomQueries = () => query.mock.calls.filter((c) => (c[0] as { type: string }).type === "roomDiagnostics").length;
    await handlers.get(IPC.diagnosticsCopy)!(ours);
    expect(roomQueries()).toBe(0);
    expect(clipboardMock.writeText).toHaveBeenLastCalledWith(expect.not.stringContaining("phone says hi"));
    await handlers.get(IPC.diagnosticsCopy)!(ours, { allDevices: "yes" });
    expect(roomQueries()).toBe(0);
    await expect(handlers.get(IPC.diagnosticsCollect)!(stranger)).rejects.toThrow("rejected");
    // Two collects at once share one query; the copies after reuse its answer.
    await Promise.all([handlers.get(IPC.diagnosticsCollect)!(ours), handlers.get(IPC.diagnosticsCollect)!(ours)]);
    expect(roomQueries()).toBe(1);
    await handlers.get(IPC.diagnosticsCopy)!(ours, { allDevices: true });
    await handlers.get(IPC.diagnosticsCopy)!(ours, { allDevices: true });
    expect(roomQueries()).toBe(1);
    expect(clipboardMock.writeText).toHaveBeenLastCalledWith(expect.stringContaining("phone says hi 1"));
    // Asking again at once reuses it (the room would turn the request away); later, it asks again.
    await handlers.get(IPC.diagnosticsCollect)!(ours);
    expect(roomQueries()).toBe(1);
    vi.setSystemTime(Date.now() + 16_000);
    await handlers.get(IPC.diagnosticsCollect)!(ours);
    vi.useRealTimers();
    expect(roomQueries()).toBe(2);
    // Refresh asks at once.
    await handlers.get(IPC.diagnosticsCollect)!(ours, { force: true });
    expect(roomQueries()).toBe(3);
    await handlers.get(IPC.diagnosticsCopy)!(ours, { allDevices: true });
    expect(clipboardMock.writeText).toHaveBeenLastCalledWith(expect.stringContaining("phone says hi 3"));
  });

  it("diagnostics: a collection the room turns away keeps the one before", async () => {
    let n = 0;
    let declined = false;
    const ctx = setup({
      query: async (q) => {
        if (q.type !== "roomDiagnostics") return { type: "text", data: "core says hi" } as QueryResult;
        n++;
        const status = { requestId: `r${n}`, done: true, elapsedMs: 0, roomAnswered: true, roomDeclined: declined ? "asked again too soon; try in a few seconds" : undefined, coordinatorLog: false, devices: [] };
        ctx.emit({ type: "roomDiagnosticsChanged", data: { status } } as Event);
        return { type: "text", data: declined ? "Other devices: (declined: asked again too soon)" : "Other devices: phone says hi" } as QueryResult;
      },
    });
    await handlers.get(IPC.diagnosticsCollect)!(ours);
    declined = true;
    await handlers.get(IPC.diagnosticsCollect)!(ours, { force: true });
    expect(n).toBe(2);
    await handlers.get(IPC.diagnosticsCopy)!(ours, { allDevices: true });
    expect(clipboardMock.writeText).toHaveBeenLastCalledWith(expect.stringContaining("phone says hi"));
  });

  it("diagnostics: a bundle with the other devices and no collection yet runs one", async () => {
    const { query } = setup({
      query: async (q) => ({ type: "text", data: q.type === "roomDiagnostics" ? "Other devices: phone says hi" : "core says hi" }) as QueryResult,
    });
    await handlers.get(IPC.diagnosticsCopy)!(ours, { allDevices: true });
    expect(query).toHaveBeenCalledWith({ type: "roomDiagnostics" });
    expect(clipboardMock.writeText).toHaveBeenLastCalledWith(expect.stringContaining("phone says hi"));
  });

  it("diagnostics: another device's request is answered with this device's bundle", async () => {
    const { dispatch, emit } = setup({ query: async () => ({ type: "text", data: "core says hi" }) as QueryResult });
    emit({ type: "diagnosticsRequested", data: { request_id: "r1", from_device: "Pixel" } });
    await vi.waitFor(() => expect(dispatch).toHaveBeenCalled());
    const cmd = dispatch.mock.calls[0]![0] as Extract<Command, { type: "answerDiagnosticsRequest" }>;
    expect(cmd.type).toBe("answerDiagnosticsRequest");
    expect(cmd.data.request_id).toBe("r1");
    expect(cmd.data.text).toContain("core says hi");
    expect(cmd.data.text).toContain("line 449");
  });

  it("diagnostics: a core that fails to answer still yields a bundle", async () => {
    setup({ query: async () => { throw new Error("gone"); } });
    const summary = (await handlers.get(IPC.diagnosticsSummary)!(ours)) as { head: string };
    expect(summary.head).toContain("the core didn't answer");
  });
});
