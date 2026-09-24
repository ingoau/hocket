import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Command, Query, QueryResult } from "@core/api";
import { IPC } from "@shared/bridge-types";

type Listener = (e: { sender: { id: number } }, ...args: unknown[]) => unknown;
const handlers = new Map<string, Listener>();
const listeners = new Map<string, Listener>();
const dialogs = { showSaveDialog: vi.fn(), showOpenDialog: vi.fn(), showMessageBox: vi.fn() };
const fsMock = { writeFile: vi.fn<(...args: unknown[]) => Promise<void>>(async () => undefined), readFile: vi.fn<(...args: unknown[]) => Promise<string>>(async () => "") };

vi.mock("electron", () => ({
  ipcMain: {
    on: (ch: string, l: Listener) => listeners.set(ch, l),
    handle: (ch: string, l: Listener) => handlers.set(ch, l),
  },
  BrowserWindow: { fromWebContents: (wc: { id: number }) => (wc.id === 1 ? { id: 1, setAlwaysOnTop: vi.fn(), setFullScreen: vi.fn() } : undefined) },
  clipboard: { writeText: vi.fn() },
  shell: { showItemInFolder: vi.fn(), openExternal: vi.fn() },
  dialog: dialogs,
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
  const artwork = { register: vi.fn(async (p: string) => (p.startsWith("/cache/") ? "a".repeat(32) : undefined)), resolve: vi.fn() };
  installIpc({
    core: { kind: "fake", dispatch, query, onEvent: () => () => undefined, shutdown: async () => undefined },
    windows: { all: () => [{ webContents: { id: 1 } }], mini: undefined, main: undefined, stateOf: () => ({}), broadcastState: vi.fn(), openMini: vi.fn(), closeMini: vi.fn(), showMain: vi.fn() } as never,
    artwork: artwork as never,
    meta: () => ({}) as never,
    onCommand,
    onVisibilityReport: vi.fn(),
    onNetworkReport: vi.fn(),
    prefs: { get: () => ({ closeToTray: true }), set: (p) => ({ closeToTray: p.closeToTray ?? true }) },
    credentials: store,
  });
  return { dispatch, query, onCommand, artwork, passwordFor };
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
});
